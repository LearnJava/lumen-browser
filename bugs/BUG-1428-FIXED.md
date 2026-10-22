# BUG-1428 — `getComputedStyle().backgroundImage` для градиентов, `cross-fade()`, `image()` — `none`; `image-set()` — `url("image-set(…)")`; `-webkit-image-set` не канонизируется

**Статус:** FIXED 2026-10-21 (P3)
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

## Как исправлено

- `style/parse/image_computed.rs` (новый): из исходного текста `<image>` строится каноническая запись, которую
  читает `getComputedStyle().backgroundImage`. Лежит в `BackgroundLayer::image_css` (отрисовка её не читает);
  `selector_query::background_layers_to_css` отдаёт её вместо `none`. Записывается из longhand `background-image`
  и из слоёв шортката `background`; сбрасывается в `adjust.rs` (forced-colors) и `presentational.rs`.
  - Градиенты (`linear/radial/conic`, `repeating-*`): цвета — computed (`rgb()`, `color(srgb …)`), длины — px,
    углы — `deg`, позиция — пара `<lp> <lp>` (`right 10px top 20px` → `calc(100% - 10px) 20px`, центр опускается),
    `ellipse 50% 40em` → `50% 640px`, `in <space>` ставится после направления; пространство по умолчанию
    (`srgb` для legacy-цветов, иначе `oklab`) и `shorter hue` опускаются, `xyz` → `xyz-d65`.
  - `image-set()` / `-webkit-image-set()` → `image-set(url("…") Ndppx [type("…")])`: строка → `url()`, `x/dpi/dpcm`
    → `dppx`, `calc()` над разрешениями считается (с проверкой размерности), `type()` перед/после разрешения.
  - `cross-fade()` (новый синтаксис): `<image> <pct>%`, проценты зажаты в `[0,100]`; формы, которые отрисовка
    не умеет (три картинки, цвет вместо картинки), хранятся как `BackgroundImage::None` + запись.
- Заданное значение (`el.style.backgroundImage`, `background_cssom::image`): градиент проверяется по грамматике
  и пишется канонически (`in lab 30deg` → `30deg in lab`, горизонтальная ось в позиции первой, `calc()` в остановках
  с процентом впереди), недопустимые формы отвергаются (`in` без пространства, метод после остановок,
  `hsl longer` без `hue`, `% + deg` вне конического); `image-set()` — по грамматике (`-1x`, `none`, вложенный
  `image-set()`, два разрешения, `type()` без кавычек, `calc(3 * 4)` отвергаются).

WPT (`run_report.py --all`, 2026-10-21, до последних правок валидации `calc()` в остановках и экранирования кавычек в
`type()` — после них прогон не повторялся): `css-images/parsing` 2759 → 2772 из 2836 (все три файла
`gradient-interpolation-method-*` и `gradient-position-{computed,valid}` зелёные), `image-set-computed.sub` 34/36
(остались два `content`), `image-set-parsing` 120 → 27 красных (остальные — `content`/`cursor`/`border-image-source`,
`sign()`), `cross-fade-computed-value` 5/7.

## Остаток

- `image(<color>)` движок не принимает вовсе (`image-function-*`) — нужен разбор `image()` в `background-image`.
- `cross-fade(30% color-mix(… currentcolor …), …)` и `sign()` в проценте; `content: image-set(…)`,
  `border-image-source`, `cursor` не канонизируют `image-set()`.
- `conic-gradient(red calc(50% + 0), blue)` (процент плюс безразмерный 0) принимается; `object-fit: contain scale-down`
  по-прежнему не отвергается (отдельное свойство, не `background-image`).
