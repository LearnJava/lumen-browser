# BUG-553: CSS Gap Decorations implemented under non-spec property names (`gap-rule*` instead of `column-rule*`/`row-rule*`/`rule*`), row axis entirely missing

**Статус:** OPEN
**Дата:** 2026-08-04
**Компонент:** css-parser/layout (`crates/engine/layout/src/style.rs:3552-3559,15586-15625`), paint (`crates/engine/paint/src/gap_decorations.rs`)
**Найден:** WPT-RUN-3 срез 37 (`ROADMAP.md`) — массовый прогон `css/css-gaps`

## Механизм

`CSS-SPECS.md:113` marks "CSS Gap Decorations L1" ✅ done, citing a
`gap-rule-width`/`gap-rule-style`/`gap-rule-color` shorthand+longhand group
wired into flex/grid/multicol gap painting (`style.rs:3552-3559` fields,
parsed at `style.rs:15586-15625`, rendered by
`paint::gap_decorations::emit_gap_rules`). Those property names do not exist
in the shipped spec (<https://www.w3.org/TR/css-gaps-1/>): the actual surface
is **per-axis** — `column-rule`/`column-rule-style`/`column-rule-color`/
`column-rule-width` for the column axis, the sibling `row-rule*` group for
the row axis, and a `rule`/`rule-style`/`rule-color`/`rule-width` shorthand
group that sets both axes at once. `gap-rule*` was apparently an earlier
draft name that never made it into the current TR, and the implementation
was never renamed to track the spec's naming change.

Consequences, both confirmed by grep:
- `crates/engine/css-parser/src/lib.rs:121-124` only registers the
  pre-existing CSS Multi-column `column-rule*` longhands (multicol's own
  rule-between-columns feature, spec'd separately and already correct for
  multicol); there is no `row-rule*`/`rule*` registration at all, so those
  identifiers are unknown properties end to end.
- `style.rs` carries exactly one axis-agnostic rule triplet
  (`gap_rule_width`/`_style`/`_color`, non-inherited) — one rendering style
  for both column and row gaps, not two independent ones — so even a
  find-and-rename from `gap-rule*` to spec names could not fully close this
  without splitting the field into `column_rule_*` (grid/flex sense,
  distinct from the multicol `column_rule_*` triplet already occupying that
  Rust name) and a new `row_rule_*` triplet.
- The parser's `gap-rule` shorthand only understands `<line-width> ||
  <line-style> || <line-color>` — none of the spec's `<gap-rule-list>` /
  `<gap-auto-rule-list>` grammar (`repeat(auto, ...)`, `repeat(<integer>,
  ...)`, per-segment lists, `outset`/`inset`/`overlap-join`/`cap`
  behavior-at-intersections keywords) is parsed.

Net effect: every WPT test that sets `column-rule`/`row-rule`/`rule` (or any
longhand) on a flex/grid container and then reads it back via inline style
(`el.style.columnRuleStyle`) or `getComputedStyle` sees the property as
completely unsupported — canonicalization/serialization tests fail with
`expected "10px" but got ""`, and `"<prop> in getComputedStyle(el)"`
feature-detects fail with `expected true got false`, because the underlying
CSS-parser property table has no entry to resolve.

## Симптом

`css/css-gaps` mass run (WPT-RUN-3 slice 37): 75/75 harness OK, 794/4148
subtests passed, 3354 failing. **3353 of those 3354** across `parsing/`,
`animation/`, and other subdirectories match this one root cause — e.g.
`gap-decorations-rule-shorthand.html`: `assert_true: column-rule-style
doesn't seem to be supported in the computed style expected true got
false`; `rule-width-interpolation-conversion-001.html`: `assert_equals:
expected "0px" but got ""`. The remaining 1 failure
(`gap-decorations-important.html`, `target is not defined`) is unrelated —
covered by the already-open BUG-384 (named access on `Window` missing).

## Масштаб находки

Dominant cluster of the whole `css-gaps` category (99.97% of its failing
subtests). Fixing requires: (1) renaming/splitting the ComputedStyle fields
into per-axis `column_rule_*`/`row_rule_*` triplets distinct from multicol's
existing `column_rule_*`, (2) registering `column-rule`/`row-rule`/`rule`
(+ longhands) as recognized properties for flex/grid containers without
colliding with multicol's identically-named `column-rule*` on multicol
containers, (3) implementing the `<gap-rule-list>`/`<gap-auto-rule-list>`
value grammar (`repeat()`, per-segment override lists, intersection-behavior
keywords) in the parser, and (4) re-verifying `CAPABILITIES.md`/
`CSS-SPECS.md:113`, which currently claims this module is done.
