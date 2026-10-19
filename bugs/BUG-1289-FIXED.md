# BUG-1289 — `transform` на таблице (`<table>`, `display: table`) не применяется

**Статус:** FIXED 2026-10-19 (P3)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 5, `css/css-transforms`)
**Область:** layout/paint (бокс-обёртка таблицы не получает `PushTransform`)

## Симптом

`--screenshot` 800×600, `--dump-display-list`:

| разметка | получено | ожидается |
|---|---|---|
| `<table style="background:blue;transform:translate(300px,0)">` | на x = 0, в display list нет `PushTransform` | на x = 300 |
| `<div style="display:table;…;transform:translate(300px,100px)">` | на месте | сдвиг |
| `<table style="position:absolute;transform:translate(300px,0)">` | на месте | сдвиг |
| `display:inline-block` / `display:list-item` с тем же `transform` | работает | — |

## Как найдено

WPT-RUN-14 срез 5: 16 упавших reftest (`transform-table-*` 7, `transform-display-*`, `transform-abspos-*`,
`transform-generated-*`); пара `transform-table-004.html` — повёрнутая таблица с `caption`, у эталона таблица внутри `<div>`
с тем же `transform`, а у теста она остаётся на месте (разница строк по снимку: тест x 8…50, эталон x 165…207).

## Что делать

Найти, где для боксов `Table`/`display:table` пропускается `forward_box_transform` (стековый контекст создаётся, а
трансформ не эмитится), и применить трансформ к бокс-обёртке (CSS Transforms L1 §2: «transformable element» включает
`display: table`).

## Как проверить

`css/css-transforms/transform-table-00{1…9}.html`, `transform-display-002.html`, `transform-abspos-006.html`.

## Причина и исправление

Путь `--screenshot`/окно строит display list через `fill_buckets` (`box_layer.rs`), а он эмитит `PushTransform` только
для боксов, прошедших `box_can_own_stacking_context`. В предикат входили `Block`/`FlowRoot`/`Image`/`FormControl`, но не
`BoxKind::Table`: у таблицы стековый контекст не заводился, `box_layer_ops` возвращал пустой набор, а `opacity`/`filter`/
`overflow`-слои терялись так же. Путь `build_display_list` (без бакетов) эмитил трансформ для любого бокса, поэтому юнит-тесты
на нём не ловили дефект.

`Table` добавлен в `box_can_own_stacking_context` ([stacking.rs](../crates/engine/layout/src/stacking.rs)) и в
`box_can_own_property_node` ([property_trees.rs](../crates/engine/layout/src/property_trees.rs)). Матрица таблицы с
`<caption>` совпадает с матрицей обёртки-`div` (бокс таблицы включает caption). Тесты —
`ordered_table_transform_wraps_table_content`, `ordered_table_with_caption_transform_equals_wrapper_div`
(`display_list/tests/ordered_build_scroll.rs`). Display list 9 страниц `graphic_tests` с таблицами до и после — идентичен.
