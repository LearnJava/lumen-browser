#!/usr/bin/env python3
"""Что даёт кольцевая дорисовка полосы (BUG-405 срез 32, пункты 43/58 остатка).

Промах скролл-композитора перестал перерисовывать полосу целиком: перекрытие
старой и новой полосы остаётся в текстуре, пасс идёт только по вышедшей вперёд
кромке, а текстура адресуется как ТОР по Y — поэтому ни копии перекрытия, ни
второй текстуры (пункт 61) схема не требует.

Предсказание, снятое замерами срезов 28–30 и проверяемое здесь:

    надбавка(доля) ≈ 2.91 + 8.75 · доля   (срез 29, полоса 1920×2541)

Штатный сдвиг полосы после промаха — ведущий запас, то есть ≈48 % её высоты,
значит надбавка обязана упасть примерно на 39 %, а НЕ на 52 %, которые обещала
арифметика среза 28 (постоянную четверть кольцо не снимает — она платится за
промах как за событие, пункт 63).

Плечи: штатное (кольца нет) против `LUMEN_BAND_RING=1`. Прогоны
интерливед с ВРАЩЕНИЕМ порядка плеч по кругам (пункт 64: фиксированный порядок
на этом стенде стоит ~2 мс/1000 px, больше измеряемого эффекта).

    python scripts/band_ring_census.py --repeats 3 --backend vulkan

Бэкенд задавать обязательно: цифры DX12 и Vulkan несопоставимы (срез 14).

Гейт тождества печатается рядом с числами: `rows N/H` из строки
`page-compose MISS`. У опорного плеча N обязан РАВНЯТЬСЯ H на каждом промахе, у
плеча с кольцом — быть меньше хотя бы на части; путь прокрутки и размер полосы обязаны
совпасть. Иначе плечи мерили разные конфигурации, и разницу читать нельзя.
"""

from __future__ import annotations

import argparse
import os
import re
import statistics
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, 'scripts'))
from band_draw_fraction_census import log_name, med, parse, run  # noqa: E402

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, 'reconfigure'):
        _stream.reconfigure(encoding='utf-8', errors='replace')

ROWS_RE = re.compile(r'page-compose MISS:.*\brows (\d+)/(\d+)\b')

ARMS = [('без кольца', {}), ('кольцо', {'LUMEN_BAND_RING': '1'})]


def rows_of(log_path: str) -> list[tuple[int, int]]:
    """`(перерисовано строк, высота полосы)` по каждому промаху прогона."""
    out: list[tuple[int, int]] = []
    try:
        with open(log_path, encoding='utf-8', errors='replace') as fh:
            for line in fh:
                m = ROWS_RE.search(line)
                if m:
                    out.append((int(m.group(1)), int(m.group(2))))
    except OSError:
        pass
    return out


def report(tag: str, data: dict, rows: list[tuple[int, int]]) -> dict:
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
    drawn = [n for n, _ in rows]
    share = statistics.mean(n / h for n, h in rows) if rows else nan

    print(f'\n=== {tag} ===')
    print(f'  {data["adapter"]}')
    if band:
        print(f'  полоса: {band[0]}x{band[1]} px')
    print(f'  путь {travel:.0f} css px; промахов {len(miss)}, попаданий {len(hit)}, '
          f'вне композитора {len(data["other"])}')
    if rows:
        print(f'  перерисовано строк: {min(drawn)}…{max(drawn)} из {rows[0][1]} '
              f'(в среднем {share:.0%} полосы)')
    if miss:
        print(f'  промах: p50 {med(miss):6.2f}  min {min(miss):6.2f}  max {max(miss):6.2f}')
    if hit:
        print(f'  попад.: p50 {hit_med:6.2f}  min {min(hit):6.2f}  max {max(hit):6.2f}')
    print(f'  надбавка промаха: {med(miss) - hit_med if miss and hit else nan:6.2f} мс')
    print(f'  на 1000 px пути: надбавка {over_kpx:6.2f} мс, полная {total_kpx:6.2f} мс')
    for reason, n in data['skips'].items():
        print(f'  skip x{n}: {reason}')
    return {'band_h': band[1] if band else 0, 'band_w': band[0] if band else 0,
            'miss_n': len(miss), 'travel': travel, 'hit_med': hit_med,
            'surcharge': med(miss) - hit_med if miss and hit else nan,
            'over_kpx': over_kpx, 'total_kpx': total_kpx,
            'rows': rows, 'share': share, 'miss_raw': miss, 'hit_raw': hit}


def _col(rows: list[dict], key: str) -> list[float]:
    return [r[key] for r in rows if r[key] == r[key]]


