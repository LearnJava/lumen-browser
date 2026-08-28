#!/usr/bin/env python3
"""BUG-480 срез 16: НАСТОЯЩИЙ клик мышью внутрь фрейма.

Чем отличается от `verify_frame_click.py` (срез 6): там `click()` вызывал
РОДИТЕЛЬСКИЙ скрипт через фасад `contentDocument`, то есть событие рождалось
уже внутри ребёнка. Здесь клик приходит СНАРУЖИ — из шелла, по координате
окна, как от пользователя, — и должен быть переведён в координаты
под-документа и разослан слушателям ребёнка.

Как меряется:

* координата задаётся точкой (`{point: {x, y}}` MCP-инструмента `click`) в
  координатах страницы, а не селектором: селектор ищется в дереве СТРАНИЦЫ,
  и элемент внутри фрейма ему не виден по определению;
* контроль на той же странице — кнопка родителя. Если она молчит, проба
  меряет собственную арифметику координат, а не движок;
* фрейм абсолютно спозиционирован, ребёнок — с `margin:0`, чтобы точка
  считалась арифметикой, а не измерением;
* доверенность (`isTrusted`) отличает нативный клик от синтетического: тот,
  что делает `element.click()` среза 6, недоверенный.

Запуск: python tests/wpt/verify_frame_hit_test.py --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
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

# Геометрия: фрейм в (FRAME_X, FRAME_Y) размером FRAME_W×FRAME_H, кнопка
# ребёнка занимает его левый верхний угол BTN_W×BTN_H. Клик в
# (FRAME_X + 60, FRAME_Y + 30) обязан попасть в кнопку ребёнка.
FRAME_X, FRAME_Y, FRAME_W, FRAME_H = 40, 120, 300, 200
BTN_W, BTN_H = 200, 100
# Смещение внука внутри среднего фрейма (вариант nested).
MID_X, MID_Y = 20, 30

PARENT_PAGE = f"""<!doctype html><meta charset="utf-8"><title>vfht parent</title>
<body style="margin:0">
<button id="pbtn" style="position:absolute;left:0;top:0;width:200px;height:60px">parent</button>
<iframe src="/.vfht-child.html" style="position:absolute;left:{FRAME_X}px;top:{FRAME_Y}px;
        width:{FRAME_W}px;height:{FRAME_H}px;border:0"></iframe>
<script>
console.log('PROBE parent-start');
document.getElementById('pbtn').addEventListener('click', function (ev) {{
  console.log('PROBE parent-btn-click trusted=' + ev.isTrusted);
}});
document.querySelector('iframe').addEventListener('click', function (ev) {{
  console.log('PROBE parent-iframe-click trusted=' + ev.isTrusted);
}});
window.addEventListener('message', function (ev) {{
  console.log('PROBE parent-got ' + JSON.stringify(ev.data));
}});
</script>
</body>
"""

CHILD_PAGE = f"""<!doctype html><meta charset="utf-8"><title>vfht child</title>
<body style="margin:0;background:#eee">
<button id="btn" style="position:absolute;left:0;top:0;width:{BTN_W}px;height:{BTN_H}px">go</button>
<script>
console.log('PROBE child-start');
document.getElementById('btn').addEventListener('click', function (ev) {{
  console.log('PROBE child-btn-click trusted=' + ev.isTrusted
              + ' tag=' + (ev.target ? ev.target.tagName : 'null')
              + ' x=' + ev.clientX + ' y=' + ev.clientY);
  window.parent.postMessage({{ childClicked: true }}, '*');
}});
document.addEventListener('click', function (ev) {{
  console.log('PROBE child-doc-click');
}});
</script>
</body>
"""

# Вариант nested: тот же клик, но по кнопке ВНУКА (глубина 1). Спуск должен
# сложить смещения обоих хостов — ошибка на одном уровне даёт либо промах, либо
# попадание с координатами чужого документа.
MID_PAGE = f"""<!doctype html><meta charset="utf-8"><title>vfht mid</title>
<body style="margin:0;background:#ddd">
<iframe src="/.vfht-child.html" style="position:absolute;left:{MID_X}px;top:{MID_Y}px;
        width:200px;height:150px;border:0"></iframe>
