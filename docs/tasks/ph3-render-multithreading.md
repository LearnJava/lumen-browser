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

**Where the boundary is (audit 2026-07-10, branch `zcode`).** BUG-171 stage 2
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

Sub-sliced (each independently shippable into `zcode`), mirroring M0/M1:

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
  `p1-mt-m2-1`, merged into `zcode`). New `crates/shell/src/engine_thread.rs`
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
    `p1-mt-m2-2`, merged into `zcode`, 2026-07-11). Made the M2.1 scaffold live.
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
    visible-range message (never render-thread → layout). Migrating
    `LayoutSource.stylesheet` to `Arc<Stylesheet>` removes the per-job stylesheet
    clone. This is where the bulk of the ~40-site conversion lands.
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
