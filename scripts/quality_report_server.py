#!/usr/bin/env python3
"""HTML-страница отчёта качества с кнопкой «Обновить отчёт».

Чистый HTML-файл с `file://` не может запускать локальные скрипты, поэтому
кнопка живёт на мини-сервере: он рендерит `docs/quality-report.md` в HTML на
127.0.0.1 и по POST /refresh запускает `scripts/quality_report.py`, после чего
страница перезагружается. Относительные ссылки отчёта (`../bugs/BUG-NNN.md`,
`wpt-status.md`) открываются тем же рендером — только `.md` внутри репозитория.

  python scripts/quality_report_server.py            # http://127.0.0.1:8740/
  python scripts/quality_report_server.py --port N   # другой порт

Остановить — Ctrl+C. Сервер слушает только localhost и ничего не пишет,
кроме того, что пишет сам quality_report.py. Как читать отчёт —
docs/quality-report-howto.md.
"""
from __future__ import annotations

import argparse
import html
import re
import subprocess
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlparse

ROOT = Path(__file__).resolve().parent.parent
REPORT = ROOT / "docs" / "quality-report.md"
GENERATOR = ROOT / "scripts" / "quality_report.py"
DEFAULT_PORT = 8740
REFRESH_TIMEOUT_S = 300

CSS = """
body { font-family: system-ui, sans-serif; max-width: 72rem; margin: 0 auto;
       padding: 1rem 2rem 4rem; line-height: 1.45; color: #1a1a1a; }
table { border-collapse: collapse; margin: 0.6rem 0; }
th, td { border: 1px solid #c8c8c8; padding: 0.25rem 0.6rem; text-align: left; }
th { background: #f0f0f0; }
code { background: #f2f2f2; padding: 0.05rem 0.3rem; border-radius: 3px;
       font-size: 0.92em; }
h1 { margin-bottom: 0.2rem; }
.toolbar { display: flex; gap: 1rem; align-items: center; margin: 0.8rem 0 1.4rem; }
.toolbar button { font-size: 1rem; padding: 0.4rem 1.2rem; cursor: pointer; }
.note { color: #555; font-size: 0.9em; }
pre.err { background: #fff0f0; border: 1px solid #d99; padding: 0.8rem;
          white-space: pre-wrap; }
"""

TOOLBAR = """
<div class="toolbar">
  <form method="post" action="/refresh">
    <button type="submit">Обновить отчёт</button>
  </form>
  <span class="note">запускает <code>scripts/quality_report.py</code> и перечитывает страницу</span>
</div>
"""

_BOLD = re.compile(r"\*\*(.+?)\*\*")
_LINK = re.compile(r"\[([^\]]+)\]\(([^)\s]+)\)")


def _inline(text: str) -> str:
    """Экранирует HTML и раскрывает `code`, **bold** и [ссылки](...)."""
    out = html.escape(text, quote=False)
    out = re.sub(r"`([^`]*)`", r"<code>\1</code>", out)
    out = _BOLD.sub(r"<strong>\1</strong>", out)
    return _LINK.sub(r'<a href="\2">\1</a>', out)


def md_to_html(text: str) -> str:
    """Минимальный рендер того подмножества markdown, которое пишет
    quality_report.py: заголовки, таблицы, маркированные списки, абзацы,
    HTML-комментарии. Не общий конвертер — ровно под формат отчёта."""
    lines = text.splitlines()
    out: list[str] = []
    i = 0
    in_list = False

    def close_list() -> None:
        nonlocal in_list
        if in_list:
            out.append("</ul>")
            in_list = False

    while i < len(lines):
        line = lines[i]
        stripped = line.strip()
        if stripped.startswith("<!--"):
            while i < len(lines) and "-->" not in lines[i]:
                i += 1
            i += 1
            continue
        if not stripped:
            close_list()
            i += 1
            continue
        m = re.match(r"^(#{1,4})\s+(.*)$", stripped)
        if m:
            close_list()
            level = len(m.group(1))
            out.append(f"<h{level}>{_inline(m.group(2))}</h{level}>")
            i += 1
            continue
        if stripped.startswith("|"):
            close_list()
            rows: list[list[str]] = []
            while i < len(lines) and lines[i].strip().startswith("|"):
                cells = [c.strip() for c in lines[i].strip().strip("|").split("|")]
                if not all(re.fullmatch(r":?-{2,}:?", c) for c in cells):
                    rows.append(cells)
                i += 1
            out.append("<table>")
            for n, row in enumerate(rows):
                tag = "th" if n == 0 else "td"
                out.append("<tr>" + "".join(
                    f"<{tag}>{_inline(c)}</{tag}>" for c in row) + "</tr>")
            out.append("</table>")
            continue
        if stripped.startswith("- "):
            if not in_list:
                out.append("<ul>")
                in_list = True
            out.append(f"<li>{_inline(stripped[2:])}</li>")
            i += 1
            continue
        close_list()
        out.append(f"<p>{_inline(stripped)}</p>")
        i += 1
    close_list()
    return "\n".join(out)