def summarize(rows: dict[str, list[dict]]) -> None:
    print('\n' + '=' * 84)
    print('Цена прокрутки с кольцом и без (среднее плеча, мс на 1000 px пути).')
    print('«разброс» — max−min повторов ТОГО ЖЕ плеча: разница меньше него ничего')
    print('не решает (пункт 64 — свипы этого бага уже спотыкались об это).\n')
    print('плечо      | строк | промахов | надбавка ср (разброс) | полная ср (разброс)')
    best: dict[str, dict] = {}
    for tag, _ in ARMS:
        rs = rows.get(tag, [])
        over, total = _col(rs, 'over_kpx'), _col(rs, 'total_kpx')
        if not total:
            print(f'{tag:10s} | нет кадров композитора')
            continue
        shares = _col(rs, 'share')
        best[tag] = {'over': statistics.mean(over) if over else float('nan'),
                     'total': statistics.mean(total),
                     'over_spread': max(over) - min(over) if len(over) > 1 else 0.0,
                     'total_spread': max(total) - min(total) if len(total) > 1 else 0.0,
                     'surcharge': statistics.mean(_col(rs, 'surcharge'))}
        print(f'{tag:10s} | {statistics.mean(shares):4.0%} | '
              f'{sorted({r["miss_n"] for r in rs})!s:8s} | '
              f'{best[tag]["over"]:9.2f} ({best[tag]["over_spread"]:5.2f}) '
              f'{"":5s}| {best[tag]["total"]:7.2f} ({best[tag]["total_spread"]:5.2f})')

    # Гейт тождества: опорное плечо обязано рисовать полосу ЦЕЛИКОМ, штатное —
    # частично; путь и размер полосы обязаны совпасть.
    full = all(n == h for r in rows.get('без кольца', []) for n, h in r['rows'])
    partial = any(n < h for r in rows.get('кольцо', []) for n, h in r['rows'])
    travels = sorted({round(r['travel']) for rs in rows.values() for r in rs})
    heights = sorted({r['band_h'] for rs in rows.values() for r in rs if r['band_h']})
    print(f'\nгейт тождества: путь {travels} css px, полоса {heights} px, '
          f'опорное плечо целиком {full}, кольцо частично {partial}')
    if not full or not partial:
        print('  ВНИМАНИЕ: плечи не различаются механизмом — числа ничего не значат')
    if len(heights) > 1 or (len(travels) > 1 and (max(travels) - min(travels)) > 0.02 * max(travels)):
        print('  ВНИМАНИЕ: плечи мерили разные конфигурации')

    if len(best) < 2:
        return
    ref, new = best['без кольца'], best['кольцо']
    print('\nПротив плеча без кольца (знак «−» = кольцо дешевле):')
    for name, key in (('надбавка', 'over'), ('полная', 'total')):
        d = new[key] - ref[key]
        spread = max(new[f'{key}_spread'], ref[f'{key}_spread'])
        verdict = 'решается над разбросом' if abs(d) > spread else 'НЕ решается над разбросом'
        print(f'  {name:9s}: {d:+6.2f} мс/1000 px ({100.0 * d / ref[key]:+5.1f} %), '
              f'разброс {spread:.2f} — {verdict}')
    # Модель среза 29: надбавка(доля) = 2.91 + 8.75·доля на полосе 1920×2541.
    share = statistics.mean(_col(rows.get('кольцо', []), 'share')) if rows.get('кольцо') else 0.0
    if share:
        pred = 1.0 - (2.91 + 8.75 * share) / (2.91 + 8.75)
        got = 1.0 - new['surcharge'] / ref['surcharge'] if ref['surcharge'] else float('nan')
        print(f'\nнадбавка ОДНОГО промаха: {ref["surcharge"]:.2f} → {new["surcharge"]:.2f} мс '
              f'({100 * got:+.0f} %); модель среза 29 при доле {share:.0%} обещала '
              f'{-100 * pred:+.0f} %')


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('--repeats', type=int, default=3, help='повторов на плечо (интерливед)')
    ap.add_argument('--ticks', type=int, default=120)
    ap.add_argument('--delta', type=float, default=120.0, help='CSS px на щелчок колеса')
    ap.add_argument('--backend', default='vulkan')
    ap.add_argument('--page', default='samples/bench-static-scroll.html')
    ap.add_argument('--report-only', action='store_true')
    args = ap.parse_args()

    rows: dict[str, list[dict]] = {}
    for rep in range(args.repeats):
        # Порядок плеч вращается по кругам (пункт 64).
        shift = rep % len(ARMS)
        for tag, env in ARMS[shift:] + ARMS[:shift]:
            name = 'ring_off' if not env else 'ring_on'
            log = os.path.join(REPO, '.tmp', log_name(name, rep, args.backend))
            if not args.report_only:
                log = run(1.0, rep, args.ticks, args.delta, args.backend, args.page,
                          extra_env=env, tag=name)
            rows.setdefault(tag, []).append(
                report(f'{tag} (повтор {rep})', parse(log), rows_of(log)))
    summarize(rows)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
