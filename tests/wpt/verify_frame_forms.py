#!/usr/bin/env python3
"""BUG-480 срез 18: НАТИВНЫЕ элементы управления формы внутри фрейма.

Срез 16 довёл клик мышью до под-документа, срез 17 — прокрутку. Но клик до
сих пор только РАССЫЛАЛСЯ слушателям ребёнка: собственного поведения
элемента (флажок, радиокнопка, `<summary>`) шелл для фрейма не исполнял
вовсе — ранний возврат в `handle_click_at` пропускает и форму, и ссылку,
потому что единственный узел СТРАНИЦЫ под этой точкой — сам `<iframe>`.

Что меряется и почему именно так:

* КОНТРОЛЬ на той же странице — флажок РОДИТЕЛЯ. Если он не переключается,
  проба меряет свою арифметику координат, а не движок;
* точки кликов считаются НЕ из разметки, а из `getBoundingClientRect()`,
  который обе стороны печатают в лог по таймеру. Первая версия пробы ставила
  элементам `position:absolute` и промахивалась мимо всех до одного:
  инлайн-уровневый бокс движок из потока не выносит (BUG-928), а `<iframe>`
  рядом позиционируется верно — так что разметка тут не адрес. Замер
  отложен таймером и по той же причине не делается в самом скрипте: до
  первого layout `getBoundingClientRect` честно отдаёт нули;
* состояние читается не в обработчике клика, а ПОЗЖЕ — в строке СЛЕДУЮЩЕГО
  клика (по пустому месту): шелл переключает атрибут ПОСЛЕ рассылки
  `click` в JS, поэтому обработчик самого флажка видит ещё старое значение
  (то же и на странице). Читается `hasAttribute`, а не IDL-свойство: шелл
  пишет именно атрибут (`forms::toggle_checkbox`/`toggle_details_open`);
* ПИКСЕЛИ (снимок живого окна) — отдельное доказательство: раскрытый
  `<details>` показывает синюю панель, значит пересчитаны и layout ребёнка, и
  его display list, и вклейка в список страницы. Состояние в DOM без пикселей
  означало бы, что атрибут переключён, а на экране ничего не изменилось;
* радиокнопка сверяется СО СТРАНИЦЕЙ, а не со спекой: контроль показал, что
  отметку с соседа по группе нативный клик не снимает и там (BUG-927), то
  есть это общий пробел шелла, и заводить расхождение во фрейме нельзя;
* `<details>` кликается ПОСЛЕДНИМ и стоит в разметке ниже всех: его
  раскрытие двигает вёрстку, и снятые в начале прямоугольники соседей
  остались бы враньём.

Запуск: python tests/wpt/verify_frame_forms.py --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
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

FRAME_X, FRAME_Y, FRAME_W, FRAME_H = 40, 120, 300, 200

BLUE = (0, 0, 255)      # панель, которую раскрывает <details>

PARENT_PAGE = f"""<!doctype html><meta charset="utf-8"><title>vff parent</title>
<body style="margin:0;background:#fff">
<div><input type="checkbox" id="pcb"></div>
<div><input type="radio" name="pg" id="pr1" checked><input type="radio" name="pg" id="pr2"></div>
<div id="pfree" style="height:60px"></div>
<iframe src="/.vff-child.html" style="position:absolute;left:{FRAME_X}px;top:{FRAME_Y}px;
        width:{FRAME_W}px;height:{FRAME_H}px;border:0"></iframe>
<script>
console.log('PROBE parent-start');
function vffRect(id) {{
  var r = document.getElementById(id).getBoundingClientRect();
  return [r.left, r.top, r.width, r.height];
}}
// Замер отложен таймером: в момент исполнения скрипта layout ещё не
// посчитан и `getBoundingClientRect` честно отдаёт нули — первая версия
// пробы кликала по (0, 0) четыре раза подряд.
setTimeout(function () {{
  console.log('PROBE parent-rects ' + JSON.stringify({{
    pcb: vffRect('pcb'), pr2: vffRect('pr2'), free: vffRect('pfree')
  }}));
}}, 800);
// Состояние печатает ОБЩИЙ слушатель `document`, а не отдельная кнопка:
// `<button>` в этом движке получает нулевую ширину, попасть по нему нельзя.
// Шелл переключает атрибут ПОСЛЕ рассылки `click`, поэтому итог виден в
// строке СЛЕДУЮЩЕГО клика — по пустому месту.
document.addEventListener('click', function () {{
  console.log('PROBE parent-click pcb='
       + document.getElementById('pcb').hasAttribute('checked')
       + ' r1=' + document.getElementById('pr1').hasAttribute('checked')
       + ' r2=' + document.getElementById('pr2').hasAttribute('checked'));
}});
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vff child</title>
<body style="margin:0;background:#eee">
<div><input type="checkbox" id="cb"></div>
<div><input type="radio" name="g" id="r1" checked><input type="radio" name="g" id="r2"></div>
<div id="free" style="height:60px"></div>
<details id="det">
  <summary id="sum">more</summary>
  <div id="panel" style="width:140px;height:40px;background:rgb(0,0,255)"></div>
