#!/usr/bin/env python3
"""BUG-480 срез 20: ОТПРАВКА ФОРМЫ под-документа фрейма.

Срез 18 довёл до ребёнка собственное поведение элементов управления, срез 19 —
ссылки. Отправка формы не работала ни там, ни там: срез 18 отклонил её явно
(«это навигация фрейма, отдельный пункт очереди»), а разбор клика формой для
ребёнка вообще не доходит до этой ветки, потому что единственный узел
СТРАНИЦЫ под точкой клика — сам `<iframe>`.

Что меряется и почему именно так:

* ЗАПРОСЫ пишет сервер пробы, а не браузер: страница о загрузке фрейма узнать
  не может, а лог браузера доказательством не является (BUG-826). Счётчик по
  ПОЛНОМУ пути с query: «форма ушла» и «форма ушла с теми полями» — разные
  ответы, и второй виден только серверу;
* событие `submit` — отдельная проверка: отправка без него означала бы, что
  страница-ребёнок не может ни узнать о ней, ни отменить. Отмена
  (`preventDefault`) меряется своим фреймом: сервер не должен увидеть НИ
  ОДНОГО запроса за её `action`;
* ПИКСЕЛИ: после отправки окно фрейма зелёное (документ-результат). Запрос без
  пикселей означал бы, что документ скачан, а на экране остался прежний;
* точки кликов считаются из `getBoundingClientRect()`, который печатают сами
  документы по таймеру (до первого layout он отдаёт нули — урок среза 18);
* КОНТРОЛЬ идёт ПЕРВЫМ и на самой странице: её форма с `preventDefault()` в
  обработчике. Он отвечает на вопрос «умеет ли проба вообще попасть по
  submit-кнопке» — и, в отличие от контроля-навигации, работает и ДО правки,
  когда ни один переход ещё не случается. Реальная сетевая отправка формы
  СТРАНИЦЫ проверяется вторым контролем, уже после `target=_top`;
* клик по кнопке ребёнка печатает и сам ребёнок (слушатель `document`): без
  этого «отправки нет» и «клик не дошёл» неразличимы.

Запуск: python tests/wpt/verify_frame_form_submit.py --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
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

GREEN = (0, 200, 0)     # документ-результат отправки
RED = (255, 0, 0)       # первый документ фрейма с формой
YELLOW = (255, 255, 0)  # фрейм, чью отправку отменил обработчик

PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfs parent</title>
<body style="margin:0;background:#fff">
<form id="pctl" method="get" action="/.vfs-never.html">
  <input type="submit" id="pgo" value="control"
         style="display:block;width:200px;height:40px;background:#ddd">
</form>
<iframe id="f1" src="/.vfs-child.html" style="position:absolute;left:40px;top:120px;
        width:300px;height:200px;border:0"></iframe>
<iframe id="f2" src="/.vfs-cancel.html" style="position:absolute;left:40px;top:360px;
        width:300px;height:200px;border:0"></iframe>
<script>
console.log('PROBE parent-start ' + location.pathname);
function vfsRect(id) {
  var r = document.getElementById(id).getBoundingClientRect();
  return [r.left, r.top, r.width, r.height];
}
setTimeout(function () {
  console.log('PROBE parent-rects ' + JSON.stringify({
    f1: vfsRect('f1'), f2: vfsRect('f2'), pgo: vfsRect('pgo')
  }));
}, 800);
// Контроль: та же машинерия на СТРАНИЦЕ, но без навигации — иначе он унёс бы
// вместе со страницей оба фрейма, ради которых проба и написана.
document.getElementById('pctl').addEventListener('submit', function (ev) {
  ev.preventDefault();
  console.log('PROBE parent-submit prevented');
});
</script>
</body>
"""

# Обычная отправка методом GET. Поля берутся из атрибутов (`collect_dom_form_fields`),
# поэтому значение стоит атрибутом, а не вводом с клавиатуры.
CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfs child</title>
<body style="margin:0;background:rgb(255,0,0)">
<form id="cf" method="get" action="/.vfs-result.html">
  <input type="hidden" name="q" value="abc">
  <input type="submit" id="go" value="send"
         style="display:block;width:200px;height:40px;background:#fff">
