# BUG-1432 — `linear-gradient(… in display-p3|a98-rgb|rec2020|prophoto-rgb …)` рисует сплошной цвет; `color-mix(in display-p3, …)` отвергается; `color(display-p3-linear …)` отвергается

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images` + `css/css-values` + `css/css-color`)
**Область:** layout/paint (`style/parse/gradient*.rs`, `style/values/color_mix.rs` — предопределённые цветовые пространства)

## Симптом

`--screenshot`, строки по 10 px, `background-image:linear-gradient(to right in <пространство>, green, blue)`; пиксели x=5 / x=90:

| пространство | x=5 | x=90 |
|---|---|---|
| `srgb` (контроль) | `(0,121,14)` | `(0,12,231)` |
| `srgb-linear` | `(0,124,62)` | `(0,39,244)` |
| `xyz`, `xyz-d50` | `(0,124,62)` | `(0,39,244)` |
| `oklab` | `(0,126,40)` | `(0,51,236)` |
| `display-p3`, `a98-rgb`, `rec2020`, `prophoto-rgb` | **`(0,58,140)`** | **`(0,58,140)`** |
| `display-p3`, `#000 → #fff` | `(140,140,140)` | `(140,140,140)` |

Цвет-функции: `color(display-p3-linear 0 1 0)` — отброшен (белый); `color(display-p3 0 1 0)`, `color(srgb-linear 0 1 0)`,
`color(a98-rgb 0 1 0)`, `color(rec2020 0 1 0)`, `color(prophoto-rgb 0 1 0)` — `(0,255,0)` (верно после гамут-обрезки).
`color-mix(in display-p3, red, blue)` — отброшен.

## Как найдено

WPT-RUN-14 срез 19: `css-images/gradient/display-p3-linear-gradient.html`, `gradient-eval-predefined-color-spaces.html`,
`srgb-linear-gradient.html`, `xyz-gradient.html`, `css-color-4-colors-default-to-oklab-gradient.html` (7 `thick`);
`css-color/display-p3-linear-001…006.html` (6 `thick`). Пробой подтверждены строки таблицы.

## Что делать

Добавить `display-p3-linear` в список цветовых пространств `color()`; для градиентов и `color-mix()` в предопределённых RGB-
пространствах интерполировать в линейных координатах пространства (сейчас результат константный — вероятно, берётся один
цвет после неверной конверсии).

## Как проверить

Страница из таблицы; `css/css-images/gradient/display-p3-linear-gradient.html`, `css/css-color/display-p3-linear-001.html`.
