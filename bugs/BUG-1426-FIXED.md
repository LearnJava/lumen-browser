# BUG-1426 — Разбор цвета: комментарии между токенами, `none`, смешение `<number>`/`<percentage>` в `rgb()`, «голая» альфа в `hsl()` принимаются или отвергаются неверно

**Статус:** FIXED 2026-10-21 (P3)
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

## Как исправлено

- `style/parse/color.rs::strip_comments`: `/* … */` внутри значения заменяется пробелом на входе `parse_color`,
  `parse_css_color_legacy`, `parse_color_function`, `canonical_specified_color` и `color_serialize::computed_color_css`
  (комментарий между токенами — разделитель; без комментариев строка не копируется).
- `none` принимают разборщики каналов `rgb()`/`hsl()`/`hwb()`/`lab()`/`lch()`/`oklab()`/`oklch()` и alpha (канал = 0,
  alpha = 0); в legacy-синтаксисе с запятыми (`rgb(none, …)`, `rgba(…, none)`, `hsl(120, 75%, none)`) отвергается.
  `color(srgb none 0.5 0)` остаётся валидным — по CSS Color 4 §10.1 это корректная запись.
- `split_color_args`: в современном синтаксисе alpha только через `/` — ровно 3 канала до слэша и один токен после;
  `hsl(120 75% 50% 0.5)`, `rgb(0 0 / 0.5)`, `rgb(0 0 0 / 0.5 0.5)` невалидны. `hwb()` с запятыми невалиден.
- Смесь числа и процента в запятом `rgb()` была отвергнута раньше (`legacy_rgb_rejects_mixed_percent_and_number`);
  добавлены регрессионные проверки на формы из заявки.
- Заодно (WPT `color-valid-hsl/hwb`): в современном `hsl()`/`hwb()` s/l/w/b бывают голыми числами (`hsl(120 30 50)`).
- Не сделано: `calc()` внутри `rgb()`/`hsl()`/`hwb()`/`lab()` (`calc(infinity)`, `calc(NaN)`, `sign(1em - 10px)`) — разбор
  каналов режет тело по пробелам и `/` и `calc()` не вычисляет; осталось в `color-valid-{rgb,hsl,hwb,lab}.html`
  (~90 из 317 кейсов). Перенос missing-компонент (`color-mix(in hsl, hsl(none 50% 50%), blue)`: `none` как 0, а не
  «унаследовать от второго цвета» по CSS Color 4 §12.2) — результат смешения может отличаться от Chrome.
- Прогон по WPT `css/css-color/parsing` через `canonical_specified_color`: `color-invalid-{rgb,hsl,hwb}` — 59/59 отвергнуты.
- Тесты: `style::tests::color::{color_function_comments_are_token_separators, color_none_keyword_is_modern_syntax_only,
  color_function_alpha_needs_slash_in_modern_syntax, hsl_hwb_modern_syntax_accepts_bare_numbers,
  cascade_accepts_color_functions_with_comments_and_none}`.
