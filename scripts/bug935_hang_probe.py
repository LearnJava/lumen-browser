#!/usr/bin/env python3
"""BUG-935 срез 3: во время живого репро ловит первый признак «тишины» в логе
(N секунд без новой строки после первой отрисовки контента) и в этот момент
пробует MCP-вызов с коротким таймаутом — отличает «весь процесс встал»
(вызов не отвечает вовсе) от «встал только render/JS-поток, IPC жив»
(вызов отвечает, пусть и с большой задержкой).

    python scripts/bug935_hang_probe.py [url] [--quiet-s N] [--max-wait-s N]
"""

from __future__ import annotations

import argparse
import json
import os
import re
import socket
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, 'reconfigure'):
        _stream.reconfigure(encoding='utf-8', errors='replace')


def free_port() -> int:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(('127.0.0.1', 0))
    port = s.getsockname()[1]
    s.close()
    return port


def wait_for_mcp_token(stderr_log: str, timeout_s: float = 20.0) -> str:
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
    def __init__(self, port: int, stderr_log: str, call_timeout: float = 150.0) -> None:
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
        self.sock.settimeout(call_timeout)
        self._reader = self.sock.makefile('r', encoding='utf-8', newline='\n')
        self._id = 0
        token = wait_for_mcp_token(stderr_log)
        self._raw_call('initialize', {'token': token})

    def _raw_call(self, method: str, params: dict, timeout: float | None = None) -> dict:
        self._id += 1
        req = json.dumps({'jsonrpc': '2.0', 'id': self._id,
                          'method': method, 'params': params})
        if timeout is not None:
            self.sock.settimeout(timeout)
        self.sock.sendall((req + '\n').encode('utf-8'))
        line = self._reader.readline()
        if not line:
            raise RuntimeError('MCP-соединение закрыто (окно упало?)')
        resp = json.loads(line)
        if resp.get('error') is not None:
            raise RuntimeError(f'{method}: {resp["error"]}')
        return resp.get('result') or {}

    def call(self, name: str, arguments: dict, timeout: float | None = None) -> dict:
        return self._raw_call('tools/call', {'name': name, 'arguments': arguments}, timeout=timeout)

    def read_resource(self, uri: str, timeout: float | None = None) -> dict:
        return self._raw_call('resources/read', {'uri': uri}, timeout=timeout)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n', 1)[0])
    ap.add_argument('url', nargs='?', default='https://ria.ru')
    ap.add_argument('--quiet-s', type=float, default=15.0,
                     help='сколько секунд без новой строки в логе считать зависанием')
    ap.add_argument('--max-wait-s', type=float, default=180.0,
                     help='общий потолок ожидания зависания после первой покраски')
    ap.add_argument('--probe-timeout-s', type=float, default=20.0,
                     help='таймаут MCP-вызова во время подозреваемого зависания')
    ap.add_argument('--profile', default='dev-release')
    args = ap.parse_args()

    exe = os.path.join(REPO, 'target', args.profile, 'lumen.exe')
    if not os.path.exists(exe):
        print(f'нет бинарника {exe}', file=sys.stderr)
        return 1

    port = free_port()
    env = dict(os.environ)
    env['LUMEN_PROFILE_TREE'] = '1'
    env['LUMEN_FRAME_LOG'] = '1'
    tmp_dir = os.path.join(REPO, '.tmp')
    os.makedirs(tmp_dir, exist_ok=True)
    log_path = os.path.join(tmp_dir, 'bug935_hang_probe_stderr.log')
    log_f = open(log_path, 'w', encoding='utf-8', errors='replace')

    proc = subprocess.Popen([exe, '--mcp-live-port', str(port), '--maximized', args.url],
                             cwd=REPO, env=env,
                             stdout=subprocess.DEVNULL, stderr=log_f)
    try:
        c = Client(port, log_path, call_timeout=25.0)
        try:
            c.call('wait', {'condition': 'document_ready', 'timeout_ms': 20000}, timeout=25.0)
        except RuntimeError as e:
            print(f'wait document_ready не дождался ({e}) — продолжаю', file=sys.stderr)

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
        print(f'painted={painted}')

        # Ждём тишины в логе (quiet_s без новой строки) или общего потолка.
        last_size = os.path.getsize(log_path)
        last_change = time.time()
        overall_deadline = time.time() + args.max_wait_s
        quiet_detected = False
        while time.time() < overall_deadline:
            time.sleep(1.0)
            size = os.path.getsize(log_path)
            if size != last_size:
                last_size = size
                last_change = time.time()
            elif time.time() - last_change >= args.quiet_s:
                quiet_detected = True
                break
        print(f'quiet_detected={quiet_detected} после {time.time() - last_change:.0f}с тишины'
              if quiet_detected else 'quiet_detected=False (лог не замолкал за max-wait-s)')

        if quiet_detected:
            try:
                tl = subprocess.run(['tasklist', '/FI', f'PID eq {proc.pid}', '/FO', 'CSV'],
                                     capture_output=True, text=True, timeout=5)
                print(f'tasklist до пробы: {tl.stdout.strip().splitlines()[-1] if tl.stdout else "?"}')
            except (subprocess.SubprocessError, OSError) as e:
                print(f'tasklist недоступен: {e}')
            print(f'проверяю MCP-порт через resource://console (timeout={args.probe_timeout_s:.0f}с)...')
            t0 = time.time()
            try:
                res = c.read_resource('resource://console', timeout=args.probe_timeout_s)
                dt = time.time() - t0
                print(f'MCP ОТВЕТИЛ за {dt:.1f}с — IPC-поток и обработчик AutomationCommand ЖИВЫ; '
                      f'зависание локализовано где-то в UI event loop / engine-thread пути ЗА '
                      f'пределами простого чтения буфера console-логов. result keys: {list(res.keys())[:5]}')
            except (socket.timeout, TimeoutError):
                dt = time.time() - t0
                print(f'MCP НЕ ОТВЕТИЛ за {dt:.1f}с — либо winit event loop (UI-поток) не '
                      f'дренирует AutomationCommand-канал вовсе, либо handle.execute сам ждёт '
                      f'что-то (DEFAULT_TIMEOUT=30с в live_session.rs, если больше — таймаут '
                      f'сработал бы раньше моего probe-timeout и вернул RuntimeError, не socket.timeout)')
            except RuntimeError as e:
                print(f'MCP вызов вернул ошибку (не таймаут): {e}')
            try:
                tl = subprocess.run(['tasklist', '/FI', f'PID eq {proc.pid}', '/FO', 'CSV'],
                                     capture_output=True, text=True, timeout=5)
                print(f'tasklist после пробы: {tl.stdout.strip().splitlines()[-1] if tl.stdout else "?"}')
            except (subprocess.SubprocessError, OSError) as e:
                print(f'tasklist недоступен: {e}')
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        log_f.close()

    print(f'\nполный лог: {log_path}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
