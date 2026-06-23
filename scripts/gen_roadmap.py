#!/usr/bin/env python3
"""Генератор данных для roadmap-деревьев (docs/roadmap-*.html).

Источник правды:
  - ROADMAP.md  — плоская (одна задача = одна строка) структура фаз/задач + связи баг→задача
                  (колонка "bugs"). Grep-friendly: `grep "| U-6 " ROADMAP.md`.
  - BUGS.md     — живой статус/заголовок/компонент каждого бага (парсится автоматически).

Что делает:
  1. Читает ROADMAP.md (структура: таблицы «Фазы» и «Задачи») и BUGS.md (актуальные баги).
  2. Собирает дерево из плоских строк по колонкам phase/parent.
  3. Сшивает: каждой задаче с непустой колонкой "bugs" подмешивает живой статус из BUGS.md.
  4. ВЫВОДИТ статус задачи автоматически (см. derive_status): из живых багов и подзадач.
     Ручной "status" в ROADMAP.md — лишь запасной вариант для фич без багов и без подзадач.
  5. Баги без ручной привязки → группа "Прочие баги (по компоненту)".
  6. Вшивает итоговый JSON в <script id="roadmap-data"> обоих HTML-файлов.

Почему авто-вывод: раньше статус задачи копировался дословно, поэтому после закрытия бага в
BUGS.md задача оставалась "blocker"/"ready" (дрейф: зелёный баг под красной задачей). Теперь
статус задачи производный — править руками нужно только статусы чисто-фичевых задач без
багов/подзадач (planned-фичи).

Почему ROADMAP.md, а не roadmap.json: вложенный json нельзя грепнуть по одной записи (задача
размазана по дереву отступов). Плоский markdown — одна строка на задачу, размер файла
нерелевантен, читается тем же приёмом, что BUGS.md.

Запуск (из корня репозитория):
  python scripts/gen_roadmap.py

Обновляй после правки ROADMAP.md ИЛИ при добавлении/закрытии багов в BUGS.md.
"""
import json
import re
import sys
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ROADMAP_MD = ROOT / "ROADMAP.md"
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


def _table_rows(lines, header_contains):
    """Возвращает строки markdown-таблицы (списки ячеек) под заголовком, содержащим header_contains.

    Ищет строку-шапку таблицы (`| col | col |`), где встречается header_contains, пропускает
    разделитель (`|---|`) и собирает строки данных до первой не-`|`-строки.
    """
    rows = []
    i = 0
    n = len(lines)
    while i < n:
        line = lines[i].strip()
        if line.startswith("|") and header_contains in line:
            i += 2  # шапка + разделитель |---|
            while i < n and lines[i].strip().startswith("|"):
                cells = [c.strip() for c in lines[i].strip().strip("|").split("|")]
                rows.append(cells)
                i += 1
            return rows
        i += 1
    return rows


def parse_roadmap():
    """Читает ROADMAP.md → {"phases": [...]} в том же виде, что прежний roadmap.json.

    Две таблицы: «Фазы» (id|status|date|title) и «Задачи»
    (id|phase|parent|status|size|bugs|note|title). Дерево собирается по колонкам phase/parent
    с сохранением порядка строк.
    """
    lines = ROADMAP_MD.read_text(encoding="utf-8").splitlines()

    phase_rows = _table_rows(lines, "title |")  # первая таблица с колонкой title — «Фазы»
    # Шапка фаз: id | status | date | title (4 колонки). Шапка задач: 8 колонок.
    phases = []
    phase_by_id = {}
    for cells in phase_rows:
        if len(cells) != 4:
            continue
        pid, status, dt, title = cells
        node = {"id": pid, "title": title, "status": status or "planned", "tasks": []}
        if dt:
            node["date"] = dt
        phases.append(node)
        phase_by_id[pid] = node

    # Таблица задач: ищем шапку с колонкой parent.
    task_rows = _table_rows(lines, "parent |")
    task_by_id = {}
    order = []
    for cells in task_rows:
        if len(cells) != 8:
            continue
        tid, phase, parent, status, size, bugs, note, title = cells
        node = {"id": tid, "title": title, "status": status or "planned"}
        if size:
            node["size"] = size
        if note:
            node["note"] = note
        if bugs:
            node["bugs"] = [b.strip() for b in bugs.split(",") if b.strip()]
        node["_phase"] = phase
        node["_parent"] = parent
        node["tasks"] = []
        task_by_id[tid] = node
        order.append(tid)

    # Сшивка дерева: parent пуст → под фазой; иначе → под задачей-родителем.
    for tid in order:
        node = task_by_id[tid]
        parent = node.pop("_parent")
        phase = node.pop("_phase")
        if parent and parent in task_by_id:
            task_by_id[parent]["tasks"].append(node)
        elif phase in phase_by_id:
            phase_by_id[phase]["tasks"].append(node)
        else:
            print(f"ВНИМАНИЕ: задача {tid} ссылается на неизвестные phase={phase!r}/parent={parent!r}")

    # Уберём пустые "tasks", чтобы JSON совпадал с прежней формой (лист без подзадач не имеет ключа).
    def _strip_empty(node):
        for t in node.get("tasks", []):
            _strip_empty(t)
        if not node.get("tasks"):
            node.pop("tasks", None)

    for ph in phases:
        for t in ph["tasks"]:
            _strip_empty(t)

    return {"phases": phases}


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
            # «должник» (KNOWN_DEBTORS): остаток-отклонение запаркован, задача считается
            # закрытой, хотя баг формально OPEN. Метку ставим, если "DEBTOR" встречается
            # где угодно в строке (в статусе «OPEN (DEBTOR)» или в тексте «Остаток DEBTOR»).
            "debtor": "DEBTOR" in line.upper(),
        }
    return bugs


