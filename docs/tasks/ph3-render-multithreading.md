# Ph3 — Multithreaded render pipeline (smooth scroll / zoom)

**Developer:** P1 · **Branches:** `p1-mt-m0` … `p1-mt-m4` (one per stage) · **Size:** XL (staged) · **Crates:** `lumen-paint`, `lumen-shell`, `lumen-layout`

Decision record: [ADR-016](../decisions/ADR-016-multithreaded-render-pipeline.md).
User decision 2026-07-09: multithreading is **mandatory and urgent**.

---

## Problem (audit 2026-07-09)

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

Sub-sliced like M0 (each independently shippable into `main`):

- **M1.1 — threaded backend infra + GL-threading spike.** ✅ (branch
  `p1-mt-m1`, merged into `main`). `ThreadedRenderBackend`
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
- **M1.2 — GL-context handoff.** ✅ (branch `p1-mt-m1-2`, merged into `main`).
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
- **M1.3 — render-side momentum.** ✅ (branch `p1-mt-m1-3`, merged into `main`).
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
  `main`). Instrumentation prerequisite for the M1 acceptance step and M2
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

**Where the boundary is (audit 2026-07-10).** BUG-171 stage 2
already moved the *initial* load off the UI thread: `LoadDone` → one-shot
`std::thread::spawn(render_bytes)` → `RenderDone` → `apply_loaded_page`
(`main.rs` ~:8184/8254). What is **still on the UI thread** is every *ongoing*
relayout: `fn relayout()` (`main.rs:6743`) runs style + layout
(`relayout_page` → `layout_measured_hyp`) + display-list build (`paint_ordered`)
+ transition/`@starting-style` sync + JS-observer delivery, all synchronously.
It has **~40 call sites** — DOM mutation from JS, hover/focus/active, form input,
panel toggles, resize, theme, debounced zoom, `content-visibility` expansion
(`maybe_expand_cv_relevant`), and the rAF DOM-dirty path. `relayout()` is the
single boundary function M2 replaces with an engine-thread commit. Note: the
QuickJS runtime already lives on its own `lumen-js` thread (ADR-014) but every
call from the UI thread is a *blocking* round-trip, so JS execution still stalls
the UI thread today — M2 keeps JS on the engine side and stops shipping the
handle back to the UI thread.

Sub-sliced (each independently shippable into `main`), mirroring M0/M1:

- **M2.0 — measure the UI-thread relayout cost.** ✅ (branch `p1-mt-m2-0`).
  Prerequisite (like M0.1 for M0): before moving `relayout()` off the UI thread
  we need before/after numbers. `FrameSummary::display_with(label)` in
  `lumen-paint` lets the same tested percentile summary print under a second
  label; the shell gains `engine_stats: FrameStats` and times the whole
  `relayout()` body (style + layout + DL build + JS-observer delivery), recording
  it **only under `LUMEN_FRAME_LOG`** (zero cost otherwise — the `Instant` is
  `None` and no histogram push happens). Each relayout logs
  `[engine] relayout <ms>ms dl=<n> styled=<n>` and an `ENGINE_SUMMARY
  count/min/p50/p95/p99/max` prints on the `LUMEN_MEM_REPORT` cadence and once at
  session exit — the baseline every later M2 slice cites. Unit test on the
  labeled summary in `lumen-paint`.
- **M2.1 — persistent engine-thread boundary (scaffold).** ✅ (branch
  `p1-mt-m2-1`, merged into `main`). New `crates/shell/src/engine_thread.rs`
  mirrors `render_thread.rs`: a long-lived named `lumen-engine` thread with an
  ordered control channel, an `EngineCommit { content: Arc<DisplayList>,
  generation, dims }` snapshot (invariant 1) and a latest-wins output slot
  (`Arc<Mutex<Option<EngineCommit>>>`, queue depth 1 — invariant 2). The loop
  idle-parks on blocking `recv()` (invariant 6), drains each batch and applies
  the newest **valid** commit via `newest_commit_index` + `apply_batch`: the
  generation-guard drops commits whose `generation` is older than the last
  applied (superseded navigation — same rule as `main.rs`'s `RenderDone` guard
  `generation != load_generation`), ties break to the later index (latest-wins).
  Gated by `LUMEN_ENGINE_THREAD=1` (default off → `Lumen.engine_thread` is `None`
  and shell behavior is byte-identical; on failure to spawn, falls back to `None`
  with a log). **No relayout moved yet** — the thread just parks; M2.2 routes the
  ~40 `relayout()` sites through it. The scaffold API is `allow(dead_code)` until
  M2.2 consumes it (documented, to be removed then). 11 unit tests: 6 on
  `newest_commit_index` (latest-wins, generation-guard drops stale/all-stale,
  highest-gen-beats-position, none-without-commits), 4 on `apply_batch`
  (deposit+advance, stale-drop keeps generation, coalesce-to-one, shutdown), 1
  spawn/commit/take/shutdown lifecycle. No new deps, no `unsafe`.
