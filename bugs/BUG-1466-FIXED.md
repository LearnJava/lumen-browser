# BUG-1466 — `cargo test -p lumen-paint --lib`: `color_input_paints_value_swatch` падает

**Статус:** FIXED 2026-10-22 (P3)
**Заведён:** 2026-10-08 (P6, замечен при BUG-1328; к `text-transform` отношения не имеет)
**Область:** paint (`crates/engine/paint/src/display_list/tests/text_and_images.rs:514`)

## Симптом

`<input type=color value="#ff0000">` при `input { background: #00ff00; }` — тест ждёт заливку `#ff0000` (образец значения, HTML §4.10.5.1.15), получает только `Color { r: 0, g: 255, b: 0 }`. Остальные 1223 теста крейта проходят.

## Что делать

Проверить, что падение воспроизводится на чистом `main` (правка BUG-1328 трогает только раскладку текста), найти коммит, сломавший образец значения у color input, и вернуть либо код, либо тест.

## Исправление

На актуальном `main` тест проходит: падение устранил BUG-1666 (`5be83c056`) — `color` исключён из `is_disableable_widget` (`crates/engine/layout/src/style/ua.rs`), поэтому авторский `background` на `input[type=color]` (BUG-1277, appearance-disabling) больше не снимает плашку значения. Код paint (`form_controls.rs`) менять не потребовалось. Проверено: `cargo test -p lumen-paint --lib color_input` — 2 passed.
