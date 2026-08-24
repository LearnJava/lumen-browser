#!/usr/bin/env python3
"""BUG-480 срез 4 — живая проверка кросс-фреймового ``postMessage``.

Запускает dev-release Lumen на странице с ``<iframe>`` и проверяет по маркерам
в stderr (тот же канал, что у остальных verify_* проб), что оба направления
иерархии доставляют MessageEvent в живом конвейере (shell -> pump ->
frame_bridge), а не только в изолятах юнит-тестов:

* ребёнок -> родитель сразу после старта (``window.parent.postMessage``);
* родитель -> ребёнок из обработчика ``load`` фрейма
  (``iframe.contentWindow.postMessage``);
* ответ ребёнка на полученное сообщение (полный раунд-трип).

Запуск: ``python tests/wpt/verify_frame_post_message.py [--binary PATH]``
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

PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfpm parent</title>
<body>parent
<iframe src="/.vfpm-child.html"></iframe>
<script>
console.log('PROBE parent-start');
window.addEventListener('message', function (e) {
  console.log('PROBE parent-got ' + JSON.stringify(e.data)
              + ' origin=' + e.origin + ' source=' + (e.source ? 'yes' : 'no'));
});
var frame = document.querySelector('iframe');
frame.addEventListener('load', function () {
  console.log('PROBE parent-load');
  frame.contentWindow.postMessage({ ping: 1 }, '*');
});
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfpm child</title>
<body>child
<script>
console.log('PROBE child-start ' + location.href);
// Постинг только после DOMContentLoaded: инлайн-скрипт исполняется ДО
// регистрации предков (ограничение срезов 1–3, см. BUG-480), и
// window.parent здесь ещё свой собственный window.
document.addEventListener('DOMContentLoaded', function () {
  console.log('PROBE child-dcl parent-is-self=' + (window.parent === window));
  window.parent.postMessage({ hello: 'from-child' }, '*');
});
window.addEventListener('message', function (e) {
  console.log('PROBE child-got ' + JSON.stringify(e.data)
              + ' origin=' + e.origin + ' source=' + (e.source ? 'yes' : 'no'));
  if (e.data && e.data.ping !== undefined) {
    window.parent.postMessage({ pong: e.data.ping }, '*');
  }
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
    parser.add_argument("--seconds", type=float, default=6.0)
    args = parser.parse_args()

    for name, body in [(".vfpm-parent.html", PARENT_PAGE), (".vfpm-child.html", CHILD_PAGE)]:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port, server = _serve()
    log_path = os.path.join(REPO, ".tmp", "vfpm-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    print(f"http://127.0.0.1:{port}/.vfpm-parent.html -> {log_path}")
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{port}/.vfpm-parent.html"],
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

    expect("parent-start")
    expect("child-start")
    expect("parent-load")
    expect("child-dcl parent-is-self=false")
    # Ребёнок -> родитель: данные, origin отправителя (наш собственный http
    # origin), фасад источника.
    expect('parent-got {"hello":"from-child"}')
    # Родитель -> ребёнок.
    expect('child-got {"ping":1}')
    # Ответ ребёнка уже ПОСЛЕ получения сообщения от родителя (раунд-трип).
    expect("pong")
    pong_idx = next((i for i, m in enumerate(markers) if "pong" in m), -1)
    child_got_idx = next((i for i, m in enumerate(markers) if "child-got" in m), -1)
    if 0 <= child_got_idx < pong_idx:
        print("[ok] pong delivered after child received ping (round-trip)")
    else:
        print(f"[MISSING] round-trip order: child-got@{child_got_idx} pong@{pong_idx}")
        ok = False

    print("markers:", *markers, sep="\n  ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
