#!/usr/bin/env python3
"""HTML-страница отчёта качества с кнопкой «Обновить отчёт».

Чистый HTML-файл с `file://` не может запускать локальные скрипты, поэтому
кнопка живёт на мини-сервере: он рендерит `docs/quality-report.md` в HTML на
127.0.0.1 и по POST /refresh запускает `scripts/quality_report.py`, после чего
страница перезагружается. Относительные ссылки отчёта (`../bugs/BUG-NNN.md`,
`wpt-status.md`) открываются тем же рендером — только `.md` внутри репозитория.

  python scripts/quality_report_server.py            # http://127.0.0.1:8740/ + браузер
  python scripts/quality_report_server.py --port N   # другой порт
  python scripts/quality_report_server.py --no-open  # не открывать браузер

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
import threading
import webbrowser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlparse

ROOT = Path(__file__).resolve().parent.parent
REPORT = ROOT / "docs" / "quality-report.md"
GENERATOR = ROOT / "scripts" / "quality_report.py"
DEFAULT_PORT = 8740
REFRESH_TIMEOUT_S = 300

CSS = """
:root { --surface: #fcfcfb; --panel: #f9f9f7; --ink: #0b0b0b; --ink-2: #52514e;
        --line: #c8c8c8; --track: #e8e7e3; --bar: #2a78d6; --good: #0ca30c;
        --bad: #d03b3b; --help-bg: #f3f6fa; }
@media (prefers-color-scheme: dark) {
  :root { --surface: #1a1a19; --panel: #0d0d0d; --ink: #f0efec; --ink-2: #c3c2b7;
          --line: #44443f; --track: #383835; --bar: #3987e5; --good: #0ca30c;
          --bad: #e66767; --help-bg: #22262b; }
}
body { font-family: system-ui, sans-serif; max-width: 72rem; margin: 0 auto;
       padding: 1rem 2rem 4rem; line-height: 1.45; color: var(--ink);
       background: var(--surface); }
a { color: var(--bar); }
table { border-collapse: collapse; margin: 0.6rem 0; }
th, td { border: 1px solid var(--line); padding: 0.25rem 0.6rem; text-align: left; }
th { background: var(--panel); }
code { background: var(--track); padding: 0.05rem 0.3rem; border-radius: 3px;
       font-size: 0.92em; }
h1 { margin-bottom: 0.2rem; }
.toolbar { display: flex; gap: 1rem; align-items: center; margin: 0.8rem 0 1.4rem; }
.toolbar button { font-size: 1rem; padding: 0.4rem 1.2rem; cursor: pointer; }
.note { color: var(--ink-2); font-size: 0.9em; }
pre.err { background: #fff0f0; border: 1px solid #d99; padding: 0.8rem;
          white-space: pre-wrap; color: #1a1a1a; }
.help { background: var(--help-bg); border-radius: 6px; padding: 0.5rem 0.9rem;
        margin: 0.4rem 0 0.8rem; color: var(--ink-2); font-size: 0.93em; }
.dash { border: 1px solid var(--line); border-radius: 10px; background: var(--panel);
        padding: 1rem 1.4rem 1.2rem; margin: 0 0 1.6rem; }
.dash h2 { margin: 0 0 1rem; font-size: 1.15rem; }
.tiles { display: flex; gap: 2.5rem; flex-wrap: wrap; border-top: 1px solid var(--line);
         margin-top: 1.2rem; padding-top: 0.8rem; }
.tile .lbl { color: var(--ink-2); font-size: 0.9em; }
.tile .val { font-size: 1.5rem; font-weight: 600; }
.tile .val.good { color: var(--good); }
.tile .val.bad { color: var(--bad); }
.tile .d { font-size: 0.85rem; color: var(--ink-2); margin-left: 0.3rem;
           font-weight: 400; }
.bar-row { margin: 0.65rem 0; }
.bar-row .cap { display: flex; justify-content: space-between; font-size: 0.95em;
                margin-bottom: 0.25rem; }
.bar-row .cap .pct { font-variant-numeric: tabular-nums; }
.bar-row .track { height: 10px; border-radius: 4px; background: var(--track); }
.bar-row .fill { height: 100%; border-radius: 4px; background: var(--bar);
                 min-width: 4px; }
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

# Пояснения для неспециалиста — вставляются под соответствующим заголовком `##`.
INTRO_HELP = (
    "Этот отчёт отвечает на вопрос «стал ли браузер лучше со времени прошлого "
    "прогона тестов». Браузер прогоняется через тысячи автоматических тестов; "
    "здесь сведено, сколько из них проходит, где стало лучше или хуже и какие "
    "известные ошибки мешают сильнее всего. Полосы ниже — доля пройденных "
    "тестов по крупным частям браузера: чем длиннее полоса, тем лучше эта часть "
    "работает (100% — проходит всё). Под полосами — общие итоги и их изменение "
    "к прошлому прогону (Δ, «дельта»)."
)
SECTION_HELP = {
    "Источники": (
        "Откуда взяты цифры: какие прогоны тестов сравниваются. «Свежий» — "
        "последний прогон, «Предыдущий» — тот, с которым считается разница; "
        "все Δ в отчёте — это «сейчас минус тогда»."),
    "Просадки с прошлого отчёта": (
        "Список того, что стало хуже с прошлого отчёта: где именно тесты "
        "перестали проходить и насколько. Пустой список — ничего не сломалось. "
        "Это главное место для проверки после изменений."),
    "WPT: статусы тестов": (
        "WPT (Web Platform Tests) — общий открытый набор тестов, которым "
        "проверяют все браузеры, поэтому по нему можно сравниваться с Chrome и "
        "Firefox. В таблице — сколько тестов закончилось каким исходом: PASS и "
        "OK — тест прошёл; FAIL — браузер дал неверный результат; ERROR, CRASH "
        "и TIMEOUT — тест упал или завис; UNSUPPORTED, SKIPPED и NOT-RUN — не "
        "запускались и в процент успеха не входят."),
    "WPT по подсистемам": (
        "Те же тесты, сгруппированные по крупным частям браузера: разбор "
        "HTML-страниц, работа с деревом документа (DOM), раскладка и стили "
        "(CSS), сеть, отрисовка. «Pass rate» — доля пройденного, Δ — изменение "
        "с прошлого прогона. Эти же доли нарисованы полосами вверху страницы."),
    "Топ-5 багов по числу заблокированных WPT-тестов": (
        "Известные ошибки, из-за которых не проходит больше всего тестов. "
        "Починка верхних строк даст наибольший прирост процента — это "
        "подсказка, за что браться в первую очередь."),
    "Открытые баги по подсистемам": (
        "Сколько известных ошибок сейчас открыто и в какой части браузера. "
        "Δ — на сколько изменилось число с прошлого среза: минус — ошибок "
        "стало меньше."),
    "graphic_tests": (
        "Собственные тесты отрисовки Lumen: браузер рисует страницу, и "
        "картинка сравнивается с эталоном попиксельно. PASS — совпало; FAIL — "
        "разошлось; DEBTOR — известное расхождение, записанное «в долг» (оно "
        "не считается пройденным, но и не новая поломка)."),
    "WPT по категориям": (
        "Самая подробная таблица: каждое направление WPT отдельно, для тех, "
        "кто ищет конкретную область. Для общего впечатления достаточно "
        "разделов выше."),
}
DASH_HELP = (
    "Наведите курсор на полосу — всплывёт точное число тестов. Данные полос и "
    "плиток взяты из таблиц этого же отчёта."
)

_SUMMARY = re.compile(
    r"WPT\s+([\d.]+)%\s*(?:\(Δ\s*([^)]+)\))?\s*·\s*graphic_tests\s+(\d+)/(\d+)\s+PASS"
    r"\s*(?:\(Δ\s*([^)]+)\))?\s*·\s*открытых багов\s+(\d+)\s*(?:\(Δ\s*([^)]+)\))?"
    r"\s*·\s*просадок\s+(\d+)")


def parse_dashboard(text: str) -> str:
    """Собирает HTML-дашборд из строки «Итог» и таблицы «WPT по подсистемам».
    Чего-то нет в отчёте — соответствующий кусок просто не рисуется."""
    bars: list[tuple[str, float, str]] = []  # (подпись, %, title для hover)
    section = None
    for line in text.splitlines():
        m = re.match(r"^##\s+(.*)$", line.strip())
        if m:
            section = m.group(1)
            continue
        if section == "WPT по подсистемам" and line.strip().startswith("|"):
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if len(cells) < 6 or cells[0] in ("Подсистема",) or set(cells[1]) <= {"-", ":"}:
                continue
            pm = re.match(r"([\d.]+)%", cells[5])
            if not pm:
                continue
            name = re.sub(r"[*`]", "", cells[0])
            name = re.sub(r"\s*\(.*\)$", "", name).strip()
            title = f"прогнано {cells[2]} из {cells[1]} тестов, OK/PASS {cells[3]}, Δ {cells[6] if len(cells) > 6 else NA}"
            bars.append((name, float(pm.group(1)), title))

    sm = _SUMMARY.search(text)
    tiles = ""
    if sm:
        wpt, wpt_d, gt_ok, gt_all, gt_d, bugs, bugs_d, regress = sm.groups()
        bars.append(("graphic_tests (отрисовка)",
                     round(int(gt_ok) / int(gt_all) * 100, 1),
                     f"пройдено {gt_ok} из {gt_all}"))

        def tile(lbl: str, val: str, delta: str | None, cls: str = "") -> str:
            d = f'<span class="d">Δ {html.escape(delta)}</span>' if delta else ""
            return (f'<div class="tile"><div class="lbl">{lbl}</div>'
                    f'<div class="val {cls}">{val}{d}</div></div>')

        tiles = ('<div class="tiles">'
                 + tile("WPT pass rate", f"{wpt}%", wpt_d)
                 + tile("graphic_tests пройдено", f"{gt_ok} / {gt_all}", gt_d, "good")
                 + tile("Открытых багов", bugs, bugs_d)
                 + tile("Просадок", regress, None,
                        "bad" if int(regress) else "good")
                 + "</div>")

    if not bars and not tiles:
        return ""
    bar_html = "".join(
        f'<div class="bar-row" title="{html.escape(t)}">'
        f'<div class="cap"><span>{html.escape(n)}</span>'
        f'<span class="pct">{p:g}%</span></div>'
        f'<div class="track"><div class="fill" style="width:{min(p, 100):g}%">'
        f"</div></div></div>"
        for n, p, t in bars)
    return ('<div class="dash"><h2>Доля пройденных тестов по частям браузера</h2>'
            + bar_html + tiles + "</div>"
            + f'<div class="help">{DASH_HELP}</div>')


def _inline(text: str) -> str:
    """Экранирует HTML и раскрывает `code`, **bold** и [ссылки](...)."""
    out = html.escape(text, quote=False)
    out = re.sub(r"`([^`]*)`", r"<code>\1</code>", out)
    out = _BOLD.sub(r"<strong>\1</strong>", out)
    return _LINK.sub(r'<a href="\2">\1</a>', out)


def md_to_html(text: str, with_help: bool = False) -> str:
    """Минимальный рендер того подмножества markdown, которое пишет
    quality_report.py: заголовки, таблицы, маркированные списки, абзацы,
    HTML-комментарии. Не общий конвертер — ровно под формат отчёта.
    with_help — вставлять пояснения SECTION_HELP под знакомыми заголовками."""
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
            heading = m.group(2)
            out.append(f"<h{level}>{_inline(heading)}</h{level}>")
            if with_help and heading in SECTION_HELP:
                out.append(f'<div class="help">{SECTION_HELP[heading]}</div>')
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
            text = REPORT.read_text(encoding="utf-8")
            body = (TOOLBAR + f'<div class="help">{INTRO_HELP}</div>'
                    + parse_dashboard(text) + md_to_html(text, with_help=True))
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
    parser.add_argument("--no-open", action="store_true",
                        help="не открывать страницу в браузере по умолчанию")
    args = parser.parse_args()
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    url = f"http://127.0.0.1:{args.port}/"
    print(f"Отчёт качества: {url}  (Ctrl+C — остановить)")
    if not args.no_open:
        # Открываем после старта serve_forever, иначе браузер может успеть
        # постучаться до того, как сервер начнёт принимать соединения.
        threading.Timer(0.3, webbrowser.open, (url,)).start()
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
