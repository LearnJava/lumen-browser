# BUG-1516 — CSSOM для `shape-outside`/`shape-margin`/`shape-image-threshold`, `clip-path: <basic-shape>` и свойств Text Decoration: `element.style` принимает недопустимое и не канонизирует допустимое

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 23, `css/css-text-decor` + `css-gaps` + `css-shapes`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js::_lumen_canonicalize_longhand`, валидация присваивания `element.style`)

## Симптом

`e.style.shapeOutside = "circle(123)"`, `"inset(0px, 1px)"`, `"polygon(100px)"`, `"xywh(0px 1px -2% 3em)"`, `"path()"`; `shapeMargin = "none"`/`"-20px"`; `shapeImageThreshold = "auto"`; `textDecorationLine = "auto"`; `textDecorationStyle = "groove"`; `textUnderlinePosition = "auto under"`; `textEmphasisPosition = "auto auto"`; `textShadow = "auto"`; `textDecorationThickness = "otto"` читаются назад как записаны (должны отвергаться). Допустимое не канонизируется: `0` → `0px` (`shape-margin: 0`, `inset(0 1px)`, `xywh(… round 0 1px)`, `text-decoration-inset: 0`), пропуск `center` (`circle(at 10%)` → `circle(at 10% center)`, `circle(20px at center)` → `… center center)`), порядок значений (`overline underline` → `underline overline`, `right under` → `under right`), лишние ключевые слова (`polygon(nonzero, …)` → `polygon(…)`, `polygon(round 0, …)`), `+10px` → `10px`, `calc(10in)` → `calc(960px)`, `.5px` → `0.5px`, `path('…')` → `path("…")`, цвет в `text-shadow` первым, `text-emphasis-position: right under` → `under`, `text-decoration: solid` → `none`. Тот же класс, что BUG-1297, BUG-1391, BUG-1408 (там — backgrounds, sizing, fonts/mask).

## Проба

Проба (`--mcp`, `t.style.<prop> = v; t.style.<prop>`; `gcs` — `getComputedStyle`):

| присваивание | `style` у нас | ожидается |
|---|---|---|
| `shapeOutside = "circle(123)"` | `circle(123)` | `` (отвергнуто) |
| `shapeOutside = "circle(at 50% left)"` | `circle(at 50% left)` | `` |
| `shapeOutside = "inset(0 1px)"` | `inset(0 1px)` | `inset(0px 1px)` |
| `shapeOutside = "circle(at 10%)"` | `circle(at 10%)` | `circle(at 10% center)` |
| `shapeOutside = "polygon(nonzero, 1px 2px, 3px 4px)"` | как записано | `polygon(1px 2px, 3px 4px)` |
| `shapeMargin = "none"` / `"0"` | `none` / `0` | `` / `0px` |
| `shapeImageThreshold = "auto"` / `"-100%"` | как записано | `` / `-1` |
| `textDecorationLine = "auto"` | `auto` | `` |
| `textDecorationLine = "overline underline"` | `overline underline` | `underline overline` |
| `textUnderlinePosition = "auto under"` | `auto under` | `` |
| `textEmphasisPosition = "right under"` | `right under` | `under` |
| `textDecorationSkipSpaces = "none start"` | `none start` | `` |

## Как найдено

WPT-RUN-14 срез 23: `css-shapes/parsing/*-{valid,invalid}`, `shape-functions/*-{valid,invalid}`, `shape-outside/values/*` (inline-варианты), `css-text-decor/parsing/*-{valid,invalid}`, `text-shadow/parsing/*`, `text-underline-offset-invalid`, `text-decoration-thickness-invalid`. 74 id: «shape: недопустимое принимается» 25 (529 из 529 сабтестов), «shape: сериализация» 30 (279 из 400), «text-decor: CSSOM разбор» 19 (164 из 228); всего 972 из 1 157 сабтестов.

## Что делать

Прогнать значения этих свойств через настоящие парсеры (`layout/src/style/parse/shape.rs`, `apply/text.rs`), как сделано для anchor-свойств (BUG-563): отвергать то, что не разбирается, и отдавать сериализацию из разобранного значения (CSSOM §6.7.2 «serialize a CSS value»).

## Как проверить

Таблица выше; `css/css-shapes/shape-functions/circle-function-invalid.html`, `circle-function-valid.html`, `css/css-text-decor/parsing/text-decoration-line-valid.html`, `text-underline-position-invalid.html`.
