#!/usr/bin/env python3
"""BUG-480 срез 23: ВИДИМЫЙ `:focus` внутри фрейма.

Срез 22 довёл до поля внутри фрейма сам ввод текста, но записал в очередь:
«видимого `:focus` внутри фрейма это не даёт и не претендует —
`frames::layout_frame_document` не зовёт `set_interactive_state` вовсе, так
что для CSS ребёнок остаётся интерактивно-слепым, ровно как и для `:hover`».
Проба измеряет именно это.

Что меряется и почему именно так:

* **Субъект — ГЕОМЕТРИЯ, а не только цвет.** Правило `:focus` меняет
  `width` поля (100px → 200px), и проба читает `getBoundingClientRect()`.
  Ожидания — 102/202, а не 100/200: `width` задаёт КОНТЕНТНЫЙ бокс, а
  `getBoundingClientRect` отдаёт border-бокс, и UA-рамка `<input>` шириной
  1px с каждой стороны добавляет свои два пикселя. Числа взяты из измерения
  КОНТРОЛЯ на странице, а не выведены из разметки.
  Ширина доказывает, что под-документ реально ПЕРЕСЧИТАН с сматчившимся
  `:focus` — то есть изменилось то самое дерево, из которого собирается его
  display list. Один лишь `getComputedStyle` этого не доказывает: он читает
  ОТДЕЛЬНЫЙ снимок, публикуемый шеллом (`update_computed_styles`), и мог бы
  позеленеть при неподвинувшихся пикселях;
* **и всё-таки `getComputedStyle` тоже** — вторым, независимым каналом:
  `layout_frame_document` публикует ребёнку только `update_layout_rects`/
  `update_viewport_size`, поэтому `getComputedStyle` внутри фрейма
  подозревается пустым НЕЗАВИСИМО от `:focus`. Чтобы не спутать «нет
  `:focus`» с «нет вычисленных стилей вообще», рядом стоит СТАТИЧЕСКИЙ
  контроль `#cplain` с `background:rgb(1,2,3)` — цвет, который не зависит ни
  от какого интерактивного состояния;
* **контроль на СТРАНИЦЕ** — поле родителя с теми же двумя правилами. Если
  у него `:focus` не срабатывает, проба меряет свою арифметику координат или
  MCP-протокол, а не движок;
* **базовая линия ДО кликов** — оба поля должны быть 102px. Это доказывает,
  что таблица стилей ребёнка вообще применяется (иначе «не 202» ничего не
  значило бы: ширина могла быть не 102 по совсем другой причине);
* **порядок кликов — страница, потом фрейм.** Последним кликом фокус уходит
  в ребёнка, и проба проверяет, что поле СТРАНИЦЫ вернулось к 102px: два
  фокуса не должны сосуществовать (`focused_node` и `focused_frame` — разные
  поля, срез 22);
* точки клика — из `getBoundingClientRect()`, как у `verify_frame_forms.py`
  и `verify_frame_text_input.py`: разметка не адрес, точный центр поля
  внутри абсолютно спозиционированного `<iframe>` знает только layout.

Чего проба НЕ меряет и почему:

* `:hover`/`:active` — их нельзя вызвать ни одной автоматизационной
  поверхностью: `InputCommand::Click` зовёт `handle_click_at` напрямую и
  кнопку не нажимает, движения мыши в протоколе нет вовсе;
* **каретку** — её нет и у СТРАНИЦЫ: `caret_color` разбирается, каскадируется
  и наследуется, но не читается ни одним потребителем в `lumen-paint`
  (`grep caret_color` — только `style/*`), а единственная каретка в движке
  нарисована руками для омнибокса хрома (`chrome_ui.rs`, «no native caret
  exists for `<input>` yet»). Это дефект СТРАНИЦЫ, а не фрейма, и по правилу
  этих срезов («работает во фрейме, но не на странице» — единственное
  расхождение, которого они не допускают) он идёт отдельным баг-номером.

Запуск: python tests/wpt/verify_frame_focus_style.py --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
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

FRAME_X, FRAME_Y, FRAME_W, FRAME_H = 40, 140, 320, 200

# Одна и та же пара правил у обеих сторон: ширина — субъект (доказывает
# пересчёт layout), фон — второй канал (доказывает публикацию вычисленных
# стилей). `#…plain` от интерактивного состояния не зависит вовсе.
STYLE = """
<style>
  input { width: 100px; background: rgb(250, 250, 250); }
  input:focus { width: 200px; background: rgb(0, 128, 0); }
  .plain { width: 60px; height: 20px; background: rgb(1, 2, 3); }
