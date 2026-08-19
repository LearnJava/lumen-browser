#!/usr/bin/env python3
"""Перепись создаваемых текстур за РЕАЛЬНУЮ сессию (BUG-405 срез 26, вопрос п.54).

Слой offscreen-уровня (`layer_textures`, метка `opacity-layer`) создаётся
размером во всю цель рендера, и первое обращение к нему стоит 3.3–16 мс
(п. 49/50, срезы 24–25). Цена одноразовая НА ОБЪЕКТ текстуры, поэтому вопрос
«окупается ли правка класса „сдвинуть координаты всех пассов уровня“» сводится
к счётному: сколько таких объектов создаётся за сессию из нескольких реальных
страниц. Три за процесс — статья одноразовая, правка не окупается; десятки —
рекуррентная.

Гоняет ОДИН процесс браузера по списку сайтов (`docs/perf/corpus.txt`),
прокручивая каждый, и снимает после каждой страницы накопительную строку
`[frame:wgpu] alloc-census (по метке)`. Печатает приращение по странице —
именно приращение, а не итог, отвечает на вопрос «кто создаёт».

    python scripts/layer_alloc_census.py --sites 8 --backend vulkan

Перепись включается только под `LUMEN_FRAME_LOG=3` (там же заполняется
`TEXTURE_CENSUS`), поэтому лог получается большим — числа здесь СЧЁТНЫЕ,
временам этого прогона верить нельзя (уровень 3 печатает разбивку каждого
кадра, см. п. 19 остатка бага).

Загрузку страницы проверяем маркером на уходящем документе, а не ответом
`navigate` (BUG-438: неудачная навигация тоже отвечает успехом).
"""

from __future__ import annotations

import argparse
import json
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

LABEL_CENSUS_RE = re.compile(r'\[frame:wgpu\]\s+alloc-census \(по метке\): (.*)')
SIZE_CENSUS_RE = re.compile(r'\[frame:wgpu\]\s+alloc-census \(total\): (.*)')
LABEL_ITEM_RE = re.compile(r'([a-z\-]+) x(\d+) \((\d+) разм\.')
TOTAL_RE = re.compile(r'\[frame\] total\s+([\d.]+)ms\s+\(scroll_y [\-\d.]+, dl (\d+) cmds\)')
ADAPTER_RE = re.compile(r'\[wgpu\] adapter: (.*)')

MARKER = '__lumen_census_marker'


def load_corpus(path: str, limit: int) -> list[tuple[str, str]]:
    sites: list[tuple[str, str]] = []
    with open(path, encoding='utf-8') as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith('#'):
                continue
            parts = line.split(None, 1)
            if len(parts) == 2:
                sites.append((parts[0], parts[1].strip()))
    return sites[:limit] if limit else sites


def read_log(path: str) -> str:
    with open(path, encoding='utf-8', errors='replace') as f:
        return f.read()


def last_label_census(text: str) -> dict[str, tuple[int, int]]:
    """Последняя накопительная перепись по метке: label -> (создано, размерных классов)."""
    hits = LABEL_CENSUS_RE.findall(text)
    if not hits:
        return {}
    return {l: (int(n), int(c)) for l, n, c in LABEL_ITEM_RE.findall(hits[-1])}


def diff(before: dict[str, tuple[int, int]],
         after: dict[str, tuple[int, int]]) -> dict[str, int]:
    out: dict[str, int] = {}
    for label, (n, _) in after.items():
        d = n - before.get(label, (0, 0))[0]
        if d:
            out[label] = d
    return out


