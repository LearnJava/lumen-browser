# BUG-355: `graft_geometry` reuses a descendant's rect when an *ancestor's* geometry-affecting style changed — stale widths on the incremental restyle path

**Статус:** FIXED 2026-08-03
**Компонент:** layout (`crates/engine/layout/src/incremental.rs::graft_geometry_with_cascade`,
together with `lay_out`'s O(1) translate fast path for `DirtyBits::CLEAN` boxes)
**Найден:** P1, 2026-07-27, while writing the differential test for BUG-341 S14
(`mutation_incremental_restyle_hover_entering_from_nothing_matches_full`).
**Исправлено:** P1, 2026-08-03.

## Симптом

On the incremental restyle path (`layout_mutation_incremental_restyle`, used by
`relayout_chrome_host` and `try_relayout_raf_incremental`), an interaction that changes an
element's *own* box geometry — `padding`, `border-width`, `width`, … — does not resize its
descendants. The descendants keep the width they had before the interaction; a full layout
of the same final state gives a different one.

Minimal repro (unit-test shaped, in `lumen-layout`):

```rust
let html = r#"<div class="card"><ul><li id="a" class="item">x</li></ul></div>"#;
let css  = r#".card { padding: 3px; } .card:hover { padding: 9px; } .item { height: 20px; }"#;
// prev  = full layout, nothing hovered
// incr  = layout_mutation_incremental_restyle with hover on a node inside .card
// full  = full layout, same hover state
// → incr gives the <li> width 778.0, full gives 766.0 (the 2×6px padding delta)
```

Reproduced with the S14-narrowed root-set **and** with every node in the document forced
into `dirty_roots` — i.e. it is independent of how the restyle root-set is derived, and
predates BUG-341 S14. The cascade output is correct in both cases (`.card`'s new `padding`
is in its `ComputedStyle`); only the *geometry* is stale.

## Root cause

`graft_geometry_with_cascade` decides reuse per box by comparing that box's own style.
`.card`'s style changed → `.card` is re-laid-out. Its descendants' styles did **not**
change → their subtrees are grafted clean (`new.rect = prev.rect`, `dirty = CLEAN`), and
`lay_out`'s incremental fast path then *translates* a clean subtree in O(1) instead of
laying it out. Translation preserves size. But a parent's `padding`/`border`/`width` change
resizes the containing block, so every in-flow descendant's used width should change even
though not one of their computed styles did.

The mechanism has no notion of "my containing block changed" — it only asks "did my own
style change". BUG-341 S8 removed the early return that used to make a style reject
propagate *down* (it propagated up through `all_clean`, which is what made the whole
document unreusable), and that removal is what exposed this: after S8 a changed parent no
longer forces its children back through layout.

## Why nothing has caught it

* `assets/chrome/chrome.html`'s `:hover` rules only change colours/backgrounds, never box
  metrics, so the chrome fixtures (CC-12, the S8/S13 count gates) cannot hit it.
* The graphic tests all go through a full layout, never this path.
* The existing `incr == full` differential tests only flip styles on the *flipped node's own
  subtree*, where the reject is on the node whose geometry changes, not above it.

## Suggested fix direction (not attempted — separate slice)

When a box is not `self_reusable` and the differing fields can affect its content box
(anything feeding the containing block: `padding`/`border`/`width`/`box-sizing`/`display`/
writing mode…), its descendants must not be grafted clean — either mark the subtree dirty
outright, or (cheaper, and the shape the rest of BUG-341 already uses) keep the graft but
give `lay_out`'s fast path a "containing block resized" signal so a clean subtree is
re-laid-out at the new width instead of translated. The narrow version — only when the
*used* content-box size the parent produces differs from the one `prev` produced — keeps
today's reuse rate for the common colour-only interactions.

Until then the incremental restyle path is correct only for interactions that leave every
ancestor's box metrics unchanged, which is what both of Lumen's current stylesheets do.

## Fix (P1, 2026-08-03)

Took the "mark the subtree dirty outright" option from the suggested direction above, not
the cheaper `lay_out`-fast-path signal: `graft_geometry_with_cascade` already knows
`self_reusable` *before* it recurses into `new`'s children, so it now computes a new
`force_descendants_dirty = !self_reusable && containing_block_style_changed(&new.style,
&prev.style)` alongside the existing `all_clean` seed and, when true, calls
`mark_subtree_dirty` on each child directly instead of grafting it — skipping the graft
entirely rather than trying to have `lay_out` notice the resize after the fact.

`containing_block_style_changed` is deliberately narrower than "any style difference":
`width`/`min-width`/`max-width`, the four `padding-*`/`border-*-width` fields, `box-sizing`,
`display`, `writing-mode`, `direction`. `height`/`min-height`/`max-height` are deliberately
*excluded* — in the normal (horizontal) writing mode a box's height does not feed the width
it hands its children, and including it regressed
`graft_style_change_still_reuses_child_geometry` (the used-value-writeback shape: `lay_out`
had written a used `height` back into `prev`'s style, and that alone must not force reuse
of the child's untouched geometry — exactly what the BUG-341 S8/S13 slices already worked to
keep fast). A vertical-writing-mode or percentage-height-child gap remains, noted in the
function's own doc comment, since `writing_mode` itself is covered (a *change* in
writing-mode does force children dirty) but a box whose writing mode was already vertical
before and after does not get its `height` diff checked as a width driver.

Verified against the report's own repro (added as
`mutation_incremental_restyle_ancestor_padding_change_resizes_descendant`, `#card` with
`box-sizing: border-box` so the padding delta is visible in the fixed border-box width) and
by restoring `padding: 9px` to `.card:hover` in
`mutation_incremental_restyle_hover_entering_from_nothing_matches_full` (previously left at
a colour-only change specifically to dodge this bug — see that test's history). `cargo test
-p lumen-layout`: 3478/3480 (2 pre-existing failures are the unrelated `ch`/`ex` flake,
BUG-339, reproduced identically on `main` before this change).
