#!/usr/bin/env python3
"""Перепись цены кадра ПОПАДАНИЯ полосы (BUG-405 срез 34, пункт 68 остатка).

Пункт 68 сказал: прокрутка текстовой страницы дорога попаданиями (84 % цены при
надбавке промахов 5.06 из 31.5 мс/1000 px), а разбивки у попадания нет. Здесь
она снимается — и заодно проверяется само число 31.5, потому что все замеры
срезов 26–33 сделаны при `LUMEN_FRAME_LOG=2`, то есть под пофазным логом.

Плечи:

* `лог 1` — `LUMEN_FRAME_LOG=1`: кадр печатает три строки ПОСЛЕ своего таймера,
  внутри кадра инструментов нет. Это честная цена кадра прокрутки.
* `лог 2` — `LUMEN_FRAME_LOG=2`: тот же путь плюс пофазный блок рендерера
  (~12 строк на кадр). Его строки печатаются ВНУТРИ окна, которое меряет
  `[frame] total` шелла, поэтому разница плеч и есть цена инструмента.

Разбивка берётся с плеча «лог 2» (только там она существует) и печатается
вместе с невязкой — временем, которое кадр провёл, но ни одна статья не назвала.
Невязка на попадании — это и есть печать пофазного блока.

    python scripts/hit_frame_census.py --repeats 3 --backend vulkan

Прогоны интерливед с ВРАЩЕНИЕМ порядка плеч (пункт 64: фиксированный порядок на
этом стенде стоит больше измеряемого эффекта). Бэкенд задавать обязательно:
цифры DX12 и Vulkan несопоставимы (срез 14).

Гейт тождества плеч: путь прокрутки (`travel`) и число кадров обязаны совпасть,
иначе плечи мерили разную работу.
"""

from __future__ import annotations

import argparse
import os
import re
import statistics
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, 'scripts'))
from band_draw_fraction_census import run  # noqa: E402

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, 'reconfigure'):
        _stream.reconfigure(encoding='utf-8', errors='replace')

TOTAL_RE = re.compile(r'\[frame\] total\s+([\d.]+)ms\s+\(scroll_y ([\-\d.]+)')
TOP_RE = re.compile(
    r'\[frame\]\s+top: scroll ([\d.]+) sda ([\d.]+) anim ([\d.]+) js ([\d.]+) '
    r'build ([\d.]+) paint ([\d.]+) \(лог ([\d.]+)\)')
HASH_RE = re.compile(r'frame-hash: ([\d.]+)ms')
# Срез 35 свёл ключ полосы в общий проход по списку: отдельной статьи `key` в
# строке больше нет, вся цена хэширования печатается строкой `frame-hash`.
COMPOSE_RE = re.compile(
    r'compose-top: skip ([\d.]+) geom ([\d.]+) split ([\d.]+) band ([\d.]+)')
WGPU_RE = re.compile(r'\[frame:wgpu\] total\s+([\d.]+)ms')
ADAPTER_RE = re.compile(r'\[wgpu\] adapter: (.*)')
# BUG-405 срез 44: зазор ДО старта секундомера `ComposeMarks`
# (`PRE_MARKS_NANOS`) — печатается той же строкой `paint:`, что и `обёртка`.
PRE_MARKS_RE = re.compile(r'предметки ([\-\d.]+)')
# BUG-405 срез 44: зазор `marks[4]` -> вызов `render`/`render_with_anim` в
# шелле (снимок/set_*-подготовка ДО обращения к рендереру).
PRE_CALL_RE = re.compile(r'предвызов ([\-\d.]+)')
# BUG-405 срез 44: зазор между решением `overlay_cache_step` и вызовом
# `render_impl` внутри `compose_page` (сборка seg_content/compose_overlay) —
# второй кандидат п. 84, названный самим текстом пункта.
POST_CACHE_RE = re.compile(r'послекэша ([\-\d.]+)')

ARMS = [('лог 1', '1'), ('лог 2', '2')]

# Статьи кадра попадания в порядке печати сводки. `лог` — измеренная цена
# печати самого пофазного блока (счётчик `FRAME_LOG_NANOS`), `предметки` —
# зазор до старта `ComposeMarks` (срез 44), `невязка` — то, что не назвала ни
# одна статья.
COLUMNS = [
    'shell', 'hash', 'band-реш.', 'пасс', 'лог', 'предметки', 'послекэша', 'предвызов', 'невязка',
]


