# BUG-1412 — Хит-тест игнорирует `clip-path`: `elementFromPoint` попадает в обрезанную область

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 18, `css/css-fonts` + `css/css-masking` + `css/WOFF2`)
**Область:** js/layout (`document.elementFromPoint` — hit-test по боксам; `crates/shell` — снимок геометрии)

## Симптом

Страница: `<div id=a style="position:absolute;left:0;top:300px;width:200px;height:200px;background:green;clip-path:inset(0 100px 0 0)">`,
`<div id=b style="…left:300px…;clip-path:circle(50px at 100px 100px)">`.

| `elementFromPoint(x, y)` | получено | ожидается |
|---|---|---|
| (50, 400) — внутри `a` | `a` | `a` |
| (150, 400) — вне формы `a` | **`a`** | `BODY` |
| (400, 400) — внутри круга `b` | `b` | `b` |
| (310, 310) — вне круга `b` | **`b`** | `BODY` |

## Как найдено

WPT-RUN-14 срез 18: `css-masking/hit-test/clip-path-shape-polygon-and-box-shadow.html` (`20,20`),
`clip-path-element-{objectboundingbox-001,-002,userspaceonuse-001}.html`, `clip-path-svg-geometry-box.html`
(`SVGSVGElement` вместо `SVGRectElement`), `clip-path/clip-path-path-with-zoom-hittest.html`.

## Что делать

В hit-test после проверки bbox проверять попадание в форму `clip-path` (для `inset`/`circle`/`ellipse`/`polygon`/`path`
уже есть геометрия в раскладке). Ссылки `url(#clip)` — после SVG-CLIPPATH.

## Как проверить

`css/css-masking/hit-test/clip-path-shape-polygon-and-box-shadow.html`.
