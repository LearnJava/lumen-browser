# P2-viewtrans — View Transitions Level 1 (full: named groups, morph, ::view-transition* pseudo)

**Developer:** P1 (with P4 handoff for the CSS pseudo-element selectors)
**Branch:** `p1-p2-view-transitions-full`
**Size:** XL
**Crates:** `lumen-js`, `lumen-shell`, `lumen-engine` (paint/layout), `lumen-css-parser` (P4 handoff)

## Progress

- **Phase 1 — Per-element snapshot infrastructure (paint + layout) — DONE 2026-06-25** (branch `p1-laguna-t1-120955`):
  - paint: `RenderBackend::register_snapshot(id, &Image)` + `clear_snapshots()` added to the trait (default no-op). femtovg backend (default) uploads straight-alpha RGBA into `self.snapshots` (replacing-id frees the old texture); wgpu backend forwards to `Renderer::upload_layer_snapshot`. `DrawLayerSnapshot` now has a public insertion path on the default backend. 1 trait-default test.
  - layout: `collect_view_transition_groups(root) -> Vec<(NodeId, Box<str>, Rect)>` (name + border-box rect) at `crates/engine/layout/src/lib.rs`. 3 unit tests. `// CSS:` handoff comment at the fn; P4 pointer `crates/engine/css-parser/src/parser.rs:345` added to `STATUS-P4.md`.
- **Remaining:** Phase 2 (named capture + pairing in shell), Phase 3 (per-element morph), Phase 4 (author timing via P4 pseudo tree), Phase 5 (skipTransition/Cancel robustness). The shell still uses the single whole-frame root cross-fade.

## Goal

Root cross-fade already works end-to-end (F2-4, ✅ 2026-06-22): `document.startViewTransition(cb)` returns a correct `ViewTransition` object, the shell snapshots the old display list on `Begin` and cross-fades it over the relaid-out page on `End`. This task is the **optional remainder of full Level 1**, NOT validated by TEST-61: (1) **named transition groups** keyed by `view-transition-name`, (2) **per-element morph** — animating each named element from its old box geometry/snapshot to its new one, and (3) wiring the **`::view-transition*` pseudo-element tree** so author rules (`animation-duration`, `animation-timing-function`, custom keyframes) drive the per-group animation. This is a large feature spanning JS, layout snapshotting, paint, and CSS selector matching (the pseudo-element tree is a P4 handoff). Ship it incrementally — each phase below is independently shippable and visually checkable.

## Current state

What works today:
- **JS object** — [`crates/js/src/view_transitions.rs:38`](../../crates/js/src/view_transitions.rs) (`VIEW_TRANSITION_SHIM`): `document.startViewTransition(callback)` runs the callback synchronously, fires `_lumen_vt_begin` / `_lumen_vt_end` / `_lumen_vt_cancel`, and returns `{ updateCallbackDone, ready, finished, skipTransition() }` with pre-resolved/rejected promises. Native bindings registered in `install_view_transition_bindings` ([`view_transitions.rs:90`](../../crates/js/src/view_transitions.rs)) push `ViewTransitionEvent::{Begin,End,Cancel}` ([`view_transitions.rs:19`](../../crates/js/src/view_transitions.rs)) onto a shared queue.
- **Shell drain + cross-fade** — [`crates/shell/src/main.rs:7780`](../../crates/shell/src/main.rs) drains the events in `about_to_wait`: `Begin` clones the current `display_list` into `ViewTransitionState.old_dl` ([`main.rs:7784`](../../crates/shell/src/main.rs)); `End` records `start_ms`, relayouts, requests redraw; `Cancel` drops the state. `ViewTransitionState` ([`main.rs:5890`](../../crates/shell/src/main.rs)) holds `old_dl`, `start_ms`, `duration_ms` (hardcoded **300 ms**).
- **Compositing** — [`main.rs:10207`](../../crates/shell/src/main.rs): each frame computes `progress = elapsed / duration_ms`, wraps `old_dl` in `PushOpacity { 1 - progress } … PopOpacity`, prepends it to the overlay buffer (renders under UI panels, over new page), clears the state at `elapsed >= duration_ms`.
- **CSS property already parsed (P4 done)** — `view-transition-name` is a recognized property: registered in [`crates/engine/css-parser/src/lib.rs:350`](../../crates/engine/css-parser/src/lib.rs); `ComputedStyle.view_transition_name: Option<Box<str>>` at [`crates/engine/layout/src/style.rs:3006`](../../crates/engine/layout/src/style.rs); parsed in `apply_declaration` at [`style.rs:12434`](../../crates/engine/layout/src/style.rs) (`none` → `None`, else `<custom-ident>`); non-inherited ([`style.rs:15268`](../../crates/engine/layout/src/style.rs)). BUG-130 confirmed it does not perturb normal-flow rendering.
- **Layout collector already exists** — `collect_view_transition_names(root) -> Vec<(NodeId, Box<str>)>` at [`crates/engine/layout/src/lib.rs:1268`](../../crates/engine/layout/src/lib.rs), one pair per named element in document order, skips `display:none`. **Currently unused by the shell** — this is the hook the morph engine consumes.
- **Per-element snapshot paint primitive exists** — `DisplayCommand::DrawLayerSnapshot { id: u64, rect: Rect, alpha: f32 }` at [`crates/engine/paint/src/display_list.rs:557`](../../crates/engine/paint/src/display_list.rs). Implemented in the wgpu renderer ([`renderer.rs:4822`](../../crates/engine/paint/src/renderer.rs), registered via `layer_snapshots.insert` at [`renderer.rs:3445`](../../crates/engine/paint/src/renderer.rs)) and in femtovg ([`femtovg_backend.rs:2231`](../../crates/engine/paint/src/backends/femtovg_backend.rs), reading `self.snapshots: HashMap<u64, ImageId>` at [`femtovg_backend.rs:330`](../../crates/engine/paint/src/backends/femtovg_backend.rs)).

