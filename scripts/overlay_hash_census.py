#!/usr/bin/env python3
"""BUG-405 срез 43: ablation по п.83 остатка (невязка честного кадра
попадания, 0.13-0.20 мс) — рычаг LUMEN_NO_OVERLAY_CACHE.

Гипотеза (найдена чтением кода, не измерением): `compose_page` между
mark(4) (конец `band-реш.`) и вызовом `render_impl(..., RenderPassMode::
Compose)` безусловно зовёт `overlay_cache_step`, которая на КАЖДОМ кадре
хэширует ВЕСЬ overlay-список (`hash_one_command` на каждую команду) —
работа не покрыта ни одной статьёй FRAME_PHASE_NANOS (те кончаются на
mark(4)) и не отражена в "пасс" (тот начинается своим t_frame0 внутри
render_impl). Ровно то место, что искал п.83.

Проверка: `LUMEN_NO_OVERLAY_CACHE=1` целиком отключает вызов
`overlay_cache_step` (в compose_page: `if compose_overlay_disabled() ||
overlay_cache_disabled() { None } else { self.overlay_cache_step(...) }`).
Если гипотеза верна — невязка на плече "cache off" должна упасть примерно
на цену хэширования, а "пасс" вырастет (полный overlay рисуется каждый
кадр вместо блита кэш-текстуры, урок среза 41: 0.88 против 0.56 мс).

    python scripts/overlay_hash_census.py --repeats 3 --backend vulkan

Прогоны интерливед с ВРАЩЕНИЕМ порядка плеч (урок п.64). Бэкенд задавать
обязательно (срез 14).
"""
from __future__ import annotations

import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, 'scripts'))
from band_draw_fraction_census import run  # noqa: E402
from hit_frame_census import parse, report  # noqa: E402

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, 'reconfigure'):
        _stream.reconfigure(encoding='utf-8', errors='replace')

ARMS = [('cache-on', {}), ('cache-off', {'LUMEN_NO_OVERLAY_CACHE': '1'})]


def main() -> int:
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument('--repeats', type=int, default=3)
    ap.add_argument('--ticks', type=int, default=60)
    ap.add_argument('--delta', type=float, default=120.0)
    ap.add_argument('--backend', default='vulkan')
    ap.add_argument('--page', default='samples/bench-text-scroll.html')
    args = ap.parse_args()

    rows: dict[str, list[dict]] = {tag: [] for tag, _ in ARMS}
    for rep in range(args.repeats):
        shift = rep % len(ARMS)
        for tag, extra in ARMS[shift:] + ARMS[:shift]:
            log = run(1.0, rep, args.ticks, args.delta, args.backend, args.page,
                      extra_env=extra, tag=f'ov_{tag}')
            rows[tag].append(report(f'{tag} (повтор {rep})', parse(log)))

    print('\n' + '=' * 78)
    print(f'{"плечо":10s} | {"невязка":>8s} | {"пасс":>8s} | {"hash":>8s} | '
          f'{"band-реш.":>10s} | {"честный":>8s}')
    import statistics
    for tag, _ in ARMS:
        rs = rows[tag]
        cols_all = [r['cols'] for r in rs if r['cols']]
        if not cols_all:
            print(f'{tag:10s} | нет попаданий с уровнем 2')
            continue

        def med(key: str) -> float:
            return statistics.median([c.get(key, float('nan')) for c in cols_all])

        fair = statistics.median([r['fair_p50'] for r in rs if r['fair_p50'] == r['fair_p50']])
        print(f'{tag:10s} | {med("невязка"):8.3f} | {med("пасс"):8.3f} | '
              f'{med("hash"):8.3f} | {med("band-реш."):10.3f} | {fair:8.3f}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
