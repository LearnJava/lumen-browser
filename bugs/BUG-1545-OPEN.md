# BUG-1545 — `baseline-shift` не сдвигает текст HTML-элемента (`<span>`): `<length-percentage>`, `sub`, `top`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout (`crates/engine/layout/src/box_tree/` — `baseline-shift` подключён только к SVG-тексту, `emit_svg_text`)

## Симптом

`<span style="baseline-shift:10px">HH</span>` в строке рисуется на той же высоте, что и без свойства; `vertical-align:10px` поднимает текст. То же для `baseline-shift: sub`, `50%`, `top`. `CSS.supports('baseline-shift','top')` — `true`, значение хранится, но вне SVG не используется (строка `CSS-SPECS.md` про `baseline-shift` ✅ относится только к SVG-тексту). 8 reftest в кластере `baseline-shift/*` (`top`, `bottom`, `center`, `length-percentage` × HTML и `-svg`): HTML-варианты этим дефектом объяснены пробой, `*-svg.html` не разобраны (значение для SVG-текста применяется, причина падения не установлена).

## Проба

Проба (`--screenshot`, `font:20px/20px Arial`, диапазон строк с чернилами по `y`):

| стиль на `<span>` | у нас | ожидается |
|---|---|---|
| — | `1…15` | `1…15` |
| `vertical-align:10px` | `0…5` | — |
| `baseline-shift:10px` | `1…15` | `0…5` |
| `baseline-shift:sub` | `1…15` | сдвиг вниз |
| `baseline-shift:50%` | `1…15` | сдвиг вверх |


## Как найдено

WPT-RUN-14 срез 24: `css-inline/baseline-shift/baseline-shift-{top,bottom,center,length-percentage}.html` (HTML-варианты — проба; `-svg.html` — не разобраны).

## Что делать

Применять `baseline-shift` в inline-раскладке HTML-текста (как `vertical-align` для длин и процентов; `top`/`bottom`/`center` — относительно строки), а не только в `emit_svg_text`.

## Как проверить

`css/css-inline/baseline-shift/baseline-shift-length-percentage.html`.
