# BUG-1389 — `scrollWidth`/`scrollHeight` не включают padding в конце и поле flex-/grid-элемента

**Статус:** FIXED 2026-10-20 (P6)
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 16, `css/css-overflow` + `css/css-sizing`)
**Область:** layout (`crates/engine/layout/src/lib.rs:1508` `contributes_to_scrollable_overflow` и `collect_scroll_containers_for_js_state` — размер прокручиваемой области)

## Симптом

`getBoundingClientRect`-независимые чтения `scrollWidth`/`scrollHeight` в `--dump-layout`:

| контейнер | получено | ожидается (CSS Overflow 3 §2.2, Chrome/Firefox) |
|---|---|---|
| `100×100; overflow:auto; padding:20px`, ребёнок `300×300` | 320 × 320 | 340 × 340 (padding в конце входит) |
| `100×100; overflow:auto; padding-bottom:30px`, ребёнок `height:200px` | `sh=200` | 230 |
| `100×100; overflow:auto; display:flex`, элемент `300×300; flex:none; margin-right:50px` | `sw=300` | 350 |
| flex-элемент с `margin-inline-end:950px`, контейнер `100×100` | 100 | 950 |

Padding в начале (`padding-left:20px`) учитывается (320 = 20 + 300), в конце — нет; `margin-*` flex-/grid-элемента в прокручиваемую область не входит (для блочных детей — и не должен: CSS Overflow 3 §2.2 включает поля только у flex-/grid-элементов).

## Как найдено

WPT-RUN-14 срез 16: `css-overflow/scrollable-overflow-padding*.html` (30 из 30 сабтестов), `scrollable-overflow-with-{flex,grid}-item-margin-inline-end*.html` (5 из 8), `scroll-overflow-padding-block-001.html`, `overflow-padding.html`, `overflow-outside-padding.html` … — 35 id, 223 из 240 сабтестов.

## Что делать

Добавлять в scrollable overflow padding в конце (inline-end/block-end, с учётом `writing-mode`/`direction`) и margin box flex-/grid-элемента.

## Как проверить

`css/css-overflow/scrollable-overflow-padding.html`, `scrollable-overflow-with-flex-item-margin-inline-end.html`.

## Исправление (2026-10-20, P6)

`lib.rs::scrollable_extent_{x,y}` и зеркальный `scroll_rollup.rs::OverflowRollup::content_size` (должны совпадать побитно — дифф-тесты в `scroll_rollup.rs`):

- padding на достижимом конце оси (`UnreachableSides`: inline-end/block-end с учётом `direction`/`writing-mode`) добавляется к краю in-flow содержимого — `max(padding box, край + padding-end)`; abspos/fixed её не получают;
- поля flex-/grid-элемента (`item_margins`: прямой ребёнок flex/grid-контейнера, не out-of-flow) входят в границы на достижимой стороне; блочные дети — только border box.

Тесты: `tests/scroll_container.rs` (BUG-1389, 5 штук).
