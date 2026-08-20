#!/usr/bin/env python3
"""Пиксельная приёмка кольцевой полосы: `LUMEN_BAND_RING` on vs off.

BUG-405 срез 32 (пункты 43/58 остатка). Промах скролл-композитора перестал
перерисовывать полосу целиком: перекрытие старой и новой полосы остаётся в
текстуре, а пасс идёт только по вышедшей вперёд кромке; текстура при этом
адресуется как ТОР по Y, поэтому копии перекрытия и второй текстуры (пункт 61)
схема не требует. Правка обязана быть пиксельно НЕЙТРАЛЬНОЙ, а корпус
графтестов прокрутку не гоняет вовсе (пункт 47), поэтому гейт — здесь.

Плечи гоняются тем же драйвером, что и `scripts/scroll_blit_accept.py` (живое
окно, MCP-скролл по списку остановок, захват `gdigrab`, кроп по магентовой
рамке): опорное плечо — штатное (полная перерисовка полосы, как в срезах
20–31), второе — `LUMEN_BAND_RING=1`. Кольцо по умолчанию ВЫКЛЮЧЕНО: оно
пиксельно верно, но выигрыша по цене не дало (`scripts/band_ring_census.py`).

    python scripts/band_ring_accept.py
    python scripts/band_ring_accept.py --only 01 --threshold 0.5

Печатается ГЕЙТ ТОЖДЕСТВА плеч: сколько промахов было и сколько строк полосы
они перерисовали (`rows N/H` в строке `page-compose MISS`). Если у плеча
«кольцо» не нашлось ни одного промаха с `N < H`, правка на этом стенде просто
не сработала — и нулевой diff НИЧЕГО не доказывает (урок
`feedback_green_test_can_mask_broken_feature`).

Требуется собранный `lumen.exe` (по умолчанию `target/dev-release`) и
`utils/ffmpeg.exe`. Никаких действий на рабочем столе во время прогона:
`gdigrab` снимает весь экран, и увод фокуса портит все кадры с этого момента.

Код возврата 0 = каждая остановка каждого стенда в пределах порога И кольцо
реально сработало; 1 = регрессия, отказ захвата или не сработавшее кольцо.
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

MISS_RE = re.compile(r'page-compose MISS:.*\brows (\d+)/(\d+)\b')
SKIP_RE = re.compile(r'page-compose skip: (.*)')


def miss_stats(tag: str) -> tuple[list[tuple[int, int]], dict[str, int]]:
    """Промахи плеча из его stderr-лога: список `(строк, высота)` плюс отказы."""
    path = os.path.join(TMP, f'{tag}.stderr.log')
    misses: list[tuple[int, int]] = []
    skips: dict[str, int] = {}
    try:
        with open(path, encoding='utf-8', errors='replace') as fh:
            for line in fh:
                m = MISS_RE.search(line)
                if m:
                    misses.append((int(m.group(1)), int(m.group(2))))
                    continue
                m = SKIP_RE.search(line)
                if m:
                    reason = m.group(1).strip()
                    skips[reason] = skips.get(reason, 0) + 1
    except OSError:
        pass
    return misses, skips


def describe(tag: str, misses: list[tuple[int, int]], skips: dict[str, int]) -> str:
    partial = [n for n, h in misses if n < h]
    head = f'{len(misses)} промахов'
    if misses:
        head += f', строк {min(n for n, _ in misses)}…{max(n for n, _ in misses)}'
        head += f' из {misses[0][1]}'
    if partial:
        head += f'; частичных {len(partial)}'
    if skips:
        head += '; skip: ' + '; '.join(f'{r} x{n}' for r, n in skips.items())
    return f'    [{tag}] {head}'


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('--only', help='один стенд по номеру (например 01)')
    ap.add_argument('--threshold', type=float, default=0.5,
                    help='diff %% на остановку, считающийся регрессией')
    ap.add_argument('--maximized', action='store_true',
                    help='развёрнутое окно (полоса выше документа стенда — промахов меньше)')
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
    ring_fired = False
    print(f'приёмка кольцевой полосы (порог {args.threshold}%), {len(STOPS)} остановок на стенд')
    for fid, fname in fixtures:
        path = os.path.join(FIXTURE_DIR, fname)
        print(f'\n[{fid}] {fname}')
        # Опорное плечо первым: если стенд сдох, это видно сразу.
        off = run_flag(exe, path, blit_on=True, tag=f'{fid}_ringoff',
                       env_extra={'LUMEN_FRAME_LOG': '2'}, maximized=args.maximized)
        on = run_flag(exe, path, blit_on=True, tag=f'{fid}_ringon',
                      env_extra={'LUMEN_BAND_RING': '1', 'LUMEN_FRAME_LOG': '2'},
                      maximized=args.maximized)
        off_m, off_s = miss_stats(f'{fid}_ringoff')
        on_m, on_s = miss_stats(f'{fid}_ringon')
        print(describe('кольцо выкл', off_m, off_s))
        print(describe('кольцо вкл', on_m, on_s))
        if any(n < h for n, h in off_m):
            print(f'  [{fid}] ГЕЙТ: опорное плечо перерисовало полосу частично — '
                  'рычаг отката не сработал')
            any_fail = True
        if any(n < h for n, h in on_m):
            ring_fired = True
        if off is None or on is None or len(off) != len(on):
            print(f'  [{fid}] FAIL (отказ захвата)')
            any_fail = True
            continue
        worst, pcts = compare(off, on, args.threshold)
        stop_fail = [i for i, p in enumerate(pcts) if p > args.threshold]
        status = 'FAIL' if stop_fail else 'PASS'
        if stop_fail:
            any_fail = True
        print(f'  [{fid}] {status}  худший {worst:.3f}%  '
              'остановки=' + ' '.join(f'{p:.2f}' for p in pcts))
        if stop_fail:
            print('        сверх порога (scroll_y): '
                  + ', '.join(f'{STOPS[i]}px={pcts[i]:.2f}%' for i in stop_fail))

    if not ring_fired:
        print('\nГЕЙТ: ни одного ЧАСТИЧНОГО промаха — кольцо на этом стенде не сработало,')
        print('нулевой diff ничего не доказывает (полоса выше документа? один промах на прогон?)')
        any_fail = True
    print('\n' + ('ИТОГ: FAIL' if any_fail else 'ИТОГ: PASS'))
    return 1 if any_fail else 0


if __name__ == '__main__':
    sys.exit(main())
