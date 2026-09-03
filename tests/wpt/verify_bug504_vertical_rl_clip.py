#!/usr/bin/env python3
"""BUG-504 part 10 — live probe for
`css/css-overflow/overflow-clip-clamps-and-ignores-scroll-offsets-vertical-rl.html`.

Mirrors the WPT file's `test()` body step by step with `console.log` markers
instead of `assert_equals` (which throws on the first failure and hides every
later step). `--mcp-live-port` + a local http server, same pattern as
`verify_scrollbar_gutter_propagation.py`.

Запуск: python tests/wpt/verify_bug504_vertical_rl_clip.py --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
"""

from __future__ import annotations

import argparse
import http.server
import json
import os
import re
import socket
import subprocess
import sys
import threading
import time

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
HERE = os.path.dirname(os.path.abspath(__file__))

PAGE = """<!doctype html><meta charset="utf-8"><title>bug504 vertical-rl clip</title>
<style>
  #scroller {
    width: 100px;
    height: 100px;
    overflow: hidden;
    resize: both;
    writing-mode: vertical-rl;
    border: 1px solid black;
  }
  #contents { width: 300px; height: 300px; }
</style>
<body>
<div id="scroller">
  <div id="contents"></div>
</div>
<script>
var scroller = document.getElementById('scroller');
var steps = {};

scroller.scrollTo(-40, 50);
steps.after_scrollTo_hidden = [scroller.scrollLeft, scroller.scrollTop];

scroller.style.overflow = 'clip';
steps.after_overflow_clip = [scroller.scrollLeft, scroller.scrollTop];

scroller.scrollTo(-60, 70);
steps.after_scrollTo_on_clip = [scroller.scrollLeft, scroller.scrollTop];

scroller.scrollBy(-10, 20);
steps.after_scrollBy_on_clip = [scroller.scrollLeft, scroller.scrollTop];

scroller.scrollLeft = -25;
scroller.scrollTop = 35;
steps.after_direct_assign_on_clip = [scroller.scrollLeft, scroller.scrollTop];

console.log('PROBE ' + JSON.stringify(steps));
</script>
</body>
"""


def _free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class _Quiet(http.server.SimpleHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=HERE, **kwargs)

    def log_message(self, *args):
        pass


def _serve() -> tuple[int, http.server.ThreadingHTTPServer]:
    port = _free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Quiet)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server


def _run_page(binary: str, filename: str, body: str, seconds: float) -> list[str]:
    with open(os.path.join(HERE, filename), "w", encoding="utf-8") as handle:
        handle.write(body)
    port, server = _serve()
    log_path = os.path.join(REPO, ".tmp", f"bug504-vrl-{filename}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/{filename}"
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()), url],
            stdout=subprocess.DEVNULL, stderr=log, text=True, cwd=HERE,
        )
        try:
            time.sleep(seconds)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
            server.shutdown()
    with open(log_path, encoding="utf-8", errors="replace") as handle:
        text = handle.read()
    return re.findall(r"PROBE ([^\n\r]+)", text)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        default=os.path.join(REPO, "target", "dev-release", "lumen.exe"),
    )
    parser.add_argument("--seconds", type=float, default=3.0)
    args = parser.parse_args()

    markers = _run_page(args.binary, ".bug504-vrl.html", PAGE, args.seconds)
    data = None
    for m in markers:
        data = json.loads(m)
    print("steps:", json.dumps(data, indent=2) if data else None)

    if data is None:
        print("ИТОГ: КРАСНЫЙ (страница не отдала снимок)")
        return 1

    ok = True

    def expect(label: str, cond: bool) -> None:
        nonlocal ok
        print(f"[{'ok' if cond else 'ФЕЙЛ'}] {label}")
        if not cond:
            ok = False

    expect("hidden allows scrollTo(-40, 50)", data["after_scrollTo_hidden"] == [-40, 50])
    expect("overflow:clip clamps existing offset to 0/0", data["after_overflow_clip"] == [0, 0])
    expect("scrollTo() is a no-op under clip", data["after_scrollTo_on_clip"] == [0, 0])
    expect("scrollBy() is a no-op under clip", data["after_scrollBy_on_clip"] == [0, 0])
    expect("direct scrollLeft/scrollTop assignment is a no-op under clip", data["after_direct_assign_on_clip"] == [0, 0])

    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
