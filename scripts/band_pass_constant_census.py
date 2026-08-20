#!/usr/bin/env python3
"""Из чего состоит ПОСТОЯННАЯ статья надбавки промаха полосы (BUG-405 срез 30).

Вопрос — пункт 62 остатка бага. Срез 29 разложил надбавку промаха на
`2.91 мс + 8.75 · доля перерисованных строк`: три четверти цены идут за
строками (их адресует инкрементальная дорисовка, пункт 43), а четверть не идёт
никуда — она платится, даже если рисовать нечего. Кандидаты пункта 62:
`LoadOp::Clear` цвета всей полосы, `Clear` полноразмерного depth и постоянная
цена самого пасса (пункт 5). Ни один не проверен.

Метод — свип ПЛЕЧ при неизменных окне, странице, шаге прокрутки и размере
полосы, на МАЛОЙ доле перерисовки (`LUMEN_BAND_DRAW_FRACTION`, по умолчанию
0.05): на ней от статьи «строки» остаётся почти ничего, и разница между
плечами — это разница постоянных статей, а не рисования.

    base   доля 0.05, штатные клиры          → вся постоянная статья
    color  доля 0.05, цвет полосы `Load`     → она же без клира цвета
    depth  доля 0.05, depth полосы `Load`    → она же без клира глубины
    both   доля 0.05, оба `Load`             → остаток: пасс + submit + прочее
    full   доля 1.0,  штатные клиры          → якорь: полная надбавка промаха

Отсюда цена клира цвета = base − color, цена клира depth = base − depth, а
`both` показывает, сколько остаётся, когда сняты оба (и сходится ли сумма).

Второй вопрос — чем ОСТАТОК является: постоянной ценой пасса (пункт 5, от
размера цели не зависит) или работой на пиксель ЦЕЛИ (растёт с площадью
полосы). Различает `--margins`: те же плечи малой доли на полосе другого
запаса (`LUMEN_BAND_MARGIN_CSS`, рычаг переписи среза 27).

    python scripts/band_pass_constant_census.py --repeats 3 --backend vulkan
    python scripts/band_pass_constant_census.py --arms base --margins 400 1500 \
        --repeats 3 --backend vulkan   # base = штатный запас окна

Запас меньше 0.25 вьюпорта композитор отключает целиком (`BAND_MIN_MARGIN_RATIO`),
и плечо возвращает НОЛЬ кадров с диагностикой `skip: вьюпорт не оставляет запаса
в лимите текстуры` — на окне 1017 css это всё, что ниже ~254 px.

Прогоны ИНТЕРЛИВЕД (round-robin по плечам), сравнение по МИНИМУМУ надбавки из
повторов: разброс прогонов на этом стенде ±4 мс, то есть больше ожидаемого
эффекта (`docs/perf-method.md`). Бэкенд задавать обязательно — цифры DX12 и
Vulkan несопоставимы (срез 14).

Гейт тождества плеч печатается вместе с числами: доля и имя плеча из строки
`page-compose MISS … frac F, load ARM` плюс счётчики работы кадра полосы
(`draw`/`filt`). Работа ОБЯЗАНА совпадать у всех плеч кроме `full`: рычаг
снимает клир, а не рисование. Если она разошлась — плечи мерили разные
конфигурации, и числам верить нельзя.

**Ловушка плеча `depth`:** без клира глубины пасс стартует со старыми
значениями, и часть фрагментов отбраковывается тестом глубины — то есть плечо
удешевляет не только клир. Счётчики этого не видят (число draw-команд не
меняется), поэтому плечо и меряется на малой доле, где рисования почти нет;
на доле 1.0 его число было бы завышено.
"""

from __future__ import annotations

import argparse
import os
import statistics
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, 'scripts'))
from band_draw_fraction_census import log_name, med, parse, run  # noqa: E402

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, 'reconfigure'):
        _stream.reconfigure(encoding='utf-8', errors='replace')

# Плечо: имя → (доля перерисовки, доп. окружение, ожидаемое имя в логе).
# `None` в доле означает «малая доля из --fraction».
ARMS: dict[str, tuple[float | None, dict[str, str], str]] = {
    'base': (None, {}, 'none'),
    'color': (None, {'LUMEN_BAND_PASS_LOAD': 'color'}, 'color'),
    'depth': (None, {'LUMEN_BAND_PASS_LOAD': 'depth'}, 'depth'),
    'both': (None, {'LUMEN_BAND_PASS_LOAD': 'both'}, 'both'),
    'full': (1.0, {}, 'none'),
}


