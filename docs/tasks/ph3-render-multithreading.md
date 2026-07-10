# Ph3 — Multithreaded render pipeline (smooth scroll / zoom)

**Developer:** P1 · **Branches:** `p1-mt-m0` … `p1-mt-m4` (one per stage) · **Size:** XL (staged) · **Crates:** `lumen-paint`, `lumen-shell`, `lumen-layout`

Decision record: [ADR-016](../decisions/ADR-016-multithreaded-render-pipeline.md).
User decision 2026-07-09: multithreading is **mandatory and urgent**.

---

## Problem (audit 2026-07-09, branch `zcode`)

Everything — input, JS dispatch, style, layout, display-list build, raster,
present — runs on the single UI/winit thread. Concrete costs, with code refs:

1. **Scroll re-rasterizes the whole display list every frame.** Scroll handlers
   only mutate `scroll_x/scroll_y` + `request_redraw()`
   (`crates/shell/src/main.rs` — `scroll_by_smooth` ~:16088, `advance_momentum`
   ~:16204). The retained `self.display_list` is reused, but the femtovg
   backend then does a full `clear_rect` + re-executes **every** content
   command under `canvas.translate(-scroll)`
   (`crates/engine/paint/src/backends/femtovg_backend.rs::render` ~:3446–3559).
   The whole-frame hash skip (~:3467) never fires while scrolling (scroll is in
   the hash) and the dirty-rect diff sees an identical content list → no
   scissor. Net: scrolling a long page = re-drawing the entire page each frame.
2. **Zoom = full relayout.** `ZoomIn/Out/Reset` (`main.rs` ~:13442) set
   `zoom_factor` and call `relayout()`; zoom is a CSS-viewport shrink
   (`crates/shell/src/zoom.rs:40`), no interim scale transform.
3. **Any main-thread stall freezes presentation.** Momentum, CSS animations,
   GIF and rAF all tick inside `RedrawRequested` on the UI thread. A long JS
   turn or relayout stops the world.
4. **`prev_content` is a full display-list clone every frame**
   (`femtovg_backend.rs` ~:3513).

Dormant infrastructure that this task wires up (do not rebuild from scratch):

| Asset | Where | State |
|---|---|---|
| `ThreadedCompositor` + `CompositorThread` (vsync tick-loop, `commit()` / `flush_pending()`, `VsyncNotifier`) | `crates/engine/paint/src/compositor.rs` (~:400–590) | Built + tested (P2 1B.1/1B.2), **zero shell consumers** |
| `TileGrid` | `crates/engine/paint/src/tile_grid.rs` | Updated on relayout, dirty tiles never read |
| Incremental layout (`DirtyBits`, `lay_out_incremental`) | `crates/engine/layout/src/incremental.rs`, `box_tree.rs` ~:2625 | Implemented, shell always calls full `layout_measured_hyp` |
| `DisplayListCache` (per-node LRU, 32 MB) | `crates/engine/paint/src/display_list_cache.rs` | Populated (whole page under root id), never consumed per-subtree |
| JS runtime on its own thread | ADR-014, `lumen-js` | Done; calls are blocking round-trips, callable from any thread |

Related: BUG-171 (off-UI-thread load pipeline) is stage M2 of this plan.
BUG-274 (wgpu idle CPU ×4, memory spike) blocks making wgpu the threaded
backend default — M1 ships on femtovg.

---

## Invariants (from ADR-016 — every stage must preserve them)

