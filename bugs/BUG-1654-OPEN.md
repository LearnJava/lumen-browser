# BUG-1654 — `box-shadow` на инлайновом `<span>` не попадает в display list

**Статус:** OPEN
**Заведён:** 2026-10-20 (P3, выделено из BUG-1302)
**Область:** paint (`crates/engine/paint/src/display_list/walk.rs`, инлайновая ветка)

## Симптом

Проба `<span style="box-shadow:0 0 0 10px red">text</span>` — на снимке тени нет. WPT `css/css-backgrounds/box-shadow-overlapping-00{1…4}.html`: 3 `thick`.

## Что делать

Эмитить внешнюю (и inset) тень для каждого фрагмента строки инлайн-бокса (как фон/рамка инлайна в `inline_frag.rs`), с учётом `box-decoration-break`.

## Как проверить

`css/css-backgrounds/box-shadow-overlapping-001.html`.
