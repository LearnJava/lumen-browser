#!/usr/bin/env python3
"""Перепись монолитного `.rs`-файла для дорожки SPLIT (метод §1 плана).

Два режима, оба нужны, потому что отвечают на разные вопросы.

1. `--items` — карта top-level item'ов: имя, вид, границы, размер, привязанный
   доккомментарий. Заменяет однострочник `rg "^pub struct |^impl |…"` из §1
   плана, который врёт в трёх местах: он не отличает код от строкового литерала
   (в `js/dom.rs` внутри `WEB_API_SHIM` лежат тысячи строк JS, начинающихся с
   ключевых слов), не знает границ item'а (а без них нельзя измерить регион) и
   не поднимает начало item'а над его атрибутами и `///`, из-за чего вырезка по
   таким числам отрывает доккомментарий от функции — осечка SH-4b/SH-5.

2. `--inner LO HI` — карта ТЕМ внутри одного inline-модуля: баннеры-разделители
   (`// ─── …`) и вес отрезка между ними. Нужен там, где регион надо разложить
   на файлы <= 2000 строк, а он состоит из одного модуля без вложенных: так
   `mod tests` в `layout/src/style.rs` (12 278 строк, ноль вложенных модулей,
   1 195 `#[test]`) разложился на 117 авторских секций.

Глубина скобок считается ВНЕ строк и комментариев (обычные и raw-строки,
символьные литералы, вложенные `/* */`), поэтому item'ом признаётся только
строка нулевой колонки на нулевой глубине. Проверка себя: итоговая глубина
обязана быть 0 — иначе лексер разошёлся с файлом и числам верить нельзя.

Файл читается побайтово и декодируется один раз: у монолитов встречается
смесь UTF-8 и двойной кодировки (`shell/src/main.rs`), и работать с ними надо
как с байтами, не перепечатывая текст.

Usage:
    python scripts/split_census.py <file>                 # карта item'ов
    python scripts/split_census.py <file> --json out.json # + машинный дамп
    python scripts/split_census.py <file> --inner 100 900 # карта тем модуля
"""

from __future__ import annotations

import json
import re
import sys
from collections import Counter

BS = chr(92)
QUOTE = chr(34)
APOS = chr(39)

ITEM_RE = re.compile(
    r'^(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?'
    r'(?:extern\s+"[^"]*"\s+)?'
    r'(struct|enum|impl|fn|mod|trait|const|static|type|union|use|macro_rules!)\b'
)
NAME_RE = re.compile(
    r'^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?'
    r'(?:extern\s+"[^"]*"\s+)?'
    r'(struct|enum|impl|fn|mod|trait|const|static|type|union|use|macro_rules!)\s+([^({<\s:=;]*)'
)
INNER_ITEM_RE = re.compile(
    r'^    (?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?'
    r'(?:fn|struct|enum|impl|mod|const|static|type|use)\b'
)
BANNER_RE = re.compile(r'^    // [─=-]{2,}')


def read_lines(path: str) -> list[str]:
    return open(path, 'rb').read().decode('utf-8').split(chr(10))


def scan_depth(lines: list[str]) -> tuple[list[int], list[bool], int]:
    """Глубина скобок в начале каждой строки + признак «строка начинается в коде»."""
    depth = 0
    block_comment = 0
    raw = None
    in_str = False
    in_char = False
    depths: list[int] = []
    is_code: list[bool] = []
    for ln in lines:
        depths.append(depth)
        is_code.append(block_comment == 0 and raw is None and not in_str and not in_char)
        i = 0
        n = len(ln)
        while i < n:
            c = ln[i]
            if block_comment:
                if ln.startswith('*/', i):
                    block_comment -= 1
                    i += 2
                    continue
                if ln.startswith('/*', i):
                    block_comment += 1
                    i += 2
                    continue
                i += 1
                continue
            if raw is not None:
                if c == QUOTE:
                    j, k = i + 1, 0
                    while j < n and ln[j] == '#' and k < raw:
                        j += 1
                        k += 1
                    if k == raw:
                        raw = None
                        i = j
                        continue
                i += 1
                continue
            if in_str:
                if c == BS:
                    i += 2
                    continue
                if c == QUOTE:
                    in_str = False
                i += 1
                continue
            if in_char:
                if c == BS:
                    i += 2
                    continue
                if c == APOS:
                    in_char = False
                i += 1
                continue
            if ln.startswith('//', i):
                break
            if ln.startswith('/*', i):
                block_comment = 1
                i += 2
                continue
            if c == 'r' and i + 1 < n and ln[i + 1] in ('#', QUOTE):
                j, h = i + 1, 0
                while j < n and ln[j] == '#':
                    j += 1
                    h += 1
                if j < n and ln[j] == QUOTE:
                    raw = h
                    i = j + 1
                    continue
            if c == 'b' and i + 1 < n and ln[i + 1] == QUOTE:
                in_str = True
                i += 2
                continue
            if c == QUOTE:
                in_str = True
                i += 1
                continue
            if c == APOS:
                # символьный литерал против лайфтайма
                if i + 1 < n and ln[i + 1] == BS:
                    in_char = True
                elif i + 2 < n and ln[i + 2] == APOS:
                    in_char = True
                i += 1
                continue
            if c == '{':
                depth += 1
            elif c == '}':
                depth -= 1
            i += 1
    return depths, is_code, depth