def parse(log_path: str) -> dict:
    """Кадры прокрутки лога: полное время плюс статьи (если уровень 2).

    Кадр попадания опознаётся строкой `page-compose HIT`, стоящей ПЕРЕД его
    `[frame] total`; на уровне 1 таких строк нет вовсе, и тогда возвращается
    только распределение полных времён.
    """
    frames: list[dict] = []
    cur: dict = {}
    hit = False
    ys: list[float] = []
    adapter = ''
    with open(log_path, encoding='utf-8', errors='replace') as fh:
        for line in fh:
            m = ADAPTER_RE.search(line)
            if m:
                adapter = m.group(1).strip()
                continue
            if 'page-compose HIT' in line:
                hit = True
                continue
            if 'page-compose MISS' in line:
                hit = False
                continue
            m = HASH_RE.search(line)
            if m:
                cur['hash'] = float(m.group(1))
                continue
            m = COMPOSE_RE.search(line)
            if m:
                g = [float(x) for x in m.groups()]
                cur['band-реш.'] = g[0] + g[1] + g[2] + g[3]
                continue
            m = WGPU_RE.search(line)
            if m:
                # На промахе таких строк две (полоса и композит) — берём сумму.
                cur['пасс'] = cur.get('пасс', 0.0) + float(m.group(1))
                continue
            # BUG-405 срез 44: три поля живут в ОДНОЙ строке `paint:` — три
            # независимых `search` без `continue` между ними, иначе первое
            # совпадение съедало бы проверку двух остальных на той же строке.
            matched_paint_extra = False
            m = PRE_MARKS_RE.search(line)
            if m:
                cur['предметки'] = float(m.group(1))
                matched_paint_extra = True
            m = PRE_CALL_RE.search(line)
            if m:
                cur['предвызов'] = float(m.group(1))
                matched_paint_extra = True
            m = POST_CACHE_RE.search(line)
            if m:
                cur['послекэша'] = float(m.group(1))
                matched_paint_extra = True
            if matched_paint_extra:
                continue
            m = TOP_RE.search(line)
            if m:
                g = [float(x) for x in m.groups()]
                cur['shell'] = g[0] + g[1] + g[2] + g[3] + g[4]
                cur['paint'] = g[5]
                cur['лог'] = g[6]
                continue
            m = TOTAL_RE.search(line)
            if m:
                cur['total'] = float(m.group(1))
                cur['hit'] = hit
                ys.append(float(m.group(2)))
                if 'paint' in cur:
                    named = sum(
                        cur.get(k, 0.0) for k in
                        ('hash', 'band-реш.', 'пасс', 'лог', 'предметки', 'послекэша', 'предвызов')
                    )
                    cur['невязка'] = max(cur['paint'] - named, 0.0)
                    cur['честный'] = max(cur['total'] - cur.get('лог', 0.0), 0.0)
                frames.append(cur)
                cur = {}
                hit = False
    travel = (max(ys) - min(ys)) if ys else 0.0
    return {'frames': frames, 'travel': travel, 'adapter': adapter}


def med(xs: list[float]) -> float:
    return statistics.median(xs) if xs else float('nan')


