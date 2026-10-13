#!/usr/bin/env python3
"""Эхо-сервер заголовков запроса (COMPAT-AB-IDENTITY-HARNESS).

Отвечает на любой GET JSON-ом `{"request_line", "headers": [[имя, значение], ...]}`
— имена и порядок такие, как клиент их прислал. Так снимают «набор заголовков»
настоящего Chrome (`compat_ab_chrome.py --echo`) и Lumen (`header_matrix.py
capture`) и сравнивают их без догадок по исходникам: комментарий «Chrome sends
DNT by default» в `headers.rs` был неверен именно потому, что никто не смотрел
на провод (BUG-1113).

Сервер слушает plain HTTP на 127.0.0.1: заголовки H1-запроса те же, что уйдут на
сайт по TLS, кроме тех, что браузер добавляет только по HTTPS-схеме или после
`Accept-CH` (см. `header_matrix.py capture --note`).

    python scripts/header_echo_server.py [--port 8099]          # руками, Ctrl+C
    from header_echo_server import EchoServer                   # из скриптов
"""

from __future__ import annotations

import argparse
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")


class _Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self) -> None:  # noqa: N802 (имя задаёт BaseHTTPRequestHandler)
        record = {
            "request_line": self.requestline,
            "path": self.path,
            "headers": [[k, v] for k, v in self.headers.items()],
        }
        self.server.captured.append(record)  # type: ignore[attr-defined]
        body = json.dumps(record, ensure_ascii=False).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    do_HEAD = do_GET  # noqa: N815

    def log_message(self, fmt: str, *args) -> None:  # тишина: вывод читает человек
        pass


class _QuietServer(ThreadingHTTPServer):
    daemon_threads = True

    def handle_error(self, request, client_address) -> None:
        # Клиенты рвут keep-alive-соединения (Lumen, Chrome) — это не ошибка эха.
        pass


class EchoServer:
    """Фоновый эхо-сервер. `captured` — запросы в порядке прихода."""

    def __init__(self, port: int = 0) -> None:
        self.httpd = _QuietServer(("127.0.0.1", port), _Handler)
        self.httpd.captured = []  # type: ignore[attr-defined]
        self.port = self.httpd.server_address[1]
        self.url = f"http://127.0.0.1:{self.port}"
        self._thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    @property
    def captured(self) -> list[dict]:
        return self.httpd.captured  # type: ignore[attr-defined]

    def __enter__(self) -> "EchoServer":
        self._thread.start()
        return self

    def __exit__(self, *exc) -> None:
        self.httpd.shutdown()
        self.httpd.server_close()

    def navigations(self) -> list[dict]:
        """Запросы документов (всё, кроме `/favicon.ico`)."""
        return [r for r in self.captured if r["path"] != "/favicon.ico"]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--port", type=int, default=8099)
    args = ap.parse_args()
    with EchoServer(args.port) as srv:
        print(f"эхо-сервер: {srv.url}/  (Ctrl+C — выход)", flush=True)
        try:
            threading.Event().wait()
        except KeyboardInterrupt:
            pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
