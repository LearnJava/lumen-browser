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
