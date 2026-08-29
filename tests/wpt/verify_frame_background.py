#!/usr/bin/env python3
"""BUG-480 срез 21: фон под-документа фрейма ниже его содержимого.

Residual срезов 14/19: сквозь фрейм, чей ребёнок короче вьюпорта, видно фон
СТРАНИЦЫ вместо фона ребёнка — фон `<body>` рисуется только до высоты
содержимого (`paint_ordered` ребёнка), а «канвас»-заливка (`canvas_background_color`
+ `set_canvas_background`) существует только у страницы, применяется на
уровне рендерера и о вклеенных фреймах ничего не знает.

Разные цвета у страницы и у ребёнка, содержимое ребёнка короче вьюпорта
фрейма — единственный способ отличить «фон ребёнка дотянут до низа фрейма» от
«сквозь фрейм видна страница».

Запуск: python tests/wpt/verify_frame_background.py --binary <абсолютный путь к lumen.exe>
"""

import argparse
import http.server
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import threading
import zlib


def read_png_rgba(path: str):
    with open(path, "rb") as f:
        data = f.read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "не PNG"
    pos, idat, w = 8, b"", None
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


class _Handler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_args):  # noqa: D102
        pass


PAGE_RGB = (0, 0, 255)    # синий — фон СТРАНИЦЫ
CHILD_RGB = (255, 0, 0)   # красный — фон РЕБЁНКА, содержимое короче фрейма

PAGE = f"""<!doctype html><html><body style="margin:0;background:rgb{PAGE_RGB}">
<iframe src="child.html" width="200" height="300" style="border:0;display:block;position:absolute;left:0;top:0"></iframe>
</body></html>"""

# Содержимое ребёнка — 20px текста от силы, фрейм — 300px высотой: под ним
# 280px, которые должны закраситься фоном ребёнка, а не остаться пустыми.
CHILD = f"""<!doctype html><html><body style="margin:0;background:rgb{CHILD_RGB}">
<div style="height:20px">x</div>
</body></html>"""


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True, help="абсолютный путь к lumen.exe")
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()

    root = tempfile.mkdtemp(prefix="frame-bg-")
    with open(os.path.join(root, "page.html"), "w", encoding="utf-8") as f:
        f.write(PAGE)
    with open(os.path.join(root, "child.html"), "w", encoding="utf-8") as f:
        f.write(CHILD)

    handler = lambda *a, **k: _Handler(*a, directory=root, **k)  # noqa: E731
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    port = srv.server_address[1]
    threading.Thread(target=srv.serve_forever, daemon=True).start()

    out_png = os.path.join(root, "shot.png")
    url = f"http://127.0.0.1:{port}/page.html"
    proc = subprocess.run(
        [args.binary, "--screenshot", out_png, url],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=120,
    )
    ok = True

    if not os.path.exists(out_png):
        print("[ФЕЙЛ] снимок не создан")
        print(proc.stderr[-2000:])
        return 1

    w, h, rows = read_png_rgba(out_png)
    print(f"--- снимок {w}x{h} ---")

    def count(y0, y1, x0, x1, rgb):
        n = 0
        for y in range(max(0, y0), min(h, y1)):
            row = rows[y]
            for x in range(max(0, x0), min(w, x1)):
                if tuple(row[x * 4 : x * 4 + 3]) == rgb:
                    n += 1
        return n

    # Полоса фрейма ниже содержимого ребёнка: y in [20, 300), x in [0, 200).
    below_child_bg = count(20, 300, 0, 200, CHILD_RGB)
    below_page_bg = count(20, 300, 0, 200, PAGE_RGB)
    total = 280 * 200

    print(
        f"[{'OK ' if below_child_bg > total * 0.9 else 'ФЕЙЛ'}] "
        f"под содержимым ребёнка закрашено фоном РЕБЁНКА: {below_child_bg}/{total}"
    )
    ok &= below_child_bg > total * 0.9
    print(
        f"[{'OK ' if below_page_bg == 0 else 'ФЕЙЛ (residual)'}] "
        f"под содержимым ребёнка НЕТ фона страницы: {below_page_bg}/{total}"
    )
    ok &= below_page_bg == 0

    srv.shutdown()
    if not args.keep:
        shutil.rmtree(root, ignore_errors=True)
    else:
        print(f"каталог пробы: {root}")
    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
