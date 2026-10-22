# BUG-1423 — `overflow: hidden`/`clip` на `<html>` или `<body>` скрывает абсолютно позиционированных потомков

**Статус:** FIXED 2026-10-20 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout/paint (распространение `overflow` корня на viewport)

## Симптом

`--screenshot` 200×100, `body{margin:0}`, потомок `position:absolute;top:0;left:0;width:100px;height:100px;background:green`;
число зелёных px:

| страница | зелёных |
|---|---|
| `html{overflow:visible}` (контроль) | 10000 |
| `html{overflow:hidden}`, потомок абсолютный в `body` | **0** |
| `html{overflow:clip}`, то же | **0** |
| `body{overflow:hidden}`, то же | **0** |
| `html{overflow:hidden}`, потомок абсолютный прямо в `html` | **0** |
| `html{overflow:hidden}`, потомок в потоке (`<div style="width:100px;height:100px">`) | 10000 |
| `html{overflow:hidden}`, потомок `float:left` | 10000 |

`--dump-layout` шаблона теста (`html{background:red;overflow:hidden}`, `#outer{position:absolute;width:100%;height:100%}`):
корневой `Block rect=(0,0,200,0) overflow=hidden/hidden` высотой 0, `#outer` — 200×100 внутри него; клип корня обрезает его в ноль.

## Как найдено

WPT-RUN-14 срез 19: 10 reftest `css-values` с шаблоном выше (красный фон `html`, зелёный `#outer` на весь viewport).
`css/CSS2` в срезах 11…15 тоже содержит такие файлы (общий grep по `tests/wpt/css`: 36 файлов с `html{…overflow:hidden|clip}`).

## Что делать

Распространять `overflow` корня (и `body`, если `html` — `visible`) на viewport, а у корневого/`body`-бокса принудительно
использовать `visible` для клипа. Отдельно от [BUG-1398](BUG-1398-OPEN.md) (перенос фона `body` на холст). Правка двигает
пиксели — полный `graphic_tests/run.py --continue-on-fail` и эталоны в том же коммите.

## Как проверить

Страница из таблицы; `css/css-values/calc-in-max.html`, `calc-parenthesis-stack.html`, `vh-interpolate-px.html`.

## Исправление

Исправлено до взятия задачи: срез 90 css-gaps (`167de3c718`) — `ComputedStyle::overflow_to_viewport` (`cascade.rs::overflow_goes_to_viewport`,
CSS Overflow L3 §3.5) и `style_overflow_clips` (`paint/src/display_list/print.rs`): `hidden`/`clip` корня и `body` при `html{overflow:visible}`
уходят во viewport и не клипуют бокс. Тесты — `paint/src/display_list/tests/viewport_overflow.rs`.
Проверено 2026-10-20 на `dev-release` (`--screenshot`, 1024×720): `html{overflow:hidden}`, `html{overflow:clip}`, `body{overflow:hidden}` и
потомок прямо в `html` — по 10 000 зелёных px; шаблон WPT (`html{background:red;overflow:hidden}` + `#outer` 100%×100%) — весь viewport зелёный, 0 красных.
