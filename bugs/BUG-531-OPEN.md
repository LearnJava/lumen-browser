# BUG-531: `CSS.registerProperty()` never validates the `syntax`/`initialValue` descriptors — no `SyntaxError` is ever thrown

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/css_properties_values_api.rs:95-137` — `CSS_PROPERTIES_VALUES_SHIM`,
shared by both engines via `install_css_properties_values_api`/`install_css_properties_values_api_v8`)
**Найден:** P2, WPT-RUN-3 срез 25 (`css/css-properties-values-api/register-property-syntax-parsing.html`,
129/246 subtests unexpected — прочитан код после чтения текста провалов)

## Симптом

```js
CSS.registerProperty({name: '--x', syntax: '<color', initialValue: 'red', inherits: false});
// spec: must throw SyntaxError (unterminated data-type name) — Lumen: no-op, silently accepts
```

`register-property-syntax-parsing.html` pairs `assert_valid(syntax, value)` (should not throw)
with `assert_invalid(syntax, value)` (`assert_throws_dom("SyntaxError", …)`) across the full
CSS Values and Units §Syntax Strings grammar (data-type names, `|` combinators, `+`/`#`
multipliers, `<custom-ident>` restrictions, quoting). 129/246 pass (all/most `assert_valid`
cases, since nothing throwing is correct there by coincidence) and 117/246 fail (the
`assert_invalid` cases, since `registerProperty()` never throws for anything with a
syntactically well-formed `name`).

## Причина

`CSS.registerProperty` (`css_properties_values_api.rs:95`) only validates `definition` is an
object and `name` starts with `--`. The `syntax` descriptor (line 110,
`const syntax = definition.syntax || '*';`) and `initialValue` (line 112) are taken verbatim
with no grammar check whatsoever — no parser call, no regex, nothing that could produce a
`SyntaxError`. The Rust-side registry (`RegisteredPropertiesMap`) likewise stores `syntax`/
`initial_value` as opaque `String`s (no validation in `register()`, `css_properties_values_api.rs:23`).
This is distinct from the `@property` **at-rule** parser
(`crates/engine/css-parser/src/parser.rs:3505` `parse_at_property_body`), which does reject
malformed bodies at the CSS-syntax level (invalid at-rule structure) — the gap is specifically
in the **syntax-string micro-grammar** validation (data type names, combinators) that both the
at-rule parser and the JS API skip, and specifically in the JS API path never validating
`initialValue` against `syntax` at all (e.g. `syntax: "<color>", initialValue: "notacolor"`
should throw and doesn't).

## Влияние

Any registered custom property with a malformed `syntax` descriptor is silently accepted
instead of rejected, and any `initialValue` that doesn't match its own `syntax` is silently
accepted instead of rejected — both are required validation steps per
[the spec's "register a custom property" algorithm](https://drafts.css-houdini.org/css-properties-values-api-1/#register-a-custom-property).
Downstream effect on cascade correctness is untested here (this file only checks whether
`registerProperty()` itself throws), but a property that should have failed registration and
instead succeeds will behave as `syntax: "*"` in the shim's actual runtime substitution path
(untyped) while claiming a typed syntax via `CSS._getRegisteredProperties()`/CSSOM — a
correctness gap for any code branching on that reported syntax.

## .ini

`tests/wpt/metadata/css/css-properties-values-api/register-property-syntax-parsing.html.ini`
— `expected: FAIL` on the ~117 `assert_invalid`-derived subtests (exact list needs a second
pass through the file's ~120 `assert_invalid(...)` call sites, not enumerated here).

## Срез P3 2026-09-08

Реализована грамматика syntax-строки и сопоставление значения с ней в новом модуле
`crate::style::syntax_string` (`crates/engine/layout/src/style/syntax_string.rs`,
`lumen-layout`), с новым native-байндингом `_lumen_validate_registered_property`
(`crates/js/src/css_properties_values_api.rs`), который `CSS.registerProperty()` в JS-шиме
теперь зовёт перед сохранением дескриптора и по его ответу бросает
`DOMException('SyntaxError')`.

Покрыто (проверено юнит-тестами `syntax_string::tests` + `dom::tests::
v8_css_storage_nav_misc::css_register_property_*`, и вручную протрассировано почти
построчно по тексту `register-property-syntax-parsing.html`):
- полная грамматика syntax-строки: `<type>` (15 типов), литералы (включая dashed-идент и
  CSS `\XX`-escape через собственный `decode_ident_token`, зеркалящий CSS Syntax §4.3.7/4.3.9),
  `|`-union, `+`/`#`-множители (включая запрет `<transform-list>+`/`<transform-list>#` и
  двойных множителей), запрет CSS-wide keyword / `default` как имени компонента, запрет `*`
  в комбинации с чем-либо ещё;
- сопоставление значения: `<length>`/`<percentage>`/`<length-percentage>` (через существующий
  `parse_length`+calc), `<color>`/`<image>` (плюс грубая, нерекурсивная поддержка
  `light-dark(a, b)` — просто распознаёт форму с двумя аргументами), `<url>`,
  `<integer>`/`<number>`, `<angle>`/`<time>`/`<resolution>` (с отклонением отрицательного
  resolution — `-5.3dpcm` теперь invalid), `<transform-function>`/`<transform-list>`
  строго all-or-nothing (не молча пропускает невалидную функцию, как
  `parse_transform_list`), `<custom-ident>`/`<string>` (собственный string-token чекер:
  bad-string на непойманный line break, незакрытая строка на EOF — валидна);
- universal `*`: собственный сканер CSS Syntax §9.1 `<declaration-value>` (сбалансированность
  скобок, bad-string, bad-url внутри `url(...)`, top-level `;`/`!`, запрет `var()`/`env()`);
- CSS-wide keyword (`initial`/`inherit`/`unset`/`revert`/`revert-layer`) как значение — invalid
  для любого syntax, включая universal (5 WPT-кейсов, которые сам тест называет "not clearly
  backed by the spec, but a good idea");
- computational independence инициального значения: `em`/`ex`/`ch`/`rem` — invalid именно как
  `initialValue` (нет элемента, относительно которого их резолвить), но **не** для обычной
  declaration на существующем элементе через `@property`/cascade
  (`property_syntax::validate_against_syntax` эту проверку сознательно не делает — иначе
  ломается уже проходящий тест `property_syntax_union_length_or_percentage`, где
  `--w: 10rem;` обязан приниматься).

**Не сделано / известный остаточный пробел:** typed `calc()` для `<number>`/`<integer>`/
`<angle>`/`<time>`/`<resolution>` — существующий calc-движок (`style::calc`) типизирован
только под `Length`, поэтому `calc(1 / 2)`, `calc(3.1415)`, `calc(50grad + 3.14159rad)`,
`calc(2s - 9ms)` остаются invalid, хотя тест ожидает valid (5 строк). Реальный прогон через
WPT-harness в этом срезе не переснят — фикс проверен только юнит-тестами и ручной трассировкой,
точный итоговый счёт `assert_valid`/`assert_invalid` и `.ini` с оставшимся списком — следующий
шаг для того, кто продолжит этот файл.
