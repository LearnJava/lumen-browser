#!/usr/bin/env python3
"""Окупается ли ДРУГОЙ запас полосы скролл-композитора (BUG-405 срез 31).

Вопрос — пункт 57 остатка бага. Срез 27 прогнал запасы 300…2600 CSS px по
ОДНОМУ прогону на точку и получил приведённую к пути цену промахов 10.6–18.3
мс/1000 px: кривая плоская у штатного значения и растёт вправо, но точка 500
(10.6) выпадала вниз при разбросе соседних точек ±4 мс, то есть внутри шума.
Пункт 57 прямо просит либо подтвердить её тремя прогонами, либо забыть.

Второй повод пересмотреть выбор — срез 30 (пункт 63): четверть надбавки
промаха постоянна и платится ЗА ПРОМАХ, а не за пиксель. Значит арифметика
выбора запаса — не «промах дороже ровно во столько, во сколько реже», а
`N(запас) × (постоянная + строки(запас))`, и меньший запас платит постоянную
статью чаще. Раньше это в выбор не входило.

Метод — интерливед-свип по `LUMEN_BAND_MARGIN_CSS` (рычаг переписи среза 27,
полное переопределение штатной формулы `min(0.75·вьюпорт, 768)`) при
неизменных окне, странице, шаге и ДЛИНЕ пути прокрутки. Сравнение по минимуму
из повторов (`docs/perf-method.md`): разброс прогонов на этом стенде больше
ожидаемого эффекта, поэтому одиночный прогон точку не решает — ровно на этом
и споткнулся срез 27.

Считаются две цены, обе приведённые к 1000 CSS px пути:

    надбавка   Σ(промах − p50 попадания) / путь — статья, которую адресует
               инкрементальная дорисовка (пункт 43); метрика среза 27, взята
               ради сопоставимости с его числами;
    полная     Σ(все кадры пути композитора) / путь — то, что платит
               пользователь. Надбавка её не заменяет: она вычитает попадание,
               а попадание тоже может зависеть от размера полосы (блит
               сэмплит текстуру другой высоты), и такой сдвиг надбавка
               спрятала бы целиком.

    python scripts/band_margin_census.py --repeats 3 --backend vulkan
    python scripts/band_margin_census.py --margins 400 500 600 --repeats 5

Бэкенд задавать обязательно: цифры DX12 и Vulkan несопоставимы (срез 14).

Запас меньше 0.25 вьюпорта отключает композитор целиком
(`BAND_MIN_MARGIN_RATIO`) — плечо тогда даёт НОЛЬ кадров композитора и
диагностику `skip: вьюпорт не оставляет запаса в лимите текстуры`, а не
маленькое число.

Гейт тождества печатается рядом с числами: путь прокрутки, ширина полосы и
доля перерисовки обязаны совпасть у всех плеч (меняется только высота полосы),
иначе плечи мерили разные конфигурации.
"""

from __future__ import annotations

import argparse
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, 'scripts'))
from band_draw_fraction_census import log_name, med, parse, run  # noqa: E402

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, 'reconfigure'):
        _stream.reconfigure(encoding='utf-8', errors='replace')


def arm_env(margin: float | None) -> dict[str, str]:
    """Окружение плеча: штатный запас — это ОТСУТСТВИЕ переменной, не её значение."""
    return {} if margin is None else {'LUMEN_BAND_MARGIN_CSS': repr(float(margin))}


def report(tag: str, data: dict) -> dict:
    """Один прогон плеча: обе цены на 1000 px пути плюс гейт тождества."""
    miss = [ms for ms, _ in data['miss']]
    hit = [ms for ms, _ in data['hit']]
    band = data['band']
    ys = [sy for _, sy in data['miss'] + data['hit'] + data['other']]
    travel = (max(ys) - min(ys)) if ys else 0.0
    hit_med = med(hit)
    nan = float('nan')
    over_kpx = sum(m - hit_med for m in miss) / travel * 1000.0 if miss and hit and travel else nan
    total_kpx = (sum(miss) + sum(hit)) / travel * 1000.0 if travel and (miss or hit) else nan

    print(f'\n=== запас {tag} ===')
    print(f'  {data["adapter"]}')
    if band:
        print(f'  полоса: {band[0]}x{band[1]} px   frac {data["fracs"] or "нет"}')
    print(f'  путь {travel:.0f} css px; промахов {len(miss)}, попаданий {len(hit)}, '
          f'вне композитора {len(data["other"])}')
    if miss:
        print(f'  промах: p50 {med(miss):6.2f}  min {min(miss):6.2f}  max {max(miss):6.2f}')
    if hit:
        print(f'  попад.: p50 {hit_med:6.2f}  min {min(hit):6.2f}  max {max(hit):6.2f}')
    print(f'  на 1000 px пути: надбавка {over_kpx:6.2f} мс, полная {total_kpx:6.2f} мс')
    for reason, n in data['skips'].items():
        print(f'  skip x{n}: {reason}')
    return {'band_h': band[1] if band else 0, 'band_w': band[0] if band else 0,
            'miss_n': len(miss), 'hit_n': len(hit), 'travel': travel,
            'miss_med': med(miss), 'hit_med': hit_med,
            'over_kpx': over_kpx, 'total_kpx': total_kpx,
            'fracs': data['fracs'], 'skips': data['skips']}


def _col(rows: list[dict], key: str) -> list[float]:
    return [r[key] for r in rows if r[key] == r[key]]


