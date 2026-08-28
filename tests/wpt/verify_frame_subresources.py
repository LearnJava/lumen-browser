#!/usr/bin/env python3
"""BUG-480 срез 11 — живая проверка подресурсов парсерных элементов фрейма.

Под-документ ``<iframe src>`` содержит парсерные ``<link rel=stylesheet>``
(есть/404), ``<img src>`` (есть/404) и ``<img loading="lazy">``. Проверяется
записью запросов на стороне http-сервера пробы:

* листы и картинки ребёнка запрашиваются (включая 404 — запрос обязан уйти,
  исход доставки различает load/error);
* lazy-картинка НЕ запрашивается вовсе (прокси вьюпорта у фреймов отсутствует);
* события приходят элементам: инлайн-скрипт ребёнка вешает слушатели до
  доставки (исходы доставляются после DCL и до window load) и рапортует
  словарь исходов родителю, как только обработчик ``load`` хоста внедрит
  мостик ``__rep`` (инлайн-скрипты ребёнка исполняются до регистрации
  предков — известное ограничение срезов 1–3);
* исходы успевают ДО window load ребёнка (спека: load документа следует
  за его подресурсами).

Запуск: ``python tests/wpt/verify_frame_subresources.py [--binary PATH]``
"""

from __future__ import annotations

import argparse
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

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
HERE = os.path.dirname(os.path.abspath(__file__))

PARENT_PAGE = """<!doctype html><meta charset="utf-8"><title>vfsrc parent</title>
<body>parent
<iframe src="/.vfsrc-child.html"></iframe>
<script>
console.log('PROBE parent-start');
window.addEventListener('message', function (ev) {
  console.log('PROBE parent-got ' + JSON.stringify(ev.data));
});
var frame = document.querySelector('iframe');
frame.addEventListener('load', function () {
  console.log('PROBE parent-load');
  var d = frame.contentDocument;
  var boot = d.createElement('script');
  boot.textContent =
    "window.__rep = function(x){try{window.parent.postMessage({out: x}, '*');}catch(e){}};";
  d.body.appendChild(boot);
  console.log('PROBE parent-booted');
});
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfsrc child</title>
<link rel="stylesheet" href="/.vfsrc-ok.css">
<link rel="stylesheet" href="/.vfsrc-bad.css">
<body>child
<img id="iok" src="/.vfsrc-ok.png">
<img id="ibad" src="/.vfsrc-bad.png">
<img loading="lazy" src="/.vfsrc-lazy.png">
<script>
window.__out = {};
function hook(el, key) {
  if (!el) { window.__out[key] = 'no-element'; return; }
  el.addEventListener('load', function () { window.__out[key] = 'load'; });
  el.addEventListener('error', function () { window.__out[key] = 'error'; });
}
var links = document.querySelectorAll('link[rel=stylesheet]');
hook(links[0], 'linkOk');
hook(links[1], 'linkBad');
hook(document.getElementById('iok'), 'imgOk');
hook(document.getElementById('ibad'), 'imgBad');
window.addEventListener('load', function () {
  window.__out.linkBeforeWindowLoad = !!(window.__out.linkOk || window.__out.linkBad);
  window.__out.imgBeforeWindowLoad = !!(window.__out.imgOk || window.__out.imgBad);
  window.__out.childWindowLoad = true;
  (function poll() {
    if (typeof window.__rep === 'function') { window.__rep(window.__out); return; }
    setTimeout(poll, 100);
  })();
});
</script>
"""

def write_solid_png(w: int, h: int, rgb: tuple) -> bytes:
    """Минимальный однотонный PNG — тот же генератор, что в `verify_frame_images.py`."""
    def chunk(tag: bytes, data: bytes) -> bytes:
        return (struct.pack(">I", len(data)) + tag + data
                + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF))

    raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw))
            + chunk(b"IEND", b""))


