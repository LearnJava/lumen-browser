#!/usr/bin/env python3
"""Перепись цены промаха полосы по числу ПЕРЕРИСОВАННЫХ строк (BUG-405 срез 29).

Вопрос — пункт 60 остатка бага. Свип среза 27 (`scripts/band_miss_census.py`)
менял площадь рисования вместе с размером ЦЕЛИ: там росла и текстура полосы, и
число строк. Инкрементальная дорисовка (пункт 43) меняет только второе — цель
остаётся 1920×2541, а рисуется доля её строк. Пункт 59 при этом назвал рост
цены на строку свойством ЦЕЛИ рендера, а срез 25 уже ловил случай, где цена
привязана к объекту текстуры, а не к обработанной площади. Значит «48 % строк =
48 % цены» — гипотеза, и без её проверки выигрыш правки не назван.

Метод — свип по `LUMEN_BAND_DRAW_FRACTION` при НЕИЗМЕННЫХ окне, странице, шаге
прокрутки и размере полосы. Рычаг клипует содержимое полосы верхней долей её
высоты; пиксельно он неверен (ниже доли полоса пуста), поэтому существует
только ради этой переписи.

    python scripts/band_draw_fraction_census.py --fractions 0.25 0.5 0.75 1.0 \
        --repeats 3 --backend vulkan

Прогоны ИНТЕРЛИВЕД (round-robin по долям), сравнение по МИНИМУМУ надбавки из
повторов — разброс прогонов на этом стенде ±4 мс, то есть больше половины
ожидаемого эффекта (`docs/perf-method.md`). Бэкенд задавать обязательно: цифры
DX12 и Vulkan несопоставимы (срез 14).

Гейт тождества плеч печатается вместе с числами: доля из строки
`page-compose MISS … frac F` и число draw-ops кадра полосы (`ops N` в
`[frame:wgpu] total`). Если `ops` не падает вместе с долей — рычаг померил одну
и ту же конфигурацию, и числам верить нельзя (ровно тот отказ, на котором
споткнулся рычаг среза 27).
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
WGPU_TOTAL_RE = re.compile(r'\[frame:wgpu\] total\s+([\d.]+)ms.*\bops (\d+)\b')
PLAN_RE = re.compile(r'plan: draw (\d+)x[\d.]+ms comp \d+x[\d.]+ms mask \d+x[\d.]+ms filt (\d+)x')
CMDMIX_RE = re.compile(r'cmd-mix: .*\bdraw (\d+)\b')
MISS_RE = re.compile(
    r'page-compose MISS: band y=([\-\d.]+)\.\.([\-\d.]+) css \((\d+)x(\d+) px.*frac ([\d.]+)')
HIT_RE = re.compile(r'page-compose HIT')
SKIP_RE = re.compile(r'page-compose skip: (.*)')
ADAPTER_RE = re.compile(r'\[wgpu\] adapter: (.*)')
LOAD_RE = re.compile(r'page-compose MISS:.*\bload (\w+)')


def log_name(tag: str, rep: int, backend: str) -> str:
    """Имя файла лога прогона — одно на (плечо, повтор, бэкенд)."""
    return f'band_{tag}_r{rep}_{backend or "auto"}.log'


def run(frac: float, rep: int, ticks: int, delta: float, backend: str, page: str,
        extra_env: dict[str, str] | None = None, tag: str | None = None) -> str:
    """Один прогон стенда с заданной долей перерисовки; путь к логу stderr.

    `extra_env`/`tag` — для свипов, которые держат долю постоянной и меняют
    другие рычаги полосы (срез 30, `scripts/band_pass_constant_census.py`):
    имя лога тогда берётся из `tag`, иначе плечи затирали бы файлы друг друга.
    """
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
    if frac >= 1.0:
        env.pop('LUMEN_BAND_DRAW_FRACTION', None)
    else:
        env['LUMEN_BAND_DRAW_FRACTION'] = repr(frac)
    env.update(extra_env or {})

    port = free_port()
    tmp_dir = os.path.join(REPO, '.tmp')
    os.makedirs(tmp_dir, exist_ok=True)
    log_path = os.path.join(tmp_dir, log_name(tag or f'frac_{frac:g}', rep, backend))
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
    """Кадры лога, помеченные путём композитора, плюс гейт тождества плеча.

    Порядок строк движка на кадре промаха: `[frame:wgpu] total … ops N` пасса
    ПОЛОСЫ печатается внутри её рендера, то есть ДО строки `page-compose MISS`;
    затем идёт `total` пасса композита и общий `[frame] total`. Поэтому
    `ops` полосы — это последняя wgpu-строка ПЕРЕД MISS.
    """
    miss, hit, other = [], [], []
    pending = None
    band = None
    fracs: set[float] = set()
    loads: set[str] = set()
    work: dict[str, list[float]] = {'ops': [], 'plan_draw': [], 'plan_filt': [], 'cmd_draw': []}
    last: dict[str, float] = {}
    adapter = ''
    skips: dict[str, int] = {}
    with open(log_path, encoding='utf-8', errors='replace') as f:
        for line in f:
            m = ADAPTER_RE.search(line)
            if m:
                adapter = m.group(1).strip()
                continue
            m = WGPU_TOTAL_RE.search(line)
            if m:
                last = {'ops': int(m.group(2))}
                continue
            m = PLAN_RE.search(line)
            if m:
                last['plan_draw'], last['plan_filt'] = int(m.group(1)), int(m.group(2))
                continue
            m = CMDMIX_RE.search(line)
            if m:
                last['cmd_draw'] = int(m.group(1))
                continue
            m = MISS_RE.search(line)
            if m:
                pending = 'miss'
                band = (int(m.group(3)), int(m.group(4)))
                fracs.add(float(m.group(5)))
                lm = LOAD_RE.search(line)
                if lm:
                    loads.add(lm.group(1))
                for k, v in last.items():
                    work[k].append(v)
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
    return {'miss': miss, 'hit': hit, 'other': other, 'band': band,
            'fracs': sorted(fracs), 'loads': sorted(loads), 'work': work,
            'adapter': adapter, 'skips': skips}


def med(xs: list[float]) -> float:
    return statistics.median(xs) if xs else float('nan')


def report(tag: str, data: dict) -> dict:
    """Одно плечо: надбавка промаха плюс гейт тождества (доля и число ops)."""
    miss = [ms for ms, _ in data['miss']]
    hit = [ms for ms, _ in data['hit']]
    band = data['band']
    m_med, h_med = med(miss), med(hit)
    ys = [sy for _, sy in data['miss'] + data['hit'] + data['other']]
    travel = max(ys) - min(ys) if ys else 0.0
    work = {k: med(v) for k, v in data['work'].items()}
    print(f'\n=== доля {tag} ===')
    print(f'  {data["adapter"]}')
    if band:
        print(f'  полоса: {band[0]}x{band[1]} px   frac из лога: {data["fracs"]}')
    print(f'  прокручено {travel:.0f} css px, промахов {len(miss)}, попаданий {len(hit)}')
    print('  работа кадра полосы (p50): ' + '  '.join(
        f'{k} {v:.0f}' for k, v in work.items() if v == v))
    if miss:
        print(f'  промах: p50 {m_med:6.2f}  min {min(miss):6.2f}  max {max(miss):6.2f}')
    if hit:
        print(f'  попад.: p50 {h_med:6.2f}  min {min(hit):6.2f}  max {max(hit):6.2f}')
    surcharge = m_med - h_med if miss and hit else float('nan')
    if miss and hit:
        print(f'  надбавка промаха: {surcharge:.2f} мс')
    for reason, n in data['skips'].items():
        print(f'  skip x{n}: {reason}')
    return {'band_h': band[1] if band else 0, 'miss_n': len(miss),
            'hit_n': len(hit), 'miss_med': m_med, 'hit_med': h_med,
            'surcharge': surcharge, 'travel': travel,
            'work': work, 'fracs': data['fracs'],
            'miss_raw': miss, 'hit_raw': hit}


def summarize(fractions: list[float], rows: dict[float, list[dict]]) -> None:
    """Сводка: надбавка по минимуму повторов против модели пропорциональности.

    Модель — надбавка(доля) = надбавка(1.0) × доля. Она и есть гипотеза пункта
    60; отклонение вверх на малых долях означает постоянную статью, которую
    инкрементальная дорисовка НЕ снимет.
    """
    print('\n' + '=' * 78)
    print('Надбавка промаха: мин из повторов и по объединённой выборке всех повторов.')
    print('«работа» — сколько draw-команд и композитов фильтра реально выполнил кадр')
    print('полосы: если она не падает вместе с долей, рычаг не режет работу и числа')
    print('слева ничего не значат.\n')
    print('доля |  мин  | объед. |     повторы     | draw | filt | модель ∝доле')
    pooled: dict[float, float] = {}
    best_of: dict[float, float] = {}
    for f in fractions:
        rs = rows.get(f, [])
        vals = [r['surcharge'] for r in rs if r['surcharge'] == r['surcharge']]
        if vals:
            best_of[f] = min(vals)
        miss = [ms for r in rs for ms in r['miss_raw']]
        hit = [ms for r in rs for ms in r['hit_raw']]
        if miss and hit:
            pooled[f] = statistics.median(miss) - statistics.median(hit)
    base = best_of.get(1.0)
    base_pooled = pooled.get(1.0)
    for f in fractions:
        rs = rows.get(f, [])
        vals = [r['surcharge'] for r in rs if r['surcharge'] == r['surcharge']]
        if not vals:
            print(f'{f:4.2f} | нет данных')
            continue
        w = {k: med([r['work'].get(k, float('nan')) for r in rs]) for k in
             ('cmd_draw', 'plan_filt')}
        model = f'{base * f:6.2f}' if base else '     —'
        print(f'{f:4.2f} | {min(vals):5.2f} | {pooled.get(f, float("nan")):6.2f} | '
              f'{", ".join(f"{v:.1f}" for v in vals):15s} | {w["cmd_draw"]:4.0f} | '
              f'{w["plan_filt"]:4.0f} | {model}')
    if base and base_pooled:
        print(f'\nМодель — надбавка(1.0) × доля, от {base:.2f} мс (мин). Отклонение вверх =')
        print('постоянная статья промаха, которую инкрементальная дорисовка НЕ снимет.')


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('--fractions', type=float, nargs='+',
                    default=[0.25, 0.5, 0.75, 1.0],
                    help='доли высоты полосы, разрешённые к перерисовке (1.0 = штатно)')
    ap.add_argument('--repeats', type=int, default=3, help='повторов на долю (интерливед)')
    ap.add_argument('--ticks', type=int, default=60)
    ap.add_argument('--delta', type=float, default=120.0, help='CSS px на щелчок колеса')
    ap.add_argument('--backend', default='vulkan')
    ap.add_argument('--page', default='samples/bench-static-scroll.html')
    ap.add_argument('--report-only', action='store_true')
    args = ap.parse_args()

    rows: dict[float, list[dict]] = {f: [] for f in args.fractions}
    for rep in range(args.repeats):
        for frac in args.fractions:
            log = os.path.join(
                REPO, '.tmp', log_name(f'frac_{frac:g}', rep, args.backend))
            if not args.report_only:
                log = run(frac, rep, args.ticks, args.delta, args.backend, args.page)
            rows[frac].append(report(f'{frac:g} (повтор {rep})', parse(log)))
    summarize(args.fractions, rows)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
