# BUG-1795 — `border-image` в cssText сериализует url() с кавычками

**Статус:** OPEN
**Заведён:** 2026-11-06 (P3, найдено в scoped-test гейте BUG-1694; воспроизводится на чистой ветке без правок BUG-1694 — stash-эксперимент)
**Область:** js/cssom (сериализация `border-image`/`border` в `cssText`)

## Симптом

`crates/js/src/dom/tests/v8_bug1552_style_float_shorthand.rs::css_float_and_border_shorthand_round_trip` падает соло (`cargo test -p lumen-js --features v8-backend --lib v8_bug1552_style_float_shorthand`): ожидалось `imageRaw=border-image: url(a.png) 30 / 10px round;`, фактически `url("a.png")` — url-токен при сериализации shorthand получает кавычки, хотя входная декларация их не содержала (или каноническая форма по спеке — без кавычек для простого url). Остальные 30+ проверок shorthand в тесте проходят — расхождение точечное.

## Где смотреть

Сериализация `border-image` в cssText (шортхенд border → развёрнутые longhand → обратная свёртка), вероятно в `crates/js/src/dom/` (cssom/style) — место, где `url(...)` оборачивается в кавычки при выводе. Тест: `v8_bug1552_style_float_shorthand.rs:67`.
