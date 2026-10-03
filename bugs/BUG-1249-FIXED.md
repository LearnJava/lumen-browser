# BUG-1249 — `translate_subtree` двигает только `rect`: `svg_paint_matrix` и маска SVG остаются на старом месте

**Статус:** FIXED 2026-10-03 (P1)
**Тип:** корректность (SVG-иконки хрома уезжают после инкрементальных проходов).
**Заведён:** 2026-10-03 (P1, найден в PERF-16 срезе 2: кэш emit по поддереву не попадал на цикле набора текста)
**Область:** layout (`crates/engine/layout/src/incremental.rs` — `translate_subtree`).

## Симптом

После каждого инкрементального прохода раскладки хрома, в котором чистое поддерево с SVG-иконками сдвигается
(цикл `CC12_KEY` — набор текста в омнибоксе), `svg_paint_matrix` у иконок уходит от `rect` на величину сдвига:
у трёх иконок панели ty растёт на 41 px за проход (329,5 против 42,5 у свежей полной раскладки после 8 проходов), у
остальных нет. 25 из 34 матриц расходились с полной раскладкой.

## Причина

`incremental::translate_subtree` (быстрый путь «чистое поддерево переехало»: `layout_dispatch`, `grid_trampoline`,
`ruby`, `chrome_float`) прибавлял `dx/dy` только к `rect`. Документ-пространственные выходы раскладки SVG — `svg_paint_matrix`
(BUG-244/BUG-424) и содержимое `<mask>` (LIB-9) — оставались на прежнем месте. `shift_tree` (путь абсолютно позиционированных
потомков) двигал их с BUG-424 (в), а `translate_subtree` остался без правки. `emit_svg_shape` берёт трансляцию матрицы
как есть для `<path>`-иконок с масштабом (`needs_ctm`), поэтому смещённая матрица — это иконка, нарисованная не там.

## Воспроизведение

`tests::chrome_incremental::incremental_typing_cycles_keep_svg_paint_matrix_equal_to_a_full_layout` (shell): 8 проходов
`cc12_bench_cycle` с растущим текстом омнибокса, затем сверка всех `svg_paint_matrix` со свежей полной раскладкой.

## Исправление

`translate_subtree` вызывает `box_tree::shift_tree` — тот же обход, что двигает `rect`, `svg_paint_matrix` и содержимое `<mask>`
вместе (один способ сдвинуть поддерево — одна реализация). Юнит-тест
`incremental::tests::translate_subtree_moves_svg_paint_matrix_and_mask_content_with_the_rect`.