def collect_linked(tasks, acc):
    for t in tasks:
        for b in t.get("bugs", []):
            acc.add(b)
        collect_linked(t.get("tasks", []), acc)


# Статусы, означающие «работа идёт / начата» — по любому из них родитель = active.
ACTIVE_ISH = {"active", "inprogress", "blocker", "wait"}


def bug_signal(bug_ids, bugs):
    """Сводный сигнал от связанных багов: 'done' | 'active' | 'open' | None.

    'done'   — все баги закрыты (FIXED) или запаркованы как должники (DEBTOR);
    'active' — есть баг IN PROGRESS;
    'open'   — остались реально открытые баги (нет завершающего сигнала);
    None     — у задачи нет привязанных багов (или они отсутствуют в BUGS.md).
    """
    live = [bugs[b] for b in bug_ids if b in bugs]
    if not live:
        return None
    if all(i["status"] == "fixed" or i.get("debtor") for i in live):
        return "done"
    if any(i["status"] == "inprogress" for i in live):
        return "active"
    return "open"


def derive_status(node, bugs, warnings, infer_active=True):
    """Вычисляет эффективный статус задачи/фазы и вшивает его обратно в node["status"].

    Приоритет: подзадачи + связанные баги. Если завершающих/активных сигналов нет —
    возвращается курируемый (ручной) статус из roadmap.json (для planned-фич без багов).

    infer_active=False (для фаз): не повышаем planned→active по частичному прогрессу,
    только авто-помечаем фазу 'done', когда ВСЕ её задачи готовы — веховую семантику не трогаем.
    """
    manual = node.get("status", "planned")
    child_eff = [derive_status(c, bugs, warnings, infer_active=True) for c in node.get("tasks", [])]
    bsig = bug_signal(node.get("bugs", []), bugs)

    evidence = child_eff + ([bsig] if bsig is not None else [])

    if not evidence:
        eff = manual
    elif all(e == "done" for e in evidence):
        eff = "done"
    elif infer_active and any(e == "done" or e in ACTIVE_ISH for e in evidence):
        eff = "active"
    else:
        eff = manual  # только open/planned/ready/opt без завершения → сохраняем ручной нюанс

    if manual == "done" and bsig == "open":
        warnings.append(f"{node.get('id', '?')} помечен 'done', но под ним есть открытые баги")

    node["status"] = eff
    return eff


def main():
    if not ROADMAP_MD.exists():
        sys.exit(f"нет {ROADMAP_MD}")
    if not BUGS_MD.exists():
        sys.exit(f"нет {BUGS_MD}")

    roadmap = parse_roadmap()
    bugs = parse_bugs()

    # авто-вывод статусов задач/фаз из живых багов + подзадач (правит roadmap["phases"] на месте)
    status_warnings = []
    for ph in roadmap["phases"]:
        derive_status(ph, bugs, status_warnings, infer_active=False)
    for w in status_warnings:
        print(f"ВНИМАНИЕ: {w}")

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
