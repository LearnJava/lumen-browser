# BUG-501: `CSS.supports()` declaration-form gaps — one-arg form missing the
spec-mandated "wrap in parens and retry" fallback, and custom properties
never recognised as supported in either form

**Статус:** FIXED 2026-09-03
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs:11645`, `CSS.supports`) + css-parser
(`crates/engine/css-parser/src/parser.rs:1323`, `SupportsCondition::evaluate`,
and `SUPPORTED_PROPERTIES` in `lib.rs:36`)
**Найден:** WPT-RUN-3 срез 10 (`ROADMAP.md`) — массовый прогон `css/css-variables`,
`revert-rule-in-fallback.html`/`revert-rule-to-var.html`

## Механизм

Two independent, compounding gaps in the same feature:

**1. One-arg form skips the CSSOM-mandated reparse-with-parens fallback.**
Per the CSSOM spec (`CSS.supports(conditionText)`): try parsing/evaluating
`conditionText` as a `<supports-condition>` directly; if that fails, retry
after wrapping it as `(conditionText)`. `CSS.supports`'s JS shim
(`dom.rs:11645`) does neither — it passes the raw string straight to
`_lumen_css_supports_cond` → `parse_supports_condition` (`parser.rs:1931`),
which never adds the fallback retry. Per the `<supports-decl>` grammar
(`<supports-decl> = ( <declaration> )`), a bare `prop: value` with no
wrapping parens is **not** a valid `<supports-condition>` at all, so it
parses to `SupportsCondition::Unknown`, which always evaluates to `false`.

**2. Custom properties are never "supported", parens or not.**
`SupportsCondition::Decl::evaluate` (`parser.rs:1325`) checks the property
name against `SUPPORTED_PROPERTIES` (`css-parser/src/lib.rs:36`), a
hand-written list of real standard property names. A `--`-prefixed custom
property name is never in that list (nor is there a wildcard rule for the
`--` prefix), so `CSS.supports('(--x: revert-rule)')` — correctly
parenthesised — still returns `false`. Per CSS Variables L1 §2, a custom
property's value can be *any* token sequence; `@supports (--x: anything)`
must always be considered supported once a UA implements custom properties
at all.

Confirmed live (`--mcp-port`):

```js
CSS.supports("margin:revert-rule")     // → false (gap 1: no parens, falls to Unknown)
CSS.supports("(margin:revert-rule)")   // → true  (parens present — property-name check alone succeeds)
CSS.supports("--x:revert-rule")        // → false (gap 1)
CSS.supports("(--x:revert-rule)")      // → false (gap 2 — parens present, still fails: custom prop not in SUPPORTED_PROPERTIES)
CSS.supports("margin", "revert-rule")  // → true  (two-arg form: value argument is ignored entirely, only property-name matters)
```

## Симптом

```
FAIL var(--unknown, revert-rule) in custom property -- assert_true: expected true got false
FAIL var(--unknown, revert-rule) in shorthand -- assert_true: expected true got false
FAIL var(--unknown, revert-rule) in shorthand observed via longhand -- assert_true: expected true got false
FAIL var(--unknown, revert-rule) in longhand -- assert_true: expected true got false
```

All four subtests of `revert-rule-in-fallback.html` fail at their leading
`assert_true(CSS.supports(...))` guard — even the three that test *known*
properties (`margin`, `margin-left`, `padding-left`, which would each
individually satisfy gap 2 since they're real property names) still fail
because of gap 1 (no parens in the WPT-authored condition text). The
subsequent `getComputedStyle` assertions in each `test()` callback are never
reached — masked, same pattern as BUG-384's masking elsewhere in this
slice. `revert-rule-to-var.html`'s single subtest fails identically at
`assert_true(CSS.supports('color:revert-rule'))` (gap 1 alone, since `color`
is a known property) — its `getComputedStyle(target)` line (bare identifier,
[BUG-384](BUG-384-FIXED.md) territory) is never reached either.

## Масштаб находки

2 files / 5 subtests this slice (`css/css-variables`). Both gaps are
independent of the `revert-rule` keyword specifically — any WPT test using
the common one-arg `CSS.supports('prop:value')` idiom without explicit
parens, or testing `@supports` on a custom property, hits this; scope
beyond `css/css-variables` unmeasured.

## .ini

Committed `.ini` for both files, `expected: FAIL` on all 5 subtests, header
citing BUG-501 as the observed cause (BUG-384 noted as a second, currently-
invisible layer in `revert-rule-to-var.html`'s single test).

## Срез 30 (`css/css-conditional`, 2026-08-03) — largest extension, gap 1 alone this time

164 files, all `container-queries/*.html`, all whole-file `ERROR`/0
subtests: `support/cq-testcommon.js`'s
`assert_implements_size_container_queries()`/
`assert_implements_scroll_state_container_queries()` guard every test with
`assert_implements(CSS.supports("container-type:size"))` /
`CSS.supports("container-type:scroll-state")` — a bare `prop:value` string,
no wrapping parens. `container-type` itself is recognized (`SUPPORTED_
PROPERTIES` in `crates/engine/css-parser/src/lib.rs` lists it, and
`ContainerType::Size` exists end-to-end in `layout/src/style.rs`), so this
is gap 1 in isolation — `parse_supports_atom`
(`crates/engine/css-parser/src/parser.rs:2107`) requires the input to start
with `(` and returns `SupportsCondition::Unknown` (→ `false`) otherwise,
never reaching the property-name check at all. 110 files fail on the
`size` guard, 54 on the `scroll-state` guard. (Note added at fix time:
`Decl::evaluate` only ever checks the *property name* — "container-type"
— never the value, so this guard's prediction was backwards: it does
NOT still fail after gap 1's fix, because `SUPPORTED_PROPERTIES`
containing `container-type` is enough for `Decl::evaluate` regardless of
which value follows the colon. That value-blind Phase 0 check is
pre-existing behaviour, not part of this bug's two gaps — a
`container-type: scroll-state` container query will still not actually
*lay out* on scroll-state per BUGS.md's own scroll-state coverage,
`CSS.supports()` merely stops gating the test on it.)
`.ini` under `tests/wpt/metadata/css/css-conditional/container-queries/`
(top-level `expected: ERROR`, no subtest section — none ran).

## Фикс 2026-09-03 (P3)

**Gap 1** (`crates/js/src/v8_runtime/install/platform.rs`,
`_lumen_css_supports_cond`): one-arg form now tries the raw condition
text first, and — only if that evaluates false — retries with the text
wrapped in parens (`format!("({condition})")`), matching CSSOM
`supports(conditionText)`'s "parse as `<supports-condition>`, else parse
`(conditionText)`" fallback. Because a syntactically valid multi-term
condition (`(a) and (b)`) parses identically whether or not it is
additionally wrapped, the retry is a no-op for input that already worked
— only the bare `prop:value` idiom is newly accepted.

**Gap 2** (`crates/engine/css-parser/src/parser/at_rules.rs`,
`SupportsCondition::evaluate`): a `Decl` whose property name starts with
`--` now evaluates `true` unconditionally, before the `SUPPORTED_
PROPERTIES` lookup — CSS Variables L1 §2 requires custom properties to
be considered supported for any value once a UA implements them at all,
and Lumen's cascade does. This is shared by the real `@supports` at-rule
(`cascade_index.rs:126`) as well as `CSS.supports()`, so `@supports
(--x: y) { … }` style rules now apply too — a correctness fix beyond the
JS-facing symptom, not just a workaround for it.

The two-argument form (`CSS.supports(property, value)`) is untouched —
it never routes through `SupportsCondition::evaluate` at all (a separate
name-only lookup against `SUPPORTED_PROPERTIES`), and BUG-501 never
flagged it (its ignoring of `value` and of custom-property names in that
form is a distinct, undocumented Phase 0 simplification, not one of this
bug's two gaps).

Regression tests: `at_supports_evaluate_custom_property_always_supported`/
`_in_combinators` (`css-parser/src/parser/tests/at_rules.rs`),
`css_supports_one_arg_bare_declaration_no_parens`/
`_unknown_property_still_false`/`_custom_property_parenthesized`/
`_custom_property_bare` (`crates/js/src/dom/tests/v8_css_storage_nav_misc.rs`).
`cargo test -p lumen-css-parser` (359/359) and `cargo test -p lumen-js
--features v8-backend css_supports` (12/12) both green; both crates'
`clippy --all-targets -- -D warnings` clean.

Scope not touched: the value-blind `Decl::evaluate` property-name-only
check (pre-existing Phase 0 design, see note above on the `scroll-state`
guard) and the two-arg form's parallel gaps — neither is one of this
bug's two flagged gaps.
