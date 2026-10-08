# BUG-1522 — `text-underline-offset`: `em`, `%` и `calc()` не работают (`1em` считается от 16 px, `25%` и `calc(…)` — как `auto`), `from-font` и `<length-percentage>` в одном значении не различаются

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 23, `css/css-text-decor` + `css-gaps` + `css-shapes`)
**Область:** css-parser/layout (`crates/engine/layout/src/style/apply/text.rs:902` — `text-underline-offset`: `Option<f32>`, `parse_length_px`)

## Симптом

Значение хранится как `Option<f32>` в px и разбирается через `parse_length_px` без контекста элемента: `1em` разрешается от 16 px (корень), а не от `font-size` элемента; `25%`, `calc(5px + 5px)`, `from-font` дают `None` (`auto`).

## Проба

Проба (`--screenshot`, `font:40px/1.5 Arial`, `<u style="text-decoration-skip-ink:none;…">xyz</u>`, сравнение снимков):

| значения | результат |
|---|---|
| `text-underline-offset:25%` и `auto` | **совпадают** (ожидается: как `10px`) |
| `calc(5px + 5px)` и `auto` | **совпадают** (ожидается: как `10px`) |
| `1em` и `40px` | различаются (ожидается: совпадают) |
| `text-decoration-thickness:50%` и `20px`; `calc(2px + 2px)` и `4px` | совпадают (верно) |

`gcs(t).textUnderlineOffset` при `font-size:40px; text-underline-offset:1em` — `16px`.

## Как найдено

WPT-RUN-14 срез 23: `text-underline-offset-calc.html`, `-percentage.html`, `-variable.html`, `-zero-position.html`, `-002.html` (5 reftest) и сабтесты `text-underline-offset-computed` (3 из 15). Остальные 15 reftest `text-decoration-thickness-*` и 4 `text-underline-offset-*` падают по другим причинам — «не разобрано».

## Что делать

Хранить `text-underline-offset` как `LengthPercentage`/`calc` (по образцу `TextDecorationThickness`), разрешать от `font-size` элемента на этапе вычисления, в `push_text_decoration` — `%` от `1em` (CSS Text Decoration 4 §3.3).

## Как проверить

Таблица выше; `css/css-text-decor/text-underline-offset-percentage.html`, `text-underline-offset-calc.html`.
