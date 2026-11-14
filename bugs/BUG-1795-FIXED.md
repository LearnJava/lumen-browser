# BUG-1795 — `border-image` в cssText сериализует url() с кавычками

**Статус:** FIXED 2026-11-15 (P3)
**Заведён:** 2026-11-06 (P3, найдено в scoped-test гейте BUG-1694; воспроизводится на чистой ветке без правок BUG-1694 — stash-эксперимент)
**Область:** js/cssom (сериализация `border-image`/`border` в `cssText`)

## Решение

Прав движка не потребовалось: `border-image` идёт через общую канонизацию
`_lumen_css_canonical_background` → `canonical_specified_border_image`, а её
`url("a.png")` — это канон CSSOM/Chrome (та же форма у `background-image`,
BUG-1551: `crates/js/src/dom/tests/v8_bug1551_misc_cssom.rs:60`). Устаревшим был
текст ожидания в тесте BUG-1552, он и исправлен. Та же находка — [BUG-1785](BUG-1785-FIXED.md),
закрыт этим же фиксом как дубликат.

## Симптом

`crates/js/src/dom/tests/v8_bug1552_style_float_shorthand.rs::css_float_and_border_shorthand_round_trip` падает соло (`cargo test -p lumen-js --features v8-backend --lib v8_bug1552_style_float_shorthand`): ожидалось `imageRaw=border-image: url(a.png) 30 / 10px round;`, фактически `url("a.png")` — url-токен при сериализации shorthand получает кавычки, хотя входная декларация их не содержала (или каноническая форма по спеке — без кавычек для простого url). Остальные 30+ проверок shorthand в тесте проходят — расхождение точечное.

## Где смотреть

Сериализация `border-image` в cssText (шортхенд border → развёрнутые longhand → обратная свёртка), вероятно в `crates/js/src/dom/` (cssom/style) — место, где `url(...)` оборачивается в кавычки при выводе. Тест: `v8_bug1552_style_float_shorthand.rs:67`.
