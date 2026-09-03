# BUG-502: CSS Values and Units Level 4 §11 sign/exponential/stepped-value
`calc()` functions (`sign()`, `round()`, `mod()`, `rem()`, `abs()`) entirely
unimplemented

**Статус:** FIXED 2026-09-03
**Дата:** 2026-08-02
**Компонент:** css-parser (`crates/engine/css-parser/src/parser.rs`)
**Найден:** WPT-RUN-3 срез 10 (`ROADMAP.md`) — массовый прогон `css/css-variables`,
`variables-animation-math-functions.html`

## Механизм

`grep -n '"sign"\|"round"\|"mod"\|"rem"\|"abs"' crates/engine/css-parser/src/parser.rs`
returns zero hits for any of these `calc()` math functions (CSS Values and
Units L4 §11 — the sole `"rem"` match anywhere nearby is the unrelated
`rem` length unit suffix, not the `rem()` function). None of `sign()`,
`round()`, `mod()`, `rem()`, `abs()` have a parsing branch — a `calc()`
expression containing any of them is not a partially-wrong evaluation, it's
simply never recognised as one of these functions at all.

## Симптом

```
FAIL CSS Transitions: property <--my-angle> from [100deg] to [calc(sign(20rem - 20px) * 180deg)] -- assert_true: 'from' value should be supported expected true got false
FAIL Web Animations: property <--my-angle> from [...] to [...] -- assert_true: Web Animations should be supported expected true got false
```

`variables-animation-math-functions.html` uses `test_interpolation()`
(`/css/support/interpolation-testcommon.js`) to check that a registered
`<angle>` custom property (`@property --my-angle`) can be transitioned/
animated between two `calc(sign(...) * <angle>)` values. Every one of the
36 subtests (CSS Transitions × CSS Animations × Web Animations, ×2 value
pairs, ×6 sample points) fails at the initial "value should be supported"
guard — the `sign()` function inside `calc()` never parses, so the
`from`/`to` keyframe values are rejected outright before interpolation is
ever attempted.

## Масштаб находки

1 file / 36 subtests this slice (`css/css-variables`). Not surveyed beyond
this file — any WPT test anywhere using `sign()`/`round()`/`mod()`/`rem()`/
`abs()` inside `calc()` will hit the same gap.

## .ini

Committed `.ini` for `variables-animation-math-functions.html`,
`expected: FAIL` on all 36 subtests, header citing BUG-502. Removed on fix —
file is now fully green.

## FIXED 2026-09-03 (P3)

The original grep (`crates/engine/css-parser/src/parser/*.rs`) was misdirected
— `calc()` math-function parsing/resolution lives in
`crates/engine/layout/src/style/calc.rs`, not `css-parser`, and by the time
this bug was picked up `sign()`/`round()`/`mod()`/`rem()`/`abs()` were already
fully implemented there (`CalcNode::Func`/`MathFn`, unit-tested in
`style/tests/values.rs::sign_*`/`round_*`/`mod_*`/`rem_*`/`abs_*`) — landed by
other work on the CSS Values L4 §10 math-function family sometime after this
bug was filed, without anyone re-checking this specific report against the
new code.

The live WPT symptom was real but had a different root cause: the WPT test
uses a **registered custom property** (`@property --my-angle`), and
`interpolation-testcommon.js` gates every subtest on
`CSS.supports(property, from)` — the **two-argument** form. That form
(`_lumen_css_supports_prop`, `crates/js/src/v8_runtime/install/platform.rs`)
checks only the property name against the hardcoded `SUPPORTED_PROPERTIES`
list and had no wildcard rule for `--`-prefixed custom properties, so
`CSS.supports('--my-angle', anything)` was unconditionally `false` — same gap
class as [BUG-501](BUG-501-FIXED.md) (CSS Variables L1 §2: a custom property
accepts any value, so once a UA implements custom properties at all it must
always answer supported for one), but BUG-501's fix landed only on the
one-argument path (`SupportsCondition::evaluate`) and explicitly left the
two-argument path untouched (see BUG-501-FIXED.md: "Двухаргументная форма не
тронута — отдельный, не входящий в этот баг путь"). This bug's WPT symptom
*was* that untouched path.

Fix: `_lumen_css_supports_prop` now returns `true` immediately for any
`--`-prefixed property name, before consulting `SUPPORTED_PROPERTIES` —
mirrors the wildcard rule already applied to the one-argument evaluator. The
pre-existing test `css_supports_two_arg_unknown_property` encoded the old
(spec-incorrect) behavior (`CSS.supports('--custom-var', '1')` expected
`false`) and was corrected to `css_supports_two_arg_custom_property_always_true`
(now expects `true`, plus the literal `--my-angle`/`calc(sign(...))` pair from
this bug's WPT test); `css_supports_two_arg_unknown_property` was renamed to
`css_supports_two_arg_unknown_standard_property` and now asserts on a
genuinely non-existent standard property name instead.

Live WPT verification (`tests/wpt/run_smoke.py`, fresh `dev-release` build):
`/css/css-variables/variables-animation-math-functions.html` — **36/36
subtests pass** (was 0/36). `.ini` removed (fully green, nothing left to
pin). Gates: `cargo test -p lumen-js --features v8-backend css_supports`
15/15, `cargo clippy -p lumen-js --features v8-backend --all-targets -- -D
warnings` clean.
