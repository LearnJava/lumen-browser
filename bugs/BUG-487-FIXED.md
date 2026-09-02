# BUG-487: `revert-rule` CSS-wide keyword not implemented

**Статус:** FIXED 2026-09-02
**Дата:** 2026-08-02
**Компонент:** css-parser (no `revert-rule` handling found anywhere in
`crates/engine/css-parser/src` or `crates/engine/layout/src`)
**Найден:** WPT-RUN-3 срез 6 (`ROADMAP.md`) — массовый прогон `css/css-cascade`

## Механизм

`revert-rule` (CSS Cascade L5 §`revert-rule`) is a CSS-wide keyword,
alongside `initial`/`inherit`/`unset`/`revert`/`revert-layer`, that rolls a
declaration back to the value it would have had from the previous matching
rule in cascade order (as opposed to `revert`, which rolls back to the next
*origin*, or `revert-layer`, which rolls back to the next *layer*). Grepped
`css-parser` and `layout` for `revert-rule`/`RevertRule`/`revert_rule`: zero
hits in either crate. `revert-layer` (a sibling, more established keyword)
**is** implemented and passes its own dedicated tests elsewhere in this
slice — so this is specifically the `revert-rule` value that's absent, not
the whole revert-family mechanism.

Practically: a declaration using `revert-rule` is presumably parsed as an
invalid value (or silently dropped) rather than being recognized as the
CSS-wide keyword it is, so the declaration never rolls back to the prior
rule — it just fails to apply at all, leaving whatever value the *next*
still-valid rule in cascade order provides (often the property's initial
value, which is why every observed failure is `assert_true: expected true
got false`, not a parse error surfaced to JS).

## Симптом

```
FAIL revert-rule in a custom property | assert_true: expected true got false
FAIL The revert-rule keyword rolls back to the previous rule | assert_true: expected true got false
FAIL Cascade order determines the previous rule, not order of application | assert_true: expected true got false
FAIL The revert-rule keyword can cross layers | assert_true: expected true got false
FAIL Combination of revert-rule and revert-layer | assert_true: expected true got false
```

## Масштаб находки

**4 files / 12 subtests cleanly attributable** in this slice — every
subtest's failure is exactly `assert_true: expected true got false` with no
other message shape, confirming `revert-rule` simply never rolls anything
back: `revert-rule-basic.html` (4), `revert-rule-custom-property.html` (1),
`revert-rule-layer.html` (2), `revert-rule-revert-layer.html` (5).

Two more files are **mixed** — `revert-rule` failures alongside unrelated
[BUG-384](BUG-384-FIXED.md) (named access on Window) failures in the same
file:
- `revert-rule-important.html`: 1 subtest is this bug (`assert_true`), 2 are
  BUG-384 (`test2`/`test3 is not defined`).
- `revert-rule-shadow.html`: 1 subtest is this bug (`assert_true`), 11 are
  BUG-384 (`slotted2`/`host3`…`host12 is not defined`).

## Что нужно