</form>
<script>
console.log('PROBE child-start ' + location.pathname + location.search);
setTimeout(function () {
  var r = document.getElementById('go').getBoundingClientRect();
  console.log('PROBE child-rects ' + JSON.stringify({go: [r.left, r.top, r.width, r.height]}));
}, 800);
document.getElementById('cf').addEventListener('submit', function () {
  console.log('PROBE child-submit');
});
// Без этой строки «отправки нет» и «клик не дошёл до ребёнка» неразличимы.
document.addEventListener('click', function (ev) {
  console.log('PROBE child-click id=' + (ev.target ? ev.target.id : 'null'));
});
</script>
</body>
"""

# Отмена (`preventDefault`) и выход наружу (`target=_top`) — в одном документе:
# отменённая отправка ничего не двигает, поэтому прямоугольники второй кнопки
# остаются верными.
CANCEL_PAGE = """<!doctype html><meta charset="utf-8"><title>vfs cancel</title>
<body style="margin:0;background:rgb(255,255,0)">
<form id="nf" method="get" action="/.vfs-never.html">
  <input type="hidden" name="n" value="1">
  <input type="submit" id="nogo" value="cancel"
         style="display:block;width:200px;height:30px;background:#fff">
</form>
<form id="tf" method="get" action="/.vfs-top.html" target="_top">
  <input type="hidden" name="t" value="1">
  <input type="submit" id="topgo" value="top"
         style="display:block;width:200px;height:30px;background:#eee">
</form>
<script>
console.log('PROBE cancel-start ' + location.pathname);
setTimeout(function () {
  function rect(id) {
    var r = document.getElementById(id).getBoundingClientRect();
    return [r.left, r.top, r.width, r.height];
  }
  console.log('PROBE cancel-rects ' + JSON.stringify({
    nogo: rect('nogo'), topgo: rect('topgo')
  }));
}, 800);
document.getElementById('nf').addEventListener('submit', function (ev) {
  ev.preventDefault();
  console.log('PROBE cancel-submit prevented');
});
document.addEventListener('click', function (ev) {
  console.log('PROBE cancel-click id=' + (ev.target ? ev.target.id : 'null'));
});
</script>
</body>
"""

# Заливка стоит ОТДЕЛЬНЫМ блоком, а не фоном `<body>`: фон под-документа
# доходит только до высоты его содержимого, и у пустой страницы своих пикселей
# нет вовсе — сквозь фрейм видно фон СТРАНИЦЫ (residual среза 14, найденный
# пробой среза 19 на себе самой).
RESULT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfs result</title>
<body style="margin:0">
<div style="width:300px;height:200px;background:rgb(0,200,0)"></div>
<script>console.log('PROBE result-start ' + location.pathname + location.search);</script>
</body>
"""

# Страница, куда уводит `target=_top`. На ней же живёт КОНТРОЛЬ — форма самой
# страницы: она отвечает на вопрос «умеет ли проба нажать submit вообще».
TOP_PAGE = """<!doctype html><meta charset="utf-8"><title>vfs top</title>
<body style="margin:0;background:rgb(0,0,255)">
<form id="pf" method="get" action="/.vfs-page-result.html">
  <input type="hidden" name="who" value="page">
  <input type="submit" id="psub" value="page send"
         style="display:block;width:200px;height:40px;background:#fff">
</form>
<script>
console.log('PROBE top-start ' + location.pathname + location.search);
setTimeout(function () {
  var r = document.getElementById('psub').getBoundingClientRect();
  console.log('PROBE top-rects ' + JSON.stringify({psub: [r.left, r.top, r.width, r.height]}));
}, 800);
</script>
</body>
"""

PAGE_RESULT = """<!doctype html><meta charset="utf-8"><title>vfs page result</title>
<body style="margin:0;background:rgb(200,200,200)">
<script>console.log('PROBE page-result-start ' + location.pathname + location.search);</script>
</body>
"""

MAGENTA = (255, 0, 255)  # фон фрейма, над которым рисуется подсказка валидации
# Фон подсказки валидации (`forms::build_validation_tooltip`), смешанный с
# пурпурным фоном того фрейма: alpha 245 из 255.
TOOLTIP_BG = (255, 243, 202)
# Та же подсказка над белым фоном СТРАНИЦЫ — цвет КОНТРОЛЯ.
TOOLTIP_ON_WHITE = (255, 253, 202)

# ── вариант «script»: второй вход в ту же отправку — скрипт самого ребёнка ──

