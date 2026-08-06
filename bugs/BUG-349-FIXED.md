# BUG-349: `restyle_root_set_for_node_change` doesn't account for `:has()` — a DOM mutation can leave an ancestor's `:has()`-dependent style stale under the incremental restyle path

**Статус:** FIXED 2026-08-06
**Компонент:** layout (`crates/engine/layout/src/style.rs::restyle_root_set_for_node_change`,
`PseudoClass::Has` matching via `matches_relative`)
**Найден:** P1, 2026-07-26, while implementing BUG-341 S7's hover-fan-out-narrowing
sub-slice and re-checking that slice's own doc comment ("this engine has no `:has()`")
against the actual matcher — `:has()` *is* implemented (`style.rs`'s `matches_pseudo_class`
handles `PseudoClass::Has(list)` via `matches_relative`), so the comment's premise was
false and the invalidation-set reasoning built on it doesn't hold.

## Симптом (theoretical — no fixture reproduces it yet)

`restyle_root_set_for_node_change(doc, nodes)` invalidates each mutated node's *parent's*
whole subtree, reasoning that class/attribute/structural selectors "don't match ancestors
by themselves". That reasoning is correct for plain descendant/child/sibling selectors but
not for `:has()`: `article:has(.expanded)` matches an `<article>` ancestor based on whether
*any descendant* currently has `.expanded` — an ancestor that can be arbitrarily far above
the mutated node's parent, not just one level up.

Under BUG-341's incremental restyle path (`INCREMENTAL_RESTYLE` flag, wired into
`relayout_chrome_host`/`try_relayout_raf_incremental`/`cc12_bench_cycle`), a class/attribute
change on a deeply nested node that flips an ancestor `article`'s `:has(.expanded)` result
would leave that ancestor's cached `ComputedStyle` untouched — the incremental cascade would
reuse a stale style for `article` instead of recomputing it, producing a real-but-currently-
unobserved rendering bug the moment any stylesheet (chrome or page) pairs `:has()` with a
dynamically-toggled class/attribute inside its argument.

## Root cause

`restyle_root_set_for_node_change` (`style.rs`, doc comment right above the function)
predates `:has()` support in this engine (or was written without checking) and its
correctness argument explicitly (and incorrectly) asserts `:has()` doesn't exist. No code
path currently derives, from a stylesheet, which ancestors have a `:has()` rule that could
depend on a given mutated node — that index doesn't exist anywhere in the codebase yet
(`RuleIndex` only buckets by *subject* compound, and `:has()`'s dependency direction is the
reverse of a normal selector's).

## Scope check performed

No fixture (graphic test, WPT-vendored test, or unit test) currently pairs a dynamically
toggled attribute/class with a `:has()` selector, so `INCREMENTAL_RESTYLE` (still
off-by-default — see BUG-341) has not been observed to actually misrender anything from
this gap. This bug documents the gap so it's not silently reintroduced or rediscovered
from scratch; it does not by itself block enabling `INCREMENTAL_RESTYLE` by default any
more than BUG-341's other open items already do.

## Suggested fix direction (not attempted here — out of scope for BUG-341 S7)

Symmetric to `:hover`/`:focus`/`:active`'s ancestor-matching handling in
`restyle_root_set_for_state_change`: precompute (once per stylesheet, memoizable the same
way `restyle_state_needs_fanout` is) whether any selector contains `:has()` at all: if none
do, `restyle_root_set_for_node_change`'s existing parent-subtree behaviour is already
correct (this is the common case for both of Lumen's current stylesheets — chrome's
`assets/chrome/chrome.html` and every graphic test — neither uses `:has()` today). If some
selector does use `:has()`, the conservative correct fallback is invalidating the whole
document for structural/attribute mutations until a real `:has()`-dependency index is
designed — a bigger, separate task.

## Fix (P3, 2026-08-06)

Implemented exactly the suggested direction above, without attempting the bigger
`:has()`-dependency-index task. `NodeRestyleIndex` (`style.rs`) gained a new
`has_dependent: bool` field, set by `restyle_node_index` from the same
`complex_selector_has_any_has` scan that already fed the pre-existing `conservative`
flag (kept as-is for the unrelated `:nth-child(… of S)`/shadow-root reasons, both of
which stay correctly served by parent-only widening — sibling reach never leaves the
parent's subtree). `restyle_root_set_for_node_change` now checks
`index.has_has_dependency()` first: when set, every reported change maps to
`doc.root()` instead of running through the parent-widen/narrow logic at all — the
"invalidate the whole document" fallback the original writeup called for. `chrome.html`
and every graphic test still use zero `:has()` selectors, so `is_conservative()`/
`needs_fanout` stay `false` for both and this is a no-op for every existing fixture —
confirmed by an unchanged `cargo test -p lumen-layout --lib` pass count baseline plus
`dump_golden.py` (no `:has()` anywhere in the display-list-producing fixtures, so no
diff possible).

2 new unit tests in `style::node_fanout_tests`: `has_anywhere_in_the_sheet_widens_to_the_whole_document`
(replaces the old `has_anywhere_in_the_sheet_disables_narrowing`, which asserted the
insufficient parent-only widening as correct) and
`has_far_above_the_mutated_node_is_caught_by_the_document_wide_widening` — the exact
`article:has(.expanded)` / three-levels-deep mutation shape from the Симптом section
above, asserting the root-set now reaches `doc.root()` instead of stopping at the
mutated node's immediate parent. `cargo test -p lumen-layout --lib`: 3494/3494 green.
`cargo clippy -p lumen-layout --all-targets -- -D warnings`: clean.

Still not attempted, same as the original writeup flagged as a bigger separate task: a
real `:has()`-dependency index that narrows the whole-document fallback to just the
ancestors a given `:has()` selector could actually reach. `INCREMENTAL_RESTYLE` remains
off by default (BUG-341, paused).
