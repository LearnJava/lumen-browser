# BUG-502: CSS Values and Units Level 4 §11 sign/exponential/stepped-value
`calc()` functions (`sign()`, `round()`, `mod()`, `rem()`, `abs()`) entirely
unimplemented

**Статус:** OPEN
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
`expected: FAIL` on all 36 subtests, header citing BUG-502.
