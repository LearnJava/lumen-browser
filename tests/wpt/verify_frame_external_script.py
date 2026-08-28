#!/usr/bin/env python3
"""BUG-480 срез 9 — живая проверка внешнего ``<script src>`` из родителя.

Родитель строит во фрейме два скрипта фасадными фабриками:

* ``A`` — каноничный порядок «src до вставки» (``s.src = url`` →
  ``appendChild``): сеттер URL-рефлексии пишет атрибут через натив записи,
  вставка ставит конверт RunScript;
* ``B`` — поздний порядок: вставка ПУСТЫМ (первая доставка не начинает
  элемент и не помечает его), затем ``s.src = url`` ПОСЛЕ appendChild —
  натив записи атрибута ставит второй конверт.

Оба исполняются ребёнком на его тике штатным ``_lumen_script_prepare``:
fetch силами провайдеров самого фрейма, отчёт кросс-фреймовым postMessage.
Дополнительно проверяется, что атрибут в дереве ребёнка виден (чтение через
фасадный геттер разрешает против базы под-документа).

Запуск: ``python tests/wpt/verify_frame_external_script.py [--binary PATH]``
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

PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfes parent</title>
<body>parent
<iframe src="/.vfes-child.html"></iframe>
<script>
console.log('PROBE parent-start');
window.addEventListener('message', function (ev) {
  console.log('PROBE parent-got ' + JSON.stringify(ev.data));
});
var frame = document.querySelector('iframe');
frame.addEventListener('load', function () {
  console.log('PROBE parent-load');
  var d = frame.contentDocument;
  // Ловушка ошибок ребёнка: любой сбой конвейера становится видимым постом.
  var boot = d.createElement('script');
  boot.textContent =
    "window.__rep = function(x){try{window.parent.postMessage({probe: String(x)}, '*');}catch(e){}};" +
    "window.onerror = function(msg){ window.__rep('onerror: ' + msg); };";
  d.body.appendChild(boot);
  // A: src ДО вставки.
  var a = d.createElement('script');
  a.src = '/.vfes-ext-a.js';
  d.body.appendChild(a);
  // B: вставка пустым, src ПОСЛЕ вставки.
  var b = d.createElement('script');
  d.body.appendChild(b);
  b.src = '/.vfes-ext-b.js';
  // Геттер рефлексии читает атрибут из дерева ребёнка, разрешённый против
  // базы под-документа (абсолютный URL того же origin).
  window.__rep2 = function(x){ try { window.parent.postMessage({ probe2: String(x) }, '*'); } catch (e) {} };
  window.__rep2(d.querySelector('script[src]').src);
  console.log('PROBE parent-inserted');
});
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfes child</title>
<body>child</body>
"""

EXT_A_JS = "window.parent.postMessage({ ranA: true }, '*');\n"
EXT_B_JS = "window.parent.postMessage({ ranB: true }, '*');\n"


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


def _serve() -> tuple[int, http.server.ThreadingHTTPServer, list[str]]:
    seen: list[str] = []
    port = _free_port()

    class _Tracking(_Quiet):
        def do_GET(self):
            seen.append(self.path)
            super().do_GET()

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Tracking)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server, seen


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        default=os.path.join(REPO, "target", "dev-release", "lumen.exe"),
    )
    parser.add_argument("--seconds", type=float, default=8.0)
    args = parser.parse_args()

    files = [
        (".vfes-parent.html", PARENT_PAGE),
        (".vfes-child.html", CHILD_PAGE),
        (".vfes-ext-a.js", EXT_A_JS),
        (".vfes-ext-b.js", EXT_B_JS),
    ]
    for name, body in files:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port, server, requests_seen = _serve()
    log_path = os.path.join(REPO, ".tmp", "vfes-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    print(f"http://127.0.0.1:{port}/.vfes-parent.html -> {log_path}")
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{port}/.vfes-parent.html"],
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
    probes: list[str] = []
    for marker in markers:
        if marker.startswith("parent-got "):
            try:
                payload = json.loads(marker[len("parent-got "):])
            except json.JSONDecodeError:
                continue
            got.append(payload)
            if isinstance(payload.get("probe"), str):
                probes.append(payload["probe"])
            if isinstance(payload.get("probe2"), str):
                probes.append("resolved:" + payload["probe2"])

    ok = True

    def expect_post(pred, label: str) -> None:
        nonlocal ok
        hit = any(pred(item) for item in got)
        status = "ok" if hit else "MISSING"
        if not hit:
            ok = False
        print(f"[{status}] {label}")

    def expect_request(suffix: str, label: str) -> None:
        nonlocal ok
        hit = any(r.endswith(suffix) for r in requests_seen)
        status = "ok" if hit else "MISSING"
        if not hit:
            ok = False
        print(f"[{status}] {label}")

    def expect_marker(label: str, pred=None) -> None:
        nonlocal ok
        hit = any((pred(p) if pred else bool(p)) for p in probes)
        status = "ok" if hit else "MISSING"
        if not hit:
            ok = False
        print(f"[{status}] {label}")

    def reject_onerror() -> None:
        nonlocal ok
        hit = any(p.startswith("onerror:") for p in probes)
        status = "ok (absent)" if not hit else "UNEXPECTED"
        if hit:
            ok = False
        print(f"[{status}] no child onerror")

    expect_request(".vfes-ext-a.js", "server saw ext-A request")
    expect_request(".vfes-ext-b.js", "server saw ext-B request (late src)")
    expect_post(lambda p: p.get("ranA") is True, "ext-A executed in child")
    expect_post(lambda p: p.get("ranB") is True, "ext-B executed in child (late src)")
    # Геттер рефлексии вернул АБСОЛЮТНЫЙ URL (настоящий _url_resolve против
    # базы под-документа), а не сырую запись '​/.vfes-ext-a.js'.
    port_str = str(port)
    expect_marker(
        "facade src getter resolves against child base",
        lambda p: f"http://127.0.0.1:{port_str}/.vfes-ext-a.js" in p,
    )
    reject_onerror()

    print("markers:", *markers, sep="\n  ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
