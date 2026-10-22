# BUG-1425 — `getComputedStyle().color` и `style.color` схлопывают `lab()`/`lch()`/`oklab()`/`oklch()`/`color()`/`color-mix()`/относительные цвета в `rgb()`

**Статус:** FIXED 2026-10-20 (P3)
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

## Как исправлено

- `ComputedStyle::color_css` — CSSOM-запись computed `color`, когда `rgb()` её не выражает; наследуется вместе с `color`,
  сбрасывается там, где `color` ставят UA/presentational-хинты/forced-colors. Пишет `apply/paint.rs` (ветка `"color"`),
  читает `selector_query.rs::computed_style_to_map`.
- `style/parse/color_serialize.rs::computed_color_css`: `lab()`/`lch()`/`oklab()`/`oklch()` (клампинг L, chroma ≥ 0, hue в
  [0, 360), `none`, `calc()`, проценты, углы, 6 значащих цифр), `color(<space> …)` (`xyz` → `xyz-d65`), `color-mix()`
  (в пространстве интерполяции: lab-подобные дают свою функцию, остальные — `color(srgb …)`), относительные цвета
  (`rgb()`/`hsl()` → `color(srgb …)` по float-каналам до округления в байты, lab-подобные — своя функция).
  Формы, которые sRGB-разбор не берёт (`calc()`, `none`), получают байты из канонической записи.
- `canonical_specified_color` (`el.style.color`): lab-подобные и `color()` — каноническая запись, `color-mix()` и
  относительные цвета — как написаны.
- Не сделано: `calc()` с font-/container-относительными единицами внутри цвета (`sign(1em - 10px)`), `display-p3-linear`
  (разбор `color()` его не знает), computed-значение `border-color` и прочих цветовых свойств (только `color`).
- Тесты: `style::parse::color_serialize::tests`, `selector_query::tests::computed_map_color_keeps_lab_and_wide_gamut_spaces`,
  `style::tests::color::canonical_specified_color_keeps_color_space_forms`, `crates/driver/tests/cases/bug1425_color_space_computed.rs`.