def area_arm(margin_css: float) -> tuple[str, tuple[float | None, dict[str, str], str]]:
    """Плечо «та же малая доля, но полоса другого размера».

    Отвечает на второй вопрос пункта 62: постоянная статья — это цена ПАССА
    (не зависит от размера цели) или работа НА ПИКСЕЛЬ цели (растёт с площадью
    полосы)? Клиры к тому моменту уже сняты с подозрения, а различить эти два
    можно только размером полосы, поэтому здесь тот же рычаг запаса, которым
    свипала полосу перепись среза 27 (`LUMEN_BAND_MARGIN_CSS`).
    """
    return (f'area{margin_css:g}',
            (None, {'LUMEN_BAND_MARGIN_CSS': repr(float(margin_css))}, 'none'))


def report(tag: str, data: dict) -> dict:
    """Одно плечо: надбавка промаха плюс гейт тождества (доля, плечо, работа)."""
    miss = [ms for ms, _ in data['miss']]
    hit = [ms for ms, _ in data['hit']]
    band = data['band']
    work = {k: med(v) for k, v in data['work'].items()}
    print(f'\n=== плечо {tag} ===')
    print(f'  {data["adapter"]}')
    if band:
        print(f'  полоса: {band[0]}x{band[1]} px   frac {data["fracs"]}   load {data["loads"]}')
    print(f'  промахов {len(miss)}, попаданий {len(hit)}')
    print('  работа кадра полосы (p50): ' + '  '.join(
        f'{k} {v:.0f}' for k, v in work.items() if v == v))
    if miss:
        print(f'  промах: p50 {med(miss):6.2f}  min {min(miss):6.2f}  max {max(miss):6.2f}')
    if hit:
        print(f'  попад.: p50 {med(hit):6.2f}  min {min(hit):6.2f}  max {max(hit):6.2f}')
    surcharge = med(miss) - med(hit) if miss and hit else float('nan')
    if miss and hit:
        print(f'  надбавка промаха: {surcharge:.2f} мс')
    for reason, n in data['skips'].items():
        print(f'  skip x{n}: {reason}')
    return {'miss_n': len(miss), 'hit_n': len(hit), 'surcharge': surcharge,
            'work': work, 'fracs': data['fracs'], 'loads': data['loads'],
            'band_h': band[1] if band else 0, 'miss_raw': miss, 'hit_raw': hit}


