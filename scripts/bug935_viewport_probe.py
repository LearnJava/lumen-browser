#!/usr/bin/env python3
"""BUG-935 срез 99: что видит движок после загрузки — `window.innerHeight` и
`document.documentElement.clientHeight` через MCP `eval`. Правильное значение
для окна с хромом — высота окна минус CHROME_H (945 при inner 1017); сырая
высота окна (1017) — дефект загрузочных путей (page_load.rs / user_event.rs).

    py -3 scripts/bug935_viewport_probe.py [url]
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from bug935_raf_relayout_census import Client, free_port  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main() -> int:
    url = sys.argv[1] if len(sys.argv) > 1 else 'https://rbc.ru'
    exe = os.path.join(REPO, 'target', 'dev-release', 'lumen.exe')
    port = free_port()
    env = dict(os.environ)
    env['LUMEN_FRAME_LOG'] = '1'
    env.setdefault('LUMEN_NO_ADBLOCK', '1')
    tmp_dir = os.path.join(REPO, '.tmp')
    os.makedirs(tmp_dir, exist_ok=True)
    log_path = os.path.join(tmp_dir, 'bug935_viewport_probe.log')
    log_f = open(log_path, 'w', encoding='utf-8', errors='replace')
    proc = subprocess.Popen(
        [exe, '--mcp-live-port', str(port), '--maximized', url],
        cwd=REPO, env=env, stdout=subprocess.DEVNULL, stderr=log_f)
    try:
        c = Client(port, log_path)
        try:
            c.call('wait', {'condition': 'document_ready', 'timeout_ms': 20000})
        except RuntimeError as e:
            print(f'wait document_ready не дождался ({e})', file=sys.stderr)
        time.sleep(10.0)  # settle: финальный pipeline + первые флаши
        for expr in (
            'window.innerHeight',
            'document.documentElement.clientHeight',
            '(window.visualViewport ? window.visualViewport.height : null)',
        ):
            r = c.call('eval', {'code': expr})
            print(f'{expr} -> {r!r}')
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
