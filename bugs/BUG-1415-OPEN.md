# BUG-1415 — grid: scrollable overflow контейнера не учитывает треки, выровненные с переполнением

**Статус:** OPEN
**Заведён:** 2026-10-08 (P6, при BUG-1313)
**Область:** layout (`crates/engine/layout/src/box_tree/grid_trampoline.rs`, расчёт scrollable overflow grid-контейнера)

## Симптом

`css/css-grid/alignment/grid-content-alignment-overflow-001.html` / `-002.html` проверяют `data-expected-scroll-width` /
`-scroll-height` контейнера, треки которого шире/выше контейнера и сдвинуты `justify-content`/`align-content`
(`center`, `end`, `safe`, `unsafe`). Получено: `scrollWidth` 100 вместо 110, 130 вместо 160, 130 вместо 60;
`scrollHeight` 230 вместо 260, 230 вместо 150, 200 вместо 205.

Высоты `fit-content` контейнера с `min-/max-height` из тех же файлов (`height expected 100 but got 250`) — это
[BUG-1314](BUG-1314-FIXED.md), не этот дефект.

## Как проверить

Оба файла, сабтесты с `scrollWidth`/`scrollHeight` в сообщении.
