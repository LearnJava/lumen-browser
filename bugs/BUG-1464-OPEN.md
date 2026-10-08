# BUG-1464 — `Selection.toString()`: смещения считаются в байтах UTF-8 и `text-transform` не применяется

**Статус:** OPEN
**Заведён:** 2026-10-08 (P6, остаток [BUG-1328](BUG-1328-FIXED.md))
**Область:** dom/js (`crates/engine/dom/src/selection.rs::range_text_filtered` — `utf8_floor` над смещениями Range; `crates/js/src/v8_runtime/install/dom_editing.rs` — `_lumen_get_selection_text`, `_lumen_get_range_text`)

## Симптом

1. Смещения Range/Selection по DOM — единицы UTF-16, а `range_text_filtered` режет строку как байты UTF-8 и округляет вниз до границы символа. Узел `"ぁ"` (3 байта), `setBaseAndExtent(node, 0, node, 1)` → `toString()` даёт `""` (смещение 1 попадает внутрь символа и округляется до 0). Для кириллицы и CJK выделение одного символа так же пустое, для двух — обрезается по байтам.
2. `Selection.toString()` не применяет `text-transform`: у `<span style="text-transform:uppercase">abc</span>` выделение целиком даёт `"abc"` (Chrome/Firefox — `"ABC"`), у `full-size-kana`/`full-width`/`math-auto` — исходный символ. У DOM нет стилей, поэтому нужен канал от layout (как `user_select_none_text_nodes`).

## Как найдено

WPT-RUN-14 срез 10, `css/css-text/text-transform/text-transform-full-size-kana-009.html` (58 сабтестов) и `math/text-transform-math-auto-003.html` (112 сабтестов) — оба проверяют `Selection.toString()` после `setBaseAndExtent`. После [BUG-1328](BUG-1328-FIXED.md) (раскладка делает преобразование) по-прежнему 0/170.

## Что делать

Перевести смещения в `range_text_filtered` (и соседние места, где Range режет текстовый узел) на UTF-16 → байты; для `toString()` передавать из layout функцию «преобразованный текст узла» (`TextTransformExtra::apply` + регистр) и применять её к вырезанному фрагменту. Затем снять `FAIL` в `tests/wpt/metadata/css/css-text/text-transform/text-transform-full-size-kana-009.html.ini` и `math/text-transform-math-auto-003.html.ini`.
