#!/usr/bin/env python3
"""BUG-480 срез 19: ССЫЛКИ под-документа — навигация фрейма.

Срез 16 довёл клик до ребёнка как событие, срез 18 — собственное поведение
элементов формы. Ссылка не работала ни там, ни там: ранний возврат в
`handle_click_at` пропускает разбор ссылки вместе с формой, потому что
единственный узел СТРАНИЦЫ под этой точкой — сам `<iframe>`.

Что меряется и почему именно так:

* ЗАПРОСЫ пишет сервер пробы, а не браузер: страница о загрузке фрейма
  узнать не может, а лог браузера доказательством не является (BUG-826).
  Счётчик, а не множество: «документ запрошен трижды» и «документ
  запрошен» — разные ответы (урок среза 28);
* ДВА фрейма, а не один: фрагментная навигация прокручивает под-документ, и
  после неё прямоугольники ссылок первого фрейма были бы враньём. Поэтому
  фрагмент живёт в своём фрейме, а обычная ссылка — в своём;
* ПИКСЕЛИ — отдельное доказательство: после навигации фрейма его окно
  зелёное (следующий документ), после фрагмента в нижнем фрейме появляется
  синяя полоса. Запрос без пикселей означал бы, что документ скачан, но на
  экране остался прежний;
* точки кликов считаются из `getBoundingClientRect()`, который печатают сами
  документы по таймеру (до первого layout он отдаёт нули — урок среза 18);
* КОНТРОЛЬ идёт ПОСЛЕДНИМ и на самой странице: ссылка страницы после
  `target=_top`-перехода. Он отвечает на вопрос «умеет ли проба вообще
  кликать по ссылке» — если красен он, всё выше меряет саму пробу.

Запуск: python tests/wpt/verify_frame_links.py --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
"""

from __future__ import annotations

import argparse
import base64
import http.server
import json
import os
import re
import socket
import struct
import subprocess
import sys
import threading
import time
import zlib
from collections import Counter

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(REPO, "scripts"))

from scroll_perf import Client  # noqa: E402  (после sys.path)

GREEN = (0, 200, 0)     # следующий документ фрейма
BLUE = (0, 0, 255)      # цель фрагмента, ниже вьюпорта фрейма
RED = (255, 0, 0)       # первый документ фрейма

PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfl parent</title>
<body style="margin:0;background:#fff">
<iframe id="f1" src="/.vfl-child.html" style="position:absolute;left:40px;top:120px;
        width:300px;height:200px;border:0"></iframe>
<iframe id="f2" src="/.vfl-frag.html" style="position:absolute;left:40px;top:360px;
        width:300px;height:200px;border:0"></iframe>
<script>
console.log('PROBE parent-start ' + location.pathname);
function vflRect(id) {
  var r = document.getElementById(id).getBoundingClientRect();
  return [r.left, r.top, r.width, r.height];
}
setTimeout(function () {
  console.log('PROBE parent-rects ' + JSON.stringify({
    f1: vflRect('f1'), f2: vflRect('f2')
  }));
}, 800);
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfl child</title>
<body style="margin:0;background:rgb(255,0,0)">
<a id="lnav" href="/.vfl-next.html" style="display:block;width:200px;height:40px;
   background:#fff">go next</a>
<script>
console.log('PROBE child-start ' + location.pathname);
setTimeout(function () {
  var r = document.getElementById('lnav').getBoundingClientRect();
  console.log('PROBE child-rects ' + JSON.stringify({lnav: [r.left, r.top, r.width, r.height]}));
}, 800);
document.addEventListener('click', function (ev) {
  console.log('PROBE child-click id=' + (ev.target ? ev.target.id : 'null'));
});
</script>
</body>
"""

FRAG_PAGE = """<!doctype html><meta charset="utf-8"><title>vfl frag</title>
<body style="margin:0;background:rgb(255,255,0)">
<a id="lfrag" href="#far" style="display:block;width:200px;height:40px;background:#fff">to far</a>
<div style="height:400px;background:rgb(255,255,0)"></div>
<div id="far" style="height:120px;background:rgb(0,0,255)"></div>
<script>
console.log('PROBE frag-start ' + location.pathname);
setTimeout(function () {
  var r = document.getElementById('lfrag').getBoundingClientRect();
  console.log('PROBE frag-rects ' + JSON.stringify({lfrag: [r.left, r.top, r.width, r.height]}));
}, 800);
window.addEventListener('hashchange', function () {
  console.log('PROBE frag-hashchange ' + location.hash);
});
</script>
</body>
"""

NEXT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfl next</title>
<body style="margin:0;background:rgb(0,200,0)">
<a id="ltop" href="/.vfl-top.html" target="_top" style="display:block;width:200px;
   height:40px;background:#fff">to top</a>
<script>
console.log('PROBE next-start ' + location.pathname);
setTimeout(function () {
  var r = document.getElementById('ltop').getBoundingClientRect();
  console.log('PROBE next-rects ' + JSON.stringify({ltop: [r.left, r.top, r.width, r.height]}));
}, 600);
</script>
</body>
"""

