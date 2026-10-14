# BUG-1593 — Media Queries 4: вложенные скобки, `or`, `not (…)` внутри скобок, диапазонный синтаксис, `calc()`, единицы `cm`/`in`/`pt`/`vw`/`ex`/`ch`/`%`, безразмерный `0` и ряд фич — запрос становится `not all`

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 27, `css/css-ruby` + `css-layout-api` + `css-box` + `motion` + `css-highlight-api` + `css-paint-api` + `css-viewport` + `mediaqueries`)
**Область:** css-parser (`crates/engine/css-parser/src/parser/media.rs` — `parse_media_clause`, `parse_media_feature`, `parse_media_length_px`)

## Симптом

Парсер условий понимает `and`, `not` в начале запроса, `min-`/`max-`/точные `width`/`height`/`aspect-ratio`/`resolution`/`orientation`/`hover`/`pointer`/`prefers-*` и длины в `px`/`em`/`rem`. Всё остальное из Media Queries 4 §2–§4 превращает весь запрос в `not all`, то есть правило молча не применяется.

## Проба

`--dump-layout`: `@media <q>{#iN{color:green}}`, 800×600; «применилось» = `getComputedStyle().color === green`; в скобках — значение `matchMedia(q).media`:

| запрос | у нас | ожидается |
|---|---|---|
| `(min-width:0px)`, `(min-width:0px) and (min-height:0px)`, `not (min-width:99999px)`, `(min-width:1em)`, `(min-width:1rem)` (контроль) | применяется | применяется |
| `(min-width: 0)`, `(min-width: 0cm)`, `(min-width: 0in)`, `(min-width: 0pt)`, `(min-width: 0vw)`, `(min-width: 0%)` | `not all` | применяется (`0` безразмерный допустим) |
| `((min-width:0px))`, `(not (min-width:99999px))`, `not (not (min-width:0px))` | `not all` | применяется |
| `(min-width:0px) or (min-height:99999px)`, `(min-width:99999px) or (min-height:0px)` | `not all` | применяется |
| `(min-width:0px) and (not (min-width:99999px))` | `not all` | применяется |
| `(width >= 0px)`, `(0px <= width)`, `(0px < width < 99999px)`, `(width = 1px)` | `not all` | применяется / нет по значению |
| `(min-width: calc(0px + 1px))`, `(min-width: calc(1rem))`, `(width > calc(1px * (1 + sign(16px - 1rem))))` | `not all` | применяется |
| `(min-width: 1ex)`, `(min-width: 1ch)`, `(min-width: 1ic)`, `(min-width: 1lh)` | `not all` | применяется (`ex`/`ch` — от начального шрифта) |
| `(width)`, `(height)`, `(aspect-ratio)` без значения | `not all` | применяется |
| `(max-aspect-ratio: 0/0)`, `(min-aspect-ratio: calc(59/79))` | `not all` | применяется |
| обратное: `not layer`, `layer`, `or`, `not or` как медиатип | принимаются (`media` не `not all`) | `not all` (`mq-invalid-media-type-002`, `-layer-001`) |
| `(monochrome)`, `(color-index)`, `(grid)`, `(scan)`, `(color-gamut)` без значения; `(color: 8)`, `(min-color: 1)`, `(monochrome: 0)`, `(color-gamut: srgb)` | `not all` | `(color-gamut: srgb)`, `(color: 8)`, `(min-color: 1)`, `(monochrome: 0)` применяются |
| `(device-width: 800px)`, `(min-device-height: 0px)`, `(device-aspect-ratio: 4/3)` | `not all` | применяется |
| `all and (Height) and (mIN-Width:0cM) and (orienTAtion:LandScape)` | `not all` | применяется (регистр не важен) |

## Как найдено

WPT-RUN-14 срез 27: `css/mediaqueries/mq-calc-*` (8), `mq-calc-sign-function-*` (5), `mq-calc-resolution`, `relative-units-003…005`, `negation-001/002`, `mq-range-001`, `mq-negative-range-*` (запрос содержит `min-color`), `mq-case-insensitive-001`, `mq-gamut-*`, `device-aspect-ratio-*`, `aspect-ratio-004`, `mq-invalid-media-type-002`, `-layer-001`. Не разобраны пробой: `min-width-tables-001`, `prefers-color-scheme-svg-as-image`, `viewport-script-dynamic`.

## Что делать

Привести `parse_media_clause` к грамматике `<media-condition>` Media Queries 4: вложенные `(<media-in-parens>)`, `and`/`or`/`not` на любом уровне, `<mf-range>` с `<`/`<=`/`=`/`>=`/`>` в трёх формах, `<mf-boolean>` для всех фич, значения `<length>` любой единицы и безразмерный `0`, `calc()`/`min()`/`max()`/`sign()` через общий вычислитель значений. Недостающие фичи: `color`, `color-index`, `monochrome`, `grid`, `scan`, `color-gamut`, `device-width`/`-height`/`-aspect-ratio`. Не путать с BUG-1497 — там `@container`, у которого свой разбор.

## Как проверить

`css/mediaqueries/mq-range-001.html`, `negation-001.html`, `mq-calc-001.html`, `relative-units-003.html`, `mq-gamut-001.html`.
