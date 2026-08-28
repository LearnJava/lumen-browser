#!/usr/bin/env python3
"""BUG-480 срез 12 — живая проверка layout под-документа `<iframe>`.

До этого среза getBoundingClientRect()/офсеты внутри фрейма отвечали
"честными нулями" — контентная геометрия ребёнка нигде не считалась
(frame_bridge.rs: «layout содержимого фрейма — отдельный срез»). Срез 12
считает cascade + layout ребёнка на UA-дефолтном вьюпорте 300×150 CSS px
(HTML LS §4.8.5) сразу после исполнения его скриптов и до его
DOMContentLoaded/load, и пушит результат в JS-контекст ребёнка.

Проверяет (тем же кросс-фреймовым postMessage-механизмом, что и
verify_frame_run_script.py — script, вставленный родителем в
contentDocument, исполняется в изоляте ребёнка):

* элемент ребёнка с явным `width`/`height` отдаёт реальный
  getBoundingClientRect() вместо нулей;
* offsetWidth/offsetHeight ребёнка совпадают с getBoundingClientRect();
* элемент, чья ширина зависит от вьюпорта (`width: 100%`), резолвится
  против UA-дефолтного вьюпорта 300×150, а не против 0×0.

Запуск: ``python tests/wpt/verify_frame_layout.py [--binary PATH]``
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

PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfly parent</title>
<body>parent
<iframe src="/.vfly-child.html"></iframe>
<script>
console.log('PROBE parent-start');
window.addEventListener('message', function (ev) {
  console.log('PROBE parent-got ' + JSON.stringify(ev.data));
});
var frame = document.querySelector('iframe');
frame.addEventListener('load', function () {
  console.log('PROBE parent-load');
  var d = frame.contentDocument;
  var s = d.createElement('script');
  s.textContent =
    "var box = document.getElementById('box');" +
    "var r = box.getBoundingClientRect();" +
    "var full = document.getElementById('full');" +
    "var fr = full.getBoundingClientRect();" +
    "window.parent.postMessage({" +
    "  boxW: r.width, boxH: r.height," +
    "  offsetW: box.offsetWidth, offsetH: box.offsetHeight," +
    "  fullW: fr.width" +
    "}, '*');";
  d.body.appendChild(s);
  console.log('PROBE parent-inserted');
});
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfly child</title>
<body>
<div id="box" style="width:120px;height:40px;">box</div>
<div id="full" style="width:100%;">full</div>
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        default=os.path.join(REPO, "target", "dev-release", "lumen.exe"),
    )
    parser.add_argument("--seconds", type=float, default=8.0)
    args = parser.parse_args()

    files = [
        (".vfly-parent.html", PARENT_PAGE),
        (".vfly-child.html", CHILD_PAGE),
    ]
    for name, body in files:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port, server = _serve()
    log_path = os.path.join(REPO, ".tmp", "vfly-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    print(f"http://127.0.0.1:{port}/.vfly-parent.html -> {log_path}")
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{port}/.vfly-parent.html"],
            stdout=subprocess.DEVNULL, stderr=log, text=True, cwd=HERE,
        )
        try:
            time.sleep(args.seconds)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
            server.shutdown()

    with open(log_path, encoding="utf-8", errors="replace") as handle:
        markers = re.findall(r"PROBE ([^\n\r]+)", handle.read())

    got: list[dict] = []
    for marker in markers:
        if marker.startswith("parent-got "):
            try:
                got.append(json.loads(marker[len("parent-got "):]))
            except json.JSONDecodeError:
                pass

    ok = True

    def expect(substr: str) -> None:
        nonlocal ok
        hit = any(substr in m for m in markers)
        status = "ok" if hit else "MISSING"
        if not hit:
            ok = False
        print(f"[{status}] {substr}")

    def expect_post(pred, label: str) -> None:
        nonlocal ok
        hit = any(pred(item) for item in got)
        status = "ok" if hit else "MISSING"
        if not hit:
            ok = False
        print(f"[{status}] {label}")

    expect("parent-load")
    expect("parent-inserted")
    # Explicit width/height must resolve to real numbers, not honest zeros.
    expect_post(
        lambda p: abs(p.get("boxW", 0) - 120.0) < 0.5 and abs(p.get("boxH", 0) - 40.0) < 0.5,
        "explicit width/height box reports real getBoundingClientRect",
    )
    # offsetWidth/offsetHeight must agree with getBoundingClientRect.
    expect_post(
        lambda p: abs(p.get("offsetW", 0) - p.get("boxW", -1)) < 0.5
        and abs(p.get("offsetH", 0) - p.get("boxH", -1)) < 0.5,
        "offsetWidth/offsetHeight match getBoundingClientRect",
    )
    # width:100% must resolve against the 300px UA-default frame viewport
    # (284 = 300 - 2*8, the UA-default <body> margin — not 0, and not 100%
    # of an unconstrained/0-width container).
    expect_post(
        lambda p: abs(p.get("fullW", 0) - 284.0) < 0.5,
        "width:100% resolves against the 300px UA-default frame viewport",
    )

    print("markers:", *markers, sep="\n  ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
