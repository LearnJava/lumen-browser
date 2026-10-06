# BUG-1338 — Float, идущий после строчного содержимого на той же строке, ставится под строку, а не к её верху

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** layout (`crates/engine/layout/src/box_tree/` — размещение float среди inline-контента; CSS 2.1 §9.5.1 правила 6 и 8)

## Симптом

`--dump-layout`, контейнер шириной 300 px:

| разметка | `y` float | ожидается |
|---|---|---|
| `text <div style="float:right;width:10px;height:50px"></div> more` | **17.72** (под строкой) | 0 (верх строки, куда float помещается) |
| `<img height=20> <span style="float:right;…">` | **24** | 0 |
| `<span style="display:inline-block;height:20px"></span> <span style="float:right">` | **24** | 0 |
| `<img style="vertical-align:top;height:96px"> <img style="float:right">` | **96** | 0 |
| `<div style="float:right">` первым в блоке | 0 | 0 (верно) |

Float, стоящий **первым** в блоке, ставится верно; после любого строчного содержимого — ниже строки.

## Как найдено

WPT-RUN-14 срез 11: 7 id `borders/border-color-001.xht`, `-006`, `border-color-shorthand-001.xht`, `border-width-shorthand-001…004.xht` — **сам тест рисуется верно, ошибочен эталон**: он собирает «полую рамку» из `<img style="vertical-align:top">` и `<img style="float:right">` в одной строке (`img + img { float: right }`). У Lumen правый столбик уезжает на высоту строки вниз; у теста (настоящая рамка `border`) он на месте.

## Что делать

Правило 6 §9.5.1: внешняя верхняя граница float не выше верха текущей строки; если float помещается в строку — он ставится у её верха и сужает строку (не откладывается на следующую). Сейчас float после непустой строки отправляется на `y` низа строки.

## Как проверить

`css/CSS2/borders/border-color-001.xht` (`reftest_pixdiff.py --viewport 800x600 --ahem` — `identical` после правки).