OK_CSS = "p { color: red; }\n"
# BUG-480 срез 16: до среза 15 здесь лежала строка «not-a-real-png», и её
# хватало — исход `<img>` означал «байты прочитались». Срез 15 стал
# ДЕКОДИРОВАТЬ картинки под-документа, поэтому нераспознанный файл теперь
# честно кончается `error`, и проба мерила бы не то, что называет.
OK_PNG = write_solid_png(8, 8, (0, 128, 0))


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


def _serve() -> tuple[int, http.server.ThreadingHTTPServer, list[str]]:
    seen: list[str] = []
    port = _free_port()

    class _Tracking(_Quiet):
        def do_GET(self):
            seen.append(self.path)
            super().do_GET()

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Tracking)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server, seen


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        default=os.path.join(REPO, "target", "dev-release", "lumen.exe"),
    )
    parser.add_argument("--seconds", type=float, default=8.0)
    args = parser.parse_args()

    files = [
        (".vfsrc-parent.html", PARENT_PAGE),
        (".vfsrc-child.html", CHILD_PAGE),
        (".vfsrc-ok.css", OK_CSS),
        (".vfsrc-ok.png", OK_PNG),
    ]
    for name, body in files:
        mode, kwargs = ("wb", {}) if isinstance(body, bytes) else ("w", {"encoding": "utf-8"})
        with open(os.path.join(HERE, name), mode, **kwargs) as handle:
            handle.write(body)

    port, server, requests_seen = _serve()
    log_path = os.path.join(REPO, ".tmp", "vfsrc-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    print(f"http://127.0.0.1:{port}/.vfsrc-parent.html -> {log_path}")
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{port}/.vfsrc-parent.html"],
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
        content = handle.read()
    markers = re.findall(r"PROBE ([^\n\r]+)", content)
    out: dict = {}
    for marker in markers:
        if marker.startswith("parent-got "):
            try:
                payload = json.loads(marker[len("parent-got "):])
            except json.JSONDecodeError:
                continue
            if isinstance(payload.get("out"), dict):
                out = payload["out"]

    ok = True

    def expect_request(suffix: str, label: str) -> None:
        nonlocal ok
        hit = any(r.endswith(suffix) for r in requests_seen)
        status = "ok" if hit else "MISSING"
        if not hit:
            ok = False
        print(f"[{status}] {label}")

    def reject_request(suffix: str, label: str) -> None:
        nonlocal ok
        hit = any(r.endswith(suffix) for r in requests_seen)
        status = "ok (absent)" if not hit else "UNEXPECTED"
        if hit:
            ok = False
        print(f"[{status}] {label}")

    def expect_out(key: str, expected, label: str | None = None) -> None:
        nonlocal ok
        got = out.get(key)
        good = got == expected and key in out
        status = "ok" if good else f"MISSING (got {got!r})"
        if not good:
            ok = False
        print(f"[{status}] {label or key}")

    expect_request(".vfsrc-ok.css", "server saw child stylesheet request")
    expect_request(".vfsrc-bad.css", "server saw 404 stylesheet request (outcome error)")
    expect_request(".vfsrc-ok.png", "server saw child img request")
    expect_request(".vfsrc-bad.png", "server saw 404 img request (outcome error)")
    reject_request(".vfsrc-lazy.png", "lazy img of frame is not requested at all")

    expect_out("linkOk", "load", "child link[ok] fired load on the element")
    expect_out("linkBad", "error", "child link[bad] fired error on the element")
    expect_out("imgOk", "load", "child img[ok] fired load on the element")
    expect_out("imgBad", "error", "child img[bad] fired error on the element")
    expect_out("linkBeforeWindowLoad", True, "link outcomes arrived before window load")
    expect_out("imgBeforeWindowLoad", True, "img outcomes arrived before window load")
    expect_out("childWindowLoad", True, "child window load fired")

    print("markers:", *markers, sep="\n  ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
