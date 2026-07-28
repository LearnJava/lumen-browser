# BUG-341: `lay_out_flex` double-lays-out every item — general engine bug, ~300× over CC-12's 2ms chrome perf-gate budget

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs::lay_out_flex`) — **general flexbox algorithm bug, affects any nested-flexbox page**, not just chrome. Surfaced via the chrome document (`crates/shell/src/main.rs::relayout_chrome_host`, `docs/tasks/p1-css-chrome.md`) because CC-12 was the first hard perf budget + realistic flex-nesting-depth bench to exist.
**Найден:** P1, CC-12 (перф-гейт хрома) 2026-07-25 — новый тест `crates/shell/src/main.rs::tests::cc12_chrome_perf_gate_hover_and_keystroke_cycles`. Root-caused: P1, 2026-07-25.

## Follow-up (P1, 2026-07-25, fourth session): profiled the *remaining* cost — the "layout-result cache" plan is insufficient; real fix is incremental **cascade** + layout

Re-profiled the current code (main + all three prior fixes below) with
`LUMEN_PROFILE_TREE=1`, averaged over 77 `layout_measured_hyp` passes of
`cc12_chrome_perf_gate` (dev-release). Split of one full layout pass:

| Stage | Avg | Share | What it does |
|---|---:|---:|---|
| `precompute_counters` | 155 ms | **53 %** | **full style cascade** — one `compute_style` per node, cached into `CounterMap::styles` for `build_box` |
| `lay_out` | 102 ms | **35 %** | box placement (where `lay_out_flex`'s double-layout lived) |
| `build_box` | 22 ms | 8 % | box-tree construction (reuses the cascade cache) |
| `post_layout_passes` | 3 ms | 1 % | container queries, anchor, first-line split |

(Absolute ms ~3× this machine's runs vs. the `p50≈85 ms` recorded below — a
machine/contention difference; the **relative split is the load-bearing result**
and is stable across runs. Clean re-measure this session: `CC12_HOVER
p50≈298 ms p95≈487 ms`, `CC12_KEY p50≈245 ms p95≈313 ms` — same shape, higher
absolute.)

**This overturns the "Fix scope note" plan below.** A layout-result cache
targets only `lay_out` — **35 %** of the cost. Even making `lay_out` free leaves
`precompute_counters` (53 %) + `build_box` (8 %) = **61 %** standing (~52 ms on
the reference machine, still **~26× over the 2 ms budget**). The dominant cost
is the **style cascade**, which *every* current entry point —
`layout_measured_hyp`, `layout_streaming_incremental`,
`layout_mutation_incremental` — recomputes in full every call. There is **no
persistent per-node `ComputedStyle` cache and no restyle-damage model** today.

**Correct fix (design brief: [`docs/tasks/p1-bug341-incremental-restyle.md`](../docs/tasks/p1-bug341-incremental-restyle.md)):**
make per-interaction work O(changed subtree) across *all three* stages —
incremental cascade (restyle-damage + a persisted `ComputedStyle` cache) +
incremental box-build + the already-existing incremental layout. This is a
Servo/Blink-class incremental-restyle system on the hottest, most correctness-
sensitive path in the engine; sliced S1–S6 in the brief, each gated on an
`incremental == full` bit-identical differential test plus graphic tests / CPU
snapshots. S1 (this profiling + the brief) is done; S2+ is the implementation.

## Симптом

CC-12's perf gate measures exactly the mutate→restyle→relayout→paint cycle
`Lumen::relayout_chrome_host` runs on every chrome interaction (hover-flip,
keystroke), headlessly (no window/GPU — see the test's doc comment for why
this is a more precise instrument than the live `LUMEN_BENCH` harness for
this specific cycle). Budget from the brief (`docs/tasks/p1-css-chrome.md`
CC-12): ≤ 2ms per cycle.

Measured on `chrome_preview::HTML` with a populated `ChromeModel` (6 tabs, 3
workspaces), `cargo test -p lumen-shell --profile dev-release
cc12_chrome_perf_gate -- --ignored --nocapture`:

```
CC12_HOVER count=60 min=539.88ms p50=587.46ms p95=743.96ms p99=831.13ms max=831.13ms
CC12_KEY   count=60 min=539.13ms p50=574.54ms p95=637.11ms p99=656.71ms max=656.71ms
```

Both scenarios (toggling `:hover` on `#sidebar`, and appending one character
to the omnibox value per iteration) land in the same ~550-850ms range —
**~300× over the 2ms budget.**

## Diagnosis so far

Split timing (spot check, not part of the committed test) around the three
stages of one cycle:

```
[cc12-diag] bind=0.42ms layout=706.92ms paint=0.57ms nodes=2261
[cc12-diag] bind=0.49ms layout=632.54ms paint=0.38ms nodes=2362
[cc12-diag] bind=0.44ms layout=572.45ms paint=0.33ms nodes=2463
[cc12-diag] bind=0.44ms layout=658.79ms paint=0.43ms nodes=2564
[cc12-diag] bind=0.51ms layout=642.78ms paint=0.37ms nodes=2665
```

- `bind_model` (the DOM mutation step) and `paint_ordered` (display-list
  build) are both consistently sub-millisecond — **not** the bottleneck.
- Essentially all the cost (>99.8%) is inside `lumen_layout::layout_measured_hyp`
  itself.
- **Ruled out as a bench artifact:** `Document::node_count()` grows across
  the 5 diagnostic iterations (2261→2665, +18%, because `bind_model`'s
  `remove_children_with_class` detaches old list nodes from the tree but
  the underlying arena never reclaims them — expected/normal for an
  arena-backed DOM). If `layout_measured_hyp` scanned the whole arena
  (including orphaned nodes) rather than walking from the document root,
  cost would scale with this growth. It does not: `layout_ms` stayed flat/
  noisy (707→633→572→659→643ms) with no monotonic trend against the
  growing node count. So the ~600-700ms is a large, roughly per-call-fixed
  cost of a full layout pass over chrome's actual (reachable) node count,
  not a leak in the diagnostic harness.

This matches the CC-12 brief's own stated risk #3 (`docs/tasks/p1-css-chrome.md`
"Риски и открытые вопросы"): *"Перф полного рестайла на каждый hover/символ
(~400 узлов, каскад всегда полный) — главный количественный риск"* — the
risk materialized far worse than the 2ms hoped-for budget.

**Not yet root-caused further** — the fixed per-call cost could be text
shaping without a cross-layout cache (harfbuzz/rustybuzz shaping runs are
not free and the brief's "каскад всегда полный" implies no reuse across
layout passes), SVG `<use>`/`<symbol>` icon resolution (many toolbar/sidebar
icons in `assets/chrome/chrome.html`, CC-1/CC-2 already found icon-sizing
issues in this area — see [BUG-333](BUG-333-OPEN.md)/[BUG-334](BUG-334-FIXED.md)),
or something else inside `layout_measured_hyp`'s per-node style/layout work.
Needs profiling (e.g. `lumen_core::tracy_zone!` spans already present in
`layout_measured_hyp`, or a manual sub-stage timing pass) to isolate.

## Root cause (confirmed 2026-07-25, P1)

Using the already-present `LUMEN_PROFILE_TREE=1` call-tree profiler
(`lumen_core::profile`) on `cc12_chrome_perf_gate` isolated the cost inside
`layout_measured_hyp` to a single sub-stage:

```
[profile]     37-45ms    precompute_counters
[profile]      5-7ms     build_box
[profile]    505-660ms   lay_out       <-- 95%+ of the cost
[profile]     0.6-0.9ms  post_layout_passes
```

`build_box` (box-tree construction + style cascade — one `compute_style` call
per node) is cheap, ruling out the CC-12 brief's original suspicion ("каскад
всегда полный") as the dominant cost. The cost is entirely inside the actual
box-placement algorithm, `lay_out`/`lay_out_inner`.

A temporary call counter (thread-local `Cell<u64>` incremented at the top of
`lay_out_inner`, reset per `layout_measured_hyp` call, printed under
`LUMEN_PROFILE_TREE=1`, removed after this diagnosis) showed
**`lay_out_inner` called 71,428 times per cycle** against a document with
~2,300-2,700 reachable nodes — roughly **31× more layout calls than nodes**,
where a single clean layout pass should call it ≈1× per node.

Root cause: `lay_out_flex` (`crates/engine/layout/src/box_tree.rs`, function
starting ~line 8288) lays out every flex item with the **full** recursive
`lay_out()` **twice**:

1. "Step 1 — preliminary layout for intrinsic sizes" (`box_tree.rs:8354`) —
   an unconditional full recursive `lay_out()` per flex item, used to read
   back `item.rect.width`/`item.rect.height` as the hypothetical main/cross
   size going into the flex-basis/line-breaking/grow-shrink algorithm.
2. The final placement pass (`box_tree.rs:~8682`) lays every item out again
   with its resolved main/cross size.

Both passes are genuinely full, recursive `lay_out()` calls — not a cheap
intrinsic-size-only probe (Lumen already has cheap probes for this,
`max_content_outer_width`/`min_content_outer_width`/`preferred_inline_block_width`,
used elsewhere for inline-block shrink-to-fit — flex does not use them because
column-direction items additionally need a real content *height*, which those
helpers don't compute).

Because each flex container fully re-lays-out its entire descendant subtree
**twice**, and Lumen's chrome UI (a flexbox-heavy design system: toolbar →
button-group → button → icon-wrap, sidebar → tab-row → tab → label, etc.)
nests flex containers several levels deep, the redundancy compounds
multiplicatively with nesting depth — a subtree at flex-nesting depth *k*
gets laid out roughly proportional to 2^k times instead of once. The observed
≈31× average call inflation over ~2,300 nodes is consistent with the chrome
design's actual flex-nesting depth (not every level is flex, so it is not a
clean power of two, but the shape of the blowup matches).

**This is not chrome-specific** — `lay_out_flex` is the one and only flex
algorithm in the engine, shared by chrome and web-page layout alike. Any real
page using nested flexbox (the overwhelming majority of modern sites) pays
the same redundant-relayout tax; it was simply never isolated with a hard
perf budget + a bench with real-world flex nesting depth before CC-12's gate
existed. `docs/perf/journal.md`'s corpus runs may already contain this cost
folded into "style+layout" phase numbers for flex-heavy sites without anyone
having attributed it to this specific mechanism.

**Fix scope note:** the correct general fix is a layout-result cache/memoization
keyed by (node, incoming constraints — available width/height, measurer
identity, dark_mode) so that a flex container's "Step 1" probe and a later
identical-constraint final pass reuse one result instead of computing it
twice, and so nested flex containers don't re-derive their whole subtree at
every ancestor level. This is a real architectural addition (a full
layout-memoization layer touching the hottest path in the engine), not a
local patch — it needs its own scoped design + risk assessment against the
whole layout test suite (graphic tests, WPT flex coverage, scoped-test) rather
than a same-session fix bolted onto this investigation. Filed as a dedicated
follow-up rather than attempted here.

## Attempted mitigation (P1, 2026-07-25): incremental layout for chrome — no effect

