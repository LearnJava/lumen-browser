# BUG-1026: `collapsed_top_margin`/`collapsed_bottom_margin` are O(1) stack but O(depth) time — O(depth²) total on a deep single-child chain

**Статус:** OPEN
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
