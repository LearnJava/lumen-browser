# BUG-1050 — `getComputedStyle()` пропускает несколько box-model шортхендов и трёх свойств, неверно сериализует `box-shadow`/`text-shadow`/`filter`

**Статус:** OPEN
**Заведён:** 2026-09-13 (BUG-532 срез P3, живой прогон `css/css-viewport/zoom/svg-computed-style.html`)
**Область:** layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`)
**Владелец:** P3/P4

## Симптом

`run_smoke.py //css/css-viewport/zoom/svg-computed-style.html` (dev-release,
2026-09-13) — 22/88 сабтестов, 66 FAIL, несколько независимых причин в одном
файле:

1. **Шортхенды не exposed вовсе** — `margin`/`padding`/`inset`/`border-width`/
   `scroll-margin`/`scroll-padding`: `computed_style_to_map` даёт только
   лонгхенды (`margin-top` и т.п.), обращение к шортхенду возвращает `""`
   (`assert_true: … doesn't seem to be supported in the computed style
   expected true got false`).
2. **Три свойства отсутствуют полностью** —
   `-webkit-text-stroke-width`/`text-decoration-thickness`/
   `text-underline-offset`: тот же симптом, но не шортхенды — этих полей нет
   в карте вовсе ни в каком виде.
3. **`box-shadow`/`text-shadow` — цвет не первым** — ожидается
   `"rgb(0, 0, 0) 10px 10px 10px 0px"`, получаем
   `"10px 10px 10px 0px rgb(0, 0, 0)"` (цвет в хвосте вместо головы).
4. **`filter: drop-shadow(…)` возвращает `"none"`** — компонент не резолвится
   в computed value вовсе (ожидается `"drop-shadow(rgb(0, 0, 0) 10px 10px
   10px)"`).
5. **`line-height` иногда без единиц** — `"0.8125"` вместо `"13px"` (unitless
   number вместо резолвленного px в части путей).

Не имеет отношения к un-zoom из BUG-532 — все пять симптомов
воспроизводятся и без `zoom`.

## Что дальше

Пять независимых причин под одним WPT-файлом — до правки разбить по
пунктам 1–5 (возможно, на 5 отдельных PR): (1)/(2) — расширение
`computed_style_to_map` новыми ключами; (3) — порядок токенов в
сериализаторе `box-shadow`/`text-shadow`; (4) — резолв `drop-shadow()` в
`filter`'s computed-value path; (5) — источник unitless `line-height`
(возможно, инлайновый `<div>` без резолва против `font-size`).

## Срез 1 (P6, 2026-09-30)

Сделано в `computed_style_to_map`: (1) шортхенды `margin`/`padding`/`inset`/
`scroll-margin`/`scroll-padding` (новые лонгхенды `scroll-padding-*`) и
`border-width` в кратчайшей 1–4-значной форме, из уже un-zoom-нутых лонгхендов;
(2) `text-decoration-thickness`, `text-underline-offset`; (3) цвет `box-shadow`/
`text-shadow` первым; (5) абсолютный `line-height` — px (`ratio × font-size / zoom`).

Остаток (не баг, а нереализованная функциональность — задачи ROADMAP):
`filter: drop-shadow()` (нет варианта `FilterFn::DropShadow`, парсер тоже не знает),
`-webkit-text-stroke-width` (нет поля в `ComputedStyle`). Плюс zoom-масштаб
`width`/`height`/`min-*`/`max-*` на SVG (см. `.ini`). Метаданные
`svg-computed-style.html.ini` не пересматривались — WPT-прогон не делался
(нужна сборка dev-release), снимать ожидания FAIL только по замеру.
