#!/usr/bin/env python3
"""BUG-405 срез 50: net-win A/B for `ChromeOverlayFrameCache` (п.85 "вариант
(б)") — does skipping the per-frame `chrome_dl` re-copy into up to 4 clip
strips actually save wall-clock, on a page-only scroll where chrome itself
is never touched (`relayout_chrome_host` doesn't run at all during the
scroll, so `chrome_layout_generation` never bumps and every frame should be
a cache HIT)?

Reads the existing `[frame]   build: chrome X.XX sbar … panels … tail … |
chrome WxH=N cmds, overlay M | band …` line (срез 37 diagnostic, already in
the tree) — no new counter needed. `chrome` here is `bmarks[0] - marks[3]`,
the time to assemble the chrome overlay segment (clip strips + caret) each
`RedrawRequested`.

    python scripts/chrome_overlay_cache_census.py --repeats 3 --backend vulkan

Interleaved A/B, compared on the MINIMUM per docs/perf-method.md — this
stand's per-run jitter can be larger than the effect itself.
"""
from __future__ import annotations

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

BUILD_RE = re.compile(
    r'\[frame\]\s+build: chrome ([\d.]+) sbar ([\d.]+) panels ([\d.]+) tail ([\d.]+) \| '
    r'chrome (\d+)x(\d+)=(\d+) cmds, overlay (\d+) \| band (\S+)')

ARMS = [('cache-on', {}), ('cache-off', {'LUMEN_NO_CHROME_OVERLAY_CACHE': '1'})]


def parse(log_path: str) -> dict:
    chrome_ms: list[float] = []
    strips_used: set[int] = set()
    cmds: set[int] = set()
    with open(log_path, encoding='utf-8', errors='replace') as fh:
        for line in fh:
            m = BUILD_RE.search(line)
            if not m:
                continue
            chrome_ms.append(float(m.group(1)))
            strips_used.add(int(m.group(6)))
            cmds.add(int(m.group(7)))
    return {'chrome_ms': chrome_ms, 'strips_used': strips_used, 'cmds': cmds}


def main() -> int:
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument('--repeats', type=int, default=3)
    ap.add_argument('--ticks', type=int, default=60)
    ap.add_argument('--delta', type=float, default=120.0)
    ap.add_argument('--backend', default='vulkan')
    ap.add_argument('--page', default='samples/bench-text-scroll.html')
    args = ap.parse_args()

    rows: dict[str, list[dict]] = {tag: [] for tag, _ in ARMS}
    for rep in range(args.repeats):
        shift = rep % len(ARMS)
        for tag, extra in ARMS[shift:] + ARMS[:shift]:
            log = run(1.0, rep, args.ticks, args.delta, args.backend, args.page,
                      extra_env=extra, tag=f'chov_{tag}')
            rows[tag].append(parse(log))

    print('\n' + '=' * 60)
    print(f'{"плечо":10s} | {"chrome p50":>10s} | {"min of reps":>11s} | {"кадров":>7s}')
    mins: dict[str, float] = {}
    for tag, _ in ARMS:
        all_ms = [v for r in rows[tag] for v in r['chrome_ms']]
        if not all_ms:
            print(f'{tag:10s} | нет строк build: chrome — LUMEN_FRAME_LOG не сработал?')
            continue
        per_rep_p50 = [statistics.median(r['chrome_ms']) for r in rows[tag] if r['chrome_ms']]
        mins[tag] = min(per_rep_p50)
        print(f'{tag:10s} | {statistics.median(all_ms):10.4f} | {mins[tag]:11.4f} | {len(all_ms):7d}')

    if 'cache-on' in mins and 'cache-off' in mins and mins['cache-off'] > 0:
        saved = (1 - mins['cache-on'] / mins['cache-off']) * 100
        print(f'\ncache-on экономит {saved:.1f}% статьи `build: chrome` (по минимуму повторов)')

    # Тождество: набор (strips_used, cmds) не должен зависеть от рычага —
    # разница означала бы, что кэш подменяет геометрию/контент, а не только
    # пропускает копирование.
    for tag, _ in ARMS:
        strips = set().union(*(r['strips_used'] for r in rows[tag])) if rows[tag] else set()
        cmds = set().union(*(r['cmds'] for r in rows[tag])) if rows[tag] else set()
        print(f'{tag:10s}: strips_used={sorted(strips)} cmds={sorted(cmds)}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
