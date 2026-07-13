#!/usr/bin/env python3
"""Chromium scroll-benchmark (Linux) — counterpart of bench_scroll.py for a fair
same-day comparison against Lumen.

Launches headed Chromium with an isolated profile and uncapped present
(--disable-gpu-vsync --disable-frame-rate-limit, the analogue of Lumen's
LUMEN_PRESENT=immediate), drives a requestAnimationFrame scroll loop over the
page via CDP, and reports median/p95 frame time (rAF delta) plus process-tree
CPU% and peak PSS sampled from /proc — the same summary shape bench_scroll.py
prints for Lumen.

Caveat (as documented in EXPERIMENT.md §13): Chromium composites scroll
off the main thread, so the rAF delta is a *lower bound* on its true per-frame
cost, not the exact cost. The GPU/CPU-occupancy and memory numbers have no such
caveat. Numbers are indicative, gate-worthy only alongside the caveat.

Usage:
  python3 scripts/bench_chromium.py --page graphic_tests/1000000-final.html \
      --frames 600 --step 60 --runs 2
"""
import argparse
import base64
import hashlib
import json
import os
import socket
import subprocess
import sys
import threading
import time
import urllib.request

CLK = 100  # USER_HZ


# ── minimal RFC6455 client (text frames only, no fragmentation) ──────────────
class WS:
    def __init__(self, url):
        assert url.startswith("ws://")
        hostport, _, path = url[5:].partition("/")
        host, _, port = hostport.partition(":")
        self.sock = socket.create_connection((host, int(port or 80)), timeout=30)
        key = base64.b64encode(os.urandom(16)).decode()
        req = (
            f"GET /{path} HTTP/1.1\r\nHost: {hostport}\r\n"
            "Upgrade: websocket\r\nConnection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        )
        self.sock.sendall(req.encode())
        buf = b""
        while b"\r\n\r\n" not in buf:
            buf += self.sock.recv(4096)
        accept = base64.b64encode(
            hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()
        ).decode()
        assert accept in buf.decode(errors="replace"), "WS handshake failed"
        self._rest = buf.split(b"\r\n\r\n", 1)[1]

    def _recv(self, n):
        while len(self._rest) < n:
            self._rest += self.sock.recv(65536)
        out, self._rest = self._rest[:n], self._rest[n:]
        return out

    def send(self, text):
        data = text.encode()
        hdr = bytearray([0x81])
        mask = os.urandom(4)
        ln = len(data)
        if ln < 126:
            hdr.append(0x80 | ln)
        elif ln < 65536:
            hdr.append(0x80 | 126)
            hdr += ln.to_bytes(2, "big")
        else:
            hdr.append(0x80 | 127)
            hdr += ln.to_bytes(8, "big")
        hdr += mask
        self.sock.sendall(bytes(hdr) + bytes(b ^ mask[i % 4] for i, b in enumerate(data)))

    def recv(self):
        b0, b1 = self._recv(2)
        ln = b1 & 0x7F
        if ln == 126:
            ln = int.from_bytes(self._recv(2), "big")
        elif ln == 127:
            ln = int.from_bytes(self._recv(8), "big")
        payload = self._recv(ln) if ln else b""
        return payload.decode(errors="replace")


class Cdp:
    def __init__(self, ws):
        self.ws = ws
        self.id = 0

    def call(self, method, params=None, timeout=30):
        self.id += 1
        mid = self.id
        self.ws.send(json.dumps({"id": mid, "method": method, "params": params or {}}))
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            msg = json.loads(self.ws.recv())
            if msg.get("id") == mid:
                if "error" in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get("result", {})
        raise TimeoutError(method)


# ── process-tree /proc sampler (CPU% + peak PSS), mirrors bench_scroll.py ─────
def child_pids(root):
    pids = {root}
    changed = True
    while changed:
        changed = False
        for pid in os.listdir("/proc"):
            if not pid.isdigit():
                continue
            try:
                with open(f"/proc/{pid}/stat") as fh:
                    ppid = int(fh.read().split(") ", 1)[1].split()[1])
            except (OSError, IndexError):
                continue
            if ppid in pids and int(pid) not in pids:
                pids.add(int(pid))
                changed = True
    return pids


def tree_jiffies(pids):
    total = 0
    for pid in pids:
        try:
            with open(f"/proc/{pid}/stat") as fh:
                parts = fh.read().split(") ", 1)[1].split()
            total += int(parts[11]) + int(parts[12])  # utime + stime
        except (OSError, IndexError):
            pass
    return total


def tree_pss_kib(pids):
    total = 0
    for pid in pids:
        try:
            with open(f"/proc/{pid}/smaps_rollup") as fh:
                for line in fh:
                    if line.startswith("Pss:"):
                        total += int(line.split()[1])
                        break
        except OSError:
            pass
    return total


class Sampler(threading.Thread):
    def __init__(self, root):
        super().__init__(daemon=True)
        self.root = root
        self.stop = threading.Event()
        self.peak_pss = 0
        self.cpu_pct = 0.0

    def run(self):
        pids = child_pids(self.root)
        j0 = tree_jiffies(pids)
        t0 = time.monotonic()
        while not self.stop.is_set():
            time.sleep(0.5)
            pids = child_pids(self.root)
            self.peak_pss = max(self.peak_pss, tree_pss_kib(pids))
        j1 = tree_jiffies(child_pids(self.root))
        dt = time.monotonic() - t0
        if dt > 0:
            self.cpu_pct = (j1 - j0) / CLK / dt * 100.0


SCROLL_JS = """
(() => new Promise(resolve => {
  const step = %STEP%, n = %N%, warm = %WARM%;
  const max = Math.max(0, document.documentElement.scrollHeight - window.innerHeight);
  if (max <= 0) { resolve({invalid: true, max}); return; }
  let dir = 1, i = 0, last = performance.now(), frames = [];
  function tick(now) {
    if (i > warm) frames.push(now - last);
    last = now;
    let y = window.scrollY + dir * step;
    if (y >= max) { y = max; dir = -1; }
    else if (y <= 0) { y = 0; dir = 1; }
    window.scrollTo(0, y);
    if (++i >= n + warm) { resolve({frames, max}); return; }
    requestAnimationFrame(tick);
  }
  requestAnimationFrame(tick);
}))()
"""


def percentiles(s):
    s = sorted(s)
    n = len(s)
    pick = lambda q: s[round((n - 1) * q)]
    return dict(median=round(pick(0.50), 3), p95=round(pick(0.95), 3),
                max=round(s[-1], 3), mean=round(sum(s) / n, 3))


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def one_run(args, idx):
    port = free_port()
    profile = os.path.join(args.tmpdir, f"chromium-bench-{idx}")
    page = args.page
    if not page.startswith(("http://", "https://", "file://")):
        page = "file://" + os.path.abspath(page)
    cmd = [
        args.binary, f"--remote-debugging-port={port}",
        f"--user-data-dir={profile}", "--no-first-run", "--no-default-browser-check",
        "--disable-gpu-vsync", "--disable-frame-rate-limit",
        "--disable-backgrounding-occluded-windows", "--disable-renderer-backgrounding",
        f"--window-size={args.width},{args.height}", "about:blank",
    ]
    proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    result = {"run": idx, "page": args.page}
    sampler = Sampler(proc.pid)
    try:
        # Wait for the DevTools HTTP endpoint, then the page ws target.
        ws_url = None
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and ws_url is None:
            try:
                data = json.load(urllib.request.urlopen(f"http://127.0.0.1:{port}/json", timeout=2))
                for t in data:
                    if t.get("type") == "page" and t.get("webSocketDebuggerUrl"):
                        ws_url = t["webSocketDebuggerUrl"]
                        break
            except Exception:
                time.sleep(0.4)
        if ws_url is None:
            result["error"] = "no CDP page target"
            return result
        cdp = Cdp(WS(ws_url))
        cdp.call("Page.enable")
        cdp.call("Page.navigate", {"url": page})
        # Poll readyState until complete (+ a settle for resources/paint).
        for _ in range(60):
            r = cdp.call("Runtime.evaluate",
                         {"expression": "document.readyState", "returnByValue": True})
            if r.get("result", {}).get("value") == "complete":
                break
            time.sleep(0.5)
        time.sleep(1.5)
        sampler.start()
        js = SCROLL_JS.replace("%STEP%", str(args.step)).replace(
            "%N%", str(args.frames)).replace("%WARM%", str(args.warmup))
        r = cdp.call("Runtime.evaluate",
                     {"expression": js, "awaitPromise": True, "returnByValue": True},
                     timeout=args.timeout)
        val = r.get("result", {}).get("value") or {}
        if val.get("invalid"):
            result["error"] = f"INVALID: max_scroll {val.get('max')}"
        elif val.get("frames"):
            result["frame"] = percentiles(val["frames"])
            result["rendered"] = len(val["frames"])
            result["max_scroll"] = val.get("max")
    except Exception as e:
        result["error"] = f"{type(e).__name__}: {e}"
    finally:
        sampler.stop.set()
        sampler.join(timeout=2)
        proc.terminate()
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
    result["cpu_pct"] = round(sampler.cpu_pct, 1)
    result["pss_peak_mib"] = round(sampler.peak_pss / 1024, 1)
    return result


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("--page", required=True)
    ap.add_argument("--frames", type=int, default=600)
    ap.add_argument("--warmup", type=int, default=30)
    ap.add_argument("--step", type=float, default=60)
    ap.add_argument("--runs", type=int, default=2)
    ap.add_argument("--width", type=int, default=1024)
    ap.add_argument("--height", type=int, default=720)
    ap.add_argument("--binary", default="chromium")
    ap.add_argument("--timeout", type=int, default=120)
    ap.add_argument("--tmpdir", default=os.environ.get("TMPDIR", "/tmp"))
    args = ap.parse_args()

    medians = []
    for i in range(args.runs):
        r = one_run(args, i)
        if "frame" in r:
            f = r["frame"]
            medians.append(f["median"])
            print(f"run {i}: median {f['median']}ms p95 {f['p95']}ms mean {f['mean']}ms "
                  f"max {f['max']}ms | rendered {r['rendered']} | "
                  f"cpu {r['cpu_pct']}% pss_peak {r['pss_peak_mib']}MiB", flush=True)
        else:
            print(f"run {i}: {r.get('error', 'no frames')}", flush=True)
    if medians:
        medians.sort()
        mm = medians[len(medians) // 2]
        print(f"== chromium {os.path.basename(args.page)} step={args.step}: "
              f"median-of-medians {mm}ms ({len(medians)}/{args.runs} valid runs)")
    return 0 if medians else 1


if __name__ == "__main__":
    sys.exit(main())
