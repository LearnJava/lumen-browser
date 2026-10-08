# BUG-1426 — Разбор цвета: комментарии между токенами, `none`, смешение `<number>`/`<percentage>` в `rgb()`, «голая» альфа в `hsl()` принимаются или отвергаются неверно

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout/css-parser (`crates/engine/layout/src/style/parse/color.rs`)

## Симптом

`background-color:<значение>` поверх `#ff0000`, пиксель (`--screenshot`); красный = декларация отброшена:

| значение | результат | ожидается |
|---|---|---|
| `hsl(120/* c */75%/* c */50%)`, `hsl(120/* c */75%/* c */50%/1.0)`, `hsl(120,/* c */75%,/* c */50%,1.0)` | красный | валидно |
| `rgb(0 /*c*/ 128 0)` | красный | валидно |
| `rgb(none 128 0)`, `rgb(0 128 0 / none)` | красный | валидно (`none` = 0) |
| `hsl(none 100% 25%)`, `hwb(none 0% 50%)` | красный | валидно |
| `color-mix(in hsl, hsl(none 50% 50%), blue)` | красный | валидно |
| `rgb(255, 0, 0%)` | `(255,0,0)` | **невалидно** (смесь) |
| `rgb(100%, 1, 0)`, `rgb(0, 0%, 0%)`, `rgb(254, 255%, 0)` | рисуются | **невалидно** |
| `hsl(120 75% 50% 0.5)` | `(143,239,143)` | **невалидно** (альфа только через `/` или запятую) |
| `rgb(0, 128, 0, 0.5)` (контроль) | `(127,191,127)` | валидно |

## Как найдено

WPT-RUN-14 срез 19: `css-color/background-color-{hsl,rgb}-00{1,2}.html` (комментарии), `t421/t422-…-no-mixed-f.xht` (смесь),
`t425-hsla-basic-a.xht`, `relative-currentcolor-hsl-02.html` (`hsl(none 100% 25%)`), `parsing/color-invalid-{hwb,rgb,hsl}.html`,
`color-valid-{rgb,hsl,hwb}.html`, `alpha-color-parsing-valid.html`.

## Что делать

В токенизаторе цвета пропускать комментарии; ввести `none` как «отсутствующую компоненту» для legacy- и modern-синтаксиса;
проверять однородность числа/процента у запятого `rgb()`; требовать `/` у modern-синтаксиса с альфой.

## Как проверить

`css/css-color/background-color-hsl-002.html`, `t421-rgb-func-no-mixed-f.xht`, `parsing/color-invalid-hwb.html`.
