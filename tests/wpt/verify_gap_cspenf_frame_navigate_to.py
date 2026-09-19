#!/usr/bin/env python3
"""GAP-CSPENF срез 36: директива `navigate-to` теперь гейтит ссылки под-
документа `<iframe>`, а не только страницы (срез 33) — до этого среза
`frame_links.rs::frame_link_click` никогда не спрашивал CSP: `navigate-to
'self'`, объявленная РЕБЁНКОМ в своей же `<meta>`, не мешала клику по его
собственной ссылке уйти на чужой origin.

Сценарий (два фрейма родителя, у каждого своя `<meta
http-equiv="Content-Security-Policy" content="navigate-to 'self'">`):

1. `f1` содержит ссылку на СВОЙ origin (`/.fnlv-allowed.html`) — должна
   пройти: сервер видит запрос, окно фрейма меняет цвет.
2. `f2` содержит ссылку на ДРУГОЙ origin (`http://127.0.0.1:<blocked_port>/x`,
   порт без поднятого сервера) — гейт обязан остановить её ДО сети: если бы
   он молчал, попытка уткнулась бы в connection-refused, а не тишину, так что
   сам факт «сервер на blocked_port не видел запроса» — недостаточная улика,
   решает `stderr`-маркер `iframe: navigation to … blocked by CSP
   navigate-to`, который печатает новый `Lumen::frame_navigate_to_link_blocked`
   ПЕРЕД тем, как `navigate_frame_to`/`navigate_to`/сеть вообще были бы
   вызваны.

Запуск: python tests/wpt/verify_gap_cspenf_frame_navigate_to.py
    --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
"""

from __future__ import annotations

import argparse
import base64
import http.server
import json
import os
import re
import socket
import subprocess
import sys
import threading
import time
from collections import Counter

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(REPO, "scripts"))

from scroll_perf import Client  # noqa: E402  (после sys.path)

CSP_META = '<meta http-equiv="Content-Security-Policy" content="navigate-to \'self\'">'

PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>fnlv parent</title>
<body style="margin:0;background:#fff">
<iframe id="f1" src="/.fnlv-allow-src.html" style="position:absolute;left:40px;top:40px;
        width:300px;height:160px;border:0"></iframe>
<iframe id="f2" src="/.fnlv-block-src.html" style="position:absolute;left:40px;top:240px;
        width:300px;height:160px;border:0"></iframe>
<script>
console.log('PROBE parent-start ' + location.pathname);
function fnlvRect(id) {
  var r = document.getElementById(id).getBoundingClientRect();
  return [r.left, r.top, r.width, r.height];
}
setTimeout(function () {
  console.log('PROBE parent-rects ' + JSON.stringify({f1: fnlvRect('f1'), f2: fnlvRect('f2')}));
}, 800);
</script>
</body>
"""

ALLOW_SRC_PAGE = f"""<!doctype html><meta charset="utf-8"><title>fnlv allow-src</title>
{CSP_META}
<body style="margin:0;background:rgb(255,0,0)">
<a id="lnav" href="/.fnlv-allowed.html" style="display:block;width:250px;height:40px;
   background:#fff">same-origin</a>
<script>
console.log('PROBE allow-src-start ' + location.pathname);
setTimeout(function () {{
  var r = document.getElementById('lnav').getBoundingClientRect();
  console.log('PROBE allow-src-rects ' + JSON.stringify({{lnav: [r.left, r.top, r.width, r.height]}}));
}}, 700);
</script>
</body>
"""

ALLOWED_PAGE = """<!doctype html><meta charset="utf-8"><title>fnlv allowed</title>
<body style="margin:0;background:rgb(0,200,0)">
<script>console.log('PROBE allowed-start ' + location.pathname);</script>
</body>
"""


def blocked_href(port: int) -> str:
    return f"http://127.0.0.1:{port}/.fnlv-blocked.html"


def block_src_page(blocked_port: int) -> str:
    return f"""<!doctype html><meta charset="utf-8"><title>fnlv block-src</title>
{CSP_META}
<body style="margin:0;background:rgb(255,0,0)">
<a id="lnav" href="{blocked_href(blocked_port)}" style="display:block;width:250px;height:40px;
   background:#fff">cross-origin</a>
