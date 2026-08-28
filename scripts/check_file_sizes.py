#!/usr/bin/env python3
"""Правило размера файла (дорожка SPLIT, docs/lint-policy.md §5.1).

Два разных правила в одной проверке — потому что долг и профилактика требуют
разного обращения:

1. **Потолок для нового файла.** Любой `.rs`, которого нет в базовой линии,
   обязан быть не длиннее CAP строк. Новый монолит завести нельзя вообще:
   код, который не помещается, кладётся в отдельный модуль рядом.

2. **Храповик для уже существующих.** Файл из базовой линии не должен расти.
   Гиганты режутся дорожкой SPLIT, и каждая правка, добавляющая им строк,
   отодвигает разрез — ровно это и случилось между аудитом 2026-08-23 и
   назначением дорожки 2026-08-26 (`box_tree.rs` +919, `network/lib.rs` +418,
   при том что план прямо запрещал заводить туда фичи).

Храповик — не запрет, а видимость: если рост действительно нужен, автор
запускает `--update` и число в `scripts/file-size-baseline.tsv` едет вверх
в том же коммите, то есть попадает в диф и в ревью. Молча вырасти нельзя.

Файл, упавший до CAP, из базовой линии удаляется (`--update` делает это сам) —
после этого он живёт по правилу №1 и обратно вырасти уже не может.

Usage:
    python scripts/check_file_sizes.py              # проверка (гейт CI)
    python scripts/check_file_sizes.py --update     # перезаписать базовую линию
    python scripts/check_file_sizes.py --top 20     # показать крупнейшие файлы
    python scripts/check_file_sizes.py --self-test  # проверить логику сравнения
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

# Repo root = parent of this script's directory (scripts/ -> repo root).
REPO_ROOT = Path(__file__).resolve().parent.parent

BASELINE_PATH = REPO_ROOT / "scripts" / "file-size-baseline.tsv"

# Потолок для файла, которого нет в базовой линии. Совпадает с целью дорожки
# SPLIT (docs/tasks/p1-monolith-split-queue.md §1): гиганты сводятся к набору
# файлов не длиннее этого.
CAP = 2000

# Каталоги вне рабочей границы проекта: вендоренный код и тестовые корпуса
# правилу не подчиняются (их не мы пишем и не нам резать). Сейчас `.rs` там нет
# вовсе — префиксы стоят на будущее, чтобы вендоринг не уронил гейт.
# `fuzz/` и `workspace-hack/` НЕ исключены: это наш код, и правило на него
# распространяется наравне с `crates/`.
EXCLUDED_PREFIXES = ("tests/", "tools/")

# Файлы, которые дорожка SPLIT явно постановила НЕ резать (§1 плана):
# таблицы и когерентные алгоритмы, где монолитность безвредна, а дробление
# только рассыпает одну сущность по файлам. Они не подчиняются ни потолку,
# ни храповику — таблица законно растёт, когда спецификация добавляет строк.
EXEMPT = frozenset(
    {
        "crates/engine/html-parser/src/entities.rs",
        "crates/engine/layout/src/counters.rs",
        "crates/engine/layout/src/incremental.rs",
        "crates/engine/layout/src/animation.rs",
        "crates/engine/paint/src/svg_path.rs",
    }
)


def count_lines(path: Path) -> int:
    """Число строк по семантике `wc -l` — то есть число переводов строки.

    Читаем байтами: `.rs` в этом репозитории UTF-8, но декодирование здесь
    ничего не даёт, а на файле со случайным не-UTF-8 байтом уронило бы гейт.
    """
    data = path.read_bytes()
    return data.count(b"\n")


def tracked_rust_files() -> list[str]:
    """Пути всех `.rs` под контролем git, в posix-форме, отсортированные.

    `git ls-files` вместо обхода каталогов: он сам исключает `target/`
    и всё прочее из `.gitignore`, где лежат сотни сгенерированных файлов.
    """
    out = subprocess.run(
        ["git", "ls-files", "-z", "*.rs"],
        cwd=REPO_ROOT,
        capture_output=True,
        check=True,
    ).stdout
    paths = [p for p in out.decode("utf-8").split("\0") if p]
    return sorted(p for p in paths if not p.startswith(EXCLUDED_PREFIXES))


def measure() -> dict[str, int]:
    """Текущий размер каждого файла, подпадающего под правило."""
    return {
        rel: count_lines(REPO_ROOT / rel)
        for rel in tracked_rust_files()
        if rel not in EXEMPT
    }


def read_baseline() -> dict[str, int]:
    """Базовая линия: `<строк>\\t<путь>`, комментарии `#` и пустые строки — мимо."""
    if not BASELINE_PATH.exists():
        return {}
    baseline: dict[str, int] = {}
    # newline="" на ЧТЕНИИ так же обязателен, как на записи: универсальный
    # перевод строк молча превращает одиночный CR внутри данных в разрыв
    # записи (CLAUDE.md §Known gotchas, случай с BUGS.md).
    with BASELINE_PATH.open("r", encoding="utf-8", newline="") as handle:
        for line in handle.read().split("\n"):
            row = line.strip()
            if not row or row.startswith("#"):
                continue
            lines, _, rel = row.partition("\t")
            baseline[rel] = int(lines)
    return baseline


def write_baseline(sizes: dict[str, int]) -> None:
    """Записать базовую линию: только файлы длиннее CAP, по убыванию размера."""
    over = sorted(
        ((rel, n) for rel, n in sizes.items() if n > CAP),
        key=lambda item: (-item[1], item[0]),
    )
    body = [
        "# Базовая линия размеров файлов — гейт scripts/check_file_sizes.py.",
        "# Правило и порядок правки — docs/lint-policy.md §5.1.",
        "# Строка здесь = файл, который дорожка SPLIT ещё не разрезала.",
        "# Число может ехать ВНИЗ свободно, ВВЕРХ — только осознанно, тем же",
        "# коммитом, что и рост, и с объяснением в теле коммита.",
        "",
    ]
    body += [f"{n}\t{rel}" for rel, n in over]
    with BASELINE_PATH.open("w", encoding="utf-8", newline="\n") as handle:
        handle.write("\n".join(body) + "\n")


def check(sizes: dict[str, int], baseline: dict[str, int]) -> list[str]:
    """Нарушения текущего состояния относительно базовой линии."""
    problems: list[str] = []

    for rel in sorted(sizes):
        now = sizes[rel]
        was = baseline.get(rel)

        if was is None:
            if now > CAP:
                problems.append(
                    f"{rel}: {now} строк > потолка {CAP}. Новый монолит заводить "
                    f"нельзя — вынесите код в отдельный модуль рядом."
                )
            continue

        if now > was:
            problems.append(
                f"{rel}: вырос {was} -> {now} (+{now - was}). Файл в очереди "
                f"на разрез; если рост осознан — `--update` тем же коммитом."
            )
        elif now <= CAP:
            problems.append(
                f"{rel}: {now} строк, потолок взят — удалите строку из "
                f"{BASELINE_PATH.name} (`--update`), дальше он живёт по потолку."
            )

    for rel in sorted(set(baseline) - set(sizes)):
        problems.append(
            f"{rel}: есть в {BASELINE_PATH.name}, но такого файла нет — "
            f"удалите строку (`--update`)."
        )

    return problems


def self_test() -> None:
    """Проверка логики сравнения на синтетических данных, без файловой системы."""
    cases: list[tuple[str, dict[str, int], dict[str, int], int, str]] = [
        ("новый маленький файл — молчим", {"a.rs": 10}, {}, 0, ""),
        ("новый большой файл — ошибка", {"a.rs": CAP + 1}, {}, 1, "потолка"),
        ("новый файл ровно в потолок — молчим", {"a.rs": CAP}, {}, 0, ""),
        ("старый гигант не изменился", {"a.rs": 9000}, {"a.rs": 9000}, 0, ""),
        ("старый гигант ужался", {"a.rs": 8000}, {"a.rs": 9000}, 0, ""),
        ("старый гигант вырос", {"a.rs": 9001}, {"a.rs": 9000}, 1, "вырос"),
        (
            "гигант ужался ниже потолка — просят убрать из линии",
            {"a.rs": CAP},
            {"a.rs": 9000},
            1,
            "потолок взят",
        ),
        ("файл из линии исчез", {}, {"a.rs": 9000}, 1, "нет"),
    ]

    failures = 0
    for name, sizes, baseline, want_count, want_text in cases:
        problems = check(sizes, baseline)
        if len(problems) != want_count:
            print(f"SELF-TEST FAIL [{name}]: ждали {want_count}, получили {problems}")
            failures += 1
        elif want_text and want_text not in problems[0]:
            print(f"SELF-TEST FAIL [{name}]: нет текста '{want_text}' в {problems[0]}")
            failures += 1

    # Список исключений обязан указывать на существующие файлы, иначе он тихо
    # перестаёт что-либо исключать после переименования.
    for rel in sorted(EXEMPT):
        if not (REPO_ROOT / rel).exists():
            print(f"SELF-TEST FAIL: исключение {rel} указывает в никуда")
            failures += 1

    if failures:
        sys.exit(1)
    print(f"self-test: {len(cases)} случаев + {len(EXEMPT)} исключений — OK")


def main() -> None:
    args = sys.argv[1:]

    if "--self-test" in args:
        self_test()
        return

    sizes = measure()

    if "--top" in args:
        n = int(args[args.index("--top") + 1])
        for rel, count in sorted(sizes.items(), key=lambda i: -i[1])[:n]:
            print(f"{count:7d}  {rel}")
        return

    if "--update" in args:
        baseline = read_baseline()
        grew = [
            (rel, baseline[rel], sizes[rel])
            for rel in sorted(baseline)
            if rel in sizes and sizes[rel] > baseline[rel]
        ]
        write_baseline(sizes)
        for rel, was, now in grew:
            print(f"ВЫРОС: {rel} {was} -> {now} (+{now - was})")
        if grew:
            print(
                f"\n{len(grew)} файл(ов) выросли. Это видно в дифе базовой линии — "
                f"объясните рост в теле коммита."
            )
        over = sum(1 for n in sizes.values() if n > CAP)
        print(f"Базовая линия перезаписана: {over} файлов длиннее {CAP} строк.")
        return

    problems = check(sizes, read_baseline())
    if problems:
        print(f"Нарушений правила размера файла: {len(problems)}\n")
        for problem in problems:
            print(f"  {problem}")
        print(f"\nПравило и порядок правки — docs/lint-policy.md §5.1.")
        sys.exit(1)

    over = sum(1 for n in sizes.values() if n > CAP)
    print(f"Размеры файлов в порядке: {len(sizes)} файлов, {over} в очереди на разрез.")


if __name__ == "__main__":
    main()
