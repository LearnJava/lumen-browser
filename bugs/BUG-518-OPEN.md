# BUG-518: CSS Mixins `@mixin`/`@apply`/`@contents` rules not implemented at all

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** css-parser (`grep -rn "\"mixin\"\|\"apply\"\|\"contents\"\|MixinRule\|
ApplyRule\|ContentsRule" crates/engine/css-parser/src/*.rs` — zero hits.
Contrast with `@function` (CSS-SPECS.md: 🟡, `FunctionRule`
parsed+stored+evaluated end-to-end — `grep -c "function_rules\|FunctionRule"
crates/engine/css-parser/src/parser.rs` → 26 hits) — this bug is the
module's *other* at-rule family, `@mixin`/`@apply`/`@contents`, which has
none of that.)
**Найден:** WPT-RUN-3 срез 22 (`ROADMAP.md`) — массовый прогон `css/css-mixins`

## Симптом

```
FAIL CSS Mixins: Basic test
  assert_equals: expected "rgb(0, 128, 0)" but got ""
FAIL @layer (statement) is invalid in @mixin
  CSSStyleSheet is not defined
```

## Механизм

`@mixin --name { ... }` (a named block of declarations/rules) and
`@apply --name(...)` (invoking one inside a style rule) are CSS Mixins
Module Level 1's other half alongside `@function` — entirely absent from
the parser's at-rule dispatch. Every test that applies a mixin and checks
the resulting computed style gets the unset initial value instead
(`expected "rgb(0, 128, 0)" but got ""` — the mixin's declarations never
reached the cascade at all, not a wrong-value bug). `@contents` (the
mixin-body placeholder for @apply's own nested block) is the same gap one
level down. Tests that probe the CSSOM surface for these rules
additionally hit the already-open [BUG-471](BUG-471-OPEN.md)
(`CSSStyleSheet`/`CSSRule` hierarchy missing) — not a separate cause, just
a second gap the same test trips over after the first.

## Масштаб находки

15 files / ~45 subtests, all under `css/css-mixins/mixins/`: `mixin-basic`,
`mixin-conditionals` (7), `mixin-cross-stylesheet`, `mixin-cycle.tentative`,
`mixin-declarations`, `mixin-from-import(-with-media-queries)`,
`mixin-locals` (6), `mixin-parameters` (18 — the largest single file),
`apply-top-level`, `apply-within-mixin`, `contents-rule` (6),
`contents-nested-declarations(-fallback)`, `mixin-shadow-dom`,
`mixin-layers` (4, additionally needs bare-id named access —
[BUG-384](BUG-384-FIXED.md) — since `e1`/`e2`/`e3`/`e4` are read as globals),
`mixin-cssom.tentative`/`mixin-invalidation.tentative` (CSSOM surface,
[BUG-471](BUG-471-OPEN.md)). Not filing the sibling `css-mixins/functions/`
subdirectory under this bug — those 20 files test `@function` itself
(partially implemented) and fail almost entirely on already-open
[BUG-471](BUG-471-OPEN.md)/[BUG-384](BUG-384-FIXED.md) or the documented
CSS-SPECS.md T3 deferred scope (`returns` typing, conditional group rules),
not on a missing `@mixin`/`@apply`/`@contents` construct.

## Что нужно

Parse `@mixin <dashed-ident> { <declaration-list> }` and `@apply
<dashed-ident>([<argument-list>])` (mirroring the already-built
`@function`/`FunctionRule` plumbing — cascade-time lookup by name, argument
substitution) plus `@contents` as the placeholder consumed at `@apply`
call sites. Re-run `run_report.py --all --root css/css-mixins --recursive`
afterward — the `mixins/` subtree's `assert_equals(..., "rgb(0, 128,
0)")`-style checks are the fast way to confirm the fix (a mixin either
applied its declarations or it didn't, no partial-credit ambiguity).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-mixins/mixins/` for all
15 files, `expected: FAIL` per subtest.
