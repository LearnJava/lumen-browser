# BUG-1540 — `filter: drop-shadow(…)` не реализован, хотя `CSS-SPECS.md` называет его ✅: функция принимается и молча отбрасывается

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout/paint (`crates/engine/layout/src/style/parse/transform.rs:236` `parse_filter_fn` — нет ветки `drop-shadow`; `style/values/transform.rs:149` `FilterFn` — нет варианта)

## Симптом

`filter: drop-shadow(50px 0 0 green)` не рисует тень (пиксель на 50 px правее — белый); `getComputedStyle().filter` — `none`; `CSS.supports('filter','drop-shadow(1px 1px 0 red)')` — `true`. В `FilterFn` девять вариантов (`Blur…Sepia`), `DropShadow` нет; `parse_filter_fn` для `drop-shadow` возвращает `None`. Строка `filter` в `CSS-SPECS.md` (`… drop-shadow ✅`) была неверна — исправлена в этом срезе. 11 reftest (6 с `reftest-wait`, `drop-shadow-currentcolor-dynamic-*`) ; testharness `drop-shadow-currentcolor-inheritance` и `animation/filter-interpolation-002…004` — в кластерах CSSOM; `tainting-css-dropshadow-001` — в кластере SVG-фильтров.

## Проба

Проба (`--screenshot`, `<div style="width:50px;height:50px;background:blue;filter:drop-shadow(50px 0 0 green)">`):

| точка | у нас | ожидается |
|---|---|---|
| `(25,25)` — сам блок | `(0,0,255)` | `(0,0,255)` |
| `(75,25)` — тень | `(255,255,255)` | `(0,128,0)` |
| `getComputedStyle(t).filter` для `drop-shadow(1px 2px 3px red)` | `none` | `drop-shadow(rgb(255, 0, 0) 1px 2px 3px)` |


## Как найдено

WPT-RUN-14 срез 24: `css/filter-effects/{drop-shadow-clipped-001,drop-shadow-currentcolor-dynamic-001…003,drop-shadow-currentcolor-inheritance,drop-shadow-with-3d-transform,filters-drop-shadow-001,filters-drop-shadow-003,invalidation-css-dropshadow-002,svg-shorthand-drop-shadow-001,svg-mutation-drop-shadow-*}`, `filter-function/filter-function-001…007` (`drop-shadow` внутри `filter()` — отдельная причина: функция `filter()` как `<image>` не реализована, строка `CSS-SPECS.md`).

## Что делать

Добавить `FilterFn::DropShadow { dx, dy, blur, color }` (цвет `currentcolor` разрешать от `color`), разбор, рисование в `cpu_raster.rs` и `renderer.rs`, интерполяцию и resolved-значение; строка `filter` в `CSS-SPECS.md` переведена в 🟡.

## Как проверить

`css/filter-effects/filters-drop-shadow-001.html`, `drop-shadow-clipped-001.html`.
