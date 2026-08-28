#!/usr/bin/env python3
"""BUG-480 срез 17: скролл ВНУТРИ фрейма.

Что меряет и почему именно так:

* `window.scrollTo` вызывает СОБСТВЕННЫЙ скрипт ребёнка — единственный путь
  скролла под-документа, доступный пробе: колесо мыши не рождается ни одной
  поверхностью автоматизации (`InputCommand` его не знает, см. отклонения
  среза 16), поэтому wheel закрывают юнит-тесты, а не эта проба;
* «прокрутилось» проверяется ТРЕМЯ независимыми способами, потому что каждый
  по отдельности может быть правдой при сломанных остальных:
  1. `window.scrollY` и событие `scroll` в самом ребёнке — состояние;
  2. ПИКСЕЛИ (снимок живого окна через `resource://screenshot`) — рисование;
  3. НАСТОЯЩИЙ клик мышью в ту же точку окна — hit-тест. Он обязан попасть в
     блок, который ПРИЕХАЛ под курсор, а не в тот, что был там до скролла;
* контроль обратного знака: страница-родитель НЕ должна прокрутиться от
  скролла ребёнка (у неё своя прокрутка, и до среза 17 запрос ребёнка либо
  не делал ничего, либо — если его дренировать не туда — увёз бы страницу).

Ребёнок раскрашен полосами по 100 px: A красная (0..100), B зелёная
(100..200), C синяя (200..300), дальше серый добор до 600 px. Вьюпорт фрейма
200 px, значит до скролла видны A+B, после `scrollTo(0, 200)` — C и серое.

Запуск: python tests/wpt/verify_frame_scroll.py --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
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

FRAME_X, FRAME_Y, FRAME_W, FRAME_H = 40, 120, 300, 200
SCROLL_TO = 200          # на сколько ребёнок прокручивает себя сам
CLICK_DY = 30            # точка клика внутри фрейма, от его верхнего края

RED, GREEN, BLUE = (255, 0, 0), (0, 255, 0), (0, 0, 255)

PARENT_PAGE = f"""<!doctype html><meta charset="utf-8"><title>vfs parent</title>
<body style="margin:0;background:#fff">
<iframe src="/.vfs-child.html" style="position:absolute;left:{FRAME_X}px;top:{FRAME_Y}px;
        width:{FRAME_W}px;height:{FRAME_H}px;border:0"></iframe>
