# BUG-1715 — остаток BUG-1526: `text-align: -webkit-*` как источник `justify-self: normal`, `fit-content` вертикального abspos-бокса, `stretch` replaced/ratio/table abspos-бокса

**Статус:** OPEN
**Заведён:** 2026-10-26 (P3, остаток [BUG-1526](BUG-1526-FIXED.md))
**Область:** layout (`crates/engine/layout/src/box_tree/multicol_abspos.rs::place_abs_child`; `style/values/typography.rs::TextAlign`; `box_tree/intrinsic.rs`)

## Симптом

Три независимых дефекта, не вошедших в BUG-1526 (проверка — `LUMEN_ALIGN_WPT=<файлы> cargo test -p lumen-layout --lib survey -- --ignored --nocapture`, эмулятор `box_tree/tests/align_self_wpt.rs`):

1. `css/css-align/blocks/justify-self-text-align-2.html` — `text-align: -webkit-right` у контейнера должен сдвигать блок с `justify-self: normal` к правому краю (x=100, у нас 0). `TextAlign` не знает `-webkit-left|center|right`.
2. `css/css-align/abspos/{align,justify}-self-default-overflow-*-vrl.html` (4 файла) — вертикальный abspos-бокс (`writing-mode: vertical-rl`) с выравниванием должен быть `min(max-content, max(min-content, зазор между инсетами))` высотой; у нас `min(max-content, зазор)`, потому что для вертикального бокса нет `min_content_outer_height` (внутренний блок высотой 80/95/120 переполняет зазор 70, а бокс обязан остаться 80/95/120).
3. `css/css-align/abspos/stretch-intrinsic-size-*.html` (4 файла) и `table-{align,justify}-self-stretch.html` — `justify-self`/`align-self: stretch` у abspos `<canvas>`, бокса с `aspect-ratio` и `display: table` между инсетами: размер должен браться из зазора (с учётом ratio и min-размеров), у нас остаётся внутренний.

Вне этого бага (flex/grid, не abspos): `css/css-align/content-distribution/default-overflow-alignment-single-axis-scroll-container.html` — у flex/grid контейнера со скроллом по одной оси `justify-content: center` обязан быть `unsafe` (−50), у нас 0.

## Как проверить

Файлы выше; после исправления добавить их в `PASSING` в `align_self_wpt.rs`.
