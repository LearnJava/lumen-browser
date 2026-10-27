# BUG-1535 — `border-collapse`, `caption-side`, `empty-cells`, `table-layout` нет в `getComputedStyle`; `border-spacing` принимает `%` и `calc()` с вырожденным resolved-значением

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** css-parser/layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`; разбор `border-spacing`)

## Симптом

`'border-collapse' in getComputedStyle(el)` — `false`, то же `caption-side`, `empty-cells`, `table-layout` (все четыре есть в `CSS.supports`). `border-spacing: 10%` принимается и в computed даёт `0px` (должно отвергаться), `calc(0.5em + 10px) calc(-0.5em + 10px)` — `0px` (должно сохраниться как `calc(…)`); `html-to-css-mapping-2` ждёт `hidden`, получает `none`; `border-spacing`-анимация — 24 из 120 сабтестов (`expected "7px " but got "20px "`). 15 id, 183 из 214 сабтестов.

## Проба

Проба (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `"border-collapse" in getComputedStyle(t)` (так же `caption-side`, `empty-cells`, `table-layout`) | `false` | `true` |
| `style.borderSpacing="10%"` → computed | `0px` | отвергнуто |
| `style.borderSpacing="calc(0.5em + 10px) calc(-0.5em + 10px)"` → computed | `0px` | `calc(…)` |


## Как найдено

WPT-RUN-14 срез 24: `css-tables/parsing/*-computed`/`*-invalid`, `inheritance.html`, `fixed-layout-2.html`, `html-to-css-mapping-2.html`, `animations/border-spacing-interpolation.html`.

## Что делать

Добавить четыре свойства в карту; отвергать `%` у `border-spacing`; сохранять `calc()` в computed.

## Как проверить

`css/css-tables/parsing/border-collapse-computed.html`, `parsing/border-spacing-valid.html`.

## Срез 1 (2026-10-27) — CSSOM таблиц и `border-spacing`

Сделано: `border-collapse`, `caption-side`, `empty-cells`, `table-layout` в карте `computed_style_to_map` (`selector_query.rs`, `as_str()` у enum-ов в `style/values/box_model.rs`) и в `_LUMEN_KEYWORD_PROPERTIES` шима (неверные ключевые слова отбрасываются в `element.style`). `border-spacing`: разбор `<length>{1,2}` в `style/apply/layout.rs` резолвит `em`/`calc()` на каскаде (результат < 0 зажимается в 0), `%`, литерал < 0, безразмерное число и три компонента отвергаются; каноническая запись для `element.style` — `values/text_cssom.rs::border_spacing` (`calc(0.5em + 10px) calc(-0.5em + 10px)`, равные компоненты схлопываются). Тесты — `selector_query/tests.rs::computed_map_table_properties_bug1535`, `crates/js/src/dom/tests/v8_bug1535_table_cssom.rs`. WPT: `css-tables/parsing` 20/37 → 37/37, `inheritance.html`, `fixed-layout-2.html` проходят, `animations/border-spacing-interpolation.html` 96 → 104 из 120.

Остаток (статус OPEN): интерполяция `border-spacing` (16 сабтестов) — отрицательный прогресс даёт `-6px` вместо 0, `inherit`/`unset` в `from` берут не значение родителя.
