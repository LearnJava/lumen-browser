# BUG-1561 — CSSOM `scroll-behavior`: `getComputedStyle().scrollBehavior` — `""`, недопустимое `normal` принимается

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** css-parser/layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`, разбор `scroll-behavior`)

## Симптом

`getComputedStyle(document.body).scrollBehavior` — `""` (свойства нет в карте), `style.scrollBehavior = "normal"` принимается и читается `normal`; `CSS.supports` для значения `smooth` — `true`. Тесты `scroll-behavior-computed.html`, `scroll-behavior-invalid.html`, `inheritance.html` (2+3+2 сабтеста).

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `getComputedStyle(document.body).scrollBehavior` | `""` | `auto` |
| `d.style.scrollBehavior="normal"; d.style.scrollBehavior` | `normal` | `""` |

## Как найдено

WPT-RUN-14 срез 25: `css/cssom-view/{parsing/scroll-behavior-computed,parsing/scroll-behavior-invalid,inheritance}.html`.

## Что делать

Добавить `scroll-behavior` в карту computed (`auto`/`smooth`), отвергать значения кроме `auto | smooth`.

## Как проверить

`css/cssom-view/parsing/scroll-behavior-computed.html`.
