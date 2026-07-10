# Chromium reference-browser baseline (p1-exp-wgpu-only tooling).
#
#   python scripts/exp/chromium_baseline.py --page graphic_tests/1000000-final.html \
#       [--browser edge|brave] [--frames 600] [--warmup 30] [--novsync] \
#       [--viewport 1024x684] [--hold-seconds 0]
#
# Mirrors the Lumen warm-frame bench (LUMEN_BENCH=scroll:N:W, bench_frames.rs)
# in a Chromium browser over CDP, so the EXPERIMENT.md §4 score table compares
# the same page, the same viewport and the same scroll pattern:
#   - scrollBy(0, 1) once per rAF, bouncing at both ends (1 CSS px per frame);
#   - W warmup frames measured and discarded, N frames kept;
#   - report = median / p95 / max / min / mean of rAF deltas;
#   - self-validation: max_scroll == 0 -> INVALID (same rule as bench_frames.rs).
#
# Also reports time-to-first-frame: process launch (wall clock, taken in this
# script right before spawning) -> first-paint / first-contentful-paint from the
# Paint Timing API (performance.timeOrigin is wall-clock based, so the two
# clocks subtract cleanly to a few ms accuracy).
#
# --novsync relaunches with --disable-gpu-vsync --disable-frame-rate-limit —
# the analogue of LUMEN_PRESENT=immediate. CAVEAT (record it next to any
# number): Chromium's pipeline is multi-threaded; an rAF delta is main-thread
# frame pacing, while raster/composite run concurrently on other threads and
# the GPU process. Under novsync the rAF rate can exceed what the compositor
# actually presents. Lumen's bench measures the full single-threaded frame.
# The honest cross-browser comparables are (a) the vsync-mode "frames > 16.7ms"
# count (jank), and (b) process-tree CPU%/GPU%/RAM under the same vsync scroll
# (chromium_stats.ps1 with --hold-seconds).
#
# --hold-seconds S: after the bench, keep scrolling under vsync for S seconds
# and print the root PID so chromium_stats.ps1 can sample the process tree.
import argparse
import json
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

import websockets.sync.client

BROWSERS = {
    "edge": r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    "brave": r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
}
# 9222/9333 fall into Windows excluded port ranges on this machine
# (netsh interface ipv4 show excludedportrange) — bind fails with WSAEACCES.
PORT = 9250

BENCH_JS = r"""
(async () => {
  const N = %(frames)d, W = %(warmup)d, STEP = %(step)d;
  const se = document.scrollingElement || document.documentElement;
  const maxScroll = () => se.scrollHeight - window.innerHeight;
  let dir = 1, warm = W, left = N;
  const deltas = [];
  let moved = 0;
  let prev = await new Promise(r => requestAnimationFrame(r));
  while (left > 0) {
    const before = se.scrollTop;
    if (dir > 0 && before + STEP >= maxScroll()) dir = -1;
    else if (dir < 0 && before - STEP <= 0) dir = 1;
    se.scrollTop = before + dir * STEP;
    const t = await new Promise(r => requestAnimationFrame(r));
    if (se.scrollTop !== before) moved++;
    const d = t - prev; prev = t;
    if (warm > 0) { warm--; } else { deltas.push(d); left--; }
  }
  const paints = {};
  for (const e of performance.getEntriesByType('paint')) paints[e.name] = e.startTime;
  return JSON.stringify({
    deltas, moved,
    scrollHeight: se.scrollHeight, innerH: window.innerHeight,
    innerW: window.innerWidth, maxScroll: maxScroll(),
    dpr: window.devicePixelRatio, timeOrigin: performance.timeOrigin,
    paints, ua: navigator.userAgent,
  });
})()
"""

HOLD_JS = r"""
(async () => {
  const END = performance.now() + %(ms)d;
  const se = document.scrollingElement || document.documentElement;
  const maxScroll = () => se.scrollHeight - window.innerHeight;
  let dir = 1;
  while (performance.now() < END) {
    const before = se.scrollTop;
    if (dir > 0 && before + 1 >= maxScroll()) dir = -1;
    else if (dir < 0 && before - 1 <= 0) dir = 1;
    se.scrollTop = before + dir;
    await new Promise(r => requestAnimationFrame(r));
  }
  return "hold done";
})()
"""