def page(title: str, body: str) -> bytes:
    doc = (f"<!DOCTYPE html><html lang=\"ru\"><head><meta charset=\"utf-8\">"
           f"<title>{html.escape(title)}</title><style>{CSS}</style></head>"
           f"<body>{body}</body></html>")
    return doc.encode("utf-8")


def resolve_md(url_path: str) -> Path | None:
    """URL-путь → .md-файл внутри репозитория; иначе None.
    Отчёт лежит в docs/, его относительные ссылки разрешаются от docs/."""
    rel = unquote(url_path).lstrip("/")
    if not rel.endswith(".md"):
        return None
    candidate = (ROOT / "docs" / rel).resolve()
    if not candidate.is_file():
        candidate = (ROOT / rel).resolve()
    if candidate.is_file() and candidate.is_relative_to(ROOT):
        return candidate
    return None


class Handler(BaseHTTPRequestHandler):
    def _send(self, code: int, body: bytes) -> None:
        self.send_response(code)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:  # noqa: N802 — имя задаёт BaseHTTPRequestHandler
        path = urlparse(self.path).path
        if path in ("/", "/index.html"):
            if not REPORT.is_file():
                self._send(200, page("Отчёт качества", TOOLBAR +
                           "<p>Отчёта ещё нет — нажмите «Обновить отчёт».</p>"))
                return
            body = TOOLBAR + md_to_html(REPORT.read_text(encoding="utf-8"))
            self._send(200, page("Отчёт качества", body))
            return
        target = resolve_md(path)
        if target is None:
            self._send(404, page("404", "<p>Отдаются только .md внутри репозитория. "
                                        "<a href=\"/\">К отчёту</a></p>"))
            return
        body = ("<p><a href=\"/\">← к отчёту</a></p>"
                + md_to_html(target.read_text(encoding="utf-8")))
        self._send(200, page(target.name, body))

    def do_POST(self) -> None:  # noqa: N802 — имя задаёт BaseHTTPRequestHandler
        if urlparse(self.path).path != "/refresh":
            self._send(404, page("404", "<p><a href=\"/\">К отчёту</a></p>"))
            return
        try:
            run = subprocess.run(
                [sys.executable, str(GENERATOR)], cwd=ROOT,
                capture_output=True, timeout=REFRESH_TIMEOUT_S)
        except subprocess.TimeoutExpired:
            self._send(500, page("Ошибка", TOOLBAR +
                       f"<pre class=\"err\">quality_report.py не уложился в "
                       f"{REFRESH_TIMEOUT_S} с</pre>"))
            return
        if run.returncode != 0:
            err = html.escape((run.stderr or run.stdout).decode("utf-8", "replace"))
            self._send(500, page("Ошибка", TOOLBAR +
                       f"<pre class=\"err\">quality_report.py завершился с кодом "
                       f"{run.returncode}\n{err}</pre>"))
            return
        self.send_response(303)
        self.send_header("Location", "/")
        self.end_headers()

    def log_message(self, fmt: str, *args: object) -> None:
        sys.stderr.write("%s - %s\n" % (self.address_string(), fmt % args))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    args = parser.parse_args()
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"Отчёт качества: http://127.0.0.1:{args.port}/  (Ctrl+C — остановить)")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
