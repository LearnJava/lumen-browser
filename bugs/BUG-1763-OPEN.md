# BUG-1763 — `getComputedStyle(el).fontFamily` не нормализует кавычки имён семейств; `cssText` хранит неизвестные свойства

**Статус:** OPEN
**Заведён:** 2026-10-27 (P3, остаток [BUG-1551](BUG-1551-FIXED.md))
**Область:** layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map` — сериализация `font-family` в getComputedStyle)

## Симптом

`css/cssom/font-family-serialization-001.html`, 6 из 24 подтестов «Computed: …» и «Serialization of quoted "<generic-family>"»: `font-family: 'Times New Roman'` читается из getComputedStyle как есть (в одинарных кавычках), ожидается `Times New Roman`; `'34J'` → `"34J"`; `"serif"` должен остаться `"serif"`; список `Twisty Tie, '34J', "serif", Veronica, sans-serif` → `Twisty Tie, "34J", "serif", Veronica, sans-serif`.

Второй симптом: `style.cssText = "a-b:c; color:red"` → `cssText` содержит `a-b: c;` — неизвестное свойство должно отбрасываться (`cssstyledeclaration-csstext.html`, «invalid property does not appear», в `.ini`). Область: `web_api_shim_mid.js::_lumen_parse_style`.

## Что делать

Применить те же правила, что `style::values::misc_cssom::canonical_specified_misc("font-family", …)` (имя из идентификаторов — без кавычек; цифра в начале, двойной пробел, слово-ключ — в двойных), к значению в `computed_style_to_map`. Правило уже написано для specified-значения: вынести `font_family` в общую функцию.

## Как проверить

`font-family-serialization-001.html` — 24/24, ожидания `.ini` снять.
