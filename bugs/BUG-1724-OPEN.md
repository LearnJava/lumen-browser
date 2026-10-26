# BUG-1724 — CSS Inline: `vertical-align` не хранит `first`/`last`/`alphabetic`…, `calc()` с контейнерными единицами в `line-height`, `baseline-shift: sub|super` у HTML-текста

**Статус:** OPEN
**Заведён:** 2026-10-26 (P3, остаток [BUG-1529](BUG-1529-FIXED.md))
**Область:** layout (`crates/engine/layout/src/style/values/box_model.rs::VerticalAlign`, `style/math_value.rs`, `box_tree/` — инлайн-раскладка)

## Симптом

После BUG-1529 `element.style` принимает грамматику `vertical-align` по CSS Inline L3, но каскад хранит только CSS 2.1-значения (`VerticalAlign`): `first`, `last`, `alphabetic`, `ideographic`, `central`, `mathematical`, `center` не доходят до `getComputedStyle`, поэтому `vertical-align-computed.html` — 15 из 23. `line-height: calc(10 + (sign(2cqw - 10px) * 5))` (и два родственных с `cqw`) даёт `15`/`48px`: `substitute_numeric_math` считает `cqw` как ноль, контейнерного базиса у него нет — `line-height-computed.html` 10 из 13. `baseline-shift: sub|super` не сдвигает HTML-текст (только SVG): `baseline-shift/baseline-shift-sub-super.html` и `-svg.html` — 0 из 4.

## Что делать

Расширить `VerticalAlign` (или хранить разобранный `vertical-align` как три части) и отдать каноническую запись из `computed_style_to_map`; передать в `substitute_numeric_math` контейнерный базис; применить `baseline-shift: sub|super|<length>` к инлайн-боксам HTML.

## Как проверить

`css/css-inline/parsing/{vertical-align,line-height}-computed.html`, `css/css-inline/baseline-shift/baseline-shift-sub-super*.html`.
