# BUG-1517 — `shape-outside: circle()/ellipse()` понимает только радиус в px: `at <position>` у `circle()` отбрасывается, `ellipse()` без `at`, `%`, `closest-side`/`farthest-side`, `em`, `calc()` дают прямоугольное обтекание

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 23, `css/css-text-decor` + `css-gaps` + `css-shapes`)
**Область:** layout (`crates/engine/layout/src/box_tree/shapes_floats.rs` — `parse_circle_px`, `parse_shape_ellipse_px`, `register_shape_outside`)

## Симптом

Парсеры форм — строковые, на голых `px`: `parse_circle_px` берёт первый токен и не читает `at …`, `parse_shape_ellipse_px` требует литерал ` at `, радиус `50%`/`closest-side`/`2em`/`calc(…)` не разбирается, `register_shape_outside` возвращает `false` — float обтекается как прямоугольник. Центр круга всегда в центре поля float: `circle(30px at 0px 0px)` обтекается так же, как `circle(30px at 30px 30px)`.

## Проба

Проба (`--mcp`, float `100×100` слева, текст `Arial 20px/20px`, `getClientRects()` первой строки — левый край первых 6 строк, 100 = обтекание прямоугольником):

| `shape-outside` | края строк у нас | ожидается |
|---|---|---|
| `circle(30px at 30px 30px)` | `50,78,80,78,50,0` | то же |
| `circle(30px at 70px 70px)`, `at 0px 0px`, `at left top`, `at 50px` | **те же `50,78,80,78,50,0`** | смещённая форма |
| `circle(50%)`, `circle()`, `circle(closest-side)`, `circle(farthest-side at 0 0)` | `100,100,100,100,100,100` | круг радиуса 50 |
| `circle(calc(10px + 20px) at 30px 30px)`, `circle(2em at 30px 30px)` | `100,…` | круг |
| `ellipse(30px 20px)` (без `at`), `ellipse(50% 50%)`, `ellipse()` | `100,…` | эллипс |
| `ellipse(40px 60px at right bottom)`, `at left top` | `100,…` | эллипс в углу |
| `inset(0 50% 50% 0)`, `polygon(0 0,100% 0,0 100%)` | `100,…` | с процентами от поля |
| `inset(10% round 10%)` | `100,…` | скруглённый прямоугольник |
| `circle(50px)` | `90,99,100,99,90,0` | верно |
| `ellipse(50px 40px at 50px 50px)`, `polygon(0 0,100px 0,0 100px)`, `inset(0 50px 50px 0)`, `inset(10px round 10px)` | верно | |

## Как найдено

WPT-RUN-14 срез 23: `shape-outside/supported-shapes/{circle,ellipse,inset,polygon}/*` с `%`, `closest-side`, `at left|right|top|bottom|center`, `calc`/`em` — 25 reftest `thick` (`shape-outside-circle-032.html`, `-033`, `-034`, `-035`, `-038`, `shape-outside-ellipse-033.html`, `-041`). `float-retry-push-circle.html` (`circle(70.71px at 0px 0px)`) — тот же `at`, но в кластере «не разобрано». Остальные reftest `supported-shapes` с аргументами в px — «не разобрано» (см. раздел `css.md`).

## Что делать

Разобрать аргументы форм настоящим парсером значений (длины с единицами, `%` от опорного бокса, ключевые слова радиуса, `<position>`), вычислять от опорного бокса. Пересекается с BUG-1405 (`clip-path`: тот же разбор `circle()`/`ellipse()`), делать общий разбор.

## Как проверить

Таблица выше; `css/css-shapes/shape-outside/supported-shapes/circle/shape-outside-circle-031.html`, `ellipse/shape-outside-ellipse-033.html`.
