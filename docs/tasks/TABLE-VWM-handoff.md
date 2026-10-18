# TABLE-VWM — записка передачи (ветка `p1-table-vwm`)

Задача: `ROADMAP.md:1048` (таблицы в вертикальном `writing-mode`). Состояние на конец сессии 1: **кода раскладки нет**, есть только разведка и пробный тест.

## Что сделано
- Ветка `p1-table-vwm` занята (`task-claim.sh`), слот `p1-work`.
- `crates/engine/layout/src/box_tree/tests/table_vwm.rs` (+ `mod table_vwm;` в `tests/mod.rs`) — пока один `probe`, печатает rect'ы. Заменить настоящими тестами.
- Базовая картина (до правок): таблица `vertical-lr` 3×2 с `td{width:20px;height:10px}` даёт строки, стеки по `y`, ячейки `width: 0` — рамки rect: table 60×20, ячейки x=0/20/40.

## Разведка (чтобы не перечитывать)
- Диспетчер: `box_tree/layout_dispatch.rs` — арка `BoxKind::Table` (~стр. 1974) строит `table::build_table_init` и возвращает `NeedsTableLoop`; раскладка идёт в `table_trampoline.rs` (горизонтальные оси, `finish_row/finish_table`, `table_height::distribute`, `table_valign::*`). Ранние арки вертикальных режимов — `layout_dispatch.rs` ~стр. 437 (grid → `grid_vertical::dispatch`) и ~стр. 462 (блоки → `vertical_trampoline`). `BoxKind::Table` в вертикальную ветку блоков не попадает.
- Шаблон для нового модуля: `box_tree/grid_vertical.rs::dispatch` (пролог: физические margin/padding, проценты от inline-размера = высоты, `inline_avail = available_height.unwrap_or(viewport.height)`, `crate::vertical::resolve_axis_size`).
- Ячейка вертикального режима — `Block` с `display: table-cell`; в `build_vertical_init` (`crate/vertical.rs:154`) inline-размер = `style.height` (иначе `available_height`), block-размер (физическая `width`) — явная `width` или по содержимому. Чтобы положить ячейку на столбец заданной высоты `I`: на время вызова `Arc::make_mut(&mut cell.style).height = Some(Length::Px(I − frame_vert для content-box | I для border-box))`, вызвать `lay_out(cell, x, y, avail_w, Some(I), …, false)`, вернуть стиль. `UsedSizeOverride` менять `box_sizing` — не годится (переинтерпретирует `width`).
- Сдвиг поддерева: `crate::incremental::translate_subtree`; `table_valign::shift_cell_content` — только по `y`.
- Intrinsic: есть `intrinsic::max_content_outer_height`; **min-content по высоте для вертикального бокса нет** — написать `min_content_outer_height` (зеркало `min_content_outer_width_of_contents`: слово текста через `measure_text_w_families`/`split_css_whitespace`, блок — максимум детей с margin top/bottom, явная `height` побеждает).
- thead/tfoot в горизонтальной раскладке **не переставляются** (идут в DOM-порядке); для вертикальной нужно: header-группы первыми, footer — последними (тест `table-progression-vlr-001`).
- `table_caption_inset: [top, bottom]` читает paint (`engine/paint/src/display_list/fieldset.rs::table_grid_rect`, физические `y`); для вертикальной таблицы с подписями поле надо обобщить (inset по `x` для `vertical-*`) — либо подписи в первом срезе не поддерживать.
- `finish_after_match` (`layout_dispatch.rs:2040`) даёт `position: relative` и abs-детей; горизонтальная таблица его не зовёт (abs-детей кладёт `finish_table` через `multicol_abspos::lay_out_abs_children`).
- Прогон тестов: `cargo test -p lumen-layout --lib table_vwm -- --nocapture > .tmp/vwm.log 2>&1` (первая сборка ~1м40с).

## План (логические оси, новый модуль `box_tree/table_vertical.rs`, подключить ранней аркой в `dispatch_box` для `BoxKind::Table` при `writing_mode != HorizontalTb`)
1. Сетка: строки (с учётом thead/tfoot), слоты ячеек `(row, col, rowspan, colspan)` по занятости rowspan (как `build_row_init`).
2. Столбцы идут по inline-оси (физическая `y`, обращается при `direction: rtl` XOR `sideways-lr`): min/max столбцов из `min/max_content_outer_height` ячеек; inline-размер таблицы — `height`, иначе shrink-to-fit в `available_height`; распределение как в `compute_table_col_widths`; `table-layout: fixed` — по явным `height` первой строки. `border-spacing`: первое значение — вдоль inline, второе — вдоль block (так в задаче).
3. Ячейки (проход 1): `lay_out` на столбец-диапазон; толщина строки (физическая `width`) = максимум `rect.width` ячеек с rowspan 1, дефицит rowspan — в последнюю строку; явная `width` таблицы растит строки пропорционально (как `table_height::distribute`).
4. Строки идут по block-оси (физическая `x`, у `*-rl` справа налево): rect строки/группы = полоса на всю высоту таблицы; ячейка растягивается до толщины, содержимое сдвигается по `x`: у `rl` на прирост, `vertical-align` top/middle/bottom — в block-оси (`baseline` считать как `top`).
5. Подписи (`caption-side: top` = block-start), `<col>`/`<colgroup>` rect'ы, abs-дети, `finish_after_match`.
6. collapse-модель: после separate (в первом срезе допустим spacing 0 + перекрытие рамок на max, как `collapse_v_edges`).
7. Ортогональные ячейки (`writing-mode: horizontal-tb` в вертикальной таблице, `table-cell-valign-*`) — отдельным срезом.

## Done-критерии задачи и WPT
Кластер 117 id `css/css-writing-modes` (`table-progression-*`, `row-progression-*`, `table-column-order-*`, `table-cell-*`, `border-conflict-element-*`, `contiguous-floated-table-*`, часть `block-flow-direction-*`; список — `docs/wpt-vendor-notes/css.md` §css-writing-modes). Тесты `table_vwm.rs` + прогон WPT-подмножества; синхронизировать `ROADMAP.md` (статус), `subsystems/layout.md`, `CAPABILITIES.md`, `docs/wpt-vendor-notes/css.md` (строка кластера «таблицы»).

## Как проверить
`cargo test -p lumen-layout --lib table_vwm`; затем `cargo clippy -p lumen-layout --all-targets -- -D warnings`; пиксели двигаются → полный `python graphic_tests/run.py --continue-on-fail` + голдены в том же коммите.
