# BUG-1536 — `display: contents` на строке или группе строк таблицы: ячейки пропадают (0×0), таблица схлопывается

**Статус:** FIXED 2026-10-27 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout (`crates/engine/layout/src/box_tree/build.rs` — `display: contents` на `tr`/`tbody` внутри таблицы)

## Симптом

`<table><tr style="display:contents"><td style="width:50px">a</td></tr></table>`: таблица 1024×0, ячейка 0×0 (ожидается: ячейки участвуют в раскладке как дети таблицы через анонимную строку). 3 reftest `display-contents-001…003`.

## Проба

Проба (`--mcp`): `tr{display:contents}` > `td{width:50px}` → `table: 1024×0`, `td: 0×0`.

## Как найдено

WPT-RUN-14 срез 24: `css-tables/display-contents-001/002/003`. Родственный [BUG-1473](BUG-1473-OPEN.md) (`display:contents` во flex).

## Что делать

Строить таблицу из дерева «плоского» `display:contents`: дети элемента с `contents` становятся детьми родителя при определении табличной структуры.

## Как проверить

`css/css-tables/display-contents-001.html`.

## Итог (2026-10-27, P3)

Заявленный симптом (`tr{display:contents}` > `td` → таблица 1024×0) к моменту разбора уже не воспроизводился: плоская раскладка `display:contents` + анонимные табличные боксы (`table_anon.rs`, BUG-1362) собирают таблицу верно. Тесты `display_contents_row_cells_join_the_table` / `display_contents_row_group_rows_join_the_table` в `box_tree/tests/table_anon.rs` это закрепляют.

Реальная причина красных `display-contents-001…003`: `box_min_max_content_w` (`box_tree/table.rs`) мерил сегменты `InlineRun` с сырыми пробелами и переводами строк исходника (`"\n    X\n    "`), не схлопывая их по CSS Text L3 §4.1.1, — колонка таблицы раздувалась (`X` шириной 98 px вместо 17 px). Дефект общий, не связан с `display:contents`: ячейка `<td>\n   X\n</td>` была шире `<td>X</td>`. Исправлено схлопыванием пробельных серий и срезом пробелов на краях строки при измерении (тест `cell_text_indentation_does_not_widen_the_column`).

Остаток: у `display-contents-003` в эталоне отступ задан `&nbsp;&nbsp;` — без Ahem (локально шрифт-фолбэк) попиксельное совпадение недостижимо; структура (X в третьей колонке) верная.
