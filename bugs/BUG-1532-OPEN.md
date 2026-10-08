# BUG-1532 — `border-collapse: collapse`: не работают `border-style: hidden`/`none` и приоритет стилей рамок при конфликте соседних ячеек

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout/paint (`crates/engine/layout/src/box_tree/table.rs` — разрешение конфликтов рамок `border-collapse: collapse`, CSS 2.1 §17.6.2.1)

## Симптом

В `border-conflict-resolution.html` рамка `hidden` у ячейки не подавляет остальные рамки на границе, `none` с ненулевой шириной даёт зазор, более слабые стили (`dashed`, `dotted`, `outset`, `inset`, `ridge`) рисуются поверх рамки таблицы (`5px solid green`), таблица выше эталона на ~10 px (по снимку). 30 id: 28 `thick`, 1 `mismatch`, 1 testharness (`border-collapse-dynamic-section`).

## Проба

Проба: скриншот `css-tables/border-conflict-resolution.html` (800×600), сравнение с эталоном глазом — у `two`/`three` нижняя граница красная (`outset`/`inset`), у `four` — красные точки (`dotted`), у `two` сверху красные штрихи (`dashed`), синие вертикали идут на всю высоту строки; у эталона нижняя граница — сплошная зелёная рамка таблицы, вертикали короче, таблица ниже на ~10 px. Остальные кластеры (`collapsed-borders-painting-order-001…013`, `border-collapse-spanning-cells-001…004`, `subpixel-collapsed-borders-001…003`, `collapsed-border-paint-phase-*`) не проверялись по отдельности.

## Как найдено

WPT-RUN-14 срез 24: `css-tables/border-conflict-resolution`, `border-collapse-double-border`, `border-collapse-dynamic-section`, `tentative/collapsed-borders-painting-order-001…013`, `tentative/border-collapse-spanning-cells-001…004`, `subpixel-collapsed-borders-*`, `collapsed-border-paint-phase-*`, `out-of-order-elements-collapsed-border`, `rowspan-cell-border-after-color`, `tentative/paint/collapsed-border-large-cell`. Родственные [BUG-1346](BUG-1346-OPEN.md), [BUG-1358](BUG-1358-OPEN.md), [BUG-1336](BUG-1336-OPEN.md): другие стороны модели collapsed borders. Причина по 13 `painting-order` и 4 `spanning-cells` — по именам и текстам, не пробой.

## Что делать

Реализовать алгоритм разрешения конфликтов (CSS 2.1 §17.6.2.1): `hidden` побеждает всё, `none` — самый слабый; при равной ширине — по стилю `double > solid > dashed > dotted > ridge > outset > groove > inset`; при равенстве — по источнику (ячейка > строка > группа строк > колонка > группа колонок > таблица); порядок отрисовки `tentative/collapsed-borders-painting-order`.

## Как проверить

`css/css-tables/border-conflict-resolution.html`, `tentative/collapsed-borders-painting-order-001.html`.