Recognize `revert-rule` as a CSS-wide keyword in `css-parser`'s value
grammar (same class of token as the already-handled `revert`/`revert-layer`)
and, in the cascade resolution in `layout`'s style computation, implement
its rollback semantics: walk cascade order (not origin, not layer) back to
the nearest earlier declaration for the same property that isn't itself
`revert-rule`, and use that value (or the property's initial value if none
exists). The existing `revert-layer` implementation is the closest
reference point for how origin/layer-scoped rollback is already wired.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-cascade/` for all 6
files (4 pure + 2 mixed, `expected: FAIL` on the whole file since the mixed
files have zero passing subtests either way).

## Срез 10 (`css/css-variables`, 2026-08-02)

Two more files exercise `revert-rule` (`revert-rule-in-fallback.html`,
`revert-rule-to-var.html`, `var(--unknown, revert-rule)` as a `var()`
fallback rather than a direct declaration value) but their observed
failures this slice are entirely masked by
[BUG-501](BUG-501-OPEN.md) (`CSS.supports()` rejects the unparenthesized
one-arg condition text `revert-rule` is guarded behind, before any
`getComputedStyle` assertion runs). Whether `revert-rule` itself works
correctly as a `var()` fallback once BUG-501 is fixed is unmeasured — given
this bug's own finding that `revert-rule` has zero implementation anywhere,
the working assumption is these two files will keep failing (now on a
`getComputedStyle` mismatch instead of `assert_true`) until this bug is
fixed too. `.ini` for both files attributes to BUG-501 (the directly
observed cause); re-check against this bug once BUG-501 lands.

## Срез 19 (`css/css-nesting`, 2026-08-03)

`nesting-revert-rule.html` — 4/4 subtests, `assert_true: expected true got
false` on all of them, same clean signature as the Срез 6 finding — the
CSS Nesting-specific cases (`revert-rule` reverting to a nested rule, to/from
a `CSSNestedDeclarationsRule`, to scoped declarations) confirm the gap is
independent of whether the prior rule in cascade order came from ordinary
nesting or a dedicated nested-declarations rule. `.ini`:
`tests/wpt/metadata/css/css-nesting/nesting-revert-rule.html.ini`.

## Срез 21 (`css/css-env`, 2026-08-03)

`env-revert-rule.html` — 1/1 subtest, same clean `assert_true: expected true
got false` signature, this time testing `revert-rule` as an `env()`
fallback value (`background-color: env(test, revert-rule)`) rather than a
direct declaration or a `var()` fallback — a third distinct syntactic
position for the same missing keyword. `.ini`:
`tests/wpt/metadata/css/css-env/env-revert-rule.html.ini`.

## Срез 2026-09-02 (P3) — cascade-level fix landed

`revert-rule` is, like its sibling `revert-layer`, intentionally NOT a
`CssWideKeyword` (it depends on which *rule* the winning declaration came
from, so it cannot be applied per-declaration). It is now resolved in
`crates/engine/layout/src/style/cascade.rs` by folding it into the existing
`revert-layer` pre-pass over the cascade-sorted `matched` set: the two loops
were merged into one that, each round, finds the winning declaration for
every property and — if its value is `revert-layer` — drops every
declaration of that property from the winning *layer* (existing behavior),
or — if `revert-rule` — drops every declaration of that property from the
winning *rule* (new; grouped by the same `rule_idx`/`gidx` already tracked
per `matched` entry, which an inline `style=""` attribute shares as a single
synthetic index across all its declarations). The loop repeats because
resolving one keyword can reveal the *other* as the new winner
(`revert-rule-revert-layer.html` chains both in both directions) — a single
non-interleaved pass would leave a literal `revert-layer`/`revert-rule`
stuck as the applied value. `color.rs::canonical_specified_color` and
`font_size.rs::parse_font_shorthand`'s CSS-wide-keyword guard list also
gained `revert-rule` alongside the existing `revert`/`revert-layer`, for
consistency (setProperty round-trip / `font` shorthand not misreading the
keyword as a value).

Verified with 6 new unit tests in
`crates/engine/layout/src/style/tests/cascade.rs` (`revert_rule_*`) mirroring
the WPT assertions directly against the production cascade code path:
basic rollback to the previous rule, cascade-order-not-appearance-order,
a 3-deep `revert-rule` chain, custom properties, `!important` interaction,
and chaining into `revert-layer`. `cargo test -p lumen-layout` — lib unit
suite 3658/3658 (3652 pre-existing + 6 new), `cases` integration suite
77/77, no failures anywhere; `cargo clippy -p lumen-layout --all-targets --
-D warnings` clean.

Closes the 6 files/14 subtests originally attributed to this bug plus the
4-subtest `css/css-nesting` slice-19 finding (18 subtests across
`revert-rule-basic.html`, `revert-rule-custom-property.html`,
`revert-rule-important.html` (all 3 — the earlier "mixed with BUG-384" read
no longer applies now that BUG-384 is fixed; re-reading the live test file
shows no unrelated named-access assertions), `revert-rule-layer.html`,
`revert-rule-revert-layer.html`, `nesting-revert-rule.html`) — their `.ini`
expectation overrides are deleted.

**Not closed by this slice** (kept as `.ini` FAIL, separate mechanisms):
`revert-rule-shadow.html` (12 subtests — Shadow DOM `:host`/`::slotted`/
`::part()` cascade specifics are unverified against this fix and shadow
trees have independent, unrelated gaps — slotting doesn't work at all,
BUG-876/877/878 — that would mask several of these regardless);
`revert-rule-in-fallback.html`/`revert-rule-to-var.html` (`var()` fallback,
still masked by BUG-501 per the original finding); `env-revert-rule.html`,
`attr-revert-rule.html`, `if-function-revert-rule.html` (the keyword only
appears inside `env()`/`attr()`/`if()`, i.e. after substitution — the
cascade-level `matched`-array prepass here operates on the raw, unsubstituted
`decl.value` and never sees it); `revert-rule-keyframes.html`/
`revert-rule-keyframes-dynamic.html` (`revert-rule` as a `@keyframes` value
resolves through the animation engine's own "underlying value" mechanism,
not the style cascade at all). A follow-up bug for the var()/env()/attr()/
if()/keyframes-substitution paths, if wanted, is a fresh finding — this
bug's own scope (declaration-level `revert-rule`) is fully closed.
