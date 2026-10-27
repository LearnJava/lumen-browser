# BUG-1533 — `visibility: collapse` на `tr`/`tbody`/`col` не убирает строку/колонку из раскладки: таблица сохраняет высоту/ширину

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout (`crates/engine/layout/src/box_tree/table.rs` — строки, группы строк и колонки с `visibility: collapse`)

## Симптом

Строка с `visibility:collapse` остаётся на месте и занимает место: таблица из трёх строк по 30 px, средняя `collapse` — высота 90 (ожидается 60); то же для `tbody` (60 вместо 30), `border-collapse: collapse`. Для колонки проба не получилась: ширина таблицы упирается в [BUG-1335](BUG-1335-FIXED.md) и ширину по содержимому ([BUG-1531](BUG-1531-FIXED.md)). Computed-значение `collapse` верное. 17 id (13 testharness: 22 из 60 сабтестов; 4 reftest: `visibility-collapse-border-spacing-001/002`, `-colspan-003`, `-rowspan-005`). Ещё 8 id `visibility-collapse-{col,colspan,rowcol}-*` падают раньше, на ширине таблицы (492/1008, [BUG-1335](BUG-1335-FIXED.md)), и входят в тот кластер.

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

## Срез 1 (2026-10-27) — строки и группы строк

Сделано в `box_tree/table_trampoline.rs` (`finish_row`, `finish_table`) и `table_height.rs`: строка с `visibility: collapse` (своим или унаследованным от `tbody` — даже при `visibility: visible` у самой строки) получает высоту 0 и не добавляет щель `border-spacing`; rowspan-ячейка тянется до нижнего края последней видимой строки охвата (все строки охвата схлопнуты — высота 0); схлопнутая строка не получает доли лишней высоты таблицы. Новое поле `TableInit::collapsed_rows`. Тесты: `box_tree/tests/table_visibility_collapse.rs`. WPT css-tables: `visibility-collapse-row-001/-003-dynamic`, `-row-group-001/-002`, `-row-005` стали PASS.

Остаток (статус остаётся OPEN):

- колонки (`col`/`colgroup` с `visibility: collapse`): ширина 0 без щели, обрезка colspan-ячеек;
- содержимое rowspan-ячейки, вылезающее за укороченную высоту, не обрезается (`visibility-collapse-rowspan-005`);
- строки схлопнутой группы с `visibility: visible` рисуют своё содержимое;
- `visibility-collapse-row-004` (`height` таблицы больше суммы видимых строк — Chrome даёт 100 при `height:150px`);
- `visibility-collapse-rowspan-002/-004-dynamic`: «ячейка = сумма высот остальных строк» расходится на 1 px × число строк: в collapse-модели рамок прямоугольники строк перекрываются на ширину общей рамки;
- `visibility-collapse-rowspan-004-dynamic`, подтест «when third row is visible, spanning cell stays the same height»: после динамической смены `visibility` JS-видимый `getBoundingClientRect()` rowspan-ячейки (вне поддерева изменённой строки) остаётся прежним, хотя финальная раскладка верная (172.16); тот же устаревший размер при динамической смене `height` строки.

## Как проверить

`css/css-tables/visibility-collapse-row-001.html`, `visibility-collapse-col-001.html`.
