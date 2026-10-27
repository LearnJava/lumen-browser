# BUG-1754 — `filter` на `display:inline` не создаёт containing block для `position:absolute|fixed` потомков

**Статус:** OPEN
**Заведён:** 2026-10-27 (P3, остаток [BUG-1543](BUG-1543-FIXED.md))
**Область:** layout (`crates/engine/layout/src/box_tree/multicol_abspos.rs::establishes_abs_cb`, `inline_build.rs` — абсолютные/fixed потомки inline-бокса уходят escape-ом в родительский блок)

## Симптом

`css/filter-effects/filtered-inline-is-container.html`: `#filter{filter:blur(0px);display:inline}` содержит `a`, `position:fixed` и `position:absolute` блоки. Эталон — тот же `transform` на блоке: filter/transform делают предка containing block для fixed и absolute (CSS Filter Effects L1 §3, CSS Transforms L1 §2), и блоки едут вместе с прокручиваемым содержимым. У нас на `display:block` результат совпадает с эталоном пиксель в пиксель, на `display:inline` — нет: fixed/absolute считаются от scroller/viewport, а не от inline-бокса (`diff bbox (108,100)-(400,508)` в `--screenshot`).

## Что делать

Для inline-бокса с `filter`/`transform`/`position != static` строить CB из объединения его фрагментов (padding box первого фрагмента строки в простейшем случае) и считать от него абсолютных/fixed потомков, ушедших escape-ом. Родственный [BUG-1472](BUG-1472-OPEN.md) (`position:relative` на inline-боксе с блоком внутри).

## Как проверить

`css/filter-effects/filtered-inline-is-container.html`.
