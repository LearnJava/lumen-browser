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
