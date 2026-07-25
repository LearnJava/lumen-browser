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
- **S4 — incremental box-build.** Skip `build_box` for undamaged subtrees.
  Measure `build_box` share drop.
- **S5 — wire chrome + page pipelines onto the incremental path**, flag on.
  Re-measure CC-12. If green → CC-14 unblocks. Full verification (§6).
- **S6 — tighten invalidation** if S5 misses budget (narrow the hover fan-out,
  cache selector-dependency indices).

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
