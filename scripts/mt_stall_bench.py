#!/usr/bin/env python3
"""ADR-016 M2.2c-0 — acceptance baseline: does a JS busy-loop freeze scroll today?

Launches `lumen.exe --mcp-live-port N samples/mt-busy-loop.html` with
`LUMEN_FRAME_LOG=1`, drives mouse-wheel scroll for a fixed wall-clock window and
timestamps every `[frame]` line **as it arrives** on the child's stderr. The
timestamping is the point: `scripts/scroll_perf.py` reports the paint-bound FPS
*ceiling* (`1000 / mean(paint-ms)`), which cannot see a stall — the paint itself
stays cheap, the frames just never get scheduled. This harness measures the
*delivered* cadence instead: the wall-clock gap between successive presents and
how far `scroll_y` actually travelled while we were commanding scroll.

Under today's single-thread engine, `samples/mt-busy-loop.html` burns 200 ms of
CPU synchronously inside every animation frame on the UI/winit thread, so input
dispatch and present are blocked: gaps balloon to ~400 ms (~2-3 fps) and scroll
lurches. That frozen number is the baseline M2.2c (engine owns `js_ctx` off the
UI thread) must beat. For the non-stalled control that proves the harness can
see a smooth run, edit `BUSY_MS` to 0 in the page and re-run: same layout/paint
cost, only the stall removed (measured ~28 fps / ~36 ms gaps, scroll tracks
fully — the delivered rate is then capped by the wheel-injection cadence).

Usage:
    python scripts/mt_stall_bench.py [--page P] [--secs S] [--delta PX]
                                     [--profile dev-release] [--build]

A drain thread reads stderr continuously — a bare `PIPE` left unread dead-locks
the child at the OS pipe buffer (~4 KB); we mirror every line to
`.tmp/mt_stall_bench_stderr.log` as well.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import socket
import statistics
import subprocess
import sys
import threading
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Windows console defaults to cp1251 — force UTF-8 so Cyrillic never crashes us.
for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, 'reconfigure'):
        _stream.reconfigure(encoding='utf-8', errors='replace')

# `[frame] total  15.01ms  (scroll_y 1200, dl 8430 cmds)` — carries scroll_y.
TOTAL_RE = re.compile(r'\[frame\] total\s+([\d.]+)ms\s+\(scroll_y ([\-\d.]+), dl (\d+) cmds\)')


def free_port() -> int:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(('127.0.0.1', 0))
    port = s.getsockname()[1]
    s.close()
    return port


class Client:
    """Line-delimited JSON-RPC client to `--mcp-live-port` (same protocol as run.py --live)."""

    def __init__(self, port: int) -> None:
        last: Exception | None = None
        for _ in range(200):
            try:
                self.sock = socket.create_connection(('127.0.0.1', port), timeout=5)
                break
            except OSError as e:
                last = e
                time.sleep(0.1)
        else:
            raise RuntimeError(f'MCP port {port} never came up: {last}')
        self.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self.sock.settimeout(60)
        self._reader = self.sock.makefile('r', encoding='utf-8', newline='\n')
        self._id = 0

    def call(self, name: str, arguments: dict) -> dict:
        self._id += 1
        req = json.dumps({'jsonrpc': '2.0', 'id': self._id,
                          'method': 'tools/call',
                          'params': {'name': name, 'arguments': arguments}})
        self.sock.sendall((req + '\n').encode('utf-8'))
        line = self._reader.readline()
        if not line:
            raise RuntimeError('MCP connection closed (window crashed?)')
        resp = json.loads(line)
        if resp.get('error') is not None:
            raise RuntimeError(f'{name}: {resp["error"]}')
        return resp.get('result') or {}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('--page', default='samples/mt-busy-loop.html',
                    help='page to scroll (default: the 200ms/rAF busy-loop baseline)')
    ap.add_argument('--secs', type=float, default=6.0, help='scroll measurement window, seconds')
    ap.add_argument('--delta', type=float, default=300.0, help='CSS px per wheel tick')
    ap.add_argument('--tick-hz', type=float, default=30.0, help='wheel ticks per second')
    ap.add_argument('--profile', default='dev-release')
    ap.add_argument('--build', action='store_true')
    args = ap.parse_args()

    if args.build:
        rc = subprocess.call(['cargo', 'build', '-p', 'lumen-shell', '--profile', args.profile], cwd=REPO)
        if rc != 0:
            return rc

    exe = os.path.join(REPO, 'target', args.profile, 'lumen.exe')
    if not os.path.exists(exe):
        print(f'no binary {exe} — run with --build', file=sys.stderr)
        return 1

    page = args.page
    if not page.startswith(('http://', 'https://', 'file://', 'about:')):
        page = 'file:///' + os.path.abspath(os.path.join(REPO, page)).replace('\\', '/')

    port = free_port()
    env = dict(os.environ)
    env.setdefault('LUMEN_FRAME_LOG', '1')
    tmp_dir = os.path.join(REPO, '.tmp')
    os.makedirs(tmp_dir, exist_ok=True)
    log_path = os.path.join(tmp_dir, 'mt_stall_bench_stderr.log')

    # Live-timestamped frame records: (arrival_wall_clock, scroll_y).
    frames: list[tuple[float, float]] = []
    frames_lock = threading.Lock()
    stop = threading.Event()

    proc = subprocess.Popen([exe, '--mcp-live-port', str(port), 'about:blank'],
                            cwd=REPO, env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

    def drain() -> None:
        # Read stderr line by line, stamp arrival time, mirror to the log file.
        with open(log_path, 'w', encoding='utf-8', errors='replace') as lf:
            assert proc.stderr is not None
            for raw in iter(proc.stderr.readline, b''):
                now = time.time()
                lf.write(raw.decode('utf-8', 'replace'))
                m = TOTAL_RE.search(raw.decode('utf-8', 'replace'))
                if m:
                    with frames_lock:
                        frames.append((now, float(m.group(2))))
                if stop.is_set():
                    # Keep draining so the child never blocks on a full pipe,
                    # but the measurement window is already closed.
                    pass

    drain_thr = threading.Thread(target=drain, daemon=True)
    drain_thr.start()

    try:
        c = Client(port)
        c.call('navigate', {'url': page})
        c.call('wait', {'condition': 'document_ready', 'timeout_ms': 20000})
        time.sleep(1.0)  # let the page paint / the busy-loop settle in

        # Mark the measurement window and drive wheel ticks the whole way.
        t0 = time.time()
        with frames_lock:
            frames.clear()
        tick_dt = 1.0 / args.tick_hz
        while time.time() - t0 < args.secs:
            try:
                c.call('scroll', {'target': {'css': 'body'},
                                  'delta': {'x': 0, 'y': args.delta}})
            except Exception:
                break  # window busy/closed — the gap data still tells the story
            time.sleep(tick_dt)
        t_window = time.time() - t0
    finally:
        stop.set()
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        drain_thr.join(timeout=2)

    with frames_lock:
        window = list(frames)

    if len(window) < 2:
        print(f'only {len(window)} frame(s) captured in the window — see {log_path}', file=sys.stderr)
        return 1

    times = [t for t, _ in window]
    scroll_ys = [y for _, y in window]
    gaps_ms = [(b - a) * 1000.0 for a, b in zip(times, times[1:])]
    gaps_ms.sort()

    def pct(xs: list[float], p: float) -> float:
        if not xs:
            return 0.0
        k = min(len(xs) - 1, int(round((p / 100.0) * (len(xs) - 1))))
        return xs[k]

    delivered_fps = len(window) / t_window if t_window > 0 else 0.0
    # Delivered cadence is capped by the on-demand scroll injection rate (one
    # redraw per wheel tick), so "jank" is a gap worse than 2× the tick period —
    # a real stall, not just the injection cadence.
    jank_ms = 2000.0 / args.tick_hz
    slow = sum(1 for g in gaps_ms if g > jank_ms)

    print(f'\npage: {page}')
    print(f'scroll window: {t_window:.1f}s '
          f'({args.tick_hz:.0f} ticks/s × {args.delta:.0f}px)')
    print(f'frames delivered:  {len(window)}   → delivered FPS {delivered_fps:5.1f}')
    print(f'inter-frame gap:   p50 {statistics.median(gaps_ms):6.1f}ms   '
          f'p95 {pct(gaps_ms, 95):6.1f}ms   max {max(gaps_ms):6.1f}ms')
    print(f'gaps > {jank_ms:.0f}ms (stall): {slow}/{len(gaps_ms)}')
    print(f'scroll_y travelled: {min(scroll_ys):.0f} → {max(scroll_ys):.0f} '
          f'(range {max(scroll_ys) - min(scroll_ys):.0f}px)')
    print(f'\nfull stderr log: {log_path}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
