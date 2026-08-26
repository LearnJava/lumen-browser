#!/usr/bin/env python3
"""BUG-480 срез 10 — живая проверка обратной доставки ресурсных событий.

Родитель строит во фрейме два внешних скрипта фасадными фабриками и назначает
на фасады обработчики ``onload``/``onerror``:

* ``A`` — сервер отдаёт 200: ребёнок исполняет тело (постит ``ranA``),
  ресурсное событие ``load`` зеркалится в родителя, ``a.onload`` вызывается;
* ``B`` — сервер отдаёт 404: спековый ``error`` на элементе ребёнка,
  ``b.onerror`` вызывается в родителе.

Дополнительно: слушатель ``addEventListener('load', …)`` на фасаде и проверка
поля события (``target === s``, ``isTrusted === true``).

Запуск: ``python tests/wpt/verify_frame_resource_events.py [--binary PATH]``
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

PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfre parent</title>
<body>parent
<iframe src="/.vfre-child.html"></iframe>
<script>
console.log('PROBE parent-start');
window.addEventListener('message', function (ev) {
  console.log('PROBE parent-got ' + JSON.stringify(ev.data));
});
var frame = document.querySelector('iframe');
frame.addEventListener('load', function () {
  var d = frame.contentDocument;
  // Ловушка ошибок ребёнка.
  var boot = d.createElement('script');
  boot.textContent =
    "window.onerror = function(msg){ try { window.parent.postMessage({childErr: String(msg)}, '*'); } catch (e) {} };";
  d.body.appendChild(boot);
  // A: загрузка успешна.
  var a = d.createElement('script');
  a.src = '/.vfre-ok.js';
  a.onload = function (ev) {
    console.log('PROBE a-onload target=' + ev.target.localName +
                ' same=' + (ev.target === a) + ' trusted=' + ev.isTrusted +
                ' bubbles=' + ev.bubbles);
  };
  a.addEventListener('load', function () { console.log('PROBE a-listener-load'); });
  d.body.appendChild(a);
  // B: 404 — error на элементе.
  var b = d.createElement('script');
  b.src = '/.vfre-missing.js';
  b.onload = function () { console.log('PROBE b-onload-UNEXPECTED'); };
  b.onerror = function () { console.log('PROBE b-onerror'); };
  d.body.appendChild(b);
  console.log('PROBE parent-inserted');
});
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfre child</title>
<body>child</body>
"""

OK_JS = "window.parent.postMessage({ ranA: true }, '*');\n"


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


def _serve() -> tuple[int, http.server.ThreadingHTTPServer, list[str], list[int]]:
    seen: list[str] = []
    statuses: list[int] = []
    port = _free_port()

    class _Tracking(_Quiet):
        def do_GET(self):
            seen.append(self.path)
            if self.path.endswith(".vfre-missing.js"):
                self.send_error(404)
                statuses.append(404)
                return
            super().do_GET()
            # SimpleHTTPRequestHandler уже ответил; статус не критичен здесь.

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Tracking)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server, seen, statuses


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        default=os.path.join(REPO, "target", "dev-release", "lumen.exe"),
    )
    parser.add_argument("--seconds", type=float, default=8.0)
    args = parser.parse_args()

    files = [
        (".vfre-parent.html", PARENT_PAGE),
        (".vfre-child.html", CHILD_PAGE),
        (".vfre-ok.js", OK_JS),
    ]
    for name, body in files:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port, server, requests_seen, _statuses = _serve()
    log_path = os.path.join(REPO, ".tmp", "vfre-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    print(f"http://127.0.0.1:{port}/.vfre-parent.html -> {log_path}")
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{port}/.vfre-parent.html"],
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
        content = handle.read()
    markers = re.findall(r"PROBE ([^\n\r]+)", content)
    got: list[dict] = []

    def on_marker(marker: str) -> None:
        if marker.startswith("parent-got "):
            try:
                payload = json.loads(marker[len("parent-got "):])
            except json.JSONDecodeError:
                return
            got.append(payload)

    for marker in markers:
        on_marker(marker)

    ok = True

    def expect(cond: bool, label: str) -> None:
        nonlocal ok
        status = "ok" if cond else "MISSING"
        if not cond:
            ok = False
        print(f"[{status}] {label}")

    def has_marker(text: str) -> bool:
        return any(m == text or m.startswith(text) for m in markers)

    expect_request = any(r.endswith(".vfre-ok.js") for r in requests_seen)
    expect(expect_request, "server saw ok.js request")
    expect(
        any(r.endswith(".vfre-missing.js") for r in requests_seen),
        "server saw missing.js request",
    )
    expect(any(g.get("ranA") is True for g in got), "ok.js executed in child")
    expect(has_marker("a-onload"), "facade a.onload fired with mirrored load event")
    expect(
        any(m.startswith("a-onload ") and "target=script" in m and "same=true" in m
            and "trusted=true" in m and "bubbles=false" in m
            for m in markers),
        "event fields: trusted script facade, bubbles=false",
    )
    expect(has_marker("a-listener-load"), "facade addEventListener('load') fired")
    expect(has_marker("b-onerror"), "facade b.onerror fired on 404")
    expect(not has_marker("b-onload-UNEXPECTED"), "no spurious b.onload")

    print("markers:", *markers, sep="\n  ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