<script>
console.log('PROBE parent-start');
window.addEventListener('scroll', function () {{
  console.log('PROBE parent-scrolled y=' + window.scrollY);
}});
window.addEventListener('message', function (ev) {{
  console.log('PROBE parent-got ' + JSON.stringify(ev.data));
}});
function vfsScrollChild() {{
  var f = document.querySelector('iframe');
  f.contentWindow.postMessage({{ scrollTo: {SCROLL_TO} }}, '*');
  console.log('PROBE parent-asked');
}}
</script>
</body>
"""

CHILD_PAGE = f"""<!doctype html><meta charset="utf-8"><title>vfs child</title>
<body style="margin:0">
<div id="a" style="height:100px;background:rgb(255,0,0)"></div>
<div id="b" style="height:100px;background:rgb(0,255,0)"></div>
<div id="c" style="height:100px;background:rgb(0,0,255)"></div>
<div id="tail" style="height:300px;background:#888"></div>
<script>
console.log('PROBE child-start');
window.addEventListener('scroll', function () {{
  console.log('PROBE child-scroll y=' + window.scrollY);
  window.parent.postMessage({{ childScrollY: window.scrollY }}, '*');
}});
window.addEventListener('scrollend', function () {{
  console.log('PROBE child-scrollend y=' + window.scrollY);
}});
document.addEventListener('click', function (ev) {{
  console.log('PROBE child-click id=' + (ev.target ? ev.target.id : 'null')
              + ' y=' + ev.clientY);
}});
window.addEventListener('message', function (ev) {{
  if (!ev.data || typeof ev.data.scrollTo !== 'number') return;
  window.scrollTo(0, ev.data.scrollTo);
  // Чтение СРАЗУ после вызова — не проверка, а запись факта: `scrollTo`
  // асинхронен и на странице тоже (запрос копится в рантайме, позицию
  // возвращает шелл), поэтому здесь ожидается прежний ноль.
  console.log('PROBE child-asked-scrollY=' + window.scrollY);
}});
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        default=os.path.join(REPO, "target", "dev-release", "lumen.exe"),
    )
    args = parser.parse_args()

    for name, body in [(".vfs-parent.html", PARENT_PAGE), (".vfs-child.html", CHILD_PAGE)]:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port = _free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Quiet)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    mcp_port = _free_port()
    log_path = os.path.join(REPO, ".tmp", "vfs-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.vfs-parent.html"
    print(f"{url} -> {log_path}")

    before = after = Counter()
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(mcp_port), url],
            stdout=subprocess.DEVNULL, stderr=log, text=True, cwd=HERE,
        )
        try:
            client = Client(mcp_port, log_path)
            client.call("wait", {"condition": "document_ready", "timeout_ms": 10000})
            time.sleep(1.0)
            shot = lambda: base64.b64decode(  # noqa: E731
                client._raw_call("resources/read", {"uri": "resource://screenshot"})
                ["contents"][0]["data"]
            )
            before = count_colors(shot())
            # Контроль ДО скролла: клик в точку фрейма попадает в первую полосу.
            client.call("click", {"target": {"point": {"x": FRAME_X + 60,
                                                       "y": FRAME_Y + CLICK_DY}}})
            time.sleep(0.5)
            # Субъект: ребёнок прокручивает СЕБЯ.
            client.call("eval", {"code": "vfsScrollChild()"})
            time.sleep(1.5)
            after = count_colors(shot())
            # Тот же клик после скролла: под курсором теперь другая полоса.
            client.call("click", {"target": {"point": {"x": FRAME_X + 60,
                                                       "y": FRAME_Y + CLICK_DY}}})
            time.sleep(1.0)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
            server.shutdown()

    with open(log_path, encoding="utf-8", errors="replace") as handle:
        markers = re.findall(r"PROBE ([^\n\r]+)", handle.read())

    ok = True

    def check(cond: bool, text: str) -> None:
        nonlocal ok
        ok &= bool(cond)
        print(f"[{'OK  ' if cond else 'ФЕЙЛ'}] {text}")

    def expect(substr: str) -> None:
        check(any(substr in m for m in markers), f"есть «{substr}»")

    def forbid(substr: str, why: str) -> None:
        check(not any(substr in m for m in markers), f"нет «{substr}» — {why}")

    expect("parent-start")
    expect("child-start")
    expect("parent-asked")
    # 1. Состояние: скролл применился к ребёнку и он о нём узнал.
    expect(f"child-scroll y={SCROLL_TO}")
    expect(f"child-scrollend y={SCROLL_TO}")
    expect(f'parent-got {{"childScrollY":{SCROLL_TO}}}')
    # 2. Контроль обратного знака: страница осталась на месте.
    forbid("parent-scrolled", "скролл ребёнка не двигает страницу")
    # 3. Hit-тест: до скролла точка в полосе A, после — в полосе C.
    clicks = [m for m in markers if m.startswith("child-click ")]
    print("клики ребёнка:", clicks)
    check(len(clicks) >= 1 and f"id=a y={CLICK_DY}" in clicks[0],
          f"клик ДО скролла попал в полосу A (контроль): {clicks[:1]}")
    check(len(clicks) >= 2 and f"id=c y={CLICK_DY}" in clicks[1],
          f"клик ПОСЛЕ скролла попал в полосу C: {clicks[1:2]}")
    # 4. Пиксели: до — красная и зелёная полосы, после — синяя.
    print(f"пиксели до:    R={before[RED]} G={before[GREEN]} B={before[BLUE]}")
    print(f"пиксели после: R={after[RED]} G={after[GREEN]} B={after[BLUE]}")
    check(before[RED] > 1000 and before[GREEN] > 1000 and before[BLUE] == 0,
          "до скролла во фрейме видны полосы A и B (контроль рисования)")
    check(after[BLUE] > 1000 and after[RED] == 0 and after[GREEN] == 0,
          "после скролла во фрейме видна полоса C, а A и B уехали")

    print("маркеры:", *markers, sep="\n  ")
    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
