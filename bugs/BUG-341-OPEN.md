# BUG-341: chrome-document relayout costs ~600ms per call — ~300× over CC-12's 2ms perf-gate budget

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs`, `layout_measured_hyp`) — chrome document specifically (`crates/shell/src/main.rs::relayout_chrome_host`, `docs/tasks/p1-css-chrome.md`)
**Найден:** P1, CC-12 (перф-гейт хрома) 2026-07-25 — новый тест `crates/shell/src/main.rs::tests::cc12_chrome_perf_gate_hover_and_keystroke_cycles`

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

## Impact

CC-track chrome rendering is currently opt-in (`LUMEN_CSS_CHROME=1`), not
shipped as the default chrome (CC-14 "Флип дефолта" has not happened) — so
there is no live-user impact yet. But if this cost is representative of
what a real interactive session would pay (the bench mirrors
`relayout_chrome_host`'s real call shape closely: same `bind_model` +
`layout_measured_hyp` + `paint_ordered` sequence, same document), every
hover and keystroke on movement-driven engine-rendered chrome would
currently freeze the UI thread for 500ms+ — this is a **hard blocker for
CC-14** until resolved, and should be treated as high priority before that
slice is attempted.

## Repro

```bash
cargo test -p lumen-shell --profile dev-release cc12_chrome_perf_gate -- --ignored --nocapture
```
