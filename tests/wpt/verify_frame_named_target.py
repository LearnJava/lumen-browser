#!/usr/bin/env python3
"""BUG-480 срез 24: именованный `target` (`target="имя"`) адресует ЖИВОЙ фрейм.

Срез 19 разобрал `_self`/`_top`/`_parent`, а любое другое имя — включая
имя, СОВПАВШЕЕ с `name` реального `<iframe>` — уходило в одну ветку с
`_blank` («вспомогательных окон движок не создаёт», BUG-883). Здесь для
совпавшего имени заводится третий исход: навигация НАЗВАННОГО фрейма, а не
отказ. Дыра была на двух разных путях сразу — `Lumen::link_destination`
(ссылка РЕБЁНКА, уже читала `target`) и клик по ссылке СТРАНИЦЫ
(`click.rs`, `target` вообще не читался) — проба меряет оба.

Что меряется и почему именно так:

* ДВА направления, а не одно: ссылка СТРАНИЦЫ на именованный фрейм (новый
  путь) и ссылка ВНУТРИ одного фрейма на ИМЕНОВАННОГО СОСЕДА (переиспользует
  `link_destination`, но до среза 24 находила там только `NewWindow`);
* ЗАПРОСЫ считает сервер пробы, а не браузер (BUG-826) — счётчик, а не
  множество: страница-хозяин не должна перезагружаться, когда навигирует
  ТОЛЬКО названный фрейм;
* КОНТРОЛЬ — несовпавшее имя (`target="нет-такого"`) со страницы: до среза 24
  `target` там игнорировался целиком, так что ссылка вела страницу как
  обычная — это же и есть спек-корректный откат для имени без совпадения
  (движок не создаёт окно), и он обязан остаться ТЕМ ЖЕ после правки, а не
  стать новым отказом;
* ПИКСЕЛИ — отдельное доказательство поверх маркеров: после навигации именно
  ТОГО фрейма его прямоугольник меняет цвет, а прямоугольник соседа — нет.

Запуск: python tests/wpt/verify_frame_named_target.py --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
"""

from __future__ import annotations

import argparse
import base64
import http.server
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

RED = (255, 0, 0)         # содержимое фрейма content ДО
GREEN = (0, 200, 0)       # содержимое фрейма content ПОСЛЕ (ссылка страницы)
BLUE = (0, 0, 255)        # содержимое фрейма other ДО
PURPLE = (128, 0, 128)    # содержимое фрейма other ПОСЛЕ (ссылка соседа-фрейма)
YELLOW = (255, 255, 0)    # СТРАНИЦА после клика по несовпавшему имени (контроль)

PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vnt parent</title>
<body style="margin:0;background:#fff">
<iframe id="f1" name="content" src="/.vnt-content.html" style="position:absolute;
        left:40px;top:80px;width:260px;height:160px;border:0"></iframe>
<iframe id="f2" name="other" src="/.vnt-other.html" style="position:absolute;
        left:40px;top:280px;width:260px;height:160px;border:0"></iframe>
<a id="pnamed" href="/.vnt-content2.html" target="content" style="display:block;
   position:absolute;left:340px;top:80px;width:180px;height:40px;background:#ccc">named</a>
<a id="pmiss" href="/.vnt-page-miss.html" target="no-such-name" style="display:block;
   position:absolute;left:340px;top:140px;width:180px;height:40px;background:#eee">miss</a>
<script>
console.log('PROBE parent-start ' + location.pathname);
function vntRect(id) {
  var r = document.getElementById(id).getBoundingClientRect();
  return [r.left, r.top, r.width, r.height];
}
setTimeout(function () {
  console.log('PROBE parent-rects ' + JSON.stringify({
    f1: vntRect('f1'), f2: vntRect('f2'), pnamed: vntRect('pnamed'), pmiss: vntRect('pmiss')
  }));
}, 800);
</script>
</body>
"""

CONTENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vnt content</title>
<body style="margin:0;background:rgb(255,0,0)">
<a id="lsib" href="/.vnt-other2.html" target="other" style="display:block;width:200px;
   height:40px;background:#fff">to sibling</a>
<script>
console.log('PROBE content-start ' + location.pathname);
setTimeout(function () {
  var r = document.getElementById('lsib').getBoundingClientRect();
  console.log('PROBE content-rects ' + JSON.stringify({lsib: [r.left, r.top, r.width, r.height]}));
}, 800);
</script>
</body>
"""

OTHER_PAGE = """<!doctype html><meta charset="utf-8"><title>vnt other</title>
<body style="margin:0;background:rgb(0,0,255)">
<script>console.log('PROBE other-start ' + location.pathname);</script>
</body>
"""

CONTENT2_PAGE = """<!doctype html><meta charset="utf-8"><title>vnt content2</title>
<body style="margin:0;background:rgb(0,200,0)">
<script>console.log('PROBE content2-start ' + location.pathname);</script>
</body>
"""

