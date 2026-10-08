# BUG-1537 — `<thead>` и `<tfoot>` не переносятся в начало и в конец таблицы: строки идут в порядке документа

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout (`crates/engine/layout/src/box_tree/table.rs` — порядок `thead`/`tbody`/`tfoot`)

## Симптом

Таблица с `<tfoot>`, `<tbody>`, `<thead>` в таком порядке рисует их сверху вниз как есть (`tfoot` y=0, `tbody` y=19, `thead` y=38); по CSS 2.1 §17.2 `table-header-group` всегда первой, `table-footer-group` — последней. 1 reftest (`row-group-order.html`).

## Проба

Проба (`--mcp`, строки по 19,36 px): `f`(tfoot) `y=0`, `b`(tbody) `y=19,36`, `h`(thead) `y=38,7`; ожидается `h` 0, `b` 19,36, `f` 38,7.

## Как найдено

WPT-RUN-14 срез 24: `css-tables/row-group-order.html`. Не путать с [BUG-1352](BUG-1352-OPEN.md) (margin/padding на внутренних табличных боксах).

## Что делать

При построении структуры таблицы выносить первый `table-header-group` вперёд и первый `table-footer-group` назад.

## Как проверить

`css/css-tables/row-group-order.html`.
