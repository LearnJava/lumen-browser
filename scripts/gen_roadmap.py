#!/usr/bin/env python3
"""Генератор данных для roadmap-деревьев (docs/roadmap-*.html).

Источник правды:
  - docs/roadmap.json  — курируемая структура фаз/задач + ручные связи баг→задача (поле "bugs").
  - BUGS.md            — живой статус/заголовок/компонент каждого бага (парсится автоматически).

Что делает:
  1. Читает roadmap.json (структура) и BUGS.md (актуальные баги).
  2. Сшивает: каждой задаче с полем "bugs" подмешивает живой статус из BUGS.md.
  3. ВЫВОДИТ статус задачи автоматически (см. derive_status): из живых багов и подзадач.
     Ручной "status" в roadmap.json — лишь запасной вариант для фич без багов и без подзадач.
  4. Баги без ручной привязки → группа "Прочие баги (по компоненту)".
  5. Вшивает итоговый JSON в <script id="roadmap-data"> обоих HTML-файлов.

Почему авто-вывод: раньше статус задачи копировался из roadmap.json дословно, поэтому
после закрытия бага в BUGS.md задача оставалась "blocker"/"ready" (дрейф: зелёный баг под
красной задачей). Теперь статус задачи производный — править руками нужно только статусы
чисто-фичевых задач без багов/подзадач (planned-фичи).

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
    if not ROADMAP_JSON.exists():
        sys.exit(f"нет {ROADMAP_JSON}")
    if not BUGS_MD.exists():
        sys.exit(f"нет {BUGS_MD}")

    roadmap = json.loads(ROADMAP_JSON.read_text(encoding="utf-8"))
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
