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
.delta { font-weight: 600; font-variant-numeric: tabular-nums; white-space: nowrap; }
.delta.good { color: var(--good); }
.delta.bad { color: var(--bad); }
.delta.flat { color: var(--ink-2); font-weight: 400; }
.tile .delta { display: block; font-size: 1.05rem; margin-top: 0.1rem; }
.bar-row { margin: 0.65rem 0; }
.bar-row .cap { display: flex; justify-content: space-between; font-size: 0.95em;
                margin-bottom: 0.25rem; }
.bar-row .cap .pct { font-variant-numeric: tabular-nums; }
.bar-row .cap .delta { margin-left: 0.6rem; font-size: 0.92em; }
.bar-row .track { position: relative; height: 10px; border-radius: 4px;
                  background: var(--track); }
.bar-row .fill, .bar-row .seg { position: absolute; top: 0; height: 100%; }
.bar-row .fill { left: 0; border-radius: 4px 0 0 4px; background: var(--bar);
                 min-width: 4px; }
.bar-row .seg.gain { background: var(--good); }
.bar-row .seg.loss { background: var(--bad); opacity: 0.45; }
.bar-row .prev { position: absolute; top: -4px; height: 18px; width: 2px;
                 margin-left: -1px; background: var(--ink); }
.changes { border: 1px solid var(--line); border-radius: 10px; background: var(--panel);
           padding: 0.9rem 1.4rem 1rem; margin: 0 0 1.2rem; }
.changes h2 { margin: 0 0 0.5rem; font-size: 1.15rem; }
.changes .cmp { color: var(--ink-2); font-size: 0.93em; margin: 0.2rem 0; }
.changes .warn { color: var(--bad); font-size: 0.93em; margin: 0.3rem 0 0.2rem 1.2rem;
                 padding: 0; }
.changes .cols { display: flex; gap: 2rem; flex-wrap: wrap; margin-top: 0.6rem; }
.changes .col { flex: 1 1 24rem; }
.changes h3 { margin: 0.2rem 0 0.3rem; font-size: 1rem; }
.changes h3.good { color: var(--good); }
.changes h3.bad { color: var(--bad); }
.changes ul { margin: 0; padding-left: 1.2rem; }
.changes li { margin: 0.2rem 0; }
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
    "известные ошибки мешают сильнее всего. Блок «Что изменилось» — с какими "
    "прогонами идёт сравнение, что выросло и что просело. Полосы ниже — доля "
    "пройденных тестов по крупным частям браузера: чем длиннее полоса, тем "
    "лучше эта часть работает (100% — проходит всё). Под полосами — общие итоги "
    "и их изменение к прошлому прогону (Δ, «дельта»). Кнопка «Обновить отчёт» "
    "тестов не запускает: она пересобирает отчёт из уже сохранённых прогонов, "
    "поэтому без нового прогона цифры и Δ не меняются."
)
SECTION_HELP = {
    "Источники": (
        "Откуда взяты цифры: какие прогоны тестов сравниваются. «Свежий» — "
        "последний прогон, «Предыдущий» — тот, с которым считается разница; "
        "все Δ в отчёте — это «сейчас минус тогда». «Отставание от main» — "
        "сколько изменений кода вышло после прогона. Предупреждения под "
        "таблицей говорят, когда сравнение нечестное: прогоны разных дней, "
        "разных настроек или с неглавной ветки."),
    "Просадки с прошлого отчёта": (
        "Список того, что стало хуже с прошлого отчёта: где именно тесты "
        "перестали проходить и насколько. Пустой список — ничего не сломалось. "
        "Это главное место для проверки после изменений."),
    "Разброс прогонов": (
        "Оба прогона сделаны на одном и том же коде, а цифры всё равно "
        "разошлись — из-за настроек запуска, нагрузки машины или нестабильных "
        "тестов. Это не поломки, а мера «шума»: изменения меньше этого шума "
        "в других отчётах не стоит принимать всерьёз."),
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
        "не считается пройденным, но и не новая поломка). Переход FAIL ↔ "
        "DEBTOR — смена учёта, а не починка; тесты, снятые другим способом, "
        "с прошлым прогоном не сравниваются."),
    "WPT по категориям": (
        "Самая подробная таблица: каждое направление WPT отдельно, для тех, "
        "кто ищет конкретную область. Для общего впечатления достаточно "
        "разделов выше."),
}
DASH_HELP = (
    "Тонкая тёмная метка на полосе — значение прошлого прогона; зелёный кусок "
    "между меткой и концом полосы — прирост, бледно-красный — потеря. Наведите "
    "курсор на полосу — всплывёт точное число тестов. Данные полос и плиток "
    "взяты из таблиц этого же отчёта."
)

