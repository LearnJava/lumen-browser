# BUG-200

**Статус:** FIXED 2026-06-19
**Компонент:** paint
**Тест:** TEST-80 (9.89% → граница ряда 3 пиксель-в-пиксель с Edge; остаток 9.91% = font-parity → KNOWN_DEBTORS)

## Описание

CSS Tables `border-collapse: collapse` с разной шириной границ соседних ячеек
(ряд 3 теста: чередование `thin` 1px / `thick` 3px). Общие вертикальные грид-линии
в Lumen ломались — отображались тонкими или пропадали через одну.

## Корень

В collapse-модели layout стягивает соседние ячейки внахлёст на ширину общей
грид-линии (`collapse_v_edges` = max границ). Ordered-путь display-list
(`build_display_list_ordered` → `fill_buckets` → `emit_box_self`) эмитит каждый
бокс рекурсивно в DOM-порядке: для каждой ячейки сначала фон, затем граница.
Когда позже рисуемая ячейка тоньше соседа (1px `thin` после 3px `thick`), её фон
закрашивает часть толстой границы соседа в зоне нахлёста, а собственная 1px-граница
восстанавливает лишь 1px → общее ребро схлопывается в 1px вместо max-ширины
(CSS 2.1 §17.6.2). Дефект был только в ordered-пути (окно femtovg + CPU-снимок);
legacy `build_display_list`/`emit_table_box` шёл через `walk`.

## Фикс

`crates/engine/paint/src/display_list.rs`:
- collapse-режим: после рекурсии в детей таблицы перерисовываем все границы ячеек
  поверх всех фонов (`collapse_border_repass_applies` + `collect_table_cells` +
  `emit_table_cell_border`) в обеих ветках `fill_buckets` (SC-root → `post`,
  non-SC → `contents`). Границы лежат внутри padding ячеек, вдали от контента, →
  репасс визуально no-op кроме общих грид-линий; границы одного цвета композитятся
  в более широкую.
- `emit_table_box` (legacy-путь) тоже переведён на 3 прохода (фоны → границы →
  контент) через те же helper'ы для симметрии.

Регресс-тест `ordered_collapse_thick_border_redrawn_after_cell_backgrounds`.

## Остаток

9.91% = font-parity вертикальный дрейф (line-height «normal» Inter ≈1.2 vs Edge
≈1.06 → ячейки на ~2px выше, накапливается вниз по 4 таблицам, проявляется уже в
table-1 separate-режиме без collapse). → KNOWN_DEBTORS (BUG-128).
