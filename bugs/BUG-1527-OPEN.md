# BUG-1527 — `align-content` не действует на блочный контейнер, `inline-block`, multicol и ячейку таблицы

**Статус:** OPEN
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