_SUMMARY = re.compile(
    r"WPT\s+([\d.]+)%\s*(?:\(Δ\s*([^)]+)\))?\s*·\s*graphic_tests\s+(\d+)/(\d+)\s+PASS"
    r"\s*(?:\(Δ\s*([^)]+)\))?\s*·\s*открытых багов\s+(\d+)\s*(?:\(Δ\s*([^)]+)\))?"
    r"\s*·\s*просадок\s+(\d+)")
_STALE = re.compile(r"·\s*(\S+) старше соседнего источника на (\d+) дн\.")


_DELTA_PP = re.compile(r"([+-]?\d+(?:\.\d+)?)\s*п\.п\.")
_GT_RATE = re.compile(r"^Pass rate\s+[\d.]+%.*?Δ\s*([+-]?\d+(?:\.\d+)?)\s*п\.п\.")
_ISO_DAY = re.compile(r"\d{4}-\d{2}-\d{2}")
CHANGES_TOP = 5  # строк в колонках «Стало лучше» / «Стало хуже»
# Статусы WPT, рост которых — плохо (у PASS/OK — хорошо, у остальных — нейтрально).
_BAD_STATUSES = {"FAIL", "ERROR", "CRASH", "TIMEOUT"}
_GOOD_STATUSES = {"PASS", "OK"}


def _sections(text: str) -> dict[str, list[str]]:
    """Разделы `## …` отчёта → их строки без заголовка; текст до первого
    заголовка — под ключом ""."""
    out: dict[str, list[str]] = {"": []}
    cur = ""
    for line in text.splitlines():
        m = re.match(r"^##\s+(.*)$", line.strip())
        if m:
            cur = m.group(1).strip()
            out[cur] = []
        else:
            out[cur].append(line)
    return out


def _tables(lines: list[str]) -> list[list[list[str]]]:
    """Таблицы среди строк раздела: каждая — список строк из ячеек, первая —
    шапка; разделитель `|---|` выброшен."""
    tables: list[list[list[str]]] = []
    cur: list[list[str]] = []
    for line in [*lines, ""]:
        s = line.strip()
        if s.startswith("|"):
            cells = [c.strip() for c in s.strip("|").split("|")]
            if not all(re.fullmatch(r":?-{2,}:?", c) for c in cells):
                cur.append(cells)
        elif cur:
            tables.append(cur)
            cur = []
    return tables


def _num(s: str) -> float | None:
    """Ведущее число ячейки (`+5047`, `53.47%`, `-26`) или None."""
    m = re.match(r"\s*([+-]?\d+(?:\.\d+)?)", s.replace("−", "-"))
    return float(m.group(1)) if m else None


def _pp(s: str) -> float | None:
    """Значение `±N.NN п.п.` из ячейки или None."""
    m = _DELTA_PP.search(s)
    return float(m.group(1)) if m else None


def _delta_html(delta: float | None, text: str, higher_is_better: bool = True) -> str:
    """Δ со стрелкой и цветом: зелёный — к лучшему, красный — к худшему."""
    if delta is None:
        return ""
    if delta == 0:
        return f'<span class="delta flat">= {html.escape(text)}</span>'
    better = (delta > 0) == higher_is_better
    arrow = "▲" if delta > 0 else "▼"
    return (f'<span class="delta {"good" if better else "bad"}">'
            f"{arrow} {html.escape(text)}</span>")


