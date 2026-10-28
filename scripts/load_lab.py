"""Стенд LOAD-*: локальный сервер с задержками подресурсов.

HTML отдаётся сразу, CSS — через CSS_MS, картинки и iframe — через IMG_MS (по умолчанию
700 мс / 3000 мс). Страница сама шлёт метки (`/mark?<имя>`): `dcl`, `raf` (первый кадр
после разбора), `load`; сервер пишет момент каждого запроса относительно запроса HTML.

    python scripts/load_lab.py --port 8765           # слушать, печатать журнал по Ctrl+C
    python scripts/load_lab.py --port 8765 --exit-on load --timeout 40 --json out.json

Откройте `http://127.0.0.1:8765/` в Lumen (`--maximized`, живое окно). Время «финального
кадра после HTML» — разность `raf` и `html` в журнале, `load` — когда пришло событие `load`.
Образец — `.tmp/seqlab` (BUG-1116, docs/perf/journal.md).
"""
import argparse
import base64
import json
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

sys.stdout.reconfigure(encoding="utf-8")

# PNG-плитки 64×64 разных цветов: достаточно, чтобы картинки были разными URL и реально декодировались.
IMG_COUNT = 12


def make_png(r: int, g: int, b: int, size: int = 64) -> bytes:
    import struct
    import zlib

    def chunk(kind: bytes, data: bytes) -> bytes:
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    raw = b"".join(b"\x00" + bytes([r, g, b]) * size for _ in range(size))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


def page(img_ms: int) -> str:
    imgs = "\n".join(
        f'<img src="/img/{i}.png" width="64" height="64" alt="">' for i in range(IMG_COUNT)
    )
    return f"""<!doctype html>
<html><head><meta charset="utf-8"><title>load-lab</title>
<link rel="stylesheet" href="/style.css">
<script>
function mark(n) {{ try {{ new Image().src = '/mark?' + n + '&t=' + Math.random(); }} catch (e) {{}} }}
document.addEventListener('DOMContentLoaded', function () {{ mark('dcl'); requestAnimationFrame(function () {{ mark('raf'); }}); }});
window.addEventListener('load', function () {{ mark('load'); document.title = 'loaded'; }});
</script></head>
<body><h1>load-lab</h1><p>HTML сразу, CSS +700 мс, картинки и iframe +{img_ms} мс.</p>
{imgs}
<iframe src="/frame.html" width="300" height="120"></iframe>
<p id="end">конец страницы</p></body></html>"""


class Lab(BaseHTTPRequestHandler):
    server_version = "load-lab"

    def log_message(self, *a):  # тихо
        pass

    def _send(self, code, body: bytes, ctype: str):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_GET(self):
        st = self.server.state
        path = urlparse(self.path).path
        now = time.monotonic()
        if path in ("/", "/index.html"):
            with st["lock"]:
                st["t0"] = now
                st["events"].clear()
            st["events"].append(("html", 0.0))
            return self._send(200, page(st["img_ms"]).encode("utf-8"), "text/html; charset=utf-8")
        t = now - st["t0"] if st["t0"] else 0.0
        if path == "/mark":
            name = urlparse(self.path).query.split("&")[0]
            st["events"].append((name, round(t, 3)))
            if name == st["exit_on"]:
                st["done"].set()
            return self._send(200, base64.b64decode("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7"), "image/gif")
        if path == "/style.css":
            time.sleep(st["css_ms"] / 1000)
            st["events"].append(("css", round(time.monotonic() - st["t0"], 3)))
            return self._send(200, b"body{font:16px sans-serif;margin:16px}img{margin:2px;background:#ddd}", "text/css")
        if path.startswith("/img/") and path.endswith(".png"):
            time.sleep(st["img_ms"] / 1000)
            i = int(path[5:-4] or 0)
            st["events"].append((f"img{i}", round(time.monotonic() - st["t0"], 3)))
            return self._send(200, make_png((i * 40) % 256, (i * 90) % 256, 160), "image/png")
        if path == "/frame.html":
            time.sleep(st["frame_ms"] / 1000)
            st["events"].append(("frame", round(time.monotonic() - st["t0"], 3)))
            return self._send(200, "<html><body><p>iframe</p></body></html>".encode("utf-8"), "text/html; charset=utf-8")
        self._send(404, b"not found", "text/plain")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8765)
    ap.add_argument("--css-ms", type=int, default=700)
    ap.add_argument("--img-ms", type=int, default=3000)
    ap.add_argument("--frame-ms", type=int, default=-1, help="задержка iframe; по умолчанию как --img-ms, 0 — изолировать картинки")
    ap.add_argument("--exit-on", default="", help="метка, после которой сервер завершается (например load)")
    ap.add_argument("--timeout", type=float, default=0, help="секунд до принудительного выхода")
    ap.add_argument("--json", default="", help="куда записать журнал меток")
    args = ap.parse_args()
    srv = ThreadingHTTPServer(("127.0.0.1", args.port), Lab)
    srv.state = {
        "t0": 0.0, "events": [], "lock": threading.Lock(), "done": threading.Event(),
        "css_ms": args.css_ms, "img_ms": args.img_ms,
        "frame_ms": args.img_ms if args.frame_ms < 0 else args.frame_ms, "exit_on": args.exit_on,
    }
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    print(f"load-lab: http://127.0.0.1:{args.port}/ (css +{args.css_ms} мс, img/iframe +{args.img_ms} мс)", flush=True)
    try:
        srv.state["done"].wait(args.timeout or None)
        if args.exit_on:
            time.sleep(0.5)
    except KeyboardInterrupt:
        pass
    ev = sorted(srv.state["events"], key=lambda e: e[1])
    for name, t in ev:
        print(f"{t:7.3f}s  {name}")
    if args.json:
        with open(args.json, "w", encoding="utf-8") as f:
            json.dump(ev, f, ensure_ascii=False)


if __name__ == "__main__":
    main()