What's missing:
- The shell uses a **single whole-frame** `old_dl`; it never calls `collect_view_transition_names`, never captures **per-element** snapshots, never matches old↔new boxes by name.
- No **morph**: named elements do not animate from old geometry to new geometry — they hard-cut while the whole old frame cross-fades.
- The **`::view-transition*` pseudo-element tree** (`::view-transition`, `::view-transition-group(name)`, `::view-transition-image-pair(name)`, `::view-transition-old(name)`, `::view-transition-new(name)`) is **not represented anywhere**. The css-parser pseudo-element enum `PseudoElementKind` ([`crates/engine/css-parser/src/parser.rs:345`](../../crates/engine/css-parser/src/parser.rs)) has no variants for them, so author rules targeting them fall into `PseudoElementKind::Unknown` and are dropped by `pseudo_element_is_supported` ([`parser.rs:546`](../../crates/engine/css-parser/src/parser.rs)).
- Timing is hardcoded (300 ms); author `animation-duration` / `animation-timing-function` on `::view-transition-group(*)` is ignored.
- **femtovg snapshot gap:** the femtovg backend reads `self.snapshots` but exposes no public insertion path (only the wgpu renderer registers snapshots). Since the shell renders through the **femtovg backend by default** (see project memory `project_femtovg_default_backend`), per-element snapshots must be wired into femtovg too, not only wgpu.

## Architecture / pipeline

Full L1 pipeline (spec: CSS View Transitions L1 §4 "Algorithms", §10 "view-transition-name"):

1. **Capture old (on `Begin`)** — `lumen-shell` + `lumen-layout` + `lumen-paint`
   - Call `collect_view_transition_names(&layout_root)` to get `[(NodeId, name)]`.
   - For each named element: record its old **border-box rect** (from its `LayoutBox`) and render its subtree into an offscreen snapshot image → register as a `DrawLayerSnapshot` id (the "old image"). Also keep the existing whole-frame `old_dl` as the fallback root cross-fade.
2. **Run callback** — JS (`lumen-js`) — already done; DOM mutated synchronously inside the user callback.
3. **Relayout + capture new (on `End`)** — `lumen-shell` + `lumen-layout`
   - `relayout()` (already called). Re-run `collect_view_transition_names` on the new tree → new rects, and snapshot each new named subtree (the "new image").
4. **Pair + build group state** — `lumen-shell`
   - Match old and new entries by `name`. Three cases: matched (both old+new → morph), only-old (exit), only-new (entry). Build a `ViewTransitionGroup { name, old_rect, new_rect, old_snap_id, new_snap_id, duration_ms, timing }` per name.
5. **Animate groups** — `lumen-shell` (driven by the existing per-frame redraw loop, analogous to `animation_scheduler`)
   - Each frame: `t = ease(progress)`. Interpolate each group's rect `old_rect → new_rect`; emit `DrawLayerSnapshot { old_snap, alpha = 1-t, rect = lerp }` and `DrawLayerSnapshot { new_snap, alpha = t, rect = lerp }`. The non-named remainder of the page keeps the root whole-frame cross-fade.
