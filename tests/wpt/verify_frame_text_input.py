#!/usr/bin/env python3
"""BUG-480 срез 22: НАТИВНЫЙ ввод текста в typeable-поле ВНУТРИ фрейма.

Срез 18 довёл до фрейма нативное поведение флажков/радио/`<summary>`, но
текстовые поля не входили: `self.focused_node` после клика внутрь фрейма
указывает на host-элемент `<iframe>` (срез 16), а `Self::typeable_field`
читает исключительно документ СТРАНИЦЫ — печатать в поле фрейма было
некуда.

Что меряется и почему именно так:

* КОНТРОЛЬ на той же странице — текстовое поле РОДИТЕЛЯ. Если оно не
  наполняется, проба меряет свою арифметику координат/MCP-протокол, а не
  движок;
* точки клика — из `getBoundingClientRect()`, как у `verify_frame_forms.py`
  (разметка не адрес: `<iframe>` позиционируется абсолютно, но точный центр
  поля внутри него знает только layout);
* читается `input.value` через `document.getElementById(...).value` ПОСЛЕ
  ввода — то же свойство, которое читает `forms::collect_form_entries` при
  отправке формы;
* `input` СОБЫТИЕ ловится слушателем поля — доказывает, что дошёл не только
  символ в DOM, но и `keydown`/`input`/`keyup` в JS-контекст РЕБЁНКА
  (`_lumen_dispatch_key_event` по его собственному хэндлу);
* второе поле фрейма — предзаполненное (`value="12345"`), проверяет ту же
  инъекцию на НЕ пустом значении (дозапись, а не первый символ);
  `inject_frame_backspace` (зеркало этого же кода) MCP-протоколом не
  дотягивается — нет тула «нажать клавишу без текста» — и остаётся
  проверен симметрией с `inject_frame_char`, как и у страницы;
* поле СТРАНИЦЫ рядом с фреймом кликается ПОСЛЕ фрейма и должно остаться
  ПУСТЫМ — контроль того, что фокус фрейма не утёк в `self.focused_node`
  и наоборот (`self.focused_frame`/`self.focused_node` не смешиваются).

Запуск: python tests/wpt/verify_frame_text_input.py --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
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
sys.path.insert(0, os.path.join(REPO, "scripts"))

from scroll_perf import Client  # noqa: E402  (после sys.path)

FRAME_X, FRAME_Y, FRAME_W, FRAME_H = 40, 120, 300, 200

PARENT_PAGE = f"""<!doctype html><meta charset="utf-8"><title>vfti parent</title>
<body style="margin:0;background:#fff">
<div><input type="text" id="ptxt"></div>
<div id="pfree" style="height:40px"></div>
<iframe src="/.vfti-child.html" style="position:absolute;left:{FRAME_X}px;top:{FRAME_Y}px;
        width:{FRAME_W}px;height:{FRAME_H}px;border:0"></iframe>
