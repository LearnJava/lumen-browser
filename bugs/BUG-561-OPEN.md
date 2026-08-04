# BUG-561: `CSS.supports()` checks a stale property allowlist that lags behind `layout/src/style.rs` — `anchor-name`/`position-anchor`/`position-area`/`anchor-scope` report as unsupported even though they're implemented

**Статус:** OPEN
**Дата:** 2026-08-04
**Компонент:** css-parser (`crates/engine/css-parser/src/lib.rs:36` — `SUPPORTED_PROPERTIES`)
**Найден:** P2, WPT-RUN-3 срез 40 (`css/css-anchor-position`), 2026-08-04

## Симптом

```
FAIL CSS Transitions with transition-behavior:allow-discrete: property
  <anchor-name> from [--foo] to [none] at (-0.3) should be [--foo] -
  assert_true: 'from' value should be supported expected true got false
FAIL Web Animations: property <anchor-name> from [--foo] to [none] at (-0.3)
  should be [--foo] - assert_true: Web Animations should be supported
  expected true got false
```

`css/support/interpolation-testcommon.js`'s `test_interpolation` helper gates
every interpolation subtest on `CSS.supports(property, value)` before
running it (lines 396/463 of that file: `assert_true(CSS.supports(property,
from), "'from' value should be supported")`). For `anchor-name` (and by the
same mechanism `position-anchor`, `position-area`, `anchor-scope`) this
assertion fails, so every transition/animation subtest for these properties
is skipped as "not supported" — roughly 670 subtests across
`anchor-name-basics.html` and its siblings.

## Причина

`window.CSS.supports(prop, value)` (`crates/js/src/dom.rs:11641`) delegates
to the native `_lumen_css_supports_prop` binding
(`crates/js/src/dom.rs:2402-2409`), which answers purely by membership in
`lumen_css_parser::SUPPORTED_PROPERTIES` — a separate, hand-maintained
`&[&str]` array in `css-parser/src/lib.rs`, disjoint from the property
dispatch actually used by the cascade. That real dispatch lives in
`layout/src/style.rs`'s `apply_declaration`/inheritance match arms, where
`anchor-name` (line 15897), `position-anchor` (15906), `"inset-area" |
"position-area"` (15916, both spec names aliased), and `anchor-scope`
(15931) are all genuinely wired to `ComputedStyle` fields — confirmed by
reading the match arms directly, not inferred from test output. None of the
four appear in `SUPPORTED_PROPERTIES`, so `CSS.supports()` and the
`@supports` at-rule (`_lumen_css_supports_cond`, same array) both lie about
these properties, even though setting them via `element.style.anchorName =
'--foo'` and reading them back through `getPropertyValue` works.

This is a second, independent introspection-vs-implementation drift next to
[BUG-539](BUG-539-OPEN.md) (`getComputedStyle()` Proxy `has`-trap gap) — different
mechanism (a stale allowlist vs. a missing Proxy trap), same failure class:
Lumen's feature-detection surface (`CSS.supports`/`@supports`) undercounts
what the engine actually implements.

## Не проверено

Whether other already-implemented properties elsewhere in the codebase have
the same `SUPPORTED_PROPERTIES` gap (this bug is scoped to what WPT-RUN-3
srez 40 actually exercised: the four CSS Anchor Positioning properties
above).