<script>
console.log('PROBE mid-start');
window.addEventListener('message', function (ev) {{
  // Внук отвечает своему родителю — среднему фрейму; тот зеркалит выше.
  window.parent.postMessage(ev.data, '*');
}});
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


def _serve() -> tuple[int, http.server.ThreadingHTTPServer]:
    port = _free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Quiet)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        default=os.path.join(REPO, "target", "dev-release", "lumen.exe"),
    )
    parser.add_argument("--variant", choices=["flat", "nested"], default="flat")
    args = parser.parse_args()

    nested = args.variant == "nested"
    # В варианте nested страница держит СРЕДНИЙ фрейм, а кнопка живёт во внуке:
    # то же место на экране, но два уровня спуска вместо одного.
    parent_page = PARENT_PAGE.replace("/.vfht-child.html", "/.vfht-mid.html") if nested \
        else PARENT_PAGE
    click_x = FRAME_X + (MID_X if nested else 0) + 60
    click_y = FRAME_Y + (MID_Y if nested else 0) + 30

    for name, body in [(".vfht-parent.html", parent_page),
                       (".vfht-mid.html", MID_PAGE),
                       (".vfht-child.html", CHILD_PAGE)]:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port, server = _serve()
    mcp_port = _free_port()
    log_path = os.path.join(REPO, ".tmp", "vfht-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.vfht-parent.html"
    print(f"{url} -> {log_path}")

    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(mcp_port), url],
            stdout=subprocess.DEVNULL, stderr=log, text=True, cwd=HERE,
        )
        try:
            client = Client(mcp_port, log_path)
            client.call("wait", {"condition": "document_ready", "timeout_ms": 10000})
            time.sleep(1.0)
            # Контроль: кнопка родителя. Доказывает, что арифметика координат
            # и сам путь клика работают на этой сборке.
            client.call("click", {"target": {"point": {"x": 100, "y": 30}}})
            time.sleep(0.5)
            # Субъект: точка внутри фрейма, попадающая в кнопку ребёнка.
            client.call("click", {"target": {"point": {"x": click_x, "y": click_y}}})
            time.sleep(1.5)
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

    def expect(substr: str) -> None:
        nonlocal ok
        hit = any(substr in m for m in markers)
        if not hit:
            ok = False
        print(f"[{'ok' if hit else 'MISSING'}] {substr}")

    def forbid(substr: str, why: str) -> None:
        nonlocal ok
        hit = any(substr in m for m in markers)
        if hit:
            ok = False
        print(f"[{'ok' if not hit else 'UNEXPECTED'}] нет «{substr}» — {why}")

    expect("parent-start")
    if nested:
        expect("mid-start")
    expect("child-start")
    # Контроль: нативный клик по кнопке родителя доверенный.
    expect("parent-btn-click trusted=true")
    # Субъект: тот же нативный клик, но по кнопке РЕБЁНКА. Координаты события —
    # в системе координат ребёнка (60, 30), а не страницы.
    expect("child-btn-click trusted=true tag=BUTTON x=60 y=30")
    # Всплытие внутри под-документа доходит до его собственного document.
    expect("child-doc-click")
    # Ребёнок смог ответить родителю из обработчика.
    expect('parent-got {"childClicked":true}')
    # Второй дефект, которого не было в постановке: событие внутри вложенного
    # browsing context родительскому документу НЕ принадлежит, а до среза
    # родитель получал `click` прямо на элементе `<iframe>`.
    forbid("parent-iframe-click", "клик внутрь фрейма не событие родителя")

    print("markers:", *markers, sep="\n  ")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
