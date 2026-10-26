# BUG-1412 — Хит-тест игнорирует `clip-path`: `elementFromPoint` попадает в обрезанную область

**Статус:** FIXED 2026-10-26 (P6)
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

## Исправление (2026-10-26, P6)

Корень: `hit_test_box`/`hit_test_all_box` (`crates/engine/paint/src/hit_test.rs`) проверяли только `rect` бокса и не знали про `clip-path`, хотя отрисовка режет по форме.

Правка: после инверсии transform бокс и всё его поддерево отбрасываются, если точка вне `clip-path` (`clip_path_contains` в `display_list/clip_url.rs` — те же слои клипа, что эмитит отрисовка: `inset` → rect, `circle`/`ellipse`/`polygon`/`path`/`inset round`/`<clipPath>` → `ResolvedClipShape::contains`, nonzero и even-odd). Неразрешённая `url(#id)` ничего не режет, как и в отрисовке. Тесты — `hit_test::tests::clip_path_*`.

Остаток: WPT `css-masking/hit-test/*` не перемерялись; `clip-path-svg-geometry-box.html` (`SVGSVGElement` вместо `SVGRectElement`) и `clip-path-path-with-zoom-hittest.html` могут упираться в другие дефекты.
