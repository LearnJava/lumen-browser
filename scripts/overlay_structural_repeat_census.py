#!/usr/bin/env python3
"""Перепись честной частоты `overlay-cache HIT` (BUG-405 срез 46, пункт 85).

Срезы 43-45 назвали и закрыли остаток невязки через A/B по РАЗНИЦЕ
wall-clock (рычаг `LUMEN_NO_OVERLAY_CACHE`) — это ответило "хэш стоит
X мс", но не ответило на вопрос п. 85: "как часто хвост overlay реально
совпадает кадр-в-кадр" — честный потолок выигрыша content_epoch-подобного
механизма (урок S30/S36 `docs/perf-method.md`: "перепиши, как часто ключ
реально повторяется — точным битовым равенством, прежде чем строить
кэш/архитектуру поверх него").

Здесь считается НАПРЯМУЮ, без вывода по времени: `overlay_cache_step`
(`crates/engine/paint/src/renderer/band_compose.rs`) на каждом кадре при
`LUMEN_FRAME_LOG=2` печатает ровно один из пяти исходов:

    overlay-cache HIT prefix=N            — хвост совпал (битовое равенство
                                             tail_digests), это и есть
                                             "структурное совпадение"
    overlay-cache STALE prefix=N          — кэш был, но хвост разошёлся
    overlay-cache MISS built prefix=N     — кэша не было, точка разреза
                                             найдена и построена заново
    overlay-cache no-change-info same_len=B — не с чем сравнить (первый
                                             кадр / длина списка сменилась)
    overlay-cache tail-empty prefix=N len=M — новая точка разреза съела
                                             весь список, кэшировать нечего

Длина overlay-списка того же кадра печатается ПОСЛЕ (строка `[frame]
build: ... overlay N | ...`, уровень 1) — обе строки одного кадра
сопоставляются по порядку появления в логе (без промежуточного
overlay-cache-события между ними).

Прогон — РЕАЛИСТИЧНАЯ смешанная прокрутка (не однородные тики среза
43-45): переменные дельты, случайные развороты направления, паузы разной
длины — имитация ручной прокрутки, а не программного цикла. Одна связная
сессия, не interleaved A/B (здесь нечего сравнивать между плечами — только
считать частоты одного механизма).

    python scripts/overlay_structural_repeat_census.py --backend vulkan \
        --seconds 20 --page graphic_tests/1000000-final.html

Бэкенд задавать обязательно — оверлей-кэш существует только на wgpu-пути
(срез 14: DX12/Vulkan числа несопоставимы, но здесь частоты, а не мс,
поэтому бэкенд влияет только на то, включён ли путь вообще).
"""
from __future__ import annotations

import argparse
import os
import random
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

HIT_RE = re.compile(r'overlay-cache HIT prefix=(\d+)')
STALE_RE = re.compile(r'overlay-cache STALE prefix=(\d+)')
MISS_RE = re.compile(r'overlay-cache MISS built prefix=(\d+)')
NOCHANGE_RE = re.compile(r'overlay-cache no-change-info same_len=(\w+)')
TAILEMPTY_RE = re.compile(r'overlay-cache tail-empty prefix=(\d+) len=(\d+)')
BUILD_RE = re.compile(r'\[frame\]\s+build:.*\boverlay (\d+) \|')


def run_session(exe: str, page: str, backend: str, seconds: float, seed: int) -> str:
    """Один прогон: смешанная прокрутка `seconds` секунд, путь к stderr-логу."""
    env = dict(os.environ)
    env['LUMEN_FRAME_LOG'] = '2'
    if backend:
        env['WGPU_BACKEND'] = backend

    url = page
    if not url.startswith(('http://', 'https://', 'file://', 'about:')):
        url = 'file:///' + os.path.abspath(os.path.join(REPO, page)).replace('\\', '/')

    port = free_port()
    tmp_dir = os.path.join(REPO, '.tmp')
    os.makedirs(tmp_dir, exist_ok=True)
    log_path = os.path.join(tmp_dir, f'overlay_repeat_census_{backend or "auto"}.log')
    log_f = open(log_path, 'w', encoding='utf-8', errors='replace')
    proc = subprocess.Popen(
        [exe, '--maximized', '--mcp-live-port', str(port), 'about:blank'],
        cwd=REPO, env=env, stdout=subprocess.DEVNULL, stderr=log_f)
    try:
        c = Client(port, log_path)
        c.call('navigate', {'url': url})
        c.call('wait', {'condition': 'document_ready', 'timeout_ms': 20000})
        time.sleep(1.5)

        rng = random.Random(seed)
        direction = 1
        t_end = time.time() + seconds
        while time.time() < t_end:
            # Реалистичная прокрутка: переменная дельта (40-260px), редкий
            # разворот направления (~12% щелчков), редкая пауза (~8%,
            # 0.2-0.6с — "остановился прочитать"), иначе короткий интервал
            # (~40-90мс — ручное вращение колеса).
            if rng.random() < 0.12:
                direction *= -1
            delta = rng.uniform(40.0, 260.0) * direction
            c.call('scroll', {'target': 'body', 'delta': {'x': 0, 'y': delta}})
            if rng.random() < 0.08:
                time.sleep(rng.uniform(0.2, 0.6))
            else:
                time.sleep(rng.uniform(0.04, 0.09))
        time.sleep(0.4)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        log_f.close()
    return log_path