</style>
"""

# Отчёт обеих сторон одинаков — печатается по таймеру, чтобы каждое состояние
# (до кликов, после клика в страницу, после клика во фрейм) было видно как
# отдельная порция маркеров.
REPORT_JS = """
function vffsSnap(side, inputId, plainId) {
  var el = document.getElementById(inputId);
  var pl = document.getElementById(plainId);
  var r = el.getBoundingClientRect();
  var cs = window.getComputedStyle ? window.getComputedStyle(el) : null;
  var cp = window.getComputedStyle ? window.getComputedStyle(pl) : null;
  var ae = document.activeElement;
  console.log('PROBE ' + side + '-snap ' + JSON.stringify({
    w: Math.round(r.width),
    bg: cs ? String(cs.backgroundColor) : '<no getComputedStyle>',
    plain: cp ? String(cp.backgroundColor) : '<no getComputedStyle>',
    active: ae ? String(ae.id || ae.tagName) : '<none>'
  }));
}
"""

PARENT_PAGE = f"""<!doctype html><meta charset="utf-8"><title>vffs parent</title>
{STYLE}
<body style="margin:0;background:#fff">
<div><input type="text" id="ptxt"></div>
<div class="plain" id="pplain"></div>
<iframe src="/.vffs-child.html" style="position:absolute;left:{FRAME_X}px;top:{FRAME_Y}px;
        width:{FRAME_W}px;height:{FRAME_H}px;border:0"></iframe>
<script>
console.log('PROBE parent-start');
{REPORT_JS}
setTimeout(function () {{
  var r = document.getElementById('ptxt').getBoundingClientRect();
  console.log('PROBE parent-rect ' + JSON.stringify([r.left, r.top, r.width, r.height]));
}}, 800);
setInterval(function () {{ vffsSnap('parent', 'ptxt', 'pplain'); }}, 400);
</script>
</body>
"""

CHILD_PAGE = f"""<!doctype html><meta charset="utf-8"><title>vffs child</title>
{STYLE}
<body style="margin:0;background:#eee">
<div><input type="text" id="ctxt"></div>
<div class="plain" id="cplain"></div>
<script>
console.log('PROBE child-start');
{REPORT_JS}
setTimeout(function () {{
  var r = document.getElementById('ctxt').getBoundingClientRect();
  console.log('PROBE child-rect ' + JSON.stringify([r.left, r.top, r.width, r.height]));
}}, 800);
setInterval(function () {{ vffsSnap('child', 'ctxt', 'cplain'); }}, 400);
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


def _rect(markers: list[str], prefix: str) -> list[float]:
    for m in markers:
        if m.startswith(prefix):
            return json.loads(m[len(prefix):])
    return []


