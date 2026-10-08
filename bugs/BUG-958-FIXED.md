# BUG-958 — `CSS.supports(conditionText)` (one-argument, no parens) всегда `false`, даже для реально поддержанного свойства

**Статус:** FIXED 2026-09-28 (P6, обнаружено при разборе очереди STATUS-P6)
**Тип:** дефект реализованного кода — `parse_supports_atom` (css-parser) требует ведущую `(` и не реализует запасной путь спецификации (парсинг строки как «голого» `<declaration>`).
**Заведён:** 2026-09-02 (WPT-RUN-6, срез 35, живая проба через `--mcp-live-port`)
**Область:** css-parser (`crates/engine/css-parser/src/parser/at_rules.rs::parse_supports_atom`) / js (`crates/js/src/v8_runtime/install/platform.rs::_lumen_css_supports_cond`)
**Владелец:** был P3.

## Симптом

`CSS.supports("writing-mode: horizontal-tb")` — однoаргументная форма без
оборачивающих скобок — отвечала `false`, хотя движок это свойство
действительно поддерживает. Живая проба на том же документе:

```js
CSS.supports("writing-mode: horizontal-tb")    // false  ← баг (было)
CSS.supports("(writing-mode: horizontal-tb)")  // true   ← корректно
CSS.supports("writing-mode", "horizontal-tb")  // true   ← корректно (двухаргументная форма)
```

Только «голая» (без скобок) однoаргументная форма была затронута; двухаргументная
и скобочная формы работали верно и тогда.

## Причина (описание на момент завода)

`parse_supports_condition` → `parse_supports_expr` → `parse_supports_term` →
`parse_supports_atom`. `parse_supports_atom` разбирает `font-tech()`/`font-format()`/`selector()`,
а затем — только если следующий байт `(` — заходит в общую ветку
condition/declaration. Если строка не начинается с `(`, функция долетает до
последней строки и возвращает `SupportsCondition::Unknown`, у которого
`.evaluate()` жёстко `false`.

## Почему закрывается без правки кода

Дефект уже устранён коммитом `402b808fb89` («BUG-501: CSS.supports() —
фоллбек скобок + custom properties», 2026-09-03) — **на день позже завода
BUG-958**, но независимо от него: BUG-501 фиксировал ту же CSSOM-грамматику
(one-arg fallback) как часть более широкой ревизии `CSS.supports`, не
ссылаясь на BUG-958 и не будучи закрыт как его дубликат. Фикс лежит не в
`parse_supports_atom` (парсер как был — не принимает голую декларацию без
скобок), а на уровне JS-биндинга `_lumen_css_supports_cond`
(`crates/js/src/v8_runtime/install/platform.rs:517-528`): условие сначала
пробуется как есть, и при `SupportsCondition::Unknown`/`false` — повторно
оборачивается в `(…)` и пробуется снова. Это реализует тот же CSSOM
запасной путь, что требовался в описании починки BUG-958, только на другом
уровне архитектуры (JS-обёртка вместо самого парсера).

## Верификация 2026-09-28

Юнит-тесты `crates/js/src/dom/tests/v8_css_storage_nav_misc.rs` уже
покрывают ровно сценарий бага (заведены при закрытии BUG-501):

```
cargo test -p lumen-js --features v8-backend --lib css_supports_one_arg
```

```
running 8 tests
test dom::tests::v8_css_storage_nav_misc::css_supports_one_arg_bare_declaration_unknown_property_still_false ... ok
test dom::tests::v8_css_storage_nav_misc::css_supports_one_arg_and_condition ... ok
test dom::tests::v8_css_storage_nav_misc::css_supports_one_arg_or_with_unknown ... ok
test dom::tests::v8_css_storage_nav_misc::css_supports_one_arg_custom_property_bare ... ok
test dom::tests::v8_css_storage_nav_misc::css_supports_one_arg_bare_declaration_no_parens ... ok
test dom::tests::v8_css_storage_nav_misc::css_supports_one_arg_unknown_property ... ok
test dom::tests::v8_css_storage_nav_misc::css_supports_one_arg_custom_property_parenthesized ... ok
test dom::tests::v8_css_storage_nav_misc::css_supports_one_arg_known_property ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 4579 filtered out; finished in 0.50s
```

`css_supports_one_arg_bare_declaration_no_parens` — прямая репродукция
симптома бага (`CSS.supports('display:grid')` → `true`, `CSS.supports('margin: 1px')`
→ `true`). Дополнительно прогнаны все 23 теста `at_supports_*` в
`crates/engine/css-parser/src/parser/tests/at_rules.rs` — зелёные (парсер на
уровне css-parser не менялся, фоллбек живёт в JS-биндинге).

## Отличие от направления починки в исходном описании

Исходное описание требовало правку `parse_supports_atom`/`parse_supports_condition`
в самом css-parser. Фактический фикс BUG-501 решил задачу на уровень выше — в
JS-биндинге, двойным вызовом `parse_supports_condition` (прямая строка, затем
обёрнутая в скобки). Наблюдаемое поведение (`CSS.supports(conditionText)`)
идентично требуемому спекой, поэтому это закрытие, а не дубликат:
`parse_supports_atom` сам по себе строже спеки, но вызывающая сторона
(единственный потребитель этой грамматики в движке) компенсирует это
ровно так, как требовала CSSOM.
