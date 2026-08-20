#!/usr/bin/env python3
"""Пиксельная приёмка слитого кадрового хэша: `LUMEN_NO_DUAL_HASH` off vs on.

BUG-405 срез 35 (пункт 70 остатка). Два O(n)-хэша кадра — тотальный (для
skip-identical) и scroll-инвариантный ключ полосы — сведены в ОДИН проход по
display list-у. Значения обеих свёрток при этом изменились (свёртка команды
считается один раз и уходит в оба хешера), а на значениях держатся ДВА
решения: «кадр тождественен предыдущему, не рисуем вовсе» и «содержимое полосы
то же, можно блитить». Ошибка в любом из них — застрявшие пиксели на экране,
поэтому гейт пиксельный, а не только счётный.

Корпус графтестов сюда не годится: прокрутку он не гоняет вовсе (пункт 47
остатка), а CPU-снимки идут мимо композитора. Поэтому используется тот же
драйвер, что у срезов 32 (`scripts/band_ring_accept.py`) и `scroll_blit_accept`:
живое окно, MCP-скролл по списку остановок, захват `gdigrab`, кроп по
магентовой рамке.

    python scripts/dual_hash_accept.py
    python scripts/dual_hash_accept.py --only 01 --threshold 0.5

Печатается ГЕЙТ ТОЖДЕСТВА плеч: сколько кадров каждое плечо пропустило как
тождественные и сколько раз промахнулось мимо полосы. Если плечи разошлись в
этих числах — они мерили разную работу, и нулевой diff ничего не доказывает.

Требуется собранный `lumen.exe` (по умолчанию `target/dev-release`) и
`utils/ffmpeg.exe`. Никаких действий на рабочем столе во время прогона:
`gdigrab` снимает весь экран, и увод фокуса портит все кадры с этого момента.

Код возврата 0 = каждая остановка каждого стенда в пределах порога.
"""

from __future__ import annotations

import argparse
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from scroll_blit_accept import (  # noqa: E402
    FIXTURE_DIR,
    FIXTURES,
    REPO,
    STOPS,
    TMP,
    compare,
    run_flag,
)

MISS_RE = re.compile(r'page-compose MISS:')
HIT_RE = re.compile(r'page-compose HIT')
SKIP_RE = re.compile(r'skip \(identical frame\)')


def frame_stats(tag: str) -> str:
    """Решения плеча из его stderr-лога: попадания, промахи, пропуски кадра."""
    path = os.path.join(TMP, f'{tag}.stderr.log')
    hits = misses = skips = 0
    try:
        with open(path, encoding='utf-8', errors='replace') as fh:
            for line in fh:
                if HIT_RE.search(line):
                    hits += 1
                elif MISS_RE.search(line):
                    misses += 1
                elif SKIP_RE.search(line):
                    skips += 1
    except OSError:
        return f'    [{tag}] лога нет'
    return f'    [{tag}] попаданий {hits}, промахов {misses}, тождественных {skips}'


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('--only', help='один стенд по номеру (например 01)')
    ap.add_argument('--threshold', type=float, default=0.5,
                    help='diff %% на остановку, считающийся регрессией')
    ap.add_argument('--maximized', action='store_true',
                    help='развёрнутое окно (полоса выше документа стенда — промахов меньше)')
    ap.add_argument('--null', action='store_true',
                    help='плечо против САМОГО СЕБЯ: собственный разброс стенда')
    args = ap.parse_args()

    profile = os.environ.get('LUMEN_PROFILE', 'dev-release')
    exe = os.path.join(REPO, 'target', profile, 'lumen.exe')
    if not os.path.exists(exe):
        print(f'нет бинарника {exe} — cargo build -p lumen-shell --profile {profile}',
              file=sys.stderr)
        return 1
    os.makedirs(TMP, exist_ok=True)

    fixtures = [f for f in FIXTURES if args.only is None or f[0] == args.only]
    if not fixtures:
        print(f'нет стенда под --only {args.only}', file=sys.stderr)
        return 1

    any_fail = False
    print(f'приёмка слитого хэша (порог {args.threshold}%), {len(STOPS)} остановок на стенд')
    for fid, fname in fixtures:
        path = os.path.join(FIXTURE_DIR, fname)
        print(f'\n[{fid}] {fname}')
        # Опорное плечо: старое поведение — или, при `--null`, то же самое
        # новое, чтобы отделить регрессию правки от разброса самого стенда.
        ref_env = {'LUMEN_FRAME_LOG': '2'}
        if not args.null:
            ref_env['LUMEN_NO_DUAL_HASH'] = '1'
        two = run_flag(exe, path, blit_on=True, tag=f'{fid}_hash_two',
                       env_extra=ref_env, maximized=args.maximized)
        one = run_flag(exe, path, blit_on=True, tag=f'{fid}_hash_one',
                       env_extra={'LUMEN_FRAME_LOG': '2'}, maximized=args.maximized)
        print(frame_stats(f'{fid}_hash_two'))
        print(frame_stats(f'{fid}_hash_one'))
        if two is None or one is None or len(two) != len(one):
            print(f'  [{fid}] FAIL (отказ захвата)')
            any_fail = True
            continue
        worst, pcts = compare(two, one, args.threshold)
        stop_fail = [i for i, p in enumerate(pcts) if p > args.threshold]
        status = 'FAIL' if stop_fail else 'PASS'
        if stop_fail:
            any_fail = True
        print(f'  [{fid}] {status}  худший {worst:.3f}%  '
              + 'остановки=' + ' '.join(f'{p:.2f}' for p in pcts))
        if stop_fail:
            print('        сверх порога (scroll_y): '
                  + ', '.join(f'{STOPS[i]}px={pcts[i]:.2f}%' for i in stop_fail))

    print('\nИТОГ: ' + ('РЕГРЕССИЯ' if any_fail else 'плечи совпали'))
    return 1 if any_fail else 0


if __name__ == '__main__':
    raise SystemExit(main())