def lift_start(lines: list[str], line_1based: int) -> tuple[int, list[str]]:
    """Поднять начало item'а над его атрибутами, `///` и обычными комментариями."""
    i = line_1based - 2
    start = line_1based
    doc: list[str] = []
    while i >= 0:
        s = lines[i].strip()
        if s.startswith('///') or s.startswith('//!') or s.startswith('#[') or s.startswith('#!['):
            start = i + 1
            doc.append(s)
            i -= 1
            continue
        if s.startswith('//'):
            start = i + 1
            doc.append(s)
            i -= 1
            continue
        if s.startswith(']') or s.startswith(')]'):
            # строка-продолжение многострочного атрибута
            start = i + 1
            i -= 1
            continue
        break
    return start, list(reversed(doc))


def census_items(path: str, dump: str | None) -> None:
    lines = read_lines(path)
    depths, is_code, final_depth = scan_depth(lines)
    items = []
    for idx, ln in enumerate(lines):
        if not is_code[idx] or depths[idx] != 0 or ln[:1] in (' ', '\t', ''):
            continue
        m = ITEM_RE.match(ln)
        if not m:
            continue
        start, doc = lift_start(lines, idx + 1)
        nm = NAME_RE.match(ln)
        items.append({
            'line': idx + 1,
            'start': start,
            'kind': m.group(1),
            'name': (nm.group(2) if nm else ln.strip()[:60]),
            'doc': doc,
            'text': ln.rstrip(),
        })
    for a, b in zip(items, items[1:]):
        a['end'] = b['start'] - 1
    if items:
        items[-1]['end'] = len(lines)
    for it in items:
        it['size'] = it['end'] - it['start'] + 1

    for it in items:
        print(f"{it['start']:6d} {it['end']:6d} {it['size']:6d} {it['kind']:<6} {it['name']}")
    print(f"# item'ов: {len(items)} | строк в файле: {len(lines)}")
    print(f"# {Counter(i['kind'] for i in items).most_common()}")
    print(f"# сумма спанов: {sum(i['size'] for i in items)} | итоговая глубина: {final_depth}"
          f" {'(ok)' if final_depth == 0 else '(ЛЕКСЕР РАЗОШЁЛСЯ — числам не верить)'}")
    # Признак отклеившегося доккомментария: item без `///` над ним рядом с теми,
    # у кого он есть. Осечка SH-3c ищется именно так.
    orphan = [i['start'] for i in items if not any(d.startswith('///') for d in i['doc'])]
    print(f"# item'ов без /// над ними: {len(orphan)}")
    if dump:
        json.dump(items, open(dump, 'w', encoding='utf-8'), ensure_ascii=False, indent=0)
        print(f'# дамп: {dump}')


def census_inner(path: str, lo: int, hi: int) -> None:
    lines = read_lines(path)
    marks = []
    for idx in range(lo - 1, min(hi, len(lines))):
        ln = lines[idx]
        if BANNER_RE.match(ln):
            marks.append(('BANNER', idx + 1, ln.strip()))
        elif INNER_ITEM_RE.match(ln):
            marks.append(('ITEM', idx + 1, ln.strip()))

    sections = []
    cur = {'banner': '(шапка модуля)', 'start': lo, 'items': 0}
    for kind, line, text in marks:
        if kind == 'BANNER':
            cur['end'] = line - 1
            sections.append(cur)
            cur = {'banner': text, 'start': line, 'items': 0}
        else:
            cur['items'] += 1
    cur['end'] = hi
    sections.append(cur)

    total = 0
    for s in sections:
        size = s['end'] - s['start'] + 1
        total += size
        print(f"{s['start']:6d}-{s['end']:<6d} {size:6d} items={s['items']:<4d} {s['banner'][:100]}")
    print(f'# секций: {len(sections)} | покрыто строк: {total}')


def main() -> int:
    argv = sys.argv[1:]
    if not argv:
        print(__doc__)
        return 2
    path = argv[0]
    if '--inner' in argv:
        k = argv.index('--inner')
        census_inner(path, int(argv[k + 1]), int(argv[k + 2]))
        return 0
    dump = None
    if '--json' in argv:
        dump = argv[argv.index('--json') + 1]
    census_items(path, dump)
    return 0


if __name__ == '__main__':
    sys.exit(main())
