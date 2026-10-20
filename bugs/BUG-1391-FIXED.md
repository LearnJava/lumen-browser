# BUG-1391 — CSSOM: `min-/max-width/height` и `contain-intrinsic-size` принимают недопустимые значения и не канонизируют

**Статус:** FIXED 2026-10-20 (P6)
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

## Исправление (2026-10-20, P6)

- `element.style` / `style=""` / `cssText`: `min-*` → `auto | <sizing>`, `max-*` → `none | <sizing>`
  (`style::canonical_specified_min_max_size`, native `_lumen_css_canonical_min_max_size`; `0` → `0px`);
  то же для `min/max-inline-size|block-size`. `contain-intrinsic-{size,width,height,inline-size,block-size}`:
  `auto? [none | <length [0,∞]>]` на ось, у `-size` одна-две оси, `A A` → `A`
  (`style::canonical_specified_contain_intrinsic`, native `_lumen_css_canonical_contain_intrinsic`).
- `getComputedStyle`: `min-width|height` по умолчанию `auto` (было `0px`); `em`/`calc()` без `%`
  (и аргумент `fit-content()`) сворачиваются в px, отрицательная сумма — `0px`
  (`selector_query.rs::sizing_computed_css`).
- Тесты: `style::tests::values` (3), `selector_query::tests::computed_map_min_max_size_initial_and_calc`,
  `dom::tests::v8_bug1391_sizing_cssom` (3).
- Не проверено прогоном WPT: метаданные `tests/wpt/metadata/css/css-sizing/**/*.ini` не менялись.
