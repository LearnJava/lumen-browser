# p1-bug341-incremental-restyle — incremental restyle + layout to close CC-12's 2 ms gate

**Owner:** P1 (layout-engine performance).
**Blocks:** CC-14 (chrome default flip), CC-15 (legacy-chrome removal).
**Bug:** [BUG-341](../../bugs/BUG-341-OPEN.md).
**Branch:** `p1-bug341-incremental-restyle`.

This is the design brief for the work BUG-341's "Fix scope note" gestured at. It
supersedes the earlier framing ("a layout-result cache for `lay_out_flex`"),
which the profiling below proves is **insufficient** — the cache would address
only ~35 % of the per-interaction cost.

---

## 1. Measured problem (why the earlier plan can't work)

`cc12_chrome_perf_gate` measures `relayout_chrome_host`'s
mutate→restyle→relayout→paint cycle on a populated chrome document
(~2300 reachable nodes). Budget: **≤ 2 ms p95**.

Profiling the *current* code (main + all three prior BUG-341 fixes) with
`LUMEN_PROFILE_TREE=1`, averaged over 77 `layout_measured_hyp` passes:

| Stage | Avg | Share | What it does |
|---|---:|---:|---|
| `precompute_counters` | 155 ms | **53 %** | **full style cascade** — one `compute_style` per node, cached into `CounterMap::styles` for `build_box` to reuse |
| `lay_out` | 102 ms | **35 %** | box placement (the `lay_out_flex` double-layout lived here) |
| `build_box` | 22 ms | 8 % | box-tree construction (reuses the cascade cache, so cheap) |
| `post_layout_passes` | 3 ms | 1 % | container queries, anchor positioning, first-line split |

(Absolute ms on this dev machine run ~3× higher than BUG-341's documented
`p50≈85 ms` — machine/contention difference. The **relative split is the load-
bearing result** and is stable across runs.)

**Consequence:** a layout-result cache that made `lay_out` free would still
leave `precompute_counters` (53 %) + `build_box` (8 %) = 61 % of the cost
standing — on BUG-341's reference machine, ~52 ms, still **~26× over budget**.
The dominant cost is the **style cascade**, and *every* current layout entry
point — `layout_measured_hyp`, `layout_streaming_incremental`,
`layout_mutation_incremental` — recomputes it in full on every call. None of
them is incremental in the cascade.

**Therefore the only path to ≤ 2 ms is to make per-interaction work O(changed
subtree) across all three stages**, not just layout.

---

## 2. What already exists (reuse, don't rebuild)

The **layout** half is mature (`crates/engine/layout/src/incremental.rs`):

- `DirtyBits` (`SELF_SIZE` / `HAS_DIRTY_DESCENDANT` / `SUBTREE`) on every
  `LayoutBox`.
- `mark_dirty` / `mark_dirty_set` / `clear_dirty` — subtree ratchet.
- `INCREMENTAL_LAYOUT_MODE` + the early-return fast path in `lay_out_inner`
  (`box_tree.rs:5507`): a clean box is O(1)-translated via `translate_subtree`
  instead of re-laid-out.
- `lay_out_incremental`, and `graft_geometry` (reuses geometry from a `prev`
  tree for structurally-identical, same-style subtrees). `graft_geometry`'s
  O(depth) redundant-clone bug was fixed in BUG-341's third session — it is now
  O(1) per node.

The **cascade** half does **not** exist. There is:

- No persistent per-node `ComputedStyle` cache across cycles (`CounterMap` is
  rebuilt every call and dropped).
