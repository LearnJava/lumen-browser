# BUG-1508 — `@mixin`: условные группы в теле и `@result`, определение с недопустимым списком параметров не игнорируется

**Статус:** FIXED 2026-10-23 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** css-parser/layout (`crates/engine/css-parser/src/parser/mixins.rs`, `crates/engine/layout/src/style/substitute.rs` — `expand_mixin_apply`)

## Симптом

`@mixin --m(){@result{color:red;@media all{color:green}}}` — `@media` внутри `@result` не раскрывается (получаем `red`); то же для `@media` прямо в теле миксина. `@mixin --m(junk-in-parameter-list){…}` (недопустимый список параметров) должно игнорироваться целиком — у нас заменяет первое определение `--m` (получаем `red` вместо `green`). Остальное работает: `@result`, параметры и значения по умолчанию, локальные `--x`, `@contents`, вложенный `@apply`.

## Проба

Проба (`--mcp`, `p{color:red;@apply --m;}`):

| определение | у нас | ожидается |
|---|---|---|
| `@mixin --m(){@result{color:red;@media all{color:green}}}` | `red` | `green` |
| `@mixin --m(){@media all{color:green}}` | `red` | `green` |
| `@mixin --m(){@result{color:green}}` + `@mixin --m(junk){@result{color:red}}` | `red` | `green` |
| `@mixin --m(--c:green){@result{color:var(--c)}}` | `green` | `green` |
| `@mixin --m(){@result{@contents}}`, `@apply --m{color:green}` | `green` | `green` |

## Как найдено

WPT-RUN-14 срез 22: `css-mixins/mixins/*` — 13 id, 44 из 66 сабтестов (`mixin-parameters` 12/24, `mixin-conditionals` 6/7, `mixin-media-query-invalidation*`, `mixin-from-import-with-media-queries`, `apply-nested-declarations`, `mixin-parsing` 14/14 — последний ещё и CSSOM).

## Что делать

Условные группы в `@result` и теле миксина (общий код с `@function`, BUG-1507); недопустимое определение — пропускать; инвалидация при смене `@media`.

## Как проверить

Таблица выше; `css/css-mixins/mixins/mixin-parameters.html`, `mixin-conditionals.html`.

## Исправление

`crates/engine/css-parser/src/parser/at_rules.rs`: `open_conditional_group` — общий разбор `@media`/`@supports` для тел `@function` и `@mixin`. `mixins.rs`: `MixinRule.locals` — `Vec<FunctionBodyItem>` (локальные `--x:` с условными группами в порядке источника: поздний голый локал перекрывает ранний условный), несколько `@result` склеиваются, группа вокруг `@result` и `@media` внутри `@result` — `MixinResultItem::Conditional` (с исходным текстом заголовка для `cssText`); один недопустимый параметр (`junk`, пустой сегмент в списке) отбрасывает весь `@mixin`, поэтому прежнее определение остаётся в силе. `layout/src/style/substitute.rs` (`expand_apply_rule`, `expand_mixin_result_items`): активные группы определяются по `MediaContext` вьюпорта на момент `@apply`. Тесты: `mixin_conditional_groups_wrap_locals_and_result`, `mixin_with_invalid_parameter_list_is_dropped` (css-parser), `css_mixin_media_inside_result_applies_when_it_holds`, `css_mixin_groups_wrap_result_and_locals_in_source_order`, `css_mixin_invalid_parameter_list_keeps_the_earlier_definition` (layout).

Остаток (функциональные пробелы): `@container` вокруг локалов и `@result` (`mixin-conditionals.html` m2/m4/m6) — не раскрывается; вложенное правило стиля внутри условной группы `@result` не материализуется; голые не-`--` объявления в теле миксина вне `@result` игнорируются по спецификации (вторая строка пробы выше ожидалась ошибочно); типизированные параметры `@mixin` (`type(<length>)`) не приводятся к вычисленному виду; CSSOM `mixin-parsing` — отдельно.