6. **Pseudo-element tree + author timing** — `lumen-css-parser` (P4) + `lumen-shell`
   - Parse `::view-transition*` selectors into real `PseudoElementKind` variants so author rules cascade onto synthetic group boxes; the shell reads `animation-duration` / `animation-timing-function` (and ideally custom `@keyframes`) from the matched pseudo-element `ComputedStyle` to drive step 5 instead of the hardcoded 300 ms.

Crate map: JS already complete; **layout** provides the name collector + per-element rects; **paint** provides `DrawLayerSnapshot` (extend femtovg registration); **shell** owns capture/pairing/animation orchestration; **css-parser (P4)** owns the pseudo-element selectors and their cascade.

## Entry points

- [`crates/js/src/view_transitions.rs:38`](../../crates/js/src/view_transitions.rs) — `VIEW_TRANSITION_SHIM`, the `startViewTransition` JS object. Per-element promise resolution / `skipTransition()` becoming a real abort would be wired here if needed.
- [`crates/js/src/view_transitions.rs:90`](../../crates/js/src/view_transitions.rs) — `install_view_transition_bindings`, native `_lumen_vt_*` bindings + `ViewTransitionEvent` queue.
- [`crates/shell/src/main.rs:7780`](../../crates/shell/src/main.rs) — event drain (`Begin`/`End`/`Cancel`); where capture of named snapshots and pairing must be added.
- [`crates/shell/src/main.rs:5890`](../../crates/shell/src/main.rs) — `ViewTransitionState`; extend with a `Vec<ViewTransitionGroup>` and per-group timing.
- [`crates/shell/src/main.rs:10207`](../../crates/shell/src/main.rs) — per-frame compositing; where per-group `DrawLayerSnapshot` lerp emission goes alongside the existing root cross-fade.
- [`crates/engine/layout/src/lib.rs:1268`](../../crates/engine/layout/src/lib.rs) — `collect_view_transition_names`; consume it; may need a sibling that also returns the border-box rect per name.
- [`crates/engine/paint/src/display_list.rs:557`](../../crates/engine/paint/src/display_list.rs) — `DrawLayerSnapshot` command.
- [`crates/engine/paint/src/backends/femtovg_backend.rs:2231`](../../crates/engine/paint/src/backends/femtovg_backend.rs) — femtovg `DrawLayerSnapshot` (default backend); needs a public snapshot-registration path (`self.snapshots` insertion at [`femtovg_backend.rs:330`](../../crates/engine/paint/src/backends/femtovg_backend.rs)).
- [`crates/engine/paint/src/renderer.rs:3445`](../../crates/engine/paint/src/renderer.rs) — wgpu snapshot registration (reference for the femtovg path).
- [`crates/shell/src/animation_scheduler.rs:116`](../../crates/shell/src/animation_scheduler.rs) — `AnimationScheduler` (timing-function evaluation, `tick` at line 133); reuse its easing helpers for group interpolation.

## Cross-team boundary (P4)

The CSS **property** `view-transition-name` is already wired by P4 (parsing + `ComputedStyle` + cascade, see Current state). What remains for P4 is the **`::view-transition*` pseudo-element selectors**:

`::view-transition`, `::view-transition-group(<name>|*)`, `::view-transition-image-pair(<name>)`, `::view-transition-old(<name>)`, `::view-transition-new(<name>)`.

These must become real variants of `PseudoElementKind` ([`crates/engine/css-parser/src/parser.rs:345`](../../crates/engine/css-parser/src/parser.rs)) and be accepted by `pseudo_element_is_supported` ([`parser.rs:546`](../../crates/engine/css-parser/src/parser.rs)) and the functional-pseudo parser (`parse_functional_pseudo_element`, [`parser.rs:3962`](../../crates/engine/css-parser/src/parser.rs)), so author rules cascade onto the synthetic group tree.

P1 marks the handoff at the shell call site with a comment:
```rust
// CSS: ::view-transition / ::view-transition-group(name) selectors —
// P4 to add PseudoElementKind variants + functional parsing so author
// animation-duration / animation-timing-function can target group pseudos.
```
and adds a `crates/...:line` pointer in `STATUS-P4.md`. Until P4 lands the selectors, P1 hardcodes/derives the per-group duration from the existing 300 ms default (already the behaviour) and reads author timing once the pseudo cascade exists. P1 does **not** add the pseudo-element variants itself (per CLAUDE.md CSS ownership).

## Steps

Phase 1 — Per-element snapshot infrastructure (paint + layout) [S]
1. Add a public snapshot-registration API to the femtovg backend so the shell can insert an offscreen-rendered image under a `u64` id (mirror wgpu `layer_snapshots.insert`). Confirm `DrawLayerSnapshot` renders that id at an arbitrary `rect`/`alpha`.
2. In layout, add a helper returning `(NodeId, name, border_box_rect)` (extend or wrap `collect_view_transition_names`).