DEEP_PARENT = """<!doctype html><meta charset="utf-8"><title>vfs2 parent</title>
<body style="margin:0;background:#fff">
<form id="pvf" method="get" action="/sub/never.html">
  <input name="p" required style="display:block;width:200px;height:30px;background:#fff">
  <input type="submit" id="pvgo" value="ctl"
         style="display:block;width:200px;height:40px;background:#ddd">
</form>
<iframe id="g1" src="/sub/s.html" style="position:absolute;left:40px;top:120px;
        width:300px;height:200px;border:0"></iframe>
<iframe id="g2" src="/sub/v.html" style="position:absolute;left:40px;top:360px;
        width:300px;height:200px;border:0"></iframe>
<iframe id="g3" src="/sub/q.html" style="position:absolute;left:400px;top:120px;
        width:300px;height:200px;border:0"></iframe>
<script>
console.log('PROBE parent-start ' + location.pathname);
setTimeout(function () {
  function rect(id) {
    var r = document.getElementById(id).getBoundingClientRect();
    return [r.left, r.top, r.width, r.height];
  }
  console.log('PROBE parent-rects ' + JSON.stringify({
    g1: rect('g1'), g2: rect('g2'), g3: rect('g3'), pvgo: rect('pvgo')
  }));
}, 800);
</script>
</body>
"""

# `submit()` по спеке (§4.10.21.3) пропускает и валидацию, и событие `submit` —
# поэтому маркера `s-submit-event` быть НЕ должно.
DEEP_S = """<!doctype html><meta charset="utf-8"><title>vfs2 s</title>
<body style="margin:0;background:rgb(255,0,0)">
<form id="sf" method="get" action="r.html"><input type="hidden" name="s" value="1"></form>
<script>
console.log('PROBE s-start ' + location.pathname);
document.getElementById('sf').addEventListener('submit', function () {
  console.log('PROBE s-submit-event');
});
setTimeout(function () {
  console.log('PROBE s-call');
  document.getElementById('sf').submit();
}, 1500);
</script>
</body>
"""

# `requestSubmit()` рассылает событие САМ, на JS-стороне: шелл повторить его не
# имеет права, поэтому маркер обязан встретиться РОВНО один раз.
DEEP_Q = """<!doctype html><meta charset="utf-8"><title>vfs2 q</title>
<body style="margin:0;background:rgb(0,0,255)">
<form id="qf" method="get" action="r.html"><input type="hidden" name="q" value="1"></form>
<script>
console.log('PROBE q-start ' + location.pathname);
document.getElementById('qf').addEventListener('submit', function () {
  console.log('PROBE q-submit-event');
});
setTimeout(function () {
  console.log('PROBE q-call');
  document.getElementById('qf').requestSubmit();
}, 2500);
</script>
</body>
"""

DEEP_R = """<!doctype html><meta charset="utf-8"><title>vfs2 r</title>
<body style="margin:0">
<div style="width:300px;height:200px;background:rgb(0,200,0)"></div>
<script>console.log('PROBE r-start ' + location.pathname + location.search);</script>
</body>
"""

DEEP_V = """<!doctype html><meta charset="utf-8"><title>vfs2 v</title>
<body style="margin:0">
<div style="position:absolute;left:0;top:0;width:300px;height:200px;background:rgb(255,0,255)"></div>
<form id="vf" method="get" action="never.html">
  <input name="x" required style="display:block;width:200px;height:30px;background:#fff">
  <input type="submit" id="vgo" value="send"
         style="display:block;width:200px;height:40px;background:#fff">
</form>
<script>
console.log('PROBE v-start ' + location.pathname);
setTimeout(function () {
  var r = document.getElementById('vgo').getBoundingClientRect();
  console.log('PROBE v-rects ' + JSON.stringify({vgo: [r.left, r.top, r.width, r.height]}));
}, 800);
document.addEventListener('click', function (ev) {
  console.log('PROBE v-click id=' + (ev.target ? ev.target.id : 'null'));
});
</script>
</body>
"""

