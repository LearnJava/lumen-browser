# BUG-1332 — `text-indent: <длина> each-line` и `hanging` разбираются, но не применяются

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 10, `css/css-text`, вторая половина)
**Область:** layout (`crates/engine/layout/src/box_tree/inline_wrap.rs` — выбор отступа строки; `style/apply/text.rs` — ключевые слова `each-line`/`hanging` не доходят до `ComputedStyle`)

## Симптом

`width: 110px; font: 20px monospace`, `x` слов `aa bb cc dd` (по строкам, перечислены `x` первых слов):

| `text-indent` | получено | ожидается |
|---|---|---|
| `40px` | `40 68 / 0 28` | верно |
| `40px each-line` (с `<br>` и словом `ee`) | `0 28 58 / 0 / 0` | 40 в начале первой строки и каждой после `<br>` |
| `40px hanging` | `0 28 58 / 0` | первая строка 0, остальные 40 |
| `40px hanging each-line` | `0 28 58 / 0 / 0` | `hanging` + каждая после `<br>` |

`getComputedStyle().textIndent` для `10px each-line` — `0px` (значение отвергнуто целиком), хотя `style.textIndent` читается обратно.

## Как найдено

WPT-RUN-14 срез 10, `css/css-text/text-indent/` (18 не зелёных id из 28): прямо про ключевые слова — `text-indent-each-line-hanging.html`, `-dynamic-hanging-002.html`, `-dynamic-each-line-002.html`, `-abspos-hanging-001.html`. Остальные 14 (`below-float*`, `text-indent-percentage-001…004`, `text-indent-length-*`, `anonymous-grid-item-001`, `text-indent-min-max-content-001`, `percentage-value-intrinsic-size`, `text-indent-tab-positions-001`) не разобраны: `text-indent: 50%` на блоке 400 px в пробе верен (200).

## Что делать

Разбор `<length-percentage> && hanging? && each-line?` в `ComputedStyle`; в раскладке — отступ первой строки (или всех, кроме первой, при `hanging`) и строк после принудительного разрыва при `each-line`.

## Как проверить

`css/css-text/text-indent/text-indent-each-line-hanging.html`.
