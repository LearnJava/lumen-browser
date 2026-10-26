# BUG-1495 — `@font-face` внутри `@media` и `@supports` отбрасывается (в `@layer` — нет)

**Статус:** FIXED 2026-10-23 (P3)
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

## Исправление

- Парсер (`at_rules.rs`, `supports.rs`): в теле `@media`/`@supports` верхнего уровня `@font-face` больше не пропускается — `parse_conditional_body_at_rule` разбирает его, кладёт в `FontFaceRule::conditions` условие группы (`LayerCondition`) и поднимает на верхний уровень листа через `Parser::bubbled` (порядок среди безусловных `@font-face` сохраняется).
- Условие нельзя решить при разборе (зависит от viewport), поэтому правило хранит его, а `Stylesheet::retain_active_font_faces(&MediaContext)` отбрасывает ложные до загрузки шрифтов. Вызывается там, где viewport известен: `page_pipeline.rs` (страница), `frames/spawn.rs` (iframe), `relayout.rs` (пересборка листа), `driver/session.rs` (ранний снимок `document.fonts`).
- Тесты: `font_face_inside_media_is_hoisted_with_condition`, `font_face_inside_supports_is_hoisted_with_condition`, `retain_active_font_faces_drops_false_media`, `at_rule_after_font_face_in_media_still_skipped_cleanly`; сквозные в `crates/driver/tests/cases/fontload3_document_fonts_population.rs` (`document.fonts.size` при истинном и ложном условии).
- Остаток (`@keyframes`/`@property`/`@counter-style`/вложенный `@media` в теле условной группы) — [BUG-1686](BUG-1686-FIXED.md). `at-media-content-003`/`at-supports-content-003` (`@keyframes`) под этот баг не попадали и не изменились.
