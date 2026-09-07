# BUG-1026: `collapsed_top_margin`/`collapsed_bottom_margin` are O(1) stack but O(depth) time — O(depth²) total on a deep single-child chain

**Статус:** FIXED 2026-09-07 (P3)
**Дата:** 2026-09-07
**Компонент:** layout (`crates/engine/layout/src/box_tree/bfc.rs` —
`collapsed_top_margin`, `collapsed_bottom_margin`)
**Найден:** P1 2026-09-07, при завершении LAYOUT-2 среза 1 (block-flow
trampoline, `block_flow_trampoline.rs`)

## Механизм

LAYOUT-1 срез 1 (2026-09-06) converted `collapsed_top_margin`/
`collapsed_bottom_margin` from one native recursive call per link in the
first-/last-child chain to an explicit loop — but that conversion only
removes the native-stack cost (was the actual BUG-987 fix target). Each
call still does O(remaining-chain-length) *work*: it walks from the given
box all the way down its own first-/last-child chain to fold in every
margin along the way.

The plain block-flow branch (`block_flow_trampoline.rs`'s `step_child`/
`post_child_bookkeeping`/`finish_frame`, née the inline loop in
`layout_dispatch.rs` before LAYOUT-2 срез 1) calls one or both of these
once **per level** of a normal-flow descent: `step_child` calls
`collapsed_top_margin` on each child before dispatching it,
`post_child_bookkeeping` calls `collapsed_bottom_margin` on each finished
child, and `finish_frame` calls it again on the container's own last
child. For a chain of `N` single-child boxes, the total work is
Σ(N−i) ≈ N²/2 per helper — O(N²) overall, not O(N).

