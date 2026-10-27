# BUG-1532 — `border-collapse: collapse`: не работают `border-style: hidden`/`none` и приоритет стилей рамок при конфликте соседних ячеек

**Статус:** FIXED 2026-10-27 (P3; остаток [BUG-1725](BUG-1725-OPEN.md))
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout/paint (`crates/engine/layout/src/box_tree/table.rs` — разрешение конфликтов рамок `border-collapse: collapse`, CSS 2.1 §17.6.2.1)

## Симптом

В `border-conflict-resolution.html` рамка `hidden` у ячейки не подавляет остальные рамки на границе, `none` с ненулевой шириной даёт зазор, более слабые стили (`dashed`, `dotted`, `outset`, `inset`, `ridge`) рисуются поверх рамки таблицы (`5px solid green`), таблица выше эталона на ~10 px (по снимку). 30 id: 28 `thick`, 1 `mismatch`, 1 testharness (`border-collapse-dynamic-section`).

## Проба

Проба: скриншот `css-tables/border-conflict-resolution.html` (800×600), сравнение с эталоном глазом — у `two`/`three` нижняя граница красная (`outset`/`inset`), у `four` — красные точки (`dotted`), у `two` сверху красные штрихи (`dashed`), синие вертикали идут на всю высоту строки; у эталона нижняя граница — сплошная зелёная рамка таблицы, вертикали короче, таблица ниже на ~10 px. Остальные кластеры (`collapsed-borders-painting-order-001…013`, `border-collapse-spanning-cells-001…004`, `subpixel-collapsed-borders-001…003`, `collapsed-border-paint-phase-*`) не проверялись по отдельности.

## Как найдено

WPT-RUN-14 срез 24: `css-tables/border-conflict-resolution`, `border-collapse-double-border`, `border-collapse-dynamic-section`, `tentative/collapsed-borders-painting-order-001…013`, `tentative/border-collapse-spanning-cells-001…004`, `subpixel-collapsed-borders-*`, `collapsed-border-paint-phase-*`, `out-of-order-elements-collapsed-border`, `rowspan-cell-border-after-color`, `tentative/paint/collapsed-border-large-cell`. Родственные [BUG-1346](BUG-1346-FIXED.md), [BUG-1358](BUG-1358-FIXED.md), [BUG-1336](BUG-1336-FIXED.md): другие стороны модели collapsed borders. Причина по 13 `painting-order` и 4 `spanning-cells` — по именам и текстам, не пробой.

## Что делать

Реализовать алгоритм разрешения конфликтов (CSS 2.1 §17.6.2.1): `hidden` побеждает всё, `none` — самый слабый; при равной ширине — по стилю `double > solid > dashed > dotted > ridge > outset > groove > inset`; при равенстве — по источнику (ячейка > строка > группа строк > колонка > группа колонок > таблица); порядок отрисовки `tentative/collapsed-borders-painting-order`.

## Как проверить

`css/css-tables/border-conflict-resolution.html`, `tentative/collapsed-borders-painting-order-001.html`.

## Исправление

`box_tree/table_collapse.rs::resolve_collapsed_borders` (вызов из `build_table_init` при `collapse`) строит сетку ячеек с учётом `colspan`/`rowspan` и для каждой стороны каждой ячейки выбирает победителя конфликта (CSS 2.1 §17.6.2.1): `hidden` > шире > стиль (`double > solid > dashed > dotted > ridge > outset > groove > inset`) > ячейка выше/левее; победитель записывается в стили обеих ячеек до раскладки, поэтому перекрытие линии сетки и отрисовка согласованы. На внешнем краю рамка таблицы, строго победившая рамку ячейки, обнуляет эту сторону ячейки (рисует таблица); при ничьей остаётся рамка ячейки. Причина «hidden не работает» была в разборе: ключевое слово `hidden` не принималось в шорткатах `border`/`border-*` (`is_border_style_kw`). Заодно шорткат больше не обнуляет заданную ширину при `none`/`hidden` (обнуление — один раз после каскада, `zero_unstyled_border_widths`, теперь и в `pseudo.rs`/`container.rs`): иначе `border: none 10px` + динамический `border-style: solid` терял ширину. Тесты `box_tree/tests/table_collapse_conflict.rs`.

Измерение (reftest-пиксель-дифф `--screenshot` 800×600, `css-tables` + `CSS2/tables`, 1463 теста): `thick` 285 → 277, `identical` 235 → 243; `border-conflict-resolution`, `border-conflict-element-001a/b/c`, `subpixel-collapsed-borders-001/002`, `fixed-table-layout-027/029` стали `identical`. Ещё 2353 теста `css-backgrounds`/`css-borders`/`CSS2/borders`/`CSS2/backgrounds`: регрессий 0.

Остаток — [BUG-1725](BUG-1725-OPEN.md).
