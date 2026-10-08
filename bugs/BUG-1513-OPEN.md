# BUG-1513 — `@import` применяется в любой позиции листа, хотя после любого правила, кроме `@charset`/`@layer`-оператора/`@import`, он недопустим

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** css-parser (`crates/engine/css-parser/src/parser.rs` — разбор `@import` и `@namespace` в листе)

## Симптом

CSS Cascade 4 §2.2 (`@import`) и CSS Namespaces 3: `@import` должен стоять перед всеми правилами, кроме `@charset`, `@layer`-оператора (`@layer a;`) и других `@import`; `@namespace` — после `@import`/`@charset`, но перед остальными. У нас импортируемый лист применяется, где бы `@import` ни стоял.

## Проба

Проба (`--dump-layout`, `imp.css` = `p{color:red !important}`; `p` получает `color=#ff0000`, если импорт применён; ожидается «применён» только в первой строке):

| лист | у нас | ожидается |
|---|---|---|
| `@import "imp.css";p{margin:0}` | применён | применён |
| `@media all{}@import "imp.css";p{margin:0}` | применён | **не** применён |
| `p{margin:0}@import "imp.css";` | применён | **не** применён |
| `@supports (color:red){}@import "imp.css";p{margin:0}` | применён | **не** применён |
| `@layer a{}@import "imp.css";p{margin:0}` | применён | **не** применён |
| `@namespace x "…";@import "imp.css";p{margin:0}` | применён | **не** применён (`@import` после `@namespace`) |

## Как найдено

WPT-RUN-14 срез 22: `css-conditional/at-media-003.html`, `at-supports-045.html` (на странице остаётся красная `support/fail.css` — вся область 800×600 красная) — 2 reftest `thick`. `at-supports-namespace-001/002` (`@namespace y` после `@supports`) сюда не входят: префиксы пространств имён не реализованы вовсе — BUG-1454, в кластер `cond-ns`.

## Что делать

В разборе верхнего уровня помнить, было ли уже правило, запрещающее `@import`/`@namespace`, и отбрасывать их после него.

## Как проверить

Таблица выше; `css/css-conditional/at-media-003.html`, `at-supports-045.html`.