</details>
<script>
console.log('PROBE child-start');
function vffRect(id) {
  var r = document.getElementById(id).getBoundingClientRect();
  return [r.left, r.top, r.width, r.height];
}
setTimeout(function () {
  console.log('PROBE child-rects ' + JSON.stringify({
    cb: vffRect('cb'), r2: vffRect('r2'), sum: vffRect('sum'), free: vffRect('free')
  }));
}, 800);
document.addEventListener('click', function (ev) {
  console.log('PROBE child-click id=' + (ev.target ? ev.target.id : 'null')
       + ' cb=' + document.getElementById('cb').hasAttribute('checked')
       + ' det=' + document.getElementById('det').hasAttribute('open')
       + ' r1=' + document.getElementById('r1').hasAttribute('checked')
       + ' r2=' + document.getElementById('r2').hasAttribute('checked'));
});
</script>
</body>
"""


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
    hist = Counter()
    for y in range(h):
        row = rows[y]
        for x in range(w):
            hist[tuple(row[x * 4 : x * 4 + 3])] += 1
    return hist


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
    args = parser.parse_args()

    for name, body in [(".vff-parent.html", PARENT_PAGE), (".vff-child.html", CHILD_PAGE)]:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port = _free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Quiet)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    mcp_port = _free_port()
    log_path = os.path.join(REPO, ".tmp", "vff-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.vff-parent.html"
    print(f"{url} -> {log_path}")

    before = after = Counter()
    pr: dict[str, list[float]] = {}
    cr: dict[str, list[float]] = {}
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

            def click(x: float, y: float, pause: float = 0.6) -> None:
                client.call("click", {"target": {"point": {"x": x, "y": y}}})
                time.sleep(pause)

            # Прямоугольники печатаются самими документами при старте — лог
            # читается на ходу, до первого клика.
            start = _markers(log_path)
            pr = _rects(start, "parent-rects ")
            cr = _rects(start, "child-rects ")
            print("прямоугольники родителя:", pr)
            print("прямоугольники ребёнка: ", cr)

            def центр(rect: list[float], dx: float = 0.0, dy: float = 0.0):
                return (rect[0] + rect[2] / 2 + dx, rect[1] + rect[3] / 2 + dy)

            shot = lambda: base64.b64decode(  # noqa: E731
                client._raw_call("resources/read", {"uri": "resource://screenshot"})
                ["contents"][0]["data"]
            )
            before = count_colors(shot())
            if pr:
                # Контроль: флажок РОДИТЕЛЯ, затем клик по пустому месту,
                # чтобы слушатель `document` напечатал итог.
                click(*центр(pr["pcb"]))
                click(*центр(pr["pr2"]))
                click(*центр(pr["free"]))
            if cr:
                # Субъект: три элемента управления ВНУТРИ фрейма. `<details>`
                # последним — его раскрытие двигает вёрстку под собой.
                click(*центр(cr["cb"], FRAME_X, FRAME_Y))
                click(*центр(cr["r2"], FRAME_X, FRAME_Y))
                click(*центр(cr["sum"], FRAME_X, FRAME_Y))
                time.sleep(1.0)
                after = count_colors(shot())
                # Итоговое состояние — в строке СЛЕДУЮЩЕГО клика (по пустому
                # месту): переключение шелл делает после рассылки `click`.
                click(*центр(cr["free"], FRAME_X, FRAME_Y), pause=1.0)
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

    expect("parent-start")
    expect("child-start")
    check(bool(pr) and bool(cr), "обе стороны отчитались прямоугольниками")
    # Контроль: те же элементы на СТРАНИЦЕ. Он же отвечает на вопрос, чей
    # дефект — фреймовый или общий: эксклюзивность радиогруппы шелл не
    # реализует ни там, ни там, поэтому проба спрашивает у страницы, а не у
    # спеки.
    parent_clicks = [m for m in markers if m.startswith("parent-click ")]
    print("клики родителя:", *parent_clicks, sep="\n  ")
    expect("parent-click pcb=true")
    # Клик доходит до ребёнка (срез 16) — иначе всё ниже мерило бы доставку.
    child_clicks = [m for m in markers if m.startswith("child-click ")]
    print("клики ребёнка:", *child_clicks, sep="\n  ")
    check(len(child_clicks) >= 4, "клики дошли до под-документа (срез 16)")
    check(any(m.startswith("child-click id=cb ") for m in child_clicks)
          and any(m.startswith("child-click id=r2 ") for m in child_clicks)
          and any(m.startswith("child-click id=sum ") for m in child_clicks),
          "клики попали в сами элементы управления, а не мимо")
    # Субъект: три состояния разом. `r1=true` — НЕ опечатка: контроль выше
    # показал, что нативный клик по радиокнопке не снимает отметку с соседа
    # и на самой СТРАНИЦЕ (`parent-click … r1=true r2=true`), то есть это
    # общий пробел шелла, а не фреймовый. Проба сверяется со страницей.
    check(bool(child_clicks)
          and "cb=true det=true r1=true r2=true" in child_clicks[-1],
          f"итоговое состояние ребёнка: {child_clicks[-1:] or '—'}")
    check(bool(parent_clicks) and "r1=true r2=true" in parent_clicks[-1],
          "у страницы радиогруппа ведёт себя так же (общий пробел, не фреймовый)")
    # Пиксели: панель <details> появилась на экране.
    print(f"синие пиксели: до={before[BLUE]} после={after[BLUE]}")
    check(before[BLUE] == 0, "до клика панель <details> не видна (контроль)")
    check(after[BLUE] > 1000, "после клика по <summary> панель нарисована")

    print("маркеры:", *markers, sep="\n  ")
    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
