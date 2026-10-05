#!/usr/bin/env python3
"""BUG-935 срез 80: проба «своп M4 показывает устаревшую страницу».

Страница на каждом rAF переключает класс у блока (`.w` — ширина 300, `.h` —
высота 120) и после 60 кадров остаётся в `.h`. Живое окно с
`LUMEN_BUG935_M4_SWAP=<режим>` должно закончить с блоком 100x120. До среза 80
режим 1 заканчивал 300x50: под движковым потоком UI-сторонний `js_ctx` пуст,
`take_dom_touched` давал «ничего не тронуто», и инкрементальный рестайл брал
стили прошлого прохода целиком.

Срез 81: вторая страница переключает класс у `<html>`, а размер блока задают
селекторы предка (`.a div`, `.b div`) — так проверяется сужение по токенам
(неглубокий корень + точечные потомки), которое UI-путь получил в этом срезе.

    python scripts/bug935_swap_stale_probe.py [режим ...]     (по умолчанию 0 1 2)

Нужен собранный `target/dev-release/lumen.exe`. Код возврата 1 — устаревшая геометрия.
"""

from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

PAGE = """<!doctype html><html><head><style>
div{height:50px;width:100px;background:red}.w{width:300px}.h{height:120px}p{margin:0}
</style></head><body><div id="b"></div><p id="t">x</p><script>
var n=0;function f(){n++;document.getElementById('b').className=(n%2?'w':'h');
document.getElementById('t').textContent='n'+n;if(n<60){requestAnimationFrame(f)}else{window.done=n}}
setTimeout(function(){requestAnimationFrame(f)},1500);</script></body></html>
"""

PAGE_ROOT = """<!doctype html><html><head><style>
div{height:50px;width:100px;background:red}.a div{width:300px}.b div{height:120px}p{margin:0}
</style></head><body><section><div id="b"></div></section><p id="t">x</p><script>
var n=0;function f(){n++;document.documentElement.className=(n%2?'a':'b');
document.getElementById('t').textContent='n'+n;if(n<60){requestAnimationFrame(f)}else{window.done=n}}
setTimeout(function(){requestAnimationFrame(f)},1500);</script></body></html>
"""


def load_census():
    spec = importlib.util.spec_from_file_location(
        'census', REPO / 'scripts' / 'bug935_raf_relayout_census.py')
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def run(census, mode: str, page: Path) -> tuple[float, float] | None:
    env = dict(os.environ, LUMEN_BUG935_M4_SWAP=mode)
    port = census.free_port()
    log_path = Path(tempfile.gettempdir()) / f'bug935_swap_stale_{mode}.log'
    with open(log_path, 'w', encoding='utf-8', errors='replace') as log:
        proc = subprocess.Popen(
            [str(REPO / 'target' / 'dev-release' / 'lumen.exe'), '--mcp-live-port', str(port),
             '--maximized', page.as_uri()],
            env=env, stdout=subprocess.DEVNULL, stderr=log, cwd=REPO)
        try:
            client = census.Client(port, str(log_path))
            time.sleep(8)
            res = client._raw_call('resources/read', {'uri': 'resource://layout'})
            boxes = json.loads(res['contents'][0]['text'])
            for b in boxes:
                if b['tag_name'] == 'div':
                    return b['border_box']['width'], b['border_box']['height']
            return None
        finally:
            proc.terminate()


def main() -> int:
    modes = sys.argv[1:] or ['0', '1', '2']
    census = load_census()
    with tempfile.TemporaryDirectory() as tmp:
        bad = 0
        for name, html in (('stale', PAGE), ('root-class', PAGE_ROOT)):
            page = Path(tmp) / f'{name}.html'
            page.write_text(html, encoding='utf-8')
            for mode in modes:
                size = run(census, mode, page)
                ok = size == (100.0, 120.0)
                bad += not ok
                print(f'{name} swap={mode}: div {size} — {"ok" if ok else "УСТАРЕЛО (ожидалось (100.0, 120.0))"}')
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
