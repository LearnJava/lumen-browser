#!/usr/bin/env python3
"""BUG-480 срез 5 — живая проверка мутаций под-документа из JS родителя.

Запускает dev-release Lumen на странице с ``<iframe>`` и проверяет по маркерам
в stderr (тот же канал, что у остальных verify_* проб), что запись через фасады
моста доходит до общего дерева и видна контексту ребёнка:

* родитель в обработчике ``load`` фрейма создаёт ``<p>``, выставляет атрибуты,
  ``textContent`` и вставляет в ``body`` ребёнка через ``contentDocument``;
* читает обратно через ``getElementById`` (тот же фасад);
* ставит ``contentDocument.title`` (создание ``<title>`` в head);
* ребёнок опрашивает собственное дерево таймером и рапортует, когда узел
  появился — доказательство, что мутация попала в общий ``Document``, а не в
  копию фасада.

Запуск: ``python tests/wpt/verify_frame_mutation.py [--binary PATH]``
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

PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfmt parent</title>
<body>parent
<iframe src="/.vfmt-child.html"></iframe>
<script>
var frame = document.querySelector('iframe');
frame.addEventListener('load', function () {
  console.log('PROBE parent-load');
  var d = frame.contentDocument;
  var p = d.createElement('p');
  p.id = 'injected';
  p.setAttribute('data-from', 'parent');
  p.textContent = 'from-parent';
  var appended = d.body.appendChild(p);
  console.log('PROBE parent-mutated ' + JSON.stringify({
    appended: appended === p,
    found: !!d.getElementById('injected'),
    text: d.getElementById('injected') ? d.getElementById('injected').textContent : null,
    attr: d.getElementById('injected') ? d.getElementById('injected').getAttribute('data-from') : null
  }));
  d.title = 'mutated-by-parent';
  console.log('PROBE parent-title ' + d.title);
});
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfmt child</title>
<body><p>seed</p>
<script>
console.log('PROBE child-start');
var tries = 0;
var iv = setInterval(function () {
  tries++;
  var el = document.getElementById('injected');
  if (el) {
    clearInterval(iv);
    console.log('PROBE child-sees text=' + el.textContent
                + ' attr=' + el.getAttribute('data-from')
                + ' title=' + document.title);
  } else if (tries > 60) {
    clearInterval(iv);
    console.log('PROBE child-timeout');
  }
}, 100);
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

    for name, body in [(".vfmt-parent.html", PARENT_PAGE), (".vfmt-child.html", CHILD_PAGE)]:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port, server = _serve()
    log_path = os.path.join(REPO, ".tmp", "vfmt-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    print(f"http://127.0.0.1:{port}/.vfmt-parent.html -> {log_path}")
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{port}/.vfmt-parent.html"],
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
    # Вставка вернула тот же фасад, обратное чтение видит узел, атрибуты и текст.
    expect("parent-mutated {\"appended\":true,\"found\":true,"
           "\"text\":\"from-parent\",\"attr\":\"parent\"}")
    # Сеттер title создал <title> в head и записал строку.
    expect("parent-title mutated-by-parent")
    # Ребёнок увидел мутацию в СВОЁМ дереве (общий Document, не копия).
    expect("child-sees text=from-parent attr=parent")
    # Порядок: load → мутация → обнаружение ребёнком.
    order = [("parent-load", idx_of("parent-load")),
             ("parent-mutated", idx_of("parent-mutated")),
             ("child-sees", idx_of("child-sees"))]
    if all(a[1] < b[1] for a, b in zip(order, order[1:])) and order[-1][1] >= 0:
        print("[ok] order parent-load -> parent-mutated -> child-sees")
    else:
        print(f"[MISSING] order: {order}")
        ok = False

    print("markers:", *markers, sep="\n  ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
