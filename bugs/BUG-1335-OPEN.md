# BUG-1335 — `<table>` / `display: table` без `width` занимает всю ширину контейнера, если ширина колонок задана не явно

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** layout (`crates/engine/layout/src/box_tree/table.rs:616` `table_intrinsic_content_width`, `layout_dispatch.rs:1778` — охрана `intrinsic > 0.0`)

## Симптом

`--dump-layout`, `body { margin: 0 }`, ширина viewport 1024:

| разметка | ширина `Table` | ожидается |
|---|---|---|
| `<table><tr><td>ab</td></tr></table>` | **1024** | по содержимому (≈ 17) |
| `display:table` > `row` > `cell` с `<div style="width:50px">` | **1024** | 50 |
| то же, ячейка `width: 96px` | 96 | 96 |
| `<table style="float:left">` с текстом | 17.1 | 17.1 |

`table_intrinsic_content_width` суммирует только явные ширины колонок (`scan_row_explicit_widths`): текст и блоки без `width` дают 0, и `if intrinsic > 0.0 && intrinsic < content_width` не срабатывает — таблица остаётся на ширину контейнера. Внутри `float` ширина сжимается другим путём (shrink-to-fit у самого float), потому там верно. Родственный дефект — BUG-1341 (`inline-block` без явной ширины).

## Как найдено

WPT-RUN-14 срез 11, `css/CSS2/{backgrounds,borders}/*-applies-to-*.xht`: 34 id (25 `display: table` в `*-applies-to-*.xht` и 9 `<table>` в `background-position-applies-to-*[ab].xht`), `#table { display: table; … }` без `width`, а фигура теста — рамка/фон строки или группы строк, которая в Chrome упирается в правый край таблицы шириной 96 px, у Lumen — в правый край окна (`border-right-width-applies-to-001.xht`: чёрный квадрат у x = 696…792). **A/B:** копия теста с `width: 96px` у `#table` даёт расхождение 2 строки (AA-шов), вместо 97 столбцов.

## Что делать

Считать max-content ячеек (`cell_min_max_border_box_w`, `box_min_max_content_w` уже есть и используются для авто-колонок, `table.rs:848`) и применять `min(max(min-content, available), max-content)` без охраны `> 0` (пустая таблица — ширина рамок и промежутков).

## Как проверить

`css/CSS2/borders/border-right-width-applies-to-001.xht`, `css/CSS2/backgrounds/background-position-applies-to-015.xht`.
