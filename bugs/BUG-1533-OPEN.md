# BUG-1533 — `visibility: collapse` на `tr`/`tbody`/`col` не убирает строку/колонку из раскладки: таблица сохраняет высоту/ширину

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout (`crates/engine/layout/src/box_tree/table.rs` — строки, группы строк и колонки с `visibility: collapse`)

## Симптом

Строка с `visibility:collapse` остаётся на месте и занимает место: таблица из трёх строк по 30 px, средняя `collapse` — высота 90 (ожидается 60); то же для `tbody` (60 вместо 30), `border-collapse: collapse`. Для колонки проба не получилась: ширина таблицы упирается в [BUG-1335](BUG-1335-OPEN.md) и ширину по содержимому ([BUG-1531](BUG-1531-OPEN.md)). Computed-значение `collapse` верное. 17 id (13 testharness: 22 из 60 сабтестов; 4 reftest: `visibility-collapse-border-spacing-001/002`, `-colspan-003`, `-rowspan-005`). Ещё 8 id `visibility-collapse-{col,colspan,rowcol}-*` падают раньше, на ширине таблицы (492/1008, [BUG-1335](BUG-1335-OPEN.md)), и входят в тот кластер.

## Проба

Проба (`--mcp`, `border-spacing:0`, ячейки `height:30px`):

| разметка | высота таблицы у нас | ожидается |
|---|---|---|
| `tr` ×3, средняя `visibility:collapse` | 90 | 60 |
| то же при `border-collapse:collapse` | 90 | 60 |
| `tbody` ×2, второй `collapse` | 60 | 30 |


## Как найдено

WPT-RUN-14 срез 24: `css-tables/visibility-collapse-{row,col,colspan,rowcol,rowspan,row-group,border-spacing}-*`. Тот же отказ у `visibility:hidden`-тестов (`visibility-hidden-*`, 5 id) не установлен: `visibility-hidden-row-001/002` расходятся на 0,68 px (округление строки), `visibility-hidden-nested-002` — 482 вместо 0; в кластер не входят.

## Что делать

В раскладке таблицы считать `visibility:collapse` строки/группы нулевой высотой (содержимое не рисуется, ячейки с `rowspan` через неё укорачиваются), для колонки — нулевой шириной; промежутки `border-spacing` схлопывать по CSS Tables 3.

## Как проверить

`css/css-tables/visibility-collapse-row-001.html`, `visibility-collapse-col-001.html`.