1. Cross-thread data = immutable snapshots (`Arc<DisplayList>`, `Arc<PropertyTrees>`); no shared mutable state, no locks held across raster/layout.
2. Latest-wins commits, queue depth 1, coalescing — slow consumers drop stale frames, never queue them.
3. Scroll/zoom are small copyable values applied render-side as a transform.
4. Render thread never waits for the engine; engine never waits for the render thread (exception: explicit request/reply readback).
5. Scroll never waits for raster — missing tiles show a placeholder (checkerboarding allowed), filled on a later frame.
6. Idle = parked on condvar, no polling wakeups (preserve BUG-271's ~0% idle CPU; the current `CompositorThread` 16.67 ms idle tick must become "sleep indefinitely unless animations active or commit pending").

---

## Stages

Each stage is independently shippable and measurable. Do them in order.

### M0 — shrink per-frame work + make it measurable (prerequisite, S–M)

No threads yet; reduces the work every later stage will move/parallelize.

- **M0.1 Frame-time histogram.** ✅ (branch `p1-mt-m0`). `FrameStats`/
  `FrameSummary` in `lumen-paint` (`lib.rs`, nearest-rank percentiles, 5 unit
  tests) accumulate frame ms; the shell records each `[frame]` time and prints
  `FRAME_SUMMARY count/min/p50/p95/p99/max` on the `LUMEN_MEM_REPORT` cadence
  and once from `ApplicationHandler::exiting`. Every later stage cites
  before/after numbers.
- **M0.2 Viewport culling.** Give display commands (or a prepass index)
  bounding rects; skip commands fully outside `viewport ∪ slop` during
  execution in both femtovg and wgpu backends. Expected: scroll frame cost on
  long pages drops by the off-screen share (audit pages: 90%+).
- **M0.3 Transform-first zoom.** Ctrl+/- applies an immediate scale transform
  to the retained display list (femtovg `canvas.scale`); full relayout is
  debounced ~150–200 ms after the last zoom step. Pinch/anim zoom follows the
  same path.
- **M0.4 Kill the per-frame `prev_content` clone.** Keep `Arc<DisplayList>`
  (or double-buffer swap) for the dirty-rect diff.
- **M0.5 Content hash excludes scroll.** Hash content and offset separately so
  the identical-frame skip can distinguish "same content, new offset" (becomes
  the blit fast-path trigger in M3).

### M1 — render thread (the core of this task, M–L)

Move the render backend + present off the main thread; reuse
`CompositorThread`/`VsyncNotifier`.

- **Ownership:** the render thread creates and exclusively owns the GL context
  and femtovg `Canvas` (`!Send` — it must be *created* on that thread, not
  moved). Main thread keeps the winit window; resize/scale-factor/DPI events
  are forwarded as messages.
- **Commit protocol:** main (later: engine thread) sends
  `Commit { content: Arc<DisplayList>, overlay: Arc<DisplayList>, scroll: (f32,f32), zoom: f32, viewport, commit_id }`
  via a latest-wins slot (Mutex<Option<Commit>> + the existing
  `VsyncNotifier`; no new dependency needed — add `crossbeam-channel` only if
  profiling shows the slot contended, with the dependency-policy justification).
- **Scroll/momentum move render-side:** wheel input on main computes the
  delta/velocity and forwards it; the render thread owns scroll position,
  easing and momentum ticks (`scroll_anim.rs` / `momentum_anim.rs` logic
  relocates). Result: momentum and compositor-offload animations keep running
  at vsync even while main is busy. (Full input independence — events arriving
  while main is blocked — lands with M2; be honest about this in the bug/task
  notes.)
- **Scheduling:** on-demand, not free-running. The thread parks until
  `notify()` (new commit / input forward) or an active-animation deadline.
  Never poll at 16 ms while idle (invariant 6).
- **Synchronous readback:** `--screenshot`, IPC acceptance (`run.py --ipc`),
  CDP `Page.captureScreenshot` become `Request::Readback { reply: SyncSender }`
  messages. Audit all `screenshot_*` call sites in the shell before starting.
- **Scope control:** chrome/overlay UI stays built on main and ships as the
  overlay snapshot — no chrome logic moves in M1.
- **Acceptance:** with a test page running a 200 ms JS busy-loop per rAF,
  momentum scroll started before the stall continues at 60 fps (frame log
  proves presentation continued); graphic tests green; idle CPU unchanged
  (BUG-271 check); no new `unsafe`.

### M2 — engine off main (= BUG-171, L)

The heavy `LoadDone` pipeline (fetch → parse → style → layout → display-list
build) runs on a background engine thread and commits snapshots. JS is already
thread-safe to call (ADR-014). Main thread shrinks to: OS events, chrome state,
input forwarding. This is what makes *input* independent of engine stalls.
Details of the load pipeline belong to the existing BUG-171 notes; this stage
must land after M1 so commits have somewhere to go.

### M3 — tiles + blit scroll (`TileGrid` revival, L)

- Content renders into pooled tile textures (reuse `layer_pool` texture-pool
  experience from BUG-272); scroll = blit of ready tiles at the new offset +
  raster of newly exposed bands only.
- Raster workers: start with **one** raster thread feeding the render thread;
  widen to a pool only with profiling evidence.
- Checkerboarding invariant: a missing tile draws the page background, never
  blocks the frame.
- M0.5's content-hash split is the trigger: "same content, new offset" →
  pure blit path.

### M4 — parallel style/layout (M, gated on incremental layout)

- First wire `lay_out_incremental` + `DirtyBits` into the live shell path for
  JS-driven mutations (today: full-tree layout always).
- Then rayon over independent dirty subtrees / selector matching. Bringing
  `rayon` in requires the dependency-policy justification block in the commit.
  Do not parallelize the full-tree pass — incrementality first, parallelism
  second.

---

## Risks / gotchas

- **GL context threading:** glutin context must be made current only on the
  render thread; create surface+canvas there. Some drivers dislike context
  creation off the window's thread — spike this first on the Windows/ANGLE and
  native-GL paths before committing to the M1 design.
- **winit is main-thread-only** on Windows/macOS — never move the event loop;
  move the backend instead.
- **`Lumen` struct is a 17k-line monolith** (`main.rs`). M1 must extract only
  the backend-owning boundary (a `RenderHandle`), not refactor the shell.
  Resist scope creep.
- **wgpu backend (BUG-274)** stays off the threaded default until fixed; the
  M1 render loop is where the fixed wgpu/vello backend will later slot in
  (ADR-010 phase 3).
- **Frame logs across threads:** carry `commit_id` + thread tag in
  `LUMEN_FRAME_LOG` lines or debugging becomes guesswork.
- **content-visibility expansion** (`maybe_expand_cv_relevant`, `main.rs`
  ~:16157) is a scroll→relayout backchannel; in M1+ it becomes a message from
  render thread (visible-range changed) to the engine side — do not let the
  render thread call layout.

## Doc-sync on landing each stage

`CAPABILITIES.md` (rendering section), `subsystems/paint.md` (+`shell.md`),
`ROADMAP.md` rows for M0–M4 (+ `python scripts/gen_roadmap.py`), this file's
Status line, and close BUG-171 at M2. ROADMAP rows must be added when this
lands on `main` (main is locked by another session as of 2026-07-09).