def summarize(arms: list[str], rows: dict[str, list[dict]], fraction: float) -> None:
    """Сводка: надбавка по плечам и разложение постоянной статьи по клирам."""
    print('\n' + '=' * 78)
    print(f'Надбавка промаха по плечам (малая доля {fraction:g}, кроме full).')
    print('«работа» обязана совпадать у всех плеч малой доли: рычаг снимает клир,')
    print('а не рисование. Разошлась — плечи мерили разные конфигурации.\n')
    print('плечо | мин   | объед. |     повторы      | draw | filt | load  | полоса')
    best: dict[str, float] = {}
    pooled: dict[str, float] = {}
    for a in arms:
        rs = rows.get(a, [])
        vals = [r['surcharge'] for r in rs if r['surcharge'] == r['surcharge']]
        if not vals:
            print(f'{a:5s} | нет данных')
            continue
        best[a] = min(vals)
        miss = [ms for r in rs for ms in r['miss_raw']]
        hit = [ms for r in rs for ms in r['hit_raw']]
        if miss and hit:
            pooled[a] = statistics.median(miss) - statistics.median(hit)
        w = {k: med([r['work'].get(k, float('nan')) for r in rs])
             for k in ('cmd_draw', 'plan_filt')}
        loads = sorted({v for r in rs for v in r['loads']})
        heights = sorted({r['band_h'] for r in rs})
        print(f'{a:5s} | {best[a]:5.2f} | {pooled.get(a, float("nan")):6.2f} | '
              f'{", ".join(f"{v:.1f}" for v in vals):16s} | {w["cmd_draw"]:4.0f} | '
              f'{w["plan_filt"]:4.0f} | {",".join(loads):5s} | '
              f'{",".join(str(h) for h in heights)}')

    if 'base' not in best:
        return
    print('\nРазложение постоянной статьи (по минимумам):')
    base = best['base']
    print(f'  вся постоянная статья (base): {base:.2f} мс')
    for arm, name in (('color', 'клир цвета полосы'), ('depth', 'клир depth полосы')):
        if arm in best:
            print(f'  {name}: {base - best[arm]:+.2f} мс  (плечо {arm} {best[arm]:.2f})')
    if 'both' in best:
        print(f'  остаток без обоих клиров (both): {best["both"]:.2f} мс '
              f'— пасс, submit и всё, что не зависит от содержимого')
        if 'color' in best and 'depth' in best:
            summed = (base - best['color']) + (base - best['depth']) + best['both']
            print(f'  сходимость: клиры + остаток = {summed:.2f} против base {base:.2f} мс')
    area = [a for a in arms if a.startswith('area')]
    if area:
        print('\nПостоянная статья против РАЗМЕРА полосы (та же доля, другой запас):')
        print('  если она — цена пасса, число не зависит от высоты; если работа на')
        print('  пиксель цели — растёт вместе с ней.')
        for a in ['base', *area]:
            if a not in best:
                continue
            h = max((r['band_h'] for r in rows.get(a, []) if r['band_h']), default=0)
            per = f'{1000.0 * best[a] / h:.3f} мс на 1000 строк' if h else '—'
            print(f'  {a:8s} полоса {h:5d} px: {best[a]:5.2f} мс   ({per})')
    if 'full' in best:
        print(f'  доля постоянной статьи в полной надбавке: '
              f'{100.0 * base / best["full"]:.0f} % ({base:.2f} из {best["full"]:.2f} мс)')


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('--arms', nargs='+', default=list(ARMS),
                    choices=list(ARMS), help='какие плечи прогнать')
    ap.add_argument('--margins', type=float, nargs='*', default=[],
                    help='запасы полосы (CSS px) — плечи «та же доля, полоса другого '
                         'размера»: цена ПАССА против работы на пиксель цели')
    ap.add_argument('--fraction', type=float, default=0.05,
                    help='малая доля перерисовки для плеч кроме full')
    ap.add_argument('--repeats', type=int, default=3, help='повторов на плечо (интерливед)')
    ap.add_argument('--ticks', type=int, default=120)
    ap.add_argument('--delta', type=float, default=120.0, help='CSS px на щелчок колеса')
    ap.add_argument('--backend', default='vulkan')
    ap.add_argument('--page', default='samples/bench-static-scroll.html')
    ap.add_argument('--report-only', action='store_true')
    args = ap.parse_args()

    arms = list(args.arms)
    for margin in args.margins:
        name, spec = area_arm(margin)
        ARMS[name] = spec
        arms.append(name)

    rows: dict[str, list[dict]] = {a: [] for a in arms}
    for rep in range(args.repeats):
        # Порядок плеч ВРАЩАЕТСЯ по кругам (срез 31, п. 64): интерливед сам по
        # себе не снимает позиционного смещения — плечо, стоящее в круге
        # первым, платит за состояние машины после сноса прошлого круга, и на
        # этом стенде такая плата (~2 мс/1000 px пути) больше измеряемого
        # эффекта. Числа среза 30 сняты ДО вращения.
        shift = rep % len(arms)
        for arm in arms[shift:] + arms[:shift]:
            frac, env, expect = ARMS[arm]
            frac = args.fraction if frac is None else frac
            tag = f'pass_{arm}'
            log = os.path.join(REPO, '.tmp', log_name(tag, rep, args.backend))
            if not args.report_only:
                log = run(frac, rep, args.ticks, args.delta, args.backend, args.page,
                          extra_env=env, tag=tag)
            data = parse(log)
            r = report(f'{arm} (повтор {rep})', data)
            if r['loads'] and r['loads'] != [expect]:
                print(f'  ВНИМАНИЕ: плечо {arm} ожидало load {expect}, лог даёт {r["loads"]}')
            rows[arm].append(r)
    summarize(arms, rows, args.fraction)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
