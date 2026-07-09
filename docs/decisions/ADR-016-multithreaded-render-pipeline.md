# ADR-016: Multithreaded render pipeline (compositor/render thread)

## Status

Accepted

## Date

2026-07-09

## Context

Today the entire frame path — input, JS dispatch, style, layout, display-list
build, rasterization, present — runs on the single UI/winit thread
(`crates/shell/src/main.rs`, `RedrawRequested`). The consequences, confirmed by
code audit (2026-07-09):

- **Every scroll frame re-rasterizes the full display list.** Scroll only
  mutates `scroll_x/scroll_y` and requests a redraw, but the femtovg backend
  then clears the whole surface and re-executes every content command with a
  new translate (`femtovg_backend.rs::render`). The existing whole-frame hash
  skip and dirty-rect scissor are both inert during scroll.
- **Zoom is a full relayout** (viewport shrink via `zoom_factor`), with no
  interim scale transform — Ctrl+/- latency equals a whole-tree layout.
- **Any main-thread stall (JS, layout, parse) freezes presentation.** Momentum
  scrolling, CSS animations and rAF all tick inside `RedrawRequested` on the
  same thread.

Infrastructure for a better model already exists but is unwired:
`ThreadedCompositor` + `CompositorThread` with a vsync tick-loop
(`crates/engine/paint/src/compositor.rs`, P2 1B.1/1B.2 scaffolding, zero shell
consumers), `TileGrid` (updated on relayout, never read), incremental layout
(`DirtyBits` / `lay_out_incremental`, never called by the shell), and
`DisplayListCache` (populated, never consumed per-subtree). ADR-014 already
moved the QuickJS runtime to its own thread, making JS callable from non-UI
threads.

User decision 2026-07-09: the transition to a multithreaded pipeline is
**mandatory and urgent** — smooth scroll/zoom must not depend on engine work.

## Decision

Adopt a staged multithreaded render pipeline built on message passing with
immutable snapshots. Full working plan: `docs/tasks/ph3-render-multithreading.md`.

**Target thread model:**

| Thread | Owns | Never does |
|---|---|---|
| **Main (winit)** | OS events, window lifecycle, chrome UI state, forwarding input to other threads | Rasterization; whole-tree layout (after M2) |
| **Render/compositor thread** | GPU/GL context, render backend, scroll/zoom transform state, momentum & smooth-scroll animation ticks, present | Layout, style, JS, DOM access |
| **Engine thread(s)** (M2+) | Parse, style, layout, display-list build; commits snapshots | Touching the GPU context |
| **JS thread** (exists, ADR-014) | QuickJS runtime | — |
| **Raster workers** (M3+) | Tile rasterization | — |

**Synchronization rules (invariants):**

1. Cross-thread data is **immutable snapshots** (`Arc<DisplayList>`,
   `Arc<PropertyTrees>`) — never shared mutable structures. No locks are held
   across rasterization or layout.
2. Frame state uses **latest-wins commit semantics** (bounded depth 1,
   coalescing): a slow consumer drops stale frames, it never queues them.
3. Scroll/zoom are small copyable values applied **render-side as a
   transform**; committing them never waits for the engine.
4. **The render thread never waits for the engine; the engine never waits for
   the render thread** (exception: explicit synchronous readback such as
   `--screenshot`, which uses a request/reply message).
5. **Scroll never waits for rasterization.** If content for a region is not
   ready, show a placeholder (checkerboarding) and fill it in a later frame.
6. Idle means idle: with no active animation and no pending commit, every
   thread parks on a condvar (no 16 ms polling), preserving the ~0% idle-CPU
   invariant from BUG-271.

**Stages** (independently shippable, in order): M0 quick wins + metrics
(viewport culling, transform-first zoom, frame-time histogram) → M1 render
thread (move backend + present off main, reuse `CompositorThread` scaffolding)
→ M2 engine off main (non-blocking load pipeline, BUG-171) → M3 tile raster
workers (`TileGrid` revival, checkerboard invariant) → M4 parallel style/layout
(rayon over the incremental-layout dirty set).

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Stay single-threaded, optimize per-frame work only (culling, blit, caches) | Necessary but not sufficient: any JS/layout stall still freezes presentation and input feedback; does not scale to heavy pages. Kept as stage M0 — it shrinks the work the threads will do. |
| Big-bang rewrite to the full Chromium-style multi-process model (architecture.md §3) | Too large to land safely; process isolation is orthogonal to smoothness. The staged thread model is a stepping stone toward it — snapshots-over-channels translates directly to IPC later. |
| Shared-state model (locks around `Lumen` fields, render thread reads them) | Guaranteed contention and deadlock surface across a 17k-line shell monolith; violates the snapshot invariant; impossible to reason about incrementally. |
| Move winit event loop off main instead of the backend | Not portable — on Windows (and macOS) the OS event loop must run on the process main thread. |
| Jump straight to vello/wgpu compute raster for speed | vello backend is a stub (ADR-010 Phase 3) and the wgpu path has an open idle-CPU/memory bug (BUG-274); threading is independent of backend choice and must not wait for it. |

## Consequences

- **Positive:** presentation (scroll offset, momentum, CSS/compositor-offload
  animations) survives main-thread stalls; scroll frames stop costing a full
  re-raster once M0/M3 land; the dormant compositor scaffolding, `TileGrid`,
  and incremental-layout code gain their intended consumers; the snapshot
  message model is the on-ramp to the Phase 3 multi-process architecture.
- **Negative / trade-offs:** GL context ownership moves to the render thread
  (femtovg `Canvas` is `!Send` — create and use it there only); every commit
  costs a channel hop; synchronous paths (screenshot, IPC acceptance, CDP
  captures) need explicit request/reply plumbing; debugging spans threads
  (frame logs must carry thread + commit ids).
- **Future:** M2 closes BUG-171; the M1 render loop is the natural home for a
  fixed BUG-274 wgpu backend and later vello; when renderer-per-origin
  processes arrive (Phase 3+), the commit channel becomes the IPC boundary.
