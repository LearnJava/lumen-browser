# BUG-1592 — `zoom` не масштабирует `box-shadow`, `text-shadow`, `outline`, `border-spacing`, смещения `transform`, `flex-basis`, размеры SVG и `contain-intrinsic-size`

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 27, `css/css-ruby` + `css-layout-api` + `css-box` + `motion` + `css-highlight-api` + `css-paint-api` + `css-viewport` + `mediaqueries`)
**Область:** layout/paint (`ComputedStyle::effective_zoom`; список масштабируемых длин в `style/apply/`)

## Симптом

Удвоение работает у `width`/`height`/`margin`/`padding`/`border-width`/`border-radius`/`left`/`top`/`gap` (зелёные в пробе), но не у остальных абсолютных длин. В `CSS-SPECS.md` (`zoom`) часть исключений уже записана («Not scaled: `line-height`, inherited length properties…»); эта запись добавляет измеренный список.

## Проба

`.z{zoom:2}`; тени, `outline`, `border-spacing`, `transform`, SVG — `--screenshot` 800×600, цвет каждого элемента уникален, границы областей взяты из PNG; `flex-basis` и `contain-intrinsic-height` — `getBoundingClientRect()`:

| свойство в `.z` | у нас | ожидается |
|---|---|---|
| `box-shadow: 20px 0 0 blue` у бокса 50×50 | полоса тени 20 px | 40 px |
| `outline: 10px solid red` | кольцо 10 px (5 снаружи, см. BUG-1279) | 20 px |
| `text-shadow: 20px 0 0` | смещение 20 px | 40 px |
| `<table style="border-spacing:10px">`, ячейки 10×10 | зазор 10 px, ячейки 20×20 | зазор 20 px |
| `transform: translate(50px,0)` | сдвиг 50 px | 100 px |
| `flex-basis: 50px` у ребёнка `display:flex` | ширина 50 px | 100 px |
| `<svg width=50 height=50>` с `<rect>` | 50×50 | 100×100 |
| `contain-intrinsic-height: 50px; contain: size` | высота 50 px | 100 px |
| `margin`, `padding`, `border-width`, `border-radius`, `width`, `top/left` у `position:absolute`, `gap` (контроль) | ×2 | ×2 |

## Как найдено

WPT-RUN-14 срез 27: `css/css-viewport/zoom/*`: `box-shadow`, `text-shadow`, `filters-drop-shadow`, `border-spacing*`, `matrix-zoom*`, `matrix3d-zoom`, `svg*`, `explicit-inherit/{flex-basis,webkit-flex-basis,outline,column}`, `contain-intrinsic-height`, `canvas`, `scroll-padding`, `scroll-margin`, `stroke`, `text-stroke-width`, `text-decoration-thickness`, `text-underline-offset`, `text-indent*`.

## Что делать

Дополнить список масштабируемых длин (`style/apply/`, `effective_zoom`) перечисленными свойствами; для SVG — масштаб `width`/`height`/`viewBox`/`stroke-width`; `transform` — смещения в `translate*`/`matrix*`. Проверять парами «с `zoom:2` и с удвоенным значением». Пикселей `scroll-padding`/`scroll-margin`/`text-*`/`canvas`/`stroke` проба не измеряла — они в списке по именам файлов.

## Как проверить

`css/css-viewport/zoom/box-shadow.html`, `border-spacing.html`, `matrix-zoom.html`, `explicit-inherit/flex-basis.html`.

## Срез 1 (P4, 2026-10-20)

Закрыто: `box-shadow`, `outline-width`/`-offset`, `border-*-radius`, `flex-basis`, `contain-intrinsic-width/height`, `scroll-margin-*`/`scroll-padding-*`, `perspective`, `translate` и смещения `transform` (`cascade.rs::scale_visual_lengths`; `getComputedStyle()` разворачивает обратно). `em`-длины в `border-width`/`outline`/`box-shadow` теперь разрешаются от незумленного `font-size` (`apply.rs::baked_then_zoomed`). **Остаётся:** `text-shadow`, `border-spacing` (inherited — масштабировать только заданное на самом элементе), размеры SVG, `text-decoration-thickness`/`text-underline-offset`/`-webkit-text-stroke-width`, `filter: drop-shadow`, `canvas`. Баг остаётся OPEN.

## Срез 2 (P4, 2026-10-20)

Закрыто: inherited-длины `text-shadow`, `border-spacing`, `text-indent`, `letter-spacing`, `word-spacing`, `text-decoration-thickness`, `text-underline-offset`, `-webkit-text-stroke-width` множатся на `effective_zoom` только если выигравшая декларация стоит на самом элементе (`cascade.rs::ZoomDeclared::of` + `scale_inherited_lengths`); унаследованное значение уже несёт зум предков, `inherit`/`unset`/`initial` не масштабируются. `em` в них считается от незумленного `font-size` (`apply.rs::baked_then_zoomed`). `getComputedStyle()` делит их на зум. `filter: blur()` масштабируется в `scale_visual_lengths`. `<svg width/height>` умножаются на зум (`build.rs`), без `viewBox` user unit внутри `<svg>` равен `effective_zoom` px (`lay_out_svg_root`). **Остаётся:** `line-height`, `canvas`, `filter: drop-shadow` (не поддержан движком); WPT `css/css-viewport/zoom` не перепрогонялся. Баг остаётся OPEN до перепрогона.
