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

## Repro

```bash
cargo test -p lumen-shell --profile dev-release cc12_chrome_perf_gate -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s3_incremental_cascade_precompute_share -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s4_incremental_box_build_share -- --ignored --nocapture
cargo test -p lumen-shell --profile dev-release bug341_s5_incremental_pipeline_share -- --ignored --nocapture
```
