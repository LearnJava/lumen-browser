# BUG-1567 — `list-style-type: "<string>"` и `symbols(...)` не рисуются: маркер — число `1`, `2`; `getComputedStyle().listStyleType` — `""`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** layout (`crates/engine/layout/src/style/values/misc.rs::ListStyleType::parse`, `counters.rs::build_list_marker_text`)

## Симптом

`list-style-type: "→ "`, `list-style: "★"`, `list-style-type: symbols(cyclic "X" "Y")` разбираются как неизвестный счётчик и дают десятичный маркер `1`, `2`. 6 reftest `css-lists/list-style-type-string-*`, 12 id `list-style-*-valid/computed`.

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `<ol style="list-style-type:'→ '"><li>…` — маркер | `1` | `→ ` |
| `<ul style="list-style:'★'"><li>…` — маркер | `1` | `★` |
| `<ul style="list-style-type:symbols(cyclic 'X' 'Y')"><li>a<li>b` — маркеры | `1`, `2` | `X`, `Y` |

## Как найдено

WPT-RUN-14 срез 25: `css/css-lists/list-style-type-string-{001a,001b,002,003,005a,005b,006,007}.html`.

## Что делать

Добавить `ListStyleType::String` и `Symbols` в разбор и `build_list_marker_text`.

## Как проверить

`css/css-lists/list-style-type-string-001a.html`.
