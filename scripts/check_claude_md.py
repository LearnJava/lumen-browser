#!/usr/bin/env python3
"""Лимит размера и абсолютные пути в файлах, которые агент читает при старте.

ЗАЧЕМ. Корневой `CLAUDE.md` грузится целиком в КАЖДУЮ сессию, поэтому каждая
строка в нём оплачивается на каждой задаче. Без лимита он растёт сам: к
2026-09 дорос до 14,4 КБ (≈3,6 тыс. токенов), из которых больше половины
дублировало `docs/*` или касалось одной подсистемы. Сокращён до ~5 КБ
2026-09-24; этот скрипт не даёт ему молча вырасти обратно.

Вложенные `CLAUDE.md` (crates/js, crates/engine/paint, graphic_tests)
грузятся только при работе в своём каталоге — у них свой, меньший лимит.

Вторая проверка — абсолютные пути машины разработчика (`D:/RustProjects`,
`/c/Users/<имя>`) в CLAUDE.md, docs верхнего уровня, скиллах и агентах:
репозиторий живёт на разных машинах по разным путям, и такой путь ломает
команду скилла (так сломался merge-шаг `/lumen-task-finish`).

Usage:
    python scripts/check_claude_md.py              # гейт
    python scripts/check_claude_md.py --self-test
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

ROOT_LIMIT = 6 * 1024
NESTED_LIMIT = 2 * 1024
NESTED = ("crates/js/CLAUDE.md", "crates/engine/paint/CLAUDE.md", "graphic_tests/CLAUDE.md")

# Файлы, которые агент читает как инструкцию. Исторические журналы (bugs/,
# HEALTH-LOG, perf-аудиты, tasks/) не проверяются: путь там — факт истории.
PATH_GLOBS = (
    "CLAUDE.md", "REVIEW.md", "README.md", "docs/*.md", "docs/roles/*.md",
    ".claude/skills/*/SKILL.md", ".claude/agents/*.md", "*/CLAUDE.md", "crates/**/CLAUDE.md",
)
# ci-offload.md цитирует такой путь как описание дефекта compare.py — это факт, а не инструкция.
PATH_EXEMPT = ("docs/HEALTH-LOG.md", "docs/build-speed.md", "docs/testing-your-site-with-lumen.md", "docs/ci-offload.md")
ABS_PATH_RE = re.compile(r"(?i)(?:\b[a-z]:[\\/]RustProjects|/[a-z]/RustProjects|/c/Users/[^/$\s]+|C:\\Users\\[^\\\s]+)")


def find_abs_paths(text: str) -> list[str]:
    """Абсолютные пути машины разработчика в тексте."""
    return ABS_PATH_RE.findall(text)


def check() -> list[str]:
    """Все нарушения, по одному сообщению на каждое."""
    errors: list[str] = []
    root = REPO_ROOT / "CLAUDE.md"
    size = len(root.read_bytes())
    if size > ROOT_LIMIT:
        errors.append(f"CLAUDE.md: {size} B > {ROOT_LIMIT} B — вынеси детали в docs/ или вложенный CLAUDE.md")
    for rel in NESTED:
        p = REPO_ROOT / rel
        if p.exists() and len(p.read_bytes()) > NESTED_LIMIT:
            errors.append(f"{rel}: {len(p.read_bytes())} B > {NESTED_LIMIT} B")
    seen: set[Path] = set()
    for pattern in PATH_GLOBS:
        for p in REPO_ROOT.glob(pattern):
            rel = p.relative_to(REPO_ROOT).as_posix()
            if p in seen or rel in PATH_EXEMPT or rel.startswith((".claude/worktrees/", "tests/wpt/")):
                continue
            seen.add(p)
            with open(p, encoding="utf-8", errors="replace", newline="") as fh:
                hits = find_abs_paths(fh.read())
            for hit in hits:
                errors.append(f"{rel}: абсолютный путь машины разработчика `{hit}` — пиши путь от корня репозитория или $HOME")
    return errors


def self_test() -> int:
    """Проверка регулярки на известных примерах."""
    bad = ["git -C /d/RustProjects/lumen-browser merge", "D:\\RustProjects\\lumen-browser", "export PATH=/c/Users/konstantin/.cargo/bin"]
    good = ["export PATH=$HOME/.cargo/bin", "file:///D:/path/to/your/site", "crates/js/src/x.rs"]
    ok = all(find_abs_paths(t) for t in bad) and not any(find_abs_paths(t) for t in good)
    print("self-test OK" if ok else "self-test FAILED")
    return 0 if ok else 1


def main() -> int:
    if "--self-test" in sys.argv:
        return self_test()
    errors = check()
    for e in errors:
        print(e)
    if errors:
        return 1
    print(f"CLAUDE.md OK ({len((REPO_ROOT / 'CLAUDE.md').read_bytes())} B / {ROOT_LIMIT} B)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
