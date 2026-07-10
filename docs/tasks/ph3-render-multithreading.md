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
- **M0.2 Viewport culling.** ✅ (branch `p1-mt-m0-2`).
  [`DisplayCommand::cull_rect`](../../crates/engine/paint/src/display_list.rs)
  returns the document-space AABB of every self-contained leaf draw (fills,
  text, images, gradients, borders, outline grown by offset+width, SVG
  geometry, scrollbar) and `None` for all structural `Push*`/`Pop*` (which must
  never be culled). Both backends map that box through the current CTM (scroll +
  nested transforms) and skip it when its transformed AABB is fully outside the
  viewport expanded by a 256 CSS px slop: femtovg via `is_command_culled` at the
  top of `render_command` (offscreen clip/filter/mask layers are full-surface
  FBOs with an unchanged transform, so culling stays valid inside them); wgpu
  via `leaf_is_offscreen` at the top of the `render` command loop (3D/perspective
  transforms disable culling — conservative). AABB-of-transformed-corners is a
  superset under rotation/scale, so no visible pixel is ever dropped. The
  femtovg `[frame]` log now reports `culled N/M leaf`. 4 unit tests on
  `cull_rect`. Expected: scroll frame cost on long pages drops by the off-screen
  share (audit pages: 90%+).
- **M0.3 Transform-first zoom.** ✅ (branch `p1-mt-m0-3`). Ctrl+/-/0 now updates
  `zoom_factor` and calls `App::begin_zoom_preview` instead of relayouting: the
  backend scales the retained display list by
  `zoom::preview_scale(zoom_factor, laid_out_zoom_factor)` via the new
  `RenderBackend::set_preview_scale` (femtovg applies `canvas.scale` before the
  scroll translate, so a doc point maps to `s·(p − scroll)` — scaled about the
  viewport top-left; culling stays correct because `is_command_culled` maps AABBs
  through the live CTM, which now includes `s`). A debounce
  (`ZOOM_RELAYOUT_DEBOUNCE_MS` = 180 ms) armed on each press folds into the
  `about_to_wait` `WaitUntil` deadline; when it elapses `relayout()` runs once,
  re-syncing `laid_out_zoom_factor = zoom_factor` and resetting the preview to
  1:1 — so a burst of key presses reflows only once. Pinch/anim zoom will follow
  the same `set_preview_scale` path. Unit tests: `preview_scale` identity/ratio/
  degenerate-guard in `zoom.rs`.
