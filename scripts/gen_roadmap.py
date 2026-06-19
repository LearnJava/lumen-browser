#!/usr/bin/env python3
"""Генератор данных для roadmap-деревьев (docs/roadmap-*.html).

Источник правды:
  - docs/roadmap.json  — курируемая структура фаз/задач + ручные связи баг→задача (поле "bugs").
  - BUGS.md            — живой статус/заголовок/компонент каждого бага (парсится автоматически).

Что делает:
  1. Читает roadmap.json (структура) и BUGS.md (актуальные баги).
  2. Сшивает: каждой задаче с полем "bugs" подмешивает живой статус из BUGS.md.
  3. Баги без ручной привязки → группа "Прочие баги (по компоненту)".
  4. Вшивает итоговый JSON в <script id="roadmap-data"> обоих HTML-файлов.

Запуск (из корня репозитория):
  python scripts/gen_roadmap.py

Обновляй после правки roadmap.json ИЛИ при добавлении/закрытии багов в BUGS.md.
"""
import json
import re
import sys
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ROADMAP_JSON = ROOT / "docs" / "roadmap.json"
BUGS_MD = ROOT / "BUGS.md"
HTML_FILES = [
    ROOT / "docs" / "roadmap-B-twotrees.html",
    ROOT / "docs" / "roadmap-svg-cleaves.html",
]

# Строка таблицы BUGS.md: | [BUG-NNN](bugs/...) | СТАТУS | компонент | описание | [доп. колонка] |
BUG_ROW = re.compile(r"^\|\s*\[(BUG-\d+)\]\([^)]*\)\s*\|(.+)$")
MAX_DESC = 160


def parse_status(raw):
    """Сырой статус → (категория, дата|None)."""
    raw = raw.strip()
    up = raw.upper()
    if up.startswith("OPEN"):
        return "open", None
    if up.startswith("IN PROGRESS"):
        return "inprogress", None
    if up.startswith("WONTFIX"):
        return "wontfix", None
    if up.startswith("FIXED"):
        m = re.search(r"(\d{4}-\d{2}-\d{2})", raw)
        return "fixed", (m.group(1) if m else None)
    return "open", None


def parse_bugs():
    bugs = {}
    for line in BUGS_MD.read_text(encoding="utf-8").splitlines():
        m = BUG_ROW.match(line)
        if not m:
            continue
        bug_id = m.group(1)
        cols = [c.strip() for c in m.group(2).split("|")]
        # cols: [статус, компонент, описание, (возможна доп. колонка file:line)]
        status_raw = cols[0] if len(cols) > 0 else ""
        component = cols[1] if len(cols) > 1 else "?"
        desc = cols[2] if len(cols) > 2 else ""
        desc = re.sub(r"\s+", " ", desc).strip()
        if len(desc) > MAX_DESC:
            desc = desc[:MAX_DESC].rstrip() + "…"
        status, fixed_date = parse_status(status_raw)
        bugs[bug_id] = {
            "status": status,
            "title": desc,
            "component": component,
            "date": fixed_date,
        }
    return bugs


def collect_linked(tasks, acc):
    for t in tasks:
        for b in t.get("bugs", []):
            acc.add(b)
        collect_linked(t.get("tasks", []), acc)


def main():
    if not ROADMAP_JSON.exists():
        sys.exit(f"нет {ROADMAP_JSON}")
    if not BUGS_MD.exists():
        sys.exit(f"нет {BUGS_MD}")

    roadmap = json.loads(ROADMAP_JSON.read_text(encoding="utf-8"))
    bugs = parse_bugs()

    # связанные баги
    linked = set()
    for ph in roadmap["phases"]:
        collect_linked(ph.get("tasks", []), linked)

    # проверка: ручные связи на несуществующие баги
    missing = sorted(b for b in linked if b not in bugs)
    if missing:
        print(f"ВНИМАНИЕ: в roadmap.json есть связи на отсутствующие в BUGS.md баги: {', '.join(missing)}")

    # прочие баги по компоненту (только реально существующие, не привязанные)
    unlinked = {}
    for bug_id, info in bugs.items():
        if bug_id in linked:
            continue
        comp = info["component"]
        unlinked.setdefault(comp, []).append(bug_id)
    for comp in unlinked:
        unlinked[comp].sort()

    counts = {"open": 0, "fixed": 0, "inprogress": 0, "wontfix": 0}
    for info in bugs.values():
        counts[info["status"]] = counts.get(info["status"], 0) + 1

    data = {
        "generated": date.today().isoformat(),
        "counts": counts,
        "total_bugs": len(bugs),
        "phases": roadmap["phases"],
        "bugs": bugs,
        "unlinked": dict(sorted(unlinked.items())),
    }
    payload = json.dumps(data, ensure_ascii=False, indent=1)

    block = re.compile(
        r'(<script id="roadmap-data" type="application/json">)(.*?)(</script>)',
        re.DOTALL,
    )
    for html in HTML_FILES:
        if not html.exists():
            print(f"пропуск (нет файла): {html.name}")
            continue
        src = html.read_text(encoding="utf-8")
        if not block.search(src):
            print(f"пропуск (нет блока roadmap-data): {html.name}")
            continue
        new = block.sub(lambda mm: mm.group(1) + "\n" + payload + "\n" + mm.group(3), src)
        html.write_text(new, encoding="utf-8")
        print(f"обновлён: {html.name}")

    print(
        f"Готово. Багов: {len(bugs)} (open {counts['open']}, fixed {counts['fixed']}), "
        f"связано вручную: {len(linked)}, прочих по компонентам: {sum(len(v) for v in unlinked.values())}."
    )


if __name__ == "__main__":
    main()