OTHER2_PAGE = """<!doctype html><meta charset="utf-8"><title>vnt other2</title>
<body style="margin:0;background:rgb(128,0,128)">
<script>console.log('PROBE other2-start ' + location.pathname);</script>
</body>
"""

PAGE_MISS = """<!doctype html><meta charset="utf-8"><title>vnt page-miss</title>
<body style="margin:0;background:rgb(255,255,0)">
<script>console.log('PROBE miss-start ' + location.pathname);</script>
</body>
"""

PAGES = {
    ".vnt-parent.html": PARENT_PAGE,
    ".vnt-content.html": CONTENT_PAGE,
    ".vnt-other.html": OTHER_PAGE,
    ".vnt-content2.html": CONTENT2_PAGE,
    ".vnt-other2.html": OTHER2_PAGE,
    ".vnt-page-miss.html": PAGE_MISS,
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
    браузер за документом фрейма/страницы."""

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
    import json
    for m in markers:
        if m.startswith(prefix):
            return json.loads(m[len(prefix):])
    return {}


def в_фрейме(frame: list[float], rect: list[float]):
    return (frame[0] + rect[0] + rect[2] / 2, frame[1] + rect[1] + rect[3] / 2)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        default=os.path.join(REPO, "target", "dev-release", "lumen.exe"),
    )
    args = parser.parse_args()

    port = _free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Recording)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    mcp_port = _free_port()
    log_path = os.path.join(REPO, ".tmp", "vnt-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.vnt-parent.html"
    print(f"{url} -> {log_path}")

    shots: dict[str, Counter] = {}
    pr: dict[str, list[float]] = {}
    ctr: dict[str, list[float]] = {}
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(mcp_port), url],
            stdout=subprocess.DEVNULL, stderr=log, text=True, cwd=HERE,
        )
        try:
            client = Client(mcp_port, log_path)
            # 30 с: на холодном профиле выбор бэкенда wgpu (`backend_probe`)
            # сам по себе занимает десятки секунд.
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
            pr = _rects(start, "parent-rects ")
            ctr = _rects(start, "content-rects ")
            print("фреймы/ссылки родителя:", pr)
            print("ссылка content на соседа:", ctr)

            shots["before"] = count_colors(shot())

            if pr and ctr:
                # 1. Ссылка ВНУТРИ фрейма content с target="other" — навигация
                # ИМЕНОВАННОГО СОСЕДА (`link_destination`, срез 24).
                click(*в_фрейме(pr["f1"], ctr["lsib"]))
                shots["sibling"] = count_colors(shot())

            if pr:
                # 2. Ссылка СТРАНИЦЫ с target="content" — навигация ИМЕНОВАННОГО
                # фрейма без перезагрузки страницы (click.rs, срез 24).
                click(*в_фрейме((0, 0), pr["pnamed"]))
                shots["named"] = count_colors(shot())

            if pr:
                # 3. КОНТРОЛЬ: ссылка страницы с несовпавшим именем ведёт себя
                # так же, как до среза 24, — навигирует САМУ страницу.
                click(*в_фрейме((0, 0), pr["pmiss"]))
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
    expect("content-start")
    expect("other-start")
    check(bool(pr) and bool(ctr), "все стартовые документы отчитались прямоугольниками")

    check(shots.get("before", Counter())[GREEN] == 0, "до кликов зелёного нет (контроль)")
    check(shots.get("before", Counter())[PURPLE] == 0, "до кликов фиолетового нет (контроль)")

    # 1. Ссылка ребёнка на ИМЕНОВАННОГО соседа: навигирует ТОЛЬКО сосед.
    check(REQUESTS.get("/.vnt-other2.html", 0) == 1,
          f"target соседа-фрейма запрошен ровно раз (x{REQUESTS.get('/.vnt-other2.html', 0)})")
    expect("other2-start")
    check(shots.get("sibling", Counter())[PURPLE] > 1000, "сосед-фрейм показал новый документ")
    check(shots.get("sibling", Counter())[RED] > 1000, "фрейм content НЕ тронут навигацией соседа")

    # 2. Ссылка страницы на ИМЕНОВАННЫЙ фрейм: навигирует фрейм, не страница.
    check(REQUESTS.get("/.vnt-content2.html", 0) == 1,
          f"именованный target страницы запрошен ровно раз (x{REQUESTS.get('/.vnt-content2.html', 0)})")
    expect("content2-start")
    check(shots.get("named", Counter())[GREEN] > 1000, "фрейм content показал новый документ")
    check(REQUESTS.get("/.vnt-parent.html", 0) == 1,
          f"страница-хозяин НЕ перезагрузилась (x{REQUESTS.get('/.vnt-parent.html', 0)})")

    # 3. Контроль: несовпавшее имя ведёт страницу, как и до среза 24.
    check(REQUESTS.get("/.vnt-page-miss.html", 0) == 1,
          f"КОНТРОЛЬ: несовпавшее имя всё ещё навигирует страницу "
          f"(x{REQUESTS.get('/.vnt-page-miss.html', 0)})")
    expect("miss-start")

    print("маркеры:", *markers, sep="\n  ")
    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
