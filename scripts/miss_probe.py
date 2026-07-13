#!/usr/bin/env python3
"""Split scroll frame costs into band-MISS vs blit-HIT frames (wgpu compositor).

Companion to `bench_scroll.py`. Where bench_scroll reports aggregate
median/p95, this profiler attributes each frame to the scroll-compositor path
it took, so you can see *why* the tail is what it is:

  * MISS     — the viewport left the cached band → full band re-raster (GPU-fill
               bound; the expensive tail frames).
  * HIT      — viewport inside the band → blit + animated overlay (the cheap
               common case).
  * unstable — content key changed (animation/GIF/streaming) → band bypassed.

It launches lumen with `LUMEN_FRAME_LOG=2` (which emits `page-compose MISS/HIT`
lines), drives a scroll pattern via the MCP live window (the reliable path on
Wayland — see docs/tasks/linux-scroll-perf.md), then correlates each
`[frame] total Nms` with the compose state seen in that frame's log window.

Usage:
    python3 scripts/miss_probe.py [page] [step] [scrolls] [pattern]

    page     HTML/URL (default graphic_tests/1000000-final.html)
    step     CSS px per MCP scroll tick (default 60)
    scrolls  number of scroll ticks (default 250)
    pattern  down | updown | zigzag (default down)
             updown = down first half, up second half (tests reversals);
             zigzag = flip direction every 20 ticks.

A/B a compositor knob on one binary, e.g. directional band-bias:
    python3 scripts/miss_probe.py samples/bench-static-scroll.html 120 250
    LUMEN_NO_BAND_BIAS=1 python3 scripts/miss_probe.py samples/bench-static-scroll.html 120 250
"""
import os, re, sys, time, subprocess, statistics
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from bench_scroll import kwin_keep_above, mcp_rpc_factory  # noqa: E402

PAGE = sys.argv[1] if len(sys.argv) > 1 else "graphic_tests/1000000-final.html"
STEP = float(sys.argv[2]) if len(sys.argv) > 2 else 60.0
NSCROLL = int(sys.argv[3]) if len(sys.argv) > 3 else 250
PATTERN = sys.argv[4] if len(sys.argv) > 4 else "down"
BIN = "target/release/lumen"
PORT = 7957
TOTAL_RE = re.compile(r"\[frame\] total\s+([\d.]+)ms")


def main() -> int:
    env = os.environ.copy()
    env["LUMEN_BACKEND"] = "wgpu"
    env["LUMEN_FRAME_LOG"] = "2"
    env["LUMEN_PRESENT"] = "immediate"
    errpath = "/tmp/lumen-miss-probe.log"
    errfh = open(errpath, "w")
    proc = subprocess.Popen([BIN, "--mcp-live-port", str(PORT), PAGE],
                            env=env, stdout=subprocess.DEVNULL, stderr=errfh, text=True)
    try:
        dl = time.monotonic() + 30
        rpc = None
        while time.monotonic() < dl:
            try:
                rpc = mcp_rpc_factory(PORT)
                break
            except OSError:
                time.sleep(0.5)
        if rpc is None:
            print("MCP never came up")
            return 1
        time.sleep(2)
        kwin_keep_above("/tmp")
        time.sleep(1)
        kwin_keep_above("/tmp")
        time.sleep(1)
        skip = os.path.getsize(errpath)

        def frames_seen():
            with open(errpath) as fh:
                fh.seek(skip)
                return len(TOTAL_RE.findall(fh.read()))

        for i in range(NSCROLL):
            fb = frames_seen()
            if PATTERN == "updown":
                dir_ = 1.0 if i < NSCROLL // 2 else -1.0
            elif PATTERN == "zigzag":
                dir_ = 1.0 if (i // 20) % 2 == 0 else -1.0
            else:
                dir_ = 1.0
            rpc("tools/call", {"name": "scroll", "arguments": {
                "target": {"selector": "body"}, "delta": {"x": 0, "y": STEP * dir_}}})
            wd = time.monotonic() + 5
            while frames_seen() <= fb and time.monotonic() < wd:
                time.sleep(0.005)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
        errfh.close()

    # Correlate: walk lines, remember last compose state, assign to next total.
    miss, hit, unstable, unknown = [], [], [], []
    state = None
    with open(errpath) as fh:
        fh.seek(skip)
        for line in fh:
            if "page-compose MISS" in line:
                state = "miss"
            elif "page-compose HIT" in line:
                state = "hit"
            elif "page-compose unstable" in line:
                state = "unstable"
            m = TOTAL_RE.search(line)
            if m:
                v = float(m.group(1))
                (miss if state == "miss" else hit if state == "hit"
                 else unstable if state == "unstable" else unknown).append(v)
                state = None

    def stats(name, xs):
        if not xs:
            print(f"{name:10s} n=0")
            return
        xs2 = sorted(xs)
        p95 = xs2[min(len(xs2) - 1, int(0.95 * len(xs2)))]
        print(f"{name:10s} n={len(xs):4d}  median={statistics.median(xs):6.2f}ms  "
              f"mean={statistics.mean(xs):6.2f}ms  p95={p95:6.2f}ms  "
              f"max={max(xs):7.2f}ms  sum={sum(xs):8.1f}ms")

    print(f"\n=== {PAGE} step={STEP} scrolls={NSCROLL} pattern={PATTERN} ===")
    stats("MISS", miss)
    stats("HIT", hit)
    stats("unstable", unstable)
    stats("unknown", unknown)
    tot = miss + hit + unstable + unknown
    if tot:
        print(f"MISS share of frames: {len(miss)}/{len(tot)} = {100 * len(miss) / len(tot):.1f}%")
        print(f"MISS share of time:   {sum(miss):.0f}/{sum(tot):.0f}ms = {100 * sum(miss) / sum(tot):.1f}%")
    return 0


if __name__ == "__main__":
    sys.exit(main())
