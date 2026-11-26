# BUG-1672 — SVG-потомки: `display: block` вместо `inline`, нулевые прямоугольники, у `svg` нет `overflow: hidden`

**Статус:** OPEN
**Заведён:** 2026-10-21 (P2, TEST-11 — [geom-diff-2026-10-21](../docs/quality/geom-diff-2026-10-21.md), example.com, ya.ru, github)
**Область:** layout/UA-стили (SVG UA-таблица) и js (`getBoundingClientRect` для элементов внутри `<svg>`)

## Симптом

```html
<!doctype html><svg id=s width=44 height=44><g id=g><path id=p d="M0 0L10 10L0 10z"/></g></svg>
```

| | Chrome | Lumen |
|---|---|---|
| `getComputedStyle(g).display`, `path` | `inline`, `inline` | `block`, `block` |
| `getComputedStyle(svg).overflow` | `hidden` | `visible` |
| `path.getBoundingClientRect()` | `8,8,10,10` | `8,8,0,0` |
| `g.getBoundingClientRect()` | `8,8,10,10` | `0,0,0,0` |

На сайтах (иконки в шапках): `display: inline → block` у `path`/`g`/`use`, нулевой прямоугольник у
`g×…`, `use×…`; позиция иконки для JS теряется.

## Что делать

1. UA-таблица SVG 2 (§ «User agent style sheet»): `svg:not(:root), symbol, image, marker, pattern,
   foreignObject { overflow: hidden }`; элементы SVG без собственного правила — `display: inline`
   (начальное значение), а не `block`.
2. `getBoundingClientRect` у графических элементов SVG — их bbox в координатах окна (CSSOM View:
   для SVG-элементов без CSS-бокса — `getBBox()` + CTM), у `g` — объединение детей.

## Как проверить

Репро выше — таблица колонки Chrome. `python scripts/geom_diff.py --only example` — нет строк
`display: inline → block` и `overflow: hidden → visible` у `svg`.
WPT: `css/cssom-view/getBoundingClientRect-svg.html`, `svg/styling/required-properties.svg`.

## Срез 2026-11-26 — живой портал, плюс `getBBox` и регистр `viewBox`

portal.nextera.ru и синтетический SVG на видимой области (двухфазный замер — свежие узлы в Lumen
синхронно не меряются), Lumen a0666c3564+патч против Chrome 158:

| замер | Chrome | Lumen |
|---|---|---|
| `path d="M9 4v16"`: `getBoundingClientRect` | `0x12` | `0x0` |
| тот же `path`: `getBBox()` | `0x16` | `0x0` |
| `path`-треугольник `M2 2 L20 2 L20 20 Z`: `getBoundingClientRect` | `13.5x13.5` | `0x0` |
| `svg.getBBox()` (svg 18×18, viewBox 0 0 24 24) | `19x18` | `0x0` |
| контроль: `rect` — `getBoundingClientRect` / `getBBox()` | `13.5x12` / `18x16` | `13.5x12` / `18x16` — совпало |
| контроль: `svg.getBoundingClientRect()` | `18x18` | `18x18` — совпало |

Дополнение к «что делать» пункта 2: нулевой не только `getBoundingClientRect` — **и `getBBox()`**
(в исходной записи он предлагался как источник прямоугольника); у `rect` геометрия верная, дефект
только у `path`. На живой странице нули у всех 425 `path`. Отрисовка самих `path` пикселями не
проверялась (окно перекрыто; `x-scope-screenshot` для живого окна не реализован).

Смежное по регистру: `outerHTML` сериализует `viewBox` как `viewbox`, и `getAttribute("viewbox")`
в нижнем регистре возвращает значение (Chrome — `null`) — в русле известного «`_lumen_set_attr` не
хранит camelCase» из BUG-1107/BUG-791, отдельного номера нет.