TOP_PAGE = """<!doctype html><meta charset="utf-8"><title>vfl top</title>
<body style="margin:0;background:#fff">
<a id="pctl" href="/.vfl-ctl.html" style="display:block;width:200px;height:40px;
   background:#ccc">control</a>
<script>
console.log('PROBE top-start ' + location.pathname);
setTimeout(function () {
  var r = document.getElementById('pctl').getBoundingClientRect();
  console.log('PROBE top-rects ' + JSON.stringify({pctl: [r.left, r.top, r.width, r.height]}));
}, 600);
</script>
</body>
"""

CTL_PAGE = """<!doctype html><meta charset="utf-8"><title>vfl ctl</title>
<body style="margin:0;background:#fff">
<script>console.log('PROBE ctl-start ' + location.pathname);</script>
</body>
"""

# ── вариант `deep`: относительный адрес, вложенный фрейм, target=_parent ─────
#
# Основной вариант проверяет механизм на абсолютных путях (`/x.html`), которые
# резолвятся одинаково от любой базы, — то есть про базу под-документа не
# говорит ничего. Здесь адреса ОТНОСИТЕЛЬНЫЕ и лежат в подкаталоге: если
# навигация резолвит их базой СТРАНИЦЫ, сервер увидит `/b.html` вместо
# `/sub/b.html` и ответит 404.

DEEP_PARENT = """<!doctype html><meta charset="utf-8"><title>vfl2 parent</title>
<body style="margin:0;background:#fff">
<iframe id="f1" src="/sub/a.html" style="position:absolute;left:40px;top:120px;
        width:300px;height:200px;border:0"></iframe>
<iframe id="f2" src="/sub/e.html" style="position:absolute;left:40px;top:360px;
        width:300px;height:200px;border:0"></iframe>
<script>
console.log('PROBE parent-start ' + location.pathname);
function vflRect(id) {
  var r = document.getElementById(id).getBoundingClientRect();
  return [r.left, r.top, r.width, r.height];
}
setTimeout(function () {
  console.log('PROBE parent-rects ' + JSON.stringify({
    f1: vflRect('f1'), f2: vflRect('f2')
  }));
}, 800);
// Заголовок под-документа глазами РОДИТЕЛЯ: после навигации `contentDocument`
// обязан указывать на НОВЫЙ документ, иначе фасады остались висеть на
// выброшенном.
document.getElementById('f1').addEventListener('load', function () {
  console.log('PROBE parent-frameload');
});
setInterval(function () {
  var d = document.getElementById('f1').contentDocument;
  console.log('PROBE parent-sees ' + (d ? d.title : 'null') + ' len=' + window.length);
}, 700);
</script>
</body>
"""

DEEP_A = """<!doctype html><meta charset="utf-8"><title>vfl2 a</title>
<body style="margin:0;background:rgb(255,0,0)">
<a id="rel" href="b.html" style="display:block;width:200px;height:40px;background:#fff">rel b</a>
<a id="miss" href="nope.html" style="display:block;width:200px;height:40px;background:#eee">404</a>
<script>
console.log('PROBE a-start ' + location.pathname);
setTimeout(function () {
  function rect(id) {
    var r = document.getElementById(id).getBoundingClientRect();
    return [r.left, r.top, r.width, r.height];
  }
  console.log('PROBE a-rects ' + JSON.stringify({rel: rect('rel'), miss: rect('miss')}));
}, 700);
</script>
</body>
"""

