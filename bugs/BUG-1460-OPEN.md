# BUG-1460 — Условные групповые правила не вкладываются друг в друга на верхнем уровне: `@media{@layer}`, `@media{@media}`, `@media{@supports}`, `@supports{@media}`, `@supports{@layer}` отбрасывают всё содержимое

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** css-parser (`crates/engine/css-parser/src/parser/` — вложенные at-правила внутри `@media`/`@supports`/`@layer`)

## Симптом

Внутри `@media`/`@supports` вложенное at-правило (`@layer`, `@media`, `@supports`, `@container`, `@scope`) приводит к тому, что стили внутри не применяются вовсе. Обратная вложенность (`@layer{@media}`, `@layer{@supports}`, `@layer{@layer}`, `@container{@layer}`) работает. Для `@media{…}` с обычными правилами внутри всё верно. Страдают `css-cascade/layer-media-query.html` (8 сабтестов, TIMEOUT), `scope-media.html`, `scope-supports.html`, `scope-layer.html` и страницы, оборачивающие `@layer` в `@media` или `@supports` (типичный приём фреймворков). `@scope(.r){ :scope{…} }`, `@scope(.r){ &{…} }` и голые декларации внутри `@scope` тоже не применяются (в `@scope(.r){ i{…} }` — работает) — смежное, может быть тем же разбором (`scope-deep`, `scope-specificity`, `scope-proximity`, `at-scope-parsing`: 17 из 43 сабтестов).

## Проба

`<style>⟨CSS⟩</style><div class=r><i class=k id=y></i></div>`, `getComputedStyle(y).color`, ожидается зелёный (правило `.k{color:green}` внутри):

| вложенность | у нас |
|---|---|
| `@media all{ .k{…} }` | зелёный |
| `@layer a{ @media all{ .k{…} } }` | зелёный |
| `@layer a{ @supports (color:red){ .k{…} } }`, `@layer a{ @layer b{ .k{…} } }` | зелёный |
| `@container (min-width:0px){ @layer a{ .k{…} } }` | зелёный |
| `@media all{ @layer a{ .k{…} } }` | **чёрный** |
| `@media all{ @media all{ .k{…} } }` | **чёрный** |
| `@media all{ @supports (color:red){ .k{…} } }` | **чёрный** |
| `@supports (color:red){ @layer a{ .k{…} } }`, `@supports (color:red){ @media all{ .k{…} } }` | **чёрный** |
| `@layer a{ @container (min-width:0px){ .k{…} } }`, `@media all{ @container (min-width:0px){ .k{…} } }` | **чёрный** |
| `@media all{ @scope(body){ #x{…} } }`, `@layer a{ @scope(body){ #x{…} } }` | **чёрный** |
| `@scope(.r){ :scope{…} }` на `<div class=r id=x>`, `@scope(.r){ &{…} }` | **чёрный** |
| `@scope(.r){ i{…} }` | зелёный |

## Как найдено

WPT-RUN-14 срез 20: `css-cascade/layer-media-query.html` (TIMEOUT, 8 сабтестов), `scope-media.html`, `scope-supports.html`, `scope-layer.html`.

## Что делать

В разборе тела `@media`/`@supports`/`@container`/`@layer`/`@scope` допускать любые вложенные at-правила (рекурсивно, один и тот же разбор «списка правил»). Для `@scope`: `:scope` и `&` внутри — корень области; голые декларации внутри — правило для корня (`@scope` §3).

## Как проверить

Таблица выше; `css/css-cascade/layer-media-query.html`, `css/css-cascade/scope-media.html`.

## Повторное измерение: WPT-RUN-14 срез 22 (2026-10-08)

`css-conditional`: `at-supports-002`, `at-supports-003`, `at-supports-023`, `css-supports-025.xht`, `css-supports-026.xht`, `css-supports-046.xht` — 6 reftest `thick`, все — `@media{@supports}` или `@supports{@media}` (проба: оба порядка и `@media{@media}`, `@supports{@supports}` дают `black`; `@layer{@supports}` — верно). Кластер `cond-nesting` в `docs/wpt-vendor-notes/css.md` §css-view-transitions + css-conditional + …