Tried the brief's own risk-#3 mitigation (`docs/tasks/p1-css-chrome.md`:
*"ворота CC-12, страховка — инкрементальный layout"*) as a possible way to
unblock CC-14 without the full layout-result cache: switched
`relayout_chrome_host` from `layout_measured_hyp` to
`lumen_layout::layout_mutation_incremental` (`graft_geometry`, same mechanism
ADR-016 M4 already uses for page-side rAF mutations), keeping a pristine
pre-`take_content_area` tree as the `prev` basis (`take_content_area`'s
salvage-reparenting shifts sibling indices, which would otherwise misalign
`graft_geometry`'s by-index matching for everything after `#contentArea`),
with a viewport-change guard to avoid grafting stale-size geometry across a
resize.

**Result: no measurable improvement.** `cc12_chrome_perf_gate` (dev-release):

```
CC12_HOVER count=60 min=564.94ms p50=607.78ms p95=774.07ms p99=815.14ms max=815.14ms
CC12_KEY   count=60 min=556.77ms p50=578.19ms p95=622.98ms p99=630.17ms max=630.17ms
```

vs. the CC-12 baseline (`p50=587/575ms`, `p95=744/637ms`) — within noise,
not the 10x+ speedup `layout_mutation_incremental`'s own doc comment
describes for a typical single-node style toggle. Change reverted, not
merged (adds a `LayoutBox::clone()` per pass — real overhead — for zero
benefit as things stand).

**Root cause of the non-improvement:** `graft_geometry` matches subtrees by
stable `NodeId` (plus box-kind and computed style). `lumen_chrome::bind_model`
(`crates/chrome/src/model.rs::rebuild_tab_list` /
`rebuild_hbar_tab_list` / `rebuild_hbar_ws_list`) unconditionally
`remove_children_with_class` + rebuilds every tab-row / hbar-tab / workspace
row from scratch on **every single call**, regardless of whether the tab/
workspace list actually changed — each rebuilt row is a freshly
`doc.create_element`'d node with a brand new `NodeId`. Since
`relayout_chrome_host` calls `bind_model` unconditionally before every
layout pass (CC-6: "no separate dirty flag, `bind_model` is cheap"), the
6-tab/3-workspace chrome document used by the perf gate gets its tab-row/
hbar-tab/ws-pill subtrees entirely re-identified every single interaction —
`graft_geometry`'s node-id comparison fails immediately at those subtrees,
and (per its by-index matching over the parent's `children` list) forces a
fresh full layout for essentially the whole document regardless of how
small the actual on-screen change was. This is an **independent
node-churn problem layered on top of the `lay_out_flex` double-layout bug**:
fixing BUG-341's flex algorithm alone would still leave chrome paying a full
relayout on every hover/keystroke unless `bind_model`'s list rebuilders are
also made to diff-and-patch (reuse existing row nodes, only touching the
subset whose displayed fields actually changed) instead of destroy-and-
recreate. Neither half alone gets CC-12's gate green; both would need
fixing together, roughly doubling the scope of an already out-of-session-
scope fix. Not attempted here — `rebuild_tab_list`/`rebuild_hbar_tab_list`/
`rebuild_hbar_ws_list` are core to every list-rendering CC slice (CC-6/CC-8/
CC-13's ARIA roles are set inside these same rebuild functions), so
diff-based rewrites need their own scoped review against all of them rather
than a bolt-on for this investigation.

## Partial fix (P1, 2026-07-25): skip the Step-1 preliminary layout when nothing reads it

`lay_out_flex`'s Step 1 loop (`box_tree.rs:8333`) unconditionally called the
full recursive `lay_out()` for every flex item, purely to populate `item.rect`
for the `all_hyp` computation right after it. Auditing every branch of that
computation shows most of them never actually read `item.rect`:

- Row direction, `FlexBasis::Length` — resolves the base size directly from
  the style length, `item.rect` unused.
- Row direction, `FlexBasis::Auto`/`Content` with no explicit `width` — uses
  the existing cheap `flex_auto_base_main_width` probe (`max_content_outer_width`
  bounded by resolved min/max-width), not `item.rect`.
- Column direction, `FlexBasis::Length` when `min-height` is set or
  `overflow-y` isn't `visible` — the `auto_min` floor is forced to `0.0`
  regardless of `item.rect.height`.

Only these still need the real preliminary layout: column direction with
`FlexBasis::Auto`/`Content` (always needs content height), and row direction
`FlexBasis::Auto`/`Content` with an explicit `width` set (still reads
`item.rect.width`). The Step-1 loop now computes a per-item `needs_prelayout`
bool from the item's own style (no layout needed to decide) and only calls
`lay_out()` when true; the final placement pass a few lines down already
calls `lay_out()` unconditionally for every item regardless, so no item loses
a layout pass — this only removes the ones nothing read.

Measured effect on the same `cc12_chrome_perf_gate_hover_and_keystroke_cycles`
bench (dev-release):

```
CC12_HOVER count=60 min=70.16ms p50=85.56ms  p95=138.47ms p99=150.95ms max=150.95ms
CC12_KEY   count=60 min=69.04ms p50=80.86ms  p95=85.50ms  p99=89.27ms  max=89.27ms
```

vs. the original baseline (`p50=587/575ms`, `p95=744/637ms`) — roughly a
**6-7× reduction**, consistent with chrome's UI eliminating most of the
Step-1 passes at every level of its flex nesting. `cargo test -p lumen-layout`
unaffected (3252/3254 passing — the 2 failures are the pre-existing
`FONT_CH_EX` flaky pair, unrelated).

**CC-12's gate is still red** (p95 138ms vs the 2ms budget) — this fix only
resolves the `lay_out_flex` half of the double cost documented above; the
independent `bind_model` list-rebuild node-churn problem (also documented
above, "Attempted mitigation" section) still forces effectively-full relayout
on every chrome interaction and has not been touched by this fix. **Both
still need fixing together to get CC-12/CC-14 green** — this change reduces
the size of that remaining gap by ~6-7× but does not close it. Root cause
(the general architectural fix — a layout-result cache) also remains
unimplemented; this is a scoped, low-risk removal of provably dead work
within `lay_out_flex` itself, not that broader memoization layer.

## Follow-up (P1, 2026-07-25, second session): `bind_model` NodeId churn fixed — `layout_mutation_incremental` retried, still no gain (new root cause found)

Fixed the "Attempted mitigation" section's independent node-churn problem:
`lumen_chrome::model`'s `rebuild_tab_list`/`rebuild_hbar_tab_list`/
`rebuild_workspace_list`/`rebuild_hbar_ws_list` now reconcile against the
existing DOM by position (`reconcile_row_list`) instead of detaching and
recreating every row on every `bind_model` call — an unchanged row (and
every one of its descendants: fav/title/close-or-badge) keeps its `NodeId`
across calls; only a genuine shape change (`is_child`/`container_color`
presence/`sleeping` flip) falls back to rebuilding that one row's children
(the row itself still keeps its id). Text updates go through a new
`set_text_in_place` that mutates the existing `NodeData::Text` payload
instead of detach+recreate, since `graft_geometry` recurses into every
descendant — a single stray fresh text-node id anywhere in the subtree
would have defeated the whole point. 6 new `cargo test -p lumen-chrome`
cases assert identity is preserved end-to-end (55/55 passing).

With that fixed, re-tried switching `relayout_chrome_host` (and the CC-12
bench's `cc12_bench_cycle`) to `layout_mutation_incremental` again, this
time with `bind_model`'s node-churn genuinely gone:

```
CC12_HOVER count=60 min=71.04ms p50=106.42ms p95=138.44ms p99=191.33ms max=191.33ms
CC12_KEY   count=60 min=68.83ms p50=81.11ms  p95=97.88ms  p99=101.94ms max=101.94ms
```

**Still no gain — hover even got measurably worse** (p50 106ms vs the
`layout_measured_hyp` baseline's 85ms below). Reverted back to
`layout_measured_hyp` in both places to confirm the bind_model fix itself
isn't the regression source:

```
CC12_HOVER count=60 min=68.24ms p50=84.93ms p95=115.91ms p99=120.14ms max=120.14ms
CC12_KEY   count=60 min=68.64ms p50=72.38ms p95=81.53ms  p99=84.30ms  max=84.30ms
```

Matches the partial-fix baseline (p50 ≈85/81ms) within run-to-run noise —
confirms the `bind_model` fix is perf-neutral (as expected: it only removes
*incidental* NodeId churn, and `layout_measured_hyp` never looked at node
identity to begin with) and correctness-only under the plain full-layout
path, while `layout_mutation_incremental` is the one that regresses.

**New root cause for the non-improvement:** `lumen_layout::incremental::graft_geometry`
clones the matched subtree **once per ancestor level** inside a clean
region, not once at the outermost clean boundary. Its recursion clones a
clean child in place (`*new.children[i] = prev.children[i].clone()`, via
the child's own `if all_clean { *new = prev.clone(); }` branch) *before*
the parent decides it too is `all_clean` and does **its own**
`*new = prev.clone()` — a full deep clone of the whole subtree, including
the child that was just individually cloned one recursion level down. For
a clean region of depth *d*, this redundantly re-clones the same
`ComputedStyle`-and-`Vec<LayoutBox>`-heavy data up to *d* times instead of
once. Chrome's sidebar/tab-list subtree (now fully clean thanks to the
`bind_model` fix) is nested several flex levels deep, so the redundant
clone cost apparently outweighs the layout work actually skipped — net
negative, not neutral. This is a `lumen-layout`-crate bug (affects any
`layout_mutation_incremental` caller with a deep clean subtree, not
chrome-specific), separate from both the `lay_out_flex` double-layout bug
above and the now-fixed `bind_model` node-churn bug. Not fixed here — needs
its own scoped change to `graft_geometry` (e.g. skip the redundant
wholesale clone when every child already reports clean, since each clean
child has already replaced itself with its own cloned subtree in place)
verified against the existing `lumen-layout` incremental test suite. Filed
as the next concrete follow-up; **CC-14 remains blocked** on either this or
the original layout-result-cache fix for `lay_out_flex`'s remaining
non-doubled cost (both still needed to reach the 2ms budget — the
`bind_model` fix alone was necessary but is not sufficient).

## Follow-up (P1, 2026-07-25, third session): `graft_geometry` quadratic clone fixed — real ~20% win, gate still red

Fixed the `graft_geometry` root cause identified above (`crates/engine/layout/src/incremental.rs`):
the `if all_clean { *new = prev.clone(); }` branch now only copies this node's
own scalar fields (`rect`, `kind`, `scroll_x`/`scroll_y`, `col_span`/`row_span`,
`svg_group_transform`) from `prev` instead of deep-cloning the whole subtree —
`new.children` is left untouched, since each clean child already replaced
itself in place one recursion level down. `kind` still needs an explicit
clone (not just the `==`-style `kind_layout_eq` check that gated entry into
this branch) because it can carry post-layout payload the freshly-built `new`
side doesn't have yet (e.g. `InlineRun`'s laid-out `lines`). This removes the
O(depth) redundant clone entirely — every node now does O(1) work in this
branch regardless of subtree size. All 23 existing `lumen-layout::incremental`
tests pass unchanged (the fix only changes performance, not the grafted
values — verified: the `graft_identical_tree_is_all_clean` test's "mutating
`prev` afterwards must not affect `fresh`" assertion still holds, since the
copied fields are by-value/independently-cloned, not shared references).

Re-measured `relayout_chrome_host` on `layout_mutation_incremental` (same
harness as the two prior attempts above, dev-release,
`cargo test -p lumen-shell --profile dev-release cc12_chrome_perf_gate --
--ignored --nocapture`):

```
CC12_HOVER count=60 min=65.66ms p50=68.89ms p95=72.79ms p99=76.39ms max=76.39ms
CC12_KEY   count=60 min=65.95ms p50=68.27ms p95=73.90ms p99=86.11ms max=86.11ms
```

vs. the current `layout_measured_hyp` production baseline (`p50≈85/72ms`,
`p95≈116/82ms`) and the pre-fix `layout_mutation_incremental` attempt
(`p50=106/81ms`, worse than full layout) — this is now a genuine, real
**~20% improvement over full layout** (p50 85→69ms), confirming the diagnosed
root cause was correct and the fix works as intended.

**CC-12's gate is still red** (p95 ≈73-74ms vs the 2ms budget, ~35× over) —
20% off an already-2-order-of-magnitude-over-budget number doesn't close the
gap. The remaining cost is the full style cascade (`build_box`, unconditional
per `layout_mutation_incremental`'s own doc comment) plus whatever residual
`lay_out_flex` cost the CC-12 partial fix above didn't remove (column
Auto/Content items, row Auto/Content with explicit width — still genuinely
need Step-1). Only the general layout-result cache (see "Fix scope note"
above) can plausibly close the remaining ~35× gap; **not attempted here**.

Given `layout_mutation_incremental` is a real (if partial) win with no
observed regression risk in the existing test suite, but CC-12's gate stays
red either way, `relayout_chrome_host`/the CC-12 bench were **not** switched
over to it in this session — adopting it is a legitimate follow-up on its
own merits (worth doing once someone is touching that code path anyway) but
is a separate decision from unblocking CC-14, which needs the cache fix
regardless. **CC-14 remains blocked** on the layout-result cache.

## Impact

CC-track chrome rendering is currently opt-in (`LUMEN_CSS_CHROME=1`), not
shipped as the default chrome (CC-14 "Флип дефолта" has not happened) — so
there is no live-user impact from the *chrome* side yet. But per the root
cause above, this is a **general flexbox layout engine performance bug**,
not a chrome-only issue: any nested-flexbox page already pays the redundant
double-relayout cost today, scaled by its own flex-nesting depth and node
count (smaller than chrome's ~2,300-node document for most pages, so less
dramatic in absolute ms, but the same multiplicative mechanism). For the
chrome document specifically, if this cost is representative of what a real
interactive session would pay (the bench mirrors `relayout_chrome_host`'s
real call shape closely), every hover and keystroke on movement-driven
engine-rendered chrome would currently freeze the UI thread for 500ms+ —
this remains a **hard blocker for CC-14** until resolved, and should be
treated as high priority before that slice is attempted. Given the
general-engine scope, the fix likely belongs to whichever developer/slice
owns layout-engine performance work broadly, not narrowly to the CC track.

## S3 — incremental cascade v1 (conservative invalidation)

Landed the mechanism the S1/S2 profiling pointed at: `precompute_counters`
(53% of the cycle) recomputed every node's `ComputedStyle` on every call, with
no notion of "what changed". S3 adds an incremental entry point
(`lumen_layout::counters::incremental_precompute_counters` +
`RestyleDelta { prev_styles, dirty_roots }`) that reuses the previous cascade's
per-node `ComputedStyle` outside a conservative dirty root-set, and derives
that root-set for the two change classes in scope
(`style::restyle_root_set_for_state_change` for `:hover`/`:focus`/`:active`
transitions — ancestor-chain-toggle-aware, since `:hover` matches ancestors
too — and `style::restyle_root_set_for_node_change` for DOM attribute/class
mutations). Gated behind `INCREMENTAL_RESTYLE` (thread-local, off by default);
`incremental_precompute_counters` falls back to a full recompute when the flag
is off, so nothing observes any behaviour change until a caller opts in.

**Not wired into any pipeline yet** — `layout_measured_hyp` /
`layout_mutation_incremental` still call the full `precompute_counters`
unconditionally; that wiring (plus re-measuring `cc12_chrome_perf_gate`) is
S5. S3 is the cascade mechanism + differential tests + a standalone
measurement, deliberately scoped that way so S4 (box-build skip) lands before
the two get wired together.

**Correctness:** `cargo test -p lumen-layout` — 4 differential tests in
`incremental.rs` (`incr_cascade_matches_full_trivial`,
`incr_cascade_matches_full_interactive_rules`,
`incr_cascade_hover_transition_matches_full_and_recomputes_subset`,
`incr_cascade_class_change_matches_full_and_recomputes_subset`), the last two
exercising a *real* root-set (not the empty/steady-state case) and asserting
strictly fewer `compute_style` calls than a full cascade, not just equality.
Full crate suite: 3256 passed, 2 failed — both the pre-existing BUG-339
`FONT_CH_EX` flaky pair (confirmed pre-existing, unrelated to this change).

**Measured `precompute_counters` wall-time drop** (standalone, real chrome
doc via `lumen_chrome::parse_document` + `bind_model` with the CC-12 6-tab/
3-workspace fixture, dev-release,
`cargo test -p lumen-shell --profile dev-release
bug341_s3_incremental_cascade_precompute_share -- --ignored --nocapture`):

```
BUG341_S3_FULL_PRECOMPUTE        count=60 min=38.40ms p50=41.98ms p95=44.45ms
BUG341_S3_INCREMENTAL_PRECOMPUTE count=60 min=18.14ms p50=19.35ms p95=20.98ms
BUG341_S3: 828 nodes, dirty_roots=1; drop=53.9% (p50)
```

For a **representative** interaction — hover moving between two sibling tab
rows, the common case for real mouse movement over already-hovered chrome —
`precompute_counters` drops ~54%. Since S1 measured this stage at 53% of the
full cycle, this alone is roughly a ~29% cut of the whole
mutate→restyle→relayout→paint cost once wired in (S5), before S4's
`build_box` skip or S6's tightening.

**Documented worst case, not silently omitted:** the *same* measurement using
CC-12's own hover fixture shape (`SIDEBAR`/`None` toggle every cycle, as
`cc12_chrome_perf_gate_hover_and_keystroke_cycles` does) shows only **1.2%**
drop, `dirty_roots` covering most of the document. This is *correct* behaviour
of the v1 model, not a bug in it: `:hover` matches an element **and all its
ancestors** (CSS Selectors L4 §4.3), so a transition from "nothing hovered" to
"`SIDEBAR` hovered" flips the `:hover` boolean on every ancestor from
`SIDEBAR` up to the document root — conservatively invalidating from close to
the root is the correct (if expensive) answer for that specific transition
shape, not a defect in the invalidation logic. Real mouse movement over an
already-interactive page rarely produces a "nothing hovered" state except at
the very first mouse-enter of a session; CC-12's on/off toggle is a
simplification of the benchmark harness, not representative of steady-state
interaction. Worth revisiting when S5 wires this in: either give CC-12's
fixture a "something is always hovered" steady state (closer to real usage),
or treat the first mouse-enter transition as a one-time cost outside the
per-interaction budget. Flagging now so S5/S6 don't have to rediscover it.

## S4 — incremental box-build v1: mechanism correct, does NOT pay for itself yet

Landed the mechanism the brief §5 S4 asked for: skip `build_box` for subtrees
proven unchanged, and measure the `build_box` share drop. Both are done; the
measurement's answer is **negative** on the representative scenario — reported
honestly per the brief's "no silently relaxing the gate" rule, not hidden.

**Mechanism:**
- `CounterMap::clean_subtrees` (new field, `counters.rs`) — `walk` now returns
  whether a node's own style **and its whole descendant subtree** are
  byte-identical to `prev_styles` (bottom-up aggregation of `must_recompute`),
  populated only when `RestyleDelta::dom_content_stable` is `true` (new field).
- `dom_content_stable` is the S4 correctness precondition: style equality alone
  does not imply `build_box`'s *content* inputs (attributes, text, DOM
  structure — e.g. `<input value>`, `<select>`'s selected option, `<details
  open>`) are unchanged. It is only safe to assert for a pure interactive-state
  transition (`restyle_root_set_for_state_change`) — a hover/focus/active flip
  never touches the DOM. DOM/attribute-change deltas
  (`restyle_root_set_for_node_change`) must set it `false`; S4 then rebuilds
  every box, exactly like today (conservative-first, same precedent as S3).
- `build_box_or_reuse` (`box_tree.rs`) — the per-child reuse decision, wired
  into all 4 of `build_box`'s recursive call sites (rayon-parallel item-
  container, sequential item-container, inline-block-in-run, plain block-flow
  child). When enabled and a child id is in `clean_subtrees` with a matching
  entry in `prev_index`, clones the whole previous `LayoutBox` subtree instead
  of recursing. `prev_index` is `crate::incremental::index_by_node(prev)` — an
  id→box map built once per `incremental_build_box` call, keep-first in
  pre-order (anonymous boxes reuse their owning element's id, so the first,
  outermost occurrence in DFS order is always the real per-element box).
- Public entry point: `lumen_layout::box_tree::incremental_build_box`, gated
  behind `set_incremental_box_build`/`incremental_box_build_enabled` (off by
  default, same thread-local-flag pattern as S3's `INCREMENTAL_RESTYLE`).
- **Known residual gap, accepted for v1** (documented on `RestyleDelta::dom_content_stable`):
  a rule conditioning `counter-reset`/`counter-increment`/`counter-set` on a
  dynamic pseudo-class could change a later sibling's rendered counter text
  without changing that sibling's own `ComputedStyle`. Not exercised anywhere
  in this codebase's CSS; accepted as an unhandled exotic edge case rather than
  adding a full counter-snapshot equality check to the reuse guard.

**Not wired into any pipeline yet** — same S5 dependency as S3.

**Correctness:** `cargo test -p lumen-layout` — 2 new differential tests in
`box_tree.rs` (`box_build_hover_transition_matches_full_and_reuses_subset`,
`box_build_node_change_disables_reuse_conservatively`), comparing **laid-out
geometry** (`lay_out` + per-node rect, 0.5px epsilon) against a full rebuild —
not `Debug`-string equality, which false-positives here: `ComputedStyle`
carries `custom_props: HashMap<String, String>` (CSS custom properties), and
two independently-computed but content-equal cascades can produce different
`HashMap` iteration order, hence different `Debug` text, for the exact same
data (`HashMap`'s real `PartialEq` is order-independent; its `Debug` impl is
not). Full crate suite: 3258 passed, 2 failed — the same pre-existing BUG-339
`FONT_CH_EX` pair. A third differential test on the real CC-12 chrome doc
(`bug341_s4_incremental_box_build_share` in `lumen-shell`) also passes,
comparing node-id/`BoxKind`-discriminant/child-count shape (the geometry
comparison there would need the crate-private `lay_out`, not reachable from
`lumen-shell`; the two in-crate tests already cover bit-for-bit geometry).

**Measured `build_box` wall-time** (same CC-12 chrome doc + hover-between-
sibling-tabs transition as the S3 measurement, dev-release,
`cargo test -p lumen-shell --profile dev-release
bug341_s4_incremental_box_build_share -- --ignored --nocapture`):

```
BUG341_S4_FULL_BUILD_BOX        count=60 min=5.09ms p50=6.09ms p95=7.76ms
BUG341_S4_INCREMENTAL_BUILD_BOX count=60 min=5.38ms p50=7.32ms p95=10.40ms
BUG341_S4: full_build_box p50=6.09ms p95=7.76ms; incremental_build_box p50=7.32ms p95=10.40ms; drop=-20.3%
```

**Honest verdict: negative on this benchmark, not a win in isolation.**
`incremental_build_box` is *slower* than a full rebuild here. Root cause:
`index_by_node` walks and hashes the *entire* previous tree (~thousands of
nodes for the chrome doc) on every call to build the id→box map, and this
fixed O(n) cost is paid regardless of how small the actually-reused region is.
Since S1 measured `build_box` at only 8% of the full cycle to begin with, the
savings from skipping a modest fraction of that 8% do not cover the index-
build overhead. This is a real property of the "index by `NodeId`, rebuild
every call" design, not a benchmark artifact — the alternative (`graft_geometry`-
style positional/lockstep matching, avoiding any index) was considered and
rejected for this slice: `build_box`'s children lists are not 1:1 positionally
with DOM children in general (inline-run text merging, injected `::before`/
`::after`/`::marker` items interleave with per-child boxes in a way that only
resolves post-construction, which is exactly why `graft_geometry` itself
operates on two *already-built* trees rather than guiding construction).
Getting positional matching right during construction would need deeper,
riskier changes to all 4 `build_box` recursion sites than this slice's budget
allows.

**Recommendation for S5/S6:** land S3 (cascade) and S4 (this) both gated off by
default as done; do **not** flip `set_incremental_box_build` on by default when
S5 wires the pipeline together. Re-measure the *combined* cascade+box-build+
layout cycle at S5 — S3's ~54%-cascade-drop scenario plus S4's negative
contribution might still net positive or might not; that is an end-to-end
question S5 must answer with real numbers, not assume from this stage-isolated
measurement. If S4 nets negative even combined, the honest fix is S6: replace
`index_by_node`'s whole-tree hash index with an amortized or positional
alternative — or drop the box-build-skip idea and rely on S3 (cascade) +
`graft_geometry` (existing geometry reuse) alone, which the CC-12 gate math
(§ "S1" above) suggests may already be sufficient once wired in.

## S5 — pipeline wiring: real, measurable win on the representative case; CC-12's own worst-case fixture still red

Wired the S3 incremental cascade into a real pipeline entry point,
`lumen_layout::box_tree::layout_mutation_incremental_restyle` (new function,
`crates/engine/layout/src/box_tree.rs`) — same structure as
`layout_streaming_incremental`/`layout_mutation_incremental` (plain `build_box`,
`graft_geometry`, `lay_out_incremental`; S4's box-build skip deliberately left
off per that section's own recommendation), except the cascade call is
`counters::incremental_precompute_counters(..., &delta)` instead of a full
`precompute_counters`. 2 new differential tests in `incremental.rs`
(`mutation_incremental_restyle_hover_transition_matches_full`,
`mutation_incremental_restyle_unchanged_state_matches_full`) — geometry
comparison against a full `layout_measured_hyp`, same pattern as the existing
M4 tests, both passing. Full `lumen-layout` suite: 3260 passed, 2 failed (the
same pre-existing `FONT_CH_EX` pair).

`Lumen::relayout_chrome_host` (`crates/shell/src/main.rs`) now takes this path
when eligible: a new `chrome_prev_model: Option<ChromeModel>` field (comparing
by `PartialEq`, added to `ChromeModel` and all ~20 of its nested types in
`crates/chrome/src/model.rs`) stands in for a `bind_model` mutation-diff — an
identical model means this pass's `bind_model` call was a no-op, so nothing but
interactive state (hover/focus/active) could have changed the cascade. Also
guarded on viewport (`chrome_prev_viewport`) and Forced Colors Mode
(`chrome_prev_forced_colors`, a thread-local not reflected in `ChromeModel`) —
either changing forces the full-layout fallback, same as a model mismatch. The
dirty root-set is the union of `restyle_root_set_for_state_change` for each of
the three interactive-state axes (hover/focus/active) independently, since
chrome tracks them as three separate fields, not one combined transition.
`chrome_prev_pristine_layout` persists the *pre*-`take_content_area` tree
(cloned before that function prunes `#contentArea`) as next cycle's `prev` —
repeats the "attempted mitigation" section's own finding that grafting needs
the untrimmed tree to keep by-index matching aligned.

**Measured** (`cc12_bench_cycle` now mirrors `relayout_chrome_host`'s own
eligibility logic exactly — a `Cc12IncrementalState` struct threaded across
iterations, real per-cycle clone cost included in the timed region — so this
is what ships, not an isolated-mechanism number):

```
# CC-12's own fixture (SIDEBAR/`None` toggle every cycle — S3's documented
# conservative-invalidation worst case: transitioning from "nothing hovered"
# flips :hover on every ancestor of both old and new target):
CC12_HOVER count=60 min=78.80ms p50=84.35ms p95=90.12ms p99=93.97ms max=93.97ms
# CC12_KEY's own model changes every cycle (omnibox text grows) — always
# ineligible for the incremental path (no DOM-mutation-diff mechanism exists
# yet, only interactive-state), so this measures the plain full-layout cost,
# unchanged by S5 as expected:
CC12_KEY   count=60 min=78.84ms p50=82.46ms p95=88.45ms p99=92.34ms max=92.34ms

# Representative case (bug341_s5_incremental_pipeline_share): hover moving
# between two sibling tab rows, the shape real mouse movement over
# already-hovered chrome actually looks like:
BUG341_S5_SIBLING_HOVER count=60 min=58.94ms p50=63.82ms p95=76.34ms p99=92.75ms max=92.75ms
```

vs. the pre-S5 full-layout baseline (~85ms p50, S4's own measurement) —
**CC12_HOVER is flat (no win)**, exactly matching S3's own prediction that its
SIDEBAR/`None` toggle is a documented worst case for the v1 invalidation
model, not representative of real usage. **The representative sibling-hover
case gets a real ~25% p50 win** (85→64ms), consistent with S1's math: S3
measured a 54% cut to `precompute_counters`, which S1 found was 53% of the
full cycle, i.e. a ~28-29% cut of the whole cycle before other stages' own
cost (`build_box`, the still-standing residual `lay_out_flex` cost, the
per-cycle tree-clone this design requires) eats into that gain — the ~25%
measured here lines up with that math, not a coincidence.

**CC-12's gate is still red** on every fixture (p95 ≈76-90ms vs the 2ms
budget, ~40-45× over) — a real, honest win on the representative case does not
close a ~2-orders-of-magnitude gap. This is the outcome brief §5 S5 flagged as
possible ("if the p95 floor is set by irreducible per-node cascade cost...
report the number and re-open the budget question") — reported here rather
than silently relaxed. **CC-14 remains blocked.**

**Not attempted in S5** (would need their own scoped design):
- A `bind_model` mutation-diff so DOM-content changes (not just interactive
  state) could also use the incremental cascade — this is what keeps
  `CC12_KEY` (and any real chrome interaction that mutates tab titles, the
  omnibox value, etc.) on the full-layout path today. Likely the single
  biggest remaining lever, since typing/tab-list changes are common
  interactions this session's wiring does nothing for.
- S4's box-build skip — left off per that section's own recommendation;
  worth re-testing combined now that S5's harness exists, but not done here.
- The residual, non-doubled `lay_out_flex` cost (brief's original "Fix scope
  note" layout-result-cache idea) — still unimplemented, still the largest
  single remaining stage by S1's split once cascade is no longer dominant.

## S6 — `bind_model` mutation-diff: real win for content changes (`CC12_KEY`), `CC12_HOVER` unaffected as expected

Closed the single lever S5 flagged as its biggest remaining gap: `bind_model`
(`crates/chrome/src/model.rs`) had no way to report which nodes it actually
touched, so any DOM-content-changing chrome interaction (typed omnibox text, a
tab title, a new tab) fell back to the full-layout path regardless of S5's
wiring.

**`bind_model_tracked`** (new, alongside the unchanged `bind_model`) threads a
thread-local `HashSet<NodeId>` through the handful of shared low-level
mutation primitives already funneling every `bind_*` helper in that file —
`set_attr`/`remove_attr` (record the node when the attribute's value actually
changes, not on every call — `bind_model` writes idempotently every cycle
regardless of whether the value changed), `remove_children_with_class`
(record the container when it actually detaches a child), and
`reconcile_row_list` (record the container when `items.len() != existing.len()`,
i.e. a row was actually added/removed — a same-count reorder/update is already
caught by the per-row `set_attr` calls `update_tab_row`/`apply_tab_row_attrs`
make). Newly-*created* nodes need no explicit tracking:
`incremental_precompute_counters` (crates/engine/layout/src/counters.rs)
already force-recomputes any node absent from `prev_styles`, and
`build_box`/`graft_geometry` always read live DOM content regardless of the
cascade's dirty-root-set in this path (`dom_content_stable: false`) — verified
by two new full-pipeline differential tests in `incremental.rs`
(`mutation_incremental_restyle_dom_class_change_matches_full`,
`_structural_change_matches_full`), on top of the existing cascade-only S3 test
covering `restyle_root_set_for_node_change` (`incr_cascade_class_change_
matches_full_and_recomputes_subset`) that already validated the underlying
mechanism before any pipeline/chrome wiring existed.

`Lumen::relayout_chrome_host` and the `cc12_bench_cycle` test harness both
call `bind_model_tracked` instead of `bind_model`, union
`restyle_root_set_for_node_change(doc, touched)` into `dirty_roots` alongside
the existing per-axis interactive-state root-sets, and set
`dom_content_stable: touched.is_empty()`. The gate dropped is `chrome_prev_
model == model` (whole-`ChromeModel`-equality, S5's coarse stand-in for a real
diff) — the incremental path is now taken whenever a previous pristine tree
exists and viewport/Forced-Colors are stable, regardless of whether `bind_model`
changed content this cycle. `chrome_prev_model` (the field) is removed —
nothing reads model-equality anymore.

**A real bug surfaced by the first version of this tracker**: `bind_palette`
unconditionally removed+recreated its `.cp-empty` "nothing found" placeholder
on *every* `bind_model` call whenever `results` was empty (the overwhelmingly
common case — the palette is normally closed), even when the placeholder was
already showing. That made `#cpList` permanently "touched" every cycle,
needlessly widening `dirty_roots` on every pass including pure hover/focus/
active transitions. Fixed by skipping the remove+recreate when the placeholder
is already in place (`bind_palette`, `crates/chrome/src/model.rs`). Checked the
other five `remove_children_with_class` call sites (`bind_history` ×2,
`bind_bookmarks` ×2, `bind_dropdown`, `bind_downloads`) — none has an
unconditional-placeholder branch like `bind_palette`'s; an empty model list
there already produces a genuine no-op (0 removed, 0 created), so no similar
fix was needed for them. New tests:
`bind_model_tracked_reports_nothing_touched_for_an_unchanged_model`,
`bind_model_tracked_reports_the_body_on_a_theme_change`,
`bind_model_tracked_reports_the_container_when_a_tab_is_added`
(`crates/chrome/src/model.rs`) — the first one is what caught the palette bug.

**Measured** (same machine, same `cc12_bench_cycle`/`cc12_chrome_perf_gate_
hover_and_keystroke_cycles` harness as S5, 3 back-to-back runs after the fix to
separate signal from this machine's run-to-run noise floor — observed ±10-15%
between consecutive runs of *identical* code, see the numbers below):

```
# Baseline (this session, same machine, pre-S6 code, 2 runs):
CC12_HOVER p50=93.47ms / 89.68ms
CC12_KEY   p50=89.25ms / 90.58ms

# Post-S6 (3 runs):
CC12_HOVER p50=85.94ms / 85.87ms / 85.61ms   (~85.8ms average)
CC12_KEY   p50=63.23ms / 62.32ms / 62.85ms   (~62.8ms average)
```

`CC12_KEY` (typed omnibox text, changes every cycle — the case this slice
targets): **~30% p50 win** (~90ms → ~63ms), the first real improvement on this
fixture since BUG-341 opened (S3/S4/S5 measured it as flat, always ineligible
for any incremental path). `CC12_HOVER` (SIDEBAR/`None` toggle — S3's own
documented conservative-invalidation worst case): **flat**, within this
machine's noise floor of the pre-S6 baseline, exactly as expected — S6 does
not touch interactive-state root-set derivation, and `bind_model_tracked`
confirmed empty for this fixture (see the unit test above), so this fixture's
`RestyleDelta` is bit-identical to what S5 already computed for it.
`bug341_s5_incremental_pipeline_share`'s representative sibling-tab-hover
scenario is likewise unaffected (p50 ≈67ms, matching S5's own ~64-68ms).

**CC-12's gate is still red** (p95 ≈85-105ms vs the 2ms budget, still
40-50× over) — a real win on `CC12_KEY` does not close a ~2-orders-of-
magnitude gap on its own. Reported honestly per the brief's stop condition,
not silently relaxed. **CC-14 remains blocked.**

**Not attempted in S6** (left for a further slice if the budget question isn't
reopened first):
- `bind_history`/`bind_bookmarks`/`bind_downloads`/`bind_dropdown` still use
  the "remove all, rebuild all" pattern rather than `reconcile_row_list`'s
  per-row reconciliation — harmless for `dirty_roots` today (an empty model
  list is already a genuine no-op there, unlike the `bind_palette` bug this
  slice fixed), but a *non-empty* history/downloads/bookmarks list would touch
  its whole container on every cycle even when unchanged, the same shape of
  gap `bind_palette` had. Not exercised by any current perf fixture (`CC12_
  HOVER`/`CC12_KEY` both use the model's default-empty history/bookmarks/
  downloads), so left as a documented gap rather than spec-implemented ahead
  of a fixture that would prove it matters.
- S4's box-build skip — still off; `layout_mutation_incremental_restyle`
  always calls plain `build_box`, never `build_box_or_reuse`. Unaffected by
  this slice.
- The residual, non-doubled `lay_out_flex` cost and the cascade's own
  irreducible per-node cost on a real hover/content root-set — still the
  standing gap between "real, measured wins" and "closes a 2ms budget";
  brief §5's stop condition (re-open the budget question with data) is
  looking more relevant with each slice that narrows the invalidation set
  without closing the two-orders-of-magnitude gap.

## S7 (part 1) — page-side DOM-mutation tracker (`lumen_js::DomTouched`), not yet wired into the pipeline

S6 closed the chrome-side gap (`bind_model_tracked`); S7's brief item is the
same gap on the *page* side — JS DOM mutations via `v8_runtime.rs`. This
sub-slice built and tested the tracker only; **pipeline wiring is not done
yet** (see "Not attempted" below) — `Lumen::try_relayout_raf_incremental`
still always calls `lumen_layout::layout_mutation_incremental` (full cascade),
unchanged from S6.

**`lumen_js::v8_runtime::DomTouched`** (new, V8-only per this file's own
mandate to never target new functionality at the rquickjs path): a
`{ nodes: HashSet<NodeId>, unattributed: bool }` pair, drained by
`V8JsRuntime::take_dom_touched()`. Instruments the 9 native mutation
primitives whose effect on selector-relevant node state is precisely
attributable — `_lumen_set_attr`/`_lumen_remove_attr` (record only when the
value actually changed, mirroring `bind_model_tracked`'s `set_attr`/
`remove_attr`), `_lumen_append_child`/`_lumen_remove_child`/`_lumen_insert_before`
(record the parent — the container's `:empty`/nth-child state plus everything
`restyle_root_set_for_node_change`'s parent-subtree invalidation already
covers), `_lumen_set_text_content`/`_lumen_set_inner_html` (record the node
itself — text/childList changes can flip `:empty`), and the CSS Typed OM
`_lumen_set_style_property`/`_lumen_delete_style_property` (both funnel into
`set_attribute`/`remove_attribute` on `style` directly, bypassing the
`_lumen_set_attr` native, so needed their own change-detection). Verified:
`classList.add`/`className`/inline `style.color =` all route through
`_lumen_set_attr` already (checked against `dom.rs`'s JS shim), so the 9
tracked primitives cover the overwhelming majority of real JS mutation
patterns without touching the shim.

The remaining 13 `dom_dirty`-setting call sites — Shadow DOM attach,
`Selection`/Range get-set-clear, all 4 contenteditable key-handler bindings
(`insert_text`/`delete_backward`/`delete_forward`/`insert_paragraph`), and
`execCommand`'s 3 mutating branches (`selectAll`/`insertText`/`delete`) —
set `unattributed: true` instead: their effect on which nodes' cascade input
changed cannot be attributed to a simple node set (arbitrary-range
deletion/insertion, cross-shadow-boundary style scoping), so the *caller*
must fall back to a full cascade whenever `unattributed` is `true` for the
cycle — conservative-but-correct, same philosophy as brief §4. `Selection`
get/set/clear specifically is marked unattributed **out of caution, not proof**:
`compute_selection_style`'s `::selection` resolution is a per-node cascade
match unconditional on live selection state (verified in `style.rs`), which
suggests pure selection changes might not need any cascade invalidation at
all — but no differential test proves that yet, so this sub-slice took the
safe default rather than bet correctness on an unverified reading. A future
slice could special-case it if `CC12`-equivalent page fixtures show selection-
heavy interaction (click-to-place-caret, drag-select) matters.

12 new unit tests in `v8_runtime.rs` (`take_dom_touched_*`) exercise each
tracked primitive individually plus the no-op (same-value `setAttribute`,
removing an absent attribute) and unattributed cases, and that the tracker
clears between calls. `cargo test -p lumen-js --features v8-backend`: all
green. `cargo clippy -p lumen-js --features v8-backend` and
`cargo clippy -p lumen-shell`: clean.

**Not attempted in this sub-slice** — the actual pipeline wiring:
- No page-side equivalent of `chrome_prev_cascade_styles`
  (`HashMap<NodeId, ComputedStyle>`, `RestyleDelta::prev_styles`) exists yet.
  Unlike chrome (one entry point, `relayout_chrome_host`), the page has
  *multiple* layout-production sites that would all need to keep it in sync —
  `compute_layout`/`relayout()` (full), the engine-thread job
  (`submit_relayout_job`/`readback_relayout_job`/`poll_engine_commit`), and
  `try_relayout_raf_incremental` itself — or the cache goes stale and
  `layout_mutation_incremental_restyle` silently produces wrong output (the
  brief's own correctness gate: "any divergence is a bug in the invalidation
  set, not an acceptable trade-off"). Threading this through the engine-thread
  job additionally needs checking `ComputedStyle`/`CounterMap` are cheaply
  `Send`-movable across that channel — not yet investigated.
- `PersistentJs::take_dom_touched` and `DomTouchedSummary` are therefore
  currently dead code (`#[allow(dead_code)]`, documented pointer to this
  section) — added so the next sub-slice starts from a compiling, tested
  base instead of a from-scratch native-binding audit.
- Hover fan-out narrowing (the other half of S7's brief bullet) — not started.

## S7 (part 2) — page-pipeline wiring: `try_relayout_raf_incremental` takes the restyle path; new JS-driven differential test

Closes part 1's gap: `Lumen::try_relayout_raf_incremental` (`shell/src/main.rs`)
now actually calls `layout_mutation_incremental_restyle` instead of always
calling the plain graft-only `layout_mutation_incremental`.

**The cache-consistency problem part 1 flagged, and how this closes it.** The
page has far more layout-production sites than chrome's single
`relayout_chrome_host` — full `relayout()`, `try_relayout_raf_incremental`
itself, the engine-thread job (`submit_relayout_job`/`readback_relayout_job`/
`poll_engine_commit`), bfcache thaw, full page load (`apply_loaded_page`),
streaming (progressive-parse) layout, and hibernate restore. A cascade cache
(`RestyleDelta::prev_styles`) that goes stale relative to whichever
`layout_box` it is diffed against would silently produce wrong rendering —
this bug's own correctness gate. Rather than thread a `CounterMap` through
every one of those sites (large blast radius, several of which have no
`CounterMap` at all today — streaming layout uses a wholly different
incremental mechanism), the new `Lumen::page_prev_cascade_styles` field is
`Option<HashMap<NodeId, ComputedStyle>>`, trusted (`Some`) only in the one
cycle right after it was produced. `apply_relayout_result` — the sink every
"live relayout" producer funnels through (`relayout()`,
`try_relayout_raf_incremental`, `readback_relayout_job`, `poll_engine_commit`)
— invalidates it (`None`) unconditionally on *every* call; the restyle
sub-path of `try_relayout_raf_incremental` re-validates it immediately
afterward, only when it actually produced a matching `CounterMap` this cycle.
The remaining producers that bypass `apply_relayout_result` entirely (bfcache
thaw, `apply_loaded_page`, streaming layout, hibernate restore) each
invalidate it explicitly at their own `self.layout_box = …` site. `PageSnapshot`
carries the field (plus `page_prev_interactive`) across tab switches in
lockstep with `layout_box` itself, so a switch-back cannot resurrect a cache
that no longer matches the restored tree.

**The restyle decision** in `try_relayout_raf_incremental`: takes the
`layout_mutation_incremental_restyle` path only when `page_prev_cascade_styles`
is `Some` *and* `take_dom_touched()` reports `unattributed: false` — either
condition failing falls back to the existing `layout_mutation_incremental`
(full cascade + `graft_geometry`, always correct, just without the
cascade-skip win). `dirty_roots` unions the interactive-state delta
(hover/focus/active vs. `page_prev_interactive`, via
`restyle_root_set_for_state_change`) with the DOM-mutation delta
(`touched.nodes`, via `restyle_root_set_for_node_change`);
`dom_content_stable` is `true` only when `touched.nodes` is empty — identical
shape to `relayout_chrome_host`'s S6 wiring.

**Deliberately not wired**: the engine-thread job path
(`make_relayout_job`/`submit_relayout_job`/`readback_relayout_job`/
`poll_engine_commit`) always calls the full `compute_layout` — its closure
would need `prev`/`dirty_roots`/`CounterMap` sent across the thread boundary,
a genuinely separate design question (is `CounterMap` cheaply `Send`-movable
through the channel? does the dirty-root computation, which needs a `Document`
lock, belong on the UI thread before dispatch or the engine thread after?) —
left for a further slice if `LUMEN_ENGINE_THREAD=1` load-bearing perf ever
needs it. Since `apply_relayout_result` invalidates the cache on *every* call
including this path's, behavior there is byte-identical to before this
sub-slice (full cascade) — no correctness risk, just no speedup on that path.
Hover fan-out narrowing (S7's other brief bullet) also not started.

**New differential test** (`lumen-js`,
`v8_runtime::tests::dom_touched_drives_incremental_restyle_matching_full_cascade`):
unlike S3/S6's differential tests (synthetic Rust-built `RestyleDelta`s), this
one drives a real V8 `classList.add('active')` call, drains the actual
`take_dom_touched()` tracker, and asserts `layout_mutation_incremental_restyle`
reproduces a fresh full-cascade recompute's geometry exactly (plus a sanity
assertion that the fixture's CSS rule actually moves geometry, so the test
cannot pass vacuously on an empty delta).

**Verification**: `cargo test -p lumen-js --features v8-backend` — 2523
passed, 0 failed. `cargo test -p lumen-shell` — 1704 passed, 0 failed.
`cargo clippy -p lumen-js --features v8-backend --all-targets -- -D warnings`
and `cargo clippy -p lumen-shell --all-targets -- -D warnings` — both clean.
`python graphic_tests/run.py --continue-on-fail` — see this file's own log for
the result recorded alongside the merge commit.

## S7 (part 3) — hover fan-out narrowing

Closes the other half of S7's brief bullet (part 1/2 above did the page-side
diff mechanism). `restyle_root_set_for_state_change`'s v1 model (S3)
unconditionally widened every flipped ancestor node `N` to `N`'s *parent's*
whole subtree, to cover selectors like `N:hover + X`/`N:hover ~ X` without a
selector-dependency index. That index now exists:
`lumen_layout::style::restyle_state_needs_fanout(doc, sheet)` scans every
selector in `sheet` (including every `@media`/`@layer`/`@supports`/`@scope`/
`@starting-style`/`@container` block) for a compound depending on dynamic
interactive state (`:hover`/`:focus`/`:active`/`:focus-within`/
`:focus-visible`, including inside `:not()`/`:is()`/`:where()`/`:host()`/the
`of <selector-list>` clause of `:nth-child()`) that is followed *anywhere* on
the path to the selector's subject by a sibling combinator (`+`/`~`) — the
only shape that can reach outside the flipped node's own subtree. A
descendant combinator after such a compound (`N:hover .icon`) stays inside
`N`'s own subtree and needs no widening; a compound in subject position
(`N:hover` itself, nothing after it) likewise needs none. `:has()` containing
a dynamic-state pseudo anywhere always forces widening (its search direction
isn't modelled here — see the new [BUG-349](BUG-349-OPEN.md), a pre-existing,
unrelated gap this work surfaced in the *DOM-mutation* sibling function,
`restyle_root_set_for_node_change`, not this one). A document with any shadow
root also always forces widening (shadow-tree stylesheets aren't scanned —
deferred, no fixture needs it yet). When none of that applies,
`restyle_root_set_for_state_change`'s new `needs_fanout: bool` parameter
narrows each flipped node's own invalidation to just that node, not its
parent's whole subtree.

Computed once per interactive-state transition (not once per hover/focus/
active axis) and threaded through every production call site:
`Lumen::relayout_chrome_host`, `Lumen::try_relayout_raf_incremental`, and the
CC-12 bench harness's `cc12_bench_cycle`.

**Correctness**: 16 new unit tests (`style::state_fanout_tests`) cover every
combinator/pseudo-class shape from the brief's model — subject-position and
descendant-position dynamic-state compounds (no fanout), `+`/`~` immediately
or transitively after one (fanout), `:is()`/`:not()` wrapping a dynamic-state
compound, `:has()` with/without a nested dynamic-state pseudo, an `@media`
block, shadow-root presence, and two end-to-end assertions against
`restyle_root_set_for_state_change` itself: real chrome-shaped CSS
(`.tab-row:hover` + `.tab-row:hover .tab-close`, matching
`assets/chrome/chrome.html`'s actual pattern) narrows to exactly the flipped
nodes, while `.item:hover + .item` still widens to their shared parent. Ran
the full existing differential-test suite unchanged
(`cargo test -p lumen-layout` — 3278 passed, only the pre-existing
BUG-339 `FONT_CH_EX` flakes fail, same as before this slice) and
`cargo test -p lumen-shell` — 1704 passed, 0 failed. Confirmed
`assets/chrome/chrome.html` (the real chrome stylesheet) has zero `:hover`/
`:focus`/`:active` selectors followed by a sibling combinator — every one of
its ~35 dynamic-state rules is either subject-position or descendant-only —
so `restyle_state_needs_fanout` returns `false` for it and every chrome
restyle now narrows for real, not just in the synthetic test fixtures.

**Measured — honest result, no clear win on CC-12's own fixtures**: an A/B
comparison on the same machine, same code, differing only in the
`needs_fanout` boolean passed to `bug341_s3_incremental_cascade_precompute_
share`'s sibling-tab-hover fixture (`#sbTabs` has 6 tab rows; hover moves
between the first two):

```
# needs_fanout=true (pre-S7 behaviour, dirty_roots=1 — the shared #sbTabs
# container, whose "dirty root" subtree covers all 6 tab rows):
BUG341_S3: incremental_precompute p50=20.84ms / p50=17.86ms (drop=47.6% / 54.1%)

# needs_fanout=false (S7 narrowing, dirty_roots=2 — just the two tabs that
# actually flipped):
BUG341_S3: incremental_precompute p50=19.33ms / p50=18.70ms (drop=54.5% / 51.9%)
```

Statistically indistinguishable given this machine's documented ±10-15%
run-to-run noise floor (S6's own finding) — narrowing dirty_roots from "the
6-tab container's subtree" to "the 2 changed tabs" doesn't move this
fixture's wall-clock at all. `CC12_HOVER` (the SIDEBAR/`None`-toggle gate
fixture) is unaffected for the same reason S3 already documented it as a
conservative-invalidation worst case: transitioning from "nothing hovered"
flips `:hover` on *every* ancestor of the target, and none of those ancestors
carry a `:hover`-triggering rule in `chrome.html`, so their cascade cost was
already near zero before this change — narrowing an already-cheap operation
has nothing to save. Re-ran `cc12_chrome_perf_gate_hover_and_keystroke_cycles`
post-narrowing: `CC12_HOVER p50=91.61ms p95=103.14ms` — flat vs. S6's
`~85.8ms` average, within noise; **gate stays red** (~45-50× budget), no
regression, no closure.

**Why this still matters despite the flat CC-12 numbers**: the win this
narrowing targets — an old sibling's *entire subtree* getting swept into
"dirty" purely because it shares a parent with the node that actually
changed — doesn't materialize on `chrome.html`'s small, shallow tab bar (6
siblings, thin per-tab subtrees), but scales with how much irrelevant content
hangs off the shared parent. A real page with a long `:hover`-styled list
(hundreds of rows under one container, each with non-trivial markup) is
exactly the shape where "widen to parent" used to force a full-list
recascade on every mouse move and this narrowing now doesn't. No such
fixture exists in this codebase yet to measure directly — recorded here as
the honest theoretical case, not a silently-assumed win (brief §5's own
stop-condition: report the number, don't relax the gate).

**Also not attempted here**: the DOM-mutation-change counterpart
(`restyle_root_set_for_node_change`) has no equivalent narrowing — every
class/attribute mutation still widens to the mutated node's parent
unconditionally. That function's over-approximation is cheaper to begin
with (one widen per mutated node, not one per ancestor in a state-change
chain) and no fixture has flagged it as a bottleneck; left as a further
slice if one does. BUG-349 (found while re-verifying this function's own
doc comment) is a separate, unrelated correctness gap in the same function,
not a performance one.

S7 (parts 1-3) is now complete — the diff mechanism for page-side JS
mutations and the hover fan-out narrowing brief bullet are both done. CC-12's
gate remains red on every fixture; see the "Also not attempted" lists across
S5/S6/S7 for the standing remaining levers (box-build skip re-evaluation,
the residual `lay_out_flex` cost / layout-result cache, DOM-mutation-change
narrowing).

## S8 — `graft_geometry` was reusing nothing at all: two defects, both fixed

**This slice changed the diagnosis of the whole track.** S1's profile — the one
every slice from S3 onwards reasoned from — describes the **full** layout pass.
Nobody had ever profiled the *incremental* path itself. Adding the same stage
scopes to `layout_mutation_incremental_restyle` (committed, so this stays
visible) produced this, on today's code, post S1-S7:

```
# cargo test -p lumen-shell --profile dev-release cc12_chrome_perf_gate -- --ignored --nocapture
# with LUMEN_PROFILE_TREE=1; averages over the 60 recorded cycles of each fixture
HOVER  total=111.4  cascade=59.9  lay_out=42.0  build_box=9.0  graft=0.04  post=1.2
KEY    total= 64.8  cascade=23.8  lay_out=33.0  build_box=6.7  graft=0.04  post=1.0
# outside `lumen-layout`, inside the timed region (new `[cc12-split]` line):
KEY    clone_tree=4.2  clone_styles=8.0  paint=0.5
```

`graft=0.04ms` over a whole document is the finding: **the incremental-layout
half — the one this task's brief §2 called "mature" and built everything else
on top of — was inert.** It reused zero boxes, on every interaction, for the
entire history of this bug. Two independent defects, either of which alone
drives reuse to zero:

**1. `kind_layout_eq` was missing 6 of `BoxKind`'s 20 variants.** `Contents`,
`Table`, `TableRowGroup`, `SvgRoot`, `SvgShape`, `SvgText` fell into its
`_ => false` arm. That is not a local loss: `graft_geometry` propagates a
failure up to every ancestor, so a single unlisted kind anywhere disables
incremental layout for the whole tree — and `assets/chrome/chrome.html` is
built out of SVG icons. Fixed by handling all six, with `PartialEq` derived on
`ViewBox`/`SvgTransform`/`SvgShapeKind`. `svg_paint_matrix` is deliberately
**excluded** from the comparison: it is a layout *output* (identity on a
freshly-built box, written during `lay_out`, see `box_tree.rs` "Stored in the
dedicated `svg_paint_matrix` output field"), so comparing it would make every
SVG shape unequal to its own laid-out predecessor — the same trap that makes
`InlineRun` compare `segments` and not its laid-out `lines`. Without that
exclusion the fix would have been a silent no-op.

**2. `graft_geometry` returned *before* recursing into children on any style
mismatch.** `lay_out` writes the used viewport `height` back into the root
box's `ComputedStyle`, so a freshly-built root can never equal its own
laid-out predecessor — every cycle, guaranteed, on any document. One node at
the root therefore threw away geometry reuse for the entire document. Fixed:
a style mismatch now marks only that node dirty and still recurses; each child
is judged on its own `style`, which already carries the new cascade's result.
Node-identity and box-kind mismatches still abandon the subtree (positional
child matching is no longer meaningful there).

**Why S2-S7's differential tests never caught this.** They assert
`incremental == full` on the *output*. A graft that reuses nothing satisfies
that perfectly — it just recomputes everything. The regression was invisible in
geometry and only observable as wall-clock, where this machine's ±10-15%
noise floor hides it. Added a direct gate on the *count* instead:
`graft_geometry_reuses_whole_chrome_tree_when_nothing_changed`
(`crates/shell/src/main.rs`) lays out an unchanged chrome document twice and
asserts **100%** of boxes graft clean. Before the fixes: 217/318 boxes, and
`false` at the root. After: 318/318. Two unit tests in `incremental.rs` cover
the newly-handled kinds, the `svg_paint_matrix` exclusion, and the
style-mismatch-still-grafts-children contract.

**Measured** (same harness/machine as S5-S7):

```
lay_out         31ms -> 16ms
CC12_HOVER p50  117.6ms -> 94.5ms
CC12_KEY   p50   78.7ms -> 69.7ms
```

`cargo test -p lumen-layout`: 3281 passed, 2 failed (the pre-existing
`FONT_CH_EX` pair, BUG-339). `clippy` clean on `lumen-layout` + `lumen-shell`.

**Full §6 verification, run before the merge (2026-07-27):** `clippy` clean on
`lumen-layout` + `lumen-shell` + `lumen-chrome`; `cargo test -p lumen-chrome`
58 passed / 0 failed; `cargo test -p lumen-shell` 1705 passed / 0 failed;
CPU snapshot references (`lumen-driver --features cpu-render
cases::snapshot_cpu`) **unchanged** — the deterministic pixel gate confirms
geometry is bit-identical, which is the exact contract a geometry-reuse change
has to keep. `python graphic_tests/run.py --continue-on-fail` (149 tests,
`LUMEN_PROFILE=dev-release`): 77 known debtors all within tolerance, **no new
regressions**; the two non-debtor lines are unrelated to this slice —
TEST-147 27.45% is the pre-existing, still-OPEN BUG-330 (identical number to
the one BUG-330 already records from the DS-9 branch), and TEST-71's "FAIL" is
a *ratchet* verdict, i.e. an improvement (BUG-199 debtor 4.53% → 2.11%) that
the harness reports so its baseline gets lowered. The TEST-71 baseline was
deliberately left untouched: BUG-199's own diagnosis is that the residual diff
is an Edge capture-timing artifact, so ratcheting on a single observation
risks a spurious REGRESS later, and it is not this slice's finding.

**CC-12's gate stays red** (2ms budget, ~35-47× over). Honest accounting: this
is a real, structural fix — the incremental-layout machinery now actually
works, which also benefits page load and every real page, not just chrome —
but it does not close a two-orders-of-magnitude gap on its own.

### What S8 exposed next (start here)

The post-fix split moves the bottleneck, and the new top item is the same root
cause in two places:

```
HOVER  total=85.5  cascade=49.1  lay_out=16.8  graft=14.1  build_box=6.8
KEY    total=59.4  cascade=22.0  lay_out=15.6  graft=13.6  build_box=6.2
```

`graft` went 0.04ms -> ~14ms because it is now doing real work: comparing a
**3216-byte, 302-field `ComputedStyle` — including a `HashMap<String, String>`
of 30 inherited custom properties — once per box.** The same fat struct is what
makes `clone_tree` + `clone_styles` cost ~12-13ms per cycle (the incremental
design's own bookkeeping: `state.prev_pristine_layout = layout.clone()` and
`state.prev_cascade_styles = counters.styles().clone()`, both O(whole
document), both inside the timed region), and it is a large part of `cascade`
too — `compute_style` clones the parent's `custom_props` map per node
(`style.rs:6198`, `style.rs:7678`).

Micro-benchmark of `compute_style`'s per-node cost by how much heap-owning
inherited state the parent carries (dev-release, 3 runs, 2300 calls each; the
first cold-machine run read ~5x higher and was discarded):

| inherited state | per node |
|---|---:|
| 0 custom props, 0 font families | 0.31-0.46 µs |
| 0 props, 5 font families | 0.70-0.87 µs |
| **30 props (what chrome.html declares), 5 families** | **3.7-4.7 µs** |
| 120 props, 5 families | 20.2-21.8 µs |

So the recommended next slice (**S9**) is representation, not invalidation:

1. `custom_props: Arc<HashMap<...>>` with copy-on-write (`Arc::make_mut`) —
   only the handful of nodes that actually declare a custom property pay a
   real copy. This gives `graft`'s style comparison an `Arc::ptr_eq` fast path
   (should collapse most of the ~14ms), cuts the per-node cascade cost, and
   shrinks both bookkeeping clones. Blast radius is small: 8 `custom_props`
   sites outside `style.rs`.
2. Then re-measure and consider `LayoutBox.style: Arc<ComputedStyle>` and
   `prev_cascade_styles: HashMap<NodeId, Arc<ComputedStyle>>`, which turns both
   O(document) bookkeeping clones into refcount bumps.

Unlike every slice from S3 to S7, this is a pure data-representation change:
output must stay bit-identical, so the existing differential tests are the
gate, and there is no invalidation-correctness risk to trade off.

## S9 — `ComputedStyle` made cheap to clone and to compare

Both steps S8 recommended, implemented and measured. Pure data-representation
change; no invalidation set was touched, and no output changed.

### 1. `custom_props` behind an `Arc` with copy-on-write

New type `lumen_layout::style::CustomProps` (`Arc<HashMap<String, String>>`,
`Deref` for reads, `make_mut` for writes) replaces the bare `HashMap` in
`ComputedStyle::custom_props` and in `ContainerContext::custom_props`.

CSS Variables L1 makes every custom property inherited, so `compute_style`
copied the parent's whole map into every node — the cost S8 measured at
3.7–4.7 µs/node with chrome's 30 properties vs 0.31–0.46 µs with none.
Inheritance is now a refcount bump; only the nodes that actually declare a
`--name` fork the map. Three write sites take the CoW path
(`style.rs`: the custom-property pass, `apply_property_initial_values`, and the
`inherits: false` `retain`); the `retain` is additionally guarded by a
"would this actually drop a key?" scan, so a page that registers `@property`
rules it does not use still shares.

Two extra wins fell out of the representation:

* `CustomProps::default()` is a process-wide singleton, so every node in a
  document that declares no custom property at all shares one allocation.
* `PartialEq` short-circuits on `Arc::ptr_eq` before comparing contents. That
  is spelled out in `CustomProps::eq` rather than left to `Arc`'s own
  (unspecified) pointer specialisation, so `graft_geometry`'s per-box
  `ComputedStyle` comparison has a guaranteed fast path.

### 2. Cascade cache holds `Arc<ComputedStyle>`

`CounterMap::styles` is now `HashMap<NodeId, Arc<ComputedStyle>>` (and so is
`RestyleDelta::prev_styles`, `Lumen::chrome_prev_cascade_styles`,
`Lumen::page_prev_cascade_styles`, `PageSnapshot::page_prev_cascade_styles`).
Three consumers that each deep-copied a whole `ComputedStyle` per node became
refcount bumps: the incremental cascade's reuse path in `counters::walk` (taken
for *every* node outside the dirty root-set), the map's own insert, and the
pipeline's per-cycle `counters.styles().clone()` snapshot.

`LayoutBox.style` was deliberately **not** converted. `lay_out` writes used
values back into a box's own style (S8 found the viewport-height write on the
root), so it needs owned, mutable styles; sharing them is a separate design,
not a mechanical change. With bookkeeping now at ~1.5 ms it is also no longer
where the money is.

### Gates: by identity, not by output

S8's lesson applied directly. Every existing differential test compares cascade
*output*, and a mechanism that deep-copies instead of sharing satisfies all of
them — it is merely slow, which is exactly how `graft_geometry` stayed inert
for five slices. So the sharing itself is asserted by pointer:

| Test | Asserts |
|---|---|
| `style::tests::custom_props_shared_with_parent_when_child_declares_none` | child shares the parent's map allocation |
| `style::tests::custom_props_copy_on_write_when_child_declares_one` | child forks, and the fork is not visible upwards |
| `style::tests::custom_props_empty_map_is_a_shared_singleton` | no per-node allocation for property-free documents |
| `style::tests::custom_props_eq_compares_contents_when_not_shared` | the pointer check is a fast path, not the semantics |
| `incremental::tests::incr_cascade_reuse_hands_back_the_same_style_allocation` | nodes outside the dirty root-set come back as the *same* `Arc`; nodes inside it do not, and their recompute lands on the same value |

### Measurement

Wall-clock had to be taken as an interleaved A/B — a parallel session was
loading the machine, and non-interleaved runs of *unmodified main* varied
between p50 71 ms and p50 109 ms on `CC12_HOVER`, i.e. the noise alone was
larger than the effect S3–S7 were chasing. Three alternating rounds
(main, S9, main, S9, main, S9), dev-release:

| | main min | S9 min | main p50 | S9 p50 |
|---|---:|---:|---:|---:|
| `CC12_HOVER` | 66.4 / 81.1 / 69.1 | **26.4 / 29.6 / 26.3** | 71.5 / 109.2 / 100.0 | **36.8 / 33.5 / 28.8** |
| `CC12_KEY` | 48.9 / 51.7 / 55.9 | **11.3 / 14.6 / 11.2** | 54.5 / 60.0 / 84.6 | **16.8 / 18.1 / 12.5** |

≈2.6× on `CC12_HOVER`, ≈4.2× on `CC12_KEY` (comparing mins, the quantity least
contaminated by contention).

Per-stage, from a quiet-machine pair of runs (`[cc12-split]` medians, and
`LUMEN_PROFILE_TREE=1` stage medians — the tree numbers carry their own
instrumentation overhead and are only comparable to each other):

| | main | after step 1 | after step 2 |
|---|---:|---:|---:|
| `clone_styles` (HOVER / KEY) | 9.30 / 9.97 | 3.76 / 3.13 | **0.95 / 0.04** |
| `clone_tree` (HOVER / KEY) | 5.03 / 5.21 | 2.16 / 1.75 | **1.71 / 1.40** |
| `precompute_counters` (tree) | 45.73 | 20.23 | — |
| `lay_out` (tree) | 22.80 | 4.00 | — |
| `graft_geometry` (tree) | 2.52 | 0.95 | — |

Note `lay_out` dropping 22.8 → 4.0 ms: it was not "layout" cost at all, it was
`ComputedStyle` copying inside layout.

### Where this leaves CC-12

Still red. Budget is 2 ms; `CC12_KEY` is now ~11–17 ms and `CC12_HOVER`
~26–37 ms — the gap is ~8–20×, down from ~300× when BUG-341 was filed and
~35–50× before this slice. The remaining top item is the cascade itself
(`precompute_counters`), i.e. the `compute_style` calls for whatever *is*
dirty, and beneath it the per-node `ComputedStyle` construction cost: the
struct is still 302 fields with ~30 heap-owning ones (`Vec<String>` font
families, transform lists, background layers, shadows), each cloned from the
parent on every inherit. That — not another invalidation-narrowing slice — is
the next thing to profile.

## S10 — the per-node pseudo-element cascades

S10 was planned as "the cost of building one `ComputedStyle`" — S9 left the
cascade on top, and the struct is 302 fields with ~30 heap-owning ones cloned
from the parent on every inherit. The brief's own rule (profile the path you
change, BUG-341 "S8") was followed first, and it overturned the plan: building
the style is **3%** of `compute_style`. More than half of it is pseudo-element
cascades run per element for a feature almost nothing uses.

### Profiling the profiler first

Instrumenting `compute_style` per node made `build_box` report **288 ms**
against a true ~5 ms. `lumen_core::profile`'s call tree is thread-local, so a
scope opened on a rayon worker (`build_box` parallelises the per-child cascade)
starts a *root* frame there and prints a whole tree per call — hundreds of trees
per pass, with the stderr writes landing inside the stage being measured. Three
changes made the utility usable at per-node granularity:

* same-named sibling scopes merge, printing one line with a `×N` call count;
* scopes on threads other than the first profiled one are ignored (their time
  still shows up in the enclosing stage, where it belongs);
* the per-node scopes sit behind `LUMEN_PROFILE_DETAIL=1`, so a plain
  `LUMEN_PROFILE_TREE=1` stage run stays comparable with the numbers recorded
  in the slices above.

### What the incremental path actually spends (before S10)

`layout_mutation_incremental_restyle`, dev-release, medians over 69 cycles.
Detail rows come from a `LUMEN_PROFILE_DETAIL=1` run and carry their own
overhead — read them as shares, not absolutes.

| Stage | `CC12_HOVER` | `CC12_KEY` |
|---|---:|---:|
| **`precompute_counters`** | **28.2 ms (68%)** | 3.7 ms |
| `lay_out` | 6.4 ms | 4.7 ms |
| `build_box` | 5.5 ms | 3.8 ms |
| `graft_geometry` | 1.4 ms | 0.7 ms |
| whole pass | 41.5 ms | 14.3 ms |

Inside `precompute_counters` on `CC12_HOVER` (828 nodes recomputed):

| | ms | share of `compute_style` |
|---|---:|---:|
| `cs_post` → `::-webkit-scrollbar*` cascades | 11.95 | **55%** |
| `cs_apply` (declarations) | 3.49 | 16% |
| `cs_match` (selectors) | 2.77 | 13% |
| `cs_revert_prepass` | 1.37 | 6% |
| **`cs_init` (the 302-field literal + inherit clones)** | **0.66** | **3%** |
| `cs_ua_hints` | 0.45 | 2% |
| `quote_pseudos` (`::before`/`::after` probe, outside `compute_style`) | 3.65 | — |

Two structural findings, neither visible in any differential test:

1. **`apply_webkit_scrollbar_pseudos` ran three full pseudo-element cascades on
   every element.** CC-CSS-1 translates `::-webkit-scrollbar`/`-thumb`/`-track`
   onto `scrollbar-width`/`scrollbar-color`; `assets/chrome/chrome.html` writes
   those rules *bare* (universal subject), so all three matched on all 828
   nodes and were fully applied — to set two inherited fields.
2. **`counters::walk` probed `::before`/`::after` on every node**, including
   nodes whose style was reused wholesale, solely to keep the quote-nesting
   counter continuous. On `CC12_KEY` — a dozen nodes recompute, 828 get probed —
   that was **79% of the cascade stage**.

### The slice

* `compute_pseudo_element_style` matches **before** building the pseudo's
  starting style (extracted as `pseudo_inherited_style`). The overwhelmingly
  common outcome is "no rule matched", which used to build a 302-field style
  and throw it away.
* Sheet-level, node-independent facts precomputed once per sheet in
  `CascadeIndex`: `has_webkit_scrollbar_rules` (false for every sheet that is
  not Lumen's chrome → all three cascades skipped outright) and
  `has_quote_content` (false for essentially every sheet → the `::before`/
  `::after` probe skipped; deliberately over-approximating, since `var()` and
  `attr()` can smuggle a quote in from anywhere).
* When every `::-webkit-scrollbar*` rule selects node-independently (bare
  selector, no combinator, no `attr()` in its declarations), an element whose
  pseudo-inheritance base is identical to its parent's must compute identical
  scrollbar values — and `scrollbar-width`/`scrollbar-color` are inherited, so
  those values are already in the style. It reuses them instead of cascading.
  The check compares two *constructed bases* rather than a hand-listed field
  set, so a property added to `pseudo_inherited_style` later is covered
  automatically instead of silently weakening it. The root element always
  cascades: its `inherited` is a synthetic `ComputedStyle::root()` that never
  went through this function, and reuse then chains inductively down the tree.
* The `revert-layer` pre-pass allocated a lowercased `String` per matched
  declaration plus a `HashMap` to discover, on every element of every real
  page, that nothing declares `revert-layer`. One allocation-free scan gates it.

### Gates: by count, not by output

Doing this work and discarding the result produces byte-identical output — the
same trap as `graft_geometry` in S8 — so the mechanism is asserted by counters:

| Test | Asserts |
|---|---|
| `style::tests::webkit_scrollbar_cascade_skipped_when_sheet_declares_none` | zero pseudo cascades when no `::-webkit-scrollbar*` rule exists |
| `style::tests::webkit_scrollbar_cascade_reused_from_parent_when_base_matches` | exactly **one** element cascades for a bare rule — and every element still ends up with the rule's effect |
| `style::tests::webkit_scrollbar_cascade_not_reused_when_rules_are_node_dependent` | a class-qualified rule disables reuse for every element |
| `style::tests::pseudo_base_not_built_when_no_rule_matches` | the starting style is not built when nothing matched (and is built when something does) |
| `style::tests::sheet_quote_content_flag_tracks_declarations` | the quote flag tracks declarations, including the over-approximating `var()`/`attr()` cases |
| `profile::tests::same_named_siblings_merge_with_a_call_count` | scope merging sums time and count recursively |

`cargo test -p lumen-layout`: 3291 passed, 2 failed — `ch_approximated_as_half_em`
and `ex_approximated_as_half_em`, the pre-existing BUG-339 flake documented in S5.

### Measurement

Interleaved A/B (main, S10, main, S10, main, S10), dev-release, comparing mins:

| | main min | S10 min | main p50 | S10 p50 |
|---|---:|---:|---:|---:|
| `CC12_HOVER` | 40.1 / 48.6 / 45.3 | **37.7 / 36.1 / 36.2** | 63.1 / 55.0 / 53.8 | **45.9 / 42.9 / 42.7** |
| `CC12_KEY` | 20.9 / 19.6 / 19.0 | **16.4 / 16.9 / 15.5** | 28.8 / 24.5 / 23.2 | **19.0 / 19.5 / 18.4** |

≈20% on both scenarios. Per stage (`LUMEN_PROFILE_TREE=1`, medians):

| | before | after |
|---|---:|---:|
| `precompute_counters` (HOVER) | 28.2 ms | **19.7 ms** |
| `precompute_counters` (KEY) | 3.69 ms | **0.75 ms** |
| `cs_scrollbar_pseudos` (HOVER, detail) | 11.95 ms / 828 elements | **5.76 ms / 356 elements** |
| `cs_revert_prepass` (HOVER, detail) | 1.37 ms | **0.34 ms** |
| `quote_pseudos` | 3.65 ms | **0 (skipped)** |

### Where this leaves CC-12

Still red. Budget 2 ms; `CC12_HOVER` is ~36-46 ms and `CC12_KEY` ~15-19 ms — a
gap of ~8-20×, the same order as after S9: this slice bought ~20%, not an order
of magnitude.

The top item on `CC12_HOVER` is still the scrollbar translation: 356 of 828
elements have a pseudo-inheritance base that differs from their parent's (chrome
varies `font-size`/`color` a lot) and so still cascade, at 5.8 ms, plus ~2.4 ms
for the base comparison itself. Two ways out, in increasing order of what they
cost to decide:

* **Cheap and exact:** memoise the parent's constructed base by pointer —
  siblings rebuild the identical base today, which is most of that 2.4 ms.
* **Big but semantic:** run the translation only for elements that can actually
  have a scrollbar (`overflow` not `visible`, plus the root, `<body>` and text
  controls). That is the 5.8 ms, and it is closer to WebKit, where
  `::-webkit-scrollbar` styles the matched element's own scrollbar and does
  *not* inherit — Lumen's inheritance of it is an artifact of translating onto
  standard inherited properties. It is a behaviour change, so it needs an
  explicit decision rather than being folded into a perf slice.

Below that, `cs_apply` (3.7 ms) and `cs_match` (3.0 ms) are the next cascade
items, and `lay_out` (5.3 ms) + `build_box` (3.6 ms) together now rival the
cascade stage. `cs_init` — the premise S10 was planned on — remains 0.76 ms and
is not worth a slice.

## S11 — `::-webkit-scrollbar*` only where a scrollbar can appear

S10 left the scrollbar translation as the top item on `CC12_HOVER`: 356 of 828
elements still ran three pseudo-element cascades because their
pseudo-inheritance base differed from their parent's, and the base comparison
that spared the other 472 cost ~2.4 ms by itself. S10 laid out two ways
forward; the user chose the semantic one (2026-07-27).

### The decision

Recorded as [ADR-022](../docs/decisions/ADR-022-webkit-scrollbar-scroll-containers-only.md).
The translation now runs only for elements that can actually show a scrollbar —
`overflow-x`/`overflow-y` is `scroll` or `auto` (exactly the condition
`lumen_paint::display_list::emit_scrollbars` and
`box_tree::scrollbar_gutter_{inline,block}` use), plus the root element and
`<body>` unconditionally, since those are the conventional target for styling
the *page* scrollbar. One definition: `style::element_can_have_scrollbar`.

It is a behaviour change, not a pure optimization. CC-CSS-1 translates the
pseudo-elements onto the standard `scrollbar-width`/`scrollbar-color`, which are
**inherited** (CSS Scrollbars L1 §2), so a rule matching a *non-scrollable*
element used to write its result there and leak it to every descendant —
including scrollable ones that matched no rule of their own. WebKit has no such
inheritance, so the narrowing is also a fidelity fix.

What did *not* change: the properties themselves still inherit. A bare
`::-webkit-scrollbar { … }` — the common page idiom, and what
`assets/chrome/chrome.html` writes — matches `<body>`, and the value reaches the
whole page from there exactly as before. Lumen's chrome and both graphic tests
(51, `1000000-final`) style scroll containers directly, so nothing they render
changes.

S10's node-independence fast path was **removed**, not kept alongside: it reused
"the parent's already-computed result", which is only sound when some ancestor
actually computed one. Under narrowing most ancestors no longer do.

### Gates

Counter gates as in S10, plus the two halves of the new semantics:

| Test | Asserts |
|---|---|
| `style::tests::webkit_scrollbar_cascade_only_for_scroll_containers` | a scroll container is still styled; an element outside its subtree is not; exactly 3 elements cascade (html, body, the container) |
| `style::tests::webkit_scrollbar_translation_does_not_leak_from_a_non_scrollable_element` | the removed behaviour: a rule on a non-scrollable element reaches neither it nor its scrollable descendant |
| `style::tests::webkit_scrollbar_bare_rule_still_reaches_the_page_through_body` | the compatibility path — a bare rule matches `<body>` and inherits down |
| `style::tests::webkit_scrollbar_cascade_skipped_for_overflow_hidden` | `overflow: hidden` scrolls but draws no bar, matching paint's condition |
| `style::tests::standard_scrollbar_width_still_inherits` | the standard properties are untouched by the narrowing |

The three pre-existing CC-CSS-1 tests (`webkit_scrollbar_width_maps_to_*`,
`webkit_scrollbar_thumb_track_map_to_*`, `webkit_scrollbar_thumb_without_track_*`)
now make their `<div>` a scroll container — they encoded the old "any element"
behaviour.

`cargo test -p lumen-layout`: 3295 passed, 2 failed (`ch`/`ex_approximated_as_half_em`,
the pre-existing BUG-339 flake, reproduced on unmodified main).

### Measurement

Interleaved A/B (S10-main, S11, ×3), dev-release, comparing mins:

| | main (S10) min | S11 min | main p50 | S11 p50 |
|---|---:|---:|---:|---:|
| `CC12_HOVER` | 22.7 / 21.0 / 24.1 | **16.4 / 16.9 / 17.4** | 26.1 / 27.4 / 37.6 | **19.9 / 19.6 / 21.0** |
| `CC12_KEY` | 9.6 / 9.0 / 9.2 | **8.2 / 8.5 / 8.3** | 11.7 / 11.6 / 12.1 | **10.7 / 11.0 / 10.9** |

≈25% off `CC12_HOVER`, ≈9% off `CC12_KEY` (the key cycle recomputes a dozen
nodes, so it had little scrollbar cost left to lose). Note the absolute numbers
are lower than S10's table on both sides — that run shared the machine with a
parallel build; only the interleaved comparison is meaningful.

Per stage after S11 (`LUMEN_PROFILE_TREE=1`, medians, quiet machine):

| Stage | `CC12_HOVER` | `CC12_KEY` |
|---|---:|---:|
| `precompute_counters` | 8.5 ms | 0.58 ms |
| `lay_out` | 4.4 ms | 3.9 ms |
| `build_box` | 2.9 ms | 2.5 ms |
| `graft_geometry` | 0.97 ms | 0.68 ms |
| whole pass | 17.6 ms | 8.6 ms |

Inside `compute_style` on `CC12_HOVER` (detail run, 828 nodes): `cs_apply`
2.76 ms, `cs_match` 2.21 ms, `cs_init` 0.60 ms, **`cs_post` 0.49 ms** (was
8.13 ms after S10 — the scrollbar translation is gone from the per-node path),
`cs_ua_hints` 0.32 ms, `cs_revert_prepass` 0.24 ms.

### Where this leaves CC-12

Still red, but the shape has changed: the cascade is no longer the dominant
stage. Budget 2 ms; `CC12_HOVER` ~16-21 ms and `CC12_KEY` ~8-11 ms — a gap of
~4-10× (was ~8-20× after S10).

The next items, in order:

* `lay_out` (4.4 / 3.9 ms) — now the single largest stage on both scenarios,
  and untouched since S8's `graft_geometry` fix. `graft_geometry` reuses
  geometry for structurally-identical subtrees; what `lay_out` still does for
  the rest has never been profiled below stage level.
* `build_box` (2.9 / 2.5 ms) — S4 built a reuse mechanism for it
  (`incremental_build_box`) and measured it as a net loss because
  `index_by_node` re-hashes the whole previous tree per call; that index, not
  the idea, is what needs fixing.
* `cs_apply` (2.8 ms) + `cs_match` (2.2 ms) — the irreducible-looking half of
  the cascade: declaration application and selector matching for the 828 nodes
  the hover flip actually dirties. Narrowing *which* nodes are dirty (S3-S7's
  line of work) would cut both at once.

## S12 — `LayoutBox::style` behind an `Arc`, copy-on-write

S11 handed S12 three candidates and put `lay_out` first. Profiling it — the
first time anything below stage level in `lay_out` had been measured — found
that a third of the stage is a single line, and that the same line's cost is
paid twice more elsewhere in the frame.

### What `lay_out` actually spends

Permanent `scope_detail` scopes now sit inside `lay_out_inner`
(`LUMEN_PROFILE_TREE=1 LUMEN_PROFILE_DETAIL=1`). On `CC12_HOVER`, before S12:

| Scope | Time | Calls | What it is |
|---|---:|---:|---|
| `lo_style_clone` | 2.33 ms | 1696 | `let s = b.style.clone()` — the snapshot `lay_out_inner` takes to dodge the borrow checker |
| `lo_wrap` | — | **0** | text wrapping: never runs on the incremental path |
| `lo_svg` | — | **0** | SVG layout: never runs either |
| `lo_chex` | 0.15 ms | 1696 | per-box `ch`/`ex` font metrics |
| `lo_translate` | 0.08 ms | 1425 | the clean-subtree fast path |

(The instrumented run inflates `lay_out` from 3.7 ms to 4.8 ms; ~0.22 µs per
scope. `lo_style_clone`'s real share is therefore ≈1.2 ms of 3.7 ms.)

Two things fall out of that table. First, the expensive content work — text
wrapping, SVG — is already fully grafted away; what is left is arithmetic over
boxes plus one deep copy per box. Second, `lo_style_clone` + `lo_translate`
give the counter no slice had until now: **1696 of 3121 boxes are re-laid-out
on every hover flip, 1425 are reused.**

`ComputedStyle` is 3.2 KB, 302 fields, ~30 of them heap-allocated. Per frame the
chrome pipeline copied it three times per box: `build_box` copying it out of the
cascade cache (`CounterMap::styles`, already `Arc<ComputedStyle>` since S9),
`lay_out_inner`'s snapshot, and the whole-tree `clone()` that persists `prev`
(`clone_tree`, 1.5 ms in the `[cc12-split]` line).

### The slice

`LayoutBox::style` is now `Arc<ComputedStyle>`. Reads are unchanged — `Arc`
derefs, so `b.style.field` and `&b.style` still compile everywhere — and all
three copies became refcount bumps. The passes that genuinely write a used value
back into a box's style take their copy through `Arc::make_mut`, on exactly the
boxes they touch: flex item stretch (main and cross axis), `font-size-adjust`,
container-query restyle, `propagate_canvas_background`, the `::first-line`
in-place restyle, and the table cell's temporary `width` swap. Semantics are
identical to before — `build_box` used to hand each box a private copy; now the
copy is taken at the moment of writing.

`apply_font_size_adjust` gained an explicit `FontSizeAdjust::None` test at the
call site. It already existed *inside* `apply_font_size_adjust_to_style`, but
reaching for `Arc::make_mut` to call it would have deep-copied the shared style
on every box in the document, for a property almost nothing sets — the one place
where the naive translation would have made things worse, not better.

`graft_geometry` compares `Arc::ptr_eq` before the 302-field `==`. When the
incremental cascade handed a node back unchanged, both trees hold the *same*
allocation. The structural compare stays behind it, so a node re-cascaded to an
equal value is still recognised as reusable.

Not done, and deliberately: `collect_box_styles`/`prev_styles` (the page-side
transition scheduler) still deep-copies every style per relayout. It owns its
snapshot to diff against the next frame, so sharing it needs `prev_styles` to
hold `Arc`s too — a page-pipeline follow-up, outside this slice's measured path.
`InlineSegment::style` and `InlineFrag::style` are likewise untouched.

### Gates: by identity, not by output

The S8/S9 lesson applies verbatim — a version that deep-copies instead of
sharing passes every differential test and is merely slow.

| Test | Asserts |
|---|---|
| `incremental::tests::built_boxes_share_the_cascade_cache_style_allocation` | `Arc::ptr_eq(box.style, counters.styles()[id])` after `build_box`, and again after `tree.clone()` |
| `incremental::tests::used_value_writeback_does_not_leak_into_the_cascade_cache` | the copy-on-write half: a stretched flex item either still shares the cache entry or took a private copy — the cache never sees the used value |

`cargo test -p lumen-layout`: 3297 passed, 2 failed (`ch`/`ex_approximated_as_half_em`,
the pre-existing BUG-339 flake). `lumen-chrome` 58, `lumen-paint` 973,
`lumen-shell` 1706 — all green. CPU snapshot references
(`cases::snapshot_cpu`, `--features cpu-render`) unchanged, pixel-identical.

`python graphic_tests/run.py --continue-on-fail`: 4 non-debtor failures, none
of them this slice. The first attempt was thrown away — TEST-00 hit the known
"magenta marker not found" focus race, which cascades "no crop offset" into all
148 remaining tests; re-run after TEST-00 passes.

| Test | Verdict |
|---|---|
| 10 (`min-max-width`) | 100.00 % here and 100.00 % in the 2026-07-26 main run — pre-existing, byte-for-byte the same diff region |
| 147 (`background-repeat: space`) | 27.4463 % vs 27.4544 % on main — pre-existing, BUG-330 |
| 71 (`@starting-style`) | *improved* 6.08 % → 1.76 %; flagged `FAIL`/`RATCHET` only because it now beats its own BUG-199 debtor baseline |
| 61 (`view-transitions`) | **not a Lumen difference at all.** `61-view-transitions-lumen-cropped.png` is byte-identical (`md5 50e4453…`) between the S12 and the main worktree; the two runs' *Edge reference* captures differ — Edge was grabbed mid-cross-fade in one of them, tinting the whole page purple, which is where the 99.5 % comes from. Headless `--screenshot` output of this page is also byte-identical between the two binaries |

Tests 61 and 71 are both animation-timing pages, and both reference and capture
are re-grabbed per run — treat a large swing on either as a capture-phase
question first, and settle it by diffing the two `-lumen-cropped.png` files
directly rather than by comparing diff percentages.

### Measurement

Interleaved A/B (main, S12, ×3), dev-release, `cc12_chrome_perf_gate`:

| | main min | S12 min | main p50 | S12 p50 |
|---|---:|---:|---:|---:|
| `CC12_HOVER` | 16.5 / 18.1 / 17.4 | **13.6 / 15.1 / 15.0** | 18.6 / 20.2 / 19.9 | **15.8 / 17.0 / 17.1** |
| `CC12_KEY` | 8.7 / 8.6 / 8.8 | **6.3 / 6.1 / 6.3** | 10.5 / 11.8 / 10.8 | **7.5 / 7.4 / 7.5** |

No overlap between the two groups on any of the four rows — ≈15 % off
`CC12_HOVER`, ≈28 % off `CC12_KEY`.

Per stage (`LUMEN_PROFILE_TREE=1`, medians, quiet machine):

| Stage | `CC12_HOVER` before → after | `CC12_KEY` before → after |
|---|---|---|
| `lay_out` | 3.70 → **2.44 ms** | 3.76 → **2.30 ms** |
| `build_box` | 2.82 → 2.45 ms | 2.53 → 2.19 ms |
| `clone_tree` (bookkeeping) | 1.48 → **0.73 ms** | 1.31 → **0.64 ms** |
| `graft_geometry` | 0.90 → 0.99 ms | 0.64 → 0.63 ms |
| `precompute_counters` | 7.40 → 7.88 ms | 0.55 → 0.55 ms |
| whole cycle (p50) | 18.49 → 16.81 ms | 10.43 → 7.96 ms |

`lo_style_clone` (renamed `lo_style_ref`) went 2.33 ms → **0.11 ms** for the same
1696 calls, which is the change doing exactly and only what it was aimed at.
`precompute_counters` and `graft_geometry` are flat within noise (their mins are
6.02 → 6.10 and 0.75 → 0.76); nothing regressed.

### Where this leaves CC-12

Still red. Budget 2 ms; `CC12_HOVER` ~13.6-17 ms and `CC12_KEY` ~6.1-7.5 ms — a
gap of ~3-8× (was ~4-10× after S11). The per-frame cost is now spread thin
rather than concentrated: no single stage is above 2.5 ms on either scenario.

The next lever is the one the new counters exposed, and it is bigger than
anything left in a single stage: **1696 of 3121 boxes are rebuilt and
re-laid-out on a hover flip that changes one subtree.** Both `build_box`
(2.2-2.45 ms) and `lay_out` (2.3-2.44 ms) are almost linear in that count, and
`cs_apply`/`cs_match` scale with the 828 elements the same flip re-cascades. So:

* Find out why the dirty set is that wide. `graft_geometry` reuses 1425 boxes;
  `mark_subtree_dirty` + the cascade's dirty-root-set decide the rest. Whether
  the remaining 1696 genuinely changed, or are collateral from a conservative
  root-set (S3-S7's line) or from `kind_layout_eq` rejecting a match, has not
  been measured — gate whatever comes out of it on the reuse *count*, which now
  exists (`lo_translate` vs `lo_style_ref`).
* S4's `incremental_build_box` is still off, still for the right reason
  (`index_by_node` re-hashes the whole previous tree per call). With `Arc`
  styles the index is cheaper to build than it was, so the trade-off is worth
  re-measuring — but only after the dirty-set question above, which may make the
  mechanism unnecessary.

## S13 — layout's own output was reading as a style change

S12 left one question: **1696 of 3121 boxes re-laid-out on a hover flip that
changes one subtree** — genuinely changed, or collateral? The answer is neither
of the two causes S12 guessed at (conservative root-set, `kind_layout_eq`
rejects). None of those boxes had changed at all.

### The census

`graft_geometry` now tallies why it accepts or refuses each box
(`incremental::GraftStats`, `take_graft_stats`), with an opt-in attribution pass
behind `set_graft_diagnostics`. On the CC-12 chrome document, before this slice:

| | `CC12_HOVER` | `SIBLING_HOVER` |
|---|---:|---:|
| visited | 318 | 318 |
| reused clean | 196 | 196 |
| rejected — node/kind identity | 0 | 0 |
| rejected — style | **81** | **81** |
| …of which differ *only* in used-value fields | **81** | **81** |
| rejected — child count | 0 | 0 |
| rejected — descendant changed | 41 | 41 |

Every single style reject was a box whose fresh style differed from its
predecessor's *only* in `width`/`height`/`box_sizing` — and a graft reject
propagates upward, which is where the other 41 came from. 122 boxes re-laid-out
per interaction, zero of them changed.

### Cause

`prev` is a **laid-out** tree, and layout writes used values back into the very
styles the graft compares. `lay_out_flex` overwrites every flex item's
`width`/`height`/`box_sizing` with the resolved used size (`box_tree.rs`, both
the column and the row arm); the post-layout passes rewrite more. The freshly
built tree carries none of that yet, so `new.style == prev.style` answers a
question nobody asked: not "did the author's style change?" but "has this box
been through layout since?". S8 found and fixed one instance of this at the root
box (used viewport `height`); it is a general property of the field, not a
special case of the root.

The chrome document is built out of flexbox, so this was every interaction.

### The slice

`graft_geometry_with_cascade` takes the `CounterMap::styles` map of the pass that
produced `prev` — the *unpolluted* cascade result — and compares against that:

1. `Arc::ptr_eq(new.style, prev.style)` — unchanged, the cheapest case.
2. `Arc::ptr_eq(new.style, prev_cascade[node])` — the incremental cascade hands
   the same allocation back for reused nodes (S9/S12), so a narrow restyle
   settles here for free.
3. `*new.style == *prev_cascade[node]` — the wide-restyle case. CC-12's own
   `SIDEBAR`/`None` toggle re-cascades most of the document to the values it
   already had; those must still read as unchanged. (Without this clause the
   toggle scenario kept all 81 rejects while `SIBLING_HOVER` dropped to 21 —
   measured, not assumed.)
4. `new.style == prev.style` — the pre-existing fallback.
5. For a box whose node has **no** cascade entry at all: styles equal once
   `width`/`height`/`box_sizing` are discounted. This is the anonymous-box
   class — the wrapper a flex container generates around inline content, keyed
   by a text node the cascade never visits. Instrumenting the residue showed all
   21 remaining rejects were exactly this: `kind = Block`, one child, `width:
   None` freshly derived against a fractional used px width in `prev`. No author
   rule can put a width on such a box, so a difference confined to those fields
   is layout output by construction. The probe copies a style, so it runs last
   and only for this class.

A reused subtree keeps its **own** freshly-cascaded style — only `rect` and
`kind` come from `prev`. `kind` must be copied (it holds layout output paint
reads back, e.g. `InlineRun`'s laid-out `lines`); the used values in the *style*
are read by nothing outside the layout pass that wrote them, and adopting them
would pin the pollution into the live tree permanently and mis-size the box if it
later went dirty.

### Gates: by count

Same reasoning as S8's and S10's: geometry is identical either way, so only
wall-clock would show a regression here, and machine noise hides it.

* `bug341_s13_hover_flip_reuses_boxes_the_layout_pass_only_wrote_used_values_into`
  (lumen-shell, runs by default) — on the real chrome document,
  `reject_style_used_value_only == 0` and `reused_clean == visited`. The first
  assert is the load-bearing one: it fails the moment a layout pass writes a used
  value the graft cannot account for, whatever the stylesheet contains.
* `graft_reuses_a_box_whose_prev_style_only_carries_used_values` — the mechanism
  in isolation.
* `graft_still_rejects_a_box_whose_cascade_style_changed` — the counterweight,
  without which the fix would degrade into "styles never matter" and no
  geometry-comparing test would notice.
* `graft_stats_partition_the_visited_set` — the counters are the only instrument
  the gates have; a double-count would make every number above wrong.

### Measurement

Census after the slice — **318 of 318 boxes reused, on both scenarios**, every
reject bucket zero.

Interleaved A/B (main, S13, ×3), dev-release, `cc12_chrome_perf_gate`:

| | main min | S13 min | main p50 | S13 p50 |
|---|---:|---:|---:|---:|
| `CC12_HOVER` | 16.24 / 15.89 / 16.04 | **12.49 / 12.59 / 11.72** | 18.14 / 18.87 / 18.13 | **15.71 / 15.35 / 15.48** |
| `CC12_KEY` | 6.19 / 6.07 / 6.14 | **3.93 / 3.89 / 3.85** | 9.33 / 8.10 / 8.89 | **4.89 / 4.96 / 4.76** |

No overlap on any row — ≈23 % off `CC12_HOVER`, ≈37 % off `CC12_KEY` (≈16 % and
≈44 % by p50).

Per stage (`LUMEN_PROFILE_TREE=1`):

| Stage | `CC12_HOVER` S12 → S13 | `CC12_KEY` S12 → S13 |
|---|---|---|
| `lay_out` | 2.44 → **0.00 ms** | 2.30 → **0.08 ms** |
| `build_box` | 2.45 → 2.2-2.4 ms | 2.19 → 1.7-2.2 ms |
| `graft_geometry` | 0.99 → 1.0-1.2 ms | 0.63 → 0.5-0.6 ms |
| `precompute_counters` | 7.88 → 9-12 ms | 0.55 → 0.5-0.8 ms |

`lay_out` is the stage this slice was aimed at and it is now free: with nothing
dirty, every box takes the O(1) translate path. `graft_geometry` pays a little
for the extra lookups; that is the trade and it is an order of magnitude smaller
than what it buys.

### Where this leaves CC-12

Still red. Budget 2 ms p95; `CC12_HOVER` p95 ≈17.5-19.3 ms and `CC12_KEY` p95
≈7.4-8.0 ms — a gap of ≈4-9× (was ≈5-12× after S12).

The shape has changed again, and the two remaining costs are now clearly
separated:

* **`precompute_counters` on `CC12_HOVER`: 9-12 ms, ~75 % of the cycle.** This
  is the conservative root-set S3 documented and every slice since has worked
  around rather than at: the `SIDEBAR`/`None` toggle transitions from "nothing
  hovered", which `restyle_root_set_for_state_change` correctly treats as "every
  ancestor of the target flipped `:hover`" and expands to most of the document.
  The census now proves the expansion is pure waste on this fixture — the
  re-cascade produces byte-identical styles for all 318 boxes. Narrowing it (or
  short-circuiting a re-cascade whose result is unchanged) is the biggest single
  number left anywhere in this bug.
* **`build_box` is now the dominant stage on `CC12_KEY` (1.7-2.2 ms of ~4 ms).**
  The whole tree is rebuilt every cycle only to be grafted back onto the previous
  geometry. This is exactly what S4's `incremental_build_box` was written for and
  it is still switched off; with `Arc` styles and a now-*empty* dirty set, its
  `index_by_node` trade-off is worth re-measuring — the mechanism may finally pay.

## S14 — the flipped ancestor chain nobody's selectors could react to

The number S13 pointed at: `precompute_counters` was 9-12 ms of `CC12_HOVER`'s
~13 ms cycle, all of it the conservative interactive-state root-set.

### Why the root-set covered the document

`:hover` matches the element under the pointer *and every ancestor of it* (CSS
Selectors L4 §4.3), so a "nothing was hovered → `#sidebar` is hovered"
transition really does flip the `:hover` boolean on `#sidebar`, `<body>`,
`<html>` and the document node. `restyle_root_set_for_state_change` put all of
them in the root-set, and a dirty root at the document node forces
`counters::walk`'s `force` flag on for the whole tree — a full re-cascade of
every node, every other cycle. S13's census had already proved the result was
byte-identical for all 318 boxes.

The flip is real. What was missing is that a flip is only *observable* through a
compound that depends on dynamic state **and matches the flipped node**:
`chrome.html`'s hover rules are `button:hover`, `.tab-row:hover`,
`.hbar-tab:hover` and ~30 more of the same shape, and not one of them can match
`<body>`, `<html>` or `aside#sidebar`.

### The slice

`restyle_state_needs_fanout(doc, sheet) -> bool` becomes
`restyle_state_index(doc, sheet) -> StateRestyleIndex<'_>`, built from the same
single scan of the sheet S7 already ran, now also collecting every compound that
depends on dynamic state (`collect_state_compounds`) and a `conservative` flag.
`restyle_root_set_for_state_change` takes the index and drops any flipped node
that `state_flip_can_matter` rejects — no state-dependent compound can even
structurally match it (`compound_could_match_after_state_flip`: every part must
match, with the dynamic-state pseudo-classes and any pseudo-element treated as
"possible", since their value is exactly what's in question).

Soundness rests on one property: every nested-selector form the engine looks
through for dynamic state (`:not()`, `:is()`, `:where()`, `:host()`,
`:nth-child(… of …)`) evaluates its inner selector against the *same* node as
the compound carrying it. `:has()` is the exception — it binds the state to a
different node than the subject — so a sheet with a dynamic `:has()`, or a
document with any shadow root (whose sheets are not scanned, same carve-out S7
made), sets `conservative` and keeps the pre-S14 behaviour exactly.

Over-approximation is one-directional: a compound whose state hides inside a
nested list (`:is(.tab:hover, .x)`) has that part treated as matching anything,
so it keeps the whole chain. That costs narrowing, never correctness.

### Gates

`bug341_s14_hover_flip_no_rule_can_react_to_recascades_nothing` (lumen-shell,
runs by default) asserts in this order: the two *full* cascades — nothing
hovered vs `#sidebar` hovered — are equal (ground truth, no incremental
machinery involved); the narrowed root-set for that transition is **empty**
(the count gate, S8's lesson: a mechanism that narrows nothing still reproduces
the full cascade exactly, just slowly); and the incremental cascade run under
that empty root-set equals the full post-transition cascade bit-for-bit.

`mutation_incremental_restyle_hover_entering_from_nothing_matches_full`
(lumen-layout) is the under-approximation gate: hover enters from nothing onto a
deep `.icon`, where two flipped ancestors (`.card`, `.item`) *do* carry hover
rules and one of them (`.item:hover .icon`) restyles a descendant — dropping
either would leave a stale style and a wrong height.

Seven unit tests in `style::state_fanout_tests` cover the shapes: chain fully
dropped, only the matching ancestor kept, `::before` on a hover compound,
dynamic `:has()`, shadow root, state nested in `:is()`, and `:focus-within` on a
matching ancestor (chrome's `.omnibox:focus-within` — the one dynamic-state
pseudo that legitimately matches ancestors).

### Measurement

Interleaved A/B (main, S14, ×3), dev-release, `cc12_chrome_perf_gate`:

| | main min | S14 min |
|---|---:|---:|
| `CC12_HOVER` | 14.09 / 14.46 / 14.95 | **3.63 / 3.70 / 4.03** |
| `CC12_KEY` | 3.86 / 3.88 / 3.82 | 3.76 / 4.01 / 3.96 |

No overlap on `CC12_HOVER` — ≈3.7× faster (−74 %). `CC12_KEY` is unchanged, as
expected: its hover is `None` on every cycle, so its state root-set was already
empty and this slice cannot touch it.

Per stage (`LUMEN_PROFILE_TREE=1`, medians over 70 cycles):

| Stage | `CC12_HOVER` S13 → S14 | `CC12_KEY` S13 → S14 |
|---|---|---|
| `precompute_counters` | 9-12 → **0.41 ms** | 0.5-0.8 → 0.56 ms |
| `build_box` | 2.2-2.4 → 2.45 ms | 1.7-2.2 → 2.23 ms |
| `graft_geometry` | 1.0-1.2 → 0.67 ms | 0.5-0.6 → 0.61 ms |
| `lay_out` | 0.00 → 0.00 ms | 0.08 → 0.11 ms |

The bench's own bookkeeping moved too: `clone_styles` (the per-cycle
`CounterMap::styles().clone()`) fell 0.86 → 0.02 ms, because the old map's
`Arc<ComputedStyle>`s are now shared with the new one instead of being the last
reference to 828 freshly-built styles that had to be dropped.

### A pre-existing hole this slice surfaced

Writing the under-approximation gate turned up **BUG-355**: when an *ancestor's*
geometry-affecting style changes (`.card:hover { padding: 9px }`), its
clean-grafted descendants keep their previous used width — `graft_geometry`
compares each box's own style only, and `lay_out` translates a clean subtree in
O(1) without resizing it. Reproduced with every node forced into `dirty_roots`,
so it is independent of the root-set and predates S14; filed separately, and the
S14 test carries the repro in a comment.

### Where this leaves CC-12

Still red. Budget 2 ms p95; by min `CC12_HOVER` is now 3.6-4.0 ms and
`CC12_KEY` 3.8-4.0 ms — a gap of ≈2× on min (p95 on this loaded machine runs
8-10 ms, ≈4-5×; the two scenarios have converged and neither is dominated by the
cascade any more).

**`build_box` is now the largest stage on both scenarios** (2.2-2.5 ms of a
~3.7-3.9 ms cycle): the whole box tree is rebuilt every cycle only to be grafted
straight back onto the previous geometry. That is precisely what S4's
`incremental_build_box` was written for, and it is still switched off — S4's
honest measurement rejected it because `index_by_node` (a full walk + hash of
the previous tree per call) cost more than the ~8 % of `build_box` it saved.
Both halves of that trade have changed since: `build_box` is now ~60 % of the
cycle rather than 8 %, styles are `Arc`s, and the dirty set is empty on both
fixtures, so `clean_subtrees` should license reusing nearly the whole tree.
Re-measure it before designing anything new.

## S15 — the box tree was rebuilt every cycle only to be grafted back

**Branch `p1-bug341-s15`.** Took the option S14's queue left: re-measure S4's
`incremental_build_box` before designing anything new. It won, decisively, and
the reason it had lost in S4 turned out to be only half the story.

### What was wrong

After S13/S14 a `#sidebar`/`None` hover flip on the chrome document re-cascades
**nothing** and `graft_geometry` reuses **all 318 boxes**. Yet `build_box` still
constructed all of them from scratch every cycle — 2.2-2.5 ms of a ~3.7 ms
cycle, the largest remaining stage — purely so the graft could immediately throw
the fresh geometry away and copy the previous one back in.

S4 had already written the mechanism for exactly this (`build_box_or_reuse` +
`CounterMap::clean_subtrees`: clone a whole `LayoutBox` subtree from `prev`
instead of rebuilding it) and left it switched off, because S4's own honest
measurement showed a net loss — `index_by_node`'s whole-prev-tree hash cost more
than the ~8 % of `build_box` it saved. Both halves of that trade had moved:
`build_box` is now ~60 % of the incremental cycle, `ComputedStyle` and
`LayoutBox::style` are behind `Arc` (S9/S12), and the dirty set is empty.

### Two defects, each of which alone drove reuse to zero

1. **The reuse gate was a thread-local the workers never saw.**
   `build_box_or_reuse` consulted `INCREMENTAL_BOX_BUILD`, but `build_box` fans
   flex/grid containers with 8 or more children out over rayon workers
   (ADR-016 M4.1), and a rayon worker's thread-locals start at their defaults —
   the very trap `StyleEnvSnapshot` exists to work around for style state.
   Chrome is built out of exactly such containers, so reuse was disabled for
   almost every node that mattered. The gate is now `prev_index.is_some()`, a
   parameter threaded down by reference; the flag is read once, in
   `incremental_build_box`, which is what decides whether an index is built.

2. **The counters were blind to the same threads.** `BOX_BUILD_COUNT` /
   `BOX_BUILD_REUSE_COUNT` were `#[cfg(test)]` thread-locals, so everything
   built on a worker was invisible: the first instrumented run of the real
   chrome document reported "7 boxes built" for a tree of 318. Replaced by a
   public `box_tree::BoxBuildStats` / `take_box_build_stats()`, with the
   parallel branch draining each worker's tally into the thread running the
   parent container. The drain stays exact when rayon work-steals a closure onto
   the calling thread — whatever it takes from that thread comes straight back
   in the fold.

Note the shape of defect 2: it is the S10 profiler bug again, in a different
costume. Instrumentation that is thread-local while the code it measures is not
does not report a small error — it reports a number from a different program.

### Wiring

`layout_mutation_incremental_restyle` now calls `incremental_build_box` when
`set_incremental_box_build` is on; both production call sites
(`relayout_chrome_host` and the page-side `try_relayout_raf_incremental`) turn
it on next to their existing `set_incremental_restyle(true)`. No new
correctness precondition: the reuse is licensed by `CounterMap::clean_subtrees`,
which is only populated when `RestyleDelta::dom_content_stable` is `true` —
exactly the contract those call sites already establish.

### Gates — by count

Output cannot distinguish "cloned the subtree" from "rebuilt an identical
subtree", so every gate here asserts the tally (S8's lesson, third slice
running):

- `bug341_s15_hover_flip_reuses_the_box_tree_instead_of_rebuilding_it`
  (lumen-shell, runs by default) — on the real `chrome.html`: the first cycle
  builds 100+ boxes, every later cycle must build fewer than 10 and reuse at
  least one subtree. This is the test that would have caught defect 1.
- `bug341_s15_hover_flip_reuses_the_clean_sibling_subtrees` (lumen-layout) — a
  10-sibling flex container, deliberately over `RAYON_MIN_FLEX_CHILDREN`, so the
  gate runs through the parallel path.
- `bug341_s15_reused_boxes_match_a_full_rebuild` — the usual `incr == full`
  differential half, on the same fixture.
- `bug341_s15_dom_content_change_disables_box_reuse` — `dom_content_stable:
  false` must reuse nothing. This is the entire correctness contract, and
  nothing else in the mechanism checks it.

### Measurement (interleaved A/B ×3, by min)

| | main | S15 |
|---|---|---|
| `CC12_HOVER` | 3.94 / 4.19 / 4.06 ms | **1.39 / 1.45 / 1.47 ms** (≈2.8×) |
| `CC12_KEY` | 3.89 / 3.67 / 4.02 ms | 3.95 / 4.03 / 3.93 ms (flat) |

The two groups do not overlap on any row of `CC12_HOVER`. Stage split
(`LUMEN_PROFILE_TREE=1`, min/p50 over 69 cycles):

| Stage | HOVER | KEY |
|---|---|---|
| `precompute_counters` | 0.27 / 0.42 ms | 0.43 / 0.56 ms |
| `build_box` | **0.25 / 0.38 ms** (was 2.2-2.5) | **1.88 / 2.31 ms** |
| `graft_geometry` | 0.19 / 0.25 ms | 0.50 / 0.66 ms |
| `lay_out` | 0.00 ms | 0.08 / 0.10 ms |
| `post_layout_passes` | 0.07 / 0.10 ms | 0.08 / 0.10 ms |
| **total** | **0.85 / 1.15 ms** | **3.13 / 3.82 ms** |

`CC12_KEY` is flat *by design*: `bind_model` rewrites the omnibox text every
keystroke, so `touched` is non-empty, `dom_content_stable` is `false`,
`clean_subtrees` stays empty and the mechanism is inert. Display-list
neutrality checked with `graphic_tests/dump_golden.py` (12/12 identical).

### Where this leaves CC-12

Still red, but the two scenarios have separated again. `CC12_HOVER` is **under
the 2 ms budget by min for the first time** (1.4-1.5 ms); its p95 on this loaded
machine is 5.2-6.0 ms, ≈3×. `CC12_KEY` is unchanged at 3.9-4.0 ms by min, ≈2×,
p95 8.3 ms.

**S16 is `CC12_KEY`, and the blocker is named:** `dom_content_stable` is a single
document-wide boolean. One changed text node in the omnibox disables box reuse
for all 318 boxes, including the ~300 that no mutation went anywhere near. The
generalisation is a per-node content-dirty set — a subtree is content-clean when
no touched node lies inside it — but it needs the chrome tracker to be
*complete* for content, which it currently is not: `record_touched` is called
from `set_attr`/`remove_attr`/`remove_children_with_class`/`reconcile_row_list`
(all selector-relevant), while `set_text`/`set_text_in_place`/`append_text`
report nothing, since a text change cannot affect selector matching. Making the
tracker content-complete is the load-bearing, and the risky, part: an
uninstrumented mutation path yields a stale box, i.e. visible corruption, not a
slow frame. Audit `crates/chrome/src/model.rs` for every direct
`append_child`/`detach`/`create_*` call, not just the primitives.

## S16 — one boolean was answering a per-node question

**Branch `p1-bug341-s16`.** Took the queue's own instruction: replace
`RestyleDelta::dom_content_stable` with a per-node content-dirty set.

### What was wrong

`build_box` reads three things a `ComputedStyle` comparison cannot see —
attribute values, text-node data, and child lists. S4 covered that gap with one
document-wide boolean: reuse a box subtree only if *nothing anywhere* had
content-changed this cycle. Correct, and hopeless for `CC12_KEY`, where one
typed omnibox character sets `dom_content_stable = false` and rebuilds all 318
boxes — including the ~300 the mutation came nowhere near. That was 1.88-2.31 ms
of the 3.13-3.82 ms cycle after S15.

### The mechanism

`ContentDirty` (in `counters.rs`) replaces the boolean with three states:

| variant | meaning | who says it |
|---|---|---|
| `Untracked` | no content record — reuse nothing | page-side JS (`DomTouched` has an `unattributed` escape hatch and never reports text writes) |
| `Nothing` | complete record, nothing changed | a pure `:hover`/`:focus`/`:active` cycle |
| `Nodes(&set)` | complete record, exactly these changed | `bind_model_tracked` |

`counters::walk` folds it into the same bottom-up `subtree_clean` it already
computed: a node is clean when its style was reused, its own id is not in the
set, and every descendant is clean. A text node — which has no style, so the
cascade has nothing to compare — now returns `false` when named, which is what
drops its parent element (whose box embeds the text) out of `clean_subtrees`.

`bind_model_tracked` returns `ChromeMutations { selector, content }` instead of
one set. `selector` is S6's, unchanged, and still drives the cascade root-set;
`content` additionally covers text writes and child-list changes, which are
invisible to selectors and were therefore deliberately unreported before.

### Completeness is structural, not editorial

`Nodes` is a promise: a mutation that goes unreported yields a **stale box on
screen**, not a slow frame. So it is not maintained by having read the file
once. Every raw `Document` mutator in `crates/chrome/src/model.rs` is wrapped
(`attach_child` / `insert_child_before` / `detach_node`, alongside the existing
`set_attr` / `remove_attr` / `set_text`), and
`every_dom_mutation_in_model_rs_goes_through_a_tracked_primitive` scans the
file's own source via `include_str!`, rejecting any `doc.append_child(` /
`doc.insert_before(` / `doc.detach(` / `doc.get_mut(` not on a line marked
`S16-tracked-primitive`. A future `bind_*` helper that mutates directly fails
the build, which is the only form of this invariant that survives the next
person.

### Tracking content exposed what it was hiding

The first run of "rebind a bit-identical model" reported **twelve**
content-dirty nodes. `bind_cert`'s title/value/fingerprint cells, `#findCount`,
`#statTrackers` and the bookmarks/history titles all called `set_text`, which
detached and recreated its text node unconditionally, changed or not — the
`set_text_in_place` variant that compares first existed but had only been
applied to the tab/workspace rows. Nothing noticed while only selector-relevance
was tracked (none of those rewrites is selector-relevant), but the moment
content is tracked, twelve unconditional rewrites would have cancelled S15's
whole-document reuse on **every hover frame** — S16 would have made `CC12_HOVER`
worse while improving `CC12_KEY`.

`set_text` absorbed `set_text_in_place`'s body: one text setter, compare then
write in place, a genuine no-op when unchanged. `bind_omnibox`'s warning banner
got the same treatment (it rebuilt its `⚠ <span>` every bind, which would have
cost the whole document for as long as a warning was displayed). This is the
`bind_palette` gotcha of S6 in a new place, and the standing rule it implies:
**any chrome binding must compare before it writes.**

### Gates

- `every_dom_mutation_in_model_rs_goes_through_a_tracked_primitive`
  (lumen-chrome) — the completeness invariant, enforced against source.
- `bind_model_tracked_reports_no_content_for_an_unchanged_model` (lumen-chrome)
  — the twelve-rewrites regression; this is what protects `CC12_HOVER`.
- `bind_model_tracked_reports_a_renamed_tab_as_content_only` (lumen-chrome) — a
  text change must land in `content` and *not* in `selector`.
- `box_build_text_mutation_reuses_everything_but_the_mutated_chain`
  (lumen-layout) — **correctness first**: every `InlineSegment`'s text must
  match a full rebuild (verified to fail with exactly the stale-text symptom
  when the mutation is under-reported), **then** the count: the untouched
  sibling subtree is cloned and the mutated chain is not.
- `bug341_s16_keystroke_rebuilds_only_the_omnibox_chain` (lumen-shell, runs by
  default) — on the real chrome document, a keystroke cycle must build under a
  quarter of the full tree.

### Measurement

Box-build census on the shell fixture, `CC12_KEY` cycle: **240 → 38 boxes
built** (8 subtrees cloned). `CC12_HOVER` unchanged at 3 built / 1 reused —
which is the point of the `..._no_content_for_an_unchanged_model` gate.

Interleaved A/B ×3 (main, S16, main, S16, …), by min — the machine's noise is
still wider than the effect on `CC12_HOVER`, so p50 is not usable here:

| | main | S16 |
|---|---|---|
| `CC12_KEY` min | 4.24 / 3.91 / 3.80 ms | **2.73 / 2.83 / 2.84 ms** (≈1.4×) |
| `CC12_KEY` p95 | 9.19 / 10.88 / 8.66 ms | **6.76 / 6.90 / 6.94 ms** |
| `CC12_HOVER` min | 1.37 / 1.37 / 1.43 ms | 1.36 / 1.67 / 1.33 ms (flat) |

Both `CC12_KEY` rows separate cleanly — no overlap between the groups on any
round. `CC12_HOVER` overlaps, i.e. is unchanged, which is the intended result:
S15 already reused everything there, and S16's job on that scenario was to not
regress it (see the twelve unconditional rewrites above, which would have).

Stage split, `LUMEN_PROFILE_TREE=1`, min / p50 over 60 `CC12_KEY` cycles:

| Stage | S15 | S16 |
|---|---|---|
| `precompute_counters` | 0.43 / 0.56 ms | 0.47 / 0.59 ms |
| `build_box` | 1.88 / 2.31 ms | **0.82 / 1.30 ms** |
| `graft_geometry` | 0.50 / 0.66 ms | 0.32 / 0.44 ms |
| `lay_out` | 0.08 / 0.10 ms | 0.07 / 0.09 ms |
| **total** | 3.13 / 3.82 ms | **2.05 / 2.63 ms** |

### Where this leaves CC-12

Still red, but much closer, and the two scenarios have converged in character.
`CC12_HOVER` is in budget by min (1.3-1.7 ms) with p95 ≈5.2-6.1 ms (≈3×).
`CC12_KEY` is now ≈1.4× by min (2.7-2.8 ms vs the 2 ms budget) and ≈3.5× by p95.

`build_box` is still the largest stage on `CC12_KEY`, and the census says why: a
one-character omnibox change rebuilds **38** boxes, not the 3 that a hover
frame does. The omnibox `value` write is a genuine attribute change, so
`restyle_root_set_for_node_change` puts `#omniInput` and a conservative
neighbourhood into `dirty_roots`; every re-cascaded node then re-enters
`must_recompute` and loses its box. **S17 is the S14 argument applied to DOM
mutations**: S14 asked "which selectors could possibly react to this `:hover`
flip" and dropped the rest of the root-set; nobody has yet asked which
selectors could react to a `[value]` write. Get the census first (which of the
38 are re-cascaded because a rule really keys on the mutated attribute, and
which are collateral), because the last two slices both found the planned
premise was not where the time was.

## S17 — the widen-to-parent nobody's selectors could react to

**Branch `p1-bug341-s17`.** The S14 argument, applied to DOM mutations instead
of interactive state.

### The census, first

The queue's own instruction for this slice was "get the census before touching
anything", because S15 and S16 both found the planned premise was not where the
time was. `bug341_s17_keystroke_restyle_census` (new, `#[ignore]`d diagnostic in
`lumen-shell`) prints, for a `CC12_KEY` cycle: the mutation
`bind_model_tracked` reported, the restyle root-set derived from it, how many
nodes that root-set re-cascaded, and how many of those ended up with a
*different* `ComputedStyle`.

```
selector-touched: input#omniInput attrs={"value"} structural=false
dirty root:       div.omnibox (subtree 12 elements)
cascade_recomputed=12  really_changed=0  recascaded_identical=12  boxes_built=38
```

One typed character writes exactly one attribute on exactly one node — and
re-cascaded twelve elements, **all twelve producing a byte-identical
`ComputedStyle`**, each of them losing its box on the way (`must_recompute` ⇒ not
in `clean_subtrees` ⇒ `build_box_or_reuse` rebuilds it). 100 % collateral.

### Why the root-set covered the omnibox

`restyle_root_set_for_node_change` mapped every changed node to **its parent**,
and re-cascaded that parent's whole subtree. The reason is real: class/attribute
selectors don't match ancestors, so the node's own subtree covers the node and
every descendant rule rooted at it (`node X`) — but a *sibling* combinator
(`node + X`, `node ~ X`) reaches outside that subtree, and the parent's subtree
is the smallest thing that contains both. S3 wrote that as an unconditional
widen and every slice since inherited it.

S7 already asked this exact question for interactive state ("does any selector
need the wider fanout?") and S14 sharpened it to per-node ("can any selector even
match *this* node?"). Nobody had asked it about attribute writes. For a `value`
write on `#omniInput` the answer is no twice over: `chrome.html` contains **no
sibling combinator at all**, and even if it did, its left-hand compound would
have to be able to match `#omniInput`.

### The slice

`restyle_root_set_for_node_change` now takes what changed, not just where:

```rust
pub enum NodeChange<'a> { Attr(&'a str), Unattributed }
pub fn restyle_root_set_for_node_change<'a>(
    doc: &Document,
    changes: impl IntoIterator<Item = (NodeId, NodeChange<'a>)>,
    index: &NodeRestyleIndex<'_>,
) -> HashSet<NodeId>
```

`NodeRestyleIndex` (`restyle_node_index(doc, sheet)`, built from one scan of the
sheet, exactly like S14's `StateRestyleIndex`) collects every compound from which
a sibling combinator is reachable — `X + Y`, `X ~ Y`, `X + Y Z` — and answers, per
node and per attribute name, whether any of them *could* match that node once the
parts keyed on that attribute are treated as unknown. If none can, the write
cannot be observed outside the node's own subtree and the root-set is the node
itself.

Reporting the attribute name is what makes the check per-node rather than
per-sheet. `bind_model_tracked` now returns
`selector: HashMap<NodeId, SelectorTouch { attrs: BTreeSet<String>, structural: bool }>`
instead of a bare node set: `set_attr`/`remove_attr` record the name they wrote,
`reconcile_row_list`/`remove_children_with_class` record `structural`. A
sheet-wide "does any selector use `+`/`~`" test would have given the same answer
on today's `chrome.html` and lost it entirely the day someone adds one unrelated
`.a + .b` rule; with the name in hand, that rule only widens the nodes `.a` can
actually match.

Three shapes keep the pre-S17 behaviour verbatim:

* `NodeChange::Unattributed` — a child-list change (no attribute name describes
  it; `:nth-child`/`:empty`/sibling combinators all react), and every page-side
  JS mutation, since `DomTouched` records node ids without names.
* `:has()` anywhere in the sheet. That is BUG-348's direction — an ancestor's
  match bound to a descendant's state — which no subtree-shaped root-set
  expresses. The old widen-to-parent at least re-evaluated the immediate
  parent's `:has()`; narrowing would take even that away, so such sheets are
  `conservative` and S17 neither fixes nor worsens BUG-348.
* `:nth-child(… of S)` / `:nth-last-child(… of S)`, and any shadow root — the
  first is sibling reach with no combinator to see it, the second is S7's
  unscanned-shadow-sheet carve-out.

Two more document-scope reads are documented on the function as pre-existing
gaps of the same family, not introduced here: `:indeterminate` on a radio group
and `:default` on a form's submit button both read *other* elements'
`name`/`checked`/`type` across the whole form, which the pre-S17
widen-to-parent did not express either.

### What S17 deliberately did **not** do

The other half of the S14 argument — dropping the changed node from the root-set
entirely when no selector keys on the mutated attribute at all — was measured
and rejected. On this fixture it saves exactly one `compute_style` call:
`#omniInput` has no element children, and its box is rebuilt regardless because
the `value` write makes it content-dirty. What it would cost is a complete table
of "every attribute name the cascade reads" — `style.rs` alone has ~40
`get_attr("…")` sites spanning presentational hints, `attr()` in declarations,
and a dozen pseudo-classes — where a single omission is a *silently wrong style*,
not a slow frame. Not worth one node.

### Gates

* `bug341_s17_keystroke_recascades_the_input_not_the_omnibox` (lumen-shell, runs
  by default) — ground truth first (two **full** cascades, before and after the
  keystroke, are equal), then the count (root-set is `{#omniInput}`,
  `CascadeStats::recomputed == 1`), then equality of the incremental cascade with
  the full one. The count is the only half that can fail silently: a mechanism
  that narrows nothing still reproduces the full cascade, just slowly.
* `mutation_incremental_restyle_narrowed_attr_change_matches_full` (lumen-layout)
  — the under-approximation gate. The sheet restyles the mutated node *and a
  descendant of it* (`[data-x="2"] span`), and the assert compares the whole
  cascade, not geometry (a colour change moves no boxes). Verified to fail with
  a stale-descendant-style diff when the root-set is emptied by hand.
* `style::node_fanout_tests` — 12 unit tests over the shapes: no sibling
  combinator, a sibling rule keyed on the changed attribute, a sibling rule whose
  left compound cannot match the node (the case a sheet-wide check gets wrong), a
  sibling combinator *before* the matching compound, `@media` blocks, `:has()`,
  `:nth-child(of)`, plain `:nth-child`, and a pseudo-class in the sibling-source
  compound.
* `CascadeStats` / `take_cascade_stats()` (`counters.rs`) replaced the
  `#[cfg(test)]` `RECOMPUTE_COUNT` thread-local and is now public, so gates
  outside `lumen-layout` — where the real chrome document lives — can assert on
  it. `walk` is a plain recursion on the calling thread, unlike `build_box`, so
  a thread-local tally really does see the whole document (S15's trap does not
  apply here).

### Measurement

Counters on the shell fixture, `CC12_KEY` cycle:

| | S16 | S17 |
|---|---|---|
| elements re-cascaded | 12 | **1** |
| of which really changed style | 0 | 0 |
| boxes built | 38 | **28** |
| box subtrees cloned | 8 | 10 |

Interleaved A/B ×3 (main = merge-base `8637e82fb`, then S17, alternating), by
min:

| | main | S17 |
|---|---|---|
| `CC12_KEY` min | 3.18 / 2.84 / 2.75 ms | **2.69 / 2.60 / 2.65 ms** |
| `CC12_KEY` p95 | 7.68 / 7.24 / 7.26 ms | **7.52 / 6.86 / 6.71 ms** |
| `CC12_HOVER` min | 1.46 / 1.51 / 1.74 ms | 1.88 / 1.54 / 1.42 ms (flat) |

The `CC12_KEY` min groups separate — worst S17 (2.69) is below best main (2.75) —
but only just: ≈5 %. `CC12_HOVER` overlaps, i.e. is unchanged, which is the
intended result (this slice touches only the DOM-mutation root-set; a hover cycle
reports no DOM mutation at all).

Stage split, `LUMEN_PROFILE_TREE=1`, min / p50 over the 60 `CC12_KEY` cycles:

| Stage | main | S17 |
|---|---|---|
| `precompute_counters` | 0.45 / 0.58 ms | **0.36 / 0.47 ms** |
| `build_box` | 1.04 / 1.33 ms | **0.90 / 1.23 ms** |
| `graft_geometry` | 0.28 / 0.43 ms | 0.28 / 0.40 ms |
| `lay_out` | 0.07 / 0.09 ms | 0.07 / 0.09 ms |
| **total** | 1.94 / 2.50 ms | **1.80 / 2.18 ms** |

This is an honest small win, and the census says why it is small: cutting the
cascade from 12 nodes to 1 removed 10 of 38 rebuilt boxes, not 35 of 38. The
remaining 28 are not the cascade's doing at all.

### Where this leaves CC-12

`CC12_HOVER` unchanged: in budget by min (1.4-1.9 ms), p95 ≈4.4-5.8 ms (≈2-3×).
`CC12_KEY` ≈1.3× by min (2.60-2.69 ms vs the 2 ms budget, was ≈1.4×) and ≈3.4× by
p95.

`build_box` is still the largest stage, and after S17 the reason has changed
shape. The cascade re-cascades one node; the box tree rebuilds 28. The census's
new column says where they come from — **the ancestor chain**:

```
rebuilt chain: Document(3) html(3) body(11) div.stage(3) div.app-frame(3)
               div.app-body(7) div.main-col(13) div.toolbar(7)
               div.omnibox-wrap(9) div.omnibox(7) input#omniInput(0)
```

Eleven levels from the document node down to `#omniInput`, and a content-dirty
node makes every one of its ancestors un-clonable — each is rebuilt, and each
rebuild re-visits all of its children (66 child slots in total, of which only 10
became whole-subtree clones). **S18's question is therefore the reuse
*granularity*, not the invalidation set**: `clean_subtrees` is recorded for
elements only, so a rebuilt ancestor's text/comment children and any child whose
own subtree wasn't recorded are reconstructed from scratch rather than moved
across. Get that census before designing — three slices running, the planned
premise has not been where the time was.

## S18 — the reuse claim the next two stages re-derived from scratch

**Branch `p1-bug341-s18`.** The queue asked for the reuse *granularity*, and for
the census first.

### The census, first

`bug341_s18_keystroke_box_build_census` (new, `#[ignore]`d diagnostic in
`lumen-shell`) logs the `NodeId` of every box a cycle really builds — a
process-wide log, not a thread-local one, because `build_box` fans out over
rayon workers (S15's trap). For the `CC12_KEY` cycle:

```
built=28 (distinct 28) reused=10 chain_len=11
on-chain:        11 — Document html body div.stage div.app-frame div.app-body
                      div.main-col div.toolbar div.omnibox-wrap div.omnibox input#omniInput
non-element:     15 — doctype, 7 comments, 7 whitespace text nodes
elem-not-clean:   0
clean-but-built:  2 — svg, aside#sidebar  (no box in `prev`: display:none ⇒ Skip ⇒ filtered out)
chain child slots total=66 reused=10 built=27
cascade_recomputed=1  compute_style_calls=31
```

So the 28 are: the 11-level ancestor chain (irreducible without a finer unit
than "element subtree"), 15 non-element children of those ancestors, and 2
elements with no predecessor box. The queue's hypothesis — that the 15
non-elements are worth chasing — **is wrong, and the counter says so before any
code changed**: they cost one `compute_style` each (`counters::walk` records no
style for a non-element, so `build_box`'s cache lookup misses), and the whole
cycle runs 31 `compute_style` calls against a cascade stage that runs 1. At S10's
measured ~0.9 µs per call that is ~30 µs of a ~2600 µs cycle.

### Where the time actually was

The stage profile of the *incremental* path (`LUMEN_PROFILE_TREE=1`, no
`DETAIL` — the detail scopes opened on rayon workers are dropped, so the
in-`build_box` breakdown understates itself):

| stage | keystroke cycle |
|---|---:|
| `precompute_counters` | 0.51-0.67 ms |
| `build_box` | 0.98-1.02 ms — **of which 0.45 ms is one deep clone of 299 boxes** |
| `graft_geometry` | 0.40-0.98 ms |
| `lay_out` | 0.07-0.13 ms |

The unit of reuse was doing its job (299 of 318 boxes came across without being
rebuilt) and then having that work re-derived twice. `build_box_or_reuse` clones
a subtree out of `prev`; `mark_subtree_dirty` then marks all 318 boxes dirty;
`graft_geometry` then compares each box **against the very box it was copied
from** and clears the bit again. Two full walks per frame to re-establish a fact
the box-build stage had already proved by construction.

### The fix

A `DirtyBits::REUSED_SUBTREE` claim, set by `build_box_or_reuse` on the root of
each wholesale clone and consumed in O(1) by `graft_geometry_with_cascade`
(clear the bit, mark clean, return "reusable" without descending). It rides in
the box's own `dirty` field rather than in a side table precisely because the
producers are rayon workers: a shared set would need a lock on the hot path and
a thread-local one would lose every claim made on a worker.

`mark_subtree_dirty` deliberately keeps walking *through* a claim (it only
preserves the bit instead of overwriting it). That is what keeps every path out
of the graft that does not explicitly clean a box — a structural mismatch, a
trailing child with no predecessor — at its exact pre-S18 meaning: the box stays
dirty and is laid out fresh. The alternative (skip the mark walk too) would have
needed an explicit claim-revocation pass on each of those paths, trading a byte
write per box for a correctness surface where a missed revocation is a *stale
clean box*, i.e. visible corruption rather than a slow frame.

### Measured

Stage profile, keystroke cycle: `graft_geometry` **0.40-0.98 → 0.04-0.05 ms**.
The graft now compares 4 boxes on a hover flip (3 rebuilt + 1 claim standing for
the other 314) and <100 on a keystroke, against 318 before.

Wall-clock, interleaved A/B ×3 against `main` (`1ae2a6ef3`), by min:
`CC12_KEY` 3.00/2.50/2.58 → **2.09/2.14/2.38 ms** (≈8-14 %, the groups do not
overlap). `CC12_HOVER` 1.99/1.58/1.47 → 1.50/1.56/1.51 ms — **within this
machine's noise**, no claim made. Gates: `CC12_HOVER` in budget by min, `CC12_KEY`
≈1.1-1.2× by min (was ≈1.3×); both still red on p95.

The S13 gate and the S13 census now run with `box_reuse_off` — with reuse on
they would measure "1 box visited, 1 reused" instead of the per-box reject
census they were written for. That is the S18 claim working, not a regression.

### What S19 should look at

The census leaves two deep copies of the whole tree in every frame, and they are
now the largest items:

- **0.45 ms** — `build_box_or_reuse`'s `(*prev_box).clone()`, 299 boxes copied
  out of a `prev` the caller borrows. Making the pipeline hand `prev` over by
  value would let those subtrees be *moved* instead: detach the maximal
  reusable roots out of the owned `prev` into an id→subtree map before the
  build, leave the gutted positions to the graft — which, thanks to this
  slice, already skips exactly those positions in O(1).
- **~0.7 ms** — the pipeline's own `prev_pristine_layout = layout.clone()`
  (`clone_tree` in the `[cc12-split]` line), a second full copy for the same
  purpose.

Both are the same question one level up: the unit of reuse is a *copy*, and it
wants to be a *move* (or a share). `precompute_counters` at ~0.55 ms for a
single recomputed node is the third item — the `CounterMap` is rebuilt from
scratch every cycle.

## S19 — the unit of reuse wanted to be a move, not a copy

### The census, first

Four slices running, the queue's planned premise had landed somewhere other than
where the time was, so S19 opened with a purpose-built census
(`bug341_s19_copy_census`, `#[ignore]`d) printing every whole-tree copy a cycle
makes, each with the boxes it touched:

| item | KEY | HOVER |
|---|---|---|
| `build_box_or_reuse`'s subtree copy | **0.51-0.68 ms** / 299 boxes | **0.24-0.25 ms** / 315 boxes |
| `index_by_node` over `prev` | 0.018-0.030 ms / 318 boxes | 0.018 ms / 318 boxes |
| pipeline's `prev_pristine_layout = layout.clone()` | 0.28-0.38 ms / 318 boxes | 0.30-0.36 ms / 318 boxes |

It confirmed one half of what S18 predicted and refuted the other. The subtree
copy really was the largest item in the cycle. The index walk — the thing S4's
own measurement had blamed for rejecting whole-subtree reuse in the first place
— costs **0.02 ms** and decides nothing; had the slice started from S4's story
it would have optimised a rounding error.

### The fix

The copy was never needed. `prev` is dead the moment the pass returns, so the
reusable subtrees can be **taken** out of it:

- `layout_mutation_incremental_restyle` takes `prev` **by value**;
  `incremental_build_box` takes `&mut LayoutBox` and guts it.
- `incremental::extract_clean_subtrees` walks `prev` top-down and stops at every
  node in `CounterMap::clean_subtrees`. That set is downward-closed (a node is
  clean only if its whole subtree is), so the topmost clean box is exactly the
  unit `build_box_or_reuse` will ask for, and the walk never descends into a
  region it has already handed over — which is also why the index is now built
  over the **spine** (19 of 318 boxes on a keystroke) instead of the tree.
  `index_by_node` was deleted with this slice.
- The index is `HashMap<NodeId, Mutex<Option<LayoutBox>>>`. The takers are rayon
  workers (`build_box` fans flex/grid containers out, and chrome is built out of
  exactly those — the S15 trap), so a shared `&mut` is impossible and a single
  lock around the map would queue the workers behind each other. One `Mutex` per
  entry, each taken at most once; that "at most once" is also what keeps a box
  from ending up in two places.

### The husk, and why it is marked

Every emptied position keeps a husk carrying `DirtyBits::MOVED_OUT`. The graft
normally never reaches one — the new tree carries S18's `REUSED_SUBTREE` claim
over it and that is honoured first — but "normally" is not something the
box-build stage can promise for a region it rebuilt around the mutation, and a
husk mistaken for a predecessor is a box declared *clean* with a subtree that no
longer exists: visible corruption, not a slow frame. `graft_geometry_with_cascade`
therefore rejects a `MOVED_OUT` position exactly as it rejects a changed
identity. The husk's `kind` is `BoxKind::Skip` — "does not participate in
layout" — which on its own would already fail `kind_layout_eq` for any real box,
but not for another `Skip` (a `display:none` element's own box), and that
coincidence is precisely what the flag exists to cover.

### Gates

By counter and by conservation, never by output — a move and a copy produce the
identical tree:

- `bug341_s19_reuse_takes_the_subtree_out_of_prev_instead_of_copying_it`
  (lumen-layout): after the pass `prev` has lost the boxes the new tree gained
  and holds exactly one husk per reuse, and the index walked the spine. A
  copying mechanism leaves `prev` whole and this at zero.
- `bug341_s19_graft_refuses_a_position_whose_subtree_was_moved_out`: a husk and
  a fresh box matching on node, kind and child count — without `MOVED_OUT` the
  graft would adopt the husk's geometry.
- `bug341_s19_reuse_index_walks_the_spine_not_the_previous_tree` (lumen-shell,
  runs by default): the production document's number, < 50 of 318.

`BoxBuildStats` gained `prev_index_visited`; `take_box_clone_stats` became
`take_box_copy_stats() -> BoxCopyStats`.

### Measured

Census after the slice, same cycles: subtree reuse **0.51-0.68 → 0.002 ms**
(KEY), **0.24 → 0.000 ms** (HOVER), carrying the same 299/315 boxes; index walk
318 → **19** (KEY) / **3** (HOVER) boxes. `boxes_built`/`boxes_reused` unchanged
at 28/10 and 3/1 — the reused region is identical, only its price moved.

Wall-clock, interleaved A/B ×3 against the branch point (`faffda85d`), by min:
`CC12_HOVER` 1.32/1.57/1.38 → **0.81/0.94/0.89 ms** (≈1.6×), `CC12_KEY`
2.43/2.28/2.24 → **1.90/2.09/2.07 ms** (≈8-16 %). Neither pair of groups
overlaps. HOVER gains more than the census's 0.25 ms: the copy also had to be
*freed* again each frame, and that allocator traffic leaves with it.

### Where this leaves CC-12

`CC12_HOVER` is inside the 2 ms budget by min (0.81-0.94) and ≈1.1-1.6× on p95
(2.27-3.17). `CC12_KEY` is at the budget by min (1.90-2.09) and ≈2.3× on p95
(4.49-4.78). Still red on p95, and the gap is now p95-only on both scenarios.

### What S20 should look at

The census's other two items are untouched and are now the largest:

- **~0.3-0.5 ms** — the pipeline's own `prev_pristine_layout = layout.clone()`.
  Unlike the copy this slice removed, this one is not obviously unnecessary:
  `relayout_chrome_host` keeps a pristine tree because `take_content_area`
  mutates the live one straight after. Making it a move needs that mutation to
  become restorable (detach `#contentArea`, re-attach it before the next cycle)
  and the live tree to be relinquished at the top of the next pass — a real
  design, not a signature change.
- **~0.55 ms** — `precompute_counters` for a *single* recomputed node: the
  `CounterMap` (styles, counter snapshots, `clean_subtrees`) is rebuilt from
  scratch every cycle, ~5 hash operations × 828 nodes to reproduce what it
  already had.

Same shape as this slice: ask what the stage re-creates that it could carry
over. And the same discipline — census first.

## S20 — the fan-out that dispatched workers to move boxes

### The census, first

The queue named two items for this slice. A purpose-built stage census
(`bug341_s20_stage_census`, `#[ignore]`d) measured them before a line was
changed, and found neither one anywhere near the top:

| item, per cycle | KEY | HOVER |
|---|---|---|
| pipeline's `layout.clone()` (`clone_tree`) | 0.29-0.34 ms | 0.19-0.22 ms |
| `CounterMap` construction (replayed over the real sizes) | 0.16-0.17 ms | 0.11 ms |
| `CascadeIndex` rebuild forced by `invalidate_rule_idx_cache` | 0.18-0.21 ms | 0.12-0.14 ms |
| pseudo-element cascades (139 / 129 calls, 12 / 6 hits) | 0.14 ms | 0.08-0.09 ms |
| boxes really built, Σ self-time | **1.15-1.24 ms** | 0.05 ms |

The queue put `clone_tree` at 0.3-0.5 ms and the `CounterMap` at 0.55 ms; they
are 0.3 and 0.16. The item that dominates a keystroke is the box-build stage —
28 boxes out of 318, of which the census's per-box breakdown gave three
containers almost all of it: `body` 0.33 ms, `.main-col` 0.27 ms,
`.omnibox-wrap` 0.45 ms of **self** time, i.e. excluding every box built
beneath them.

Nothing in `build_box`'s body explains a third of a millisecond for one
container. Its two `compute_pseudo_element_style` calls were the obvious
suspect (S10 found exactly that shape at the cascade stage) — and the census
priced the whole document's 139 pseudo cascades at 0.14 ms, so they are not it.

The answer was the one thing those three containers have in common and the two
cheap ones do not: eight or more children, hence ADR-016 M4.1's rayon fan-out.
Setting `RAYON_MIN_FLEX_CHILDREN` to `usize::MAX` and re-running the census
settled it — KEY pass **2.51 → 0.907 ms**, Σ self-time **1.24 → 0.28 ms**,
`body` 0.33 → **0.004 ms**, `.main-col` 0.27 → **0.004 ms**.

### What was wrong

M4.1 sized that threshold against a **full** pass, where each of a container's
children costs a selector match, a cascade and a recursive build; eight of
those comfortably outweigh a worker dispatch. On the incremental path since
S15/S19 a child that is in the reuse index costs a `Mutex` lock and a move. The
threshold never learned this: it reads `dom_children.len()`, so a chrome
interaction dispatched a worker per subtree the pass was about to relocate in
O(1) — and chrome is built out of exactly such containers, which is the same
sentence S15 wrote about the reuse gate and S19 about the index.

This is S18's lesson at a different stage. The reuse mechanism had already
established which children are free; the fan-out decision re-derived a *worse*
answer from the DOM instead of asking it.

### The fix, and the half-fix that came first

The estimate now counts the children the pass will really **build**:

```rust
let children_to_build = match prev_index {
    None => dom_children.len(),                       // full path, M4.1 verbatim
    Some(idx) => dom_children.iter()
        .filter(|&&c| !idx.contains_key(&c)
            && matches!(doc.get(c).data, NodeData::Element { .. }))
        .count(),
};
```

The first version had only the `contains_key` half, and it moved nothing:
still 2 fan-outs, still 1.08-1.24 ms of self time. `clean_subtrees` records
elements only, so the whitespace text nodes between pretty-printed markup are
never in the index — and `chrome.html` has enough of them per container to hold
`body` above eight on their own. They cost a `Skip` box or one small anonymous
item, never the cascade the threshold was sized against, so they are excluded
from the estimate too. `prev_index: None` — every full-layout entry point —
keeps `dom_children.len()` and M4.1's behaviour byte for byte.

### Gate — by counter

`BoxBuildStats::fanouts` counts containers dispatched onto rayon (workers fold
their own nested dispatches back through the same drain as `built`/`reused`).
`bug341_s20_keystroke_moves_subtrees_without_dispatching_workers` (lumen-shell,
runs by default) asserts **both** arms: the first, full cycle still fans out,
and the keystroke cycle does not. The full-pass arm is the one that matters
most — the cheapest way to make the headline number pass is to stop
parallelising altogether, which would cost the full pass the parallel selector
matching M4.1 exists for.

A counter and not wall-clock, for the usual reason: the fan-out produces the
identical tree, so every differential test in this track passes either way.

### Measured

Census after the slice, same cycles: `fanouts` **2 → 0**, KEY pass 2.33-2.51 →
**0.92-1.08 ms**, Σ self-time 1.15-1.24 → **0.27-0.30 ms**, `body` 0.005 ms,
`.main-col` 0.004 ms. HOVER is untouched by construction — it builds 3 boxes,
none of them an element, so it never reached the branch.

Wall-clock, interleaved A/B ×3 against the branch point (`eb72f811a`), by min:
`CC12_KEY` 2.20/2.16/1.99 → **1.26/1.29/1.18 ms** (≈1.7×), groups do not
overlap; p95 4.71/5.38/4.35 → **3.39/3.10/3.47 ms**. `CC12_HOVER`
0.97/0.97/0.91 → 1.02/0.89/0.91 — **unchanged, and no claim is made**; its p95
3.31/3.35/3.06 → 2.46/2.97/3.05 is inside the machine's noise.

One census artefact worth recording: the pseudo-cascade count went 139 → 160
across the fix. Nothing started calling more of them — the tally is
thread-local, and the calls that used to happen on rayon workers were invisible
to it. That is the S15 trap for the third time, here in a census rather than in
a gate.

### Where this leaves CC-12

`CC12_KEY` is inside the 2 ms budget by min (1.18-1.29) and ≈1.6-1.7× on p95
(3.10-3.47, was ≈2.2-2.7×). `CC12_HOVER` is inside by min (0.89-1.02) and
≈1.2-1.5× on p95. Still red on p95 only, on both scenarios.

### What S21 should look at

The census's own table, minus the item this slice removed. The largest is no
longer a copy or a map:

- **0.12-0.21 ms every pass, on both scenarios** — `CascadeIndex::build`. Every
  layout pass opens with `invalidate_rule_idx_cache()`, so the sheet is
  re-indexed from scratch (`RuleIndex` for the top level plus every
  `@layer`/`@media`/`@supports` block, plus two full-sheet predicate scans) on
  the first `compute_style` of the frame — on a HOVER cycle that recomputes
  **zero** nodes, this is the single biggest item in the pass. It is paid again
  per rayon worker that does any style work, because `StyleEnvSnapshot::install`
  calls the same invalidation. The invalidation exists to defend a raw-pointer
  cache key against address reuse across sessions (see its doc comment); a
  sheet identity that is not an address would retire it. Note the counter for
  it (`style::take_cascade_index_stats`) is thread-local and therefore reports
  the layout thread only — the worker rebuilds are still uncounted.
- ~0.16-0.34 ms — the pipeline's `layout.clone()`, with the design S19
  described (make `take_content_area` restorable so the live tree can be
  relinquished at the top of the next pass).
- ~0.09-0.17 ms — the `CounterMap` rebuilt from scratch.

Same discipline. Census first: this was the sixth slice in a row to run one,
and the fifth where it moved the target.

## S21 — the cascade index that could not outlive the pass that built it

### The census, first

`bug341_s21_cascade_index_census` (`#[ignore]`d) — the seventh in a row, and
the **first to confirm the queue's premise** rather than move it. It also
doubled the size of the finding, because the counter S20 read it with was
thread-local while the code it counts is not (the S15 trap, third appearance):
`build_box` fans flex/grid containers onto rayon, and every worker's
`StyleEnvSnapshot::install` dropped that worker's index cache too. Making the
tally see the workers is what turned "one rebuild per pass" into this:

| pass | rebuilds | Σ time | share of pass |
|---|---:|---:|---:|
| KEY, incremental cycle | 1 | 0.16-0.22 ms | 7-13 % |
| HOVER, incremental cycle | 1 | 0.14-0.21 ms | 14-19 % |
| full pass | **33** | **7.57 ms** (Σ over the worker pool) | of a 27.3 ms pass |

Split of one rebuild on `chrome.html` (373 rules, no `@media`/`@layer`/
`@supports` blocks): the top-level `RuleIndex::build` 0.08-0.11 ms and the two
sheet-wide predicate scans (`has_webkit_scrollbar_rules`, `has_quote_content`)
0.07-0.11 ms — roughly half each; the per-block indexes and the `@media`
activity evaluation are zero because this sheet has no blocks. On a HOVER cycle,
which re-cascades **zero** nodes since S14, the re-index was the single largest
item in the whole pass.

### What was wrong

The cache key was the stylesheet's **address** (plus its rule count and a media
key). An address is not an identity: the allocator hands it straight back when
the sheet is freed, so a freed sheet's index could be served to whatever landed
at the same place next. The defence was `invalidate_rule_idx_cache()` at the top
of every layout pass — 28 call sites — and the same call inside
`StyleEnvSnapshot::install` on every rayon worker. Both are correct, and
together they reduce a cross-pass cache to a within-pass one: the index is
rebuilt on the first `compute_style` of every frame, on every thread that does
style work, whether or not the sheet changed.

Note the shape. This is not a cache that was mis-sized or mis-keyed for
performance — it was **keyed by something that cannot be an identity**, and
every mitigation that follows from that is a mitigation of the key.

### The fix

`lumen_css_parser::StylesheetRevision` — a process-unique `u64` minted for
every `Stylesheet` that comes into existence (`parse`, `Default`, `Clone`) and
never reused. `Clone` and `PartialEq` are hand-written for it: a clone is a
separate sheet its owner may mutate independently, so it gets its **own**
revision; equality is content, so it ignores the revision. The cache key becomes
(revision, media key), and the invalidation is deleted outright — the function
is gone, not made a no-op.

The cache holds **two** slots, not one. A browser frame lays out two documents
on one thread — its own chrome and the page — and one slot would make them evict
each other every frame, which is the same per-pass rebuild reintroduced through
the cache's size instead of its key. A third sheet evicts the least recently
used one, so the cache stays bounded rather than growing per navigation.

### The invariant, and who guards it

A revision-keyed cache is sound only while *"revision unchanged ⇒ rules
unchanged"* holds. Breaking that promise produces **visibly wrong styles**, not
a slow frame, and it breaks the day someone three crates away adds an innocuous
`sheet.rules.push(..)` — so it is not left to review:

- `Stylesheet::merge_from` performs the whole merge and mints a new revision;
  `Stylesheet::mark_mutated` is the escape hatch for direct field access.
- `every_stylesheet_mutation_in_the_workspace_announces_itself` (lumen-css-parser)
  walks the workspace sources and fails the build on any
  `push`/`extend`/`insert`/… into a rule container outside `parser.rs`. Only
  files that name `Stylesheet` are scanned — the field names (`rules`,
  `imports`, `properties`) are ordinary words other types use. Verified red by
  reinstating the mutation it was written for. Same shape as `lumen_chrome`'s
  `every_dom_mutation_in_model_rs_goes_through_a_tracked_primitive` (S16).

`merge_from` also replaced a hand-rolled field-by-field merge in
`LoadEvent::CssLoaded`, which had fallen two fields behind the struct — a
streamed `@color-profile` or `@function` was silently dropped. Its exhaustive
destructuring pattern makes the next added field a compile error rather than a
silent omission.

### Gates — by counter

An index rebuilt from scratch on every node of every pass produces byte-identical
styles, so nothing in the differential suite can see it, and 0.2 ms is well
inside machine noise. Each gate asserts **both** arms, because "never reuse" and
"never rebuild" are equally trivial one-liners and the second one serves an
empty index, i.e. wrong styles:

- `bug341_s21_repeated_cascades_over_one_sheet_build_the_index_once` — cold
  build is 1, twenty more cascades add 0.
- `bug341_s21_a_mutated_sheet_is_reindexed_and_its_new_rules_apply` — the
  soundness arm: a rule merged in after the index was built must apply.
- `bug341_s21_a_resize_reindexes_because_it_changes_which_media_blocks_apply`.
- `bug341_s21_two_documents_on_one_thread_do_not_evict_each_other` — ten
  alternations build two indexes; a third sheet does evict.
- `bug341_s21_interaction_cycles_do_not_reindex_the_stylesheet` (lumen-shell,
  runs by default) — the real chrome pipeline: cold pass ≥ 1, eight keystroke
  and hover cycles exactly 0.

`CascadeIndexStats` stayed thread-local but the rayon fan-out now drains each
worker's tally into the parent thread, exactly as `BoxBuildStats` does. A
process-wide counter is accurate but unusable as a gate: "this pass rebuilt
nothing" fails whenever a concurrent test builds an index.

### Measurement

Interleaved A/B ×3, by min (`cc12_chrome_perf_gate`):

| scenario | main | S21 |
|---|---|---|
| `CC12_HOVER` | 0.82 / 0.85 / 0.88 ms | **0.78 / 0.70 / 0.77 ms** |
| `CC12_KEY` | 1.17 / 1.25 / 1.27 ms | **1.12 / 1.15 / 1.10 ms** |

Groups do not overlap on either scenario (≈10-13 % and ≈5-13 %). The first A/B
run was discarded: both arms had launched from the branch worktree — S9's `cd`
gotcha, in its exact documented form.

Census after the slice: **0 rebuilds** on every incremental cycle and on the
full pass. The full pass's 33 rebuilds / 7.57 ms of aggregate worker CPU are
gone as a **count**; its wall-clock was not measured A/B, so no claim is made
for it. `dump_golden.py` 12/12 with no diff.

CC-12 gate: unchanged in shape from S20 — both scenarios inside budget by min,
red on p95 only.

### What S22 should look at

The census table minus this slice's item. Both remaining entries are the ones
S20 listed, unchanged:

- ~0.16-0.34 ms — the pipeline's `layout.clone()`, with the design S19
  described (make `take_content_area` restorable so the live tree can be
  relinquished at the top of the next pass).
- ~0.09-0.17 ms — the `CounterMap` rebuilt from scratch.

Neither has been re-measured since S20's census, and every slice that trusted an
un-remeasured note has paid for it (S19's `index_by_node`, S20's `clone_tree`).
Census first.

## S22 — the copy that existed to hold a difference

### The census, first

`bug341_s20_stage_census`, re-run unchanged on the branch point — the eighth
census in a row, and the second (after S21) to **confirm** the queue's premise
rather than move it:

| item, per cycle | KEY | HOVER |
|---|---:|---:|
| whole pass | 0.75-1.13 ms | 0.44 ms |
| pipeline's `layout.clone()` (`clone_tree`) | **0.16-0.40 ms** | **0.17 ms** |
| pseudo-element cascades (160 / 129 calls) | 0.17-0.21 ms | 0.09-0.10 ms |
| `CounterMap` construction (replayed) | 0.11 ms | 0.10 ms |
| `CascadeIndex` rebuilds | **0** (S21) | **0** (S21) |
| boxes really built, Σ self-time | 0.30 ms | 0.05 ms |

`clone_tree` is the largest item on both scenarios and ~40 % of a hover cycle,
which since S14 recomputes zero nodes. The other item the queue named — the
`CounterMap` — is 0.10-0.11 ms, i.e. below the pseudo-element cascades nobody
had queued.

### What was wrong

The copy existed to hold a **difference**, not a tree. `relayout_chrome_host`
lays out the whole chrome document, then immediately prunes `#contentArea` out
of the live tree (`take_content_area`, CC-4/CC-9) because the real page is
painted separately at that rect. The next pass needs the *pre*-pruning tree as
its incremental basis, so S5 kept a whole second copy of it — 318 boxes copied
every frame to preserve the 163 that were about to be removed, plus their slot.

### The fix

The pruning became **reversible**. `take_content_area` now returns a
`ContentAreaDetachment` — the holder's child-index path, the slot,
`#contentArea`'s own subtree, and the path each salvaged popover was lifted
from — and `restore_content_area` undoes it at the top of the next pass, on the
tree the previous pass left in `chrome_layout`. The basis is reconstructed for
the price of one insert per salvaged popover; `chrome_prev_pristine_layout` is
gone. Sound because nothing mutates `chrome_layout` between passes: it is
read-only from the moment it is assigned until the next pass replaces it
wholesale (checked — every other reference is `as_ref`).

Salvaged paths are replayed in **reverse** order, because each was recorded
against the tree state of its own removal.

### What the wrong basis actually costs — measured, not assumed

The first draft of the gate assumed a misaligned basis would merely make the
pass rebuild the region. It does not. `#contentArea`'s parent is clean on an
interaction cycle, so `incremental_build_box` moves that whole subtree across
from the basis in O(1) — a basis missing `#contentArea` therefore yields a
**document** missing `#contentArea`, 155 boxes instead of 318, and the next
cycle inherits that tree in turn. It never recovers. And no differential test
in this track can see it, because the chrome host paints the real page over
that rect anyway.

So the failure mode here is a wrong tree, not a slow frame — which is why the
restore is gated on exact identity rather than on a count. The one genuinely
soft failure is `restore_content_area` returning `false` (a recorded path no
longer addresses a box): the caller drops the basis and takes the full-layout
path, so a stale record costs a slow frame.

### Gates — each on both arms

- `bug341_s22_restoring_a_detachment_reproduces_the_pristine_tree` — the
  restored chrome tree equals the copy box for box, comparing `node`, `rect`,
  `BoxKind` discriminant and `style` by **`Arc` identity**. Other arm: the
  pruning must actually remove boxes.
- `bug341_s22_restoring_puts_salvaged_popovers_back_where_they_came_from` —
  the salvage half, which the real document cannot exercise: every salvageable
  popover is `display:none` until opened, so at rest `salvage_paths` is empty
  and a broken salvage restore would go unnoticed. The fixture nests one
  popover inside another element so the paths are deeper than one level and
  their order matters.
- `bug341_s22_a_restored_basis_carries_the_whole_document_forward` — three
  production-shaped cycles per arm: restored basis 318 boxes, pruned basis 155.
  Second arm: the production arm's steady-state cycle must stay *incremental*
  (`built` small, `reused > 0`), or a restore that silently failed would pass
  the box-count assert via a full-layout fallback.

All three run by default.

### Measured

Interleaved A/B ×3 against the branch point (`1b6d84df2`), by min:

| scenario | main | S22 |
|---|---|---|
| `CC12_HOVER` | 0.73 / 0.78 / 0.83 ms | **0.58 / 0.59 / 0.56 ms** |
| `CC12_KEY` | 1.11 / 1.18 / 1.10 ms | **0.93 / 0.92 / 0.97 ms** |

Groups do not overlap on either scenario (≈24-33 % and ≈12-18 %). p95:
`CC12_HOVER` 2.40/2.53/2.37 → **1.68/1.91/2.09**, `CC12_KEY` 3.54/3.25/2.93 →
**3.00/2.59/2.83**.

`cc12_bench_cycle` persists the tree by move instead of `clone()`. That bench
has never modelled the pruning, so the move is the whole of the change there;
production additionally pays the restore, which is one insert per salvaged
popover plus a path walk and does not scale with the tree.

### Where this leaves CC-12

`CC12_HOVER` is inside the 2 ms budget by min (0.56-0.59) and **inside it on
p95 in two of three runs** (1.68/1.91/2.09) — the first time either scenario
has come this close on p95. `CC12_KEY` is inside by min (0.92-0.97) and
≈1.3-1.5× on p95 (2.59-3.00, was ≈1.5-1.8×). Still formally red on p95, on
`CC12_KEY` clearly and on `CC12_HOVER` marginally.

### What S23 should look at

The census table minus this slice's item. The queue's own remaining entry is no
longer the largest:

- **0.09-0.21 ms** — pseudo-element cascades: 160 calls on a KEY cycle, 129 on
  a HOVER cycle, 19 / 6 of them hits. Never queued; it has simply outlived
  everything above it. A HOVER cycle recomputes zero nodes and still runs 129
  of these.
- ~0.10-0.11 ms — the `CounterMap` rebuilt from scratch, the last of S20's
  original pair.

Census first, and re-measure the note before trusting it — S22 is the second
slice running where the queue was right, after five where it was not.

## S23 — the pseudo-element nobody had written a rule for

### The census, first

`bug341_s20_stage_census`, re-run on the branch point — the ninth in a row.
Before a line changed it was **taught to see all threads**: `PseudoCascadeStats`
was thread-local while `build_box` fans flex/grid containers out over rayon
workers (the S15 trap, fourth appearance), so the worker tallies were dropped
on the floor. That is why S20's reading moved from 139 to 160 "for no reason".
The fan-out closure now drains each worker's tally into the parent, as
`BoxBuildStats` and `CascadeIndexStats` already did.

The second half of the census was a **split by pseudo-element name**: "160
calls" does not say which of the ~14 call sites made them, and the fix for each
is different. That split is the whole finding:

| pseudo, per cycle | KEY calls / hits | HOVER calls / hits | time |
|---|---:|---:|---:|
| `::first-line` | **123 / 0** | **123 / 0** | 0.076-0.143 ms |
| `::-webkit-scrollbar*` | 18 / 18 | 6 / 6 | 0.03-0.06 ms |
| `::before` + `::after` | 18 / 0 | 0 / 0 | 0.006-0.039 ms |
| `::placeholder` | 1 / 1 | 0 / 0 | 0.007-0.013 ms |

A HOVER cycle recomputes **zero** nodes (S14) and builds three boxes (S15/S19),
and still asked the cascade about `::first-line` 123 times. `chrome.html` has
no `::first-line` rule at all, so not one of those probes could ever hit.

The queue's own second item, the `CounterMap`, replayed at 0.115-0.272 ms —
still real, and now the largest named item.

### What was wrong

Two things, both the S21 shape: a **node-independent fact about the sheet**
re-derived at every node.

1. `apply_first_line_pseudo_styles` is a post-layout walk over the laid-out
   tree that asks `compute_pseudo_element_style(node, "first-line", …)` on
   every block box. Whether the sheet contains a `::first-line` rule is decided
   once for the whole sheet, but the walk asked per box, and the walk itself was
   entered unconditionally. `build_box` has a second such probe, per
   inline-content block.
2. `counters::walk` stored one counter snapshot per element unconditionally. In
   a document with no `counter-reset`/`-increment`/`-set` in scope — every
   document that does not use counters, `chrome.html` among them — all 828 of
   them were clones of an empty map. Both consumers read a snapshot through
   `snap.and_then(|s| s.get(name))`, so an empty snapshot and an absent one are
   indistinguishable.

Alongside those, `CounterMap`'s two per-element collections were built from
`HashMap::default()` and rehashed their way up to ~830 entries — moving roughly
as many entries again as they finally hold.

### The fix

`CascadeIndex::pseudo_subjects` — every pseudo-element name the sheet uses as a
selector **subject**, which is the only position `matches_complex_for_pseudo`
inspects, so the predicate is exactly as wide as the matcher it short-circuits.
`style::sheet_targets_pseudo` exposes it, and it is used to skip *traversals*,
not just individual cascades: `apply_first_line_pseudo_styles` checks once and
does not recurse at all, `build_box`'s probe is guarded the same way, and
`compute_pseudo_element_style_inner` short-circuits for every other
pseudo-element.

`::marker` is the one exception the short-circuit honours: CSS Lists L3 §2.1
synthesizes a marker style out of `list-style-type` with **no rule at all**, so
"the sheet does not mention `::marker`" says nothing about whether the cascade
should return one.

The kind-to-name correspondence is now a single function, `pseudo_element_name`,
which `pseudo_element_matches` delegates to. A predicate that drifted from its
matcher would silently drop a pseudo-element's styling — visible corruption,
not a slow frame — so the two are made structurally incapable of disagreeing
rather than kept in sync by review.

For the counters: `walk` stores a snapshot only when the counter stacks are
non-empty. The check is on the **live stacks**, not on a sheet-wide predicate,
so a document that uses counters in one subtree still pays nothing outside it.
`CounterMap::with_capacity` sizes `styles`/`clean_subtrees` from
`node_count` (full pass) or `prev_styles.len()` (incremental — a closer
estimate, it does not count text nodes).

### Gates

By counter, both arms each, because every one of these mechanisms produces
byte-identical output when broken — just slowly, or silently empty:

- `bug341_s23_first_line_is_probed_only_by_a_sheet_that_uses_it` (lumen-layout)
  — a sheet without the rule makes **zero** probes; the same fixture with
  `p::first-line { color: green }` still probes *and* still paints the first
  line green. Without the second arm the cheapest way to zero the probe count
  is to stop applying `::first-line` altogether.
- `bug341_s23_counter_snapshots_are_stored_only_where_a_counter_is_in_scope`
  (lumen-layout) — zero snapshots on a counter-free document; on an `<ol>` the
  two `<li>` still resolve `n` to 1 and 2. Storing nothing at all would pass the
  first arm and render every `counter()` as `0`.
- `sheet_pseudo_subjects_cover_every_container_and_only_the_subject` — every
  container (`@media`/`@supports`/`@layer`) arms the predicate on its own,
  a non-subject position does not, case and vendor names carry through.
- `marker_pseudo_style_survives_a_sheet_that_never_mentions_it`.

The census was taught the same lesson it just applied:
`CounterMap::counter_snapshot_count` reports the real map size (deriving it
from `styles`' size would have kept reporting the removed cost), and the
census's replays reserve capacity because production now does.

### Measurement

Census after the slice: pseudo-element cascades on HOVER **129 calls to 6**
(0.176 to 0.057 ms, 6 hits out of 6), on KEY 160 to 37; `CounterMap` replay on
HOVER 0.271 to 0.032 ms, on KEY 0.157 to 0.056-0.138 ms, snapshots 828 to **0**.

Interleaved A/B x3 (S9's protocol), by min:

| scenario | main | S23 |
|---|---|---|
| `CC12_HOVER` min | 0.57 / 0.56 / 0.56 ms | **0.45 / 0.45 / 0.49 ms** (~18-21 %) |
| `CC12_KEY` min | 0.91 / 0.98 / 0.90 ms | **0.73 / 0.77 / 0.77 ms** (~14-21 %) |
| `CC12_HOVER` p95 | 1.52 / 2.56 / 1.58 ms | **1.39 / 1.66 / 1.20 ms** |
| `CC12_KEY` p95 | 2.54 / 2.70 / 2.86 ms | 2.35 / 2.50 / 2.09 ms |

The groups do not overlap on min in any round on either scenario.
`dump_golden.py` 12/12 with no diff.

### Where this leaves CC-12

`CC12_HOVER` is inside the 2 ms budget by min **and on p95 in all three runs**
(1.20-1.66) — the first time either scenario has cleared p95 outright.
`CC12_KEY` is inside by min (0.73-0.77) and ~1.05-1.25x on p95 (2.09-2.50, was
~1.3-1.5x). The gate is formally red on `CC12_KEY`'s p95 alone.

### What S24 should look at

Both of this slice's items are now floors, not costs: the pseudo stage is 6
calls with 6 hits, and the counter snapshots are gone. What remains on the KEY
cycle, from the same census:

- **`CounterMap::styles` and `clean_subtrees`, still rebuilt from scratch** —
  828 inserts each, ~0.02-0.12 ms after the capacity reservation. A HOVER cycle
  reuses every one of the 828 styles from `prev_styles` and then re-inserts all
  828 into a fresh map, and the pipeline afterwards clones the map again to
  become the next cycle's `prev_styles`. The S19 answer applies verbatim: hand
  the previous map **in by value** and overwrite only the recomputed entries
  instead of rebuilding it. Two things must be settled first — how entries for
  nodes deleted from the DOM are evicted (a recycled `NodeId` served a stale
  style is visible corruption, not a slow frame), and that `clean_subtrees` is
  a *per-pass* fact and cannot be carried over, only recomputed or inverted
  (represent the dirty closure instead of its complement).
- the box-build stage on KEY (28 boxes, ~0.3 ms of self-time), the largest
  single stage left.

Census first, and check it sees all threads: S23 is the fourth slice where a
thread-local tally was the thing that had to be fixed before the finding could
be trusted.

## Repro

```bash
cargo test -p lumen-shell --profile dev-release cc12_chrome_perf_gate -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s3_incremental_cascade_precompute_share -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s4_incremental_box_build_share -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s5_incremental_pipeline_share -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s13_graft_reject_census -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s17_keystroke_restyle_census -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s18_keystroke_box_build_census -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s19_copy_census -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s20_stage_census -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s21_cascade_index_census -- --ignored --nocapture
```

S22 re-used `bug341_s20_stage_census` unchanged — it already prints both items
the queue named.

S23 extended the same census rather than adding one: it drains the
pseudo-element tally off the rayon workers, splits it by pseudo-element name,
and reports `CounterMap`'s real snapshot count. Read its `pseudo ::<name>` lines
for the per-call-site split.
