#!/usr/bin/env python3
"""Перепись BUG-935 срез 1: сколько off-thread relayout'ов на живом сайте
с активным rAF+DOM-циклом (ria.ru), сколько из них settle-фаза против
прокрутки, и какой RTT платит MCP `scroll` пока движковый поток занят.

Метод — BUG-286 «Замер 2026-08-06», повторён 2026-09-01 для BUG-935.
Этот скрипт формализует его в код, чтобы срез был воспроизводим без
ручного чтения stderr.

    python scripts/bug935_raf_relayout_census.py [url] [--settle-s N]
                                                  [--ticks N] [--build]
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

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, 'reconfigure'):
        _stream.reconfigure(encoding='utf-8', errors='replace')

RELAYOUT_RE = re.compile(
    r'\[engine\] relayout ([\d.]+)ms \(off-thread\) dl=(\d+) styled=(\d+)'
)

# BUG-935 S12: `relayout_raf_dirty`/`_readback` now try the on-thread
# incremental path first (see `try_relayout_raf_incremental`'s new logging) —
# a census comparing before/after the routing swap must count both kinds, not
# just the off-thread one the original census (S11) was written against.
# BUG-935 S14: the incremental line also reports `restyle={0|1}` (S13) — the
# cheap cascade-skip branch vs. the expensive full-cascade fallback branch —
# an optional trailing group so the off-thread line (which has no such field)
# still matches.
ANY_RELAYOUT_RE = re.compile(
    r'\[engine\] relayout ([\d.]+)ms \((off-thread|incremental, on-thread)\) '
    r'dl=(\d+) styled=(\d+)(?: restyle=(\d))?'
)


def free_port() -> int:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(('127.0.0.1', 0))
    port = s.getsockname()[1]
    s.close()
    return port


def _wait_for_mcp_token(stderr_log: str, timeout_s: float = 20.0) -> str:
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        try:
            with open(stderr_log, encoding='utf-8', errors='replace') as fh:
                for line in fh:
                    if line.startswith('[mcp] token: '):
                        return line.strip()[len('[mcp] token: '):]
        except OSError:
            pass
        time.sleep(0.1)
    raise RuntimeError(f'lumen --mcp-live-port token not found in {stderr_log}')


class Client:
    def __init__(self, port: int, stderr_log: str) -> None:
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
        self.sock.settimeout(150)
        self._reader = self.sock.makefile('r', encoding='utf-8', newline='\n')
        self._id = 0
        token = _wait_for_mcp_token(stderr_log)
        self._raw_call('initialize', {'token': token})

    def _raw_call(self, method: str, params: dict) -> dict:
        self._id += 1
        req = json.dumps({'jsonrpc': '2.0', 'id': self._id,
                          'method': method, 'params': params})
        self.sock.sendall((req + '\n').encode('utf-8'))
        line = self._reader.readline()
        if not line:
            raise RuntimeError('MCP-соединение закрыто (окно упало?)')
        resp = json.loads(line)
        if resp.get('error') is not None:
            raise RuntimeError(f'{method}: {resp["error"]}')
        return resp.get('result') or {}

    def call(self, name: str, arguments: dict) -> dict:
        return self._raw_call('tools/call', {'name': name, 'arguments': arguments})


def parse_relayouts(log_path: str) -> list[tuple[float, int, int]]:
    out = []
    with open(log_path, encoding='utf-8', errors='replace') as f:
        for line in f:
            m = RELAYOUT_RE.search(line)
            if m:
                out.append((float(m.group(1)), int(m.group(2)), int(m.group(3))))
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('url', nargs='?', default='https://ria.ru')
    ap.add_argument('--settle-s', type=float, default=15.0,
                     help='сколько секунд ждать после document_ready до первого scroll')
    ap.add_argument('--ticks', type=int, default=20)
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

    port = free_port()
    env = dict(os.environ)
    env['LUMEN_PROFILE_TREE'] = '1'
    env['LUMEN_FRAME_LOG'] = '1'
    tmp_dir = os.path.join(REPO, '.tmp')
    os.makedirs(tmp_dir, exist_ok=True)
    log_path = os.path.join(tmp_dir, 'bug935_census_stderr.log')
    log_f = open(log_path, 'w', encoding='utf-8', errors='replace')

    proc = subprocess.Popen([exe, '--mcp-live-port', str(port), '--maximized', args.url],
                             cwd=REPO, env=env,
                             stdout=subprocess.DEVNULL, stderr=log_f)
    scroll_rtts = []
    try:
        c = Client(port, log_path)
        try:
            c.call('wait', {'condition': 'document_ready', 'timeout_ms': 20000})
        except RuntimeError as e:
            print(f'wait document_ready не дождался ({e}) — продолжаю по фиксированной паузе', file=sys.stderr)

        # `document_ready` замер по document.readyState гоняется через engine
        # thread и может опередить реальную покраску контента (dl=0, ещё только
        # chrome); ждём первого кадра с непустым content-display-list как более
        # надёжного свидетеля «страница реально нарисована» (perf-method.md
        # §2 «свидетеля события бери в той подсистеме, которую меряешь»).
        content_deadline = time.time() + 90.0
        painted = False
        while time.time() < content_deadline:
            with open(log_path, encoding='utf-8', errors='replace') as fh:
                for line in fh:
                    m = re.search(r'\[frame\] total\s+[\d.]+ms\s+\(scroll_y [\-\d.]+, dl (\d+) cmds\)', line)
                    if m and int(m.group(1)) > 0:
                        painted = True
                        break
            if painted:
                break
            time.sleep(1.0)
        if not painted:
            print('контент так и не нарисован за 90с (dl остался 0) — census будет пустым', file=sys.stderr)

        settle_mark_byte = os.path.getsize(log_path)
        print(f'painted={painted}; жду {args.settle_s:.0f}с settle...')
        time.sleep(args.settle_s)
        scroll_mark_byte = os.path.getsize(log_path)

        for _ in range(args.ticks):
            t0 = time.time()
            c.call('scroll', {'target': {'selector': 'body'}, 'delta': {'x': 0, 'y': 400}})
            scroll_rtts.append((time.time() - t0) * 1000.0)
            time.sleep(0.2)
        time.sleep(1.0)
        end_byte = os.path.getsize(log_path)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        log_f.close()

    def read_span(start: int, end: int) -> list[tuple[float, str, int, int, str]]:
        with open(log_path, encoding='utf-8', errors='replace') as f:
            f.seek(start)
            data = f.read(end - start)
        return [(float(m.group(1)), m.group(2), int(m.group(3)), int(m.group(4)),
                  m.group(5) if m.group(5) is not None else '-')
                for m in ANY_RELAYOUT_RE.finditer(data)]

    before_settle = read_span(0, settle_mark_byte)
    during_settle = read_span(settle_mark_byte, scroll_mark_byte)
    during_scroll = read_span(scroll_mark_byte, end_byte)

    print(f'\nURL: {args.url}')
    print(f'relayout до document_ready:     {len(before_settle)}')
    print(f'relayout за {args.settle_s:.0f}с settle (без scroll): {len(during_settle)}')
    print(f'relayout за {args.ticks} scroll-тиков:  {len(during_scroll)}')

    for name, rows in (('settle', during_settle), ('scroll', during_scroll)):
        if rows:
            by_kind: dict[str, list[tuple[float, str, int, int, str]]] = {}
            for r in rows:
                by_kind.setdefault(r[1], []).append(r)
            for kind, krows in sorted(by_kind.items()):
                ms = [r[0] for r in krows]
                dls = sorted(set(r[2] for r in krows))
                styleds = sorted(set(r[3] for r in krows))
                extra = ''
                if kind == 'incremental, on-thread':
                    restyled = sum(1 for r in krows if r[4] == '1')
                    extra = f', restyle=1 in {restyled}/{len(krows)}'
                print(f'  {name} [{kind}] x{len(krows)}: relayout ms min/avg/max = '
                      f'{min(ms):.1f}/{statistics.mean(ms):.1f}/{max(ms):.1f}, '
                      f'dl in {dls}, styled in {styleds}{extra}')

    if scroll_rtts:
        print(f'\nscroll RTT ms: min={min(scroll_rtts):.1f} '
              f'avg={statistics.mean(scroll_rtts):.1f} max={max(scroll_rtts):.1f}')
        print('  все значения: ' + ', '.join(f'{x:.0f}' for x in scroll_rtts))

    print(f'\nполный лог: {log_path}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
