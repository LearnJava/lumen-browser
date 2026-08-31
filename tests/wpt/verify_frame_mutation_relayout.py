#!/usr/bin/env python3
"""BUG-480 срез 25 — relayout ребёнка после мутации ЕГО DOM.

Очередь срезов 14/15 записывала: «relayout ребёнка при мутациях (его
собственных или пришедших от родителя, срез 5) … не входит». Проба меряет
именно это — не тот факт, что мутация ПОПАЛА в дерево (это уже доказано
срезом 5), а то, что после неё пересчитывается layout, чей продукт видит сам
ребёнок через ``getBoundingClientRect()``.

Два независимых источника мутации, оба должны довести до relayout:

* **своя** — ребёнок сам меняет ``style.width`` своим таймером. Идёт через
  ОБЫЧНЫЕ нативы ``dom.rs`` в собственном рантайме ребёнка — тот же путь,
  которым страница уже поднимает свой ``dom_dirty``;
* **мостовая** — родитель меняет тот же стиль через
  ``contentDocument``/фасад (``setAttribute``), в СВОЁМ изоляте — это
  Rust-level запись мимо нативов ребёнка, штатный ``dom_dirty`` ребёнка её не
  видит вовсе.

Геометрия, а не только DOM-дерево: `getBoundingClientRect().width` читает
СНИМОК, который публикует шелл (`update_layout_rects`) после
`layout_frame_document`, поэтому число доказывает, что пересчитан именно
layout, а не что мутация видна дереву (это доказывает JS сам по себе, срез 5).

Запуск: python tests/wpt/verify_frame_mutation_relayout.py --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
"""

from __future__ import annotations

import argparse
import http.server
import json
import os
import re
import socket
import subprocess
import sys
import threading
import time

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
HERE = os.path.dirname(os.path.abspath(__file__))

# own-мутация — таймер ребёнка в 1.5с; bridge-мутация — таймер родителя в 3.0с
# (после load — 3.0 отсчитывается от него, а не от старта процесса). Снимки
# каждые 0.5с, прогон 6с — обе мутации должны успеть отразиться в снимках.
PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfmr parent</title>
<body style="margin:0">
<iframe id="f" src="/.vfmr-child.html"
        style="position:absolute;left:0;top:0;width:400px;height:100px;border:0"></iframe>
<script>
var frame = document.getElementById('f');
frame.addEventListener('load', function () {
  console.log('PROBE parent-load');
  setTimeout(function () {
    var d = frame.contentDocument;
    var el = d.getElementById('bridge');
    el.setAttribute('style', 'width:300px;height:20px;background:blue');
    console.log('PROBE parent-mutated-bridge');
  }, 3000);
});
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfmr child</title>
<body style="margin:0">
<div id="own" style="width:100px;height:20px;background:red">a</div>
<div id="bridge" style="width:100px;height:20px;background:blue">b</div>
<script>
console.log('PROBE child-start');
setTimeout(function () {
  document.getElementById('own').style.width = '300px';
  console.log('PROBE child-mutated-own');
}, 1500);
setInterval(function () {
  var o = document.getElementById('own').getBoundingClientRect();
  var b = document.getElementById('bridge').getBoundingClientRect();
  console.log('PROBE child-snap ' + JSON.stringify({
    own: Math.round(o.width), bridge: Math.round(b.width)
  }));
}, 500);
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
    parser.add_argument("--seconds", type=float, default=7.0)
    args = parser.parse_args()

    for name, body in [(".vfmr-parent.html", PARENT_PAGE), (".vfmr-child.html", CHILD_PAGE)]:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port, server = _serve()
    log_path = os.path.join(REPO, ".tmp", "vfmr-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.vfmr-parent.html"
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

    snaps = []
    for m in markers:
        if m.startswith("child-snap "):
            snaps.append(json.loads(m[len("child-snap "):]))

    print("снимки геометрии ребёнка:", *snaps, sep="\n  ")

    ok = True

    def expect(label: str, cond: bool) -> None:
        nonlocal ok
        print(f"[{'ok' if cond else 'ФЕЙЛ'}] {label}")
        if not cond:
            ok = False

    expect("child-start", any("child-start" in m for m in markers))
    expect("parent-load", any("parent-load" in m for m in markers))
    expect("child-mutated-own", any("child-mutated-own" in m for m in markers))
    expect("parent-mutated-bridge", any("parent-mutated-bridge" in m for m in markers))

    baseline_ok = bool(snaps) and snaps[0].get("own") == 100 and snaps[0].get("bridge") == 100
    expect("базовая линия 100/100 до любых мутаций", baseline_ok)

    own_after = [s for s in snaps if s.get("own") == 300]
    bridge_after = [s for s in snaps if s.get("bridge") == 300]
    expect("СВОЯ мутация ребёнка отразилась в getBoundingClientRect (own=300)", bool(own_after))
    expect(
        "МОСТОВАЯ мутация родителя отразилась в getBoundingClientRect (bridge=300)",
        bool(bridge_after),
    )

    print("markers:", *markers, sep="\n  ")
    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
