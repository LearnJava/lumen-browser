# BUG-1547 — `<ol>`: маркер по умолчанию — `disc`, атрибуты `start`, `reversed`, `<li value>` и `type` не действуют, `li.value` — пустая строка

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** layout (`crates/engine/layout/src/style/ua.rs`, `crates/engine/layout/src/box_tree/inline_build.rs::li_ordinal`, `crates/engine/layout/src/counters.rs`), js (`HTMLLIElement.value`)

## Симптом

У `<ol>` нет UA-правила `list-style-type: decimal` (комментарий в `graphic_tests/32-list-markers.html`: «no UA stylesheet in Lumen»): `<ol><li>a<li>b</ol>` рисует точки. С явным `list-style-type: decimal` нумерация идёт `1.`, `2.`, но `<ol start=5>` даёт `1.`, `<ol reversed>` — `1.`, `2.`, `3.`, `<li value=7>` — `1.`: номер берёт `li_ordinal` (позиция среди `li`-соседей, `crates/engine/layout/src/box_tree/inline_build.rs:1374`), а не счётчик `list-item`. `<ol type="a">` игнорируется; вложенные `ul` тоже рисуются `disc` (по таблице — `circle`, `square`). 39 reftest + 5 testharness в `css-lists`, 124 reftest `css-counter-styles` используют `<ol start>`.

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `<ol><li>x<li>y</ol>` под `--screenshot` | маркер — точка | `1.`, `2.` |
| `<ol start=5><li>a<li>b</ol>`, `list-style-type:decimal` — маркеры | `1.`, `2.` | `5.`, `6.` |
| `<ol reversed><li>…×3</ol>` — маркеры | `1.`, `2.`, `3.` | `3.`, `2.`, `1.` |
| `<ol><li value=7>a<li>b</ol>` — маркеры | `1.`, `2.` | `7.`, `8.` |
| `<ol type=a>` — маркер | точка | `a.` |
| `li.value` и `getComputedStyle(li).listStyleType` у `<li>` в `<ol>` | `""` и `""` | `1` (номер) и `decimal` |
| `<ol start=5><li>` — `counter(list-item)` в `li::before` | `0` | `5` |

## Как найдено

WPT-RUN-14 срез 25: `css/css-lists/counter-reset-reversed-*` (36), `counter-list-item*`, `li-value-reversed-*`, `foo-counter-reversed-*`, `css-counter-styles/*/css3-counter-styles-NNN` (`<ol start=…>`, 124 id).

## Что делать

UA-таблица HTML Rendering §15.3.7: `ol { list-style-type: decimal }` (`ul` — `disc`, вложенные `ul` — `circle`/`square`); `<li>` — `counter-increment: list-item`; `ol`/`ul` — `counter-reset: list-item` с учётом `start` (`counter-set` для `start`, `reversed()` для `reversed` — CSS Lists 3 §4.4.2 и HTML LS §15.3.7), `<li value>` — `counter-set: list-item <value>`, `<ol type>` — `list-style-type` (`1`/`a`/`A`/`i`/`I`). Реверс — вместе с BUG-1568 (`counter-reset: reversed(name)`). `HTMLLIElement.value` — число (позиция в списке).

## Как проверить

`css/css-lists/counter-reset-reversed-list-item.html`, `css/css-counter-styles/lower-roman/css3-counter-styles-019.html` (после BUG-1546, BUG-1548); `<ol start=5>` рисует `5.`.
