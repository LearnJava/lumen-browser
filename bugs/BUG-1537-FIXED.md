# BUG-1537 — `<thead>` и `<tfoot>` не переносятся в начало и в конец таблицы: строки идут в порядке документа

**Статус:** FIXED 2026-10-27 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout (`crates/engine/layout/src/box_tree/table.rs` — порядок `thead`/`tbody`/`tfoot`)

## Симптом

Таблица с `<tfoot>`, `<tbody>`, `<thead>` в таком порядке рисует их сверху вниз как есть (`tfoot` y=0, `tbody` y=19, `thead` y=38); по CSS 2.1 §17.2 `table-header-group` всегда первой, `table-footer-group` — последней. 1 reftest (`row-group-order.html`).

## Проба

Проба (`--mcp`, строки по 19,36 px): `f`(tfoot) `y=0`, `b`(tbody) `y=19,36`, `h`(thead) `y=38,7`; ожидается `h` 0, `b` 19,36, `f` 38,7.

## Как найдено

WPT-RUN-14 срез 24: `css-tables/row-group-order.html`. Не путать с [BUG-1352](BUG-1352-FIXED.md) (margin/padding на внутренних табличных боксах).

## Что делать

При построении структуры таблицы выносить первый `table-header-group` вперёд и первый `table-footer-group` назад.

## Как проверить

`css/css-tables/row-group-order.html`.

## Исправление

`table_anon::hoist_header_footer` (вызывается из `fix_children` для `Ctx::Table`): первый `table-header-group` переносится перед первой строкой/группой, первый `table-footer-group` — после последней; последующие header/footer остаются обычными группами в порядке документа. Тест `first_header_and_footer_group_are_hoisted_to_the_table_ends`. Графические тесты не затронуты: во всех трёх страницах с `<thead>` он уже стоит перед `<tbody>`.
