# BUG-1410 — `font-weight`/`font-style`/`font-stretch`: дробный вес и `calc()` читаются как 400, `oblique <angle>` теряет угол, `lighter`/`bolder` считаются неверно

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 18, `css/css-fonts` + `css/css-masking` + `css/WOFF2`)
**Область:** layout (`crates/engine/layout/src/style/apply/text.rs`, `style/parse/font_size.rs` — разбор `font`; `selector_query.rs` — сериализация)

## Симптом

`el.style.X = v; getComputedStyle(el).X`:

| значение | получено | ожидается |
|---|---|---|
| `fontWeight = '150.25'` | `400` | `150.25` |
| `fontWeight = 'calc(100 + 200)'` | `400` | `300` |
| `fontWeight = '1001'` / `'0'` / `'-1'` | `400` (в `style` остаётся записанным) | значение отвергается |
| `fontWeight = 'bolder'` у родителя 901 / `lighter` у 99 | `700` | цепочка таблицы CSS Fonts L4 §2.2.1 |
| `fontStyle = 'oblique 45deg'` | `oblique` | `oblique 45deg` |
| `fontStyle = 'oblique 0deg'` | `oblique` | `normal` |
| `fontStretch = '50%'` | `50%` | `50%` |
| `fontStretch = '0.5'` | принимается, читается `100%` | отвергается (число без `%` недопустимо) |
| `style.font = 'italic bold 12px/30px Georgia, serif'`, `getComputedStyle().font` | `""` | `""` (шорткод не выражается — по CSSOM) / `style.font` — каноническая строка |

## Как найдено

WPT-RUN-14 срез 18: `css-fonts/parsing/{font-weight-computed,font-style-computed,font-style-valid}.html`,
`font-weight-lighter-bolder.html` (2 из 22), `font-stretch.html` (19 из 51), `font-style-parsing.html` (25 из 43),
`font-weight-parsing.html` (14 из 26), `variations/font-parse-numeric-stretch-style-weight.html` (37 из 81),
`variations/font-shorthand.html` (18 из 22), `font-weight-sign-function.html`, `font-style-sign-function.html`,
`font-style-angle.html`.

## Что делать

Хранить вес как число `[1, 1000]` с дробной частью и считать `calc()`; для `oblique` хранить угол; применить таблицу
`lighter`/`bolder`; отвергать числа вне диапазона при разборе.

## Как проверить

`css/css-fonts/parsing/font-weight-computed.html`, `font-weight-lighter-bolder.html`, `font-style-computed.html`.
