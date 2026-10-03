# BUG-1259 — `flex-wrap: balance` и `flex-line-count` (CSS Flexbox L2, `#algo-balance`) не реализованы: `enum FlexWrap` знает только `nowrap`/`wrap`/`wrap-reverse`

**Статус:** OPEN (ДОРАБОТКА → CSS-SPECS.md)
**Заведён:** 2026-10-03 (P2, WPT-RUN-14 срез 1, `css/css-flexbox`)
**Область:** css-parser/layout (`crates/engine/layout/src/style/values/flexgrid.rs` — `FlexWrap`)

## Симптом

`flex-wrap: balance` и `flex-line-count` (CSS Flexbox L2, `#algo-balance`) не реализованы: `enum FlexWrap` знает только `nowrap`/`wrap`/`wrap-reverse` (слово `balance` есть лишь у `text-wrap-style`). WPT-RUN-14-S1: 40 id `css/css-flexbox/balance/*` (reftest'ы с `flex-wrap: balance` и `getComputedStyle`-тесты, 23 сабтеста).

## Описание

ДОРАБОТКА → `CSS-SPECS.md` (P4): свойство и значение отсутствуют. Строка добавлена в таблицу Flexbox `CSS-SPECS.md`.

## Как найдено

WPT-RUN-14 срез 1.
