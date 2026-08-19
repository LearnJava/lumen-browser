#!/usr/bin/env python3
"""Перепись цены ПРОМАХА полосы скролл-композитора по её площади (BUG-405 срез 27).

Вопрос, ради которого написан: пункт 43 остатка бага предлагает не
перерисовывать на промахе всю полосу, а сдвигать её и дорисовывать только
вышедшую кромку. Правка дорогая (полосе нужен либо второй буфер, либо
кольцевая адресация плюс частичный пасс), поэтому сначала нужно число: какая
доля цены промаха пропорциональна ПЛОЩАДИ полосы, а какая постоянна. Инкрементальная
дорисовка снимает только первую, и ровно ту её часть, что приходится на
переиспользуемый перекрыв.

Метод — свип по высоте полосы при неизменных окне, странице и шаге прокрутки.
Высоту задаёт `LUMEN_BAND_MARGIN_CSS` (потолок запаса с каждой стороны,
`band_geometry`): полоса = вьюпорт + 2×запас. Для каждого значения печатается
медиана кадра-промаха, медиана кадра-попадания и надбавка промаха
(промах − попадание) — та самая статья, которую правка адресует.

Стенд — `samples/bench-static-scroll.html`: содержимое статично, скриптов нет,
то есть ключ полосы стабилен и все промахи вызваны только уходом вьюпорта
(живой сайт дал бы вдобавок промахи по смене содержимого, п. 51 остатка).

    python scripts/band_miss_census.py --margins 300 500 768 1200 --backend vulkan

Бэкенд задавать обязательно: цифры DX12 и Vulkan несопоставимы (срез 14).
"""

from __future__ import annotations

import argparse
import os
import re
import statistics
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, 'scripts'))
from scroll_perf import Client, free_port  # noqa: E402

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, 'reconfigure'):
        _stream.reconfigure(encoding='utf-8', errors='replace')

TOTAL_RE = re.compile(r'\[frame\] total\s+([\d.]+)ms\s+\(scroll_y ([\-\d.]+), dl (\d+) cmds\)')
MISS_RE = re.compile(r'page-compose MISS: band y=([\-\d.]+)\.\.([\-\d.]+) css \((\d+)x(\d+) px')
HIT_RE = re.compile(r'page-compose HIT')
SKIP_RE = re.compile(r'page-compose skip: (.*)')
ADAPTER_RE = re.compile(r'\[wgpu\] adapter: (.*)')


def run(margin: float | None, ticks: int, delta: float, backend: str, page: str) -> str:
    """Один прогон стенда с заданным потолком запаса; путь к логу stderr."""
    exe = os.path.join(REPO, 'target', 'dev-release', 'lumen.exe')
    if not os.path.exists(exe):
        raise SystemExit(f'нет {exe} — cargo build -p lumen-shell --profile dev-release')
    stand = os.path.join(REPO, page)
    if not os.path.exists(stand):
        raise SystemExit(f'нет стенда {stand}')
    # BUG-651: `file://` нельзя передать начальным аргументом — только navigate.
    url = 'file:///' + stand.replace('\\', '/')

    env = dict(os.environ)
    env['LUMEN_FRAME_LOG'] = '2'
    if backend:
        env['WGPU_BACKEND'] = backend
    if margin is not None:
        env['LUMEN_BAND_MARGIN_CSS'] = str(margin)
    else:
        env.pop('LUMEN_BAND_MARGIN_CSS', None)

    port = free_port()
    tmp_dir = os.path.join(REPO, '.tmp')
    os.makedirs(tmp_dir, exist_ok=True)
    tag = 'default' if margin is None else f'{margin:g}'
    log_path = os.path.join(tmp_dir, f'band_miss_{tag}_{backend or "auto"}.log')
    log_f = open(log_path, 'w', encoding='utf-8', errors='replace')
    proc = subprocess.Popen(
        [exe, '--maximized', '--mcp-live-port', str(port), 'about:blank'],
        cwd=REPO, env=env, stdout=subprocess.DEVNULL, stderr=log_f)
    try:
        c = Client(port, log_path)
        c.call('navigate', {'url': url})
        c.call('wait', {'condition': 'document_ready', 'timeout_ms': 20000})
        time.sleep(1.5)
        for _ in range(ticks):
            c.call('scroll', {'target': 'body', 'delta': {'x': 0, 'y': delta}})
            time.sleep(0.05)
        time.sleep(0.4)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        log_f.close()
    return log_path


