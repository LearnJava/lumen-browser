# BUG-1534 — Потомок ячейки с `height: <percent>` не получает высоту, если высота ячейки определена строкой или таблицей, а не самой ячейкой

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout (`crates/engine/layout/src/box_tree/table_height.rs` — процентные высоты потомков ячейки)

## Симптом

В `display:table-row` с `height:100px` и ячейкой без `height` потомок `height:100%` — 0 (ожидается 100; по CSS Tables 3 §«Percentage heights» высота строки/таблицы, распределённая на ячейку, определена). Когда `height` задан на самой ячейке, потомок `height:50%` считается верно (50). 5 id (`percentage-sizing-of-table-cell-007`, `-children`, `percent-height-overflow-auto-in-*-block-size-cell`, `percent-height-table-cell-child`), ещё 11 id кластера по именам (`percentage-sizing-of-table-cell-children-002…006`, `percent-height-replaced-in-percent-cell-002…004`, `percentages-grandchildren-quirks-mode-*`) этой причиной не объяснены — они строят ячейку без строки (см. [BUG-1362](BUG-1362-OPEN.md)).

## Проба

Проба (`--mcp`, потомок `height:100%`/`50%`):

| разметка | высота потомка у нас | ожидается |
|---|---|---|
| `table-row{height:100px}` > `table-cell` > `div{height:50%}` | 0 | 50 |
| `table{height:100px}` > `td{height:100px}` > `div{height:50%}` | 50 | 50 |


## Как найдено

WPT-RUN-14 срез 24: `css-tables/height-distribution/percentage-sizing-of-table-cell-*`, `percent-height-*`. У `percentage-sizing-of-table-cell-children.html` кроме того харнесс-`ERROR` «1 duplicate test name» — дубль имён в самом тесте, не движок.

## Что делать

Считать процентную высоту потомка ячейки от высоты ячейки после распределения высоты строки/таблицы (CSS Tables 3 §«Cell box height»), если она определена не контентом.

## Как проверить

`css/css-tables/height-distribution/percentage-sizing-of-table-cell-007.html`, `percent-height-table-cell-child.html`.
