#!/usr/bin/env python3
"""BUG-480 срез 15: картинки внутри фрейма — от запроса до ПИКСЕЛЕЙ.

Что меряет (и почему именно так):

* `--screenshot` — единственный headless-путь, дающий настоящие пиксели.
  `--dump-display-list` показал бы команду `DrawImage`, но не отличил бы
  зарегистрированный ключ от незарегистрированного: второй рисуется серой
  заглушкой, а команда в списке в обоих случаях одна и та же.
* сервер пишет ЖУРНАЛ ЗАПРОСОВ (со счётчиком, не множеством): «байты
  запрошены» и «пиксели нарисованы» — разные утверждения, и до этого среза
  верным было только первое.
* namespace-проба: страница и фрейм держат `<img src="pic.png">` с ОДНИМ
  и тем же относительным путём, но из разных каталогов и разного цвета.
  Общий ключ регистрации (raw src) означал бы, что во фрейме окажется
  картинка страницы.

Запуск: python tests/wpt/verify_frame_images.py --binary <абсолютный путь к lumen.exe>
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
from collections import Counter

# --- минимальный PNG: пишем однотонный RGBA, читаем RGBA8-без-интерлейса ---


def _png_chunk(tag: bytes, data: bytes) -> bytes:
    return (
        struct.pack(">I", len(data))
        + tag
        + data
        + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    )


def write_solid_png(path: str, w: int, h: int, rgb: tuple) -> None:
    raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))
    body = (
        b"\x89PNG\r\n\x1a\n"
        + _png_chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
        + _png_chunk(b"IDAT", zlib.compress(raw))
        + _png_chunk(b"IEND", b"")
    )
    with open(path, "wb") as f:
        f.write(body)


def read_png_rgba(path: str):
    """(width, height, [(r,g,b,a), ...]) — только то, что пишет lumen: RGBA8."""
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
    log_counter = Counter()

    def log_message(self, *_args):  # noqa: D102 — тишина вместо stderr-шума
        pass

    def do_GET(self):  # noqa: N802
        _Handler.log_counter[self.path] += 1
        super().do_GET()


PAGE = """<!doctype html><html><body style="margin:0;background:#fff">
<img src="pic.png" width="60" height="40">
<iframe src="child/child.html" width="200" height="100" style="border:0;display:block"></iframe>
</body></html>"""

CHILD = """<!doctype html><html><body style="margin:0;background:#fff">
<img src="pic.png" width="200" height="100">
</body></html>"""

# Вариант nested: картинка живёт во ВНУКЕ (глубина 1). Ключи там переписываются
# при сборке display list ребёнка, куда содержимое внука уже вклеено, — если
# порядок «переписать, потом вклеить» нарушить, картинка внука потеряет ключ, а
# заглушка внука — свой `src`.
PAGE_NESTED = """<!doctype html><html><body style="margin:0;background:#fff">
<img src="pic.png" width="60" height="40">
<iframe src="child/mid.html" width="200" height="100" style="border:0;display:block"></iframe>
</body></html>"""

MID = """<!doctype html><html><body style="margin:0;background:#fff">
<iframe src="grand.html" width="200" height="100" style="border:0;display:block"></iframe>
</body></html>"""

GRAND = """<!doctype html><html><body style="margin:0;background:#fff">
<img src="pic.png" width="200" height="100">
</body></html>"""

PAGE_RGB = (0, 0, 255)   # синий — картинка СТРАНИЦЫ
CHILD_RGB = (255, 0, 0)  # красный — картинка ФРЕЙМА, тот же относительный src


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True, help="абсолютный путь к lumen.exe")
    ap.add_argument("--variant", choices=("flat", "nested"), default="flat")
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()

    root = tempfile.mkdtemp(prefix="frame-img-")
    os.mkdir(os.path.join(root, "child"))
    with open(os.path.join(root, "page.html"), "w", encoding="utf-8") as f:
        f.write(PAGE if args.variant == "flat" else PAGE_NESTED)
    with open(os.path.join(root, "child", "child.html"), "w", encoding="utf-8") as f:
        f.write(CHILD)
    with open(os.path.join(root, "child", "mid.html"), "w", encoding="utf-8") as f:
        f.write(MID)
    with open(os.path.join(root, "child", "grand.html"), "w", encoding="utf-8") as f:
        f.write(GRAND)
    write_solid_png(os.path.join(root, "pic.png"), 60, 40, PAGE_RGB)
    write_solid_png(os.path.join(root, "child", "pic.png"), 200, 100, CHILD_RGB)

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

    print("--- журнал сервера (счётчик, не множество) ---")
    for path, n in sorted(_Handler.log_counter.items()):
        print(f"  GET {path} x{n}")
    frame_doc = "/child/child.html" if args.variant == "flat" else "/child/grand.html"
    for want in ("/page.html", frame_doc, "/pic.png", "/child/pic.png"):
        got = _Handler.log_counter[want]
        print(f"[{'OK ' if got else 'ФЕЙЛ'}] запрошен {want}: {got}")
        ok &= got > 0

    if not os.path.exists(out_png):
        print("[ФЕЙЛ] снимок не создан")
        print(proc.stderr[-2000:])
        return 1

    w, h, rows = read_png_rgba(out_png)
    print(f"--- снимок {w}x{h} ---")

    def count(y0, y1, rgb):
        n = 0
        for y in range(max(0, y0), min(h, y1)):
            row = rows[y]
            for x in range(w):
                px = row[x * 4 : x * 4 + 3]
                if tuple(px) == rgb:
                    n += 1
        return n

    # Геометрия страницы: <img> 60x40 сверху, затем <iframe> 200x100.
    page_img = count(0, 40, PAGE_RGB)
    frame_red = count(40, 140, CHILD_RGB)
    frame_blue = count(40, 140, PAGE_RGB)

    # Гистограмма полосы фрейма: отличает «серая заглушка» (фрейм не вклеен
    # вовсе) от «содержимое вклеено, но картинка в нём — заглушка».
    band = Counter()
    for y in range(40, min(h, 140)):
        row = rows[y]
        for x in range(0, 200):
            band[tuple(row[x * 4 : x * 4 + 3])] += 1
    print("--- цвета полосы фрейма (топ-4) ---")
    for rgb, n in band.most_common(4):
        print(f"  {rgb}: {n}")

    print(f"[{'OK ' if page_img > 2000 else 'ФЕЙЛ'}] картинка СТРАНИЦЫ нарисована: {page_img} px синего (контроль пути)")
    ok &= page_img > 2000
    print(f"[{'OK ' if frame_red > 15000 else 'ФЕЙЛ'}] картинка ФРЕЙМА нарисована: {frame_red} px красного из 20000")
    ok &= frame_red > 15000
    print(f"[{'OK ' if frame_blue == 0 else 'ФЕЙЛ'}] во фрейме нет пикселей картинки страницы (общий ключ src): {frame_blue} px синего")
    ok &= frame_blue == 0

    srv.shutdown()
    if not args.keep:
        shutil.rmtree(root, ignore_errors=True)
    else:
        print(f"каталог пробы: {root}")
    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
