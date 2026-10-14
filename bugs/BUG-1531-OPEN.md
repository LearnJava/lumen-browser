# BUG-1531 — Ширина таблицы с `width` меньше суммы min-content колонок не раздвигается: ячейки вылезают за таблицу, а `table.offsetWidth` равна `width`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout (`crates/engine/layout/src/box_tree/table.rs` — вычисление ширины таблицы и колонок)

## Симптом

`table{width:50px}` с двумя ячейками `width:30px` даёт таблицу 50 при ячейках 30+30 (ожидается 60: ширина таблицы не может быть меньше суммы минимальных ширин колонок, CSS Tables 3 §3.2). `table{width:1px}` с блоком 50 px внутри — 1 (ожидается 50). `width:50px` с ячейкой `width:100px` — таблица 50 (ожидается 100). С `table-layout: fixed` та же ошибка. 18 id, 170 из 209 сабтестов (`table-width-redistribution*`, `column-widths`, `colspan-redistribution`, `td-box-sizing-001/003`, `absolute-tables-002/003`, `percent-width-ignored-*`).

## Проба

Проба (`--mcp`, `border-spacing:0`, `td{padding:0}`):

| разметка | ширина таблицы у нас | ожидается |
|---|---|---|
| `table{width:50px}` + `td{width:30px}` ×2 | 50 | 60 |
| `table{width:1px}` + `td` с блоком 50 px | 1 | 50 |
| `table{width:50px}` + `td{width:100px}` | 50 | 100 |
| `table{width:50px}` + `td` с блоком 80 px | 50 | 80 |
| авто-ширина: `table` в `float:left` / `inline-block` / `width:fit-content`, две ячейки `width:50px` | 50 | 100 (сумма колонок) |


## Как найдено

WPT-RUN-14 срез 24: `css-tables/tentative/{table-width-redistribution*,column-widths,colspan-redistribution,td-box-sizing-001,td-box-sizing-003,table-minmax,table-quirks,table-limited-quirks}`, `column-track-merging`, `dynamic-rowspan-change`, `fixed-layout-excess-width-distribution-001`, `percent-width-ignored-001…003`, `absolute-tables-002/003`.

## Что делать

Итоговая ширина таблицы = `max(width, сумма min-content колонок + промежутки + рамки)` (при `table-layout:auto`) и `max(width, сумма заданных ширин колонок)` (при `fixed`); не считать заданную `width` верхней границей. Отдельно: ширина по содержимому у таблицы в `float`/`inline-block`/`fit-content` берёт самую широкую колонку вместо суммы (`table_intrinsic_content_width`, `table.rs:616`, суммирует только явные ширины).

## Как проверить

Таблица выше; `css/css-tables/tentative/table-width-redistribution.html`, `tentative/column-widths.html`.
