# BUG-1539 — `backdrop-filter` не применяется к элементу с `opacity<1`, `transform`, `clip-path`, `mix-blend-mode`; не обрезается по `border-radius`; `isolation` на предке ошибочно образует корень

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** paint (`crates/engine/paint/src/cpu_raster.rs` `PushBackdropFilter`; `display_list/box_layer.rs:268` — порядок `PushBackdropFilter`/`PushTransform`/`PushClipPath`/`PushOpacity`)

## Симптом

Обычный `<div style="backdrop-filter:invert(1)">` над зелёным фоном инвертирует фон (верно, как и `grayscale`, `brightness`, `hue-rotate`, `sepia`, `contrast`, `opacity()`). Но: при `opacity:.5` на том же элементе фон остаётся зелёным; при любом `transform` (в том числе `translate`, `scale(1)`, `rotate(0)`, `translateZ(0)`), `clip-path`, `mix-blend-mode:multiply` — то же; `border-radius:50px` инвертирует и углы за скруглением (должны остаться исходными); `isolation:isolate` у предка отключает фильтр (по Filter Effects 2 `isolation` не образует корень фона). 48 id (42 `thick`, 4 `thin-only`, 2 TIMEOUT: `backdrop-filter-basic-blur` — эталон отрисовывается 9 с, `backdrop-filter-root-toggle-crash` ждёт `TestRendered`). Ещё 10 id `css-backdrop-filters-animation-*` — в кластере анимации фильтров (BUG-1234).

## Проба

Проба (`--screenshot`, 200×120, зелёный блок `100×100` под элементом `100×100` с `backdrop-filter:invert(1)` + дополнительное свойство, цвет `(50,50)`):

| дополнительное свойство | у нас | ожидается |
|---|---|---|
| — | `(255,127,255)` | `(255,127,255)` |
| `opacity:.5` | `(0,128,0)` | фильтр применён и смешан на 50 % |
| `transform:translate(10px,10px)` / `scale(.5)` / `rotate(45deg)` / `translateZ(0)` | `(0,128,0)` | инвертировано |
| `clip-path:circle(30px at 50px 50px)` | `(0,128,0)` | инвертировано внутри круга |
| `mix-blend-mode:multiply` | `(0,128,0)` | инвертировано |
| `border-radius:50px`, пиксель в углу `(3,3)` | `(255,127,255)` | `(0,128,0)` |
| у предка `isolation:isolate`, `backdrop-filter` у потомка | `(0,128,0)` | `(255,127,255)` |


## Как найдено

WPT-RUN-14 срез 24: `css/filter-effects/backdrop-filter-*` (`-3d-transform-perspective`, `-backdrop-root-*`, `-nested-border-radius-clip*`, `-clip-*`, `-transform`, `-scale-transform`, `-isolation-*`, `-with-mix-blend-mode-same-element`, `-basic-opacity-2`, `-plus-filter`, `-edge-*`, `-boundary`, `-svg*`), `backdrop-filters-opacity`, `repaint-added-backdrop-filter`. Измерено только в `--screenshot` (CPU, `cpu_raster.rs`); живое окно (wgpu) не проверялось (`crates/engine/paint/CLAUDE.md`: независимая реализация). Причина по `edge-*`/`boundary`/`svg*` — по именам. `box-shadow` под прозрачным фоном рисуется поверх (проба: `backdrop-filter:invert(1);box-shadow:0 0 10px black` → центр `(0,0,0)`) — [BUG-1302](BUG-1302-OPEN.md).

## Что делать

В `emit` слоёв: `PushBackdropFilter` должен вычисляться после `PushTransform`/`PushClipPath`/`PushOpacity`-эквивалентов с тем же преобразованием и обрезкой; `cpu_raster` применяет фильтр к прямоугольнику `bounds` без учёта трансформации/клипа/скругления — вести область фильтра как путь, а не `Rect`; `isolation` не считать корнем фона.

## Как проверить

`css/filter-effects/backdrop-filter-basic-opacity-2.html`, `backdrop-filter-isolation-isolate.html`, `backdrop-filter-clip-rounded-clip.html`, `backdrop-filter-transform.html`.
