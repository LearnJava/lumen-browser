# BUG-1527 — `align-content` не действует на блочный контейнер, `inline-block`, multicol и ячейку таблицы

**Статус:** FIXED 2026-10-26 (P3); остаток — [BUG-1716](BUG-1716-OPEN.md)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout (`crates/engine/layout/src/box_tree/` — блочные контейнеры и ячейки таблицы: `align-content` применяется только в flex/grid)

## Симптом

Блок высотой 100 px с `align-content: start|center|end` и потомком 20 px оставляет потомка в `y=0` для всех значений (при `center` ожидается 40, при `end` — 80); в `inline-block` то же. У ячейки таблицы (`display:table-cell`) `align-content` игнорируется: ячейка всегда выравнивается как `vertical-align: middle` (потомок в `y=40`) при `start`, `end`, `center`, `baseline`, тогда как `vertical-align: top` даёт 0. В multicol `align-content` не смещает содержимое колонок (32 из 32 сабтестов: ожидалось 10, получено 20). 22 id (14 testharness: 214 из 219 сабтестов; 8 reftest).

## Проба

Проба (`--mcp`, потомок `height:20px`, контейнер `height:100px`):

| контейнер | значение | `y` потомка у нас | ожидается |
|---|---|---|---|
| `div` | `align-content: start` | 0 | 0 |
| `div` | `center` | **0** | 40 |
| `div` | `end` | **0** | 80 |
| `display:inline-block` | `center` | **0** | 40 |
| `td` | `start` | **40** | 0 |
| `td` | `end` | **40** | 80 |
| `td` | `vertical-align: top` | 0 | 0 |


## Как найдено

WPT-RUN-14 срез 24: `css-align/blocks/align-content-block-001…012`, `align-content-block-break-overflow-*`, `align-content-block-display-coverage`, `align-content-block-simple-height-change`, `align-content-table-cell*`, `multicol/align-content-multicol`. CSS Box Alignment 3 §«Block Containers» (2025): `align-content` применяется к блочным контейнерам.

## Что делать

Для блочных контейнеров, `inline-block`, `flow-root`, ячеек таблицы и multicol: после раскладки потомков сдвигать содержимое по свободному месту по оси блока согласно `align-content` (`normal` — как сейчас; для ячейки таблицы `normal` = `vertical-align`); `safe`/`unsafe` для переполнения.

## Как проверить

`css/css-align/blocks/align-content-block-002.html`, `blocks/align-content-table-cell.html`, `multicol/align-content-multicol.html`.

## Исправление (2026-10-26, P3)

`box_tree/block_align_content.rs`:

- **Блок.** Не-`normal` `align-content` у блочного контейнера (`block`, `flow-root`, `list-item`, `inline-block`; не flex/grid/ячейка/вертикальный режим) — независимый контекст форматирования (`establishes_bfc`): margin первого ребёнка остаётся внутри. В `block_flow_trampoline::finish_frame`, когда высота уже итоговая, весь поток (блоки, float, маркер, inline-строки) сдвигается на свободное место `высота content box − высота потока`; статические позиции отложенных abspos-детей (`abs_deferred`) сдвигаются вместе с ним, `position: relative` не учитывается, потому что поток измеряется до него.
- **Значения.** `start`/`end`/`center` и `flex-start`/`flex-end`; `baseline`→`start`, `last baseline`→`end`; `stretch`/`space-between`→`start`; `space-around`/`space-evenly`→`safe center`. Без ключевого слова переполнение остаётся у начала (default overflow alignment блока), явный `unsafe` (`ContentAlignExtra::align_unsafe`) уходит за край.
- **Ячейка таблицы.** `table_valign::cell_block_align` подставляет `align-content` вместо `vertical-align` (`start`→`top`, `center`→`middle`, `end`→`bottom`, `baseline`→`baseline`) во всех местах, где таблица читала `vertical-align` ячейки; сама ячейка блочным путём не двигается (иначе сдвиг применялся бы дважды).
- **Попутный дефект.** `post_child_bookkeeping`: пустой первый ребёнок, чей верхний margin ушёл в родителя (`b_collapses_top`), сдвигал `child_y` на этот margin ещё раз, и margin следующего соседа складывался с ним, а не схлопывался (в потоке без `align-content`: два блока по `margin-top: 10px`, первый пустой, — второй стоял на 20 px вместо 10 px).
- Проверка: `box_tree/tests/align_content_block.rs`; вендоренные `align-content-block-003/004/005/012` — в `PASSING` эмулятора `align_self_wpt.rs`. Реальный прогон `css/css-align/blocks`: `004`, `005` — 17/17, `display-coverage` — 8/8, `align-content-table-cell` — 7/8, остальное — в BUG-1716. A/B `--dump-display-list` по 191 странице корпуса (`graphic_tests/*.html`, `samples/*.html`) с отключённой правкой и с ней — 0 различий.

