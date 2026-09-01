#!/usr/bin/env python3
"""FRAME-1 — `resize` событие под-документу + rAF внутри фрейма.

До этой задачи ни один под-документ не получал `resize` при смене своего
вьюпорта (`frames.rs::sync_frame_viewports` уже пересчитывала layout ребёнка
под реальный host-бокс, но никому об этом не сообщала), а
`requestAnimationFrame` внутри фрейма не тикал вовсе (`about_to_wait.rs`
пампил только страницу). Проба меряет оба симптома из тела задачи:

* **«застревает на UA-умолчании 300x150»** — `load` ребёнка срабатывает на
  временном UA-дефолтном вьюпорте ([`FRAME_UA_DEFAULT_SIZE`], до того как
  родитель посчитал свой layout), поэтому `child-load-width` ниже — снимок
  геометрии СРАЗУ на `load` — должен остаться на UA-дефолтной ширине (300), а
  не на реальной ширине host-бокса (450). Первый `resize` (переход UA-дефолт
  → реальный бокс — тот же самый проход, что уже вызвал ту же геометрию, но
  до этой задачи никак не сообщал о ней скрипту) должен принести реальную
  ширину.
* **rAF не тикает** — `rafCount` должен расти между двумя снимками.

Плюс отдельно — `resize` при ПОСЛЕДУЮЩЕЙ смене размера host-бокса (родитель
меняет `iframe.style.width` таймером ПОСЛЕ load), не только при первом
проходе.

Запуск: python tests/wpt/verify_frame_resize_raf.py --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
"""

from __future__ import annotations

import argparse
import http.server
import os
import re
import socket
import subprocess
import sys
import threading
import time

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
HERE = os.path.dirname(os.path.abspath(__file__))

# iframe изначально 450x140 — НАРОЧНО отличается от FRAME_UA_DEFAULT_SIZE
# (300x150) по обеим осям, чтобы переход UA-дефолт -> реальный бокс дал
# наблюдаемую разницу. Через 2с после load родитель меняет ширину на 250px —
# второй, независимый от загрузки resize.
PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfrr parent</title>
<body style="margin:0">
<iframe id="f" src="/.vfrr-child.html"
        style="position:absolute;left:0;top:0;width:450px;height:140px;border:0"></iframe>
<script>
var frame = document.getElementById('f');
frame.addEventListener('load', function () {
  console.log('PROBE parent-load');
  setTimeout(function () {
    frame.style.width = '250px';
    console.log('PROBE parent-resized-250');
  }, 2000);
});
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfrr child</title>
<body style="margin:0">
<script>
console.log('PROBE child-start');
window.addEventListener('load', function () {
  var r = document.documentElement.getBoundingClientRect();
  console.log('PROBE child-load-width ' + Math.round(r.width));
});
window.addEventListener('resize', function () {
  var r = document.documentElement.getBoundingClientRect();
  console.log('PROBE child-resize-width ' + Math.round(r.width));
});
var rafCount = 0;
function tick() { rafCount++; requestAnimationFrame(tick); }
requestAnimationFrame(tick);
setInterval(function () {
  console.log('PROBE child-raf-count ' + rafCount);
}, 1000);
</script>
</body>
"""


def _free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class _Quiet(http.server.SimpleHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=HERE, **kwargs)

    def log_message(self, *args):
        pass


def _serve() -> tuple[int, http.server.ThreadingHTTPServer]:
    port = _free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Quiet)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        default=os.path.join(REPO, "target", "dev-release", "lumen.exe"),
    )
    parser.add_argument("--seconds", type=float, default=6.0)
    args = parser.parse_args()

    for name, body in [(".vfrr-parent.html", PARENT_PAGE), (".vfrr-child.html", CHILD_PAGE)]:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port, server = _serve()
    log_path = os.path.join(REPO, ".tmp", "vfrr-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.vfrr-parent.html"
    print(f"{url} -> {log_path}")
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(_free_port()), url],
            stdout=subprocess.DEVNULL, stderr=log, text=True, cwd=HERE,
        )
        try:
            time.sleep(args.seconds)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
            server.shutdown()

    with open(log_path, encoding="utf-8", errors="replace") as handle:
        text = handle.read()
    markers = re.findall(r"PROBE ([^\n\r]+)", text)

    ok = True

    def expect(label: str, cond: bool) -> None:
        nonlocal ok
        print(f"[{'ok' if cond else 'ФЕЙЛ'}] {label}")
        if not cond:
            ok = False

    def width_of(prefix: str) -> list[int]:
        out = []
        for m in markers:
            if m.startswith(prefix):
                out.append(int(m[len(prefix):]))
        return out

    expect("child-start", any(m.startswith("child-start") for m in markers))
    expect("parent-load", any(m.startswith("parent-load") for m in markers))
    expect("parent-resized-250", any(m.startswith("parent-resized-250") for m in markers))

    load_widths = width_of("child-load-width ")
    resize_widths = width_of("child-resize-width ")
    raf_counts = [int(m[len("child-raf-count "):]) for m in markers if m.startswith("child-raf-count ")]

    expect("ровно один снимок на load (child-load-width)", len(load_widths) == 1)
    expect(
        "load застаёт UA-дефолтную ширину (300), НЕ реальный host-бокс (450)",
        bool(load_widths) and load_widths[0] == 300,
    )
    expect("хотя бы два resize (переход на реальный бокс + смена родителем)", len(resize_widths) >= 2)
    expect(
        "первый resize приносит РЕАЛЬНУЮ ширину host-бокса (450)",
        bool(resize_widths) and resize_widths[0] == 450,
    )
    expect(
        "последующий resize приносит НОВУЮ ширину после смены родителем (250)",
        250 in resize_widths,
    )
    expect("rAF внутри фрейма хотя бы дважды отчитался", len(raf_counts) >= 2)
    expect(
        "rAF внутри фрейма реально тикает (счётчик растёт между снимками)",
        len(raf_counts) >= 2 and raf_counts[-1] > raf_counts[0] > 0,
    )

    print("markers:", *markers, sep="\n  ")
    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