- No restyle-damage / style-invalidation model (nothing computes "which nodes'
  computed style could change given this state change").
- A per-thread *rule-index* cache (`style::invalidate_rule_idx_cache`) that
  speeds selector matching within one pass but is invalidated every pass.

Interactive state (`:hover`/`:focus`/`:active`) is a thread-local set by
`set_interactive_state(hover, focus, active)` before layout and read by
`matches_pseudo_class` (`style.rs:8480`). A hover flip therefore changes the
cascade input for a *small, computable* set of nodes.

---

## 3. Target architecture

Persist a `LayoutState` across interaction cycles (owned by the shell for the
chrome document, and by the page pipeline for the page document):

```
LayoutState {
    box_tree: LayoutBox,                    // last laid-out tree (already persisted for graft)
    styles:   HashMap<NodeId, ComputedStyle>, // last cascade result (NEW — today thrown away)
    counters: CounterMap,                   // last counter snapshot (NEW)
    // selector-dependency indices (NEW, §4)
}
```

Per interaction:

1. **Compute a restyle root-set** (§4) — the minimal set of nodes whose
   `ComputedStyle` can change given the state/DOM delta.
2. **Incremental cascade** — recompute `compute_style` only for the root-set and
   their style-descendants (inheritance forces children to re-cascade when a
   parent's inherited properties change); reuse cached `ComputedStyle` for every
   other node. Produce restyle-damage per node (does size change? paint only?).
3. **Incremental box-build** — reuse cached `LayoutBox`es for subtrees whose
   style is unchanged (extend the `graft_geometry` idea to skip *box construction*
   too, not just geometry); rebuild only damaged subtrees.
4. **Incremental layout** — `mark_dirty` the size-damaged nodes, run
   `lay_out_incremental`.
5. **Post-layout passes** — already sub-ms; run as today, or gate behind "any
   container-query/anchor node damaged".

For a hover flip on `#sidebar`, the root-set is `#sidebar` + nodes matched by
hover-dependent rules touching it — dozens of nodes, not 2300.

---

## 4. Style invalidation model (the hard, correctness-critical part)

This is where regressions hide. Conservative-but-correct first, tighten later.

**Inputs that change per cycle:**
- Interactive state delta: `:hover`/`:focus`/`:active` node id changed.
- DOM delta: `bind_model` attribute/text/structure mutations (chrome), or JS DOM
  mutations (page).

**Root-set derivation (v1, conservative):**
- For an interactive-state change on node `N`: invalidate `N` and its entire
  subtree, **plus** any node reachable by a rule whose selector combines a
  dynamic pseudo with a combinator crossing out of `N`'s subtree
  (`N:hover ~ X`, `N:hover + X`, `N:hover X`). v1 may over-approximate the
  sibling/descendant fan-out by invalidating from `N`'s parent down; measure
  before tightening.
- For a DOM attribute/class change on `N`: invalidate `N`'s subtree + selector-
  affected siblings/descendants (same combinator reasoning against
  class/attribute selectors).
- Inheritance: when a re-cascaded node's *inherited* properties differ from its
  cached value, its children join the root-set (transitive).

**Correctness gate:** an incremental restyle+relayout must be **bit-identical**
to a full `layout_measured_hyp` for the same final state. This is the invariant
the existing incremental tests already assert for layout
(`mutation_incremental_style_change_matches_full`, etc.); extend the same
"incr == full" differential-test pattern to cover the cascade. Any divergence is
a bug in the invalidation set (too narrow), not an acceptable trade-off.

---

## 5. Slices

Each slice is independently mergeable, guarded, and check-in-gated.

- **S1 — design + profiling instrumentation (this doc).** Record the split in
  BUG-341. Land a committed (not ad-hoc) sub-stage timing so regressions in any
  of the three stages are visible later. *Deliverable: this file + BUG-341
  update.* ✅
- **S2 — persist `styles` + differential-test harness.** Return `CounterMap`
  (incl. `styles`) from `layout_measured_hyp` alongside the tree; add an
  `incr_cascade == full_cascade` differential test scaffold (initially trivially
  equal — full recompute — to lock the harness in). No behaviour change. ✅
- **S3 — incremental cascade, v1 conservative invalidation.** ✅ Done.
  `lumen_layout::counters::incremental_precompute_counters` + `RestyleDelta`
  reuse cached `ComputedStyle` outside the dirty root-set; root-set derived by
  `style::restyle_root_set_for_state_change` (interactive-state, ancestor-
  chain-toggle-aware) / `style::restyle_root_set_for_node_change` (DOM
  attribute/class). Gated behind `INCREMENTAL_RESTYLE` (off by default, falls
  back to full recompute). 4 differential tests in `incremental.rs`, two of
  them asserting a real root-set recomputes strictly fewer nodes than a full
  cascade. Measured `precompute_counters` p50 drop: **~54%** for a
  representative sibling-tab hover move, **~1%** for CC-12's own SIDEBAR/None
  toggle fixture (a documented worst case, not a regression — see BUG-341 "S3"
  section for the full numbers and why). Not yet wired into any pipeline
  (`layout_measured_hyp`/`layout_mutation_incremental` still full-recompute
  unconditionally) — that's S5.
- **S4 — incremental box-build.** ✅ Done. `build_box_or_reuse` (wired into
  all 4 `build_box` recursion sites) clones a whole `LayoutBox` subtree from
  `prev` instead of rebuilding it, gated by `CounterMap::clean_subtrees` +
  `RestyleDelta::dom_content_stable` (only safe for pure interactive-state
  deltas — DOM-mutation deltas conservatively rebuild everything, same
  precedent as S3). Public entry: `box_tree::incremental_build_box`, off by
  default via `set_incremental_box_build`. 2 differential tests (laid-out
  geometry comparison, not `Debug` string — `custom_props: HashMap` iteration
  order isn't guaranteed stable across independent cascades) + 1 real-chrome-
  doc test, all passing (bit-identical to a full rebuild). **Measured result is
  negative**: `index_by_node`'s whole-prev-tree hash-index cost outweighs the
  savings from skipping ~8%-baseline-share `build_box` work — see BUG-341 "S4"
  section for the full numbers and the honest recommendation to re-measure
  combined with S3 at S5 before deciding whether to keep it enabled.
- **S5 — wire the chrome pipeline onto the incremental path.** ✅ Done. New
  `lumen_layout::box_tree::layout_mutation_incremental_restyle` combines S3's
  incremental cascade with the existing `graft_geometry` reuse (S4's
  box-build skip left off per its own recommendation).
  `Lumen::relayout_chrome_host` takes this path only when nothing but
  interactive state changed since the last pass — `ChromeModel` equality
  (new `PartialEq` derive) stands in for a `bind_model` mutation-diff (none
  exists yet), guarded further by viewport and Forced Colors Mode stability.
  Measured: CC-12's own SIDEBAR/`None`-toggle fixture shows no improvement
  (expected — S3's documented worst case), a representative sibling-tab
  hover transition shows a real **~25% p50 win** (85→64ms). **CC-12's gate
  stays red** (~40-45× the 2ms budget) — see BUG-341 "S5" for full numbers.
  The page pipeline was *not* wired (no DOM-mutation-diff mechanism exists to
  derive a safe `dirty_roots` for arbitrary JS mutations — same gap as
  `CC12_KEY`, chrome's own keystroke case).
- **S6 — DOM-mutation diff (chrome side).** ✅ Done. `bind_model_tracked`
  (`crates/chrome/src/model.rs`) reports every node whose selector-relevant
  attribute/class actually changed, or whose row-list container gained/lost a
  member, by instrumenting the shared low-level mutation primitives
  (`set_attr`/`remove_attr`/`remove_children_with_class`/`reconcile_row_list`)
  every `bind_*` helper funnels through — no per-function threading needed.
  `relayout_chrome_host` unions `restyle_root_set_for_node_change(doc,
  touched)` into `dirty_roots` and drops the old whole-`ChromeModel`-equality
  gate, so the incremental path now covers content changes (typed omnibox
  text, tab titles), not just interactive state. Fixed a real bug the tracker
  surfaced: `bind_palette` unconditionally rebuilt its empty-state placeholder
  every cycle, permanently widening `dirty_roots`. **Measured**: `CC12_KEY`
  ~30% p50 win (~90ms→~63ms, the first real improvement on this fixture);
  `CC12_HOVER` flat as expected (S6 doesn't touch interactive-state
  derivation). CC-12's gate stays red (~40-50× over budget) — see BUG-341 "S6"
  for full numbers. Page-side JS DOM mutations (`v8_runtime.rs`) were *not*
  wired — no fixture currently exercises that path, and it needs its own
  scoped design (JS mutations go through different call sites entirely).
  Hover fan-out narrowing / selector-dependency caching, and the JS-mutation
  side of this diff, remain open if a further slice is taken.
- **S7 — diff for page-side JS mutations + narrowing hover fan-out.** ✅ Done.
  🟡 **Part 1 done**: `lumen_js::v8_runtime::DomTouched` /
  `V8JsRuntime::take_dom_touched()` — a page-side, V8-only mutation tracker
  mirroring `bind_model_tracked`, instrumenting the 9 attributable native
  primitives (`set_attr`/`remove_attr`, `append_child`/`remove_child`/
  `insert_before`, `set_text_content`/`set_inner_html`,
  `set_style_property`/`delete_style_property`) and marking the other 13
  DOM-mutating natives (Shadow DOM attach, Selection/Range, contenteditable,
  `execCommand`) `unattributed` (forces a conservative full-cascade fallback).
  12 new unit tests, all green. See BUG-341 "S7 (part 1)" for the full
  writeup.
  ✅ **Part 2 done**: `Lumen::try_relayout_raf_incremental` (`shell/src/main.rs`)
  now takes the restyle-aware `layout_mutation_incremental_restyle` path when
  `take_dom_touched()` reports an attributed summary and a matching cascade
  cache exists, falling back to the existing graft-only
  `layout_mutation_incremental` otherwise — same correctness contract as
  chrome's S6. The cache (`Lumen::page_prev_cascade_styles`, an
  `Option<HashMap<NodeId, ComputedStyle>>`) is only ever trusted immediately
  after the one call site that produces a matching one; `apply_relayout_result`
  invalidates it (`None`) unconditionally on every entry, and every other
  page-layout producer that bypasses `apply_relayout_result` (bfcache thaw,
  full page load, streaming layout, hibernate restore) invalidates it
  explicitly too — a stale-cache-vs-fresh-tree mismatch was the exact
  correctness risk part 1 stopped short of. The engine-thread job
  (`make_relayout_job`/`submit_relayout_job`/`readback_relayout_job`/
  `poll_engine_commit`) is deliberately **not** wired yet — it always
  invalidates the cache via the same `apply_relayout_result` sink, so behavior
  there is unchanged (still full cascade), left as a follow-up since crossing
  the thread boundary with `CounterMap`/dirty-roots is its own scoped problem.
  New JS-driven differential test (`lumen-js`,
  `dom_touched_drives_incremental_restyle_matching_full_cascade`): a real V8
  `classList.add` mutation → `take_dom_touched()` → `RestyleDelta` →
  `layout_mutation_incremental_restyle` must match a fresh
  `layout_measured_hyp_with_counters` recompute exactly — passing.
  `cargo test -p lumen-js --features v8-backend` (2523 passed) and
  `cargo test -p lumen-shell` (1704 passed) both green; both crates' clippy
  clean.
  ✅ **Part 3 done**: hover fan-out narrowing.
  `lumen_layout::style::restyle_state_needs_fanout(doc, sheet)` scans every
  selector in `sheet` (top-level rules plus every `@media`/`@layer`/
  `@supports`/`@scope`/`@starting-style`/`@container` block) for a compound
  depending on dynamic interactive state that is followed anywhere on the
  path to the subject by a sibling combinator (`+`/`~`) — the only shape
  reaching outside the flipped node's own subtree. `restyle_root_set_for_
  state_change` takes the result as a new `needs_fanout: bool` parameter:
  `true` keeps S3's widen-to-parent behaviour, `false` narrows each flipped
  node's invalidation to just that node. `:has()` containing a dynamic-state
  pseudo, or any shadow root present, always forces `true` (both directions
  this v1 doesn't model). `assets/chrome/chrome.html` has zero selectors
  needing the wider fanout, so real chrome restyles narrow for real. 16 new
  unit tests (`style::state_fanout_tests`) cover every combinator/pseudo-class
  shape. **Honest measurement**: an A/B on the CC-12 sibling-tab-hover
  fixture (`#sbTabs`, 6 tabs) showed no statistically significant wall-clock
  difference between the old (dirty_roots=1, the shared container) and new
  (dirty_roots=2, just the changed tabs) behaviour — within this machine's
  documented noise floor. `CC12_HOVER`'s own SIDEBAR/`None`-toggle fixture is
  also flat, for the same reason S3 already documented (its ancestor chain's
  cascade cost was already near zero). CC-12's gate stays red. The narrowing
  is correctness-verified and structurally sound (a real page with a wide
  `:hover`-styled sibling list would show a bigger win — no such fixture
  exists to measure), but is not, by itself, what closes CC-12's gate — see
  BUG-341 "S7 (part 3)" for the full numbers. Found and documented (not
  fixed — separate scope) BUG-348 while re-verifying this area: `restyle_
  root_set_for_node_change`'s doc comment incorrectly claimed this engine has
  no `:has()`.

**Stop conditions / honesty:** if after S5 the p95 floor is set by irreducible
per-node cascade cost on the hover root-set and stays > 2 ms, report the number
and re-open the CC-12 budget question with data (2 ms may be the wrong target;
one frame = ~16 ms is the defensible alternative). Do **not** silently relax the
gate.

---

## 6. Verification (mandatory before any incremental-path merge)

The incremental cascade sits on the hottest, most correctness-sensitive path in
the engine. Every incremental-path slice runs, in the foreground:

1. `cargo test -p lumen-layout` (incl. the new `incr == full` differential
   tests) — the `FONT_CH_EX` flaky pair (BUG-339) is the only allowed failure.
2. `cargo test -p lumen-chrome` (identity-preservation tests from the
   `reconcile_row_list` work).
3. `python graphic_tests/run.py --continue-on-fail` — no new regressions vs
   `KNOWN_DEBTORS`.
4. CPU snapshot references (`SAVE_CPU_SNAPSHOTS=… snapshot_cpu`) unchanged, or
   regenerated + eyeballed if paint legitimately shifts (it should not — geometry
   must be identical).
5. `cc12_chrome_perf_gate` re-measured, numbers recorded in BUG-341.
6. The full `/lumen-task-finish` gate (workspace clippy + scoped-test) once, at
   the end.

Anything that changes a single pixel on any page is a correctness failure of the
invalidation set, not a rendering trade-off — incremental output must equal full
output exactly.
