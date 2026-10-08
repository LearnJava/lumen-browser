# BUG-1428 — `getComputedStyle().backgroundImage` для градиентов, `cross-fade()`, `image()` — `none`; `image-set()` — `url("image-set(…)")`; `-webkit-image-set` не канонизируется

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout (`crates/engine/layout/src/selector_query.rs` — computed `background-image` и `image-set`)

## Симптом

`getComputedStyle(el).backgroundImage`:

| значение | получено | ожидается |
|---|---|---|
| `linear-gradient(red, blue)` | `none` | `linear-gradient(rgb(255, 0, 0), rgb(0, 0, 255))` |
| `linear-gradient(90deg in oklab, red, blue)` | `none` | `linear-gradient(90deg in oklab, rgb(255, 0, 0), rgb(0, 0, 255))` |
| `radial-gradient(at 10% 20%, red, blue)` | `none` | `radial-gradient(at 10% 20%, rgb(255, 0, 0), rgb(0, 0, 255))` |
| `conic-gradient(red, blue)` | `none` | то же с `conic-gradient(…)` |
| `cross-fade(url(a.png), url(b.png))` | `none` | `cross-fade(url("a.png") 50%, url("b.png") 50%)` |
| `image-set(url(a.png) 1x)` | `url("image-set(url(a.png) 1x)")` | `image-set(url("a.png") 1dppx)` |
| `url(a.png)` (контроль) | `url("a.png")` | так же |
| `url(a.png), linear-gradient(red,blue)` | `url("a.png"), none` | |

Серилизация specified-значения (`el.style.backgroundImage`): `linear-gradient(30deg in lab, red, blue)` → `linear-gradient(in lab 30deg, red, blue)`
(порядок `<angle>`/`<position>` и `in <space>` по CSS Images 4 §3.5 обратный), `object-fit: contain scale-down` не отвергается.

## Как найдено

WPT-RUN-14 срез 19: `css-images/parsing/gradient-interpolation-method-{computed,valid,invalid}.html` (три файла — 2 037 из 2 622
сабтестов всего `css-images`), `gradient-position-{computed,valid,invalid}`, `image-set-{parsing,computed.sub}`, `cross-fade-computed-value`,
`image-function-{computed,valid,invalid}`, `conic-gradient-calc-angle-percentage-{valid,invalid}`.

## Что делать

Заполнить computed-значение для градиентов (цвета в `rgb()`/пространстве записи, см. BUG-1425), `cross-fade()`, `image()`,
`image-set()`; канонизировать порядок аргументов; приводить `-webkit-image-set` к `image-set`.

## Как проверить

`css/css-images/parsing/gradient-interpolation-method-computed.html`, `image-set-computed.sub.html`.
