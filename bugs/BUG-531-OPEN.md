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
