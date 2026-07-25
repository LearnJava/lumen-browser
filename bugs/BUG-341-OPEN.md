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

## Repro

```bash
cargo test -p lumen-shell --profile dev-release cc12_chrome_perf_gate -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s3_incremental_cascade_precompute_share -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s4_incremental_box_build_share -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s5_incremental_pipeline_share -- --ignored --nocapture
```
