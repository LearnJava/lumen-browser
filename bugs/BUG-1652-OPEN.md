# BUG-1652 — `clip-path: url(#id)` не отсекает SVG-группы и фигуры

**Статус:** OPEN
**Заведён:** 2026-10-19 (P3, остаток [BUG-1294](BUG-1294-FIXED.md))
**Область:** layout/paint (`crates/engine/layout/src/box_tree/svg.rs`, `style/apply/paint.rs`, `display_list/svg_text_decoration.rs`)

## Симптом

`<g clip-path="url(#clip)"><rect fill="url(#pattern)" transform="matrix(1 0 1 1 0 0)"/></g>` с `<clipPath id="clip"><rect x="100" y="50" width="40" height="40"/></clipPath>`: отсечение не применяется, виден весь скошенный паттерн, включая красные плитки. Ожидается зелёный квадрат 40×40.

## Как найдено

Проверка BUG-1294: из 71 reftest `css-transforms` не проходят `matrix/svg-matrix-036…048` (13 штук), паттерн в них нарисован верно, остаётся только неотсечённая часть.

## Что делать

Разобрать `clip-path` со ссылкой на `<clipPath>` (атрибут и CSS), собрать контур из дочерних фигур (`clipPathUnits`, `transform` на `<clipPath>`), эмитить `PushClipPath` вокруг группы/фигуры.

## Как проверить

`css/css-transforms/matrix/svg-matrix-036…048.html` через `--screenshot` против `reference/svg-matrix-clipped-rect-ref.html`.
