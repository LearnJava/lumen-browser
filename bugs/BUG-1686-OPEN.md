# BUG-1686 — `@keyframes`, `@property`, `@counter-style` и вложенный `@media`/`@supports` внутри тела `@media`/`@supports` отбрасываются

**Статус:** OPEN
**Заведён:** 2026-10-23 (P3, остаток [BUG-1495](BUG-1495-FIXED.md))
**Область:** css-parser (`crates/engine/css-parser/src/parser/at_rules.rs` `parse_media_rule`, `parse_conditional_body_at_rule`; `supports.rs` `parse_supports_rule`)

## Симптом

Тело `@media`/`@supports` верхнего уровня разбирает только style-правила и `@font-face` (BUG-1495). Любое другое `@`-правило уходит в `skip_at_rule`: `@media all{@keyframes k{…}}` не регистрирует `k`, `@property`/`@counter-style` под условием не действуют, а вложенный `@media`/`@supports` в теле `@media`/`@supports` теряется вместе со своими style-правилами (в отличие от вложения в style-правило, где CSS Nesting работает).

## Что делать

Тем же приёмом, что `@font-face` в BUG-1495: хранить условие группы в правиле (`conditions: Vec<LayerCondition>`), поднимать через `Parser::bubbled`, а потребителей (`animation.rs`, `counters.rs`, `cascade.rs`, `custom_flow.rs`, `shell/animation_scheduler.rs`, `js/.../stylesheets.rs`) фильтровать по контексту. Вложенные условные группы — склеивать условия по AND. Учесть, что `at-media-content-003`/`at-supports-content-003` красные и с верхнеуровневым `@keyframes` (см. BUG-1495, «Симптом»): причина там не в `@media`.

## Как проверить

`@media all{@keyframes k{from{color:red}to{color:blue}}}` + `animation: k 1s` — `getAnimations()` непуст; `css/css-conditional/at-media-content-003.html`.
