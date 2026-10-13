# BUG-1495 — `@font-face` внутри `@media` и `@supports` отбрасывается (в `@layer` — нет)

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** css-parser (`crates/engine/css-parser/src/parser/at_rules.rs:547` `@font-face`; тело `@media`/`@supports`)

## Симптом

Верхнеуровневый `@font-face` и `@font-face` в `@layer` регистрируют шрифт (`document.fonts.size` = 1); тот же `@font-face` внутри `@media all{…}` или `@supports (color:red){…}` — нет (`document.fonts.size` = 0), потому что разбор тела условного блока не поднимает ему не-стилевые at-правила (в `@layer` это сделано: `nested: Vec<AtRuleOutcome>`). `@keyframes` в тех же блоках этой пробой не проверялся: `at-media-content-003`/`at-supports-content-003` (`@keyframes` в условном блоке, `animation: green 4s both`) красные и с верхнеуровневым `@keyframes` (`--screenshot` страницы с `animation: green 4s both` и `@keyframes` без `@media` — красный квадрат; в `--mcp` `getAnimations()` пуст и через 1,2 с, а `element.animate()` работает), так что причина там не `@media`; что именно не стартует в этих двух путях — не установлено.

## Проба

Проба (`--mcp`, `document.fonts.size`):

| лист | у нас | ожидается |
|---|---|---|
| `@font-face{font-family:zz;src:local(Arial)}` | 1 | 1 |
| `@layer a{@font-face{…}}` | 1 | 1 |
| `@media all{@font-face{…}}` | **0** | 1 |
| `@supports (color:red){@font-face{…}}` | **0** | 1 |

## Как найдено

WPT-RUN-14 срез 22: `css-conditional/at-media-content-002.html`, `at-supports-content-002.html` (2 reftest `thick`); `at-media-content-003.html`, `at-supports-content-003.html` — те же с `@keyframes`, причина другая (см. выше), под этот баг не попадают.

## Что делать

Поднять `@font-face` (и, после проверки, `@keyframes`, `@property`, `@counter-style`) из тела `@media`/`@supports` на верхний уровень листа при активном условии — так же, как это делает `@layer`.

## Как проверить

Таблица выше; `css/css-conditional/at-media-content-002.html`.