def _bar_html(name: str, pct: float, delta: float | None, title: str) -> str:
    """Полоса: синяя часть — общее у двух прогонов, цветной кусок — прирост или
    потеря, тёмная метка — прошлое значение."""
    cur = min(max(pct, 0.0), 100.0)
    prev = None if delta is None else min(max(round(pct - delta, 2), 0.0), 100.0)
    lo = cur if prev is None else min(cur, prev)
    inner = f'<div class="fill" style="width:{lo:g}%"></div>'
    if prev is not None and prev != cur:
        kind = "gain" if cur > prev else "loss"
        inner += (f'<div class="seg {kind}" style="left:{lo:g}%;'
                  f'width:{abs(cur - prev):g}%"></div>')
    if prev is not None:
        inner += f'<div class="prev" style="left:{prev:g}%"></div>'
        title += f"; было {prev:g}%, стало {pct:g}%"
        d = _delta_html(delta, f"{delta:+g} п.п.")
    else:
        title += "; прошлого значения нет"
        d = ""
    return (f'<div class="bar-row" title="{html.escape(title)}">'
            f'<div class="cap"><span>{html.escape(name)}</span>'
            f'<span class="pct">{pct:g}%{d}</span></div>'
            f'<div class="track">{inner}</div></div>')


def parse_dashboard(text: str) -> str:
    """Собирает HTML-дашборд из строки «Итог» и таблицы «WPT по подсистемам».
    Чего-то нет в отчёте — соответствующий кусок просто не рисуется."""
    sec = _sections(text)
    bars: list[tuple[str, float, float | None, str]] = []  # (подпись, %, Δ п.п., title)
    for table in _tables(sec.get("WPT по подсистемам", [])):
        for cells in table[1:]:
            pm = re.match(r"([\d.]+)%", cells[5]) if len(cells) >= 6 else None
            if not pm:
                continue
            name = re.sub(r"[*`]", "", cells[0])
            name = re.sub(r"\s*\(.*\)$", "", name).strip()
            title = f"прогнано {cells[2]} из {cells[1]} тестов, OK/PASS {cells[3]}"
            bars.append((name, float(pm.group(1)),
                         _pp(cells[6]) if len(cells) > 6 else None, title))

    sm = _SUMMARY.search(text)
    tiles = ""
    if sm:
        wpt, wpt_d, gt_ok, gt_all, gt_d, bugs, bugs_d, regress = sm.groups()
        gt_rate = next((m for line in sec.get("graphic_tests", [])
                        if (m := _GT_RATE.match(line.strip()))), None)
        bars.append(("graphic_tests (отрисовка)",
                     round(int(gt_ok) / int(gt_all) * 100, 1),
                     float(gt_rate.group(1)) if gt_rate else None,
                     f"пройдено {gt_ok} из {gt_all}"))

        def tile(lbl: str, val: str, delta: str | None, cls: str = "",
                 higher_is_better: bool = True) -> str:
            d = (_delta_html(_num(delta), delta.strip(), higher_is_better)
                 if delta else "")
            return (f'<div class="tile"><div class="lbl">{lbl}</div>'
                    f'<div class="val {cls}">{val}</div>{d}</div>')

        stale = _STALE.search(text)
        tiles = ('<div class="tiles">'
                 + tile("WPT pass rate", f"{wpt}%", wpt_d)
                 + tile("graphic_tests пройдено", f"{gt_ok} / {gt_all}", gt_d, "good")
                 + tile("Открытых багов", bugs, bugs_d, higher_is_better=False)
                 + tile("Просадок", regress, None,
                        "bad" if int(regress) else "good")
                 + (tile(f"{html.escape(stale.group(1))} старше на", f"{stale.group(2)} дн.", None, "bad")
                    if stale else "")
                 + "</div>")

    if not bars and not tiles:
        return ""
    bar_html = "".join(_bar_html(*b) for b in bars)
    return ('<div class="dash"><h2>Доля пройденных тестов по частям браузера</h2>'
            + bar_html + tiles + "</div>"
            + f'<div class="help">{DASH_HELP}</div>')


def _anchor(heading: str) -> str:
    """id заголовка раздела — для ссылок из блока «Что изменилось»."""
    return "s-" + re.sub(r"\W+", "-", heading).strip("-")


def _short_run(cell: str) -> str:
    """Ячейка «Свежий»/«Предыдущий» таблицы «Источники» → `имя` (дата)."""
    name = re.search(r"`([^`]+)`", cell)
    day = _ISO_DAY.search(cell.split("`")[-1]) if name else None
    out = f"<code>{html.escape(name.group(1))}</code>" if name else _inline(cell)
    return out + (f" ({day.group(0)})" if day else "")