Phase 2 — Named capture + pairing (shell) [M]
3. On `Begin`: call the layout collector, render each named subtree to an offscreen image, register snapshot id, store `{ name, old_rect, old_snap_id }`.
4. On `End`: after `relayout()`, re-collect, snapshot new named subtrees, **pair by name**, populate `Vec<ViewTransitionGroup>` in `ViewTransitionState`. Enforce per-name uniqueness (first occurrence wins, per the collector doc).

Phase 3 — Per-element morph (shell paint) [M]
5. In the per-frame compositing block, for each group emit two `DrawLayerSnapshot`s with rect lerped `old_rect → new_rect` and alphas `1-t` / `t` (entry: only-new fades/scales in; exit: only-old fades/scales out). Keep the existing root whole-frame cross-fade for the un-named remainder. Use easing from `animation_scheduler`.

Phase 4 — Author timing via pseudo tree (P4 handoff + shell) [M]
6. File the P4 handoff (pseudo-element selectors). Once landed, resolve `ComputedStyle` for the matched `::view-transition-group(name)` and read `animation-duration` / `animation-timing-function` (and custom keyframes if feasible) to override the 300 ms default per group.

Phase 5 — Robustness [S]
7. Handle `skipTransition()` as a real abort (clear groups, jump to end state) and `Cancel` mid-animation. Resolve `finished` only when the animation truly completes (currently pre-resolved). Optional, only if time allows.

## Tests / verification

- **TEST-61 does NOT validate this** (project memory `project_test61_view_transitions_debtor`): TEST-61's earlier 99.53% was a blank-gdigrab artifact; the real `--ipc` ~10.71% diff is Edge async-callback timing + text noise, the same class as TEST-71/77. The full L1 morph path is **not** exercised by that test — do not treat TEST-61 green/red as a signal for this work, and do not chase it.
- **Unit tests (layout):** extend the existing `collect_view_transition_names` tests ([`crates/engine/layout/src/lib.rs:16837`](../../crates/engine/layout/src/lib.rs)) for the new rect-returning helper (matched/only-old/only-new, duplicate-name dedup).
- **Unit tests (paint):** `DrawLayerSnapshot` already has serialization tests (`crates/engine/paint/tests/snapshot_tests.rs:148`); add femtovg registration round-trip coverage.
- **Manual visual check (the real gate):** a dedicated `graphic_tests/`-style page (or `samples/`) with two named elements (`view-transition-name: hero` on an element that changes size/position across the callback) loaded via `--ipc`; step the animation and confirm the hero morphs (scales/translates between old and new box) rather than hard-cutting while the rest cross-fades. CPU-snapshot path (`--screenshot`) does not run JS, so use the `--ipc` window path for any morph check.
- Per-crate gate before merge: `cargo clippy -p lumen-shell -p lumen-paint -p lumen-layout --all-targets -- -D warnings`, then `cargo test -p <crate>` for each touched crate.

## Definition of done

- [x] femtovg backend exposes a public snapshot-registration API; `DrawLayerSnapshot` renders shell-registered images (default backend works, not just wgpu). *(Phase 1, 2026-06-25)*
- [ ] Shell captures **per-element** old + new snapshots for every `view-transition-name`, paired by name (matched / only-old / only-new handled).
- [ ] Matched named elements **morph** (rect lerp old→new, cross-faded snapshots); un-named remainder still uses the root whole-frame cross-fade.
- [ ] Group easing uses real timing-function evaluation (shared with `animation_scheduler`), not a linear/hardcoded curve.
- [x] `// CSS:` handoff comment placed (at the layout collector, the morph engine's input — shell call site lands in Phase 2/3) and a `crates/...:line` pointer added in `STATUS-P4.md` for the `::view-transition*` selectors. *(Phase 1, 2026-06-25)*
- [ ] (If P4 selectors land in time) per-group `animation-duration` / `animation-timing-function` from the pseudo cascade override the 300 ms default.
- [ ] Layout + paint unit tests added/extended; clippy clean and tests pass for `lumen-shell`, `lumen-paint`, `lumen-layout`.
- [ ] Manual `--ipc` visual check confirms a named element morphs across the callback.
- [ ] `CAPABILITIES.md` View Transitions row updated; `subsystems/` notes appended in the same commit.
- [ ] Reminder: TEST-61 is a known debtor and is **not** the acceptance signal for this feature.