def report(tag: str, data: dict) -> dict:
    """Одно плечо: полное время кадра и — если есть — его статьи."""
    frames = data['frames']
    # Первые кадры после загрузки — прогрев полосы и атласа, к цене прокрутки
    # они не относятся; отбрасываем ровно столько же в обоих плечах.
    frames = frames[3:]
    tot = [f['total'] for f in frames]
    hits = [f for f in frames if f.get('hit')]
    hit_tot = [f['total'] for f in hits]
    print(f'\n=== {tag} ===')
    print(f'  {data["adapter"]}')
    print(f'  кадров {len(frames)} (попаданий {len(hits)}), '
          f'путь {data["travel"]:.0f} css px')
    if tot:
        print(f'  кадр целиком: p50 {med(tot):6.2f}  min {min(tot):6.2f}  '
              f'p90 {statistics.quantiles(tot, n=10)[8] if len(tot) > 9 else float("nan"):6.2f}')
    if hit_tot:
        print(f'  попадание:    p50 {med(hit_tot):6.2f}  min {min(hit_tot):6.2f}')
    fair = [f['честный'] for f in hits if 'честный' in f]
    if fair:
        print(f'  попадание без лога: p50 {med(fair):6.2f}')
    cols = {}
    if hits and 'hash' in hits[0]:
        for k in COLUMNS:
            cols[k] = med([f.get(k, 0.0) for f in hits])
        print('  статьи попадания (p50): ' +
              '  '.join(f'{k} {v:.2f}' for k, v in cols.items()))
    return {'total_p50': med(tot), 'hit_p50': med(hit_tot), 'hit_n': len(hits),
            'n': len(frames), 'travel': data['travel'], 'cols': cols,
            'raw': tot, 'hit_raw': hit_tot, 'fair_p50': med(fair)}


def summarize(rows: dict[str, list[dict]], delta: float) -> None:
    """Сводка: цена инструмента и разложение честного кадра попадания."""
    print('\n' + '=' * 78)
    per_1000 = 1000.0 / delta  # кадров на 1000 css px пути
    print('Плечо      | кадр p50 | объед. p50 | мс/1000 px | кадров')
    pooled: dict[str, float] = {}
    for tag, _ in ARMS:
        rs = rows.get(tag, [])
        if not rs:
            continue
        raw = [x for r in rs for x in r['raw']]
        p50 = med([r['total_p50'] for r in rs])
        pooled[tag] = med(raw)
        print(f'{tag:10s} | {p50:8.2f} | {pooled[tag]:10.2f} | '
              f'{pooled[tag] * per_1000:10.1f} | {sum(r["n"] for r in rs):6d}')
    if len(pooled) == 2:
        tax = pooled['лог 2'] - pooled['лог 1']
        print(f'\nЦена пофазного лога: {tax:+.2f} мс на кадр '
              f'({tax / pooled["лог 1"] * 100:+.0f} % к честному кадру, '
              f'{tax * per_1000:+.1f} мс/1000 px).')
        print('Все замеры срезов 26–33 сняты на плече «лог 2» — то есть с этой')
        print('надбавкой внутри каждого кадра.')
    rs2 = rows.get('лог 2', [])
    if rs2 and rs2[0]['cols']:
        print('\nСтатьи кадра попадания (плечо «лог 2», p50 по повторам):')
        for k in COLUMNS:
            v = med([r['cols'].get(k, float('nan')) for r in rs2])
            print(f'  {k:10s} {v:5.2f} мс')
        print('«лог» — ИЗМЕРЕННАЯ цена печати пофазного блока (счётчик')
        print('FRAME_LOG_NANOS), «невязка» — остаток, который не назвала ни одна')
        print('статья. Сверка плеч: «лог» обязан сойтись с разницей плеч выше.')
        fair = med([r['fair_p50'] for r in rs2 if r['fair_p50'] == r['fair_p50']])
        print(f'\nЧестный кадр попадания (total − лог): {fair:.2f} мс.')


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('--repeats', type=int, default=3, help='повторов на плечо (интерливед)')
    ap.add_argument('--ticks', type=int, default=60)
    ap.add_argument('--delta', type=float, default=120.0, help='CSS px на щелчок колеса')
    ap.add_argument('--backend', default='vulkan')
    ap.add_argument('--page', default='samples/bench-text-scroll.html')
    ap.add_argument('--report-only', action='store_true')
    args = ap.parse_args()

    rows: dict[str, list[dict]] = {tag: [] for tag, _ in ARMS}
    for rep in range(args.repeats):
        shift = rep % len(ARMS)
        for tag, lvl in ARMS[shift:] + ARMS[:shift]:
            name = f'hit_{lvl}'
            log = os.path.join(REPO, '.tmp', f'band_{name}_r{rep}_{args.backend}.log')
            if not args.report_only:
                log = run(1.0, rep, args.ticks, args.delta, args.backend, args.page,
                          extra_env={'LUMEN_FRAME_LOG': lvl}, tag=name)
            rows[tag].append(report(f'{tag} (повтор {rep})', parse(log)))
    summarize(rows, args.delta)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
