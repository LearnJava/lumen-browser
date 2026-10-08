# BUG-1425 — `getComputedStyle().color` и `style.color` схлопывают `lab()`/`lch()`/`oklab()`/`oklch()`/`color()`/`color-mix()`/относительные цвета в `rgb()`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout/js (`crates/engine/layout/src/selector_query.rs` — computed `color`; `web_api_shim_mid.js` — specified-значение)

## Симптом

`el.style.color = <значение>; getComputedStyle(el).color`:

| значение | получено | ожидается |
|---|---|---|
| `lab(46 -50 50)` | `rgb(0, 128, 0)` | `lab(46 -50 50)` |
| `oklch(0.5 0.1 90)` | `rgb(121, 96, 6)` | `oklch(0.5 0.1 90)` |
| `color(srgb 0 0.5 0)` | `rgb(0, 128, 0)` | `color(srgb 0 0.5 0)` |
| `color-mix(in srgb, red, blue)` | `rgb(128, 0, 128)` | `color(srgb 0.5 0 0.5)` |
| `rgb(from rebeccapurple r g b)` | `rgb(102, 51, 153)` | `color(srgb 0.4 0.2 0.6)` |
| `hwb(120 0% 50%)` | `rgb(0, 128, 0)` | `rgb(0, 128, 0)` (верно) |

Specified-значение (`el.style.color` сразу после присваивания): `color-mix(in srgb, red, blue)` → `rgb(128, 0, 128)` (ожидается
исходная запись; WPT `color-valid-*`: «`Colors do not match. Actual: rgb(102, 51, 153) Expected: rgb(from rebeccapurple r g b)`»).

## Как найдено

WPT-RUN-14 срез 19: `css-color/parsing/color-computed-{lab,lch,oklab,oklch,color-function,relative-color,rgb,hsl,…}.html`,
`color-valid-{relative-color,color-mix-function,lab,…}.html`, `color-mix-missing-components.html`,
`relative-color-out-of-gamut.html`, `color-mix-out-of-gamut.html`. Пробой подтверждены строки таблицы; остальные сабтесты —
то же сообщение (`expected "lab(…)" but got "rgb(…)"`/`Colors do not match … Expected: color(srgb …)`).

## Что делать

Хранить в computed-стиле исходное цветовое пространство (`CssColor::Wide` уже есть у `color()`); сериализатор
`selector_query.rs:560` отдавать по пространству. Для specified-значения в `style.color` хранить строку записи
(`web_api_shim_mid.js::_lumen_canonicalize_longhand`).

## Как проверить

`css/css-color/parsing/color-computed-lab.html`, `color-computed-color-function.html`, `color-valid-color-mix-function.html`.