def parse(log_path: str) -> list[dict]:
    """Список событий overlay-cache одной сессии, с overlay_len где известно."""
    events: list[dict] = []
    pending: dict | None = None
    with open(log_path, encoding='utf-8', errors='replace') as fh:
        for line in fh:
            m = HIT_RE.search(line)
            if m:
                pending = {'kind': 'HIT', 'prefix_len': int(m.group(1)), 'overlay_len': None}
                events.append(pending)
                continue
            m = STALE_RE.search(line)
            if m:
                pending = {'kind': 'STALE', 'prefix_len': int(m.group(1)), 'overlay_len': None}
                events.append(pending)
                continue
            m = MISS_RE.search(line)
            if m:
                pending = {'kind': 'MISS', 'prefix_len': int(m.group(1)), 'overlay_len': None}
                events.append(pending)
                continue
            m = NOCHANGE_RE.search(line)
            if m:
                pending = {'kind': 'NO-CHANGE-INFO', 'prefix_len': None, 'overlay_len': None}
                events.append(pending)
                continue
            m = TAILEMPTY_RE.search(line)
            if m:
                pending = {'kind': 'TAIL-EMPTY', 'prefix_len': int(m.group(1)),
                           'overlay_len': int(m.group(2))}
                events.append(pending)
                pending = None  # длина уже известна из самой строки
                continue
            m = BUILD_RE.search(line)
            if m and pending is not None:
                pending['overlay_len'] = int(m.group(1))
                pending = None
    return events


def report(label: str, events: list[dict]) -> dict:
    total = len(events)
    by_kind: dict[str, int] = {}
    for e in events:
        by_kind[e['kind']] = by_kind.get(e['kind'], 0) + 1
    print(f'\n{label}  событий overlay-cache: {total}')
    for kind in ('HIT', 'STALE', 'MISS', 'NO-CHANGE-INFO', 'TAIL-EMPTY'):
        n = by_kind.get(kind, 0)
        pct = 100.0 * n / total if total else 0.0
        print(f'  {kind:16s} {n:5d}  ({pct:5.1f}%)')
    return {'total': total, 'by_kind': by_kind, 'events': events}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument('--backend', default='vulkan')
    ap.add_argument('--page', default='graphic_tests/1000000-final.html')
    ap.add_argument('--seconds', type=float, default=20.0)
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--repeats', type=int, default=1)
    args = ap.parse_args()

    exe = os.path.join(REPO, 'target', 'dev-release', 'lumen.exe')
    if not os.path.exists(exe):
        print(f'нет {exe} — cargo build -p lumen-shell --profile dev-release', file=sys.stderr)
        return 1
    stand = os.path.join(REPO, args.page)
    if not (args.page.startswith(('http://', 'https://', 'about:')) or os.path.exists(stand)):
        print(f'нет стенда {stand}', file=sys.stderr)
        return 1

    print(f'стенд: {args.page}  бэкенд: {args.backend}  сессия: {args.seconds:.0f}с × {args.repeats}')

    all_events: list[dict] = []
    for rep in range(args.repeats):
        seed = args.seed + rep
        log_path = run_session(exe, args.page, args.backend, args.seconds, seed)
        events = parse(log_path)
        if not events:
            print(f'событий overlay-cache в логе нет — смотрите {log_path}', file=sys.stderr)
            continue
        report(f'повтор {rep} (сид {seed}, {log_path}):', events)
        all_events.extend(events)

    if not all_events:
        return 1

    agg = report('=== ИТОГО ===', all_events)
    total = agg['total']

    hits = [e for e in all_events if e['kind'] == 'HIT' and e['overlay_len']]
    if hits:
        fracs = [1.0 - e['prefix_len'] / e['overlay_len'] for e in hits]
        fracs.sort()
        n = len(fracs)
        p50 = fracs[n // 2]
        p10 = fracs[max(0, int(n * 0.10))]
        p90 = fracs[min(n - 1, int(n * 0.90))]
        print(f'\nна HIT-кадрах ({len(hits)} шт.): доля overlay-списка в стабильном '
              f'(кэшируемом) хвосте (overlay_len-prefix_len)/overlay_len:')
        print(f'  p10 {p10:.3f}   p50 {p50:.3f}   p90 {p90:.3f}   (1.0 = весь список стабилен)')

    print(f'\nвсего событий: {total}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
