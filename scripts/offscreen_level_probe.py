#!/usr/bin/env python3
"""Перепись цены ПЕРВОГО обращения к свежему offscreen-слою уровня (BUG-405 срез 25).

Гоняет стенд `scripts/perf-fixtures/offscreen-level-<вариант>.html` в живом
развёрнутом окне под `LUMEN_FRAME_LOG=3`, прокручивает его и печатает по каждому
дорогому кадру разбивку элементов плана плюс перепись созданных текстур.

Вопрос, ради которого написан: платит ли ту же цену уровень БЕЗ фильтра.
Ответ (срез 25, Intel Iris Plus / Vulkan, 3 прогона): да — `opacity`-группа на
полосном слое 1920×2541 стоит 13.9–16.1 мс ПЕРВЫМ обращением и 0.02–0.03 мс
вторым, то есть цена принадлежит уровню, а не фильтру. Вариант `clip` уровня
вообще не заводит (скруглённый клип идёт контуром в шейдере, срез 4).

Варианты стенда:
  opacity — `opacity: 0.6`, уровень без фильтра (главный)
  blur    — `filter: blur(3.5px)`, форма среза 24 (контроль)
  clip    — `border-radius` + `overflow: hidden`
  none    — тот же блок без уровня (база)

    python scripts/offscreen_level_probe.py opacity blur --backend vulkan

Бэкенд задавать обязательно: цифры DX12 и Vulkan несопоставимы (срез 14).
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, 'scripts'))
from scroll_perf import Client, free_port  # noqa: E402

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, 'reconfigure'):
        _stream.reconfigure(encoding='utf-8', errors='replace')

TOTAL_RE = re.compile(r'\[frame\] total\s+([\d.]+)ms\s+\(scroll_y ([\-\d.]+)')
MODE_RE = re.compile(r'\[frame\] (?:delta|band) (.+)')
SEQ_RE = re.compile(r'\[frame:wgpu\]\s+seq: (.*)')
ALLOC_RE = re.compile(r'\[frame:wgpu\]\s+alloc: this frame created (\d+) tex in ([\d.]+)ms')
CENSUS_RE = re.compile(r'\[frame:wgpu\]\s+alloc-census \(total\): (.*)')
# Элемент `seq`: `<вид><n>[<дескриптор с пробелами>]:<мс>`.
ITEM_RE = re.compile(r'([a-z]+\d*)\[([^\]]*)\]:([\d.]+)')


def run(variant: str, ticks: int, delta: float, backend: str) -> str:
    """Один прогон стенда; возвращает путь к логу stderr."""
    exe = os.path.join(REPO, 'target', 'dev-release', 'lumen.exe')
    if not os.path.exists(exe):
        raise SystemExit(f'нет {exe} — cargo build -p lumen-shell --profile dev-release')
    stand = os.path.join(REPO, 'scripts', 'perf-fixtures', f'offscreen-level-{variant}.html')
    if not os.path.exists(stand):
        raise SystemExit(f'нет стенда {stand}')
    # BUG-651: `file://` нельзя передать начальным аргументом — только navigate.
    page = 'file:///' + stand.replace('\\', '/')

    env = dict(os.environ)
    env['LUMEN_FRAME_LOG'] = '3'
    if backend:
        env['WGPU_BACKEND'] = backend

    port = free_port()
    tmp_dir = os.path.join(REPO, '.tmp')
    os.makedirs(tmp_dir, exist_ok=True)
    log_path = os.path.join(tmp_dir, f'offscreen_level_{variant}_{backend or "auto"}.log')
    log_f = open(log_path, 'w', encoding='utf-8', errors='replace')
    proc = subprocess.Popen(
        [exe, '--maximized', '--mcp-live-port', str(port), 'about:blank'],
        cwd=REPO, env=env, stdout=subprocess.DEVNULL, stderr=log_f)
    try:
        c = Client(port, log_path)
        c.call('navigate', {'url': page})
        c.call('wait', {'condition': 'document_ready', 'timeout_ms': 20000})
        time.sleep(1.5)
        for direction in (+1, -1):
            for _ in range(ticks):
                c.call('scroll', {'target': 'body',
                                  'delta': {'x': 0, 'y': direction * delta}})
                time.sleep(0.05)
        time.sleep(0.5)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        log_f.close()
    return log_path


def parse(log_path: str) -> tuple[list[dict], str, str]:
    """Разбирает лог в список кадров.

    Порядок строк движка: `[frame:wgpu] …` (seq/alloc) идут ДО `[frame] total`
    своего кадра, а `[frame] delta …` / `[frame] band …` — ПОСЛЕ него.

    `seq` копится СПИСКОМ, а не одним значением: строк `seq:` в логе больше,
    чем строк `total` (движок печатает разбивку и для проходов, не дающих
    своего `[frame] total`). При перезаписи одним полем такие элементы
    молча пропадают — на этом стенде терялись два композита уровня из трёх.
    """
    frames: list[dict] = []
    cur: dict = {}
    census = adapter = ''
    with open(log_path, encoding='utf-8', errors='replace') as f:
        for line in f:
            if line.startswith('[wgpu] adapter:'):
                adapter = line.strip()
                continue
            m = MODE_RE.search(line)
            if m and frames:
                frames[-1].setdefault('mode', []).append(m.group(1).strip())
                continue
            m = SEQ_RE.search(line)
            if m:
                cur.setdefault('seq', []).append(m.group(1).strip())
                continue
            m = ALLOC_RE.search(line)
            if m:
                cur['alloc'] = (int(m.group(1)), float(m.group(2)))
                continue
            m = CENSUS_RE.search(line)
            if m:
                census = m.group(1).strip()
                continue
            m = TOTAL_RE.search(line)
            if m:
                cur['total'] = float(m.group(1))
                cur['scroll_y'] = float(m.group(2))
                frames.append(cur)
                cur = {}
    return frames, census, adapter


def report(log_path: str, top: int, floor_ms: float) -> None:
    frames, census, adapter = parse(log_path)
    print(f'\n=== {os.path.basename(log_path)} ===')
    print(f'  {adapter}')
    print(f'  кадров: {len(frames)}')
    tot = sorted((fr.get('total', 0.0) for fr in frames), reverse=True)
    if tot:
        print(f'  total: max {tot[0]:.2f}  p50 {tot[len(tot) // 2]:.2f}  '
              f'сумма {sum(tot):.1f} мс')
    print(f'  alloc-census (за процесс): {census}')

    # Композиты уровня по кадрам — первое обращение против повторного.
    print('  композиты уровня по кадрам:')
    seen = False
    for i, fr in enumerate(frames, 1):
        hits = [(k, d, float(t))
                for seq in fr.get('seq', [])
                for k, d, t in ITEM_RE.findall(seq)
                if k.startswith(('comp', 'filt', 'mask', 'bdrop', 'mlayer', 'rclip'))]
        for k, d, t in hits:
            seen = True
            print(f'    кадр#{i}: {t:7.2f} мс  {k}[{d}]')
    if not seen:
        print('    нет — уровень на этом стенде не заводится')

    for fr in sorted(frames, key=lambda f: -f.get('total', 0.0))[:top]:
        alloc = fr.get('alloc')
        print(f'\n  -- кадр total {fr.get("total", 0):.2f} мс '
              f'scroll_y {fr.get("scroll_y", 0):.0f} [{" / ".join(fr.get("mode", []))}]'
              + (f' alloc {alloc[0]} tex / {alloc[1]:.2f} мс' if alloc else ''))
        items = [(k, d, float(t))
                 for seq in fr.get('seq', [])
                 for k, d, t in ITEM_RE.findall(seq)
                 if float(t) >= floor_ms]
        for k, d, t in items:
            print(f'     {t:7.2f} мс  {k}[{d}]')
        if not items:
            print(f'     элементов дороже {floor_ms} мс нет')


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('variants', nargs='+',
                    choices=['opacity', 'blur', 'clip', 'none'])
    ap.add_argument('--ticks', type=int, default=30)
    ap.add_argument('--delta', type=float, default=300.0)
    ap.add_argument('--backend', default='vulkan',
                    help='vulkan|dx12|gl — пинует бэкенд (цифры несопоставимы между ними)')
    ap.add_argument('--top', type=int, default=3, help='сколько самых дорогих кадров печатать')
    ap.add_argument('--floor-ms', type=float, default=0.2)
    ap.add_argument('--report-only', action='store_true',
                    help='не гонять окно, разобрать лог прошлого прогона')
    args = ap.parse_args()

    for v in args.variants:
        log = os.path.join(REPO, '.tmp',
                           f'offscreen_level_{v}_{args.backend or "auto"}.log')
        if not args.report_only:
            log = run(v, args.ticks, args.delta, args.backend)
        report(log, args.top, args.floor_ms)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
