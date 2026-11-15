# BUG-1050 — `getComputedStyle()` пропускает несколько box-model шортхендов и трёх свойств, неверно сериализует `box-shadow`/`text-shadow`/`filter`

**Статус:** FIXED 2026-11-15 (P3) — все пять симптомов закрыты багами-преемниками, см. «Закрытие» в конце
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

## Повторное измерение: WPT-RUN-14 срез 23 (2026-10-08)

`text-decoration-thickness` и `text-underline-offset` теперь **есть** в карте `getComputedStyle`, но значение неверное: `text-underline-offset: 1em` при `font-size:40px` — `16px` (разрешено от 16 px, не от размера элемента), `calc(10px - 8px)` — `auto`, `text-decoration-thickness: 200%` — `2%`. Остальное перечисленное здесь и ещё `shape-outside`/`shape-margin`/`shape-image-threshold`/`text-decoration`/`text-emphasis*`/`text-underline-position` — BUG-1515.

## Закрытие (P3, 2026-11-15)

Замер по коду и юнит-тестам `lumen-layout`, без WPT-прогона (dev-release не
собирался): пять исходных симптомов закрыты багами-преемниками, сам BUG-1050
перестал быть отдельным дефектом.

| Симптом | Чем закрыт | Тест |
|---|---|---|
| (1) шортхенды `margin`/`padding`/`inset`/`border-width`/`scroll-margin`/`scroll-padding` | срез 1 (P6) + `crates/engine/layout/src/selector_query.rs:2647` | `computed_map_box_shorthands_bug1050`, `computed_map_border_width_style_shorthands` |
| (2) `-webkit-text-stroke-width` | `eeb4036565` (COMPAT-CSS-TEXT-STROKE) + `selector_query.rs:1890` | `computed_map_webkit_text_stroke_bug1050` (добавлен этим закрытием) |
| (2) `text-decoration-thickness`/`text-underline-offset` | BUG-1515 (`2ca00eb72`) | `computed_map_text_decoration_and_shapes_bug1515` |
| (3) цвет `box-shadow`/`text-shadow` первым | срез 1 (P6) | `computed_map_box_shadow_and_text_shadow`, `computed_map_box_shadow_color_and_border_radius` |
| (4) `filter: drop-shadow()` не резолвится | BUG-1540 (`6b93784e9`) + BUG-1541 (`c1fbc9c7f`) | `computed_map_backdrop_filter_and_filter_canonical_forms`, `computed_map_filter_invalid_value_is_dropped` |
| (5) unitless `line-height` | срез 1 (P6), ratio → px от `font-size` (`selector_query.rs:1723`) | `computed_map_box_shorthands_bug1050` (`line-height: 20px`), `style/tests/cascade.rs` (zoom ×2) |

`cargo test -p lumen-layout --lib selector_query::tests` — 158/158 (включая
новый `computed_map_webkit_text_stroke_bug1050`).

Вне объёма этого бага и остаётся открытым: zoom-масштаб
`width`/`height`/`min-*`/`max-*` на SVG — BUG-1592 срез 2 (закрыл размеры SVG
под `zoom`), каноническая запись `shape-outside` — ROADMAP
`SHAPE-OUTSIDE-COMPUTED`. Пересчёт `svg-computed-style.html.ini` по-прежнему
требует отдельного WPT-прогона и закрытием здесь не заявляется.

