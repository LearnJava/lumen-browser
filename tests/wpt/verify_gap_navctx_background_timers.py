#!/usr/bin/env python3
"""GAP-NAVCTX срез 15 (BUG-883, последний открытый пункт карточки):
`window.open()` parks the opener tab in `self.bg_tabs`, and before this
slice its `setInterval`/`setTimeout` never fired again while parked — the
engine's per-tick timer pump (`about_to_wait.rs`) only ever drained the
ACTIVE tab's `js_ctx`, plus `frame_js_handles` for sub-documents; a
backgrounded top-level tab's own runtime was left untouched even though its
V8 isolate kept running on its own thread the whole time.

Различитель нужен без переключения вкладок обратно на A (в MCP нет tab-
переключающего инструмента, а `window.close()` в этом движке — намеренно
неполный: он не закрывает саму вкладку, HTML LS §7.4.6, см. комментарий над
`window.close` в `web_api_shim_tail_mc.js`). Поэтому наблюдаем эффект
таймера A, оставаясь на ЕЁ ЖЕ активной вкладке — B:

1. Страница A хранит счётчик `window.__n` и, если попап уже открыт,
   постит его дочерней вкладке через `popup.postMessage(...)` из САМОГО
   `setInterval`-колбэка — то есть сообщение может уйти только если
   колбэк вообще выполнился.
2. A вызывает `window.open('/.bgt-child.html')` — активной становится B
   (уже влитые срезы 2/14 BUG-883), A паркуется в `self.bg_tabs`.
3. B копит входящие `message`-события в `window.__received`. `eval`
   после паузы читает это на самой B, БЕЗ переключения вкладок — то есть
   тест не зависит ни от чего, кроме факта «таймер A сработал хотя бы
   один раз, пока A была в фоне, и смог достучаться до B postMessage-ом»
   (тот же канал доставки, что уже используют срезы 4/5 GAP-NAVCTX,
   независимо влитый и не то, что чинит этот срез).

Запуск: python tests/wpt/verify_gap_navctx_background_timers.py
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

PAGE_A = """<!doctype html><meta charset="utf-8"><title>bgt a</title>
<body style="margin:0">
<script>
window.__n = 0;
window.__popup = null;
setInterval(function () {
  window.__n += 1;
  if (window.__popup) {
    window.__popup.postMessage(String(window.__n), '*');
  }
}, 200);
console.log('PROBE a-start');
</script>
</body>
"""

CHILD = """<!doctype html><meta charset="utf-8"><title>bgt child</title>
<body>
<script>
window.__received = [];
window.addEventListener('message', function (ev) {
  window.__received.push(ev.data);
});
console.log('PROBE child-start');
</script>
</body>
"""

PAGES = {
    ".bgt-a.html": PAGE_A,
    ".bgt-child.html": CHILD,
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
    log_path = os.path.join(REPO, ".tmp", "bgt-background-timers.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.bgt-a.html"
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
            results["baseline_n"] = read("window.__n")

            # window.open() parks A in bg_tabs, B (child) becomes active.
            # A keeps its own reference to the popup so its *own* setInterval
            # callback (running while A is parked) can reach it.
            client.call("eval", {"code": "window.__popup = window.open('/.bgt-child.html')"})
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(1.0)
            results["child_path"] = read("location.pathname")

            # A sits in the background for ~1.5s while B stays active and
            # collects postMessage envelopes A's interval callback sends it —
            # each one only exists if the callback actually ran while parked.
            time.sleep(1.5)

            results["received_len"] = read("window.__received.length")
            results["received_last"] = read(
                "window.__received[window.__received.length - 1]"
            )
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

    check(REQUESTS.get("/.bgt-child.html", 0) == 1, "child запрошен ровно раз")
    check(results.get("child_path") == "/.bgt-child.html",
          "window.open() открыл child в новой активной вкладке")

    baseline_n = results.get("baseline_n")
    check(isinstance(baseline_n, (int, float)) and baseline_n > 0,
          f"базовый счётчик тикал до открытия попапа (__n={baseline_n!r})")

    received_len = results.get("received_len")
    # At 200ms/tick over ~1.5s of background time, expect several deliveries;
    # require at least 2 to rule out a single stray tick racing the pump.
    check(isinstance(received_len, (int, float)) and received_len >= 2,
          f"child получил {received_len!r} postMessage-ов от A, пока A была "
          f"в фоне (0 значило бы, что setInterval A не тикает в бэкграунде)")

    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