DEEP_B = """<!doctype html><meta charset="utf-8"><title>vfl2 b</title>
<body style="margin:0;background:rgb(0,200,0)">
<a id="rel2" href="a.html" style="display:block;width:200px;height:40px;background:#fff">rel a</a>
<script>
console.log('PROBE b-start ' + location.pathname);
setTimeout(function () {
  var r = document.getElementById('rel2').getBoundingClientRect();
  console.log('PROBE b-rects ' + JSON.stringify({rel2: [r.left, r.top, r.width, r.height]}));
}, 700);
</script>
</body>
"""

DEEP_E = """<!doctype html><meta charset="utf-8"><title>vfl2 e</title>
<body style="margin:0;background:rgb(200,200,200)">
<iframe id="g" src="c.html" style="position:absolute;left:20px;top:60px;
        width:200px;height:100px;border:0"></iframe>
<script>
console.log('PROBE e-start ' + location.pathname);
setTimeout(function () {
  var r = document.getElementById('g').getBoundingClientRect();
  console.log('PROBE e-rects ' + JSON.stringify({g: [r.left, r.top, r.width, r.height]}));
}, 700);
</script>
</body>
"""

DEEP_C = """<!doctype html><meta charset="utf-8"><title>vfl2 c</title>
<body style="margin:0;background:rgb(0,0,255)">
<a id="up" href="d.html" target="_parent" style="display:block;width:150px;
   height:30px;background:#fff">up</a>
<script>
console.log('PROBE c-start ' + location.pathname);
setTimeout(function () {
  var r = document.getElementById('up').getBoundingClientRect();
  console.log('PROBE c-rects ' + JSON.stringify({up: [r.left, r.top, r.width, r.height]}));
}, 700);
</script>
</body>
"""

DEEP_D = """<!doctype html><meta charset="utf-8"><title>vfl2 d</title>
<body style="margin:0;background:#fff">
<div style="width:280px;height:180px;background:rgb(255,255,0)"></div>
<script>console.log('PROBE d-start ' + location.pathname);</script>
</body>
"""

YELLOW = (255, 255, 0)

PAGES = {
    ".vfl-parent.html": PARENT_PAGE,
    ".vfl-child.html": CHILD_PAGE,
    ".vfl-frag.html": FRAG_PAGE,
    ".vfl-next.html": NEXT_PAGE,
    ".vfl-top.html": TOP_PAGE,
    ".vfl-ctl.html": CTL_PAGE,
    ".vfl2-parent.html": DEEP_PARENT,
    "sub/a.html": DEEP_A,
    "sub/b.html": DEEP_B,
    "sub/c.html": DEEP_C,
    "sub/d.html": DEEP_D,
    "sub/e.html": DEEP_E,
}

REQUESTS: Counter = Counter()


def read_png_rgba(data: bytes):
    """(width, height, [строка RGBA8, ...]) — формат, который пишет lumen."""
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "не PNG"
    pos, idat, w, h = 8, b"", None, None
    while pos < len(data):
        (ln,) = struct.unpack(">I", data[pos : pos + 4])
        tag = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + ln]
        if tag == b"IHDR":
            w, h, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert (depth, color, interlace) == (8, 6, 0), (depth, color, interlace)
        elif tag == b"IDAT":
            idat += body
        pos += 12 + ln
    raw, stride, out, prev = zlib.decompress(idat), w * 4, [], bytearray(w * 4)
    for y in range(h):
        ft = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        for x in range(stride):
            a = line[x - 4] if x >= 4 else 0
            b = prev[x]
            c = prev[x - 4] if x >= 4 else 0
            if ft == 1:
                line[x] = (line[x] + a) & 0xFF
            elif ft == 2:
                line[x] = (line[x] + b) & 0xFF
            elif ft == 3:
                line[x] = (line[x] + (a + b) // 2) & 0xFF
            elif ft == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 0xFF
        prev = line
        out.append(bytes(line))
    return w, h, out


def count_colors(png: bytes) -> Counter:
    w, h, rows = read_png_rgba(png)
    hist: Counter = Counter()
    for y in range(h):
        row = rows[y]
        for x in range(w):
            hist[tuple(row[x * 4 : x * 4 + 3])] += 1
    return hist


def _free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class _Recording(http.server.SimpleHTTPRequestHandler):
    """Отдаёт страницы пробы и СЧИТАЕТ запросы: только сервер знает, ходил ли
    браузер за документом фрейма."""

    protocol_version = "HTTP/1.1"

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=HERE, **kwargs)

    def do_GET(self):  # noqa: N802
        REQUESTS[self.path.split("?")[0]] += 1
        body = PAGES.get(self.path.split("?")[0].lstrip("/"), "").encode("utf-8")
        if not body:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def _markers(log_path: str) -> list[str]:
    with open(log_path, encoding="utf-8", errors="replace") as handle:
        return re.findall(r"PROBE ([^\n\r]+)", handle.read())


