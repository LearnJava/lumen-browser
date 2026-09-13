#!/usr/bin/env python3
"""GAP-NAVCTX срез 14 (BUG-883): `window.open(url, '_self')` (HTML LS §7.3.2)
must navigate the CALLING browsing context in place, not open a new tab —
before this slice `_self` fell through to the unnamed-target branch in
`about_to_wait.rs` and minted a brand new tab on every call, exactly like an
unnamed `window.open(url)` does.

Сценарий:

1. Страница A грузится (history.length=1), затем сама навигируется на себя
   ещё раз через `location.href` (history.length=2) — известная база ДО
   вызова `_self`.
2. A вызывает `window.open('/.wos-child.html', '_self')`.
3. Различитель "навигация на месте / новая вкладка": у ПРАВИЛЬНОГО поведения
   (навигация ТЕКУЩЕЙ вкладки) `history.length` в активной вкладке после
   вызова растёт на 1 относительно базы (2 → 3, несёт сессионную историю A
   дальше). У БАГОВОГО поведения (открыть новую вкладку) `history.length`
   активной вкладки после вызова был бы 1 (свежая вкладка с единственной
   записью — child).

Запуск: python tests/wpt/verify_gap_navctx_window_open_self_target.py
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

PAGE_A = """<!doctype html><meta charset="utf-8"><title>wos a</title>
<body style="margin:0">
<script>console.log('PROBE a-start');</script>
</body>
"""

CHILD = """<!doctype html><meta charset="utf-8"><title>wos child</title>
<body><script>console.log('PROBE child-start historylen=' + history.length);</script></body>
"""

PAGES = {
    ".wos-a.html": PAGE_A,
    ".wos-child.html": CHILD,
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
    log_path = os.path.join(REPO, ".tmp", "wo-self-target.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.wos-a.html"
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

            # 1. Bump history.length to 2 with an ordinary self-navigation,
            # so a later "did it grow by 1 or reset to 1" check has a base
            # above 1 (a fresh reset would otherwise be indistinguishable
            # from "grew from 1 to 1+1=2" by coincidence).
            client.call("eval", {"code": "location.href = location.href"})
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(1.0)
            results["baseline_historylen"] = read("history.length")

            # 2. window.open(url, '_self') must navigate THIS tab in place.
            client.call("eval", {"code": "window.open('/.wos-child.html', '_self')"})
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(1.0)
            results["after_path"] = read("location.pathname")
            results["after_historylen"] = read("history.length")
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

    check(REQUESTS.get("/.wos-child.html", 0) == 1, "child запрошен ровно раз")
    check(results.get("after_path") == "/.wos-child.html",
          "window.open(url,'_self') навигировал на child")
    # Различитель "навигация на месте/новая вкладка": ТА ЖЕ вкладка несёт
    # свою историю дальше (+1 относительно базы), НОВАЯ вкладка начала бы
    # со свежей историей длиной 1.
    baseline = results.get("baseline_historylen")
    check(baseline is not None and baseline > 1,
          f"база history.length={baseline!r} > 1 (иначе тест не различает рост/сброс)")
    check(baseline is not None and results.get("after_historylen") == baseline + 1,
          f"ТА ЖЕ вкладка несёт историю дальше (было {baseline!r}, стало "
          f"{results.get('after_historylen')!r} — +1, не сброс к 1 как у новой вкладки)")

    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
