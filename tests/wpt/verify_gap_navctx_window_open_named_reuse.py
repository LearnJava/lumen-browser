#!/usr/bin/env python3
"""GAP-NAVCTX срез 13 (BUG-883): `window.open(url, name)` с ИМЕНОВАННЫМ (не
`_blank`/`_self`) `target`, не совпавшим ни с одной живой вкладкой, теперь
сперва ищет уже открытую вкладку с таким `window.name` (HTML LS §7.3.2, тот
же `Lumen::find_tab_by_window_name`, что срез 12 уже подключил к клику по
`<a target=…>`) и лишь при отсутствии совпадения открывает новую — раньше
`window.open()` минтил новую вкладку при КАЖДОМ вызове вне зависимости от
`target` (`PopupRequest::target` был захвачен, но нигде не читался).

Сценарий:

1. Страница A вызывает `window.open('/.wo-child1.html', 'dup')` напрямую
   (`eval`, не клик по ссылке — `window.open()` не идёт через
   `click.rs`/`frame_links.rs`, это отдельный путь в `about_to_wait.rs`).
   Вкладок с именем `dup` ещё нет — открывается НОВАЯ вкладка B, шелл
   присваивает ей `window.name = "dup"`.
2. `new_tab` открывает третью вкладку C — B при этом паркуется в `bg_tabs`.
3. C вызывает `window.open('/.wo-child2.html', 'dup')`:
   `find_tab_by_window_name("dup")` обязан найти запаркованную B и
   ПЕРЕИСПОЛЬЗОВАТЬ её (`switch_tab`+`navigate_to`), не создавать
   четвёртую вкладку. Различитель — `history.length` активной вкладки
   после вызова: у ПЕРЕИСПОЛЬЗОВАННОЙ B это 2 (child1 → child2 в одной
   сессионной истории), у НОВОЙ вкладки было бы 1.

Запуск: python tests/wpt/verify_gap_navctx_window_open_named_reuse.py
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

PAGE_A = """<!doctype html><meta charset="utf-8"><title>wo a</title>
<body style="margin:0">
<script>console.log('PROBE a-start');</script>
</body>
"""

PAGE_C = """<!doctype html><meta charset="utf-8"><title>wo c</title>
<body style="margin:0">
<script>console.log('PROBE c-start');</script>
</body>
"""

CHILD1 = """<!doctype html><meta charset="utf-8"><title>wo child1</title>
<body><script>console.log('PROBE child1-start name=' + window.name);</script></body>
"""

CHILD2 = """<!doctype html><meta charset="utf-8"><title>wo child2</title>
<body><script>console.log('PROBE child2-start name=' + window.name +
  ' historylen=' + history.length);</script></body>
"""

PAGES = {
    ".wo-a.html": PAGE_A,
    ".wo-c.html": PAGE_C,
    ".wo-child1.html": CHILD1,
    ".wo-child2.html": CHILD2,
}

REQUESTS: Counter = Counter()


def _free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class _Recording(http.server.SimpleHTTPRequestHandler):
    """Отдаёт страницы пробы и считает запросы (сервер, не браузер, — единственный
    надёжный свидетель того, что документ реально запрашивался, BUG-826)."""

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
    mcp_port = _free_port()
    log_path = os.path.join(REPO, ".tmp", "wo-named-target-reuse.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.wo-a.html"
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

            # 1. window.open с target="dup": нет вкладок с этим именем —
            # открывается новая.
            client.call("eval", {"code": "window.open('/.wo-child1.html', 'dup')"})
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(1.0)
            results["after_o1_path"] = read("location.pathname")
            results["after_o1_name"] = read("window.name")
            results["after_o1_historylen"] = read("history.length")

            # 2. Третья вкладка C — вкладка "dup" при этом паркуется в bg_tabs.
            client.call("new_tab", {"url": f"http://127.0.0.1:{port}/.wo-c.html"})
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(1.0)

            # 3. window.open с тем же target="dup" с ДРУГОЙ вкладки обязан
            # ПЕРЕИСПОЛЬЗОВАТЬ вкладку из шага 1, а не открыть четвёртую.
            client.call("eval", {"code": "window.open('/.wo-child2.html', 'dup')"})
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(1.0)
            results["after_o2_path"] = read("location.pathname")
            results["after_o2_name"] = read("window.name")
            results["after_o2_historylen"] = read("history.length")
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

    ok = True

    def check(cond: bool, text: str) -> None:
        nonlocal ok
        ok &= bool(cond)
        print(f"[{'OK  ' if cond else 'ФЕЙЛ'}] {text}")

    check(REQUESTS.get("/.wo-child1.html", 0) == 1, "child1 запрошен ровно раз")
    check(results.get("after_o1_path") == "/.wo-child1.html",
          "window.open(url,'dup') открыл child1 в НОВОЙ (единственно возможной) вкладке")
    check(results.get("after_o1_name") == "dup",
          "новая вкладка получила window.name=dup (иначе повторный open её не найдёт)")

    check(REQUESTS.get("/.wo-child2.html", 0) == 1, "child2 запрошен ровно раз")
    check(results.get("after_o2_path") == "/.wo-child2.html",
          "window.open(url,'dup') с ДРУГОЙ вкладки C навигировал куда надо")
    check(results.get("after_o2_name") == "dup",
          "активная после open вкладка всё ещё называется dup")
    # Различитель "переиспользована/создана заново": у ПЕРЕИСПОЛЬЗОВАННОЙ
    # вкладки history.length растёт на 1 относительно того, чем он уже был
    # ПОСЛЕ первой навигации той же вкладки (child1 → child2 в одной сессии);
    # у совсем НОВОЙ вкладки он был бы равен тому же базовому значению, что и
    # после o1 (свежая сессия с одной записью — child2).
    baseline = results.get("after_o1_historylen")
    check(baseline is not None and results.get("after_o2_historylen") == baseline + 1,
          f"ПЕРЕИСПОЛЬЗОВАННАЯ вкладка несёт свою историю дальше (было {baseline!r} "
          f"после o1, стало {results.get('after_o2_historylen')!r} после o2 — "
          f"+1, не сброс к {baseline!r} на новой вкладке)")

    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
