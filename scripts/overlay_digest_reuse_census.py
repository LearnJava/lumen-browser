#!/usr/bin/env python3
"""BUG-405 срез 47: A/B по LUMEN_NO_OVERLAY_DIGEST_REUSE.

`hash_display_list_dual_memo` (кадровый хэш) и `Renderer::overlay_cache_step`
(решение попадание/промах overlay-кэша) независимо обходили ОДИН И ТОТ ЖЕ
overlay-список `hash_one_command`-ом на каждом кадре — статьи `hash`
(срез 34) и `послекэша` (срез 44) обе платят за него. `render_with_anim`
теперь считает дайджест ([`fold_overlay`]) один раз и передаёт его в оба
места; `LUMEN_NO_OVERLAY_DIGEST_REUSE=1` заставляет `overlay_cache_step`
пересчитать его заново — воспроизводит цену ДО среза 47.

Ожидание: `послекэша` на плече `reuse-off` растёт примерно на цену второго
обхода (то же, что срез 44 измерил как хэш внутри `overlay_cache_step`),
`hash` не меняется (тот же обход, что и раньше, — просто платится дважды
на выключенном плече). Общий кадр попадания должен УПАСТЬ на плече
`reuse-on`.

    python scripts/overlay_digest_reuse_census.py --repeats 3 --backend vulkan

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

ARMS = [('reuse-on', {}), ('reuse-off', {'LUMEN_NO_OVERLAY_DIGEST_REUSE': '1'})]


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
                      extra_env=extra, tag=f'odr_{tag}')
            rows[tag].append(report(f'{tag} (повтор {rep})', parse(log)))

    print('\n' + '=' * 78)
    print(f'{"плечо":10s} | {"невязка":>8s} | {"предметки":>10s} | {"послекэша":>10s} | '
          f'{"предвызов":>10s} | {"пасс":>8s} | {"hash":>8s} | {"band-реш.":>10s} | '
          f'{"честный":>8s}')
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
        print(f'{tag:10s} | {med("невязка"):8.3f} | {med("предметки"):10.3f} | '
              f'{med("послекэша"):10.3f} | {med("предвызов"):10.3f} | {med("пасс"):8.3f} | '
              f'{med("hash"):8.3f} | {med("band-реш."):10.3f} | {fair:8.3f}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
