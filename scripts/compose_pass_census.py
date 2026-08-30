#!/usr/bin/env python3
"""Разбивка КОМПОЗИТНОГО ПАССА кадра попадания (BUG-405 срез 36, пункт 68).

Срез 34 разложил кадр попадания на статьи и назвал крупнейшей пару O(n)-хэшей;
срез 35 свёл их в один проход (0.65 → 0.38 мс). После этого крупнейшей статьёй
честного кадра попадания остаётся сам композитный пасс — `render_impl` в режиме
`Compose` (0.54–0.85 мс по срезу 34), и разбивки у него в срезе 34 не было: он
входил туда одним числом.

Разбивка у пасса на самом деле уже печатается (`faces collect prep acquire
encode submit`, BUG-274) — не хватало не инструмента, а прогона, который читает
эти статьи ИМЕННО у кадра попадания и отделяет работу от ожидания.

Два набора плеч, `--arms`:

* `present` — `fifo` (штатный `PresentMode::Fifo`, `desired_maximum_frame_latency
  2`) против `immediate` (`LUMEN_PRESENT=immediate`). Отвечает, работа статья
  `acquire` (`surface.get_current_texture()`) или ожидание swapchain-а: ожидание
  обязано обрушиться во втором плече, работа — совпасть.
* `overlay` — штатный кадр против `LUMEN_NO_COMPOSE_OVERLAY=1`. Отвечает,
  сколько из пасса стоит ХРОМ: overlay — единственное содержимое композитного
  кадра помимо блита полосы. Плечо пиксельно неверно (хром исчезает) и
  существует только ради этой переписи.

    python scripts/compose_pass_census.py --arms overlay --repeats 3 --backend vulkan

Порядок плеч ВРАЩАЕТСЯ по кругам (пункт 64: фиксированный порядок на этом стенде
стоит больше измеряемого эффекта). Бэкенд задавать обязательно: цифры DX12 и
Vulkan несопоставимы (срез 14).

Гейт тождества плеч — счётчик РАБОТЫ, а не эхо рычага (пункт 69): для `present`
это режим из строки `[wgpu] surface: … present …` (плечо обязано ДОЕХАТЬ до
конфигурации, а не только попросить), для `overlay` — `ops` и `cmd-mix draw`
кадра, которые обязаны упасть вместе с рычагом. Плюс общий для обоих наборов:
путь прокрутки и число кадров.

Заодно печатается инвариантность хрома — сколько РАЗНЫХ дайджестов overlay
встретилось на кадрах попадания (строка `[frame:wgpu]   overlay: … digest …`).
Кэшировать хром имеет смысл ровно настолько, насколько он повторяется.

Числа снимаются под `LUMEN_FRAME_LOG=2`, то есть с надбавкой пофазного лога в
каждом кадре (пункт 71). Для РАЗБИВКИ это допустимо — печать лежит вне окна
`[frame:wgpu] total`, — но полное время кадра тут справочное, а не гейт.
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

PHASE_RE = re.compile(
    r'\[frame:wgpu\] total\s+([\d.]+)ms \| faces\s+([\d.]+) collect\s+([\d.]+) '
    r'prep\s+([\d.]+) acquire\s+([\d.]+) encode\s+([\d.]+) submit\s+([\d.]+) '
    r'\| ops (\d+) layers (\d+)')
TOTAL_RE = re.compile(r'\[frame\] total\s+([\d.]+)ms\s+\(scroll_y ([\-\d.]+)')
ADAPTER_RE = re.compile(r'\[wgpu\] adapter: (.*)')
PRESENT_RE = re.compile(r'\[wgpu\] surface: .*present (\w+)')
CMDMIX_RE = re.compile(r'cmd-mix: .*\bdraw (\d+)\b')
OVERLAY_RE = re.compile(r'overlay: (\d+) cmds digest ([0-9a-f]+) changed (\d+)/(\d+)')

ARM_SETS = {
    'present': [('fifo', {}), ('immediate', {'LUMEN_PRESENT': 'immediate'})],
    'overlay': [('штатный', {}), ('без overlay', {'LUMEN_NO_COMPOSE_OVERLAY': '1'})],
    # BUG-405 срез 41: реализация п.76 — блит retained-текстуры «overlay
    # минус горячая команда» вместо полной перерисовки. В отличие от
    # `overlay` выше это плечо пиксельно ВЕРНО в обе стороны (гейт —
    # `overlay_cache_matches_full_overlay_redraw` в renderer.rs), так что
    # разница `ops`/`draw` — не диагностика, а сам измеряемый выигрыш.
    'overlay-cache': [('штатный', {}), ('без кэша', {'LUMEN_NO_OVERLAY_CACHE': '1'})],
}

# Статьи пасса в порядке печати рендерером.
PHASES = ['faces', 'collect', 'prep', 'acquire', 'encode', 'submit']


def parse(log_path: str) -> dict:
    """Пассы кадров ПОПАДАНИЯ с их пофазной разбивкой.

    Порядок строк на попадании: `page-compose HIT` → `compose-top` →
    `[frame:wgpu] total …` композитного пасса → `[frame] total` шелла. На
    промахе первой идёт wgpu-строка ПОЛОСЫ (до `page-compose MISS`), поэтому
    флаг попадания взводится только строкой HIT и гасится и MISS, и концом
    кадра — пасс полосы в выборку попасть не может.
    """
    passes: list[dict] = []
    hit = False
    fresh = False
    overlay: tuple[int, str, int] | None = None
    ys: list[float] = []
    adapter = ''
    present = ''
    frames = 0
    with open(log_path, encoding='utf-8', errors='replace') as fh:
        for line in fh:
            m = ADAPTER_RE.search(line)
            if m:
                adapter = m.group(1).strip()
                continue
            m = PRESENT_RE.search(line)
            if m:
                present = m.group(1)
                continue
            if 'page-compose HIT' in line:
                hit = True
                continue
            if 'page-compose MISS' in line or 'page-compose skip' in line:
                hit = False
                continue
            m = OVERLAY_RE.search(line)
            if m:
                overlay = (int(m.group(1)), m.group(2), int(m.group(3)))
                continue
            m = PHASE_RE.search(line)
            if m:
                fresh = hit
                if hit:
                    g = [float(x) for x in m.groups()]
                    rec = {'total': g[0], 'ops': int(g[7]), 'layers': int(g[8]),
                           'ov_cmds': overlay[0] if overlay else 0,
                           'ov_digest': overlay[1] if overlay else '',
                           'ov_changed': overlay[2] if overlay else 0}
                    rec.update(dict(zip(PHASES, g[1:7])))
                    # Остаток пасса, не названный ни одной статьёй: сумма фаз
                    # меряется теми же часами, что и `total`, поэтому невязка
                    # здесь — это работа между фазами, а не цена инструмента.
                    rec['остаток'] = max(g[0] - sum(g[1:7]), 0.0)
                    passes.append(rec)
                continue
            m = CMDMIX_RE.search(line)
            if m:
                # Строка идёт ПОСЛЕ пофазной строки того же пасса.
                if fresh and passes:
                    passes[-1]['draw'] = int(m.group(1))
                continue
            m = TOTAL_RE.search(line)
            if m:
                frames += 1
                ys.append(float(m.group(2)))
                hit = False
                fresh = False
    travel = (max(ys) - min(ys)) if ys else 0.0
    return {'passes': passes, 'travel': travel, 'adapter': adapter,
            'present': present, 'frames': frames}


def med(xs: list[float]) -> float:
    return statistics.median(xs) if xs else float('nan')


def report(tag: str, data: dict) -> dict:
    """Одно плечо: p50/min каждой статьи пасса попадания."""
    # Первые кадры после загрузки — прогрев полосы и атласа; отбрасываем
    # ровно столько же в обоих плечах.
    passes = data['passes'][3:]
    print(f'\n=== {tag} ===')
    print(f'  {data["adapter"]}')
    print(f'  present {data["present"] or "?"} | кадров {data["frames"]}, '
          f'пассов попадания {len(passes)}, путь {data["travel"]:.0f} css px')
    cols: dict[str, float] = {}
    if passes:
        cols['пасс'] = med([p['total'] for p in passes])
        for k in PHASES + ['остаток']:
            cols[k] = med([p[k] for p in passes])
        print('  p50: ' + '  '.join(f'{k} {v:.2f}' for k, v in cols.items()))
        print('  min: пасс {:.2f}  acquire {:.2f}  submit {:.2f}'.format(
            min(p['total'] for p in passes),
            min(p['acquire'] for p in passes),
            min(p['submit'] for p in passes)))
        print('  работа кадра: ops {:.0f}  draw {:.0f}  layers {:.0f}'.format(
            med([float(p['ops']) for p in passes]),
            med([float(p.get('draw', float('nan'))) for p in passes]),
            med([float(p['layers']) for p in passes])))
        digests = {p['ov_digest'] for p in passes if p['ov_digest']}
        if digests:
            ch = [float(p['ov_changed']) for p in passes]
            print('  overlay: {:.0f} cmds, РАЗНЫХ дайджестов {} на {} попаданий, '
                  'изменено команд p50 {:.0f} (min {:.0f}, max {:.0f})'
                  .format(med([float(p['ov_cmds']) for p in passes]),
                          len(digests), len(passes), med(ch), min(ch), max(ch)))
    return {'cols': cols, 'n': len(passes), 'travel': data['travel'],
            'present': data['present'], 'frames': data['frames'],
            'digests': {p['ov_digest'] for p in passes if p['ov_digest']},
            'ov_changed': [float(p['ov_changed']) for p in passes],
            'ov_cmds': med([float(p['ov_cmds']) for p in passes]),
            'work': {k: med([float(p.get(k, float('nan'))) for p in passes])
                     for k in ('ops', 'draw')},
            'raw': {k: [p[k] for p in passes] for k in ['total'] + PHASES}}


def summarize(arms: list[tuple[str, dict]], rows: dict[str, list[dict]]) -> None:
    """Сводка: статьи пасса по объединённой выборке повторов, плечо к плечу."""
    print('\n' + '=' * 78)
    print('Гейт тождества плеч (счётчик работы, не эхо рычага):')
    for tag, _ in arms:
        rs = rows.get(tag, [])
        if not rs:
            continue
        print('  {:12s} present={:10s} ops {:.0f}  draw {:.0f}  '
              'кадров {}  путь {:.0f} css px'.format(
                  tag, '/'.join(sorted({r['present'] for r in rs})),
                  med([r['work']['ops'] for r in rs]),
                  med([r['work']['draw'] for r in rs]),
                  sum(r['frames'] for r in rs),
                  med([r['travel'] for r in rs])))
    print('\nстатья   | ' + ' | '.join(f'{tag:>11s}' for tag, _ in arms) + ' | разница')
    for k in ['total'] + PHASES:
        line = f'{k:8s} |'
        vals = []
        for tag, _ in arms:
            pool = [x for r in rows.get(tag, []) for x in r['raw'].get(k, [])]
            vals.append(med(pool) if pool else float('nan'))
            line += f' {vals[-1]:11.2f} |' if pool else '           — |'
        if len(vals) == 2 and vals[0] == vals[0] and vals[1] == vals[1]:
            line += f' {vals[0] - vals[1]:+7.2f}'
        print(line)
    print('\n«total» — весь композитный пасс, статьи — его фазы (BUG-274).')
    print('`acquire` = surface.get_current_texture(); `submit` включает present().')
    digests = {tag: set().union(*(r['digests'] for r in rows[tag]))
               for tag, _ in arms if rows.get(tag)}
    hits = {tag: sum(r['n'] for r in rows[tag]) for tag, _ in arms if rows.get(tag)}
    for tag, d in digests.items():
        if not d:
            continue
        ch = [x for r in rows[tag] for x in r['ov_changed']]
        cmds = med([r['ov_cmds'] for r in rows[tag]])
        print(f'Хром плеча «{tag}»: {len(d)} разных дайджестов overlay на '
              f'{hits[tag]} кадров попадания; изменено команд за кадр '
              f'p50 {med(ch):.0f} из {cmds:.0f} ({med(ch) / cmds * 100:.0f} %).')


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('--arms', choices=sorted(ARM_SETS), default='overlay')
    ap.add_argument('--repeats', type=int, default=3, help='повторов на плечо (интерливед)')
    ap.add_argument('--ticks', type=int, default=60)
    ap.add_argument('--delta', type=float, default=120.0, help='CSS px на щелчок колеса')
    ap.add_argument('--backend', default='vulkan')
    ap.add_argument('--page', default='samples/bench-text-scroll.html')
    ap.add_argument('--report-only', action='store_true')
    args = ap.parse_args()

    arms = ARM_SETS[args.arms]
    slug = {tag: f'cpass_{args.arms}{i}' for i, (tag, _) in enumerate(arms)}
    rows: dict[str, list[dict]] = {tag: [] for tag, _ in arms}
    for rep in range(args.repeats):
        shift = rep % len(arms)
        for tag, env in arms[shift:] + arms[:shift]:
            name = slug[tag]
            log = os.path.join(REPO, '.tmp', f'band_{name}_r{rep}_{args.backend}.log')
            if not args.report_only:
                log = run(1.0, rep, args.ticks, args.delta, args.backend, args.page,
                          extra_env=env, tag=name)
            rows[tag].append(report(f'{tag} (повтор {rep})', parse(log)))
    summarize(arms, rows)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
