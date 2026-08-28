#!/usr/bin/env python3
"""BUG-480 срез 7 — живая проверка focus()/blur()/dispatchEvent через фасад.

Запускает dev-release Lumen на странице с ``<iframe>`` и проверяет по маркерам
в stderr (тот же канал, что у остальных verify_* проб), что действия родителя
через ``contentDocument`` исполняют СОБСТВЕННУЮ семантику ребёнка:

* родитель в обработчике ``load`` фрейма вызывает ``btn.focus()`` — ребёнок
  отвечает маркером из слушателя ``focus`` (не пузырится) и рапортует свой
  ``document.activeElement``;
* ``btn.dispatchEvent(new CustomEvent('hello', {detail}))`` — ребёнок получает
  CustomEvent со сохранённым detail;
* ``btn.blur()`` — ребёнок получает и ``blur``, и всплывший ``focusout``,
  activeElement уходит; финальный ответ родителю идёт кросс-фреймовым
  ``postMessage`` (срез 4).

Запуск: ``python tests/wpt/verify_frame_actions.py [--binary PATH]``
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

PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfa parent</title>
<body>parent
<iframe src="/.vfa-child.html"></iframe>
<script>
window.addEventListener('message', function (ev) {
  console.log('PROBE parent-got ' + JSON.stringify(ev.data));
});
var frame = document.querySelector('iframe');
frame.addEventListener('load', function () {
  var d = frame.contentDocument;
  var btn = d.getElementById('btn');
  console.log('PROBE parent-facade ' + (btn ? btn.tagName : 'null'));
  btn.focus();
  btn.dispatchEvent(new CustomEvent('hello', { detail: { n: 7, tag: 'slice7' } }));
  btn.blur();
});
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfa child</title>
<body><button id="btn">go</button>
<script>
console.log('PROBE child-start');
var btn = document.getElementById('btn');
btn.addEventListener('focus', function () {
  console.log('PROBE child-focused active=' + document.activeElement.id);
});
document.addEventListener('focusin', function (ev) {
  console.log('PROBE child-focusin target=' + ev.target.id);
});
btn.addEventListener('hello', function (ev) {
  console.log('PROBE child-hello detail=' + JSON.stringify(ev.detail));
});
btn.addEventListener('blur', function () {
  console.log('PROBE child-blurred active=' + document.activeElement.tagName);
});
document.addEventListener('focusout', function (ev) {
  console.log('PROBE child-focusout target=' + ev.target.id);
  window.parent.postMessage({ done: true }, '*');
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

    for name, body in [(".vfa-parent.html", PARENT_PAGE), (".vfa-child.html", CHILD_PAGE)]:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port, server = _serve()
    log_path = os.path.join(REPO, ".tmp", "vfa-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    print(f"http://127.0.0.1:{port}/.vfa-parent.html -> {log_path}")
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{port}/.vfa-parent.html"],
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
    expect("parent-facade BUTTON")
    # focus(): слушатель на кнопке + непузырящийся focus дошёл только до цели.
    expect("child-focused active=btn")
    expect("child-focusin target=btn")
    # dispatchEvent(): CustomEvent с JSON-круготрипом detail.
    expect('child-hello detail={"n":7,"tag":"slice7"}')
    # blur(): и сам blur, и всплывший focusout; activeElement покинул кнопку.
    expect("child-blurred active=BODY")
    expect("child-focusout target=btn")
    # Обратный канал: focusout-обработчик ответил родителю postMessage.
    expect("parent-got {\"done\":true}")
    order = [("child-focused", idx_of("child-focused")),
             ("child-hello", idx_of("child-hello")),
             ("child-blurred", idx_of("child-blurred")),
             ("parent-got", idx_of("parent-got"))]
    if all(a[1] < b[1] for a, b in zip(order, order[1:])) and order[-1][1] >= 0:
        print("[ok] order focused -> hello -> blurred -> parent-got")
    else:
        print(f"[MISSING] order: {order}")
        ok = False

    print("markers:", *markers, sep="\n  ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
