#!/usr/bin/env python3
"""DEVX-3: golden-гейт на текстовых дампах --dump-layout / --dump-display-list.

Запуск:
    python graphic_tests/dump_golden.py            # сверить дампы с эталонами
    python graphic_tests/dump_golden.py --update    # перезаписать эталоны текущим выводом
    python graphic_tests/dump_golden.py --build     # пересобрать lumen.exe перед прогоном

Дополняет пиксельный пайплайн (run.py), не заменяет его: гоняет
`--dump-layout`/`--dump-display-list` по фиксированному набору страниц и
сверяет текстовый вывод с закоммиченными эталонами в
`graphic_tests/dump-golden/`. Ловит регрессии геометрии/порядка отрисовки/
z-index стабильно и кросс-платформенно — без GPU, без Edge, без ffmpeg.
"""
from __future__ import annotations
import argparse
import difflib
import os
import subprocess
import sys

if hasattr(sys.stdout, 'reconfigure'):
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
LUMEN_PROFILE = os.environ.get('LUMEN_PROFILE', 'release')
LUMEN = os.path.join(REPO, 'target', LUMEN_PROFILE, 'lumen.exe')
GOLDEN_DIR = os.path.join(REPO, 'graphic_tests', 'dump-golden')

# Фиксированный набор страниц: разные layout-режимы (block/table/grid/flex/
# transform-stacking), дёшево и детерминированно (без JS, локальные ассеты).
PAGES: list[str] = [
    'samples/page.html',
    'samples/test-06-layout.html',
    'graphic_tests/25-table-layout.html',
    'graphic_tests/35-grid-named-areas.html',
    'graphic_tests/65-flex-align-content.html',
    'graphic_tests/106-transform-zindex.html',
]

DUMP_KINDS: list[tuple[str, str]] = [
    ('layout', '--dump-layout'),
    ('display-list', '--dump-display-list'),
]


def _build_lumen() -> bool:
    print(f'Сборка lumen-shell --profile {LUMEN_PROFILE}...')
    env = os.environ.copy()
    env['PATH'] = r'C:\Users\konstantin\.cargo\bin' + os.pathsep + env.get('PATH', '')
    res = subprocess.run(
        ['cargo', 'build', '-p', 'lumen-shell', '--profile', LUMEN_PROFILE], cwd=REPO, env=env
    )
    return res.returncode == 0


def ensure_lumen(force_build: bool) -> None:
    if force_build or not os.path.exists(LUMEN):
        if not _build_lumen():
            print('Сборка Lumen упала.')
            sys.exit(2)


def dump(page: str, flag: str) -> str:
    """Прогоняет `lumen <flag> <page>` и возвращает stdout (сам дамп)."""
    page_path = os.path.join(REPO, page)
    res = subprocess.run(
        [LUMEN, flag, page_path], cwd=REPO, capture_output=True, text=True, encoding='utf-8'
    )
    if res.returncode != 0:
        raise RuntimeError(f'{flag} {page} завершился с кодом {res.returncode}:\n{res.stderr}')
    return res.stdout


def golden_path(page: str, kind: str) -> str:
    stem = page.replace('/', '__')
    return os.path.join(GOLDEN_DIR, f'{stem}.{kind}.txt')


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument('--update', action='store_true', help='Перезаписать эталоны текущим выводом')
    parser.add_argument('--build', action='store_true', help='Пересобрать lumen.exe перед прогоном')
    args = parser.parse_args()

    ensure_lumen(args.build)

    mismatches: list[str] = []
    total = 0
    for page in PAGES:
        for kind, flag in DUMP_KINDS:
            total += 1
            label = f'{page} [{flag}]'
            try:
                actual = dump(page, flag)
            except RuntimeError as e:
                print(f'{label}: ERROR\n{e}')
                mismatches.append(label)
                continue

            gpath = golden_path(page, kind)
            if args.update:
                os.makedirs(GOLDEN_DIR, exist_ok=True)
                with open(gpath, 'w', encoding='utf-8', newline='\n') as f:
                    f.write(actual)
                print(f'{label}: UPDATED')
                continue

            if not os.path.exists(gpath):
                print(f'{label}: FAIL (нет эталона {gpath}, запусти --update)')
                mismatches.append(label)
                continue

            with open(gpath, encoding='utf-8') as f:
                expected = f.read()

            if actual != expected:
                diff = ''.join(
                    difflib.unified_diff(
                        expected.splitlines(keepends=True),
                        actual.splitlines(keepends=True),
                        fromfile='expected',
                        tofile='actual',
                    )
                )
                print(f'{label}: FAIL\n{diff}')
                mismatches.append(label)
            else:
                print(f'{label}: PASS')

    if args.update:
        print(f'\nЭталоны обновлены: {total}')
        return 0

    if mismatches:
        print(f'\n{len(mismatches)} несовпадение(й) из {total}: ' + ', '.join(mismatches))
        return 1

    print(f'\nВсе {total} дампов совпадают с эталоном.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