def summarize(arms: list[float | None], rows: dict[str, list[dict]]) -> None:
    """Сводка по минимумам из повторов плюс проверка, решается ли разница над разбросом."""
    print('\n' + '=' * 88)
    print('Цена прокрутки против запаса полосы (мин из повторов, мс на 1000 px пути).')
    print('«разброс» — max−min повторов ТОГО ЖЕ плеча: разница между плечами меньше')
    print('него ничего не решает, ровно этим и был плох одиночный свип среза 27.\n')
    print('запас  | полоса | промахов | надбавка мин (разброс) | полная мин (разброс) | попад. p50')
    best: dict[str, dict] = {}
    for margin in arms:
        tag = 'штатный' if margin is None else f'{margin:g}'
        rs = rows.get(tag, [])
        over, total = _col(rs, 'over_kpx'), _col(rs, 'total_kpx')
        if not total:
            skips = sorted({r for x in rs for r in x['skips']})
            print(f'{tag:6s} | нет кадров композитора  {"; ".join(skips)}')
            continue
        heights = sorted({r['band_h'] for r in rs})
        miss_n = sorted({r['miss_n'] for r in rs})
        best[tag] = {'over': min(over) if over else float('nan'), 'total': min(total),
                     'band_h': heights[-1], 'miss_n': miss_n}
        print(f'{tag:6s} | {",".join(str(h) for h in heights):6s} | '
              f'{",".join(str(n) for n in miss_n):8s} | '
              f'{min(over) if over else float("nan"):8.2f} ({max(over) - min(over) if over else 0:5.2f}) '
              f'{"":5s}| {min(total):7.2f} ({max(total) - min(total):5.2f}) '
              f'{"":5s}| {med([r["hit_med"] for r in rs]):6.2f}')

    # Гейт тождества: путь, ширина полосы и доля перерисовки обязаны совпасть.
    travels = sorted({round(r['travel']) for rs in rows.values() for r in rs})
    widths = sorted({r['band_w'] for rs in rows.values() for r in rs if r['band_w']})
    fracs = sorted({f for rs in rows.values() for r in rs for f in r['fracs']})
    print(f'\nгейт тождества: путь {travels} css px, ширина полосы {widths} px, '
          f'доля перерисовки {fracs or "нет (полная)"}')
    if len(travels) > 1 and (max(travels) - min(travels)) > 0.02 * max(travels):
        print('  ВНИМАНИЕ: путь прокрутки разошёлся между плечами — цены на 1000 px '
              'сопоставимы, но число промахов сравнивать нельзя')
    if len(widths) > 1 or len(fracs) > 1:
        print('  ВНИМАНИЕ: плечи мерили разные конфигурации, числам верить нельзя')

    if 'штатный' not in best or len(best) < 2:
        return
    ref = best['штатный']
    print('\nПротив штатного запаса (по полной цене; знак «−» = дешевле штатного):')
    for tag, b in best.items():
        if tag == 'штатный':
            continue
        d = b['total'] - ref['total']
        print(f'  {tag:6s} полоса {b["band_h"]:5d} px: {d:+6.2f} мс/1000 px '
              f'({100.0 * d / ref["total"]:+5.1f} %)')
    spread = max((max(_col(rs, 'total_kpx')) - min(_col(rs, 'total_kpx'))
                  for rs in rows.values() if len(_col(rs, 'total_kpx')) > 1), default=0.0)
    win_tag = min(best, key=lambda t: best[t]['total'])
    win = ref['total'] - best[win_tag]['total']
    print(f'\nлучшее плечо {win_tag}: {win:+.2f} мс/1000 px против штатного при '
          f'наибольшем внутреннем разбросе {spread:.2f} мс')
    print('  РЕШАЕТСЯ над разбросом' if win > spread else
          '  НЕ решается над разбросом — точку не подтверждать')


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('--margins', type=float, nargs='+',
                    default=[300, 400, 500, 600, 900, 1200],
                    help='запасы полосы в CSS px; штатное плечо добавляется всегда')
    ap.add_argument('--repeats', type=int, default=3, help='повторов на плечо (интерливед)')
    ap.add_argument('--ticks', type=int, default=120)
    ap.add_argument('--delta', type=float, default=120.0, help='CSS px на щелчок колеса')
    ap.add_argument('--backend', default='vulkan')
    ap.add_argument('--page', default='samples/bench-static-scroll.html')
    ap.add_argument('--report-only', action='store_true')
    args = ap.parse_args()

    # Штатное плечо (`None`) идёт первым в первом же круге: оно опорное, и
    # если сдох стенд, это видно на первом прогоне, а не после всего свипа.
    arms: list[float | None] = [None, *args.margins]
    rows: dict[str, list[dict]] = {}
    for rep in range(args.repeats):
        # Порядок плеч ВРАЩАЕТСЯ по кругам. Интерливед сам по себе не снимает
        # позиционного смещения: плечо, стоящее в круге первым, платит за
        # состояние машины после сноса прошлого круга. На свипе «штатный + 500»
        # (срез 31) это дало опорному плечу два выброса из пяти повторов —
        # 14.3 и 14.1 против 10.2 мс/1000 px — и перевернуло знак разницы.
        shift = rep % len(arms)
        for margin in arms[shift:] + arms[:shift]:
            tag = 'штатный' if margin is None else f'{margin:g}'
            name = f'margin_{"default" if margin is None else f"{margin:g}"}'
            log = os.path.join(REPO, '.tmp', log_name(name, rep, args.backend))
            if not args.report_only:
                log = run(1.0, rep, args.ticks, args.delta, args.backend, args.page,
                          extra_env=arm_env(margin), tag=name)
            rows.setdefault(tag, []).append(report(f'{tag} (повтор {rep})', parse(log)))
    summarize(arms, rows)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
