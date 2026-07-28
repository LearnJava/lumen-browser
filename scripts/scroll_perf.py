#!/usr/bin/env python3
"""Замер плавности прокрутки в живом окне Lumen.

Поднимает `lumen.exe --mcp-live-port N <page>` с `LUMEN_FRAME_LOG=1`,
прокручивает страницу вниз и обратно вверх MCP-инструментом `scroll`
(эмуляция колеса мыши), собирает покадровый лог `[frame] …` из stderr
и печатает статистику: среднее/медиану/максимум времени кадра и
эффективный FPS во время прокрутки.

Использование:
    python scripts/scroll_perf.py [page] [--ticks N] [--delta PX]
                                  [--profile dev-release] [--build]

    page      — HTML-файл или URL (по умолчанию graphic_tests/1000000-final.html)
    --ticks   — число «щелчков колеса» в каждую сторону (по умолчанию 25)
    --delta   — CSS-пикселей на щелчок (по умолчанию 300)
    --build   — пересобрать lumen-shell перед запуском

Формат строк лога (см. `frame_log_enabled` в lumen-paint; `culled N/M leaf` —
опциональный хвост, есть только на femtovg-бэкенде):
    [frame] paint  12.34ms  (content  10.11ms / 8432 cmds, overlay  0.42ms / 12 cmds, flush  1.55ms, swap  0.26ms, culled 0/12 leaf)
    [frame] total  15.01ms  (scroll_y 1200, dl 8430 cmds)
"""

from __future__ import annotations

import argparse
import json
import os
import re
import socket
import statistics
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Windows-консоль по умолчанию cp1251 — переключаем вывод на UTF-8,
# чтобы русские строки и спецсимволы не роняли скрипт.
for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, 'reconfigure'):
        _stream.reconfigure(encoding='utf-8', errors='replace')

PAINT_RE = re.compile(
    r'\[frame\] paint\s+([\d.]+)ms\s+\(content\s+([\d.]+)ms / (\d+) cmds, '
    r'overlay\s+([\d.]+)ms / (\d+) cmds, flush\s+([\d.]+)ms, swap\s+([\d.]+)ms'
    r'(?:, culled \d+/\d+ leaf)?\)'
)
TOTAL_RE = re.compile(r'\[frame\] total\s+([\d.]+)ms\s+\(scroll_y ([\-\d.]+), dl (\d+) cmds\)')


def free_port() -> int:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(('127.0.0.1', 0))
    port = s.getsockname()[1]
    s.close()
    return port


class Client:
    """Line-delimited JSON-RPC клиент к `--mcp-live-port` (тот же протокол, что run.py --live)."""

    def __init__(self, port: int) -> None:
        last: Exception | None = None
        for _ in range(200):
            try:
                self.sock = socket.create_connection(('127.0.0.1', port), timeout=5)
                break
            except OSError as e:
                last = e
                time.sleep(0.1)
        else:
            raise RuntimeError(f'MCP-порт {port} не поднялся: {last}')
        self.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self.sock.settimeout(60)
        self._reader = self.sock.makefile('r', encoding='utf-8', newline='\n')
        self._id = 0

    def call(self, name: str, arguments: dict) -> dict:
        self._id += 1
        req = json.dumps({'jsonrpc': '2.0', 'id': self._id,
                          'method': 'tools/call',
                          'params': {'name': name, 'arguments': arguments}})
        self.sock.sendall((req + '\n').encode('utf-8'))
        line = self._reader.readline()
        if not line:
            raise RuntimeError('MCP-соединение закрыто (окно упало?)')
        resp = json.loads(line)
        if resp.get('error') is not None:
            raise RuntimeError(f'{name}: {resp["error"]}')
        return resp.get('result') or {}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('page', nargs='?', default='graphic_tests/1000000-final.html')
    ap.add_argument('--ticks', type=int, default=25)
    ap.add_argument('--delta', type=float, default=300.0)
    ap.add_argument('--profile', default='dev-release')
    ap.add_argument('--build', action='store_true')
    args = ap.parse_args()

    if args.build:
        rc = subprocess.call(['cargo', 'build', '-p', 'lumen-shell', '--profile', args.profile], cwd=REPO)
        if rc != 0:
            return rc

    exe = os.path.join(REPO, 'target', args.profile, 'lumen.exe')
    if not os.path.exists(exe):
        print(f'нет бинарника {exe} — запустите с --build', file=sys.stderr)
        return 1

    page = args.page
    if not page.startswith(('http://', 'https://', 'file://', 'about:')):
        page = 'file:///' + os.path.abspath(os.path.join(REPO, page)).replace('\\', '/')

    port = free_port()
    env = dict(os.environ)
    env.setdefault('LUMEN_FRAME_LOG', '1')
    tmp_dir = os.path.join(REPO, '.tmp')
    os.makedirs(tmp_dir, exist_ok=True)
    log_path = os.path.join(tmp_dir, 'scroll_perf_stderr.log')
    log_f = open(log_path, 'w', encoding='utf-8', errors='replace')

    proc = subprocess.Popen([exe, '--mcp-live-port', str(port), 'about:blank'],
                            cwd=REPO, env=env,
                            stdout=subprocess.DEVNULL, stderr=log_f)
    try:
        c = Client(port)
        c.call('navigate', {'url': page})
        c.call('wait', {'condition': 'document_ready', 'timeout_ms': 20000})
        time.sleep(1.0)  # дать странице дорисоваться / докачать ресурсы

        t_scroll_start = time.time()
        for direction in (+1, -1):
            for _ in range(args.ticks):
                c.call('scroll', {'target': {'css': 'body'},
                                  'delta': {'x': 0, 'y': direction * args.delta}})
                time.sleep(0.03)  # ~33 щелчка/с — быстрая ручная прокрутка
        t_scroll = time.time() - t_scroll_start
        time.sleep(0.5)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        log_f.close()

    # ── Разбор лога ──────────────────────────────────────────────────────
    paints, totals = [], []
    with open(log_path, encoding='utf-8', errors='replace') as f:
        for line in f:
            m = PAINT_RE.search(line)
            if m:
                paints.append(tuple(float(x) for x in m.groups()))
                continue
            m = TOTAL_RE.search(line)
            if m:
                totals.append((float(m.group(1)), float(m.group(2)), int(m.group(3))))

    if not totals:
        print(f'кадров в логе нет — смотрите {log_path}', file=sys.stderr)
        return 1

    tot = [t[0] for t in totals]
    pt = [p[0] for p in paints]
    content = [p[1] for p in paints]
    flush = [p[5] for p in paints]
    cmds = [int(p[2]) for p in paints]

    def stats(name: str, xs: list[float]) -> None:
        print(f'  {name:<10} avg {statistics.mean(xs):7.2f}ms   '
              f'p50 {statistics.median(xs):7.2f}ms   max {max(xs):7.2f}ms')

    print(f'\nстраница: {page}')
    print(f'кадров: {len(totals)}  (прокрутка {2 * args.ticks} щелчков × {args.delta:.0f}px за {t_scroll:.1f}s)')
    print(f'display list: {min(cmds)}–{max(cmds)} команд')
    stats('total', tot)
    if pt:
        stats('paint', pt)
        stats('content', content)
        stats('flush', flush)
    fps = 1000.0 / statistics.mean(tot)
    print(f'  эффективный потолок FPS по среднему кадру: {fps:.1f}')
    print(f'\nполный лог: {log_path}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
