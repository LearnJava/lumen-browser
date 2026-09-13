#!/usr/bin/env python3
"""GAP-NAVCTX срез 12 (BUG-883): именованный `target` ссылки, не совпавший
ни с одним живым фреймом текущего документа, теперь сперва ищет уже
открытую ВКЛАДКУ с таким `window.name` (HTML LS §7.3.2 «the rules for
choosing a navigable») и лишь при отсутствии совпадения открывает новую —
вместо того чтобы либо тихо навигировать текущий документ на месте
(ссылка СТРАНИЦЫ, `click.rs`), либо всегда плодить новую вкладку
(ссылка РЕБЁНКА, `frame_links.rs`).

Реальный клик мышью через MCP, а не `element.click()` — тот идёт другим
путём (`_lumen_navigate_or_fragment`), полностью игнорирующим `target`
(тот же урок, что и у среза 10/11 BUG-797).

Сценарий:

1. Страница A — ссылка `#l1 target=dup href=child1.html`. Клик: вкладок с
   именем `dup` ещё нет (`bg_tabs` пуст) — открывается НОВАЯ вкладка B,
   которой шелл присваивает `window.name = "dup"` (иначе повторный клик
   никогда не нашёл бы её).
2. `new_tab` открывает третью вкладку C (страница со ссылкой
   `#l2 target=dup href=child2.html`) — B при этом паркуется в `bg_tabs`.
3. Клик по `#l2` на C: `find_tab_by_window_name("dup")` обязан найти
   запаркованную B и ПЕРЕИСПОЛЬЗОВАТЬ её (`switch_tab`+`navigate_to`) —
   не создавать четвёртую вкладку. Различитель — `history.length` активной
   вкладки после клика: у ПЕРЕИСПОЛЬЗОВАННОЙ B это 2 (child1 → child2 в
   одной сессионной истории), у НОВОЙ вкладки было бы 1.

Запуск: python tests/wpt/verify_gap_navctx_named_target_reuse.py
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

PAGE_A = """<!doctype html><meta charset="utf-8"><title>gnt a</title>
<body style="margin:0">
<a id="l1" href="/.gnt-child1.html" target="dup"
   style="display:block;width:200px;height:40px">go dup</a>
<script>console.log('PROBE a-start');</script>
</body>
"""

PAGE_C = """<!doctype html><meta charset="utf-8"><title>gnt c</title>
<body style="margin:0">
<a id="l2" href="/.gnt-child2.html" target="dup"
   style="display:block;width:200px;height:40px">go dup again</a>
<script>console.log('PROBE c-start');</script>
</body>
"""

CHILD1 = """<!doctype html><meta charset="utf-8"><title>gnt child1</title>
<body><script>console.log('PROBE child1-start name=' + window.name);</script></body>
"""

CHILD2 = """<!doctype html><meta charset="utf-8"><title>gnt child2</title>
<body><script>console.log('PROBE child2-start name=' + window.name +
  ' historylen=' + history.length);</script></body>
"""

PAGES = {
    ".gnt-a.html": PAGE_A,
    ".gnt-c.html": PAGE_C,
    ".gnt-child1.html": CHILD1,
    ".gnt-child2.html": CHILD2,
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
    log_path = os.path.join(REPO, ".tmp", "gnt-named-target-reuse.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.gnt-a.html"
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

            # 1. Клик по ссылке A: нет вкладок с именем "dup" — открывается новая.
            client.call("click", {"target": "#l1"})
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(1.0)
            results["after_l1_path"] = read("location.pathname")
            results["after_l1_name"] = read("window.name")
            results["after_l1_historylen"] = read("history.length")

            # 2. Третья вкладка C — вкладка "dup" при этом паркуется в bg_tabs.
            client.call("new_tab", {"url": f"http://127.0.0.1:{port}/.gnt-c.html"})
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(1.0)

            # 3. Клик по ссылке C: тот же target="dup" обязан ПЕРЕИСПОЛЬЗОВАТЬ
            # вкладку из шага 1, а не открыть четвёртую.
            client.call("click", {"target": "#l2"})
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(1.0)
            results["after_l2_path"] = read("location.pathname")
            results["after_l2_name"] = read("window.name")
            results["after_l2_historylen"] = read("history.length")
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

    check(REQUESTS.get("/.gnt-child1.html", 0) == 1, "child1 запрошен ровно раз")
    check(results.get("after_l1_path") == "/.gnt-child1.html",
          "клик по #l1 открыл child1 в НОВОЙ (единственно возможной) вкладке")
    check(results.get("after_l1_name") == "dup",
          "новая вкладка получила window.name=dup (иначе повторный клик её не найдёт)")

    check(REQUESTS.get("/.gnt-child2.html", 0) == 1, "child2 запрошен ровно раз")
    check(results.get("after_l2_path") == "/.gnt-child2.html",
          "клик по #l2 (та же target=dup, с ДРУГОЙ вкладки C) навигировал куда надо")
    check(results.get("after_l2_name") == "dup",
          "активная после клика вкладка всё ещё называется dup")
    # Различитель "переиспользована/создана заново": у ПЕРЕИСПОЛЬЗОВАННОЙ
    # вкладки history.length растёт на 1 относительно того, чем он уже был
    # ПОСЛЕ первой навигации той же вкладки (child1 → child2 в одной сессии);
    # у совсем НОВОЙ вкладки он был бы равен тому же базовому значению, что и
    # после l1 (свежая сессия с одной записью — child2).
    baseline = results.get("after_l1_historylen")
    check(baseline is not None and results.get("after_l2_historylen") == baseline + 1,
          f"ПЕРЕИСПОЛЬЗОВАННАЯ вкладка несёт свою историю дальше (было {baseline!r} "
          f"после l1, стало {results.get('after_l2_historylen')!r} после l2 — "
          f"+1, не сброс к {baseline!r} на новой вкладке)")

    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
