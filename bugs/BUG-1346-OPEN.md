# BUG-1346 — `border-collapse: collapse` с `width: 100px` и рамкой 4 px у строки/группы: внешняя ширина 100, а не 104

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** layout (`crates/engine/layout/src/box_tree/table.rs` — геометрия collapsed-границ: половина внешней рамки сверх `width`)

## Симптом

Таблица `display:table; border-collapse: collapse; table-layout: fixed; width: 100px`, внутри группа строк с `border: 4px solid green`: рамка занимает `x = 0…3` и `96…99` — внешняя ширина 100 px. Эталон теста — квадрат 104×104 (`x` 8…111 против 8…107 у Lumen).

## Как найдено

WPT-RUN-14 срез 11: 10 id `borders/border-applies-to-001…007.xht`, `border-color-applies-to-001…004.xht`, `…007` — «полая зелёная рамка 104×104». Расхождение: у Lumen столбец рамки на 4 px ближе (`x` 8…107 против 8…111).

## Что делать

Свериться с CSS 2.1 §17.6.2: в collapsed-режиме половина внешней рамки выступает за `width`; сейчас вся рамка укладывается в `width`.

## Как проверить

`css/CSS2/borders/border-applies-to-001.xht`.
