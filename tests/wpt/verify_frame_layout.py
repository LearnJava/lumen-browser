#!/usr/bin/env python3
"""BUG-480 срезы 12–13 — живая проверка layout под-документа `<iframe>`.

До среза 12 getBoundingClientRect()/офсеты внутри фрейма отвечали
"честными нулями" — контентная геометрия ребёнка нигде не считалась
(frame_bridge.rs: «layout содержимого фрейма — отдельный срез»). Срез 12
считает cascade + layout ребёнка на UA-дефолтном вьюпорте 300×150 CSS px
(HTML LS §4.8.5) сразу после исполнения его скриптов и до его
DOMContentLoaded/load. Срез 13 добавляет второй проход: как только layout
РОДИТЕЛЯ посчитан, ребёнок пересчитывается под РЕАЛЬНЫЙ контентный бокс
своего host-элемента.

UA-дефолтную стадию среза 12 видно только из СОБСТВЕННОГО `load` ребёнка:
скрипт, вставленный родителем, доставляется асинхронно на тике пумпы
(срез 8), то есть всегда уже после layout страницы — обе фазы репорта
(`load` и `late`) показывают уже пересчитанное состояние. Проба поэтому
снимает и то, и другое: `window.__atLoad` ребёнка = стадия 12,
фазы репорта = стадия 13.

Проверяет (тем же кросс-фреймовым postMessage-механизмом, что и
verify_frame_run_script.py — script, вставленный родителем в
contentDocument, исполняется в изоляте ребёнка):

* элемент ребёнка с явным `width`/`height` отдаёт реальный
  getBoundingClientRect() вместо нулей;
* offsetWidth/offsetHeight ребёнка совпадают с getBoundingClientRect();
* в собственном `load` ребёнка `width: 100%` резолвится против UA-дефолтных
  300px (срез 12);
* в обеих фазах репорта тот же элемент резолвится против контентного бокса хоста
  (`<iframe width=500>`), а `matchMedia('(min-width: 400px)')` в ребёнке
  переключается false → true — независимый признак того, что до ребёнка
  доехал именно новый вьюпорт, а не только новые прямоугольники (срез 13).

Запуск: ``python tests/wpt/verify_frame_layout.py [--binary PATH]``
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

PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfly parent</title>
<body>parent
<iframe src="/.vfly-child.html" width="500" height="120" style="border:0;padding:0"></iframe>
<script>
console.log('PROBE parent-start');
window.addEventListener('message', function (ev) {
  console.log('PROBE parent-got ' + JSON.stringify(ev.data));
});
var frame = document.querySelector('iframe');
function report(phase) {
  var d = frame.contentDocument;
  var s = d.createElement('script');
  s.textContent =
    "var box = document.getElementById('box');" +
    "var r = box.getBoundingClientRect();" +
    "var full = document.getElementById('full');" +
    "var fr = full.getBoundingClientRect();" +
    "window.parent.postMessage({" +
    "  phase: '" + phase + "'," +
    "  boxW: r.width, boxH: r.height," +
    "  offsetW: box.offsetWidth, offsetH: box.offsetHeight," +
    "  fullW: fr.width," +
    "  mq400: matchMedia('(min-width: 400px)').matches," +
    "  atLoadW: (window.__atLoad || {}).fullW," +
    "  atLoadMq: (window.__atLoad || {}).mq400" +
    "}, '*');";
  d.body.appendChild(s);
  console.log('PROBE parent-inserted-' + phase);
}
frame.addEventListener('load', function () {
  console.log('PROBE parent-load');
  // Фаза load: layout страницы ещё не считался, вьюпорт ребёнка — UA-дефолт.
  report('load');
  // Фаза late: страница уже разложена, срез 13 пересчитал ребёнка под хост.
  setTimeout(function () { report('late'); }, 1500);
});
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfly child</title>
<body>
<div id="box" style="width:120px;height:40px;">box</div>
<div id="full" style="width:100%;">full</div>
<script>
// Собственный `load` ребёнка — единственный момент, где UA-дефолтный вьюпорт
// среза 12 вообще наблюдаем: скрипт, ВСТАВЛЕННЫЙ родителем, доставляется
// асинхронно на тике пумпы (срез 8), то есть всегда уже после layout страницы.
window.__atLoad = null;
window.addEventListener('load', function () {
  window.__atLoad = {
    fullW: document.getElementById('full').getBoundingClientRect().width,
    mq400: matchMedia('(min-width: 400px)').matches
  };
});
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
    parser.add_argument("--seconds", type=float, default=8.0)
    args = parser.parse_args()

    files = [
        (".vfly-parent.html", PARENT_PAGE),
        (".vfly-child.html", CHILD_PAGE),
    ]
    for name, body in files:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port, server = _serve()
    log_path = os.path.join(REPO, ".tmp", "vfly-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    print(f"http://127.0.0.1:{port}/.vfly-parent.html -> {log_path}")
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{port}/.vfly-parent.html"],
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
        markers = re.findall(r"PROBE ([^\n\r]+)", handle.read())

    got: list[dict] = []
    for marker in markers:
        if marker.startswith("parent-got "):
            try:
                got.append(json.loads(marker[len("parent-got "):]))
            except json.JSONDecodeError:
                pass

    ok = True

    def expect(substr: str) -> None:
        nonlocal ok
        hit = any(substr in m for m in markers)
        status = "ok" if hit else "MISSING"
        if not hit:
            ok = False
        print(f"[{status}] {substr}")

    def expect_post(pred, label: str) -> None:
        nonlocal ok
        hit = any(pred(item) for item in got)
        status = "ok" if hit else "MISSING"
        if not hit:
            ok = False
        print(f"[{status}] {label}")

    expect("parent-load")
    expect("parent-inserted-load")
    expect("parent-inserted-late")

    # Slice 12: explicit width/height resolve to real numbers, not honest zeros.
    expect_post(
        lambda p: abs(p.get("boxW", 0) - 120.0) < 0.5
        and abs(p.get("boxH", 0) - 40.0) < 0.5,
        "explicit width/height box reports real getBoundingClientRect",
    )
    expect_post(
        lambda p: abs(p.get("offsetW", 0) - p.get("boxW", -1)) < 0.5
        and abs(p.get("offsetH", 0) - p.get("boxH", -1)) < 0.5,
        "offsetWidth/offsetHeight match getBoundingClientRect",
    )
    # Slice 12 is still the state the child's OWN load handler sees: the child
    # is laid out before its DOMContentLoaded/load, i.e. before the page's own
    # layout exists. 284 = 300 - 2*8 (UA-default <body> margin).
    expect_post(
        lambda p: abs(p.get("atLoadW", 0) - 284.0) < 0.5 and p.get("atLoadMq") is False,
        "child's own load handler still sees the 300px UA-default viewport",
    )

    # Slice 13: once the parent's layout exists the child is re-laid-out against
    # the host's content box. 484 = 500 - 2*8; the iframe carries no border and
    # no padding, so its content box is exactly the width attribute.
    #
    # Both phases assert it: `load` (script injected from the host's load
    # handler, delivered on the next pump) and `late` (+1.5 s). They agree
    # because delivery across the isolate boundary is always asynchronous —
    # the `late` phase is what makes "after the page layout" guaranteed rather
    # than incidental, and pins that the value does not drift afterwards.
    for name in ("load", "late"):
        expect_post(
            lambda p, name=name: p.get("phase") == name
            and abs(p.get("fullW", 0) - 484.0) < 0.5,
            f"{name}: width:100% resolves against the 500px host content box",
        )
        expect_post(
            lambda p, name=name: p.get("phase") == name and p.get("mq400") is True,
            f"{name}: matchMedia(min-width:400px) flipped — the viewport itself moved",
        )
    # The fixed-size box must be unaffected by the viewport change: a relayout
    # that got the child wrong would move it too.
    expect_post(
        lambda p: p.get("phase") == "late"
        and abs(p.get("boxW", 0) - 120.0) < 0.5
        and abs(p.get("boxH", 0) - 40.0) < 0.5,
        "late: the fixed 120x40 box is unchanged by the viewport change",
    )

    print("markers:", *markers, sep="\n  ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
