#!/usr/bin/env python3
"""Перенумеровать указатели STATUS-PN.md после сдвига строк в источниках.

ЗАЧЕМ. Указатель в `STATUS-PN.md` — это `<источник>:<НОМЕР СТРОКИ>` (схема в
`docs/dev-roles.md` §Task tracking schema). Номер строки уезжает от любой правки
ВЫШЕ него в том же файле, и молча: указатель продолжает разрешаться, просто в
чужую строку. Раньше это случалось редко (баги дописывались в конец, а починка
меняла ячейку статуса на месте). С 2026-08-31 закрытые баги переносятся из
`BUGS.md` в `BUGS-FIXED.md`, то есть КАЖДАЯ починка сдвигает всё, что ниже, —
поэтому ручная сверка 194 указателей P3 больше не вариант.

КАК. Скрипт берёт версию источника из git (`HEAD` по умолчанию), смотрит, какой
якорь стоял на строке N ТАМ, и находит тот же якорь в рабочей копии. Якорь —
первая колонка таблицы: `BUG-NNN` для BUGS.md/BUGS-FIXED.md, id задачи для
ROADMAP.md, имя свойства для CSS-SPECS.md.

    python scripts/remap_status_pointers.py            # показать, что изменится
    python scripts/remap_status_pointers.py --apply    # записать
    python scripts/remap_status_pointers.py --base origin/main --apply

Указатель на баг, который в рабочей копии оказался в архиве (то есть починен),
НЕ переписывается на архив, а помечается как протухший: строка указателя должна
быть удалена по протоколу (шаг 4 чеклиста), и скрипт этого сам не делает —
удаление чужой очереди остаётся решением человека.

Файлы читаются с `newline=''` и режутся только по '\\n': `splitlines()` режет
ещё и по одиночному CR, а в описаниях багов сырые CR встречаются (CLAUDE.md
§Known gotchas).

ВАЖНО: `--base` должен быть ревизией, в которой указатели были ВЕРНЫ. Скрипт
читает якорь по номеру строки в базе, поэтому повторный прогон по уже
перенумерованному файлу разрешит номера в чужие якоря и всё испортит. Один
сдвиг источника — один прогон, до коммита.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Консоль Windows по умолчанию cp1251 — на «→» скрипт падал бы UnicodeEncodeError
# ровно в тот момент, когда сообщает о протухшем указателе.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

# Первая колонка markdown-таблицы, с необязательной ссылкой: | [BUG-123](...) | …
ANCHOR = re.compile(r"^\|\s*(?:\[)?\s*([A-Za-z0-9][\w\-./]*)")

STATUS_FILES = sorted(ROOT.glob("STATUS-P*.md"))
POINTER = re.compile(r"^([\w.\-]+\.md):(\d+)$")


def read_lines(path: Path) -> list[str]:
    """Строки рабочей копии, разрезанные ТОЛЬКО по '\\n'."""
    return path.read_text(encoding="utf-8", newline="").split("\n")


def read_lines_at(base: str, rel: str) -> list[str] | None:
    """Строки файла в ревизии `base`, или None, если файла там нет."""
    res = subprocess.run(
        ["git", "show", f"{base}:{rel}"], cwd=ROOT, capture_output=True
    )
    if res.returncode != 0:
        return None
    return res.stdout.decode("utf-8", "replace").split("\n")


def anchor_at(lines: list[str], num: int) -> str | None:
    if not 1 <= num <= len(lines):
        return None
    m = ANCHOR.match(lines[num - 1])
    return m.group(1) if m else None


def line_of(lines: list[str], anchor: str, near: int | None = None) -> int | None:
    """Строка с этим якорем; при нескольких — ближайшая к `near`.

    Первое совпадение брать нельзя: в ROADMAP.md и CSS-SPECS.md первая колонка
    повторяется (одно и то же имя свойства стоит в нескольких таблицах), и
    «первое» уводило указатель в чужую таблицу — проверено на STATUS-P4.md,
    где шесть указателей разом схлопнулись в строку 154.
    """
    hits = [i for i, line in enumerate(lines, 1)
            if (m := ANCHOR.match(line)) and m.group(1) == anchor]
    if not hits:
        return None
    if near is None:
        return hits[0]
    return min(hits, key=lambda i: abs(i - near))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--base", default="HEAD", help="ревизия-источник (по умолчанию HEAD)")
    ap.add_argument("--apply", action="store_true", help="записать изменения")
    args = ap.parse_args()

    old_cache: dict[str, list[str] | None] = {}
    new_cache: dict[str, list[str]] = {}
    changed = stale = broken = ok = 0

    for status in STATUS_FILES:
        src_lines = read_lines(status)
        out: list[str] = []
        touched = False

        for raw in src_lines:
            ptr = raw.strip()
            m = POINTER.match(ptr)
            if not m:
                out.append(raw)
                continue

            rel, num = m.group(1), int(m.group(2))
            if rel not in old_cache:
                old_cache[rel] = read_lines_at(args.base, rel)
            old = old_cache[rel]
            target = ROOT / rel
            if old is None or not target.exists():
                out.append(raw)
                continue
            if rel not in new_cache:
                new_cache[rel] = read_lines(target)

            # Источник не менялся — двигаться нечему. Без этой проверки скрипт
            # «перенумеровывал» указатели в нетронутых файлах по совпадению
            # якорей и портил их (STATUS-P4.md, 6 указателей).
            if old == new_cache[rel]:
                ok += 1
                out.append(raw)
                continue

            anchor = anchor_at(old, num)
            if anchor is None:
                broken += 1
                print(f"{status.name}: {ptr} — в {args.base} на этой строке нет строки таблицы")
                out.append(raw)
                continue

            new_num = line_of(new_cache[rel], anchor, near=num)
            if new_num is None:
                # Якорь исчез из своего файла — для бага это значит «переехал в архив».
                where = ""
                if rel == "BUGS.md":
                    arch = ROOT / "BUGS-FIXED.md"
                    if arch.exists():
                        if "BUGS-FIXED.md" not in new_cache:
                            new_cache["BUGS-FIXED.md"] = read_lines(arch)
                        if line_of(new_cache["BUGS-FIXED.md"], anchor):
                            where = " — он в BUGS-FIXED.md, то есть починен"
                stale += 1
                print(f"{status.name}: {ptr} → {anchor} ПРОТУХ{where}; указатель нужно снять")
                out.append(raw)
                continue

            if new_num != num:
                changed += 1
                touched = True
                out.append(f"{rel}:{new_num}")
            else:
                ok += 1
                out.append(raw)

        if touched and args.apply:
            status.write_text("\n".join(out), encoding="utf-8", newline="")
            print(f"{status.name}: записан")

    print(
        f"\nбез изменений: {ok} | перенумеровано: {changed} | "
        f"протухших: {stale} | битых: {broken}"
    )
    if changed and not args.apply:
        print("это была сухая прогонка — повтори с --apply")
    return 1 if (stale or broken) else 0


if __name__ == "__main__":
    sys.exit(main())