<script>
console.log('PROBE block-src-start ' + location.pathname);
setTimeout(function () {{
  var r = document.getElementById('lnav').getBoundingClientRect();
  console.log('PROBE block-src-rects ' + JSON.stringify({{lnav: [r.left, r.top, r.width, r.height]}}));
}}, 700);
</script>
</body>
"""


def _free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


REQUESTS: Counter = Counter()


class _Recording(http.server.SimpleHTTPRequestHandler):
    """Отдаёт страницы пробы и считает запросы — только сервер знает, ходил ли
    браузер за документом (BUG-826)."""

    protocol_version = "HTTP/1.1"

    def __init__(self, *args, pages=None, **kwargs):
        self._pages = pages or {}
        super().__init__(*args, directory=HERE, **kwargs)

    def do_GET(self):  # noqa: N802
        path = self.path.split("?")[0]
        REQUESTS[path] += 1
        body = self._pages.get(path.lstrip("/"), "").encode("utf-8")
        if not body:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def _markers(log_path: str) -> list[str]:
    with open(log_path, encoding="utf-8", errors="replace") as handle:
        return re.findall(r"PROBE ([^\n\r]+)", handle.read())


def _rects(markers: list[str], prefix: str) -> dict[str, list[float]]:
    for m in markers:
        if m.startswith(prefix):
            return json.loads(m[len(prefix):])
    return {}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO, "target", "dev-release", "lumen.exe"))
    args = parser.parse_args()

    blocked_port = _free_port()
    pages = {
        ".fnlv-parent.html": PARENT_PAGE,
        ".fnlv-allow-src.html": ALLOW_SRC_PAGE,
        ".fnlv-allowed.html": ALLOWED_PAGE,
        ".fnlv-block-src.html": block_src_page(blocked_port),
    }

    port = _free_port()

    def handler(*a, **kw):
        return _Recording(*a, pages=pages, **kw)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    mcp_port = _free_port()
    log_path = os.path.join(REPO, ".tmp", "fnlv-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.fnlv-parent.html"
    print(f"{url} -> {log_path}")

    pr: dict[str, list[float]] = {}
    ar: dict[str, list[float]] = {}
    br: dict[str, list[float]] = {}
    shots: dict[str, bytes] = {}
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(mcp_port), url],
            stdout=subprocess.DEVNULL, stderr=log, text=True, cwd=HERE,
        )
        try:
            client = Client(mcp_port, log_path)
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(3.0)

            def click(x: float, y: float, pause: float = 1.5) -> None:
                client.call("click", {"target": {"point": {"x": x, "y": y}}})
                time.sleep(pause)

            start = _markers(log_path)
            pr = _rects(start, "parent-rects ")
            ar = _rects(start, "allow-src-rects ")
            br = _rects(start, "block-src-rects ")
            print("фреймы родителя:", pr)
            print("ссылка allow-src:", ar, " ссылка block-src:", br)

            def в_фрейме(frame: list[float], rect: list[float]):
                return (frame[0] + rect[0] + rect[2] / 2, frame[1] + rect[1] + rect[3] / 2)

            if pr and ar:
                # 1. Same-origin ссылка ребёнка: navigate-to 'self' пропускает.
                click(*в_фрейме(pr["f1"], ar["lnav"]), pause=2.0)
            if pr and br:
                # 2. Cross-origin ссылка ребёнка: navigate-to 'self' блокирует.
                click(*в_фрейме(pr["f2"], br["lnav"]), pause=2.0)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
            server.shutdown()

    markers = _markers(log_path)
    with open(log_path, encoding="utf-8", errors="replace") as handle:
        stderr_text = handle.read()
    ok = True

    def check(cond: bool, text: str) -> None:
        nonlocal ok
        ok &= bool(cond)
        print(f"[{'OK  ' if cond else 'ФЕЙЛ'}] {text}")

    print("запросы к серверу:")
    for path, n in sorted(REQUESTS.items()):
        print(f"  GET {path} x{n}")

    check(any(m.startswith("parent-start") for m in markers), "родитель загрузился")
    check(any(m.startswith("allow-src-start") for m in markers), "allow-src фрейм загрузился")
    check(any(m.startswith("block-src-start") for m in markers), "block-src фрейм загрузился")
    check(bool(pr) and bool(ar) and bool(br), "все прямоугольники получены")

    # 1. Разрешённая навигация: сервер видит ровно один запрос, второй документ отчитался.
    check(REQUESTS.get("/.fnlv-allowed.html", 0) == 1,
          f"same-origin ссылка ребёнка прошла (x{REQUESTS.get('/.fnlv-allowed.html', 0)})")
    check(any(m.startswith("allowed-start") for m in markers),
          "документ same-origin навигации реально загрузился")

    # 2. Заблокированная навигация: ни один сервер (ни blocked_port, ни основной) не видел запроса.
    check(f"/.fnlv-blocked.html" not in REQUESTS,
          "cross-origin документ не запрашивался вовсе")
    check("iframe: navigation to " in stderr_text and "blocked by CSP navigate-to" in stderr_text,
          "stderr содержит маркер блокировки navigate-to")

    print("маркеры:", *markers, sep="\n  ")
    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
