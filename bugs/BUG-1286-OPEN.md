# BUG-1286 — SVG-фигуры не читают CSS `transform`/`translate`/`rotate`/`scale`, `transform-origin` (атрибут и CSS) и `transform-box`

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 5, `css/css-transforms`)
**Область:** layout (`crates/engine/layout/src/box_tree/svg.rs` — `parse_svg_transform(get_attr("transform"))` для `rect`/`circle`/`ellipse`/`line`/`path`/`polygon`/`polyline`), paint (`display_list/svg_text_decoration.rs::emit_svg_shape`)

## Симптом

Проба (`--screenshot`, SVG 800×400, цвет — по пикселям снимка):

| разметка | получено | ожидается |
|---|---|---|
| `<rect x=0 style="transform:translate(100px,0)">` | на месте | сдвиг на 100 |
| `#a{transform:translate(200px,0)}` + `<rect id=a>` | на месте | сдвиг на 200 |
| `<rect style="translate:100px 0">` | на месте | сдвиг на 100 |
| `<rect width=10 height=10 style="transform:scale(4)">` | 10×10, на месте | 40×40 |
| `<rect x=300 width=40 height=20 style="transform:rotate(90deg);transform-origin:320px 10px">` | не повёрнут | 20×40 |
| `<rect transform="scale(2)" transform-origin="420 20">` (атрибут) | рисуется вне кадра (origin 0 0 игнорирован) | вокруг (420, 20) |
| `<g style="transform:translate(0,100px)">` | **работает** | — |
| `<rect transform="translate(100,0)">` (атрибут) | работает | — |

То есть CSS-свойства трансформаций применяются к `<g>` и к HTML-боксам, но не к самим фигурам; из презентационных атрибутов
читается только `transform`. Свойства `transform-box` в движке нет вовсе (`grep -ri 'transform.box' crates/` пуст; строка
`CSS-SPECS.md` заведена этим срезом).

## Как найдено

WPT-RUN-14 срез 5: 158 из 513 упавших reftest `css/css-transforms` — SVG-фигура с `style`/таблицей стилей/`transform-origin=`
(`transform-origin/svg-origin-*` 81, `transform-box/*` 34, `document-styles`/`inline-styles`/`external-styles` 38,
`2d-rotate-001`, `css-skew-002`, `translate/*`). Классификация по исходнику теста и эталону; `--screenshot` по пробам выше.

## Что делать

Фигура должна брать матрицу из каскада (`transform`, `translate`, `rotate`, `scale`) и из `transform-origin`/`transform-box`
тем же путём, что `<g>`/HTML-бокс (CSS Transforms L1 §SVG, L2 §transform-box); без CSS-значения — атрибут `transform`
(с презентационным `transform-origin=`), по умолчанию origin `0 0` для SVG-элементов без CSS-бокса. Свойство
`transform-box` — P4 (`CSS-SPECS.md`, строка `transform-box`).

## Как проверить

`css/css-transforms/transform-origin/svg-origin-*`, `transform-box/*`, `*-styles/svg-*-styles-*`, `translate/*-in-svg.html`,
`2d-rotate-001.html` (`run_corpus.py --prefixes css/css-transforms/transform-origin`). Часть из 158 id упирается ещё и в
BUG-1287/BUG-1288 — число выигрыша после правки заранее не известно.