- **M2.2 — route `relayout()` through the engine thread.** Turn the ~40 direct
  `self.relayout()` calls into a message send; the engine thread owns
  `LayoutSource`/`Document`/`js_ctx`, runs style+layout, and commits an
  `EngineCommit`. `content-visibility` expansion becomes a *visible-range*
  message to the engine (never a render-thread → layout call — see gotcha).
  Sub-sliced further (the full "engine owns `js_ctx`" is really the M2 endgame —
  `js_ctx` is used by dozens of main-thread event paths — so only the *pure
  layout computation* moves off-thread; JS/observer delivery stays on main):
  - **M2.2a — off-thread layout for async-safe triggers.** ✅ (branch
    `p1-mt-m2-2`, merged into `main`, 2026-07-11). Made the M2.1 scaffold live.
    `engine_thread.rs` is now a **generic latest-wins executor**
    `EngineThread<C>`: `submit(generation, job)` sends a `FnOnce() -> C` closure,
    the thread runs **only the newest** valid job of each drained batch (`Shutdown`
    aside → `newest_job_index` + `run_batch`, coalescing + generation-guard —
    invariants 2/6) and deposits the result into the queue-depth-1 slot for
    `take_committed()`. `relayout_page` was split into a pure
    `compute_layout(document, stylesheet, viewport, hp, dark_mode, web_fonts)`
    callable off-thread from `Arc` snapshots; `relayout()` split into
    `relayout_viewport()` (shared viewport derivation) + `apply_relayout_result()`
    (all `&mut self` post-work: caches, transitions/`@starting-style`, will-change
    promotion, zoom-preview reset, scroll clamp, JS-observer delivery). The shell
    defines `EngineCommit { content: DisplayList, layout_box, viewport, generation,
    compute_ms }` (owned/`Send`, invariant 1) and uses `EngineThread<EngineCommit>`.
    `submit_relayout_job()` captures `Arc` snapshots (document/stylesheet-clone/
    web-fonts/hyphenation) + interactive/forced-colors/cv thread-local state and,
    on the engine thread, re-establishes that thread-local state, runs
    `compute_layout` and returns the commit; `poll_engine_commit()` (in
    `about_to_wait`) applies the newest commit via `apply_relayout_result` under a
    generation-guard (`commit.generation != engine_job_generation` → dropped — a
    newer job or a synchronous `relayout()` superseded it). A synchronous
    `relayout()` bumps `engine_job_generation` **and** sets
    `engine_applied_generation` equal, so an in-flight off-thread result is dropped
    and no poll-wakeup is armed for it. **Wired for the one inherently-async trigger
    only — the debounced transform-first zoom** (M0.3; its visual is already
    covered by `set_preview_scale`, no caller reads geometry synchronously after
    it). All other ~44 `relayout()` sites stay synchronous. Gated by
    `LUMEN_ENGINE_THREAD=1`; **off by default → byte-identical behavior**
    (`compute_layout`/`apply_relayout_result` are the same code the sync path
    runs). Wakeup: while a job is in flight the parked winit loop arms a 4 ms poll
    deadline (a future slice can replace this with an `EventLoopProxy` wake on
    commit). Known limitation (behind the flag): a rapid re-zoom during the ~180 ms
    debounce can briefly show the previous-zoom layout before the new job's commit
    lands. 12 executor unit tests (`newest_job_index` × 6, `run_batch` × 5 incl.
    "only newest closure runs", spawn/submit/shutdown lifecycle). No new deps, no
    `unsafe`.
  - **M2.2b (remaining) — route the sync-geometry sites.** The ~44 remaining
    `relayout()` callers (DOM mutation → geometry read, hover/focus, form input,
    panel toggles, resize, theme, `content-visibility` expansion, rAF DOM-dirty)
    need the async-vs-sync contract worked out per site (which may read layout
    synchronously afterwards) and `content-visibility` expansion turned into a
    visible-range message (never render-thread → layout). This is where the bulk
    of the ~40-site conversion lands.
    - **M2.2b-1 — `LayoutSource.stylesheet` → `Arc<Stylesheet>`.** ✅ (branch
      `p1-mt-m2-2b-arc-stylesheet`, merged into `main`, 2026-07-11). Prerequisite
      slice: `LayoutSource.stylesheet` is now an immutable `Arc` snapshot, so
      `submit_relayout_job` clones only the handle (`Arc::clone(&src.stylesheet)`)
      instead of deep-cloning the whole `Stylesheet` on every off-thread submit —
      the per-job clone the audit flagged is gone. All read sites (starting-style
      check, `resolve_starting_style`, `animation_scheduler.tick`,
      `matched_rules_for_node`, `compute_layout`) are unchanged (deref coercion /
      auto-deref of `Arc`). The cold bfcache-freeze path still deep-clones into
      the owned `frozen_styles` map (`(*ls.stylesheet).clone()`), so freeze/thaw
      behavior is byte-identical. No new deps, no `unsafe`, no behavior change —
      pure allocation win on the M2.2a off-thread path.
    - **M2.2b-2 — off-thread layout for async-safe chrome-inset toggles.** ✅
      (branch `p1-mt-m2-2b-2-chrome`, merged into `main`, 2026-07-11). Routes the
      next batch of async-safe triggers off the UI thread: the ones that shift only
      *chrome* geometry (content viewport width/height) and are **not** followed by
      a synchronous read of page geometry — vertical-tabs toggle (keyboard +
      palette), tree-tabs toggle, workspace-bar toggle, active-sidebar dock flip
      (`flip_active_sidebar_dock`), docked-panel resize drag (`drag_panel_resize`)
      and web-sidebar open (`open_sidebar_page`, `!was_visible` reflow). New helper
      `Lumen::relayout_chrome()` = `if !submit_relayout_job() { relayout() }`, the
      same fall-back-to-sync pattern the M2.2a zoom path uses, so with the flag off
      (default) it is byte-identical to the previous synchronous `relayout()`. When
      `LUMEN_ENGINE_THREAD=1` the reflow lands a few frames later via the existing
      `poll_engine_commit` + generic in-flight poll-wakeup; the chrome itself draws
      from its own state on the immediately-requested redraw. 7 sites converted
      (45 → 38 sync callers + the one inside the helper). The remaining sync-geometry
      sites (DOM mutation → geometry read, hover/focus, form input, resize, theme,
      `content-visibility` expansion, rAF DOM-dirty) still need per-site
      async-vs-sync analysis and stay synchronous. No new deps, no `unsafe`.
    - **M2.2b-3 — off-thread layout for async-safe side-panel toggles.** ✅
      (branch `p1-mt-m2-2b-3-panels`, merged into `main`, 2026-07-11). Continues
      the M2.2b-2 chrome-inset batch with the two remaining side panels whose
      toggle shifts the content viewport width but is **not** followed by a
      synchronous page-geometry read: the AI panel (`ToggleAiPanel` keybinding +
      `Escape` close in `handle_ai_panel_key`) and the accessibility panel
      (`ToggleA11y` close path, which also re-styles under the newly-applied
      forced-colors preference). All three now call the existing
      `Lumen::relayout_chrome()` helper, so with the flag off (default) they are
      byte-identical to the previous synchronous `relayout()`; under
      `LUMEN_ENGINE_THREAD=1` the page reflow lands a few frames later via
      `poll_engine_commit` while the panel chrome draws from its own state on the
      immediately-requested redraw. 3 sites converted (38 → 35 sync callers). The
      remaining sync-geometry sites (DOM mutation → geometry read, hover/focus,
      form input, resize, theme, `content-visibility` expansion, rAF DOM-dirty)
      still need per-site async-vs-sync analysis and stay synchronous. No new
      deps, no `unsafe`.
    - **M2.2b-4 — off-thread layout for async-safe theme changes.** ✅ (branch
      `p1-mt-m2-2b-4-theme`, merged into `main`, 2026-07-11). Extends the
      async-safe batch beyond chrome-inset shifts to the `prefers-color-scheme`
      restyle: the OS theme flip (`WindowEvent::ThemeChanged`) and the settings-panel
      explicit dark/light lock (`SettingsHit::Close`, `shell_theme.is_dark`) both set
      `self.dark_mode` and re-run layout to re-evaluate `@media (prefers-color-scheme)`
      + push the new value to JS `matchMedia` listeners. Neither reads page geometry
      synchronously afterwards (OS path → `request_redraw`; settings path → chrome
      state only), and the off-thread job captures `dark_mode` at submit while
      `apply_relayout_result` delivers the `matchMedia` change on the UI thread — so
      routing both through `Lumen::relayout_chrome()` is byte-identical with the flag
      off (default) and lands the reflow a few frames later under
      `LUMEN_ENGINE_THREAD=1`. The helper's doc comment now covers "restyle with no
      geometry read" alongside chrome-inset shifts. 2 sites converted (35 → 33 sync
      callers). Remaining sync-geometry sites (DOM mutation → geometry read,
      hover/focus, form input, resize, `content-visibility` expansion, rAF DOM-dirty)
      still need per-site analysis and stay synchronous. No new deps, no `unsafe`.
    - **M2.2b-5 — off-thread layout for async-safe interactive pseudo-class
      restyles.** ✅ (branch `p1-mt-m2-2b-5-pseudo`, merged into `main`,
      2026-07-11). Extends the async-safe restyle batch (M2.2b-4) from theme flips
      to the interactive pointer pseudo-classes: the `:hover` change on
      `CursorMoved` (`hovered_nid` flip) and the `:active` set-on-press /
      clear-on-release (`active_nid` flip). A pseudo-class flip restyles appearance
      (color/background/border) but essentially never moves layout, and none of the
      three sites reads the *resulting* page geometry synchronously: the `:hover`
      site dispatches the follow-up JS pointer/mouse events against
      `old_nid`/`new_hovered` (node ids, not the reflow); the `:active`-press site's
      subsequent click hit-test reads the pre-`:active` `layout_box` (the geometry
      the user actually pressed on — correct); the `:active`-release site fires
      mouseup/pointerup against `hovered_nid`. All three now call the existing
      `Lumen::relayout_chrome()` helper, so with the flag off (default) they are
      byte-identical to the previous synchronous `relayout()`; under
      `LUMEN_ENGINE_THREAD=1` the highlight lands a few frames later via
      `poll_engine_commit`, and any DOM mutation from those JS events takes its own
      generation-guarded relayout (the rAF DOM-dirty path), superseding the stale
      pseudo-class job. The helper's doc comment now lists the `:hover`/`:active`
      case. 3 sites converted (33 → 30 sync callers). Remaining sync-geometry sites
      (DOM mutation → geometry read, focus, form input, resize, `content-visibility`
      expansion, rAF DOM-dirty) still need per-site analysis and stay synchronous.
      No new deps, no `unsafe`.
    - **M2.2b-6 — off-thread layout for async-safe mouse-click panel-close paths.**
      ✅ (branch `p1-mt-m2-2b-6-panel-close`, merged into `main`, 2026-07-11).
      Routes the **mouse-click** close paths of the AI, sidebar and accessibility
      panels — the pointer-driven counterparts of the keyboard toggles already moved
      off-thread in M2.2b-2 (`open_sidebar_page`) and M2.2b-3 (`ToggleAiPanel`,
      `ToggleA11y`). On a `MouseInput` press the panel hit-test fires: the AI panel's
      `AiHit::Close` and the sidebar's `SidebarHit::Close` shift chrome inset
      (`ai_panel.close()` / `sidebar.close()` removes a docked panel → content
      viewport widens), while the accessibility panel's `A11yHit::Close` /
      `A11yHit::Outside` apply the draft, hide the panel and re-style under the
      (possibly toggled) forced-colors pref. None of the four reads page geometry
      after the relayout — each does `request_redraw()` then `return`, and the
      panel hit-test's `win_w`/`win_h` are read *before* the relayout — so routing
      all four through the existing `Lumen::relayout_chrome()` helper is
      byte-identical with the flag off (default) and lands the reflow a few frames
      later under `LUMEN_ENGINE_THREAD=1`. The helper's doc comment now lists the
      mouse-click-close case. 4 sites converted (30 → 26 sync callers). Remaining
      sync-geometry sites (DOM mutation → geometry read, focus, form input, resize,
      `content-visibility` expansion, rAF DOM-dirty) still need per-site analysis and
      stay synchronous. No new deps, no `unsafe`.
    - **M2.2b-7 — off-thread layout for async-safe `:focus` restyles.** ✅ (branch
      `p1-mt-m2-2b-7-focus`, merged into `main`, 2026-07-11). Extends the async-safe
      restyle batch (M2.2b-5's `:hover`/`:active`) to the two focus-change sites that
      re-evaluate `:focus`/`:focus-within`: the JS focus request drained from
      `showModal()`/`close()` (`take_focus_requests` → `focused_node` flip) and the
      mouse-click focus set in the form/link click handler (`hit_result.node` →
      `focused_node` flip). In both, `self.focused_node` is assigned **synchronously**
      *before* the relayout, and it feeds `set_interactive_state` at the top of every
      layout pass, so any later relayout (sync or off-thread) re-evaluates the focus
      pseudo-classes correctly — deferring the focus-specific restyle never loses the
      state. Neither site reads page geometry after the relayout: the JS-request path
      only notifies `platform_bridge.focused_node_changed`; the click path dispatches
      the follow-up JS click against the pre-`:focus` `hit_result` (the geometry the
      user actually clicked — correct, mirroring M2.2b-5's `:active`-press), and any
      DOM mutation from those handlers takes its own generation-guarded relayout (rAF
      DOM-dirty), superseding the stale focus job. Both now call the existing
      `Lumen::relayout_chrome()` helper — flag off (default) → byte-identical
      synchronous `relayout()`; under `LUMEN_ENGINE_THREAD=1` the focus highlight lands
      a few frames later via `poll_engine_commit`. The helper's doc comment now lists
      the `:focus`/`:focus-within` case. 2 sites converted (26 → 24 sync callers).
      Remaining sync-geometry sites (DOM mutation → geometry read, form input, resize,
      `content-visibility` expansion, rAF DOM-dirty) still need per-site analysis and
      stay synchronous. No new deps, no `unsafe`.
    - **M2.2b-8 — off-thread layout for the last async-safe stragglers.** ✅ (branch
      `p1-mt-m2-2b-8-strays`, merged into `main`, 2026-07-11). Routes the final three
      async-safe triggers through `Lumen::relayout_chrome()`: the web-font FOUT→FOIT
      swap (`LoadEvent::FontFace` — whole-page restyle, the just-pushed font is in the
      `web_fonts` snapshot the job captures, so the off-thread reflow sees it); the
      `:hover` clear on `CursorLeft` (same async-safe restyle as the in-window hover
      flip of M2.2b-5, leave-events target the old node not this reflow); and the
      sidebar error-placeholder open (content-viewport narrowing identical to the
      success path `open_sidebar_page` already routed in M2.2b-3). Flag off (default)
      → byte-identical synchronous `relayout()`. 3 sites converted (24 → 21 sync
      callers at merge time). No new deps, no `unsafe`.
    - **M2.2b — CLOSED (async-safe routing exhausted, 2026-07-11).** After M2.2b-8
      every *async-safe* `relayout()` trigger (all interactive restyles + all
      chrome-inset shifts) runs off-thread through `relayout_chrome()`. The ~22
      `self.relayout()` sites that remain are **synchronous by design** — each was
      audited and reads page geometry in the same tick or depends on `js_ctx`, so it
      cannot use `submit_relayout_job`'s "no synchronous geometry read after" contract:
      - **resize** (`WindowEvent::Resized`, `poll_fullscreen_resize`) — followed
        immediately by `deliver_observer_records(Resize)`, which reports the *new*
        element sizes; deferral would fire ResizeObserver against stale geometry.
      - **content-visibility expansion** (`maybe_expand_cv_relevant`) — the very next
        `about_to_wait` step reads `self.layout_box` for ScrollTimeline block/inline
        progress; deferral staleness the brief's gotcha flags. Becomes a visible-range
        message in M3, not a plain `relayout_chrome` swap.
      - **`:target` cascade + navigation scroll** (`self.relayout()` before
        scroll-into-view of the fragment target) — reads the target's post-layout box.
      - **form control input / clicks** (color/date/select commit, checkbox/radio
        toggle, textarea/contenteditable edit) — direct-manipulation; caret/hit-test
        and follow-up JS read the fresh geometry synchronously.
      - **rAF DOM-dirty** (`raf_dom_dirty`, observer DOM-dirty) and **js_ctx teardown**
        — bound to `js_ctx` on the UI thread; these are the M2.2c endgame, not M2.2b.

      The one arguable exception (spellcheck-replace, `SpellMenuAction::Replace`) is a
      rare context-menu action off any hot path, so routing it off-thread buys no
      stall reduction and is intentionally left synchronous. **Conclusion: no further
      trivial `relayout_chrome` slice exists; the remaining conversions require M2.2c.**
  - **M2.2c — engine owns `Document` + `js_ctx` (the M2 endgame, L).** The remaining
    sync sites cannot move with the M2.2a/b pattern (capture-`Arc`-snapshot → compute
    → apply) because they mutate the DOM through `js_ctx` and/or read geometry in the
    same tick. This slice moves ownership of the mutable `Document` and the `lumen-js`
    handle to the engine thread so DOM-mutation → style → layout → observer delivery
    all happen off the UI thread and the UI thread shrinks to OS events + input
    forwarding + chrome. Proposed sub-slices (each independently shippable into
    `main`, mirroring M0/M1/M2.2a-b; **measure first**, then move one site class at a
    time behind `LUMEN_ENGINE_THREAD`):
    - **M2.2c-0 — acceptance baseline (measure). ✅ (branch `p1-mt-m2-2c-0`,
      merged into `main`, 2026-07-11).** Prerequisite like M2.0/M0.1. Deliverables:
      - `samples/mt-busy-loop.html` — a tall page whose rAF loop burns `BUSY_MS`
        (200) ms of CPU *synchronously* per animation frame on the UI/winit thread.
        `BUSY_MS = 0` (edit in place) is the non-stalled control on the identical
        page. (URL query/fragment can't reach a local `file://` load and the `eval`
        MCP tool isn't wired to the live JS context, so the burn is a plain constant.)
      - `scripts/mt_stall_bench.py` — drives wheel scroll for a fixed wall-clock
        window over `--mcp-live-port` with `LUMEN_FRAME_LOG=1` and **timestamps each
        `[frame]` line as it arrives** (via a stderr drain thread — a bare `PIPE`
        left unread dead-locks the child at ~4 KB). Reports the *delivered* cadence
        (p50/p95/max inter-frame gap, delivered FPS, `scroll_y` travel), which
        `scroll_perf.py`'s paint-bound FPS *ceiling* cannot see: paint stays cheap,
        the frames just never get scheduled.
      - **Recorded baseline (Windows, dev-release, 6 s window, 30 wheel ticks/s):**
        with the 200 ms burn, presentation freezes to **~2.4 fps** (inter-frame gap
        **p50/p95/max ≈ 404 ms**, all gaps a stall) and scroll only lurches **4200 px**
        over the window. Control (`BUSY_MS = 0`, same page): **~28 fps**, gap
        **p50 ≈ 36 ms**, zero stalls, scroll tracks fully (**~49 500 px**). So today
        input/scroll *does* freeze during the busy-loop (JS is a blocking round-trip
        on the UI thread) — the **~404 ms gap / ~2.4 fps** is the number M2.2c must beat
        (target: ~16 ms / 60 fps, scroll unaffected by the burn). No Rust changes, no
        new deps.
    - **M2.2c-1 — request/reply geometry readback. ✅ (branch `p1-mt-m2-2c-1`,
      merged into `main`, 2026-07-11).** Added `EngineMsg::Readback { job, reply:
      SyncSender }` + `EngineThread::readback(job) -> Option<C>` in
      `crates/shell/src/engine_thread.rs`: a UI-thread caller that needs fresh
      geometry right after a relayout (hit-test, caret, scrollIntoView) can block
      for exactly that one result instead of running layout inline. Readback is
      **not coalesced** and **skips the generation-guard** (the caller is blocking
      on it), replies directly over a `sync_channel(1)` — never through the
      latest-wins slot — and never touches `applied_generation`. In a batch it runs
      in order (after any earlier `submit`), so it observes consistent thread state;
      a `Shutdown` in the batch drops its `reply` sender → caller unblocks with
      `None` → falls back to sync. Mechanism-only, mirroring how M2.1 shipped the
      parked-thread skeleton: the variant/method are `#[allow(dead_code)]` until
      **M2.2c-3** wires live callers (which needs **M2.2c-2** to move `js_ctx`
      engine-side first). Covered by 5 new `run_batch_*`/`readback_*` unit tests
      (execute-and-reply, run-alongside-newest-`Run`, never-coalesce, shutdown-drops-reply,
      end-to-end block-and-return). No Rust behavior change with the flag off, no new deps.
      **Unblocks routing** the geometry-reading sites (M2.2c-3) without changing
      their observable semantics.
    - **M2.2c-2 — move `js_ctx` ownership to the engine thread.** The hard core:
      `js_ctx` is touched by dozens of UI-thread event paths (scroll-Y sync, event
      dispatch, observer delivery, matchMedia, lazy-image drain — ~119 `js_ctx`
      references in `crates/shell/src/main.rs` alone). Introduce an engine-side owner
      + a typed message for each UI→JS call currently done inline, so the UI thread
      stops holding the JS handle. Because this is L-sized and cross-cutting, split
      into independently-shippable sub-slices (each merged into `main`, mechanism
      before wiring, byte-identical with the flag off — mirroring M0/M1/M2.2c-0/-1):
      - **M2.2c-2a — engine-thread persistent-state primitive. ✅ (branch
        `p1-mt-m2-2c-2a`, 2026-07-11).** Gave the engine thread the ability to
        **own** long-lived engine-side state `S` (the future seat for the mutable
        `Document` + `js_ctx` handle) and run **ordered, non-coalesced** jobs against
        it. In `crates/shell/src/engine_thread.rs`: `EngineThread<C, S = ()>` +
        `EngineMsg::Task(Box<dyn FnOnce(&mut S) + Send>)`, executed in-order in
        `run_batch` (never coalesced, never touches `latest`/`applied_generation`);
        `spawn_with_state(initial)` owns `S` on the thread, `spawn()` keeps working
        via `S: Default`. UI-side helpers `task()` (fire-and-forget void UI→JS calls:
        `eval_js`, `tick_timers`, `run_animation_frame`, observer delivery) and
        `query()` (request/reply for value-returning calls: `take_dom_dirty` → bool,
        `eval_js_value`, `take_raf_pending` — built atop `Task` with a captured
        reply channel, like `readback`). State `S` is owned **solely** by the engine
        thread (UI never shares it — talks via messages), so ADR-016 invariant 1
        ("no shared mutable state") holds. Default `S = ()` → the existing stateless
        `Run`/`Readback` path (`EngineThread<EngineCommit>`) is byte-identical; the
        primitive is `#[allow(dead_code)]` until 2b. Covered by 8 new `run_batch_*`/
        `task_*`/`query_*`/`spawn_*` unit tests (execute-against-state, in-order/
        not-coalesced, positional order, shutdown-skips-task, task-alongside-newest-
        Run, end-to-end task+query, default-state). No behavior change with the flag
        off, no new deps.
      - **M2.2c-2b — move `js_ctx` into engine-side `S` behind `LUMEN_ENGINE_THREAD`.**
        ✅ (branch `p1-mt-m2-2c-2b`, merged into `main`, 2026-07-11). Сделал
        JS-хэндл **разделяемым** и посадил его на движковый поток. `PersistentJs`
        теперь `Send + Sync` (`QuickJsRuntime` уже `Send+Sync` по ADR-014 — все
        вызовы туннелируются на `lumen-js`-поток через `SyncSender`), а поле `js_ctx`
        (в `Lumen`, `LoadedPage`, `PageSnapshot`) и все сигнатуры — `Arc<dyn
        PersistentJs>` вместо `Box`, поэтому UI-поток и движковый поток могут держать
        один хэндл (регресс-защита: новый `_assert_sync::<Arc<dyn PersistentJs>>()`).
        Новая конкретная `EngineJsState { document: Option<Arc<Mutex<Document>>>,
        js: Option<Arc<dyn PersistentJs>> }` — состояние `S` движкового потока;
        поток поднимается через `EngineThread::<EngineCommit, EngineJsState>::spawn()`
        (`EngineJsState: Default`, внутри — `spawn_with_state`). `Lumen::sync_engine_js_state`
        зеркалит текущий хэндл + разделяемый DOM в состояние `task`-сообщением при
        **каждой** смене страницы (fresh load, `RenderDone`, bfcache-thaw,
        snapshot-restore, tab-switch, blank-tab) — no-op при выключенном флаге, так
        что поведение shell **байт-идентично** (по умолчанию `LUMEN_ENGINE_THREAD`
        выкл). Шим (`route_eval_js` — свободная функция ради disjoint-borrow полей
        `engine_thread`/`js_ctx`): изолированный fire-and-forget void `eval_js`
        (`_lumen_run_navigate_handler()` на deferred-start пути Navigation API) при
        включённом потоке уходит off-UI-thread через `EngineThread::task`, иначе —
        прежний синхронный `js.eval_js`. Известное ограничение под флагом (паттерн
        M2.2a): маршрутизированный `eval_js` асинхронен, поэтому read-after-eval-
        цепочки (`tick_timers` + `take_navigate_request`/`take_timer_wakeup`,
        `take_dom_dirty`) **намеренно оставлены синхронными** — они уходят на
        `query`-путь в M2.2c-2c, где ordering восстанавливается. Снял
        `#[allow(dead_code)]` с `EngineThread::task`/`spawn_with_state`/`EngineMsg::Task`
        (появились живые вызывающие); `query`/`readback` пока `dead_code` (2c/2c-3).
        3 новых теста (`EngineJsState::default` пуст; `EngineThread<_, EngineJsState>`
        несёт и мутирует реальный тип состояния через `task`/`query`;
        `route_eval_js(None, None)` — no-op) + `_assert_sync`. No new deps, no `unsafe`.
      - **M2.2c-2c — shim value-returning UI→JS calls to `query()`** (`take_dom_dirty`,
        `take_raf_pending`, `eval_js_value`, timer wakeup / nav-update drains), one
        call class at a time, each byte-identical with the flag off.
        🟡 **Первый под-срез готов** (branch `p1-mt-m2-2c-2c`, merged into `main`,
        2026-07-11): свободная функция `route_query_js` (аналог `route_eval_js`, но
        поверх [`EngineThread::query`] — блокирующий request/reply) маршрутизирует
        три value-returning класса чтений — `take_dom_dirty` (2 сайта: rAF-pump в
        `about_to_wait` + Step 4 в `RedrawRequested`), `take_raf_pending` (2 сайта,
        результат отбрасывается — очистка флага, но обязана лечь **перед**
        синхронным `run_animation_frame`, что блокирующий `query` и гарантирует под
        флагом) и `eval_js_value` (`AutomationCommand::Eval`). Под флагом чтение
        встаёт **в очередь после** уже отправленных `task` (восстанавливает
        read-after-eval порядок 2b); без флага (`engine = None`) — `js.map(read)`,
        байт-идентично прежним прямым вызовам. `query` вернул `None`
        (хэндл не зеркалирован / поток завершён при shutdown) → вызывающая сторона
        подставляет ветку «без JS» (`unwrap_or(false)` / «JS context not available»).
        Снял `#[allow(dead_code)]` с `EngineThread::query` (появились живые
        вызывающие). 3 новых теста (`route_query_js(None, None)` → `None`;
        `route_query_js(Some(engine), None)` под флагом без зеркалированного хэндла
        → `None`, `read` не исполняется). No new deps, no `unsafe`.
        ✅ **Остаток 2c готов** (branch `p1-mt-m2-2c-2c-rest`, 2026-07-11): те же
        read-after-eval цепочки, оставленные синхронными в 2b, переведены на
        `route_query_js` — nav-request/timer-wakeup чтения в `about_to_wait`
        (`take_navigate_request` → `Option<JsNavigateRequest>`, `take_timer_wakeup` →
        `Option<f64>`, оба схлопываются `flatten`) и nav-update drain в
        `RedrawRequested` (`take_nav_updates` → `Vec<_>`, `unwrap_or_default` на `None`).
        Под флагом читаются блокирующим `query` (в очереди после уже отправленных
        `task`); без флага — `js.map(read)`, байт-идентично прежним прямым вызовам.
        1 новый тест (nav/timer/nav-update без хэндла → ветка «без JS»). No new deps,
        no `unsafe`. Оставшиеся синхронные UI→JS чтения (`tick_timers`,
        `pump_*`, `take_nav_intercept_result`, canvas/worker drains) — намеренно
        синхронны, их перенос — M2.2c-2d/-3.
      - **M2.2c-2d — retire the UI-thread `js_ctx` field under the flag.** Once every
        call site routes through `task`/`query`, the UI thread stops holding the JS
        handle entirely (flag on); the flag-off legacy field is removed last.
        🟡 **Первый под-срез готов** (branch `p1-mt-m2-2c-2d-1`, merged into `main`,
        2026-07-11): обобщил `route_eval_js` (частный случай `|js| js.eval_js(&script)`)
        новой свободной функцией `route_task_js(engine, js, action)` — маршрутизатор
        любого fire-and-forget void-действия над `&Arc<dyn PersistentJs>`; сам
        `route_eval_js` теперь делегирует ей (байт-идентично, устранён дубль ветвления).
        Перевёл per-tick pump-батч в `about_to_wait` (`tick_timers` + `pump_websockets`
        + `pump_sse` + `pump_workers` + `pump_broadcast_channels` + `pump_shared_workers`,
        `main.rs` ~:8801) с прямых `js.<method>()` на `route_task_js`. Под флагом
        (`LUMEN_ENGINE_THREAD=1`) батч уходит off-UI-thread одним `task` (порядок
        вызовов внутри сохранён), а последующие `route_query_js`-чтения nav/timer встают
        в очередь **после** него — read-after-write порядок восстановлен, как для routed
        `eval_js` в 2b/2c. Без флага (по умолчанию) — прежние синхронные вызовы,
        байт-идентично. 2 новых теста (`route_task_js` без хэндла = no-op;
        флаг-он без зеркалированного хэндла → действие пропущено, барьер-`query`
        подтверждает исполнение task). No new deps, no `unsafe`. Оставшиеся синхронные
        UI→JS чтения (`take_nav_intercept_result`, canvas/history drains) — следующие
        под-срезы 2d, затем снятие самого поля.
        🟡 **Второй под-срез готов** (branch `p1-mt-m2-2c-2d-2`, merged into `main`,
        2026-07-11): перевёл оставшиеся per-tick value-returning дренажи в
        `about_to_wait` — canvas (`flush_canvas_updates`, `main.rs` ~:8965), history
        pushState/replaceState (`take_history_url_updates`) и history.go/back/forward
        (`take_history_traversals`) — с прямого `js_ctx.map(<drain>).unwrap_or_default()`
        на `route_query_js`. Под флагом (`LUMEN_ENGINE_THREAD=1`) читаются блокирующим
        `query`, встающим в очередь **после** уже отправленного pump-`task` (2d-1), —
        read-after-write порядок сохранён; без флага (по умолчанию) — прежний `js.map`,
        байт-идентично. History-дренажи собираются в локальный `Vec` **до**
        `&mut self`-мутаций стека навигации (disjoint-borrow полей `engine_thread`/
        `js_ctx` уживается с последующими `self.nav_back.push`/`navigate_by`). 1 новый
        тест (canvas/history дренажи без хэндла → пустой `Vec`). No new deps, no
        `unsafe`. Единственное оставшееся синхронное UI→JS чтение —
        `take_nav_intercept_result` (4 сайта в `navigate_to`/`_replace`/`_back`/
        `_forward`, read-after-eval цепочка) — следующий под-срез 2d, затем снятие
        самого поля `js_ctx` под флагом.
        ✅ **Третий под-срез готов** (branch `p1-mt-m2-2c-2d-3`, merged into `main`,
        2026-07-11): последнее синхронное read-after-eval UI→JS чтение —
        `take_nav_intercept_result` в `navigate_to`/`_replace`/`_back`/`_forward` —
        переведено на маршрутизаторы. В каждом из 4 сайтов nav-dispatch eval
        (`_lumen_dispatch_navigate`) и intercept-handler eval
        (`_lumen_run_navigate_handler`) ушли на `route_task_js`, а само
        `take_nav_intercept_result` → `route_query_js` (возврат `Option<Vec<(bool,
        bool)>>`; внешний `None` = ветка «без JS», как прежний `if let Some(js) =
        &self.js_ctx`). Под флагом (`LUMEN_ENGINE_THREAD=1`) dispatch уходит
        off-UI-thread одним `task`, блокирующий `query` встаёт в очередь **после**
        него — read-after-eval порядок сохранён; без флага (по умолчанию) — прежние
        синхронные вызовы, байт-идентично. Прямых `self.js_ctx`-чтений в nav-методах
        не осталось. 1 новый тест (nav-intercept без хэндла → `None` → intercept-блок
        пропущен). No new deps, no `unsafe`. **Все value-returning UI→JS чтения
        зашимлены** — следующий под-срез 2d снимает само поле `js_ctx` с UI-потока
        под флагом.
        ✅ **Четвёртый под-срез готов** (branch `p1-mt-m22d`, merged into `main`,
        2026-07-11): пост-рендер блок дренажей JS-очередей в `RedrawRequested`
        (`main.rs` ~:9370) переведён с прямых `if let Some(js) = &self.js_ctx { …
        js.take_*() … }` на `route_query_js`. 8 value-drain сайтов: Web Notifications
        (`take_notification_requests`), `window.open` (`take_window_open_requests`),
        Fullscreen (`take_fullscreen_requests`), Print (`take_print_requests`), dialog
        focus (`take_focus_requests`), View Transitions (`take_view_transition_events`),
        DevTools console (`take_console_messages`), page-scroll
        (`take_page_scroll_requests`). Каждый — чистый drain-`Vec` с последующим
        `&mut self`-действием; `route_query_js` собирает owned-`Vec` и сразу
        отпускает borrow полей `engine_thread`/`js_ctx`, поэтому `&mut self`-вызовы
        (`navigate_to`/`handle_print_request`/…) не конфликтуют. Под флагом
        (`LUMEN_ENGINE_THREAD=1`) дренажи идут off-UI-thread блокирующим `query`;
        без флага (по умолчанию) — `js.map(read)`, байт-идентично прежнему
        `js.take_*()`; `None` → `unwrap_or_default` = пустой дренаж (как ветка
        `js_ctx == None`). No new deps, no `unsafe`. Остаются прямые `self.js_ctx`
        write-back-сайты в том же блоке (element-scroll `update_scroll_states`/
        `fire_element_scroll`, GC `gc_collect`) и синхронные fire-and-forget
        event-dispatch сайты — следующие под-срезы 2d перед снятием самого поля.
        ✅ **Пятый под-срез готов** (branch `p1-mt-m22d-5`, merged into `main`,
        2026-07-11): navigation/lifecycle fire-and-forget void-сайты переведены с
        прямых `if let Some(js) = &self.js_ctx { … }` на `route_eval_js`/
        `route_task_js`. 6 сайтов: `deliver_a11y_media_changes`
        (`_lumen_deliver_media_changes` eval), `commit_nav_state`
        (`_lumen_navigation_set_state` eval), `fire_navigate_success`,
        `fire_navigate_error`, `fire_current_entry_change` (Navigation API
        события) и `bfcache_thaw` pageshow-lifecycle eval. Все — чистый void без
        чтения результата следом; под флагом (`LUMEN_ENGINE_THREAD=1`) уходят
        off-UI-thread одним `task`, без флага (по умолчанию) — синхронный вызов по
        UI-хэндлу, байт-идентично. Сериализация payload в `commit_nav_state`
        вынесена перед маршрутизацией (ранние `return` на ошибке сериализации
        сохранены). No new deps, no `unsafe`. Механизм не менялся — покрыт
        существующими route/engine_thread тестами.
        ✅ **Шестой под-срез готов** (branch `p1-mt-m22d-final`, merged into `main`,
        2026-07-11): navigation-history pagehide/popstate fire-and-forget void-сайты
        переведены с прямых `if let Some(js) = &self.js_ctx { … }` на `route_task_js`.
        5 сайтов в `navigate_to`/`navigate_back`/`navigate_forward`:
        `fire_page_lifecycle("pagehide", …)` ×3 (full-doc unload перед `reload`,
        HTML LS §8.6) и `fire_popstate(&state_json, &url)` ×2 (same-doc back/forward).
        Все — чистый void без чтения результата следом; owned `state_json`/`url`
        (и Copy-`persisted`) переезжают в `move`-замыкание. Под флагом
        (`LUMEN_ENGINE_THREAD=1`) уходят off-UI-thread одним `task` (для popstate —
        перед уже маршрутизированными `fire_current_entry_change`/`commit_nav_state`,
        порядок сохранён); без флага (по умолчанию) — синхронный вызов по UI-хэндлу,
        байт-идентично. No new deps, no `unsafe`. Механизм не менялся — покрыт
        существующими route/engine_thread тестами. Остаются смешанные сайты
        (element-scroll write-back `take_scroll_requests`, GC `gc_tick`, pointer-lock
        mouse-motion, PiP-close eval) и sync event-dispatch — следующие под-срезы 2d
        перед снятием самого поля.
        ✅ **Седьмой под-срез готов** (branch `p1-mt-m22d-7`, merged into `main`,
        2026-07-11): смешанные read+write-back сайты пост-рендер блока
        `RedrawRequested` переведены с прямых `if let Some(js) = &self.js_ctx { … }` на
        маршрутизаторы. 2 сайта: (1) **element-scroll** (`main.rs` ~:9567) — дренаж
        `take_scroll_requests` → `route_query_js`, а write-back после layout-работы
        (`update_scroll_states` + `fire_element_scroll` ×N) собран в один
        `route_task_js` (owned `HashMap<u32,[f32;4]>` + `Vec<u32>` scrolled_nids
        переезжают в `move`-замыкание); (2) **GC** (`main.rs` ~:9628) — сам
        `gc_collect(&ids)` → `route_task_js`, тогда как dead-node computation
        (`layout_source`-документ + `&mut gc_tick.poll`) осталась на UI-потоке; гейт
        `Some(_js)` сохранён, чтобы `gc_tick` тикал только при наличии JS-контекста
        (байт-идентично флаг-офф). Под флагом (`LUMEN_ENGINE_THREAD=1`) дренаж скролла —
        блокирующий `query`, write-back и `gc_collect` — `task` в очередь **после** него
        (read-after-write порядок сохранён); без флага (по умолчанию) — прежние
        синхронные `js.<method>()`, байт-идентично. Disjoint-borrow полей
        `layout_box`/`display_list`/`engine_thread`/`js_ctx` (прямой доступ к полям
        `self`) уживается с `&mut lb`. No new deps, no `unsafe`. Механизм не менялся —
        покрыт существующими route/engine_thread тестами. Остаются pointer-lock
        mouse-motion, PiP-close eval и sync event-dispatch — следующие под-срезы 2d
        перед снятием самого поля.
        ✅ **Восьмой под-срез готов** (branch `p1-mt-m22d-8`, merged into `main`,
        2026-07-11): pointer-lock / PiP fire-and-forget void-eval сайты переведены с
        прямых `if let Some(js) = &self.js_ctx { js.eval_js(…) }` на `route_eval_js`.
        3 сайта: (1) **pointer-lock raw mouse-motion** (`device_event`, `main.rs`
        ~:9722) — `_lumen_dispatch_locked_mousemove(...)` (guard упрощён с
        `(Some(ctx), Some(nid))` до одного `Some(nid)`; `script` строится до
        маршрутизации, борроу `js_ctx`/`engine_thread` — раздельный); (2) **PiP
        close-button** (`window_event`, `main.rs` ~:9770) — `exitPictureInPicture()`
        mirror после `close_pip_os`; (3) **pointerlockchange** на Escape
        (`main.rs` ~:9832) — `document.dispatchEvent(new Event('pointerlockchange'))`.
        Все три — чистый void без чтения результата следом; под флагом
        (`LUMEN_ENGINE_THREAD=1`) уходят off-UI-thread одним `task`, без флага (по
        умолчанию) — синхронный вызов по UI-хэндлу, байт-идентично. No new deps, no
        `unsafe`. Механизм не менялся — покрыт существующими route/engine_thread
        тестами. Остаётся только категория **sync event-dispatch** (mouse/key/wheel/
        input-обработчики, ~30 прямых `self.js_ctx`-сайтов) — следующие под-срезы 2d
        перед снятием самого поля.
        ✅ **Девятый под-срез готов** (branch `p1-mt-m22d-9`, merged into `main`,
        2026-07-11): ядро **mouse/pointer/drag/capture event-dispatch** переведено с
        прямых `if let Some(ctx) = &self.js_ctx { ctx.eval_js(…) }` на `route_eval_js`.
        4 helper-метода `Lumen`, через которые текут все mouse/pointer/drag/capture
        DOM-события: `js_mouse_event` (`_lumen_dispatch_mouse_event` — mousedown/up/
        over/out/enter/leave/move), `js_pointer_event` (`_lumen_dispatch_pointer_event`),
        `js_drag_event` (`_lumen_dispatch_drag_event` — dragstart/drag/enter/leave/over/
        drop/end) и `js_capture_event` (`_lumen_dispatch_capture_event` — got/lost
        pointercapture). Все четыре — чистый fire-and-forget void `eval_js`, результат
        диспатча нигде синхронно не читается (в `main.rs` нет чтения preventDefault —
        Lumen не гейтит default-действия на JS), поэтому маршрутизация безопасна: под
        флагом (`LUMEN_ENGINE_THREAD=1`) диспатч уходит off-UI-thread одним `task` (в
        порядке среди прочих `Task`, так что последующие `route_query_js`-чтения встают
        в очередь после него — read-after-write порядок сохранён), без флага (по
        умолчанию) — синхронный вызов по UI-хэндлу, **байт-идентично** прежнему
        `ctx.eval_js(&script)`. `script` строится до маршрутизации; методы `&self`, борроу
        `engine_thread`/`js_ctx` — раздельный. Синхронный pre-dispatch read
        `pointer_capture_nid()` в `dispatch_mouse_move` не тронут (читает **до** диспатча).
        No new deps, no `unsafe`. Механизм не менялся — покрыт существующими route/
        engine_thread тестами. Остаток категории sync event-dispatch (keyboard/input-
        обработчики, click-диспатч, ~20 прямых `self.js_ctx`-сайтов) — следующие под-срезы
        2d перед снятием самого поля.
        ✅ **Десятый под-срез готов** (branch `p1-mt-m22d-10`, merged into `main`,
        2026-07-11): **read-after-eval click + keyboard event-dispatch** — 4 сайта с
        идентичным паттерном «`_lumen_dispatch_*` void-eval, затем
        `take_navigate_request`» — переведены с прямых `if let Some(ctx) = &self.js_ctx {
        … ctx.eval_js(…); ctx.take_navigate_request() … }` на `route_eval_js` +
        `route_query_js`. Сайты: (1) **mouse click** (`handle_mouse_input`, `main.rs`
        ~:13259) — `_lumen_dispatch_mouse_event('click', …)`; (2) **inject_special_key**
        (`main.rs` ~:13616) — `_lumen_dispatch_key_event` keydown→keyup; (3) **inject_char**
        (`main.rs` ~:13634) — keydown→input→keyup; (4) **activate_node** (hint-mode click,
        `main.rs` ~:15936) — тот же click-eval. В каждом: сам `_lumen_dispatch_*` уходит
        fire-and-forget через `route_eval_js`, а последующий `take_navigate_request`
        (навигация, что handler мог поставить) — через `route_query_js` (`Option<Option<
        JsNavigateRequest>>`; внешний `None` = ветка «без JS», как прежний early-`return`/
        несматчившийся `Some(ctx)`). Под флагом (`LUMEN_ENGINE_THREAD=1`) dispatch уходит
        off-UI-thread одним `task`, блокирующий `query` встаёт в очередь **после** него —
        read-after-eval порядок сохранён; без флага (по умолчанию) — прежние синхронные
        вызовы по UI-хэндлу, байт-идентично. `script` строится до маршрутизации, борроу
        `engine_thread`/`js_ctx` — раздельный. Прямых `self.js_ctx` read-after-eval
        event-dispatch сайтов не осталось. No new deps, no `unsafe`. Механизм не менялся —
        покрыт существующими route/engine_thread тестами (`route_eval_js_without_handle_is_noop`,
        `route_query_js_nav_reads_without_handle_default_to_no_op`). Остаток категории sync
        event-dispatch (чистые fire-and-forget void-eval формо-действий: `toggle`/dialog-close,
        ~15 сайтов) — следующие под-срезы 2d перед снятием самого поля.
        ✅ **Одиннадцатый под-срез готов** (branch `p1-mt-m22d-11`, 2026-07-11):
        **form-action fire-and-forget void-dispatch** — 4 сайта переведены с прямых
        `if let Some(ctx) = &self.js_ctx { … }` на `route_eval_js`/`route_task_js`.
        Сайты: (1) **file-input change** (`open_file_picker`, `main.rs` ~:6907) —
        `_lumen_deliver_file_list(id, json)`: токены (`register_file_token`) и JSON
        строятся **на UI-потоке** (регистрация в глобальном реестре happens-before
        постановки в очередь), сам `eval_js` — через `route_eval_js`; гейт заменён
        с `if let Some(js)` на `if self.js_ctx.is_some()`, чтобы токены регистрировались
        только при наличии JS-контекста (байт-идентично флаг-офф). (2) **`<details>`
        toggle** ×2 — mouse-click `handle_form_click` (`main.rs` ~:13418) и
        keyboard-`activate_node` (`main.rs` ~:16007) — `dispatchEvent(new Event('toggle'))`
        (HTML §4.11.1); за каждым идёт синхронный `self.relayout()`, читающий
        `layout_source.document` (открытость `<details>` уже применена `toggle_details_open`
        **до** маршрутизации, событие лишь уведомляет JS — read-after-write сохранён).
        (3) **dialog-close** (`method="dialog"` form-submit, `main.rs` ~:13482) —
        `fire_dialog_close(dnid, rv)` через `route_task_js`; гейт `(Some(dnid), Some(js))`
        разбит на внешний `if let Some(dnid)` (ancestor-`<dialog>` обязателен) + owned
        `rv.to_string()`/`dnid_idx`, переезжающие в `move`-замыкание. Все четыре — чистый
        void без синхронного чтения результата диспатча следом. Под флагом
        (`LUMEN_ENGINE_THREAD=1`) диспатч уходит off-UI-thread одним `task`; без флага
        (по умолчанию) — синхронный вызов по UI-хэндлу, **байт-идентично** прежним
        `ctx.eval_js`/`js.fire_dialog_close`. `script`/owned-аргументы строятся до
        маршрутизации, борроу `engine_thread`/`js_ctx` — раздельный. No new deps, no
        `unsafe`. Механизм не менялся — покрыт существующими route/engine_thread тестами
        (`route_eval_js_without_handle_is_noop`, `route_task_js_without_handle_is_noop`).
        Остаётся категория sync event-dispatch (contenteditable-key input-eval, fullscreen-exit
        lifecycle, per-frame scroll/rAF/paint-timing вызовы в `RedrawRequested`) —
        следующие под-срезы 2d перед снятием самого поля.
        ✅ **Двенадцатый под-срез готов** (branch `p1-mt-m22d-12`, 2026-07-11):
        **per-frame scroll + paint-timing fire-and-forget void-dispatch** — 4 сайта
        переведены с прямых `if let Some(js) = &self.js_ctx { js.<void>() }` на
        `route_task_js`. Сайты: (1) **wheel `fire_window_scroll`** (`MouseScrollDelta::
        LineDelta` arm, `main.rs` ~:11669) — window `scroll`-событие после smooth-скролла;
        (2) **`set_page_scroll_y`** (Step 1 `RedrawRequested`, `main.rs` ~:11791) —
        синхронизация `window.scrollY` (`scroll_y` считывается в локаль **до**
        маршрутизации, чтобы `move`-замыкание не заимствовало `self` повторно); (3)
        **`deliver_scroll_progress`** (Step 1.5 scroll-driven анимации, `main.rs`
        ~:11824) — `p_y`/`p_x` (уже локали) переезжают в `move`-замыкание; (4)
        **`deliver_paint_timing`** ×2 (Step 5 Paint Timing, `main.rs` ~:11962) —
        first-paint / first-contentful-paint; здесь гейт `if let Some(js)` заменён на
        `if self.js_ctx.is_some()`, чтобы флаги `first_*_delivered` защёлкивались
        **только** при наличии JS-контекста (байт-идентично прежнему поведению — при
        `js_ctx == None` флаги не выставлялись), а сами void-вызовы уходят через
        `route_task_js`. Все четыре — чистый fire-and-forget void без синхронного
        чтения результата следом. Под флагом (`LUMEN_ENGINE_THREAD=1`) уходят
        off-UI-thread одним `task` (порядок FIFO среди прочих `Task`); без флага (по
        умолчанию) — синхронный вызов по UI-хэндлу, **байт-идентично** прежним
        `js.<method>()`. No new deps, no `unsafe`. Механизм не менялся — покрыт
        существующими route/engine_thread тестами (`route_task_js_without_handle_is_noop`).
        Остаётся категория sync event-dispatch (contenteditable-key input-eval,
        fullscreen-exit lifecycle, rAF `run_animation_frame` + `has_raf_pending`) —
        следующие под-срезы 2d перед снятием самого поля.
        ✅ **Тринадцатый под-срез готов** (branch `p1-mt-m22d-13`, 2026-07-11):
        последняя категория sync event-dispatch переведена с прямых `if let Some(js) =
        &self.js_ctx { js.<method>() }` на маршрутизаторы. 4 сайта в двух классах:
        (1) **rAF-батч** ×2 — `about_to_wait` rAF-памп (`main.rs` ~:8886) и
        `RedrawRequested` Step 3.1 (`main.rs` ~:11930): прямые `js.has_raf_pending()`
        (value read) → `route_query_js`, `js.run_animation_frame(raf_ts)` (void) →
        `route_task_js`; порядок `has_raf_pending` → `take_raf_pending` →
        `run_animation_frame` → `take_dom_dirty` сохранён (под флагом чтения —
        блокирующий `query`, батч — `task` в очередь между ними; последующий Step 4
        `take_dom_dirty`-query встаёт после батч-`task`). (2) **fullscreen-exit**
        (`_lumen_notify_fullscreen_exit` на Escape, `main.rs` ~:13920) — fire-and-forget
        void `eval_js` → `route_eval_js`. (3) **contenteditable-key** (`main.rs` ~:13970)
        — `_lumen_handle_contenteditable_key`-вызовы (backspace/delete/enter/insertText)
        → `route_eval_js`; DOM-read `find_editing_host` остаётся на UI-потоке (читает
        разделяемый `src.document`, не JS-хэндл), гейт заменён с `if let Some(js)` на
        `if self.js_ctx.is_some()` (editing-host detection + eval только при наличии
        JS-контекста). Все — чистый fire-and-forget void без синхронного чтения
        результата следом. Под флагом (`LUMEN_ENGINE_THREAD=1`) уходят off-UI-thread;
        без флага (по умолчанию) — прежние синхронные вызовы, **байт-идентично**.
        No new deps, no `unsafe`. Механизм не менялся — покрыт существующими
        route/engine_thread тестами (`route_eval_js_without_handle_is_noop`,
        `route_task_js_without_handle_is_noop`, `route_query_js_without_handle_is_none`).
        **Все категории event-dispatch зашимлены.** Остаток прямых `self.js_ctx`-чтений
        (не event-dispatch) — под-срезы дальше: pointer-capture pre-dispatch reads,
        `WaitCondition::JsIdle` wait-poll, layout-geometry push (`update_layout_rects`
        и Co.), lazy-images/pageshow setup, focus/scroll-states/hashchange, tab
        park/unpark (`pause_event_loop`/`unpause_event_loop`, зависит от bg-tab
        snapshot). Само поле `js_ctx` снимается с UI-потока последним под-срезом,
        когда ни одного прямого чтения не останется.
        ✅ **Четырнадцатый под-срез готов** (branch `p1-mt-m22d-14`, 2026-07-11):
        оставшиеся синхронные **value-returning** UI→JS чтения переведены с прямых
        `self.js_ctx.as_ref().and_then(...)` / `is_none_or(...)` на `route_query_js`.
        4 сайта в двух классах: (1) **pre-dispatch pointer-capture** ×3 —
        `pointer_capture_nid()` в mouseup (`main.rs` ~:11431) и pointermove
        (`dispatch_mouse_move`, ~:13026, явно оставлен непереведённым в срезе 9) +
        `take_pointer_capture()` (implicit-release на mouseup, ~:11437). Каждое
        `route_query_js(...)` возвращает `Option<Option<u32>>`; `.flatten()` схлопывает
        «без JS» (внешний `None`) и «нет capture» (внутренний `None`) в ту же ветку —
        `unwrap_or(hit_nid)` / пропуск `lostpointercapture`, байт-идентично прежнему
        `and_then(...)`. Под флагом (`LUMEN_ENGINE_THREAD=1`) capture-read — блокирующий
        `query`; `take_pointer_capture` встаёт в очередь **после** уже
        маршрутизированных pointerup/mouseup eval-`task` — read-after-eval порядок
        сохранён. (2) **wait-poll `has_raf_pending`** (`WaitCondition::JsIdle`,
        `check_wait_condition`, ~:18662) — `!route_query_js(...).unwrap_or(false)`;
        «без JS» (`None`) → `unwrap_or(false)` → `!false` = `true` (idle), как прежний
        `is_none_or`. Без флага (по умолчанию) — синхронный вызов по UI-хэндлу,
        байт-идентично. 1 новый тест
        (`route_query_js_pointer_capture_and_raf_reads_without_handle_default_to_no_op`).
        No new deps, no `unsafe`. Остаются прямые `self.js_ctx`-чтения вне
        event-dispatch/value-read категории (layout-geometry push, lazy-images/pageshow
        setup, focus/scroll-states/hashchange void-eval, tab park/unpark) — следующие
        под-срезы 2d перед снятием самого поля.
        ✅ **Пятнадцатый под-срез готов** (branch `p1-mt-m22d-15`, 2026-07-11):
        класс **layout-geometry push** (`update_layout_rects` и Co.) — 3 сайта
        `if let (Some(js), Some(lb_ref)) = (&self.js_ctx, self.layout_box.as_ref())`
        переведены на маршрутизаторы. (1) **relayout observer-delivery** (`relayout`,
        `main.rs` ~:7196) — смешанный read+write: вся упорядоченная последовательность
        (rects/styles/viewport push → `deliver_layout_observers` + `deliver_media_query_changes`
        + `deliver_lazy_images` → `take_lazy_image_requests` read → `update_scroll_states`
        push) обёрнута в **один** `route_query_js`, возвращающий `lazy_reqs`, так что под
        флагом (`LUMEN_ENGINE_THREAD=1`) она исполняется атомарно **в порядке** на
        движковом потоке (value-read после void-push сохраняет read-after-write порядок),
        блокируя лишь ради одного результата. (2) **fresh-load seed** (`main.rs` ~:7758)
        и (3) **bfcache-thaw seed** (`main.rs` ~:18006) — по 3 owned-arg void-вызова
        (`update_layout_rects`/`update_computed_styles`/`update_viewport_size`) через
        `route_task_js`. Все captured-данные owned (`HashMap`/`Vec`) → замыкания
        `Send + 'static`; сбор геометрии (`collect_layout_rects`/`_computed_styles`/
        `_scroll_containers`, без побочных эффектов) идёт на UI-потоке до маршрутизации.
        Гейт `if let Some(js)` заменён на `if self.js_ctx.is_some() && let Some(lb_ref)`,
        чтобы сбор геометрии происходил только при наличии JS-контекста — байт-идентично
        флаг-офф (`route_*(…, None, …)` = синхронный вызов по UI-хэндлу / no-op без него).
        1 новый тест (`route_query_js_layout_geometry_push_without_handle_defaults_to_empty`).
        No new deps, no `unsafe`. Остаются прямые `self.js_ctx`-чтения (lazy-images/pageshow
        setup, focus/scroll-states/hashchange void-eval, tab park/unpark) — следующие
        под-срезы 2d перед снятием самого поля.
        ✅ **Шестнадцатый под-срез готов** (branch `p1-mt-m22d-16`, 2026-07-11):
        класс **focus/scroll-states/hashchange void-eval** — 3 сайта
        `if let Some(js) = &self.js_ctx { js.<void>() }` переведены на маршрутизаторы,
        все чистый fire-and-forget void без синхронного чтения результата следом.
        (1) **hashchange fragment-nav** (`navigate_to_fragment`, `main.rs` ~:7632) — два
        `_lumen_dispatch_navigate('fragment', …)` + `_lumen_navigate_or_fragment(…)`
        через `route_eval_js` (двумя `task` в FIFO, dispatch→navigate сохранён);
        `location`/`hashchange` фиксируются JS-стороной, а `HistoryUrlUpdate` дренится
        позже через `take_nav_updates` — read-after-eval нет. Гейт `if let Some(js)`
        заменён на `if self.js_ctx.is_some()` (эскейп-строка строится только при JS).
        (2) **focus-changed** (`main.rs` ~:13357) — `notify_focus_changed(focus_idx)`
        через `route_task_js`; `focus_idx` (owned `Option<u32>`) вычисляется до
        маршрутизации, гейт снят (route-хелпер сам обрабатывает `None`). (3)
        **overflow-container element-scroll** (`try_scroll_overflow_container`,
        `main.rs` ~:17022) — `update_scroll_states(states)` push → `fire_element_scroll(target_nid)`
        одним `route_task_js`; `states` (owned `HashMap`) и `target_nid` (`u32`, Copy)
        переезжают в `move`-замыкание, порядок push→dispatch сохранён внутри одного
        `task`. Под флагом (`LUMEN_ENGINE_THREAD=1`) все три уходят off-UI-thread; без
        флага (по умолчанию) — синхронные вызовы по UI-хэндлу, **байт-идентично** прежним
        `js.eval_js`/`js.<method>`. No new deps, no `unsafe`. Механизм не менялся —
        покрыт существующими route/engine_thread тестами (`route_eval_js_without_handle_is_noop`,
        `route_task_js_without_handle_is_noop`). Остаются прямые `self.js_ctx`-чтения
        (lazy-images/pageshow setup, tab park/unpark — зависит от bg-tab snapshot) —
        следующие под-срезы 2d перед снятием самого поля.
        ✅ **Семнадцатый под-срез готов** (branch `p1-mt-m22d-17`, 2026-07-11):
        класс **lazy-images/pageshow setup + resize-eval** — 3 сайта переведены с
        прямых `if let Some(js) = &self.js_ctx { … }` на маршрутизаторы. (1)
        **lazy-image регистрация + immediate proximity check** (`apply_loaded_page`,
        `main.rs` ~:8395, BUG-163) — смешанный read+write: вся упорядоченная
        последовательность (`register_lazy_images` → `update_layout_rects`/
        `update_viewport_size` push → `deliver_layout_observers` + `deliver_lazy_images`
        → `take_lazy_image_requests` read) обёрнута в **один** `route_query_js`,
        возвращающий `Vec<(u32,String)>`, так что под флагом (`LUMEN_ENGINE_THREAD=1`)
        она исполняется атомарно **в порядке** на движковом потоке (value-read после
        void-push сохраняет read-after-write), блокируя лишь ради одного результата;
        owned-данные (`owned_pairs: Vec<(u32,String)>`, `geom: Option<(HashMap<u32,
        [f32;4]>, f32, f32)>`) собираются на UI-потоке до маршрутизации (замыкание
        `Send + 'static`), гейт `if let Some(js)` заменён на `if self.js_ctx.is_some()`
        (сбор геометрии JS-гейтнут — байт-идентично флаг-офф). (2) **pageshow
        lifecycle** (`main.rs` ~:8437) — `notify_window_loaded()` +
        `fire_page_lifecycle("pageshow", persisted)` одним `route_task_js` (void, guard
        снят — helper сам обрабатывает `None`). (3) **resize-eval** (element resize
        handle drag `CursorMoved`, `main.rs` ~:10240) — `_lumen_apply_resize(...)`
        через `route_eval_js` (fire-and-forget void; `self.resize_active` — Copy,
        borrow не удерживается). Под флагом все три уходят off-UI-thread; без флага
        (по умолчанию) — синхронные вызовы по UI-хэндлу, **байт-идентично** прежним
        `js.<method>()`/`js.eval_js`. 1 новый тест
        (`route_lazy_pageshow_resize_without_handle_default_to_no_op`). No new deps, no
        `unsafe`. Остаются прямые `self.js_ctx`-чтения только в **tab park/unpark**
        (`switch_tab`: `pause_event_loop`/`unpause_event_loop`, `main.rs` ~:18909/18941
        — завязаны на bg-tab snapshot save/restore) — следующий под-срез 2d, затем
        снятие самого поля `js_ctx` с UI-потока.
        ✅ **Восемнадцатый под-срез готов** (branch `p1-mt-m22d-18`, 2026-07-11):
        класс **tab park/unpark** в `switch_tab` — 2 последних прямых
        `if let Some(js) = &self.js_ctx { … }`-обращения переведены на `route_task_js`.
        (1) **park** (T0→T1, `main.rs` ~:18938) — `pause_event_loop()` перед
        `save_page_snapshot()`; под флагом уходит `task`-ом, где `state.js` ещё
        зеркалит уходящую в фон вкладку (ре-зеркалирование `sync_engine_js_state`
        встанет в очередь только при загрузке/восстановлении новой) — pause на верном
        хэндле. (2) **unpark** (T1→T0, `main.rs` ~:18970) — `unpause_event_loop()` +
        `run_gc_pass(0)` после `restore_page_snapshot()`; последний уже вызвал
        `sync_engine_js_state()` (зеркалит восстановленный хэндл `task`-ом), а этот
        `task` встаёт **после** него — unpause+GC на восстановленном хэндле. Оба —
        fire-and-forget void; disjoint borrow полей `engine_thread`/`js_ctx`. Без флага
        (по умолчанию) — синхронные вызовы по UI-хэндлу, **байт-идентично** прежним
        `js.pause_event_loop()`/`js.unpause_event_loop()`/`js.run_gc_pass(0)`.
        **Не тронут** bg-tab-side `run_gc_pass(1)` (`main.rs` ~:18945) — он читает
        `bg_tabs[old_id].js_ctx` (снимок запаркованной вкладки), а не `self.js_ctx`;
        `state.js` движкового потока зеркалит **активную** вкладку, поэтому его нельзя
        маршрутизировать — остаётся прямым. 1 новый тест
        (`route_tab_park_unpark_without_handle_default_to_no_op`). No new deps, no
        `unsafe`. **Прямых `self.js_ctx`-обращений с вызовом методов больше нет** —
        остаются только `.is_some()`-гейты, `.as_ref()` в маршрутизаторах, `.clone()`/
        `.take()` и присваивания поля (lifecycle снапшота) → следующий шаг 2d: снятие
        самого поля `js_ctx` с UI-потока (перенос его lifecycle + snapshot save/restore
        на движковую сторону).
        ✅ **Девятнадцатый под-срез готов** (branch `p1-mt-m22d-19`, 2026-07-11):
        **декаплинг `.is_some()`-гейтов от владения `Arc`** — пролог к переносу
        самого хэндла на движковый поток. Введено UI-поле `js_present: bool`
        (`main.rs` ~:6316), которое держится в связке с `self.js_ctx` через новый
        централизованный сеттер `set_js_ctx` (`main.rs` ~:7366): все ~10 сайтов
        присваивания `self.js_ctx = …` (load, apply_loaded_page, bfcache-thaw,
        restore-path, restore_page_snapshot, reset_for_fresh_tab) переведены на
        `set_js_ctx(handle)`, а `save_page_snapshot` сбрасывает `js_present = false`
        рядом с `js_ctx.take()`. Все 8 боевых гейтов `if self.js_ctx.is_some()`
        (drag&drop token-register, layout-observer push ×2, void-dispatch,
        lazy-images collect, first-paint delivery, contenteditable-eval, resize-eval)
        читают теперь `self.js_present`. Пока `Arc` ещё на UI-стороне значение
        тождественно `self.js_ctx.is_some()` в **обоих** режимах флага, поэтому срез
        **байт-идентичен** (чистый рефактор, маршрутизация не менялась —
        покрыт существующими `route_*`-тестами). No new deps, no `unsafe`. Теперь
        решение «есть ли JS?» отвязано от факта держания хэндла: следующий срез 2d
        сможет под флагом перенести сам `Arc` в `EngineJsState.js` (сеттер — в
        `engine.task`, save/restore снапшота — через `query.take()`/`task`), оставив
        `self.js_ctx == None`, а `js_present` останется верным сигналом для гейтов.
        ✅ **Двадцатый под-срез готов** (branch `p1-mt-m22d-20`, 2026-07-11):
        сняты **последние прямые Arc-читы** `self.js_ctx` на UI-потоке (кроме самих
        lifecycle-операций поля — `set_js_ctx`-присваивание, `sync_engine_js_state`-
        клон, snapshot `take`) — пролог к физическому снятию поля под флагом. 5 сайтов:
        (1)+(2) **nav-timing delivery** (reload-путь `main.rs` ~:7896 и streaming-load
        `main.rs` ~:8817) — прямой `if let (Some(js), …) { js.deliver_nav_timing(…) }`
        переведён на `self.js_present`-гейт + `route_task_js` (fire-and-forget void);
        `self.nav_start.take()` по-прежнему выполняется безусловно (как прежний кортеж),
        `url` овнится (`str::to_owned`) для `Send + 'static`-замыкания. (3) **MEM_REPORT
        heap-проба** (`debug_js_heap`, `main.rs` ~:8864) — value-read через
        `route_query_js(...).unwrap_or((-1, -1))` = прежний `map_or((-1, -1), …)`.
        (4) **per-tick pump-батч гейт** (`main.rs` ~:8951) — внешний
        `if let Some(js) = &self.js_ctx` заменён на `if self.js_present`, внутренние
        routed-вызовы получают `self.js_ctx.as_ref()`. (5) **DOM-GC tick гейт**
        (`main.rs` ~:9762) — `if let (Some(ls), Some(_js)) = …` заменён на let-chain
        `if self.js_present && let Some(ls) = self.layout_source.as_ref()`. `js_present`
        держится сеттером `set_js_ctx` в связке с `self.js_ctx`, поэтому пока `Arc` ещё
        на UI-стороне срез **байт-идентичен** в обоих режимах флага (flag-on routed-
        вызовы уже игнорировали переданный клон; nav-timing/heap под флагом теперь
        уходят off-UI-thread / блокирующим `query`). 1 новый тест
        (`route_nav_timing_and_js_heap_without_handle_default_to_no_op`). No new deps,
        no `unsafe`. **Прямых Arc-читов `self.js_ctx` вне lifecycle больше нет** —
        следующий срез 2d переносит сам `Arc` в `EngineJsState.js` под флагом
        (`set_js_ctx` → `engine.task`, snapshot save/restore → `query.take()`/`task`),
        оставляя `self.js_ctx == None`, а `js_present` — сигналом для гейтов.
        ✅ **Двадцать первый под-срез готов** (branch `p1-mt-m22d-21`, 2026-07-11):
        сам `Arc`-хэндл физически перенесён на движковый поток под флагом — три
        lifecycle-операции поля `js_ctx` (последний остаток UI-владения) переведены
        так, что при `LUMEN_ENGINE_THREAD=1` `self.js_ctx == None`, а `Arc` живёт в
        `EngineJsState::js`. (1) **`set_js_ctx`** — теперь единственная точка
        владения: под флагом кладёт хэндл в `state.js` через `engine.task`, оставляя
        `self.js_ctx = None` (маршрутизаторы под флагом и так игнорировали переданный
        UI-клон и читают `state.js`, поэтому все ~90 routed-сайтов остаются корректны);
        без флага — прежнее `self.js_ctx = handle`, байт-идентично. (2)
        **`sync_engine_js_state`** — больше **не** трогает `state.js` (иначе занулило
        бы депонированный `set_js_ctx`-ом хэндл клоном `self.js_ctx == None`); зеркалит
        только `document`. (3) **`save_page_snapshot`** — `self.js_ctx.take()` заменён
        новым `take_js_ctx()`: под флагом вынимает `Arc` из `state.js` блокирующим
        `query(|s| s.js.take())` (встаёт в очередь после park-`task` слайса 18 →
        `pause_event_loop` уже применён к тому же хэндлу), без флага — прежний
        `self.js_ctx.take()`. Снапшот держит **реальный** `Arc` даже под флагом, так
        что bg-tab GC (`run_gc_pass(1/2)`, читает `bg_tabs[id].js_ctx`) и
        `restore_page_snapshot` (→ `set_js_ctx(snap.js_ctx)`, ре-депонирует) работают
        без изменений. Также починены перепутанные doc-комменты (doc `sync_engine_js_state`
        был осиротевшим над `set_js_ctx`). Инвариант владения: `self.js_ctx.is_some()`
        ⟺ `engine_thread.is_none() && js_present`; `state.js.is_some()` ⟺
        `engine_thread.is_some() && js_present`. Без флага (по умолчанию) — всё
        байт-идентично. 1 новый тест
        (`engine_thread_query_take_extracts_and_clears_state` — `query`-take извлекает
        депонированное поле и очищает состояние, механизм `take_js_ctx`). No new deps,
        no `unsafe`. **M2.2c-2d закрыт: поле `js_ctx` под флагом на UI-потоке пусто.**
        Следующий срез — M2.2c-3 (форм-инпут/DOM-mutation relayout'ы off-thread через
        readback M2.2c-1).
    - **M2.2c-3 — route form-input / DOM-mutation relayouts off-thread.** Once
      `js_ctx` lives engine-side, the form-control and rAF-DOM-dirty sites become
      engine-thread jobs (mutate DOM → layout → deliver observers there), with any
      synchronous geometry read served by M2.2c-1's readback.
      **Site audit (2026-07-11):** the 11 direct form-input `relayout()` callers
      (`handle_click_at`, `activate_node`, `exec_spell_menu_action`) all mutate the
      shared layout `Document` directly then relayout with **no** synchronous
      geometry read afterward — Bucket A, routable exactly like M2.2b's async-safe
      `relayout_chrome`; the pre-relayout `find_box_rect` reads (color/date/select
      anchor, range-slider x→value) are against the *old* layout, which is correct.
      **Bucket B (needs the M2.2c-1 blocking `readback`) is empty for form input:**
      text typing / contenteditable / `<input>` edits never call `relayout()` inline
      — they dispatch JS events and the reflow lands in the rAF DOM-dirty flush; the
      caret is paint-derived, not read back in the handler. The one DOM-mutation site
      that *does* read a layout product synchronously after its rAF flush is
      `RedrawRequested` Step 4 (`take_dom_dirty` → Step 5 reads
      `display_list.is_empty()` for PerformancePaintTiming) — a later sub-slice
      wiring `readback` there.
      ✅ **Первый под-срез готов** (branch `p1-mt-m22c3-1`, 2026-07-11): новый
      хелпер `Lumen::relayout_form()` = `if !submit_relayout_job() { relayout() }`
      (сиблинг `relayout_chrome`, но с form-control-семантикой в доке) маршрутизирует
      **7 mouse-click form-control DOM-mutation** сайтов в `handle_click_at` с
      прямого `self.relayout()` на off-thread: checkbox/radio toggle, color-picker
      swatch commit, date-picker day commit, `<select>` option choice, `<details>`
      open toggle, range-slider value. Все семь уже применили мутацию к
      разделяемому `Arc<Mutex<Document>>` на UI-потоке (виден снимку off-thread
      job'а, инвариант 1) и **не** читают геометрию следом. Под флагом
      (`LUMEN_ENGINE_THREAD=1`) reflow уходит на движковый поток и садится через
      `poll_engine_commit` на несколько кадров позже (тот же контракт, что зум M2.2a
      и chrome-тогглы M2.2b); без флага (по умолчанию) `submit_relayout_job` → `false`
      → синхронный `relayout()`, **байт-идентично**. `<details>`-тоггл уже шлёт
      `toggle`-событие через `route_eval_js` (M2.2c-2d) — оно независимо от layout
      job'а. No new deps, no `unsafe`. Как и `relayout_chrome` в M2.2b, хелпер —
      чистая делегация в уже покрытые тестами `submit_relayout_job`/`relayout`
      (executor-тесты submit/take_committed/generation-guard), собственного
      unit-теста не требует (нет тест-харнесса `Lumen`). Остаток M2.2c-3:
      `activate_node`/`exec_spell_menu_action` form-тогглы (Bucket A, слайс 2), затем
      rAF DOM-dirty flush + paint-timing readback (Bucket B-сайт 12175).
    - **M2.2c-4 — content-visibility as a visible-range message.** Replace
      `maybe_expand_cv_relevant`'s direct `relayout()` with a visible-range message to
      the engine (never a render-thread → layout call, per the brief gotcha); make
      the ScrollTimeline step tolerate a one-frame-late cv layout.
- **M2.3 — synchronous readback + acceptance.** `--screenshot`, `run.py --ipc`,
  CDP `Page.captureScreenshot` become `Request::Readback { reply }` messages
  (audit all `screenshot_*` sites first — most are already CPU per the M1.1
  discovery). Acceptance: a 200 ms JS busy-loop no longer freezes input/scroll;
  `ENGINE_SUMMARY` p95 off the UI thread; graphic tests green; idle CPU
  unchanged (BUG-271); no new `unsafe`.

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