def _rects(markers: list[str], prefix: str) -> dict[str, list[float]]:
    for m in markers:
        if m.startswith(prefix):
            return json.loads(m[len(prefix):])
    return {}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        default=os.path.join(REPO, "target", "dev-release", "lumen.exe"),
    )
    parser.add_argument("--variant", choices=("links", "deep", "all"), default="all")
    args = parser.parse_args()
    rc = 0
    if args.variant in ("links", "all"):
        rc |= variant_links(args)
    if args.variant in ("deep", "all"):
        REQUESTS.clear()
        rc |= variant_deep(args)
    return rc


def variant_links(args) -> int:
    port = _free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Recording)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    mcp_port = _free_port()
    log_path = os.path.join(REPO, ".tmp", "vfl-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.vfl-parent.html"
    print(f"{url} -> {log_path}")

    shots: dict[str, Counter] = {}
    pr: dict[str, list[float]] = {}
    cr: dict[str, list[float]] = {}
    fr: dict[str, list[float]] = {}
    nr: dict[str, list[float]] = {}
    tr: dict[str, list[float]] = {}
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(mcp_port), url],
            stdout=subprocess.DEVNULL, stderr=log, text=True, cwd=HERE,
        )
        try:
            client = Client(mcp_port, log_path)
            # 30 с, а не 10: на холодном профиле выбор бэкенда wgpu
            # (`backend_probe`) сам по себе занимает десятки секунд.
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(3.0)

            def click(x: float, y: float, pause: float = 1.2) -> None:
                client.call("click", {"target": {"point": {"x": x, "y": y}}})
                time.sleep(pause)

            shot = lambda: base64.b64decode(  # noqa: E731
                client._raw_call("resources/read", {"uri": "resource://screenshot"})
                ["contents"][0]["data"]
            )

            start = _markers(log_path)
            pr = _rects(start, "parent-rects ")
            cr = _rects(start, "child-rects ")
            fr = _rects(start, "frag-rects ")
            print("фреймы родителя:", pr)
            print("ссылка ребёнка: ", cr, " ссылка фрагмента:", fr)

            def в_фрейме(frame: list[float], rect: list[float]):
                return (frame[0] + rect[0] + rect[2] / 2, frame[1] + rect[1] + rect[3] / 2)

            shots["before"] = count_colors(shot())
            if pr and fr:
                # 1. Фрагмент ВНУТРИ фрейма: прокрутка под-документа, без сети.
                click(*в_фрейме(pr["f2"], fr["lfrag"]))
                shots["frag"] = count_colors(shot())
            if pr and cr:
                # 2. Обычная ссылка ребёнка: навигация САМОГО фрейма.
                click(*в_фрейме(pr["f1"], cr["lnav"]), pause=2.5)
                shots["nav"] = count_colors(shot())
            nr = _rects(_markers(log_path), "next-rects ")
            print("ссылка _top:", nr)
            if pr and nr:
                # 3. `target=_top` из ребёнка: навигация СТРАНИЦЫ.
                click(*в_фрейме(pr["f1"], nr["ltop"]), pause=2.5)
            tr = _rects(_markers(log_path), "top-rects ")
            print("контрольная ссылка страницы:", tr)
            if tr:
                # 4. КОНТРОЛЬ: обычная ссылка страницы. Красный контроль
                # означает, что проба меряет себя, а не движок.
                click(tr["pctl"][0] + tr["pctl"][2] / 2,
                      tr["pctl"][1] + tr["pctl"][3] / 2, pause=2.0)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
            server.shutdown()

    markers = _markers(log_path)
    ok = True

    def check(cond: bool, text: str) -> None:
        nonlocal ok
        ok &= bool(cond)
        print(f"[{'OK  ' if cond else 'ФЕЙЛ'}] {text}")

    def expect(substr: str) -> None:
        check(any(substr in m for m in markers), f"есть «{substr}»")

    print("запросы к серверу:")
    for path, n in sorted(REQUESTS.items()):
        print(f"  GET {path} x{n}")

    expect("parent-start")
    expect("child-start")
    expect("frag-start")
    check(bool(pr) and bool(cr) and bool(fr), "все три документа отчитались прямоугольниками")
    child_clicks = [m for m in markers if m.startswith("child-click ")]
    print("клики ребёнка:", *child_clicks, sep="\n  ")
    check(any("id=lnav" in m for m in child_clicks), "клик попал в саму ссылку ребёнка (срез 16)")

    # 1. Фрагмент: под-документ прокрутился, сети не было.
    check(any(m.startswith("frag-hashchange #far") for m in markers),
          "фрагментная ссылка ребёнка обновила его location (hashchange)")
    check(shots.get("before", Counter())[BLUE] == 0, "до клика цель фрагмента не видна (контроль)")
    check(shots.get("frag", Counter())[BLUE] > 1000, "после клика фрейм прокручен к цели фрагмента")

    # 2. Навигация фрейма.
    check(REQUESTS.get("/.vfl-next.html", 0) == 1,
          f"следующий документ фрейма запрошен ровно раз (было {REQUESTS.get('/.vfl-next.html', 0)})")
    expect("next-start")
    check(shots.get("before", Counter())[GREEN] == 0, "до клика фрейм не зелёный (контроль)")
    check(shots.get("nav", Counter())[GREEN] > 1000, "после клика в окне фрейма новый документ")
    check(shots.get("nav", Counter())[RED] == 0, "прежний документ фрейма с экрана ушёл")

    # 3. target=_top — навигация страницы, а не фрейма.
    check(REQUESTS.get("/.vfl-top.html", 0) == 1,
          f"target=_top увёл СТРАНИЦУ (запросов {REQUESTS.get('/.vfl-top.html', 0)})")
    expect("top-start")

    # 4. Контроль.
    check(REQUESTS.get("/.vfl-ctl.html", 0) == 1,
          f"КОНТРОЛЬ: ссылка страницы работает (запросов {REQUESTS.get('/.vfl-ctl.html', 0)})")

    print("маркеры:", *markers, sep="\n  ")
    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


