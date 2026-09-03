#!/usr/bin/env python3
"""BUG-504 — `scrollbar-gutter` on `:root` propagated to the viewport.

`css/css-overflow/scrollbar-gutter-propagation-{001,002,003,007}.html` were
`expected: FAIL` under the generic BUG-504 note (`content_width`/
`content_height` don't reserve a gutter for the *viewport*, only for a plain
scrolling element's own children). This probe mirrors each file's actual
`assert_*` calls with plain `console.log`, since none of them need
`testharness.js` beyond `assert_equals`/`assert_less_than` — reimplemented
here as thin JS helpers so the probe needs no local WPT resource server.

001/002/003 share one assertion shape (`:root { scrollbar-gutter: stable }`,
varying only `overflow`/whether content actually scrolls, which
`propagate_viewport_scrollbar_gutter` doesn't look at) — one page suffices to
stand in for all three. 007 is a distinct shape (`body { overflow: scroll }`,
`root.clientWidth`/`window.outerWidth`, no `scrollbar-gutter` on `:root`
itself contributing anything beyond the same viewport reservation) — probed
separately, kept in the report as a data point, not assumed fixed.

Запуск: python tests/wpt/verify_scrollbar_gutter_propagation.py --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
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

# Mirrors scrollbar-gutter-propagation-001.html's single `test()` body.
PAGE_001 = """<!doctype html><meta charset="utf-8"><title>sgp 001</title>
<style>
  body, html { margin: 0; padding: 0; border: none; }
  :root { scrollbar-gutter: stable; }
  #content { background: green; width: 100%; height: 100px; }
</style>
<body>
<div id="content"></div>
<script>
var root = document.documentElement, body = document.body, content = document.getElementById('content');
console.log('PROBE 001 ' + JSON.stringify({
  rootOffsetWidth: root.offsetWidth,
  innerWidth: window.innerWidth,
  bodyOffsetWidth: body.offsetWidth,
  bodyClientWidth: body.clientWidth,
  contentOffsetWidth: content.offsetWidth,
}));
</script>
</body>
"""

# Mirrors scrollbar-gutter-propagation-007.html's single `test()` body.
PAGE_007 = """<!doctype html><meta charset="utf-8"><title>sgp 007</title>
<style>
  body, html { margin: 0; padding: 0; border: none; }
  :root { scrollbar-gutter: stable; }
  body { overflow: scroll; }
  #content { background: green; width: 100%; height: 100px; }
</style>
<body>
<div id="content"></div>
<script>
var root = document.documentElement, body = document.body, content = document.getElementById('content');
console.log('PROBE 007 ' + JSON.stringify({
  rootClientWidth: root.clientWidth,
  outerWidth: window.outerWidth,
  bodyOffsetWidth: body.offsetWidth,
  bodyClientWidth: body.clientWidth,
  contentOffsetWidth: content.offsetWidth,
}));
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
    log_path = os.path.join(REPO, ".tmp", f"sgp-{filename}.log")
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

    ok = True

    def expect(label: str, cond: bool) -> None:
        nonlocal ok
        print(f"[{'ok' if cond else 'ФЕЙЛ'}] {label}")
        if not cond:
            ok = False

    markers = _run_page(args.binary, ".sgp-001.html", PAGE_001, args.seconds)
    data001 = None
    for m in markers:
        if m.startswith("001 "):
            data001 = json.loads(m[len("001 "):])
    print("001:", data001)
    if data001 is None:
        expect("001: страница отдала снимок", False)
    else:
        expect("001: viewport has gutter (root.offsetWidth < window.innerWidth)",
               data001["rootOffsetWidth"] < data001["innerWidth"])
        expect("001: body matches root", data001["bodyOffsetWidth"] == data001["rootOffsetWidth"])
        expect("001: body has no gutter (clientWidth == offsetWidth)",
               data001["bodyClientWidth"] == data001["bodyOffsetWidth"])
        expect("001: content matches body", data001["contentOffsetWidth"] == data001["bodyClientWidth"])

    markers = _run_page(args.binary, ".sgp-007.html", PAGE_007, args.seconds)
    data007 = None
    for m in markers:
        if m.startswith("007 "):
            data007 = json.loads(m[len("007 "):])
    print("007:", data007)
    if data007 is None:
        expect("007: страница отдала снимок", False)
    else:
        print(
            "  (007 — root.clientWidth vs window.outerWidth, отдельный класс "
            "«классический против overlay-скроллбара», не обязан сойтись этим срезом)"
        )
        print("  007 raw:", data007)

    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
