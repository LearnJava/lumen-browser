# BUG-1725 — `border-collapse: collapse`: рамки строк/групп/колонок не участвуют в конфликте, `hidden` не режет рамку таблицы, порядок отрисовки и границы спанящих ячеек

**Статус:** OPEN
**Заведён:** 2026-10-27 (P3, остаток [BUG-1532](BUG-1532-FIXED.md))
**Область:** layout/paint (`crates/engine/layout/src/box_tree/table_collapse.rs`, `table.rs::collapse_v_edges`/`collapse_max_cross_border`, `crates/engine/paint/src/display_list/table.rs`)

## Симптом

После BUG-1532 конфликт рамок разрешается только между ячейками и между ячейкой и рамкой таблицы. Не сделано: (1) рамки `tr`/`tbody`/`col`/`colgroup` в конфликте не участвуют; (2) `hidden` у ячейки на внешнем краю не прячет рамку самой таблицы (таблица рисует её целиком); (3) ячейка рисует собственные четыре стороны, а не общую сетку линий, поэтому порядок отрисовки (`tentative/collapsed-borders-painting-order-*`) и угловые стыки не по спецификации; (4) модель «обе ячейки несут полную ширину и строки перекрываются на одно общее значение» (`collapse_max_cross_border`) вместо половинных ширин; сторона спанящей ячейки берёт конфликт только с первым соседом.

Падающие после BUG-1532 (пиксель-дифф `thick`): `border-collapse-dynamic-section`, `collapsed-border-large-rowgroup-002`, `collapsed-border-paint-phase-001`, `collapsed-border-remove-cell`, `out-of-order-elements-collapsed-border`, `rowspan-cell-border-after-color`, `subpixel-collapsed-borders-003`, `tentative/border-collapse-spanning-cells-001…004`, `tentative/collapsed-borders-painting-order-001/002/003/006/012/013`, `tentative/paint/background-image-column-collapsed`, `…-row-collapsed`, `…/collapsed-border-large-cell`.

## Проба

Не проводилась отдельно: список — разность `identical`/`thick` между бинарём до и после BUG-1532 по `css-tables`; причины по пунктам выше — по устройству `table_collapse.rs` и `emit_table_cell_border`, не по пробе каждого теста.

## Что делать

Хранить разрешённые линии сетки (по рёбрам, не по сторонам ячеек) и рисовать их отдельным проходом в порядке CSS Tables L3; ячейке отдавать половину ширины линии; подключить рамки строк/групп/колонок как источники конфликта.

## Как проверить

`css/css-tables/tentative/collapsed-borders-painting-order-001.html`, `tentative/border-collapse-spanning-cells-001.html`.
