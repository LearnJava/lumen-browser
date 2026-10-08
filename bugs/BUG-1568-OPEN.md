# BUG-1568 — Счётчики: `counter(list-item)` в `<ol><li>` — `0`, `counter-reset: reversed(name)` не реализован, `display:none` элемент увеличивает счётчик

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** layout (`crates/engine/layout/src/counters.rs::precompute_counters`)

## Симптом

CSS Lists 3 §4: (а) `li` не получает неявный `counter-increment: list-item`, поэтому `counter(list-item)` — `0` (`counter-set:list-item 9` и `counter-increment:list-item 5` работают); (б) `counter-reset: reversed(foo)` не разбирается как обратный счётчик — `counter(foo)` при `counter-increment: foo -1` — `0`, `0`, `0` (ожидалось `3`, `2`, `1`); (в) `display:none; counter-increment: x 10` меняет счётчик (`11` вместо `1`), `display: contents` — не проверено на всех путях; (г) `counter-reset: x` на `::before`/`::after` — BUG-1368. 19 reftest + 13 id `li-counter-other`.

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `<ol><li>` — `li::before{content:counter(list-item)}` | `0` | `1`, `2` |
| `ol{counter-reset:reversed(foo)} li::before{counter-increment:foo -1;content:counter(foo)}` при трёх `li` | `0.` ×3 | `3.`, `2.`, `1.` |
| `<div style="counter-reset:x 1"><div style="display:none;counter-increment:x 10"></div><div class=p>` — `.p::before{content:counter(x)}` | `11` | `1` |
| `.b::before{counter-increment:x;content:counter(x)}` при `counter-reset:x 5` | `5`, `5` | `6`, `7` |

## Как найдено

WPT-RUN-14 срез 25: `css/css-lists/{counter-*,counters-*,counter-reset-reversed-*,foo-counter-reversed-*,li-value-reversed-*,implicit-and-explicit-list-item-counters}.html`.

## Что делать

Неявный `counter-increment: list-item` у `display: list-item`; `reversed()` (CSS Lists 3 §4.4.1); пропускать `display:none` в `precompute_counters`; вместе с BUG-1368 и BUG-1547.

## Как проверить

`css/css-lists/counter-reset-reversed-nested.html`, `counter-reset-increment-set-display-none.html`.
