# BUG-1492 — Правила внутри `@media`/`@supports` каскадируются после ВСЕХ обычных правил листа, а не на своём месте в исходнике

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** layout (`crates/engine/layout/src/style/cascade.rs:633-679` — `next_rule_idx`, блоки `media_rules` и `supports_rules`)

## Симптом

Обычное правило, стоящее в листе ПОСЛЕ `@media all{…}` или `@supports (…){…}`, проигрывает правилу из этого блока при равной специфичности — наоборот, чем требует порядок появления (CSS Cascade 4 §6.4.3). В `cascade.rs` это записано комментарием «все @media идут после обычных — это известное ограничение», индексы блоков начинаются с `sheet.rules.len()`. `@layer` и `@container` порядок сохраняют. Порядок есть в `Stylesheet::top_level_order` (`parser.rs:255`), но каскад его не читает.

## Проба

Проба (`--mcp`, `<p id=t class=a>`, цвет `getComputedStyle(t).color`):

| лист | у нас | ожидается |
|---|---|---|
| `@media all{.a{color:red}} .a{color:green}` | `red` | `green` |
| `@supports (margin:0){.a{color:red}} .a{color:green}` | `red` | `green` |
| `.a{color:red} @media all{.a{color:blue}} .a{color:green}` | `blue` | `green` |
| `.a{color:red} @supports (margin:0){.a{color:blue}} .a{color:green}` | `blue` | `green` |
| `.a{color:green} @media all{.a{color:red}}` | `red` | `red` |
| `@layer x{.a{color:red}} .a{color:green}` | `green` | `green` |
| `@container (min-width:0px){.a{color:red}} .a{color:green}` | `green` | `green` |

То же при двух отдельных `<style>`, если первый содержит `@media`. По исходнику листов (`.tmp/s22/order.py`): в 11 reftest `css-conditional` после первого условного блока стоит обычное правило (`at-media-001`, `at-supports-001`, `at-supports-023`, `038`, `039`, `044`, `045`, `at-media-003`, `at-supports-namespace-001/002`).

## Как найдено

WPT-RUN-14 срез 22: `css-conditional/at-media-001.html`, `at-supports-001.html`. Реальные сайты затронуты шире теста: типичный `@media (min-width:…){.x{…}}` перед общим `.x{…}` в том же листе.

## Что делать

Сохранять порядок появления между `rules`, `media_rules` и `supports_rules` в каскаде: единый индекс правила из `top_level_order` вместо `next_rule_idx = sheet.rules.len()`. Учесть `cascade_index.rs` (кандидаты по блокам) и `pseudo.rs:458`.

## Как проверить

Таблица выше; `css/css-conditional/at-media-001.html`, `at-supports-001.html`.
