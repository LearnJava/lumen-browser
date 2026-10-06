# BUG-1287 — процентные длины в SVG (`width="100%"`, `cx="50%"`, `x="10%"`) вычисляются в 0

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 5, `css/css-transforms`)
**Область:** layout (`crates/engine/layout/src/box_tree/svg.rs::svg_attr_f32` и разбор атрибутов фигур)

## Симптом

`--dump-display-list` на `<svg style="width:200px;height:100px">`:

| атрибут | получено | ожидается |
|---|---|---|
| `<rect width="100%" height="100%">` | команды `FillRect` нет вовсе | 200×100 |
| `<rect width="50" height="50%">` | `FillRect (0,0,50,0)` | 50×50 |
| `<rect width="10%" y="80" height="10">` | ширина 0 | 20 |
| `<circle cx="50%" cy="50%" r="10">` | `FillRoundedRect (-10,-10,20,20)` | центр (100, 50) |
| `<rect x="10%" y="10%" width="20" height="20">` | в (0,0) | в (20, 10) |
| `<rect width="5em">` | ширина 0 | 5em |

Число с суффиксом `%`/`em` не разбирается, значение молча становится 0 (`svg_attr_f32` читает голое число).

## Как найдено

WPT-RUN-14 срез 5: 41 reftest `css/css-transforms` (`scale/svg-scale-0*.html` 16, `rotate/svg-rotate-angle-45-*`,
`group/svg-transform-nested-008`) — эталон `scale/reference/svg-scale-ref.html` это `<rect width="100%" height="100%"
fill="green"/>`, у Lumen он пуст, тест рисует зелёный квадрат: расхождение целиком в эталоне. Тот же приём у
`matrix/`, `skewX/`, `skewY/`.

## Что делать

Разбирать `<length-percentage>` в атрибутах геометрии: `%` от ширины/высоты (для `r` — от нормализованной диагонали)
ближайшего viewport-а (`viewBox`, иначе размер `<svg>`), абсолютные единицы и `em`/`rem` через те же функции, что
CSS-длины. Затем проверить `x1/x2/y1/y2`, `rx/ry`, `<use width/height>`, `<image>`.

## Как проверить

`css/css-transforms/scale/svg-scale-0{01…17}.html`, `rotate/svg-rotate-angle-45-001.html`; плюс `svg/` (категория WPT).
