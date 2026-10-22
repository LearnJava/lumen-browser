# BUG-1462 — `white-space: normal`: U+3000 и прочие не-CSS пробелы схлопываются в U+0020 при разбиении на слова

**Статус:** FIXED 2026-10-22 (P3)
**Заведён:** 2026-10-08 (P6, остаток [BUG-1327](BUG-1327-FIXED.md))
**Область:** layout (`crates/engine/layout/src/box_tree/inline_wrap.rs` — разбиение текста на слова через `split_css_whitespace` / `is_wrap_whitespace`, `box_tree/entry.rs`)

## Симптом

CSS Text L3 §4.1.1: схлопываются только U+0020, U+0009, U+000A, U+000C, U+000D; U+3000 (IDEOGRAPHIC SPACE), U+2003 и прочие пробелы Unicode — обычные символы, каждый занимает свою ширину (U+3000 — 1em). Перенос строки после них допустим (класс UAX #14 `BA`).

`<b>a</b>` + два U+3000 + `<b>b</b>` в `white-space: normal` даёт один фрагмент `a b` с обычным U+0020 (проверено на `--dump`-эквиваленте: тест с `Fixed10`), ожидается `a`, два U+3000, `b`. В `white-space: pre` символы сохраняются (после [BUG-1327](BUG-1327-FIXED.md)).

## Как найдено

Остаток [BUG-1327](BUG-1327-FIXED.md): после того как узел из одних U+3000 перестал отбрасываться, видно, что `wrap_inline_run` режет текст по `is_wrap_whitespace` (`char::is_whitespace()` без NBSP-семейства) и склеивает слова одним пробелом.

## Что делать

Разделять слова по `is_collapsible_whitespace`, а U+3000 и подобные оставлять внутри текста с точкой переноса после них. Затронуты `split_css_whitespace` и его вызовы, ширина и позиции фрагментов.

## Исправление

`is_wrap_whitespace` (`crates/engine/layout/src/box_tree/entry.rs`) больше не считает разделителем слов U+1680, U+2000…200A, U+205F и U+3000: они остаются внутри слова со своей шириной, а перенос после них даёт `line_break::break_opportunities` (UAX #14, класс BA). Тесты — `box_tree/tests/ws_only_text.rs` (`ideographic_spaces_keep_their_width_in_normal_mode`, `line_may_break_after_an_ideographic_space`). Исправлено вместе с [BUG-1465](BUG-1465-FIXED.md), которому нужен тот же код.
