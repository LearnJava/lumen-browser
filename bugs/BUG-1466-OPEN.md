# BUG-1466 — `cargo test -p lumen-paint --lib`: `color_input_paints_value_swatch` падает

**Статус:** OPEN
**Заведён:** 2026-10-08 (P6, замечен при BUG-1328; к `text-transform` отношения не имеет)
**Область:** paint (`crates/engine/paint/src/display_list/tests/text_and_images.rs:514`)

## Симптом

`<input type=color value="#ff0000">` при `input { background: #00ff00; }` — тест ждёт заливку `#ff0000` (образец значения, HTML §4.10.5.1.15), получает только `Color { r: 0, g: 255, b: 0 }`. Остальные 1223 теста крейта проходят.

## Что делать

Проверить, что падение воспроизводится на чистом `main` (правка BUG-1328 трогает только раскладку текста), найти коммит, сломавший образец значения у color input, и вернуть либо код, либо тест.
