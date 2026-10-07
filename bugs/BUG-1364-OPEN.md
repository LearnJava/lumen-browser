# BUG-1364 — Фон `display:table` закрашивает и `table-caption`: подпись внутри фона таблицы, а не снаружи (CSS 2.1 §17.4)

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 13, `css/CSS2` (tables, positioning, floats, floats-clear, abspos, stacking-context, zindex, zorder))
**Область:** layout/paint (`crates/engine/layout/src/box_tree/table.rs` — рамка/фон таблицы включает подпись)

## Симптом

`--dump-display-list`, Ahem 20 px, таблица с фоном и подписью:

| разметка | получено | ожидается |
|---|---|---|
| `display:table; background:blue; caption-side:bottom` > `table-caption` «CAP» + строка с ячейкой 100×50 | `FillRect (0,0,100,70)`, `DrawText` «CAP» в `y=50` — фон захватил подпись | `FillRect (0,0,100,50)`, подпись ниже фона |
| `<table style="background:blue">` + `<caption>` (по умолчанию сверху) | `FillRect (0,0,100,70)`, «CAP» в `y=0` | `FillRect (0,20,100,50)`, подпись выше фона |

Подпись — потомок «обёртки таблицы» (table wrapper box), но не самого блока таблицы: фон и рамки таблицы рисуются вокруг табличной сетки без подписи. Проверено на `<table>` и `display:table` одинаково.

## Как найдено

WPT-RUN-14 срез 13: `tables/caption-side-applies-to-*` — 11 id: `caption-side-applies-to-006.xht` — снимок 800×600, синий прямоугольник теста `y=26…138` (21 332 px), эталона — `26…121` (18 432 px): на высоту подписи (20 px × 192) больше.

## Что делать

Рисовать `background`/`border` таблицы только по её сетке (без подписи); подпись — вне фонового прямоугольника.

## Как проверить

`css/CSS2/tables/caption-side-applies-to-006.xht`.
