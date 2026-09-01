#!/usr/bin/env python3
"""BUG-405 срез 58: честный (живое окно) A/B по LUMEN_NO_CHROME_DIGEST_REUSE.

Срез 57 реализовал переиспользование дайджеста `ChromeOverlayFrameCache` в
`fold_overlay` (не пересчитывать хэш уже известного HIT-сегмента хрома) и
замерил эффект ИЗОЛИРОВАННО — синтетическим `#[ignore]`d юнит-тестом на
готовом срезе массива (60.6% цены `fold_overlay`, 0.41→0.16мс). По
`docs/perf-method.md` изолированный замер не заменяет честный: этот скрипт
закрывает п.85 живым числом — счётчик `послекэша` (`POST_CACHE_NANOS`,
срез 44), который уже включает хэш `overlay_cache_step` целиком, должен
упасть на плече `reuse-on`, и «честный» полный кадр попадания — вместе с ним.

    python scripts/chrome_digest_reuse_census.py --repeats 3 --backend vulkan

Интерливед с вращением порядка плеч (п. 64). Бэкенд задавать обязательно
(срез 14).
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

ARMS = [('reuse-on', {}), ('reuse-off', {'LUMEN_NO_CHROME_DIGEST_REUSE': '1'})]


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
                      extra_env={**extra, 'LUMEN_FRAME_LOG': '2'}, tag=f'cdr_{tag}')
            rows[tag].append(report(f'{tag} (повтор {rep})', parse(log)))

    print('\n' + '=' * 78)
    print(f'{"плечо":10s} | {"невязка":>8s} | {"послекэша":>10s} | {"hash":>8s} | '
          f'{"честный":>8s}')
    import statistics
    fairs: dict[str, float] = {}
    for tag, _ in ARMS:
        rs = rows[tag]
        cols_all = [r['cols'] for r in rs if r['cols']]
        if not cols_all:
            print(f'{tag:10s} | нет попаданий с уровнем 2')
            continue

        def med(key: str) -> float:
            return statistics.median([c.get(key, float('nan')) for c in cols_all])

        fair = statistics.median([r['fair_p50'] for r in rs if r['fair_p50'] == r['fair_p50']])
        fairs[tag] = fair
        print(f'{tag:10s} | {med("невязка"):8.3f} | {med("послекэша"):10.3f} | '
              f'{med("hash"):8.3f} | {fair:8.3f}')

    if 'reuse-on' in fairs and 'reuse-off' in fairs and fairs['reuse-off'] > 0:
        saved = (1 - fairs['reuse-on'] / fairs['reuse-off']) * 100
        print(f'\nreuse-on экономит {saved:.1f}% честного кадра попадания '
              f'(медиана по повторам, не минимум — см. текст среза).')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