class Cdp:
    """Minimal synchronous CDP client over one page-target websocket."""

    def __init__(self, ws_url):
        # Page targets never fragment sanely; lift the message size limit.
        self.ws = websockets.sync.client.connect(ws_url, max_size=64 * 1024 * 1024)
        self.next_id = 1

    def call(self, method, params=None, timeout=300):
        msg_id = self.next_id
        self.next_id += 1
        self.ws.send(json.dumps({"id": msg_id, "method": method, "params": params or {}}))
        deadline = time.monotonic() + timeout
        while True:
            left = deadline - time.monotonic()
            if left <= 0:
                raise TimeoutError(f"CDP {method} timed out after {timeout}s")
            raw = json.loads(self.ws.recv(timeout=left))
            if raw.get("id") == msg_id:
                if "error" in raw:
                    raise RuntimeError(f"CDP {method}: {raw['error']}")
                return raw.get("result", {})
            # events and stale replies are dropped — this harness polls, it
            # does not subscribe

    def eval_js(self, expr, timeout=300):
        res = self.call(
            "Runtime.evaluate",
            {"expression": expr, "awaitPromise": True, "returnByValue": True},
            timeout=timeout,
        )
        exc = res.get("exceptionDetails")
        if exc:
            raise RuntimeError(f"JS exception: {json.dumps(exc)[:500]}")
        return res["result"].get("value")

    def close(self):
        self.ws.close()


def wait_for_page_target(port, expect_url, deadline_s=30):
    """Polls /json/list until a page target for `expect_url` appears.

    Polls both loopbacks: when the IPv4 port is lingering in TIME_WAIT (or held
    by a zombie browser), Chromium silently binds [::1] only. Matching the URL
    guards against connecting to a stale target of a half-dead instance —
    that exact failure hung a run for 300 s (CDP call to a dead renderer).
    """
    deadline = time.monotonic() + deadline_s
    last_err = None
    while time.monotonic() < deadline:
        for host in ("127.0.0.1", "[::1]"):
            try:
                with urllib.request.urlopen(f"http://{host}:{port}/json/list", timeout=2) as r:
                    targets = json.loads(r.read())
            except Exception as e:  # noqa: BLE001 — retry until deadline
                last_err = e
                continue
            for t in targets:
                if (t.get("type") == "page" and t.get("webSocketDebuggerUrl")
                        and t.get("url", "").lower() == expect_url.lower()):
                    return t["webSocketDebuggerUrl"]
        time.sleep(0.2)
    raise RuntimeError(f"no CDP page target for {expect_url} on port {port}: {last_err}")


