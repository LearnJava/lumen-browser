# BUG-1415 — grid: scrollable overflow контейнера не учитывает треки, выровненные с переполнением

**Статус:** FIXED 2026-10-20 (P3)
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

## Причина и исправление

Раскладка треков верная (`item.rect.x = -20` и т. д.). Ошибка в расчёте протяжённости scrollable overflow:
`content_width`/`content_height` (`lib.rs::scrollable_extent_{x,y}`) и `scroll_rollup.rs::content_size` вычитали
`min` из `max`, а `min` опускался ниже 0, когда трек (`justify-content: center|end`) уходил за начальный край
padding box. Начальная сторона контейнера недостижима (CSS Overflow L3 §3.3), поэтому она не входит в
прокручиваемую область: `scrollWidth` = 130 вместо 110. Теперь протяжённость на недостижимой стороне
зажимается краем padding box (`min >= 0` для LTR/верхней оси, `max <= pb.width` для RTL).

Тесты: `tests/scroll_container.rs::grid_center_overflow_keeps_end_padding_after_the_tracks`,
`grid_end_overflow_scroll_extent`. Тест `scoped_collect` с отрицательным margin-left теперь ожидает 300/500,
а не 320/520 (выход влево не расширял бы прокрутку в браузерах).