PAGES = {
    ".vfs-parent.html": PARENT_PAGE,
    ".vfs-child.html": CHILD_PAGE,
    ".vfs-cancel.html": CANCEL_PAGE,
    ".vfs-result.html": RESULT_PAGE,
    ".vfs-top.html": TOP_PAGE,
    ".vfs-page-result.html": PAGE_RESULT,
    ".vfs-never.html": RESULT_PAGE,
    ".vfs2-parent.html": DEEP_PARENT,
    "sub/s.html": DEEP_S,
    "sub/q.html": DEEP_Q,
    "sub/r.html": DEEP_R,
    "sub/v.html": DEEP_V,
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
    """Отдаёт страницы пробы и СЧИТАЕТ запросы по ПОЛНОМУ пути (с query):
    только сервер знает, ушла ли форма и с какими полями."""

    protocol_version = "HTTP/1.1"

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=HERE, **kwargs)

    def do_GET(self):  # noqa: N802
        REQUESTS[self.path] += 1
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


def _pixels_near(png: bytes, color, tol: int = 8):
    """Точки, чей цвет близок к `color`, — (x, y). Подсказка валидации рисуется
    полупрозрачной (alpha 245), поэтому её жёлтый смешан с тем, что под ним, и
    точное сравнение здесь не годится."""
    w, h, rows = read_png_rgba(png)
    out = []
    for y in range(h):
        row = rows[y]
        for x in range(w):
            px = row[x * 4 : x * 4 + 3]
            if all(abs(px[i] - color[i]) <= tol for i in range(3)):
                out.append((x, y))
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        default=os.path.join(REPO, "target", "dev-release", "lumen.exe"),
    )
    parser.add_argument("--variant", choices=("submit", "script", "all"), default="all")
    args = parser.parse_args()
    rc = 0
    if args.variant in ("submit", "all"):
        rc |= variant_submit(args)
    if args.variant in ("script", "all"):
        REQUESTS.clear()
        rc |= variant_script(args)
    return rc