def stats(samples):
    s = sorted(samples)
    n = len(s)
    pick = lambda q: s[round((n - 1) * q)]  # noqa: E731
    return {
        "n": n,
        "median": pick(0.50),
        "p95": pick(0.95),
        "max": s[-1],
        "min": s[0],
        "mean": sum(s) / n,
        "over_16_7": sum(1 for x in s if x > 16.7),
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--page", required=True)
    ap.add_argument("--browser", choices=BROWSERS, default="edge")
    ap.add_argument("--frames", type=int, default=600)
    ap.add_argument("--warmup", type=int, default=30)
    ap.add_argument("--step", type=int, default=1, help="CSS px scrolled per frame")
    ap.add_argument("--viewport", default="1024x684", help="CSS px, WxH; Lumen 1024x720 window = 1024x684 viewport")
    ap.add_argument("--novsync", action="store_true",
                    help="--disable-gpu-vsync --disable-frame-rate-limit (analogue of LUMEN_PRESENT=immediate)")
    ap.add_argument("--hold-seconds", type=int, default=0,
                    help="after the bench: keep a vsync scroll running so chromium_stats.ps1 can sample")
    args = ap.parse_args()

    exe = BROWSERS[args.browser]
    if not Path(exe).exists():
        sys.exit(f"browser not found: {exe}")
    page = Path(args.page).resolve()
    if not page.exists():
        sys.exit(f"page not found: {page}")
    url = page.as_uri()
    vw, vh = (int(x) for x in args.viewport.split("x"))

    profile = Path(".tmp") / f"chromium-profile-{args.browser}"
    if profile.exists():
        shutil.rmtree(profile, ignore_errors=True)
    profile.mkdir(parents=True, exist_ok=True)

    flags = [
        exe,
        f"--remote-debugging-port={PORT}",
        f"--user-data-dir={profile.resolve()}",
        "--no-first-run", "--no-default-browser-check",
        "--disable-extensions", "--disable-background-networking",
        "--disable-component-update", "--disable-sync",
        # An occluded/backgrounded window stops BeginFrames -> rAF stalls and
        # the bench hangs forever (observed 2026-07-10, run 3 of 3).
        "--disable-backgrounding-occluded-windows", "--disable-renderer-backgrounding",
        "--disable-background-timer-throttling",
        f"--window-size={vw},{vh + 120}",  # outer window; exact viewport set via CDP below
    ]
    if args.novsync:
        flags += ["--disable-gpu-vsync", "--disable-frame-rate-limit"]
    flags.append(url)

    launch_epoch_ms = time.time() * 1000.0
    proc = subprocess.Popen(flags, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    print(f"[chromium] {args.browser} pid={proc.pid} novsync={args.novsync} url={url}")

    cdp = None
    try:
        ws_url = wait_for_page_target(PORT, url)
        print(f"[chromium] page target: {ws_url}", flush=True)
        cdp = Cdp(ws_url)
        cdp.call("Emulation.setDeviceMetricsOverride",
                 {"width": vw, "height": vh, "deviceScaleFactor": 0, "mobile": False})
        # Wait for load: poll readyState instead of subscribing to events.
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            if cdp.eval_js("document.readyState", timeout=30) == "complete":
                break
            time.sleep(0.1)
        else:
            sys.exit("page never reached readyState=complete")
        print("[chromium] load complete, starting bench", flush=True)
        time.sleep(1.0)  # settle: fonts, images, first frames

        # 630 vsync frames = ~11 s; 120 s means rAF has stalled — fail fast.
        raw = cdp.eval_js(BENCH_JS % {"frames": args.frames, "warmup": args.warmup,
                                      "step": args.step}, timeout=120)
        r = json.loads(raw)

        st = stats(r["deltas"])
        mode = "novsync (uncapped rAF)" if args.novsync else "vsync"
        print(f"[bench] chromium-{args.browser} scroll {mode}: n={st['n']} warmup={args.warmup} | "
              f"median {st['median']:.3f}ms p95 {st['p95']:.3f}ms max {st['max']:.3f}ms "
              f"min {st['min']:.3f}ms mean {st['mean']:.3f}ms | frames>16.7ms: {st['over_16_7']} | "
              f"moved {r['moved']}/{args.frames + args.warmup}")
        print(f"[bench] geometry: scrollHeight {r['scrollHeight']} viewport {r['innerW']}x{r['innerH']} "
              f"max_scroll {r['maxScroll']} dpr {r['dpr']}")
        if r["maxScroll"] <= 0:
            print("[bench] INVALID: max_scroll is 0 — the page cannot scroll; these deltas are "
                  "an idle rAF loop, not a repaint (same rule as bench_frames.rs).")
        if r["moved"] == 0:
            print("[bench] INVALID: scrollTop never changed.")

        fcp = r["paints"].get("first-contentful-paint")
        fp = r["paints"].get("first-paint")
        if fcp is not None:
            ttff = r["timeOrigin"] + fcp - launch_epoch_ms
            print(f"[bench] first frame: launch->FCP {ttff:.0f}ms "
                  f"(nav->first-paint {fp:.0f}ms, nav->FCP {fcp:.0f}ms)")
        else:
            print("[bench] first frame: no first-contentful-paint entry (graphics-only page? "
                  f"paints={r['paints']})")
        print(f"[bench] ua: {r['ua']}")

        if args.hold_seconds > 0:
            print(f"[hold] root pid {proc.pid}: vsync scroll for {args.hold_seconds}s — "
                  f"sample now with: powershell -ExecutionPolicy Bypass -File "
                  f"scripts/exp/chromium_stats.ps1 -RootPid {proc.pid} -Seconds {args.hold_seconds}")
            cdp.eval_js(HOLD_JS % {"ms": args.hold_seconds * 1000},
                        timeout=args.hold_seconds + 60)
    finally:
        if cdp is not None:
            try:
                cdp.close()
            except Exception:  # noqa: BLE001 — teardown must not mask the report
                pass
        proc.kill()


if __name__ == "__main__":
    main()
