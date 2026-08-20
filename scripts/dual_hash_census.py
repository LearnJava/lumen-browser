#!/usr/bin/env python3
"""Цена второго O(n)-хэша кадра (BUG-405 срез 35, пункт 70 остатка).

Срез 34 разложил кадр ПОПАДАНИЯ полосы и назвал крупнейшую его статью: два
хэша одного и того же display list-а — тотальный хэш кадра (skip-identical) и
scroll-инвариантный ключ полосы — стоили вместе 0.52–1.05 мс против 0.54–0.85
мс всего композитного пасса. Срез 35 свёл их в ОДИН проход по списку (свёртка
команды считается один раз и уходит в оба хешера), и здесь эта правка меряется
на живом окне.

Плечи (оба под `LUMEN_FRAME_LOG=2`, иначе статьи не печатаются):

* `один проход` — штатное поведение после среза 35;
* `два прохода` — `LUMEN_NO_DUAL_HASH=1`, поведение до среза 35.

Гейт — СЧЁТЧИК, а не секундомер кадра: обе ветки печатают строку
`frame-hash: Nms (… cmds, <режим>)`, измеряющую ровно ту работу, которую правка
меняет. Полное время кадра здесь не разрешает правку (разброс повторов на этом
стенде больше её эффекта, п. 37), поэтому оно печатается справочно.

    python scripts/dual_hash_census.py --repeats 3 --backend vulkan

Прогоны интерливед с ВРАЩЕНИЕМ порядка плеч (п. 64: фиксированный порядок на
этом стенде стоит больше измеряемого эффекта). Бэкенд задавать обязательно:
цифры DX12 и Vulkan несопоставимы (срез 14).
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
HASH_RE = re.compile(r'frame-hash: ([\d.]+)ms \((\d+) \+ (\d+) cmds, ([^)]+)\)')
ADAPTER_RE = re.compile(r'\[wgpu\] adapter: (.*)')

ARMS = [('один проход', {}), ('два прохода', {'LUMEN_NO_DUAL_HASH': '1'})]


def parse(log_path: str) -> dict:
    """Кадры прокрутки: цена хэширования и полное время кадра.

    Строка `frame-hash` печатается ДО решения о композиции, то есть на каждом
    кадре — и на попадании, и на промахе, и на монолитном.
    """
    frames: list[dict] = []
    cur: dict = {}
    hit = False
    ys: list[float] = []
    adapter = ''
    modes: set[str] = set()
    cmds = 0
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
                cmds = int(m.group(2)) + int(m.group(3))
                modes.add(m.group(4))
                continue
            m = TOTAL_RE.search(line)
            if m:
                cur['total'] = float(m.group(1))
                cur['hit'] = hit
                ys.append(float(m.group(2)))
                frames.append(cur)
                cur = {}
                hit = False
    return {'frames': frames, 'travel': (max(ys) - min(ys)) if ys else 0.0,
            'adapter': adapter, 'modes': modes, 'cmds': cmds}


def med(xs: list[float]) -> float:
    return statistics.median(xs) if xs else float('nan')


def report(tag: str, data: dict) -> dict:
    """Одно плечо: цена хэширования на кадр и полное время кадра."""
    # Первые кадры после загрузки — прогрев полосы и атласа; отбрасываем
    # ровно столько же в обоих плечах.
    frames = data['frames'][3:]
    hits = [f for f in frames if f.get('hit')]
    hashes = [f['hash'] for f in frames if 'hash' in f]
    hit_hashes = [f['hash'] for f in hits if 'hash' in f]
    tot = [f['total'] for f in frames]
    print(f'\n=== {tag} ===')
    print(f'  {data["adapter"]}')
    print(f'  кадров {len(frames)} (попаданий {len(hits)}), '
          f'путь {data["travel"]:.0f} css px, {data["cmds"]} cmds, '
          f'режим {"/".join(sorted(data["modes"])) or "?"}')
    if hashes:
        print(f'  хэш кадра: p50 {med(hashes):.3f}  min {min(hashes):.3f} мс')
    if hit_hashes:
        print(f'  хэш на попадании: p50 {med(hit_hashes):.3f} мс')
    if tot:
        print(f'  кадр целиком: p50 {med(tot):.2f} мс')
    return {'hash_p50': med(hashes), 'hash_min': min(hashes) if hashes else float('nan'),
            'hit_hash_p50': med(hit_hashes), 'total_p50': med(tot),
            'n': len(frames), 'travel': data['travel'], 'modes': data['modes'],
            'raw': hashes}


def summarize(rows: dict[str, list[dict]]) -> None:
    """Сводка по плечам: счётчик хэша решает, полное время справочно."""
    print('\n' + '=' * 78)
    print('Плечо        | хэш p50 | хэш min | хэш на попад. | кадр p50 | кадров')
    pooled: dict[str, float] = {}
    for tag, _ in ARMS:
        rs = rows.get(tag, [])
        if not rs:
            continue
        raw = [x for r in rs for x in r['raw']]
        pooled[tag] = med(raw)
        print(f'{tag:12s} | {pooled[tag]:7.3f} | '
              f'{min(r["hash_min"] for r in rs):7.3f} | '
              f'{med([r["hit_hash_p50"] for r in rs]):13.3f} | '
              f'{med([r["total_p50"] for r in rs]):8.2f} | '
              f'{sum(r["n"] for r in rs):6d}')
    if len(pooled) == 2:
        one, two = pooled['один проход'], pooled['два прохода']
        print(f'\nВторой обход списка стоил {two - one:+.3f} мс на кадр '
              f'({(one - two) / two * 100:+.0f} % к цене хэширования).')
        print('Гейт тождества плеч: путь прокрутки и число кадров обязаны')
        print('совпасть, иначе плечи мерили разную работу.')


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('--repeats', type=int, default=3, help='повторов на плечо (интерливед)')
    ap.add_argument('--ticks', type=int, default=60)
    ap.add_argument('--delta', type=float, default=120.0, help='CSS px на щелчок колеса')
    ap.add_argument('--backend', default='vulkan')
    ap.add_argument('--page', default='samples/bench-text-scroll.html')
    args = ap.parse_args()

    rows: dict[str, list[dict]] = {tag: [] for tag, _ in ARMS}
    for rep in range(args.repeats):
        shift = rep % len(ARMS)
        for tag, env in ARMS[shift:] + ARMS[:shift]:
            name = 'dual_one' if not env else 'dual_two'
            log = run(1.0, rep, args.ticks, args.delta, args.backend, args.page,
                      extra_env=env, tag=name)
            rows[tag].append(report(f'{tag} (повтор {rep})', parse(log)))
    summarize(rows)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
