#!/usr/bin/env python3
"""BUG-480 срез 6 — живая проверка событий через границу изолятов.

Запускает dev-release Lumen на странице с ``<iframe>`` и проверяет по маркерам
в stderr (тот же канал, что у остальных verify_* проб), что ``click()`` через
фасад родителя доходит до слушателей ребёнка и исполняет его собственную
семантику click:

* ребёнок вешает ``addEventListener('click')`` на кнопку и рапортует маркером;
* родитель в обработчике ``load`` фрейма вызывает
  ``contentDocument.getElementById('btn').click()``;
* доставка асинхронная (ящик событий разбирается на тике пумпы);
* слушатель ребёнка доказывает тип события (bubbles/isTrusted) и отправляет
  результат обратно родителю кросс-фреймовым ``postMessage`` (срез 4).

Запуск: ``python tests/wpt/verify_frame_click.py [--binary PATH]``
"""

from __future__ import annotations

import argparse
import http.server
import os
import re
import socket
import subprocess
import sys
import threading
import time

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
HERE = os.path.dirname(os.path.abspath(__file__))

PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfcl parent</title>
<body>parent
<iframe src="/.vfcl-child.html"></iframe>
<script>
window.addEventListener('message', function (ev) {
  console.log('PROBE parent-got ' + JSON.stringify(ev.data));
});
var frame = document.querySelector('iframe');
frame.addEventListener('load', function () {
  console.log('PROBE parent-load');
  var d = frame.contentDocument;
  var btn = d.getElementById('btn');
  console.log('PROBE parent-facade ' + (btn ? btn.tagName : 'null'));
  btn.click();
  console.log('PROBE parent-clicked');
});
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfcl child</title>
<body><button id="btn">go</button>
<script>
console.log('PROBE child-start');
document.getElementById('btn').addEventListener('click', function (ev) {
  console.log('PROBE child-clicked bubbles=' + ev.bubbles
              + ' trusted=' + ev.isTrusted
              + ' type=' + ev.type
              + ' tag=' + ev.target.tagName);
  window.parent.postMessage({ clicked: true, trusted: ev.isTrusted }, '*');
});
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        default=os.path.join(REPO, "target", "dev-release", "lumen.exe"),
    )
    parser.add_argument("--seconds", type=float, default=8.0)
    args = parser.parse_args()

    for name, body in [(".vfcl-parent.html", PARENT_PAGE), (".vfcl-child.html", CHILD_PAGE)]:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port, server = _serve()
    log_path = os.path.join(REPO, ".tmp", "vfcl-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    print(f"http://127.0.0.1:{port}/.vfcl-parent.html -> {log_path}")
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{port}/.vfcl-parent.html"],
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

    ok = True

    def expect(substr: str) -> None:
        nonlocal ok
        hit = any(substr in m for m in markers)
        status = "ok" if hit else "MISSING"
        if not hit:
            ok = False
        print(f"[{status}] {substr}")

    def idx_of(substr: str) -> int:
        return next((i for i, m in enumerate(markers) if substr in m), -1)

    expect("child-start")
    expect("parent-load")
    expect("parent-facade BUTTON")
    # Фасад нашёл кнопку; сам вызов click() синхронно только ставит конверт.
    expect("parent-clicked")
    # Ребёнок получил событие и видит семантику синтетического click:
    # пузырьковый, недоверенный, target — его собственная кнопка.
    expect("child-clicked bubbles=true trusted=false type=click tag=BUTTON")
    # Обратный канал: сообщение из слушателя клика дошло до родителя.
    expect('parent-got {"clicked":true,"trusted":false}')
    # Порядок: load → фасад → постановка → доставка в ребёнке → ответ родителю.
    order = [("parent-load", idx_of("parent-load")),
             ("parent-clicked", idx_of("parent-clicked")),
             ("child-clicked", idx_of("child-clicked")),
             ("parent-got", idx_of("parent-got"))]
    if all(a[1] < b[1] for a, b in zip(order, order[1:])) and order[-1][1] >= 0:
        print("[ok] order parent-load -> parent-clicked -> child-clicked -> parent-got")
    else:
        print(f"[MISSING] order: {order}")
        ok = False

    print("markers:", *markers, sep="\n  ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
