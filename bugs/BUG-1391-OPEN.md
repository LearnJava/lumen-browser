# BUG-1391 — CSSOM: `min-/max-width/height` и `contain-intrinsic-size` принимают недопустимые значения и не канонизируют

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 16, `css/css-overflow` + `css/css-sizing`)
**Область:** js/layout (`crates/js/src/shim/web_api_shim_mid.js` — валидация `element.style`; `crates/engine/layout/src/selector_query.rs::computed_style_to_map`)

## Симптом

`e.style[p] = v; e.style[p]` (после сброса `cssText`):

| присваивание | получено | ожидается |
|---|---|---|
| `maxWidth = 'auto'`, `minWidth = 'none'`, `maxHeight = 'auto'`, `minHeight = 'none'` | сохраняется | `""` (отброшено) |
| `maxHeight = '0'`, `minWidth = '0'`, `maxWidth = '0'` | `"0"` | `"0px"` |
| `containIntrinsicSize = 'legacy'` | `"legacy"` | `""` |
| `containIntrinsicSize = '5px 5px'` | `"5px 5px"` | `"5px"` |
| `getComputedStyle(e).minWidth` при `calc(10px - 0.5em)` | `calc(10px - 0.5em)` | значение в px (в тесте — `0px`) |

## Как найдено

WPT-RUN-14 срез 16: `css-sizing/parsing/{min,max}-{width,height}-{valid,invalid,computed}.html` (12 id, 46 из 116 сабтестов), `contain-intrinsic-size/parsing/contain-intrinsic-size-{valid,invalid}.html` (40 из 40 и 3 из 27), `inheritance-001.html` (`min-height` начальное — `auto`, получено `0px`). Всего 15 id, 91 сабтест.

## Что делать

Общая таблица грамматики для размеров (`<length-percentage [0,∞]> | auto | none | min-content | …`): `min-*` не принимает `none`, `max-*` не принимает `auto`; канонизация `0` → `0px`; `calc()` в computed — число.

## Как проверить

`css/css-sizing/parsing/max-width-invalid.html`, `max-width-valid.html`, `min-width-computed.html`.
