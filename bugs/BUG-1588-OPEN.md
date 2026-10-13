# BUG-1588 — `offset-path`: `circle()`/`ellipse()`/`inset()`/`polygon()`/`rect()`/`xywh()`/`shape()`, `url(#id)` и `<coord-box>` принимаются и не действуют

**Статус:** OPEN (ДОРАБОТКА → CSS-SPECS.md)
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 27, `css/css-ruby` + `css-layout-api` + `css-box` + `motion` + `css-highlight-api` + `css-paint-api` + `css-viewport` + `mediaqueries`)
**Область:** css-parser/layout (`resolve_motion_transform` знает `path()` и `ray()`; остальные формы разбираются и игнорируются)

## Симптом

В `CSS-SPECS.md` (строка Motion Path) значится «`url()` paths ⬜», а про basic-shape ничего нет. Проба показывает, что не работает ничего, кроме `path()` и `ray()`.

## Проба

`--dump-layout` + `console.log(getBoundingClientRect())`, контейнер 400×400 в (100,100), дочерний 50×50 `position:absolute;left:0;top:0`:

| `offset-path`, `offset-distance` | положение у нас | ожидается |
|---|---|---|
| без свойства | `100, 100` | `100, 100` |
| `path("M0 0 h 200")`, `50%` (контроль) | сдвиг есть | сдвиг есть |
| `circle(100px at 200px 200px)`, `25%` | `100, 100` | точка на окружности |
| `ellipse(100px 50px at 200px 200px)`, `25%` | `100, 100` | точка на эллипсе |
| `inset(10px)`, `polygon(0 0,100px 0,100px 100px)`, `shape(from 0 0, line to 100px 0)` | `100, 100` | сдвиг вдоль контура |
| `url(#svgRect)` на `<rect>` в `<svg>` на той же странице, `37.5%` | `100, 100` | сдвиг вдоль прямоугольника |
| `border-box` (`<coord-box>` как путь), `25%` | `100, 100` | сдвиг вдоль границы |

## Как найдено

WPT-RUN-14 срез 27: `css/motion/offset-path-shape-{circle,ellipse,polygon,rect,xywh,shape}-*`, `offset-path-url-*`, `offset-path-coord-box-*`.

## Что делать

Строка в `CSS-SPECS.md` (Motion Path L1 — уже есть, дополнена) для P4: геометрия контура для каждой формы (длина, точка и касательная по расстоянию), `<coord-box>` как опорный прямоугольник (`fill-box`/`stroke-box`/`view-box`/`content-box`/`padding-box`/`border-box`), `url()` — разбор `<path>`/`<rect>`/`<circle>`/`<ellipse>`/`<line>`/`<polyline>`/`<polygon>` в SVG. Зависит от BUG-1587: проценты и оборот замкнутого пути.

## Как проверить

`css/motion/offset-path-shape-circle-004.html`, `offset-path-url-002.html`, `offset-path-coord-box-003.html`.