- **M0.4 Kill the per-frame display-list clone.** ✅ (branch `p1-mt-m0-4`). The
  original `prev_content` clone from the audit (femtovg dirty-rect diff) was
  already removed in the M0.2 render rewrite. The remaining per-frame full-list
  clone was in the shell: every `RedrawRequested` copied the whole display list
  into a `PushTransform(translate(page_offset))`-wrapped buffer just to shift the
  page below the tab bar / right of a docked sidebar — an O(n) deep clone of
  every `DisplayCommand` on each momentum-scroll frame. Replaced by a render-side
  `RenderBackend::set_page_offset` (+ `supports_page_offset` capability query,
  default no-op/`false` — same pattern as M0.3's `set_preview_scale`): femtovg
  applies the fixed offset as a `translate` right after the scroll translate
  (CTM `scale · translate(-scroll) · translate(offset)` and sticky/zoom behavior
  unchanged), so the shell renders the display list **by reference**. Backends
  that don't support it (wgpu/vello/cpu window path) keep the old
  `PushTransform`-wrapper path — no regression, no extra memory. The wrapper also
  survives for the rare devtools-inspector-overlay frame (it must ride inside the
  page transform). Trait-default contract test in `backend.rs`.
- **M0.5 Content hash excludes scroll.** ✅ (branch `p1-mt-m0-5`). New
  `lumen_paint::hash_content(content, surface_w, surface_h)` folds **only** the
  page-content commands + surface size into the hash (scroll and the fixed page
  offset excluded), reusing the allocation-free `HashFmt` Debug-streaming of
  `hash_display_list`. `FrameFingerprint { content_hash, scroll, offset }` pairs
  that hash with the two offsets kept as raw copyable values, and
  `FrameFingerprint::delta_from` classifies a frame as `FrameDelta::Identical` /
  `OffsetOnly` (the M3 blit trigger — same content, new offset) / `ContentChanged`
  (content edit or resize wins). Overlay commands are deliberately excluded from
  the hash: the scrollbar thumb is rebuilt from `scroll_y` every frame and would
  otherwise make every scroll look like a content change. The shell records the
  previous frame's fingerprint (`last_frame_fp`) and, **only under
  `LUMEN_FRAME_LOG`**, logs `[frame] delta {Identical|OffsetOnly|ContentChanged}`
  so the scroll-vs-content frame mix is measurable before M3 acts on it; normal
  runs pay nothing. No skip/blit yet — that is M3. 4 unit tests in
  `display_list.rs` (content-hash excludes scroll, offset-only on scroll/dock,
  identical, content-change-wins).

### M1 — render thread (the core of this task, M–L)

Move the render backend + present off the main thread; reuse
`CompositorThread`/`VsyncNotifier`.

Sub-sliced like M0 (each independently shippable into `zcode`):

- **M1.1 — threaded backend infra + GL-threading spike.** ✅ (branch
  `p1-mt-m1`, merged into `zcode`). `ThreadedRenderBackend`
  (`crates/shell/src/render_thread.rs`) implements `RenderBackend` and proxies
  to a real backend on a dedicated `lumen-render` thread: ordered control
  channel + latest-wins frame coalescing (drain-per-batch, only the newest
  `Frame` renders), on-demand parking via blocking `recv()` (invariant 6), and
  a startup handshake caching `supports_page_offset` / initial scale+viewport so
  the proxy answers those synchronously. Enabled behind `LUMEN_RENDER_THREAD=1`
  (default off); the whole cutover is one factory branch —
  **zero changes to the 12k-line `RedrawRequested` block** (the trait *is* the
  boundary). Key discovery — the screenshot/IPC/CDP readback paths are all CPU
  (`render_to_image_cpu`), never the windowed GL backend, so **M1 needs no GL
  readback** (the brief's "Synchronous readback" bullet below is moot). 4 unit
  tests on the coalescing rule. **Spike result (Windows):** creating the backend
  *on* the render thread fails — winit exposes the Win32 window handle only on
  the thread that created the window (`the underlying handle is not available`);
  the proxy then falls back to in-process with no regression. So the Ownership
  bullet below is corrected: the context must be **created on main and handed
  off**, not created on the thread.
- **M1.2 — GL-context handoff.** ✅ (branch `p1-mt-m1-2`, merged into `zcode`).
  `FemtovgBackend` now stores its glutin context as a two-state enum
  (`GlContextState::{Current(PossiblyCurrentContext), NotCurrent(NotCurrentContext)}`
  in an `Option`, since both glutin transitions consume by value) plus inherent
  `detach_gl_context` (`make_not_current`) / `attach_gl_context`
  (`make_current`, idempotent). `backend_factory::create_threaded_femtovg`
  builds the backend **on main** (window handle valid there — M1.1 spike),
  detaches the context, then moves the concrete `FemtovgBackend` (`Send` via its
  manual `unsafe impl`) into the `ThreadedRenderBackend` ctor closure, which
  `attach_gl_context`es on the render thread and drives present there — no
  `RenderBackend: Send` supertrait needed. `swap_buffers`/`resize` go through a
  `current_ctx()` accessor that errors if the context is detached (used off the
  owning thread before attach). Any failure (create/detach/handshake) falls back
  to the single-threaded in-process path with no regression. **Verified
  (Windows, `LUMEN_RENDER_THREAD=1`):** `[frame] paint … swap` succeeds with zero
  fallback/`make_current` errors — present now runs off the UI thread. The
  existing `femtovg_backend_is_send` test guards the `Send` invariant the handoff
  relies on. Next: measure acceptance (200 ms JS busy-loop keeps momentum at
  60 fps) and consider making the render thread default.
- **M1.3 — render-side momentum.** ✅ (branch `p1-mt-m1-3`, merged into `zcode`).
  With M1.2 present ran off the UI thread, but momentum still *froze* on a main
  stall because main produces the frames. M1.3 hands momentum ownership to the
  render thread. Two new no-op-default `RenderBackend` methods
  (`start_render_momentum { vel_y, vel_x, max_scroll_y, max_scroll_x }` /
  `stop_render_momentum`) — the single-threaded path ignores them, so
  `LUMEN_RENDER_THREAD` off is unchanged. The shell forwards `start` on
  `TouchPhase::Ended` (with the scroll extents) and `stop` on new gesture / wheel
  / navigation / natural end, while keeping its own `advance_momentum`
  (authoritative `window.scrollY`). `ThreadedRenderBackend` retains the last
  committed frame (`RenderState`) and, when momentum is active, waits on
  `recv_timeout(MOMENTUM_TICK≈16 ms)` instead of blocking `recv()`: a timeout
  means main sent nothing this vsync → it stalled → the render thread self-ticks,
  recomputing scroll from the last anchor and re-presenting the retained frame.
  While main is alive its frames (latest-wins) keep driving and re-anchor
  momentum; self-tick fires **only** on starvation, so no double-render in the
  common case and invariant 6 (park on `recv()` while idle) holds when no
  momentum is active. Momentum physics is deterministic and cadence-free via new
  stateless `momentum_anim::velocity_at` / `displacement_since`, so UI- and
  render-side never drift. Unit tests: 4 on the stateless helpers (match the
  stateful `advance` to <0.5 %), 5 on `momentum_scroll_at` (advance / clamp top &
  bottom / done-on-decay / continue-from-anchor). Full input independence
  (events arriving *during* a stall) remains M2. Acceptance (interactive 200 ms
  busy-loop) to be smoke-verified via `LUMEN_FRAME_LOG` before flipping default.
- **M1.4 — cross-thread frame-log tags.** ✅ (branch `p1-mt-m1-log`, merged into
  `zcode`). Instrumentation prerequisite for the M1 acceptance step and M2
  debugging (closes the "Frame logs across threads" gotcha below). A third
  no-op-default `RenderBackend` method `set_frame_commit_id(commit_id, self_tick)`
  is called by the render thread before each present; the femtovg backend appends
  `[thr <name> commit <id>[ self-tick]]` to its `[frame] paint …` line
  (`std::thread::current().name()` → `lumen-render` off the UI thread, `main`
  otherwise). Self-tick presents (momentum continuing *while main is stalled*,
  M1.3) are now visible in the log with the retained frame's commit id — this is
  the "frame log proves presentation continued" evidence the acceptance needs.
  Single-threaded path (`LUMEN_RENDER_THREAD` off) never sets it → line unchanged
  bar the `[thr main]` tag. No new deps, no `unsafe`.

- **Ownership:** ~~the render thread creates and exclusively owns~~ **[M1.1 spike
  correction]** — on Windows the GL context/`Canvas` **cannot** be created on
  the render thread (window handle unavailable off the main thread). It must be
  created on main and the context handed off (`make_not_current`→move→
  `make_current`). The render thread then exclusively owns and drives it. Main
  thread keeps the winit window; resize/scale-factor/DPI events are forwarded as
  messages.
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
  `LUMEN_FRAME_LOG` lines or debugging becomes guesswork. ✅ done in M1.4:
  `[frame] paint …` lines end with `[thr <name> commit <id>[ self-tick]]`.
- **content-visibility expansion** (`maybe_expand_cv_relevant`, `main.rs`
  ~:16157) is a scroll→relayout backchannel; in M1+ it becomes a message from
  render thread (visible-range changed) to the engine side — do not let the
  render thread call layout.

## Doc-sync on landing each stage

`CAPABILITIES.md` (rendering section), `subsystems/paint.md` (+`shell.md`),
`ROADMAP.md` rows for M0–M4 (+ `python scripts/gen_roadmap.py`), this file's
Status line, and close BUG-171 at M2. ROADMAP rows must be added when this
lands on `main` (main is locked by another session as of 2026-07-09).
