# BUG-193

**Статус:** FIXED 2026-06-17
**Компонент:** layout
**Тест:** TEST-64 (13.89% → 8.99%, → KNOWN_DEBTORS)

## Описание

CSS 2.1 §17 Table layout: визуальное расхождение в TEST-64 (две таблицы —
`border-collapse: separate` с `border-spacing` и `border-collapse: collapse`).

## Корень

`display: table`-обёртка не участвовала в схлопывании margin'ов с соседями
(CSS 2.1 §8.3.1). В `lay_out` (блочный поток) признак `is_block`, определяющий
участников схлопывания соседних margin'ов, включал только `Block`/`FlowRoot`, но
не `Table`. В результате `margin-bottom: 20px` первой таблицы складывался с
`margin-top: 18.72px` (1em) следующего `<h3>` (38.72px зазор вместо
collapsed 20px), и вся нижняя таблица «Collapse Mode» уезжала вниз на ~19px →
её фон/рамки/строки не совпадали с эталоном.

Таблица — блок-уровневый бокс, её margin'ы схлопываются с соседними как у
обычного блока, хотя сама таблица устанавливает BFC для своих строк/ячеек
(поэтому `collapsed_top_margin`/`collapsed_bottom_margin` возвращают собственный
margin таблицы без сворачивания в строки — для не-`Block` они и так так делают).

## Фикс

`box_tree.rs` (~5462): `is_block = matches!(kind, Block | FlowRoot | Table)`.
Замер: `<h3>` «Collapse Mode» 282.42 → 263.70, collapse-table top 323.61 →
304.89 (зазор таблица↔h3 38.72 → 20px = max(20, 18.72)). TEST-64 13.89% → 8.99%.

Регресс-тест: `table_bottom_margin_collapses_with_next_sibling`
(layout/src/lib.rs).

## Остаток (→ KNOWN_DEBTORS, BUG-128)

8.99% = font-parity (rule 3): ghosting текста во всех ~21 ячейках + заголовках
(Inter vs дефолтный шрифт Edge) и ~3px накопленный сдвиг collapse-таблицы из-за
разницы line-height ячеек/заголовков (геометрия таблиц совпадает — рамки/фон
separate-таблицы выровнены). Не P3-задача; TEST-64 → KNOWN_DEBTORS.