def variant_deep(args) -> int:
    """Относительный адрес, вложенный фрейм и `target=_parent`."""
    port = _free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Recording)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    mcp_port = _free_port()
    log_path = os.path.join(REPO, ".tmp", "vfl-deep.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.vfl2-parent.html"
    print(f"\n=== вариант deep: {url} -> {log_path}")

    shots: dict[str, Counter] = {}
    pr: dict[str, list[float]] = {}
    ar: dict[str, list[float]] = {}
    er: dict[str, list[float]] = {}
    cr: dict[str, list[float]] = {}
    br: dict[str, list[float]] = {}
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(mcp_port), url],
            stdout=subprocess.DEVNULL, stderr=log, text=True, cwd=HERE,
        )
        try:
            client = Client(mcp_port, log_path)
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(3.0)

            def click(x: float, y: float, pause: float = 2.0) -> None:
                client.call("click", {"target": {"point": {"x": x, "y": y}}})
                time.sleep(pause)

            shot = lambda: base64.b64decode(  # noqa: E731
                client._raw_call("resources/read", {"uri": "resource://screenshot"})
                ["contents"][0]["data"]
            )

            start = _markers(log_path)
            pr, ar, er = _rects(start, "parent-rects "), _rects(start, "a-rects "), _rects(start, "e-rects ")
            cr = _rects(start, "c-rects ")
            print("фреймы родителя:", pr, " ссылка a:", ar)
            print("вложенный фрейм:", er, " ссылка c:", cr)

            def в_фрейме(*rects):
                x = sum(r[0] for r in rects) + rects[-1][2] / 2
                y = sum(r[1] for r in rects) + rects[-1][3] / 2
                return (x, y)

            shots["before"] = count_colors(shot())
            if pr and ar:
                # 1. ОТНОСИТЕЛЬНЫЙ адрес: резолв базой ребёнка, а не страницы.
                click(*в_фрейме(pr["f1"], ar["rel"]))
                shots["rel"] = count_colors(shot())
            br = _rects(_markers(log_path), "b-rects ")
            if pr and br:
                # 2. Второй переход подряд: у НОВОГО хэндла база и адрес свои.
                click(*в_фрейме(pr["f1"], br["rel2"]))
            if pr and ar:
                # 3. Ссылка в никуда: запрос уходит, документ остаётся прежним.
                click(*в_фрейме(pr["f1"], ar["miss"]))
                shots["miss"] = count_colors(shot())
            if pr and er and cr:
                # 4. `target=_parent` из фрейма ГЛУБИНЫ 1: меняется внешний.
                click(*в_фрейме(pr["f2"], er["g"], cr["up"]))
                shots["up"] = count_colors(shot())
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
            server.shutdown()

    markers = _markers(log_path)
    ok = True

    def check(cond: bool, text: str) -> None:
        nonlocal ok
        ok &= bool(cond)
        print(f"[{'OK  ' if cond else 'ФЕЙЛ'}] {text}")

    print("запросы к серверу:")
    for path, n in sorted(REQUESTS.items()):
        print(f"  GET {path} x{n}")

    check(all(any(m.startswith(f"{k}-start ") for m in markers) for k in ("parent", "a", "e", "c")),
          "все четыре стартовых документа загрузились")
    # 1. Относительный адрес разрешён базой РЕБЁНКА (иначе был бы /b.html).
    check(REQUESTS.get("/sub/b.html", 0) == 1,
          f"относительная ссылка ушла в /sub/b.html (x{REQUESTS.get('/sub/b.html', 0)}), "
          f"а не в /b.html (x{REQUESTS.get('/b.html', 0)})")
    check(REQUESTS.get("/b.html", 0) == 0, "мимо базы ребёнка запросов не было")
    check(shots.get("rel", Counter())[GREEN] > 1000, "первый переход виден на экране")
    # 2. Новый хэндл сам навигабелен: второй переход, снова относительный.
    check(REQUESTS.get("/sub/a.html", 0) == 2,
          f"второй переход подряд состоялся (/sub/a.html x{REQUESTS.get('/sub/a.html', 0)})")
    # 3. Неудачная навигация: запрос был, документ остался прежним, окно живо.
    check(REQUESTS.get("/sub/nope.html", 0) == 1,
          f"за несуществующим документом сходили (x{REQUESTS.get('/sub/nope.html', 0)})")
    check(shots.get("miss", Counter())[RED] > 1000,
          "после неудачи во фрейме остался прежний документ")
    # 4. Родитель видит НОВЫЙ под-документ, а не выброшенный.
    seen = [m.split(" ", 1)[1] for m in markers if m.startswith("parent-sees ")]
    print("родитель видит contentDocument.title:", sorted(set(seen)))
    check(any(s.startswith("vfl2 b") for s in seen), "contentDocument родителя переехал на новый документ")
    # 4. target=_parent из глубины 1 меняет ВНЕШНИЙ фрейм, а не страницу.
    check(REQUESTS.get("/sub/d.html", 0) == 1,
          f"target=_parent увёл внешний фрейм (/sub/d.html x{REQUESTS.get('/sub/d.html', 0)})")
    check(REQUESTS.get("/.vfl2-parent.html", 0) == 1, "страница при этом не перезагружалась")
    for name, hist in shots.items():
        print(f"  цвета {name}: {hist.most_common(6)}")
    check(shots.get("before", Counter())[YELLOW] == 0, "до клика жёлтого нет (контроль)")
    check(shots.get("up", Counter())[YELLOW] > 1000, "внешний фрейм показывает новый документ")
    check(shots.get("up", Counter())[BLUE] == 0, "вложенный фрейм ушёл вместе со своим хозяином")

    print("маркеры:", *[m for m in markers if not m.startswith("parent-sees ")], sep="\n  ")
    print("ИТОГ deep:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
