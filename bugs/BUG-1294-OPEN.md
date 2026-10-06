# BUG-1294 — SVG `<pattern>` не реализован: заливка `fill="url(#pattern)"` ничего не рисует

**Статус:** OPEN (ДОРАБОТКА → [SVG-PATTERN](../ROADMAP.md))
**Тип:** нереализованная функциональность — элемента `<pattern>` (и `patternTransform`/`patternUnits`/`patternContentUnits`) нет вовсе: `grep -i pattern crates/engine/layout/src/box_tree/svg*.rs` пуст, `SvgPaint` знает `Color`/`Gradient`/`Url`, но не `Pattern`. Градиенты (`LIB-5`) делались отдельной парой типов; для паттерна нужна такая же («семейство», не точечная правка). Ведётся задачей `SVG-PATTERN`.
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 5, `css/css-transforms`)
**Область:** layout (`box_tree/svg.rs`, `SvgPaint`), paint (`emit_svg_shape` — заливка плиткой)

## Симптом

`--screenshot`, `<svg><defs><pattern id=p patternUnits=userSpaceOnUse width=50 height=100>…</pattern></defs>
<rect width=50 height=50 fill="url(#p)"/></svg>` — прямоугольник не нарисован (белый фон). Градиент в тех же условиях
рисуется. Эталоны WPT `css-transforms/matrix/*` строят «четырёхцветный квадрат» из паттерна и трансформа
`matrix(...)`: тест — красный прямоугольник-«якорь» под паттерном; без паттерна остаётся только красный.

## Как найдено

WPT-RUN-14 срез 5: 71 упавший reftest `css/css-transforms` (`matrix/svg-matrix-0*` 69, `patternTransform/*` 2) использует
`<pattern>` — красный «якорь» остаётся на снимке у 58 из 71 (подсчёт красных пикселей в `--screenshot` 800×600).

## Что делать

См. `SVG-PATTERN` в `ROADMAP.md`.

## Как проверить

`css/css-transforms/matrix/svg-matrix-0{01…48}.html`, `patternTransform/*`, `svg/painting/pattern*` (категория WPT `svg`).
