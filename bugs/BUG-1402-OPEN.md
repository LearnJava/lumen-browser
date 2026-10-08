# BUG-1402 — CSSOM multicol: `getComputedStyle` не отдаёт `column-count`/`column-width`/`columns`/`column-fill`/`column-span`/`column-gap`; грамматика `columns` и `column-*`

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 17, `css/css-multicol` + `css/css-contain`)
**Область:** js/layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map` — нет строк для `column-*`; `crates/js/src/shim/web_api_shim_mid.js` — валидация)

## Симптом

`--dump-layout`, скрипт; `d = <div style="columns:3 100px;column-fill:auto;column-span:none;column-gap:5px;column-rule:2px solid red">`:

| запрос | получено | ожидается |
|---|---|---|
| `k in getComputedStyle(d)` для `column-count`, `column-width`, `columns`, `column-fill`, `column-span`, `column-gap`, `container-type` | `false` | `true` |
| то же для `column-rule-width`, `column-rule-style` | `true` | `true` |
| `getPropertyValue('column-count')`, `('column-width')` | `""` | `3`, `100px` |
| `d.style.columns='auto 3'; d.style.columns` | `auto 3` | `3` |
| `d.style.columns='none'` / `columnCount='none'` / `columnFill='none'` | принимается | `""` |
| `d.style.columnWidth='0'` | `0` | `0px` |
| `CSS.supports('column-count','2')` | `true` | `true` |

Раскладка эти свойства знает (`--dump-layout` строит колонки); не хватает строк в рукописной карте `computed_style_to_map` и валидатора.

## Как найдено

WPT-RUN-14 срез 17: `css-multicol/parsing/{column-count,column-fill,column-span,column-width,columns}-{computed,invalid,valid}.html`, `inheritance.html` (11 из 14), `zero-column-width-computed-style.html`, `columns-shorthand-reset-wrap.html`, `multicol-gap-animation-00{1,2,3}.html`, `column-wrap-reset-interpolation.html` (`expected "nowrap" but got ""`).

## Что делать

Добавить `column-count`, `column-width`, `columns`, `column-fill`, `column-span`, `column-gap`, `column-wrap`, `column-height` в `computed_style_to_map`; общая грамматика `column-*` в валидаторе (`none` недопустимо, `0` → `0px`, шорткат сворачивается до кратчайшей формы).

## Как проверить

`css/css-multicol/parsing/column-count-computed.html`, `columns-invalid.html`, `columns-valid.html`.
