# BUG-1464 — `Selection.toString()`: смещения считаются в байтах UTF-8 и `text-transform` не применяется

**Статус:** FIXED 2026-10-21
**Заведён:** 2026-10-08 (P6, остаток [BUG-1328](BUG-1328-FIXED.md))
**Область:** dom/js (`crates/engine/dom/src/selection.rs` — `utf16_to_byte_offset`/`byte_to_utf16_offset`, `node_length`; `crates/js/src/v8_runtime/install/dom_editing.rs` — `_lumen_set_selection`, `_lumen_get_selection`, `_lumen_get_range_text`, `_lumen_range_delete_contents`)

## Симптом

1. Смещения Range/Selection по DOM — единицы UTF-16, а `range_text_filtered` режет строку как байты UTF-8 и округляет вниз до границы символа. Узел `"ぁ"` (3 байта), `setBaseAndExtent(node, 0, node, 1)` → `toString()` даёт `""` (смещение 1 попадает внутрь символа и округляется до 0). Для кириллицы и CJK выделение одного символа так же пустое, для двух — обрезается по байтам.
2. `Selection.toString()` не применяет `text-transform`: у `<span style="text-transform:uppercase">abc</span>` выделение целиком даёт `"abc"` (Chrome/Firefox — `"ABC"`), у `full-size-kana`/`full-width`/`math-auto` — исходный символ. У DOM нет стилей, поэтому нужен канал от layout (как `user_select_none_text_nodes`).

## Как найдено

WPT-RUN-14 срез 10, `css/css-text/text-transform/text-transform-full-size-kana-009.html` (58 сабтестов) и `math/text-transform-math-auto-003.html` (112 сабтестов) — оба проверяют `Selection.toString()` после `setBaseAndExtent`. После [BUG-1328](BUG-1328-FIXED.md) (раскладка делает преобразование) по-прежнему 0/170.

## Первопричина

Разные единицы смещения на границе JS↔DOM. Внутри движка текстовая позиция — это смещение в **байтах UTF-8**: так работают `range_text_filtered`/`delete_range`/`insert_text_at` в `crates/engine/dom/src/selection.rs`, так считает каретку раскладка (`crates/engine/layout/src/selection.rs`, `source_char_offset`) и так же попадает мышиное выделение из шелла (`crates/shell/src/lumen/doc_select.rs` → `caret_at_point`). Web API, наоборот, считают **единицы UTF-16** (DOM §4.4, Web IDL). Преобразования между двумя моделями не было нигде: нативы `_lumen_set_selection`, `_lumen_get_selection`, `_lumen_get_range_text`, `_lumen_range_delete_contents` передавали байты в обе стороны, а `node_length` (длина `CharacterData` и предел для `Range.setStart`) считала `char`, а не кодовые единицы UTF-16.

## Исправление

Граница JS↔DOM переведена на UTF-16, внутренняя модель движка осталась байтовой:

- `crates/engine/dom/src/selection.rs` — новые публичные помощники `utf16_len`, `utf16_to_byte_offset` (UTF-16 → байты; смещение внутрь сурогатной пары округляется вниз, чтобы не разрезать символ) и `byte_to_utf16_offset` (обратное преобразование); `node_length` теперь действительно считает кодовые единицы UTF-16, как и обещает его докстринг.
- `crates/js/src/v8_runtime/install/dom_editing.rs` — два локальных помощника `internal_offset`/`web_offset` переводят смещение на границе; ими переведены `_lumen_set_selection` (UTF-16 → байты), `_lumen_get_selection` (байты → UTF-16), `_lumen_get_range_text` и `_lumen_range_delete_contents` (UTF-16 → байты) и ветка `execCommand('selectAll')`, кладящая `node_length` в выделение.

Побочный эффект тот же, что нужен был: `Selection.toString()`/`Range.toString()`/`Range.deleteContents()` режут не-ASCII текст по кодовым единицам, `Selection.anchorOffset`/`focusOffset` и `getRangeAt(0).endOffset` отдают странице UTF-16, а выделение, сделанное скриптом, копируется в буфер обмена целиком (раньше шелл копил байтовый обрезок).

`text-transform` (пункт 2) закрыт через канал, заведённый [BUG-1329](BUG-1329-FIXED.md): шим пересобирает покрытые текстовые узлы и прогоняет их через `_lumen_rt_transform`. Раньше этот путь не доходил до конца — базовый текст выделения приходил пустым, и шим выходил по раннему `return`. Теперь база непустая, и `full-size-kana`/`full-width`/`math-auto` применяются.

## Проверка

- `crates/js/src/dom/tests/v8_bug1464_utf16_offsets.rs` — 9 регрессионных тестов: выделение одной кириллической буквы / CJK-символа / целого слова, `Range.toString()`, `Range.deleteContents()`, `Selection.anchorOffset`/`focusOffset` в UTF-16 (в том числе для выделения, поставленного шеллом в байтах), границы смещения для астральных символов (`"😀a"` — 3 кодовые единицы) и `text-transform: full-size-kana` поверх выделения в одну единицу. До исправления падали 8 из 9.
- `cargo test -p lumen-dom` — 347 passed.
- `cargo test -p lumen-js --features v8-backend` — 4876 passed, 3 failed (`v8_gap_uashadowslot` × 2 и `v8_idle_message_clipboard` × 1 — падают и на чистом дереве до правки, см. отчёт).

## Остаток

`.ini` для `css/css-text/text-transform/text-transform-full-size-kana-009.html` и `math/text-transform-math-auto-003.html` по-прежнему помечены `expected: FAIL` — перезапуск WPT не выполнялся (длинный набор вне рамок задачи), снимать эти ожидания нужно отдельным прогоном.
