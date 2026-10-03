# BUG-1254 — `getComputedStyle()` не отдаёт ни одного flex-лонгхенда: `flex-direction`, `flex-wrap`, `flex-grow`, `flex-shrink`, `flex-basis`, `order`, шорткоды `f

**Статус:** OPEN
**Заведён:** 2026-10-03 (P2, WPT-RUN-14 срез 1, `css/css-flexbox`)
**Область:** layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`)

## Симптом

`getComputedStyle()` не отдаёт ни одного flex-лонгхенда: `flex-direction`, `flex-wrap`, `flex-grow`, `flex-shrink`, `flex-basis`, `order`, шорткоды `flex`/`flex-flow` — пустая строка (в карте есть `align-*`/`justify-*`, но нет этих). Часть BUG-472 (остаток CSSOM-9), но там записан «flex/grid auto-margin», а не отсутствие самих свойств. WPT-RUN-14-S1: 35 id `css/css-flexbox/{parsing,getcomputedstyle,balance}` + `inheritance.html`, 191 упавший сабтест, в сообщениях `expected "row" but got ""`.

## Описание

Проба: `getComputedStyle(el).getPropertyValue("flex-direction")` на `display:flex;flex-direction:column` — `""`, ожидается `"column"`; то же для остальных перечисленных свойств.

## Как найдено

WPT-RUN-14 срез 1, кластер «computed flex props empty». Значения в `ComputedStyle` есть (`flex_direction` и др.) — не хватает строк в `computed_style_to_map` и парсинг-сериализации шорткодов.

## Как проверить

`run_report.py --root css/css-flexbox/parsing --recursive` и `.../getcomputedstyle`.
