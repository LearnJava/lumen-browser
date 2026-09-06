# BUG-1010: A custom property's own computed value never resolves `attr()`/`--fn()`/`@apply` — only `var()`/`env()` do

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/src/style/substitute.rs::expand_vars_and_env`, consumed by `crates/engine/layout/src/lib.rs::collect_custom_properties_rec` — the channel `getComputedStyle().getPropertyValue('--x')` actually reads, per [BUG-499](BUG-499-FIXED.md)'s fix)
**Найден:** P3 2026-09-06, investigating [BUG-519](BUG-519-OPEN.md)

## Симптом

For any custom property whose declared value is (or contains) `attr()`, a custom `--fn()` function call, or is produced by `@apply`'ing a mixin's `@result` block, `getComputedStyle(el).getPropertyValue('--name')` returns the **literal, unexpanded source text** instead of the resolved value — confirmed at the `ComputedStyle` level with a throwaway unit test:

```rust
let s = cascade_at(
    "<div class=\"box\"></div>",
    "@function --double(--x) { result: calc(var(--x) * 2); } \
     .box { --gap: --double(10px); width: var(--gap); }",
    &[0],
);
// s.custom_props.get("--gap") == Some("--double(10px)")   — raw, unresolved
// s.width                     == Some(Calc(Length(Px(20.0))))  — correctly resolved
```

`width` (a real typed property referencing `--gap` via `var()`) resolves correctly, because the property-application pipeline re-expands `var()`/`attr()`/`--fn()` at the point of use. `--gap` itself never does — its own stored value is exactly the raw declaration text.

Plain `var()`/`env()` chains between custom properties are **not** affected — `--a: var(--b); --b: 10px` does resolve `--a` to `"10px"` when read back, per [BUG-499](BUG-499-FIXED.md)'s verification. Only `attr()`, `--fn()`, and mixin-produced (`@apply`) values are affected.

## Механизм

Two independent code paths read a custom property's value, and only one of them expands anything:

1. **`ComputedStyle::custom_props`** (`crates/engine/layout/src/style/cascade.rs:1219-1231`, the "Custom-properties pass"): for every `--name: value` declaration, inserts `decl.value.clone()` verbatim — no substitution at all, not even `var()`. This is deliberate and correct *for this pass's own purpose*: it exists so a later declaration on the same element can see the raw text via `var()` regardless of source order, and `apply_declaration`'s own per-property pipeline (further down the same cascade loop) re-resolves `var()`/`attr()`/`--fn()`/`@apply` freshly at each point of use against this raw map. That later pipeline (`cascade.rs:1385-1428`) computes a fully-expanded `effective_decl` for `--`-prefixed declarations too — but `apply_declaration` (`crates/engine/layout/src/style/apply.rs`) has no branch for `--`-prefixed properties at all, so the expanded value is silently discarded and never written back into `custom_props`.

2. **`collect_custom_properties_rec`** (`crates/engine/layout/src/lib.rs:1623-1663`, published to JS as the dedicated custom-property snapshot per [BUG-732](BUG-732-FIXED.md)/[BUG-499](BUG-499-FIXED.md) — `getComputedStyle().getPropertyValue('--x')` reads *this*, not `computed_style_to_map`): calls `expand_vars_and_env` once per raw custom property. That function (`substitute.rs:31-47`) only conditionally expands `var(` and, after that, `env(` — it has no knowledge of `attr()`, `--fn()` calls, or the `@apply` marker, so any of those pass through completely untouched.

Neither path is "wrong" in isolation — each does exactly what its own docstring says. The gap is that **no path resolves a custom property's own `attr()`/`--fn()`/`@apply`-derived value**, even though the exact same constructs resolve correctly when a *different*, typed property references that custom property via `var()`.

## Масштаб

This is the load-bearing mechanism behind the entire vendored `css-mixins` WPT category's test methodology: every file under `tests/wpt/css/css-mixins/{mixins,functions}/` (and likely much of `css/css-values`' `attr()` coverage) uses the `--actual`/`--expected` custom-property pattern (`tests/wpt/css/css-mixins/resources/utils.js::test_all_templates`) and reads the result via exactly the `getComputedStyle().getPropertyValue('--actual')` channel this bug breaks. Concretely reproduced for:

- `--x: --f();` (direct custom-function call)
- `--x: attr(data-x type(*));` (typed `attr()`)
- `@apply --mixin;` when the mixin's `@result` block sets a **custom** property (a real, typed property in the same `@result` block, e.g. `margin-left`, already resolves correctly — every existing `@mixin`/`@function` unit test in `crates/engine/layout/src/style/tests/values.rs` checks a typed property for exactly this reason, never a custom one)

This means [BUG-518](BUG-518-OPEN.md)'s slices ("expected effect on the vendored category once re-triaged: `mixin-basic.html` ... should now go green") are almost certainly **too optimistic** for any test whose observable is `--actual`/`--expected` rather than a typed property — no slice on that track re-verified through this exact channel (all of them note "No live WPT run"). Re-triage of that whole category should wait on this bug, not the other way around.

## Что нужно

`collect_custom_properties_rec` needs the same `attr()`/`--fn()`/`@apply` expansion `apply_declaration`'s pipeline already does for typed properties, threaded through with the right context: `functions`/`mixins`/`layer_order` come from the `Stylesheet` (plus shadow-tree overlays per [BUG-1009](BUG-1009-FIXED.md)/[BUG-518](BUG-518-OPEN.md) срез 5 — `collect_custom_properties_rec` currently only has `LayoutBox`/`viewport`, no `Document`/`sheet` at all), and `attr()` needs the owning DOM node. This is a real plumbing change (new parameters through `collect_custom_properties`'s public signature and every caller in `crates/shell`), not a one-line fix — likely its own slice. `expand_vars_and_env`'s docstring/name should also change to reflect the wider scope once it does more than `var`/`env`.

A minimal, narrower alternative: make the *cascade-time* `custom_props` pass (`cascade.rs:1219-1231`) itself resolve `attr()`/`--fn()`/`@apply` before inserting (not just `var()`, which it doesn't do either) — that would fix `getComputedStyle` for free by making `collect_custom_properties_rec`'s existing `expand_vars_and_env` a no-op re-application of already-resolved text, but requires solving the ordering problem noted in the pass's own comment (a custom property can legitimately reference another one declared later in source, which today works only because `expand_vars`/`expand_custom_functions` recurse through the raw map on demand at point-of-use, not at insertion time).
