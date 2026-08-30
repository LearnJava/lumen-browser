#!/usr/bin/env python3
"""Разбивка статьи `build` кадра ПОПАДАНИЯ (BUG-405 срез 37, пункты 68/76).

Срез 34 разложил честный кадр попадания на статьи, срез 36 разложил крупнейшую
из них — композитный пасс — и нашёл там работу, которая делается зря (хром
собирается заново ради одной команды из 132). Вторая по величине статья того же
кадра, `build` (0.22–0.32 мс), в срез 34 вошла ОДНИМ числом: это шаг 6
`RedrawRequested`, где шелл собирает overlay-буфер до обращения к рендереру.

Здесь она снимается по подстатьям — строка `[frame]   build: chrome … sbar …
panels … tail … | chrome NxK=M cmds, overlay L | band hit`:

* `chrome` — раскладка снимка хрома по клип-полосам вокруг страницы;
* `sbar` — полоса прокрутки;
* `panels` — все остальные overlay-строители (формы, подсказки, панели DevTools);
* `tail` — split-view, инспектор, фон канвы.

Плюс состав: `NxK=M` — снимок хрома длиной N команд, скопированный в K непустых
клип-полос, даёт M команд, и это ровно тот overlay, чью цену внутри пасса срез 36
намерил в 0.18–0.23 мс.

**Уровень лога 1, а не 2** (пункт 71): пофазный блок уровня 2 стоит 1.3–3.5 мс на
кадр, то есть больше самого кадра попадания. Признак попадания на уровне 1 даёт
не строка `page-compose HIT` (её там нет), а счётчик `lumen_paint::last_compose`,
печатаемый в конце той же строки как `band hit|miss|-`.

    python scripts/build_phase_census.py --repeats 3 --backend vulkan

Порядок плеч вращается по кругам (пункт 64). Бэкенд задавать обязательно (срез
14). Гейт тождества плеч — путь прокрутки, число кадров и состав хрома (`NxK=M`):
плечи обязаны мерить одну и ту же работу.
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
BUILD_RE = re.compile(
    r'\[frame\]\s+build: chrome ([\-\d.]+) sbar ([\-\d.]+) panels ([\-\d.]+) '
    r'tail ([\-\d.]+) \| chrome (\d+)x(\d+)=(\d+) cmds, overlay (\d+) \| band (\S+)')
PAINT_RE = re.compile(
    r'\[frame\]\s+paint: prep ([\-\d.]+) hash ([\-\d.]+) band ([\-\d.]+) '
    r'пасс ([\-\d.]+) лог ([\-\d.]+) предметки ([\-\d.]+) послекэша ([\-\d.]+) '
    r'предвызов ([\-\d.]+) обёртка ([\-\d.]+) \| невязка ([\-\d.]+)')
ADAPTER_RE = re.compile(r'\[wgpu\] adapter: (.*)')

ARM_SETS = {
    # Одно плечо: перепись состава, а не сравнение. Для правки сюда встанет
    # второе плечо с её рычагом.
    'plain': [('штатный', {})],
    # BUG-405 срез 38: фаст-пас страничного смещения против обёртки
    # `PushTransform`, которую шелл строил каждый кадр. Статья `обёртка` в
    # разбивке `paint` и есть предмет сравнения: у фаст-паса её быть не должно.
    'offset': [('фаст-пас', {}), ('обёртка', {'LUMEN_NO_PAGE_OFFSET': '1'})],
    # BUG-405 срез 39: мемоизация свёртки content-части кадровых хэшей против
    # полного обхода списка. Предмет сравнения — статья `hash` в разбивке
    # `paint`: у плеча «свёртка» в неё входит только overlay.
    'epoch': [('свёртка', {}), ('обход', {'LUMEN_NO_DL_EPOCH': '1'})],
}

PHASES = ['chrome', 'sbar', 'panels', 'tail']
# Подстатьи вызова рендерера (слоты FRAME_PHASE_NANOS + цена печати + остаток).
PAINT_PHASES = [
    'prep', 'hash', 'band', 'пасс', 'лог', 'предметки', 'послекэша', 'предвызов', 'обёртка',
    'невязка',
]


def parse(log_path: str) -> dict:
    """Кадры прокрутки: полное время, статьи шага 6 и признак попадания.

    Шелл печатает кадр «шапкой вперёд»: `total` идёт ПЕРВОЙ строкой блока, а
    `top`/`build`/`paint` — за ней. Поэтому кадр закрывается не по своей строке
    `total`, а по строке `total` СЛЕДУЮЩЕГО кадра (и последний — по концу
    файла); иначе полное время склеивалось бы со статьями соседнего кадра.
    """
    frames: list[dict] = []
    cur: dict = {}
    ys: list[float] = []
    adapter = ''

    def close(frame: dict) -> None:
        if not frame:
            return
        if 'build' in frame:
            named = sum(frame.get(k, 0.0) for k in PHASES)
            frame['build-ост.'] = max(frame['build'] - named, 0.0)
        frames.append(frame)

    with open(log_path, encoding='utf-8', errors='replace') as fh:
        for line in fh:
            m = ADAPTER_RE.search(line)
            if m:
                adapter = m.group(1).strip()
                continue
            m = TOP_RE.search(line)
            if m:
                g = [float(x) for x in m.groups()]
                cur['shell'] = g[0] + g[1] + g[2] + g[3] + g[4]
                cur['build'] = g[4]
                cur['paint'] = g[5]
                continue
            m = BUILD_RE.search(line)
            if m:
                g = m.groups()
                cur.update(dict(zip(PHASES, (float(x) for x in g[:4]))))
                cur['chrome_n'] = int(g[4])
                cur['strips'] = int(g[5])
                cur['chrome_m'] = int(g[6])
                cur['overlay'] = int(g[7])
                # НЕ ключ `band`: так называется и подстатья `paint`, и строка
                # `paint` идёт после строки `build` — общий ключ затирал бы
                # признак кадра числом, и попаданий в выборке было бы ноль.
                cur['outcome'] = g[8]
                continue
            m = PAINT_RE.search(line)
            if m:
                cur.update(dict(zip(PAINT_PHASES, (float(x) for x in m.groups()))))
                continue
            m = TOTAL_RE.search(line)
            if m:
                close(cur)
                cur = {'total': float(m.group(1))}
                ys.append(float(m.group(2)))
    close(cur)
    travel = (max(ys) - min(ys)) if ys else 0.0
    return {'frames': frames, 'travel': travel, 'adapter': adapter}


def med(xs: list[float]) -> float:
    return statistics.median(xs) if xs else float('nan')


def report(tag: str, data: dict) -> dict:
    """Одно плечо: статьи шага 6 на кадрах попадания."""
    # Первые кадры после загрузки — прогрев полосы и атласа; отбрасываем
    # ровно столько же в каждом плече.
    frames = data['frames'][3:]
    hits = [f for f in frames if f.get('outcome') == 'hit']
    print(f'\n=== {tag} ===')
    print(f'  {data["adapter"]}')
    print(f'  кадров {len(frames)} (попаданий {len(hits)}), '
          f'путь {data["travel"]:.0f} css px')
    if frames and not hits:
        # Разбор, который не нашёл ни одного попадания, — это почти всегда
        # разъехавшийся формат строки, а не стенд без попаданий: молча выдать
        # пустую таблицу здесь опаснее, чем упасть.
        seen = sorted({f.get('outcome', '(нет строки build)') for f in frames})
        raise SystemExit(
            f'{tag}: ни одного кадра попадания на {len(frames)} кадров; '
            f'признаки в выборке: {seen}. Проверить формат строки '
            f'"[frame]   build: … | band …" против BUILD_RE.')
    cols: dict[str, float] = {}
    if hits:
        cols['build'] = med([f['build'] for f in hits])
        for k in PHASES + ['build-ост.']:
            cols[k] = med([f.get(k, 0.0) for f in hits])
        cols['paint'] = med([f['paint'] for f in hits])
        for k in PAINT_PHASES:
            cols[k] = med([f.get(k, 0.0) for f in hits])
        print('  p50: ' + '  '.join(f'{k} {v:.2f}' for k, v in cols.items()))
        print('  кадр целиком p50 {:.2f}, shell {:.2f}, paint {:.2f}'.format(
            med([f['total'] for f in hits]),
            med([f['shell'] for f in hits]),
            med([f['paint'] for f in hits])))
        mix = {(f['chrome_n'], f['strips'], f['chrome_m'], f['overlay'])
               for f in hits}
        print(f'  состав хрома (разных сочетаний {len(mix)}): ' +
              '; '.join(f'{n}x{k}={m} cmds, overlay {ov}'
                        for n, k, m, ov in sorted(mix)))
    return {'cols': cols, 'n': len(frames), 'hit_n': len(hits),
            'travel': data['travel'],
            'mix': {(f['chrome_n'], f['strips'], f['chrome_m'], f['overlay'])
                    for f in hits},
            'raw': {k: [f.get(k, 0.0) for f in hits]
                    for k in ['build', 'total', 'shell', 'paint', 'build-ост.']
                    + PHASES + PAINT_PHASES}}


def summarize(arms: list, rows: dict[str, list[dict]]) -> None:
    """Сводка по объединённой выборке повторов, плечо к плечу."""
    print('\n' + '=' * 78)
    print('Гейт тождества плеч (работа, а не эхо рычага):')
    for tag, _ in arms:
        rs = rows.get(tag, [])
        if not rs:
            continue
        mix = set().union(*(r['mix'] for r in rs))
        print('  {:12s} кадров {}  попаданий {}  путь {:.0f} css px  состав {}'
              .format(tag, sum(r['n'] for r in rs), sum(r['hit_n'] for r in rs),
                      med([r['travel'] for r in rs]),
                      '; '.join(f'{n}x{k}={m}/{ov}' for n, k, m, ov in sorted(mix))))
    print('\nстатья   | ' + ' | '.join(f'{tag:>11s}' for tag, _ in arms) + ' | разница')
    for k in (['total', 'shell', 'build'] + PHASES + ['build-ост.', 'paint']
              + PAINT_PHASES):
        line = f'{k:8s} |'
        vals = []
        for tag, _ in arms:
            pool = [x for r in rows.get(tag, []) for x in r['raw'].get(k, [])]
            vals.append(med(pool) if pool else float('nan'))
            line += f' {vals[-1]:11.2f} |' if pool else '           — |'
        if len(vals) == 2 and vals[0] == vals[0] and vals[1] == vals[1]:
            line += f' {vals[0] - vals[1]:+7.2f}'
        print(line)
    print('\n`build` = шаг 6 до обращения к рендереру; `paint` = сам вызов')
    print('рендерера (внутри него composite-пасс среза 36). Уровень лога 1,')
    print('поэтому надбавки пункта 71 в числах нет.')


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('--arms', choices=sorted(ARM_SETS), default='plain')
    ap.add_argument('--repeats', type=int, default=3, help='повторов на плечо (интерливед)')
    ap.add_argument('--ticks', type=int, default=60)
    ap.add_argument('--delta', type=float, default=120.0, help='CSS px на щелчок колеса')
    ap.add_argument('--backend', default='vulkan')
    ap.add_argument('--page', default='samples/bench-text-scroll.html')
    ap.add_argument('--report-only', action='store_true')
    args = ap.parse_args()

    arms = ARM_SETS[args.arms]
    slug = {tag: f'build_{args.arms}{i}' for i, (tag, _) in enumerate(arms)}
    rows: dict[str, list[dict]] = {tag: [] for tag, _ in arms}
    for rep in range(args.repeats):
        shift = rep % len(arms)
        for tag, env in arms[shift:] + arms[:shift]:
            name = slug[tag]
            log = os.path.join(REPO, '.tmp', f'band_{name}_r{rep}_{args.backend}.log')
            if not args.report_only:
                log = run(1.0, rep, args.ticks, args.delta, args.backend, args.page,
                          extra_env={'LUMEN_FRAME_LOG': '1', **env}, tag=name)
            rows[tag].append(report(f'{tag} (повтор {rep})', parse(log)))
    summarize(arms, rows)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