def parse(log_path: str) -> dict:
    """Кадры лога, помеченные путём композитора.

    Порядок строк движка: `[frame:wgpu] page-compose …` печатается ВНУТРИ
    рендера, то есть ДО `[frame] total` своего кадра. Кадр без такой строки —
    это кадр, до композитора не дошедший (первые кадры загрузки).
    """
    miss, hit, other = [], [], []
    pending = None
    band = None
    adapter = ''
    skips: dict[str, int] = {}
    with open(log_path, encoding='utf-8', errors='replace') as f:
        for line in f:
            m = ADAPTER_RE.search(line)
            if m:
                adapter = m.group(1).strip()
                continue
            m = MISS_RE.search(line)
            if m:
                pending = 'miss'
                band = (int(m.group(3)), int(m.group(4)))
                continue
            if HIT_RE.search(line):
                pending = 'hit'
                continue
            m = SKIP_RE.search(line)
            if m:
                skips[m.group(1).strip()] = skips.get(m.group(1).strip(), 0) + 1
                pending = None
                continue
            m = TOTAL_RE.search(line)
            if m:
                ms, sy = float(m.group(1)), float(m.group(2))
                {'miss': miss, 'hit': hit, None: other}[pending].append((ms, sy))
                pending = None
    return {'miss': miss, 'hit': hit, 'other': other,
            'band': band, 'adapter': adapter, 'skips': skips}


def med(xs: list[float]) -> float:
    return statistics.median(xs) if xs else float('nan')


def report(tag: str, data: dict) -> dict:
    miss = [ms for ms, _ in data['miss']]
    hit = [ms for ms, _ in data['hit']]
    band = data['band']
    band_h = band[1] if band else 0
    m_med, h_med = med(miss), med(hit)
    ys = [sy for _, sy in data['miss'] + data['hit'] + data['other']]
    travel = max(ys) - min(ys) if ys else 0.0
    print(f'\n=== запас {tag} ===')
    print(f'  {data["adapter"]}')
    if band:
        print(f'  полоса: {band[0]}x{band[1]} px')
    print(f'  прокручено {travel:.0f} css px (scroll_y до {max(ys) if ys else 0:.0f})')
    print(f'  промахов {len(miss)}, попаданий {len(hit)}, вне композитора {len(data["other"])}')
    if miss:
        print(f'  промах: p50 {m_med:6.2f}  min {min(miss):6.2f}  max {max(miss):6.2f}  '
              f'сумма {sum(miss):7.1f} мс')
    if hit:
        print(f'  попад.: p50 {h_med:6.2f}  min {min(hit):6.2f}  max {max(hit):6.2f}  '
              f'сумма {sum(hit):7.1f} мс')
    surcharge = m_med - h_med if miss and hit else float('nan')
    if miss and hit:
        print(f'  надбавка промаха (p50 промах − p50 попадание): {surcharge:.2f} мс')
        # Цена промахов, приведённая к пути прокрутки: она и решает, окупается
        # ли БОЛЬШАЯ полоса (промах дороже, но реже), тогда как надбавка выше
        # отвечает только на вопрос про цену одного промаха.
        if travel > 0:
            per_kpx = sum(m - h_med for m in miss) / travel * 1000.0
            print(f'  надбавка промахов на 1000 px пути: {per_kpx:.2f} мс')
    for reason, n in data['skips'].items():
        print(f'  skip x{n}: {reason}')
    return {'band_h': band_h, 'miss_n': len(miss), 'miss_med': m_med,
            'hit_med': h_med, 'surcharge': surcharge, 'travel': travel,
            'miss_sum_over': sum(m - h_med for m in miss) if miss and hit else float('nan')}


def fit(rows: list[dict]) -> None:
    """Линейная регрессия надбавки промаха по высоте полосы (МНК).

    Свободный член — постоянная цена промаха (её инкрементальная дорисовка НЕ
    снимает), наклон — цена пикселя высоты полосы (её снимает пропорционально
    переиспользованному перекрыву).
    """
    pts = [(r['band_h'], r['surcharge']) for r in rows
           if r['band_h'] and r['surcharge'] == r['surcharge']]
    if len(pts) < 2:
        print('\nсвип короче двух точек — регрессии нет')
        return
    n = len(pts)
    sx = sum(x for x, _ in pts)
    sy = sum(y for _, y in pts)
    sxx = sum(x * x for x, _ in pts)
    sxy = sum(x * y for x, y in pts)
    denom = n * sxx - sx * sx
    if denom == 0:
        print('\nвсе точки на одной высоте — регрессии нет')
        return
    slope = (n * sxy - sx * sy) / denom
    inter = (sy - slope * sx) / n
    print(f'\nнадбавка(h) ≈ {inter:.2f} мс + {slope * 1000:.4f} мс/1000px  ({n} точек)')
    for x, y in sorted(pts):
        print(f'   h={x:5d}  надбавка {y:6.2f}  модель {inter + slope * x:6.2f}')


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('--margins', type=float, nargs='+', default=[300, 500, 768, 1200],
                    help='потолки запаса полосы в CSS px (свип по площади)')
    ap.add_argument('--ticks', type=int, default=60)
    ap.add_argument('--delta', type=float, default=120.0, help='CSS px на щелчок колеса')
    ap.add_argument('--backend', default='vulkan')
    ap.add_argument('--page', default='samples/bench-static-scroll.html')
    ap.add_argument('--report-only', action='store_true')
    args = ap.parse_args()

    rows = []
    for margin in args.margins:
        tag = f'{margin:g}'
        log = os.path.join(REPO, '.tmp', f'band_miss_{tag}_{args.backend or "auto"}.log')
        if not args.report_only:
            log = run(margin, args.ticks, args.delta, args.backend, args.page)
        rows.append(report(tag, parse(log)))
    fit(rows)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
