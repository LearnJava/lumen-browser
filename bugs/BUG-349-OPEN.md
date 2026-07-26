# BUG-349: `restyle_root_set_for_node_change` doesn't account for `:has()` — a DOM mutation can leave an ancestor's `:has()`-dependent style stale under the incremental restyle path

**Статус:** OPEN
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
