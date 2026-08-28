#!/usr/bin/env python3
"""BUG-480 срез 8 — живая проверка исполнения <script>, вставленных из родителя.

Запускает dev-release Lumen на странице с ``<iframe>`` без собственных скриптов
и проверяет по маркерам в stderr (родительский console.log; консоль контекста
фрейма на stderr не выводится, поэтому всё из ребёнка идёт через кросс-фреймовый
postMessage), что ``contentDocument.createElement('script')`` + ``appendChild``
исполняются ребёнком на его тике штатной семантикой:

* инлайн-классика с честным ``document.currentScript``;
* data-блок (``type="application/json"``) НЕ исполняется.

Внешний ``src`` идёт через штатный `_lumen_script_prepare` и проверяется вместе
с загрузкой подресурсов фреймов (будущий срез очереди BUG-480).

Запуск: ``python tests/wpt/verify_frame_run_script.py [--binary PATH]``
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

PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfrs parent</title>
<body>parent
<iframe src="/.vfrs-child.html"></iframe>
<script>
console.log('PROBE parent-start');
window.addEventListener('message', function (ev) {
  console.log('PROBE parent-got ' + JSON.stringify(ev.data));
});
var frame = document.querySelector('iframe');
frame.addEventListener('load', function () {
  console.log('PROBE parent-load');
  var d = frame.contentDocument;
  // Инлайн-классика: исполнение доказывает и честный document.currentScript.
  var s = d.createElement('script');
  s.textContent =
    "window.parent.postMessage(" +
    "{ inlineRan: true, cs: document.currentScript && document.currentScript.tagName }, '*');";
  d.body.appendChild(s);
  // Data-блок: если type-гейт сломан, он ответит постом и провалит прогон.
  var data = d.createElement('script');
  data.setAttribute('type', 'application/json');
  data.textContent = "window.parent.postMessage({ jsonRan: true }, '*');";
  d.body.appendChild(data);
  console.log('PROBE parent-inserted');
});
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfrs child</title>
<body>child</body>
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
        (".vfrs-parent.html", PARENT_PAGE),
        (".vfrs-child.html", CHILD_PAGE),
    ]
    for name, body in files:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port, server = _serve()
    log_path = os.path.join(REPO, ".tmp", "vfrs-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    print(f"http://127.0.0.1:{port}/.vfrs-parent.html -> {log_path}")
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{port}/.vfrs-parent.html"],
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

    # Все посты родителя в порядке поступления — порядок доставки конвертов.
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

    def reject_post(pred, label: str) -> None:
        nonlocal ok
        hit = any(pred(item) for item in got)
        status = "ok (absent)" if not hit else "UNEXPECTED"
        if hit:
            ok = False
        print(f"[{status}] {label}")

    expect("parent-load")
    expect("parent-inserted")
    # Инлайн-классика: исполнена ребёнком с честным currentScript.
    expect_post(
        lambda p: p.get("inlineRan") is True and p.get("cs") == "SCRIPT",
        "inline script ran with currentScript",
    )
    # Data-блок не исполняется ни в каком виде.
    reject_post(lambda p: p.get("jsonRan") is True, "json data block")

    print("markers:", *markers, sep="\n  ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
