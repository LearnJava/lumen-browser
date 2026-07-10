# A/B pixel diff for the page scroll compositor (p1-exp-wgpu-only, §2).
#
#   python scripts/exp/compose_ab_diff.py <page.html> [scroll_css_px]
#
# Runs the same binary twice — with the compositor and with
# LUMEN_NO_SCROLL_COMPOSITOR=1 — drives the window to the same scroll
# position over MCP-live, captures both via PrintWindow (capture_pid.ps1)
# and reports the pixel difference.
#
# Expected difference class (document, don't panic): elements with filters
# (blur) that CROSS the viewport edge. The monolith rasterizes their layer
# clipped at the surface edge, so the blur halo misses the off-screen part;
# the band includes ±margin of real content, so its blur is *more* complete.
# Differences confined to filtered elements at the top/bottom edges are the
# compositor being more correct, not less.
#
# stderr is drained on a thread — 4 KB pipe buffer deadlocks the browser
# otherwise (EXPERIMENT.md p.15 lesson).
import json
import os
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

PORT = 9251


def drain(pipe, sink):
    while True:
        data = pipe.read(65536)
        if not data:
            break
        sink.append(data)


def recv_line(sock):
    buf = b""
    while b"\n" not in buf:
        chunk = sock.recv(65536)
        if not chunk:
            raise ConnectionResetError("peer closed")
        buf += chunk
    return buf


def send(sock, msg):
    sock.sendall((json.dumps(msg) + "\n").encode())


def run_one(page, scroll_px, disable_compositor, out_png):
    env = dict(os.environ)
    env["LUMEN_WINDOW"] = "1024x720"
    env.pop("LUMEN_BENCH", None)
    env.pop("LUMEN_FRAME_LOG", None)
    if disable_compositor:
        env["LUMEN_NO_SCROLL_COMPOSITOR"] = "1"
    else:
        env.pop("LUMEN_NO_SCROLL_COMPOSITOR", None)
    # CreateProcess needs a resolvable executable path — forward-slash
    # relative paths fail with WinError 2.
    exe = str(Path("target/dev-release/lumen.exe").resolve())
    proc = subprocess.Popen(
        [exe, "--mcp-live-port", str(PORT), page],
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, env=env,
    )
    chunks = []
    threading.Thread(target=drain, args=(proc.stderr, chunks), daemon=True).start()
    try:
        time.sleep(8)  # load + fonts + backend probe
        s = socket.socket()
        s.settimeout(30)
        s.connect(("127.0.0.1", PORT))
        nid = 1
        send(s, {"jsonrpc": "2.0", "id": nid, "method": "initialize",
                 "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                            "clientInfo": {"name": "abdiff", "version": "1.0"}}})
        recv_line(s)
        # 3×1px to let the content-stability rule legalize the band, then big
        # steps past the band margin so at least one band re-render happens.
        deltas = [1, 1, 1] + [120] * (scroll_px // 120)
        for d in deltas:
            nid += 1
            send(s, {"jsonrpc": "2.0", "id": nid, "method": "tools/call",
                     "params": {"name": "scroll",
                                "arguments": {"target": {"type": "viewport"},
                                              "delta": {"x": 0, "y": d}}}})
            recv_line(s)
            time.sleep(0.03)
        s.close()
        time.sleep(1.0)  # settle
        r = subprocess.run(
            ["powershell", "-ExecutionPolicy", "Bypass", "-File",
             "scripts/exp/capture_pid.ps1", "-TargetPid", str(proc.pid),
             "-Out", out_png],
            capture_output=True, text=True, timeout=60,
        )
        print(r.stdout.strip() or r.stderr.strip())
    finally:
        proc.kill()


def main():
    page = sys.argv[1] if len(sys.argv) > 1 else "samples/bench-static-scroll.html"
    scroll_px = int(sys.argv[2]) if len(sys.argv) > 2 else 3600
    Path(".tmp").mkdir(exist_ok=True)
    a, b = ".tmp/ab_compose_on.png", ".tmp/ab_compose_off.png"
    run_one(page, scroll_px, False, a)
    time.sleep(2)
    run_one(page, scroll_px, True, b)

    from PIL import Image, ImageChops  # noqa: PLC0415 — optional dep, fail late
    ia, ib = Image.open(a).convert("RGB"), Image.open(b).convert("RGB")
    if ia.size != ib.size:
        sys.exit(f"size mismatch: {ia.size} vs {ib.size}")
    diff = ImageChops.difference(ia, ib)
    bbox = diff.getbbox()
    px = list(diff.getdata())
    ndiff = sum(1 for p in px if p != (0, 0, 0))
    total = ia.size[0] * ia.size[1]
    print(f"[abdiff] {ndiff}/{total} px differ ({100.0 * ndiff / total:.3f}%), bbox={bbox}")
    if ndiff:
        diff.save(".tmp/ab_compose_diff.png")
        print("[abdiff] diff mask: .tmp/ab_compose_diff.png")


if __name__ == "__main__":
    main()