def drive(sites: list[tuple[str, str]], args) -> tuple[list[dict], str, str]:
    exe = os.path.join(REPO, 'target', 'dev-release', 'lumen.exe')
    if not os.path.exists(exe):
        raise SystemExit(f'нет {exe} — cargo build -p lumen-shell --profile dev-release')

    env = dict(os.environ)
    env['LUMEN_FRAME_LOG'] = '3'
    if args.backend:
        env['WGPU_BACKEND'] = args.backend

    tmp_dir = os.path.join(REPO, '.tmp')
    os.makedirs(tmp_dir, exist_ok=True)
    log_path = os.path.join(tmp_dir, f'layer_alloc_census_{args.backend or "auto"}.log')
    log_f = open(log_path, 'w', encoding='utf-8', errors='replace')
    port = free_port()
    proc = subprocess.Popen(
        [exe, '--maximized', '--mcp-live-port', str(port), 'about:blank'],
        cwd=REPO, env=env, stdout=subprocess.DEVNULL, stderr=log_f)

    rows: list[dict] = []
    try:
        c = Client(port, log_path)
        prev = last_label_census(read_log(log_path))
        frames_before = 0
        for slug, url in sites:
            # Маркер на уходящем документе — вспомогательный признак смены
            # документа: `navigate` отвечает успехом и на неудачной загрузке
            # (BUG-438), поэтому его ответу верить нельзя ни в каком виде.
            try:
                c.call('eval', {'code': f'window.{MARKER} = 1; 1'})
            except Exception:
                pass
            loaded, nodes, title = False, 0, ''
            host = url.split('//', 1)[-1].split('/', 1)[0].removeprefix('www.')
            try:
                c.call('navigate', {'url': url})
                c.call('wait', {'condition': 'document_ready', 'timeout_ms': args.timeout})
                # Признак загрузки — `location.href`, а не ответ `navigate`
                # (BUG-438) и не отсутствие маркера: контекст JS сменяется
                # позже готовности документа, и на первой же странице сессии
                # маркер `about:blank` переживает реальную навигацию.
                # Вторая проба — на этот лаг, а не на медленную сеть.
                for attempt in range(2):
                    time.sleep(args.settle)
                    probe = c.call('eval', {'code':
                        f'JSON.stringify({{href: location.href, '
                        f'stale: !!window.{MARKER}, '
                        f'nodes: document.querySelectorAll("*").length, '
                        f'title: (document.title || "").slice(0, 40)}})'})
                    raw = probe['result']
                    info = json.loads(json.loads(raw) if isinstance(raw, str) else raw)
                    cur_host = (info['href'].split('//', 1)[-1].split('/', 1)[0]
                                .removeprefix('www.'))
                    loaded = cur_host == host and not info['stale']
                    nodes, title = info['nodes'], info['title']
                    if loaded or attempt:
                        break
            except Exception as e:  # сайт мог не ответить — это данные, а не сбой
                title = f'ошибка: {type(e).__name__}'

            for direction in (+1, -1):
                for _ in range(args.ticks):
                    try:
                        c.call('scroll', {'target': 'body',
                                          'delta': {'x': 0, 'y': direction * args.delta}})
                    except Exception:
                        break
                    time.sleep(0.05)
            time.sleep(0.4)

            text = read_log(log_path)
            cur = last_label_census(text)
            all_frames = TOTAL_RE.findall(text)
            mine = all_frames[frames_before:]
            # `dl N cmds` — свидетельство самого движка о том, что кадр
            # что-то нарисовал, и оно надёжнее пробы через JS: контекст JS на
            # части реальных сайтов отдаёт пустой документ в момент, когда
            # движок уже нарисовал кадры и создал десятки текстур картинок.
            # Но и оно НЕ различает загрузку в одиночку: неудачная навигация
            # оставляет прежний документ, и `dl_max` повторяет число
            # предыдущей страницы — «загрузилась» = dl_max отличается ОТ
            # ПРЕДЫДУЩЕГО плюс ненулевое приращение переписи.
            dl_max = max((int(n) for _, n in mine), default=0)
            rows.append({'slug': slug, 'url': url, 'loaded': loaded, 'nodes': nodes,
                         'title': title, 'frames': len(mine), 'dl_max': dl_max,
                         'delta': diff(prev, cur)})
            prev, frames_before = cur, len(all_frames)
            print(f'  {slug:<14} кадров {len(mine):>4}  dl_max {dl_max:>6}  узлов {nodes:>6}  '
                  f'{"" if loaded else "js-контекст пуст  "}{rows[-1]["delta"]}', flush=True)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        log_f.close()

    text = read_log(log_path)
    adapter = (ADAPTER_RE.findall(text) or [''])[0]
    sizes = (SIZE_CENSUS_RE.findall(text) or [''])[-1]
    return rows, adapter, sizes


def report(rows: list[dict], adapter: str, sizes: str) -> None:
    labels: list[str] = []
    for r in rows:
        for l in r['delta']:
            if l not in labels:
                labels.append(l)
    labels.sort(key=lambda l: -sum(r['delta'].get(l, 0) for r in rows))

    print(f'\n=== перепись за сессию ===\n  адаптер: {adapter}')
    head = (f'{"страница":<14}{"кадров":>7}{"dl_max":>8}{"узлов":>8}  '
            + ''.join(f'{l:>18}' for l in labels))
    print(head)
    for r in rows:
        cells = ''.join(f'{r["delta"].get(l, 0):>18}' for l in labels)
        print(f'{r["slug"]:<14}{r["frames"]:>7}{r["dl_max"]:>8}{r["nodes"]:>8}  {cells}')
    total = ''.join(f'{sum(r["delta"].get(l, 0) for r in rows):>18}' for l in labels)
    print(f'{"ИТОГО":<14}{sum(r["frames"] for r in rows):>7}{"":>8}{"":>8}  {total}')
    drew = [r for r in rows if r['dl_max'] > 20]
    print(f'\n  страниц, действительно нарисовавших содержимое (dl_max > 20): '
          f'{len(drew)} из {len(rows)} — {", ".join(r["slug"] for r in drew)}')
    print(f'\n  топ-8 по (метка, размер) за процесс: {sizes}')


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('--corpus', default=os.path.join(REPO, 'docs', 'perf', 'corpus.txt'))
    ap.add_argument('--sites', type=int, default=0, help='взять первые N строк корпуса (0 — все)')
    ap.add_argument('--ticks', type=int, default=10)
    ap.add_argument('--delta', type=float, default=300.0)
    ap.add_argument('--settle', type=float, default=2.0)
    ap.add_argument('--timeout', type=int, default=25000)
    ap.add_argument('--backend', default='vulkan',
                    help='vulkan|dx12|gl — пинует бэкенд; счётные числа от него не зависят')
    args = ap.parse_args()

    sites = load_corpus(args.corpus, args.sites)
    if not sites:
        raise SystemExit(f'пустой корпус {args.corpus}')
    print(f'сайтов: {len(sites)}, тиков прокрутки: {args.ticks}×2')
    rows, adapter, sizes = drive(sites, args)
    report(rows, adapter, sizes)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
