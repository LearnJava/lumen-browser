# BUG-1536 — `display: contents` на строке или группе строк таблицы: ячейки пропадают (0×0), таблица схлопывается

**Статус:** OPEN
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