def parse_changes(text: str) -> str:
    """Блок «Что изменилось»: что с чем сравнивается, предупреждения о
    нечестном сравнении, сдвиг статусов WPT, лучший рост и первые просадки."""
    sec = _sections(text)
    if "Источники" not in sec:
        return ""
    out = ['<div class="changes"><h2>Что изменилось с прошлого прогона</h2>']

    src = _tables(sec["Источники"])
    for row in src[0][1:] if src else []:
        if len(row) >= 3:
            label = re.sub(r"\s*\(.*\)$", "", row[0]).strip()
            out.append(f'<div class="cmp">{_inline(label)}: {_short_run(row[1])} '
                       f"сравнивается с {_short_run(row[2])}</div>")
    warns = [s[2:] for s in (x.strip() for x in sec["Источники"]) if s.startswith("- ")]
    if warns:
        out.append('<ul class="warn">' + "".join(f"<li>{_inline(w)}</li>" for w in warns)
                   + "</ul>")

    st = _tables(sec.get("WPT: статусы тестов", []))
    moved = []
    for row in st[0][1:] if st else []:
        d = _num(row[3]) if len(row) >= 4 else None
        if d and not row[0].startswith("**"):
            better = None if row[0] not in _GOOD_STATUSES | _BAD_STATUSES else row[0] in _GOOD_STATUSES
            text_d = f"{row[0]} {row[2]} → {row[1]} ({row[3]})"
            moved.append(_delta_html(d, text_d, better) if better is not None
                         else f'<span class="delta flat">{html.escape(text_d)}</span>')
    if moved:
        out.append('<div class="cmp">Статусы WPT: ' + " · ".join(moved) + "</div>")

    # Рост: категории WPT с ростом pass rate, по «выигранным тестам» ≈ тестов × Δ п.п.
    gains: list[tuple[float, str]] = []
    for table in _tables(sec.get("WPT по категориям", [])):
        for row in table[1:]:
            if len(row) < 6:
                continue
            tests, rate, dpp = _num(row[1]), _num(row[4]), _pp(row[5])
            if tests and rate is not None and dpp and dpp > 0:
                gains.append((tests * dpp, f"WPT {_inline(row[0])}: pass rate "
                              f"{round(rate - dpp, 2):g} → {rate:g}% (+{dpp:g} п.п.), "
                              f"OK/PASS {html.escape(row[3])}"))
    gains.sort(key=lambda g: -g[0])
    good = [g for _, g in gains[:CHANGES_TOP]]
    for table in _tables(sec.get("graphic_tests", [])):
        head = table[0]
        if "Было" in head and "Стало" in head:
            was, now = head.index("Было"), head.index("Стало")
            good += [f"graphic_tests {_inline(r[0])}: "
                     + ("новый тест, PASS" if r[was] in ("—", "-", "") else f"{html.escape(r[was])} → PASS")
                     for r in table[1:] if len(r) > now and r[now] == "PASS" != r[was]]

    reg_lines = sec.get("Просадки с прошлого отчёта", [])
    regs = [s[2:] for s in (x.strip() for x in reg_lines) if s.startswith("- ")]
    bad = [_inline(r) for r in regs[:CHANGES_TOP]]
    if len(regs) > CHANGES_TOP:
        bad.append(f'ещё {len(regs) - CHANGES_TOP} — <a href="#{_anchor("Просадки с прошлого отчёта")}">'
                   "раздел «Просадки с прошлого отчёта»</a>")
    if not regs:
        note = next((s.strip() for s in reg_lines if s.strip()), "")
        bad = [_inline(note)] if note else []
    if "Разброс прогонов" in sec:
        bad.append('оба прогона WPT на одном коммите — их разница это шум, см. '
                   f'<a href="#{_anchor("Разброс прогонов")}">«Разброс прогонов»</a>')

    def col(title: str, cls: str, items: list[str], empty: str) -> str:
        body = ("<ul>" + "".join(f"<li>{i}</li>" for i in items) + "</ul>"
                if items else f'<div class="cmp">{empty}</div>')
        return f'<div class="col"><h3 class="{cls}">{title}</h3>{body}</div>'

    out.append('<div class="cols">'
               + col("▲ Стало лучше", "good", good, "роста нет")
               + col("▼ Стало хуже", "bad", bad, "просадок нет")
               + "</div></div>")
    return "".join(out)


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
            out.append(f'<h{level} id="{html.escape(_anchor(heading))}">'
                       f"{_inline(heading)}</h{level}>")
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
                    + parse_changes(text) + parse_dashboard(text)
                    + md_to_html(text, with_help=True))
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
