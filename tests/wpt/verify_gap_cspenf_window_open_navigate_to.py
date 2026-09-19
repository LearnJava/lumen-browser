#!/usr/bin/env python3
"""GAP-CSPENF срез 35: директива `navigate-to` теперь гейтит `window.open()`
— пятая навигационная директива этого модуля (после `frame-ancestors`/
`form-action`/`base-uri`/навигации `location.href=`/`.assign()`/`.replace()`
среза 34). До этого среза `Lumen::on_about_to_wait`'а `window_open_requests`-
ветка (`about_to_wait.rs`) никогда не спрашивала CSP — `navigate-to 'self'`
не мешало `window.open('https://other.example/…')` уйти куда угодно.

Сценарий (опекунская страница A, `navigate-to 'self'`):

1. `window.open('/.wo-allowed.html')` — тот же origin, что документа A;
   должно пройти нормально: сервер видит запрос, новая вкладка на нём.
2. `window.open('http://127.0.0.1:<blocked_port>/.wo-blocked.html')` —
   ДРУГОЙ origin (порт не совпадает — `navigate-to 'self'` сравнивает origin
   целиком, включая порт), причём на `blocked_port` сервер НЕ поднят: если
   бы гейт молчал, попытка навигации уткнулась бы в connection-refused, а не
   зависла — сам факт «нет отказа сети в логе» ничего не доказывает. Улика —
   `stderr`-строка `window.open: navigation to … blocked by CSP navigate-to`,
   которую печатает новый `Lumen::window_open_navigate_to_blocked` ПЕРЕД тем,
   как `resolve_js_navigation`/сеть вообще были бы вызваны.

Запуск: python tests/wpt/verify_gap_cspenf_window_open_navigate_to.py
    --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
"""

from __future__ import annotations

import argparse
import http.server
import json
import os
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

PAGE_A_TEMPLATE = """<!doctype html><meta charset="utf-8"><title>wo a</title>
<meta http-equiv="Content-Security-Policy" content="navigate-to 'self'">
<body style="margin:0">
<script>console.log('PROBE a-start');</script>
</body>
"""

CHILD_ALLOWED = """<!doctype html><meta charset="utf-8"><title>wo allowed</title>
<body><script>console.log('PROBE allowed-start');</script></body>
"""

PAGES = {
    ".wo-a.html": PAGE_A_TEMPLATE,
    ".wo-allowed.html": CHILD_ALLOWED,
}

REQUESTS: Counter = Counter()


def _free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class _Recording(http.server.SimpleHTTPRequestHandler):
    """Отдаёт страницы пробы и считает запросы (сервер, не браузер, —
    единственный надёжный свидетель того, что документ реально запрашивался,
    BUG-826)."""

    protocol_version = "HTTP/1.1"

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=HERE, **kwargs)

    def do_GET(self):  # noqa: N802
        path = self.path.split("?")[0]
        REQUESTS[path] += 1
        body = PAGES.get(path.lstrip("/"), "").encode("utf-8")
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary", default=os.path.join(REPO, "target", "dev-release", "lumen.exe")
    )
    args = parser.parse_args()

    port = _free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Recording)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    # Порт для "чужого" origin — НАМЕРЕННО без сервера: если гейт молчит,
    # запрос уткнётся в connection-refused (быстро), а не зависнет.
    blocked_port = _free_port()
    mcp_port = _free_port()
    log_path = os.path.join(REPO, ".tmp", "gap-cspenf-wo-navigate-to.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.wo-a.html"
    blocked_url = f"http://127.0.0.1:{blocked_port}/.wo-blocked.html"
    print(f"{url} -> {log_path}")

    results: dict[str, object] = {}
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(mcp_port), url],
            stdout=subprocess.DEVNULL, stderr=log, text=True, cwd=HERE,
        )
        try:
            def read(code: str):
                raw = client.call("eval", {"code": code}).get("result")
                return json.loads(raw) if isinstance(raw, str) else raw

            client = Client(mcp_port, log_path)
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(1.0)

            # 1. Разрешённая (same-origin) цель — должна пройти нормально.
            client.call("eval", {"code": "window.open('/.wo-allowed.html')"})
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(1.0)
            results["after_allowed_path"] = read("location.pathname")

            # 2. Cross-origin цель под запрещающей `navigate-to 'self'` —
            # должна быть заблокирована ДО сети.
            client.call("new_tab", {"url": url})
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(1.0)
            client.call("eval", {"code": f"window.open('{blocked_url}')"})
            time.sleep(1.5)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
            server.shutdown()

    print("запросы к серверу:")
    for path, n in sorted(REQUESTS.items()):
        print(f"  GET {path} x{n}")
    print("результаты:", results)

    with open(log_path, "r", encoding="utf-8", errors="replace") as f:
        stderr_text = f.read()
    blocked_line = "window.open: navigation to " in stderr_text and "blocked by CSP navigate-to" in stderr_text
    blocked_requested = REQUESTS.get("/.wo-blocked.html", 0) > 0

    ok = True

    def check(cond: bool, text: str) -> None:
        nonlocal ok
        ok &= bool(cond)
        print(f"[{'OK  ' if cond else 'ФЕЙЛ'}] {text}")

    check(REQUESTS.get("/.wo-allowed.html", 0) == 1,
          "same-origin window.open() запросил allowed-страницу ровно раз")
    check(results.get("after_allowed_path") == "/.wo-allowed.html",
          "same-origin window.open() навигировал новую вкладку туда")
    check(blocked_line,
          "cross-origin window.open() дал строку "
          "'window.open: navigation to … blocked by CSP navigate-to' в stderr")
    check(not blocked_requested,
          "cross-origin window.open() НЕ дошёл до попытки сети (blocked-порт не тронут)")

    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