def _last_snap(markers: list[str], side: str, since: int) -> dict:
    """Последний снимок стороны `side` среди маркеров ПОСЛЕ индекса `since`.

    Смотреть надо именно на последний: клик и перерисовка асинхронны, первый
    же тик таймера после клика может успеть напечататься ещё до relayout.
    """
    prefix = f"{side}-snap "
    for m in reversed(markers[since:]):
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

    for name, body in [(".vffs-parent.html", PARENT_PAGE), (".vffs-child.html", CHILD_PAGE)]:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    port = _free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), _Quiet)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    mcp_port = _free_port()
    log_path = os.path.join(REPO, ".tmp", "vffs-smoke.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{port}/.vffs-parent.html"
    print(f"{url} -> {log_path}")

    base_parent: dict = {}
    base_child: dict = {}
    focus_parent: dict = {}
    frame_parent: dict = {}
    frame_child: dict = {}
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

            def click(x: float, y: float, pause: float = 1.2) -> None:
                """Кликнуть — и НЕ упасть, если движок отказал.

                Отказ обязан быть виден отдельной строкой `[ФЕЙЛ]` с текстом
                самого движка: исключение оборвало бы прогон трейсбеком, то
                есть измерение «ДО» нельзя было бы ни повторить, ни прочитать
                (урок среза 22).
                """
                try:
                    client.call("click", {"target": {"point": {"x": x, "y": y}}})
                except RuntimeError as exc:
                    отказы.append(f"клик ({x:.0f},{y:.0f}): {exc}")
                time.sleep(pause)

            start = _markers(log_path)
            pr = _rect(start, "parent-rect ")
            cr = _rect(start, "child-rect ")
            print("прямоугольник поля родителя:", pr)
            print("прямоугольник поля ребёнка: ", cr)

            # Базовая линия: ничего не сфокусировано.
            base_parent = _last_snap(start, "parent", 0)
            base_child = _last_snap(start, "child", 0)

            def центр(rect: list[float], dx: float = 0.0, dy: float = 0.0):
                return (rect[0] + rect[2] / 2 + dx, rect[1] + rect[3] / 2 + dy)

            # Фаза B — контроль: фокус в поле СТРАНИЦЫ.
            if pr:
                mark = len(_markers(log_path))
                click(*центр(pr))
                after_b = _markers(log_path)
                focus_parent = _last_snap(after_b, "parent", mark)

            # Фаза C — субъект: фокус в поле ФРЕЙМА. Идёт последней, чтобы
            # заодно проверить, что фокус страницы при этом снят.
            if cr:
                mark = len(_markers(log_path))
                click(*центр(cr, FRAME_X, FRAME_Y))
                after_c = _markers(log_path)
                frame_child = _last_snap(after_c, "child", mark)
                frame_parent = _last_snap(after_c, "parent", mark)
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

    if отказы:
        print("движок отказал клику:", *отказы, sep="\n  ")
    check(not отказы, f"ни один клик не отклонён движком ({len(отказы)} отказов)")

    print("базовая линия родителя:", base_parent)
    print("базовая линия ребёнка: ", base_child)
    print("после клика в страницу (родитель):", focus_parent)
    print("после клика во фрейм  (ребёнок): ", frame_child)
    print("после клика во фрейм  (родитель):", frame_parent)

    # ── базовая линия: таблица стилей применяется по обе стороны ────────────
    check(base_parent.get("w") == 102, f"страница: поле без фокуса 102px ({base_parent.get('w')})")
    check(base_child.get("w") == 102, f"фрейм: поле без фокуса 102px ({base_child.get('w')})")

    # ── КОНТРОЛЬ на странице: `:focus` там обязан работать ──────────────────
    check(focus_parent.get("w") == 202,
          f"КОНТРОЛЬ страницы: `:focus` расширил поле до 202px ({focus_parent.get('w')})")
    check(focus_parent.get("bg") == "rgb(0, 128, 0)",
          f"КОНТРОЛЬ страницы: getComputedStyle видит фон `:focus` ({focus_parent.get('bg')})")

    # ── КОНТРОЛЬ вычисленных стилей внутри фрейма, БЕЗ интерактива ──────────
    # Отделяет «нет `:focus`» от «нет вычисленных стилей во фрейме вообще».
    check(base_child.get("plain") == "rgb(1, 2, 3)",
          f"фрейм: getComputedStyle статического узла отдаёт свой фон ({base_child.get('plain')})")

    # ── СУБЪЕКТ ────────────────────────────────────────────────────────────
    check(frame_child.get("w") == 202,
          f"СУБЪЕКТ: `:focus` пересчитал поле ВНУТРИ фрейма до 202px ({frame_child.get('w')})")
    check(frame_child.get("bg") == "rgb(0, 128, 0)",
          f"СУБЪЕКТ: getComputedStyle внутри фрейма видит фон `:focus` ({frame_child.get('bg')})")
    check(frame_child.get("active") == "ctxt",
          f"СУБЪЕКТ: document.activeElement ребёнка — его поле ({frame_child.get('active')})")

    # ── фокус не сосуществует: страница отпустила своё поле ─────────────────
    check(frame_parent.get("w") == 102,
          f"поле СТРАНИЦЫ после клика во фрейм вернулось к 102px ({frame_parent.get('w')})")

    print("ИТОГ:", "ЗЕЛЁНЫЙ" if ok else "КРАСНЫЙ")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
