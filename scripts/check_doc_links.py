#!/usr/bin/env python3
"""Битые относительные ссылки в markdown — храповик, а не запрет.

ЗАЧЕМ. Документация Lumen — навигационное дерево: CLAUDE.md маршрутизирует в
профильные файлы, те — в подсистемы, ADR и багфайлы. Ссылка, ведущая в никуда,
не роняет ничего и потому живёт годами, а агент, пришедший по ней, либо читает
не тот файл, либо считает, что источника нет вовсе.

Аудит 2026-09-03 нашёл 237 таких ссылок. Разбирать их разом бессмысленно —
подавляющее большинство сидит в `bugs/*.md`, то есть в исторических разборах,
которые никто не перечитывает. Но класс РАСТЁТ сам: с 2026-08-31 закрытие бага
переименовывает `bugs/BUG-NNN-OPEN.md` в `-FIXED.md`, и каждая такая правка
ломает все ссылки на старое имя.

Поэтому здесь храповик того же вида, что в `check_file_sizes.py`: базовая линия
фиксирует ИЗВЕСТНЫЕ битые ссылки поимённо, а любая новая роняет проверку.
Починить старую — всегда можно и всегда бесплатно (`--update` уберёт её из
линии). Завести новую молча — нельзя.

Три класса, потому что чинятся они по-разному:

* `stale-status-suffix` — цель существует под другим статусом
  (`-OPEN.md` <-> `-FIXED.md` <-> `-DUPLICATE.md`). Почти всегда следствие
  закрытия бага без прогона `grep -rl "BUG-NNN-OPEN"`. Чинится заменой суффикса.
* `wrong-relative-path` — файл с таким именем есть, но путь посчитан от другого
  каталога (классика: ссылка из `docs/plan/x.md` написана как
  `docs/decisions/ADR-NNN.md` и резолвится в `docs/plan/docs/decisions/...`).
* `missing` — цели нет нигде; либо файл удалён, либо ссылка была написана
  на будущее.

Проверяются только относительные ссылки на файлы внутри репозитория. Внешние
URL, якоря (`#...`) и `mailto:` не трогаем: их живость этой проверкой не
установить, а сетевой запрос в гейте недопустим.

Usage:
    python scripts/check_doc_links.py             # проверка (гейт)
    python scripts/check_doc_links.py --update    # перезаписать базовую линию
    python scripts/check_doc_links.py --report    # сводка по классам, без гейта
    python scripts/check_doc_links.py --self-test # проверить логику классификации
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

# Repo root = parent of this script's directory (scripts/ -> repo root).
REPO_ROOT = Path(__file__).resolve().parent.parent

BASELINE_PATH = REPO_ROOT / "scripts" / "doc-links-baseline.tsv"

# Вендоренные корпуса и логи чужих сессий: не наш текст, ссылки в нём не наша
# забота. `bugs/` СОЗНАТЕЛЬНО не исключён — там живёт основная масса гнили, и
# храповик нужен именно чтобы она не росла.
EXCLUDED_PREFIXES = ("tests/wpt/", ".claude-manager/", ".claude/worktrees/")

# `[текст](цель)`, где цель — без пробелов; опциональный ` "title"` отбрасываем.
LINK_RE = re.compile(r"\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")

STATUS_SUFFIXES = ("-OPEN.md", "-FIXED.md", "-DUPLICATE.md")


def tracked_markdown() -> list[str]:
    """Отслеживаемые git'ом `.md`, за вычетом чужих корпусов."""
    out = subprocess.run(
        ["git", "ls-files", "*.md"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return [
        p
        for p in out.splitlines()
        if p and not p.startswith(EXCLUDED_PREFIXES)
    ]


def classify(target_rel: str, names: dict[str, list[str]]) -> tuple[str, str]:
    """Класс битой ссылки и подсказка, где цель лежит на самом деле."""
    base = Path(target_rel).name
    for suffix in STATUS_SUFFIXES:
        if not base.endswith(suffix):
            continue
        stem = base[: -len(suffix)]
        for other in STATUS_SUFFIXES:
            if other != suffix and stem + other in names:
                return "stale-status-suffix", names[stem + other][0]
    if base in names:
        return "wrong-relative-path", names[base][0]
    return "missing", ""


def collect() -> list[tuple[str, str, str, str]]:
    """Все битые ссылки: (файл, цель, класс, подсказка). Порядок стабилен."""
    docs = tracked_markdown()

    names: dict[str, list[str]] = {}
    for rel in docs:
        names.setdefault(Path(rel).name, []).append(rel)
    for paths in names.values():
        paths.sort()

    broken: set[tuple[str, str, str, str]] = set()
    for rel in docs:
        path = REPO_ROOT / rel
        try:
            text = path.read_text(encoding="utf-8", errors="replace", newline="")
        except OSError:
            continue
        for match in LINK_RE.finditer(text):
            raw = match.group(1)
            if raw.startswith(("http://", "https://", "#", "mailto:")):
                continue
            frag = raw.split("#", 1)[0].replace("\\", "/")
            if not frag:
                continue
            resolved = (path.parent / frag).resolve()
            if resolved.exists():
                continue
            try:
                target_rel = resolved.relative_to(REPO_ROOT).as_posix()
            except ValueError:
                # Ссылка уводит за пределы репозитория — считаем её целью саму
                # строку, иначе запись в базовой линии окажется непортируемой.
                target_rel = frag
            kind, hint = classify(target_rel, names)
            broken.add((rel, frag, kind, hint))

    return sorted(broken)


def read_baseline() -> set[tuple[str, str]]:
    """Известные битые ссылки как (файл, цель). Класс/подсказка справочные."""
    if not BASELINE_PATH.exists():
        return set()
    known: set[tuple[str, str]] = set()
    for line in BASELINE_PATH.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) >= 2:
            known.add((parts[0], parts[1]))
    return known


def write_baseline(broken: list[tuple[str, str, str, str]]) -> None:
    lines = [
        "# Известные битые относительные ссылки в markdown.",
        "# Перезаписывается `python scripts/check_doc_links.py --update`.",
        "# Новая ссылка, которой здесь нет, роняет гейт; починенную --update уберёт.",
        "# файл\tцель\tкласс\tгде цель на самом деле",
    ]
    lines += ["\t".join(row) for row in broken]
    BASELINE_PATH.write_text("\n".join(lines) + "\n", encoding="utf-8")


def self_test() -> int:
    """Классификация не должна зависеть от того, где лежит ссылающийся файл."""
    names = {
        "BUG-100-FIXED.md": ["bugs/BUG-100-FIXED.md"],
        "README.md": ["docs/tasks/README.md"],
    }
    cases = [
        ("bugs/BUG-100-OPEN.md", "stale-status-suffix"),
        ("docs/docs/tasks/README.md", "wrong-relative-path"),
        ("docs/nope.md", "missing"),
    ]
    failed = 0
    for target, expected in cases:
        got, _ = classify(target, names)
        if got != expected:
            print(f"FAIL {target}: ожидался {expected}, получен {got}")
            failed += 1
    print("self-test: OK" if not failed else f"self-test: {failed} провал(ов)")
    return 1 if failed else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--update", action="store_true", help="перезаписать базовую линию")
    ap.add_argument("--report", action="store_true", help="сводка по классам, без гейта")
    ap.add_argument("--self-test", action="store_true", help="проверить классификацию")
    args = ap.parse_args()

    if args.self_test:
        return self_test()

    broken = collect()

    if args.update:
        write_baseline(broken)
        print(f"Базовая линия перезаписана: {len(broken)} известных битых ссылок.")
        return 0

    if args.report:
        by_kind: dict[str, int] = {}
        for _, _, kind, _ in broken:
            by_kind[kind] = by_kind.get(kind, 0) + 1
        in_bugs = sum(1 for f, _, _, _ in broken if f.startswith("bugs/"))
        print(f"Битых ссылок: {len(broken)} (в bugs/: {in_bugs})")
        for kind in sorted(by_kind):
            print(f"  {kind}: {by_kind[kind]}")
        return 0

    known = read_baseline()
    fresh = [row for row in broken if (row[0], row[1]) not in known]

    if fresh:
        print(f"Новых битых ссылок: {len(fresh)}\n")
        for rel, target, kind, hint in fresh:
            where = f"   -> цель лежит в {hint}" if hint else ""
            print(f"  {rel}: {target}  [{kind}]{where}")
        print(
            "\nПочини ссылку либо, если она осознанно битая, внеси её в базовую линию:"
            "\n  python scripts/check_doc_links.py --update"
        )
        return 1

    healed = len(known) - (len(broken) - len(fresh))
    msg = f"Doc links OK ({len(broken)} известных битых)."
    if healed > 0:
        msg += f" Починено с прошлой линии: {healed} — прогони --update."
    print(msg)
    return 0


if __name__ == "__main__":
    sys.exit(main())