def variant_submit(args) -> int:
    port = _free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Recording)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    mcp_port = _free_port()
    log_path = os.path.join(REPO, ".tmp", "vfs-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.vfs-parent.html"
    print(f"{url} -> {log_path}")

    shots: dict[str, Counter] = {}
    pr: dict[str, list[float]] = {}
    cr: dict[str, list[float]] = {}
    kr: dict[str, list[float]] = {}
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
            kr = _rects(start, "cancel-rects ")
            print("прямоугольники родителя:", pr)
            print("прямоугольники ребёнка: ", cr)
            print("прямоугольники отмены:  ", kr)
            shots["start"] = count_colors(shot())

            def центр(rect: list[float], dx: float = 0.0, dy: float = 0.0):
                return (rect[0] + rect[2] / 2 + dx, rect[1] + rect[3] / 2 + dy)

            if pr:
                # 0. КОНТРОЛЬ: форма СТРАНИЦЫ, отменённая своим обработчиком.
                # Без навигации — иначе он унёс бы оба фрейма.
                click(*центр(pr["pgo"]), pause=1.0)
            if pr and cr:
                # 1. Обычная отправка формы ребёнка.
                click(*центр(cr["go"], pr["f1"][0], pr["f1"][1]), pause=2.0)
                shots["after_submit"] = count_colors(shot())
            if pr and kr:
                # 2. Отмена: обработчик ребёнка зовёт preventDefault().
                click(*центр(kr["nogo"], pr["f2"][0], pr["f2"][1]), pause=1.5)
                shots["after_cancel"] = count_colors(shot())
                # 3. `target=_top` — уводит СТРАНИЦУ.
                click(*центр(kr["topgo"], pr["f2"][0], pr["f2"][1]), pause=2.5)
                shots["after_top"] = count_colors(shot())
            # 4. КОНТРОЛЬ последним: форма самой страницы.
            tr = _rects(_markers(log_path), "top-rects ")
            print("прямоугольники страницы:", tr)
            if tr:
                click(*центр(tr["psub"]), pause=2.0)
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
        print(f"  {path} x{n}")
    print("маркеры:", *markers, sep="\n  ")
    for name, hist in shots.items():
        print(f"цвета {name}: red={hist[RED]} green={hist[GREEN]} "
              f"yellow={hist[YELLOW]} blue={hist[(0, 0, 255)]}")

    expect("parent-start")
    expect("child-start")
    expect("cancel-start")
    check(bool(pr) and bool(cr) and bool(kr), "все три документа отчитались прямоугольниками")

    # --- КОНТРОЛЬ: та же машинерия на СТРАНИЦЕ ---
    expect("parent-submit prevented")

    # --- субъект: отправка формы ребёнка ---
    # Клик отдельно от отправки: без него «отправки нет» и «клик не дошёл»
    # неразличимы (срез 16 доводит клик, срез 20 — то, что за ним следует).
    check(any(m.startswith("child-click id=go") for m in markers),
          "клик дошёл до самой кнопки под-документа (срез 16)")
    expect("child-submit")
    subm = [p for p in REQUESTS if p.startswith("/.vfs-result.html")]
    check(subm == ["/.vfs-result.html?q=abc"],
          f"сервер увидел отправку с полями формы: {subm}")
    check(shots.get("after_submit", Counter())[GREEN] > 10000,
          "окно фрейма показывает документ-результат (зелёный)")
    check(shots.get("after_submit", Counter())[RED] == 0,
          "прежний документ фрейма ушёл с экрана")
    check(REQUESTS["/.vfs-parent.html"] == 1,
          "страница НЕ перезагрузилась от отправки внутри фрейма")

    # --- отмена ---
    check(any(m.startswith("cancel-click id=nogo") for m in markers),
          "клик дошёл до кнопки отменяющей формы")
    expect("cancel-submit prevented")
    # Тот же адрес стоит `action` у контрольной формы СТРАНИЦЫ: ни одна
    # отменённая отправка не имеет права уйти в сеть.
    check(not any(p.startswith("/.vfs-never.html") for p in REQUESTS),
          "ни одна отменённая отправка не ушла в сеть")
    check(shots.get("after_cancel", Counter())[YELLOW]
          == shots.get("start", Counter())[YELLOW],
          "фрейм с отменой остался прежним (жёлтый нетронут)")

    # --- target=_top ---
    top = [p for p in REQUESTS if p.startswith("/.vfs-top.html")]
    check(top == ["/.vfs-top.html?t=1"], f"target=_top увёл СТРАНИЦУ: {top}")
    expect("top-start /.vfs-top.html?t=1")

    # --- контроль: форма самой страницы ---
    ctl = [p for p in REQUESTS if p.startswith("/.vfs-page-result.html")]
    check(ctl == ["/.vfs-page-result.html?who=page"],
          f"КОНТРОЛЬ — форма страницы отправляется: {ctl}")

    print("ИТОГ variant=submit:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


def variant_script(args) -> int:
    """Второй вход в ту же отправку — скрипт самого ребёнка, плюс блокировка
    валидацией.

    Что здесь есть такого, чего нет в первом варианте:

    * `form.submit()` и `requestSubmit()` — та же отправка, начатая не кликом.
      Они различаются ровно шагом 11: `submit()` его пропускает по спеке, а
      `requestSubmit()` рассылает событие САМ, на JS-стороне, — поэтому шелл не
      имеет права разослать его повторно, и маркер обязан встретиться РОВНО
      один раз;
    * ОТНОСИТЕЛЬНЫЙ `action` из подкаталога: он резолвится базой РЕБЁНКА, а не
      страницы, и увидеть разницу может только сервер (`/sub/r.html`, а не
      `/r.html`);
    * непройденная валидация: отправка не уходит в сеть, и шелл говорит об
      этом в лог. Пиксельной проверки подсказки здесь НЕТ, и это установлено
      контролем: снимок `resource://screenshot` не содержит оверлеев вовсе —
      подсказка САМОЙ СТРАНИЦЫ на нём тоже даёт ноль пикселей. Перевод
      координат ребёнка в координаты страницы проверяется юнит-тестом
      `frame_page_origin_*` в `lumen-shell`.
    """
    port = _free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Recording)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    mcp_port = _free_port()
    log_path = os.path.join(REPO, ".tmp", "vfs2-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.vfs2-parent.html"
    print(f"{url} -> {log_path}")

    pr: dict[str, list[float]] = {}
    vr: dict[str, list[float]] = {}
    tip: list[tuple[int, int]] = []
    ctl_tip: list[tuple[int, int]] = []
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(mcp_port), url],
            stdout=subprocess.DEVNULL, stderr=log, text=True, cwd=HERE,
        )
        try:
            client = Client(mcp_port, log_path)
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            # Оба скриптовых вызова стоят на таймерах ребёнка (1.5 и 2.5 с) —
            # ждём с запасом, ничего не нажимая.
            time.sleep(6.0)
            shot = lambda: base64.b64decode(  # noqa: E731
                client._raw_call("resources/read", {"uri": "resource://screenshot"})
                ["contents"][0]["data"]
            )
            markers = _markers(log_path)
            pr = _rects(markers, "parent-rects ")
            vr = _rects(markers, "v-rects ")
            print("прямоугольники родителя:", pr)
            print("прямоугольники валидации:", vr)
            if pr:
                # КОНТРОЛЬ: та же подсказка на СТРАНИЦЕ. Он отвечает, видна ли
                # она вообще на снимке — оверлеи рисуются отдельным проходом, и
                # без него «подсказки нет» и «снимок её не показывает»
                # неразличимы.
                client.call("click", {"target": {"point": {
                    "x": pr["pvgo"][0] + pr["pvgo"][2] / 2,
                    "y": pr["pvgo"][1] + pr["pvgo"][3] / 2,
                }}})
                time.sleep(1.5)
                ctl_tip = _pixels_near(shot(), TOOLTIP_ON_WHITE)
            if pr and vr:
                # Клик по submit формы с незаполненным `required`.
                client.call("click", {"target": {"point": {
                    "x": vr["vgo"][0] + vr["vgo"][2] / 2 + pr["g2"][0],
                    "y": vr["vgo"][1] + vr["vgo"][3] / 2 + pr["g2"][1],
                }}})
                time.sleep(1.5)
                tip = _pixels_near(shot(), TOOLTIP_BG)
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
        print(f"  {path} x{n}")
    print("маркеры:", *markers, sep="\n  ")

    check(any(m.startswith("s-call") for m in markers)
          and any(m.startswith("q-call") for m in markers),
          "оба скриптовых вызова состоялись")
    # `submit()` — отправка есть, события нет (§4.10.21.3).
    check(REQUESTS["/sub/r.html?s=1"] == 1,
          "form.submit() ребёнка ушёл в сеть с относительным адресом от ЕГО базы")
    check(not any(m == "s-submit-event" for m in markers),
          "form.submit() не разослал событие submit (спека)")
    # `requestSubmit()` — событие рассылает JS-сторона, шелл не повторяет.
    check(REQUESTS["/sub/r.html?q=1"] == 1, "form.requestSubmit() ребёнка ушёл в сеть")
    check(sum(1 for m in markers if m == "q-submit-event") == 1,
          f"событие submit разослано РОВНО один раз: "
          f"{sum(1 for m in markers if m == 'q-submit-event')}")
    # Валидация: в сеть не ушло ничего, подсказка нарисована на месте.
    check(not any(p.startswith("/sub/never.html") for p in REQUESTS),
          "отправка с незаполненным required не ушла в сеть")
    check(any(m.startswith("v-click id=vgo") for m in markers),
          "клик дошёл до submit-кнопки формы с валидацией")
    # Отказ обязан быть виден в логе, а не только по отсутствию запроса:
    # «форма не ушла» само по себе не отличает отказ валидации от потерянного
    # клика.
    with open(log_path, encoding="utf-8", errors="replace") as handle:
        shell_log = handle.read()
    check("iframe submit blocked" in shell_log,
          "шелл отклонил отправку по непройденной валидации")
    # ПОДСКАЗКА не проверяется пикселями, и это установлено КОНТРОЛЕМ, а не
    # предположением: снимок `resource://screenshot` не содержит оверлеев
    # вовсе — подсказка СТРАНИЦЫ, чей отказ виден в логе строкой
    # `forms: submit blocked`, даёт на снимке ровно ноль пикселей так же, как
    # и фреймовая. Перевод координат ребёнка в координаты страницы проверяется
    # юнит-тестом `frame_page_origin_*` в `lumen-shell`.
    print(f"пикселей подсказки: у ребёнка {len(tip)}, у КОНТРОЛЯ-страницы {len(ctl_tip)}")
    check("submit blocked" in shell_log,
          "КОНТРОЛЬ — та же валидация на СТРАНИЦЕ тоже отклоняет отправку")
    check(len(ctl_tip) == 0 and len(tip) == 0,
          "оверлеи на снимок MCP не попадают ни у страницы, ни у фрейма "
          "(потому пиксельной проверки подсказки здесь нет)")

    print("ИТОГ variant=script:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