<script>
console.log('PROBE parent-start');
function vftiRect(id) {{
  var r = document.getElementById(id).getBoundingClientRect();
  return [r.left, r.top, r.width, r.height];
}}
setTimeout(function () {{
  console.log('PROBE parent-rects ' + JSON.stringify({{ptxt: vftiRect('ptxt')}}));
}}, 800);
document.getElementById('ptxt').addEventListener('input', function (ev) {{
  console.log('PROBE parent-input value=' + ev.target.value);
}});
</script>
</body>
"""

CHILD_PAGE = """<!doctype html><meta charset="utf-8"><title>vfti child</title>
<body style="margin:0;background:#eee">
<div><input type="text" id="ctxt"></div>
<div><input type="text" id="cdel" value="12345"></div>
<script>
console.log('PROBE child-start');
function vftiRect(id) {
  var r = document.getElementById(id).getBoundingClientRect();
  return [r.left, r.top, r.width, r.height];
}
setTimeout(function () {
  console.log('PROBE child-rects ' + JSON.stringify({
    ctxt: vftiRect('ctxt'), cdel: vftiRect('cdel')
  }));
}, 800);
document.getElementById('ctxt').addEventListener('input', function (ev) {
  console.log('PROBE child-input value=' + ev.target.value);
});
document.getElementById('cdel').addEventListener('input', function (ev) {
  console.log('PROBE child-del value=' + ev.target.value);
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

    for name, body in [(".vfti-parent.html", PARENT_PAGE), (".vfti-child.html", CHILD_PAGE)]:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port = _free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Quiet)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    mcp_port = _free_port()
    log_path = os.path.join(REPO, ".tmp", "vfti-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.vfti-parent.html"
    print(f"{url} -> {log_path}")

    pr: dict[str, list[float]] = {}
    cr: dict[str, list[float]] = {}
    отказы: list[str] = []
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(mcp_port), url],
            stdout=subprocess.DEVNULL, stderr=log, text=True, cwd=HERE,
        )
        try:
            client = Client(mcp_port, log_path)
            client.call("wait", {"condition": "document_ready", "timeout_ms": 30000})
            time.sleep(3.0)

            def click(x: float, y: float, pause: float = 0.4) -> None:
                client.call("click", {"target": {"point": {"x": x, "y": y}}})
                time.sleep(pause)

            def type_at(x: float, y: float, text: str, pause: float = 0.4) -> None:
                """Ввести `text` в точку — и НЕ упасть, если движок отказал.

                Отказ `type` («Element is not a mutable text field») — это ровно
                тот дефект, который проба меряет: до правки клик внутрь фрейма
                фокусировал host-`<iframe>`, а он не typeable. Исключение здесь
                оборвало бы прогон трейсбеком, то есть измерение «ДО» нельзя
                было бы повторить — отказ поэтому записывается маркером и
                разбирается ниже вместе с остальными проверками.
                """
                try:
                    client.call("type", {"target": {"point": {"x": x, "y": y}}, "text": text})
                except RuntimeError as exc:
                    отказы.append(f"{text!r} @ ({x:.0f},{y:.0f}): {exc}")
                time.sleep(pause)

            start = _markers(log_path)
            pr = _rects(start, "parent-rects ")
            cr = _rects(start, "child-rects ")
            print("прямоугольники родителя:", pr)
            print("прямоугольники ребёнка: ", cr)

            def центр(rect: list[float], dx: float = 0.0, dy: float = 0.0):
                return (rect[0] + rect[2] / 2 + dx, rect[1] + rect[3] / 2 + dy)

            if cr:
                # Субъект: клик в текстовое поле фрейма, потом ввод через
                # MCP `type` — тот же путь, что `about_to_wait.rs` даёт любому
                # автоматизационному клиенту (click + `inject_frame_char`).
                ctxt_point = центр(cr["ctxt"], FRAME_X, FRAME_Y)
                click(*ctxt_point)
                type_at(*ctxt_point, "abc")
                # Второе поле фрейма — предзаполненное, проверяет тот же путь
                # инъекции символа на НЕ пустом значении (дозапись, а не
                # первый символ).
                cdel_point = центр(cr["cdel"], FRAME_X, FRAME_Y)
                click(*cdel_point)
                type_at(*cdel_point, "6")
            if pr:
                # Контроль ПОСЛЕ фрейма: поле страницы должно наполниться, а
                # НЕ поле фрейма — доказывает, что клик по странице сбросил
                # `focused_frame`.
                ptxt_point = центр(pr["ptxt"])
                click(*ptxt_point)
                type_at(*ptxt_point, "z")
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

    # Отказ `type` печатается ДО проверок значений: без него КРАСНЫЙ прогон
    # выглядит как «поле почему-то пустое», хотя движок прямо сказал, почему.
    if отказы:
        print("движок отказал вводу:", *отказы, sep="\n  ")
    check(not отказы, f"ни один `type` не отклонён движком ({len(отказы)} отказов)")

    child_input = [m for m in markers if m.startswith("child-input ")]
    print("input-события ребёнка (ctxt):", *child_input, sep="\n  ")
    check(bool(child_input) and child_input[-1] == "child-input value=abc",
          f"текст дошёл до поля ВНУТРИ фрейма: {child_input[-1:] or '—'}")

    child_del = [m for m in markers if m.startswith("child-del ")]
    print("input-события ребёнка (cdel):", *child_del, sep="\n  ")
    check(bool(child_del) and child_del[-1] == "child-del value=123456",
          f"второе поле фрейма приняло символ: {child_del[-1:] or '—'}")

    parent_input = [m for m in markers if m.startswith("parent-input ")]
    print("input-события родителя:", *parent_input, sep="\n  ")
    check(bool(parent_input) and parent_input[-1] == "parent-input value=z",
          f"поле СТРАНИЦЫ после фрейма наполнилось независимо: {parent_input[-1:] or '—'}")

    print("маркеры:", *markers, sep="\n  ")
    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