Before LAYOUT-1/LAYOUT-2 this was unobservable: native recursion in
`lay_out_inner`'s block-flow branch overflowed the thread stack around
~150–800 levels of nesting (BUG-987), long before the quadratic cost of
these two helpers became large enough to notice. LAYOUT-2's block-flow
trampoline (`block_flow_trampoline::run`) removed that stack-depth
ceiling — deep chains now run to completion instead of crashing — which
is exactly what exposed this: a synthetic `DEPTH: usize = 200_000`
regression test for the trampoline (`tests/block_flow_trampoline.rs`,
`deep_chain_with_margins_collapses_through_the_trampoline`) took over an
hour of CPU time without finishing (confirmed via a live `tasklist`
inspection — 1h50m+ CPU time on a still-running test process, no other
plausible bottleneck) before it was reduced to `DEPTH: usize = 20_000`
(matching LAYOUT-2's actual ROADMAP acceptance depth) to make the test
suite tractable — at 20_000 the same test takes on the order of two
minutes, still dominated by this quadratic cost.

## Симптом

A block-flow subtree that is a long single-child (or single-collapsible-
child) chain — the exact DOM shape BUG-987 was filed against
(fandom.com/OneTrust-style deeply nested wrapper `<div>`s) — pays
quadratic, not linear, layout cost from margin-collapse alone once the
chain is a few thousand levels deep. Real pages are very unlikely to hit
tens of thousands of levels (BUG-987's repros were in the hundreds), so
this is not yet known to cause a user-visible stall — filed as a latent
correctness-adjacent performance defect discovered while proving
LAYOUT-2's block-flow trampoline slice, not as a live-site regression.

## Масштаб

Both `collapsed_top_margin` and `collapsed_bottom_margin` (`bfc.rs`), and
every caller that invokes either once per level of a block-flow descent
(`block_flow_trampoline.rs`'s three call sites today; LAYOUT-2's five
remaining trampolines — flex/grid/table/multicol/vertical — will each
need their own version of this same accounting once they're converted,
so the fix should land before or alongside those slices rather than be
rediscovered per dispatcher).

## Что нужно

Turn the per-call full-chain walk into an amortized O(1) per level:
either (a) thread the already-computed `collapsed_top_margin`/
`collapsed_bottom_margin` result for a box's first/last collapsible child
down/up through the trampoline's own state instead of recomputing it from
scratch at the next level (the recurrence
`collapsed_top_margin(box) == max(own_margin, collapsed_top_margin(first_collapsible_child(box)))`
is linear if each level reuses the child's already-known value), or (b)
memoize per-`NodeId` within one layout pass. Not attempted in LAYOUT-2
срез 1 — out of scope for a slice whose acceptance criterion was
termination/pixel-parity at `<div>`×20000, not micro-benchmarking the
margin-collapse helpers, and touching the recurrence shape risks a subtle
correctness regression in the hottest layout path on every page.

## Fix (P3, 2026-09-07)

Went with a variant of option (b): a per-`run()`-pass `MarginCollapseCache`
(`bfc.rs`), threaded by `&mut` through `step_child`/`post_child_bookkeeping`/
`finish_frame` (two instances — top and bottom margins are unrelated
quantities and must not share one map). Key is `(NodeId, BoxRole)`, not bare
`NodeId`: one DOM node can back several distinct `LayoutBox`es (an element's
principal box and an anonymous wrapper/pseudo-element box both carry the same
`LayoutBox::node`, ADR-025 §1) — the same disambiguation `LayoutInPlaceKey`/
`LayoutResultKey` (BUG-341 S38/S40) already use for the identical reason.
Required adding `#[derive(Hash)]` to `BoxRole` and `PseudoKind`
(`box_tree/types.rs`) — both already derived `PartialEq, Eq`, so this is
additive, no behavior change.

Correctness hazard considered and designed around: the containing-block width
(`cb`) a caller passes in for a box is not always equal to the `cb` this
module's own internal chain-walk would derive for that same box while folding
an ancestor's result (the walk only subtracts padding/border, not margins or
`scrollbar-gutter`, while the box's real content width — used by the direct
caller — can subtract those too). Rather than try to prove the two always
match, every cache entry stores the `cb` it was computed with, and a lookup
only reuses the cached value when the fresh call's `cb` matches exactly
(bit-for-bit, since both are the same float when derived identically for the
realistic zero-margin wrapper-`<div>` shape this bug targets). A mismatch
falls back to a full recompute — always correct, just not always O(1) — so
the fix cannot silently return a wrong margin, only fail to speed up a chain
with non-zero margins/scrollbar-gutter along it.

Each call now walks its chain into a `Vec` (as before), then folds a suffix
max backward from the end, populating the cache for every node visited along
the way — so the *first* call from the top of a chain is the only full O(n)
walk; every subsequent direct call for a node further down the same chain
(made by the trampoline as it descends level by level) is an O(1) cache hit.

`tests/block_flow_trampoline.rs`'s `DEPTH` constant (reduced to 20_000 by this
bug's own discovery) is restored to 200_000, matching the sibling LAYOUT-1
traversal-only tests, and `deep_chain_with_margins_collapses_through_the_trampoline`
gained an explicit 10s wall-clock deadline assertion — both trampoline tests
together now finish in ~1.3s at `DEPTH: usize = 200_000` (previously
confirmed not to finish in an hour). `bfc_margin_collapse.rs`'s existing
correctness tests pass unchanged with a cache parameter threaded in.

Pixel neutrality: `graphic_tests/run.py --continue-on-fail` run on
unmodified `main` and on this branch produced byte-identical diff
percentages and regions on every compared test (00–49); the handful of
differences seen (TEST-02/04/18/19/21/32/34/45/46/47) reproduce identically
on `main` (known debtors BUG-128/BUG-176/BUG-219, or a pre-existing,
unrelated defect) — none are new. The full suite did not finish end-to-end
on this machine (repeated `ffmpeg` diff timeouts under concurrent load from
other roles' worktrees), but the covered range spans plenty of block/margin
layout tests and matched exactly.

`cargo test -p lumen-layout --lib`: 3909/3909 (0 failed, 1 ignored, pre-existing).
`cargo clippy -p lumen-layout --all-targets -- -D warnings`: clean.
