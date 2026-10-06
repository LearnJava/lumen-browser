# ADR-032: Thin main thread — wheel input and page scroll never wait for the UI thread

## Status

Accepted

## Date

2026-10-06

## Context

ADR-016 invariant 3 says scroll is applied render-side and never waits for the
engine. In practice it still waits for the **UI thread**: winit delivers
`MouseWheel` on the process main thread, and that thread is also where the whole
`Lumen` shell state lives (`crates/shell/src/lumen/state.rs:15`, owned by value by
`event_loop.run_app` at `crates/shell/src/window_mode.rs:679`). Every wheel
decision — chrome-panel interception, scroll-chain hit-test, page vs overflow
container, clamp, snap — runs there (`app/window_event/mouse_wheel.rs:10`,
`lumen/scrolling.rs:57`, `:604`).

THREAD-6 moved what it could to the render thread (notch curve, touchpad
momentum, `frame_scroll_y` override — `render_thread.rs:601`, `:634`, `:762`),
but the render thread only *continues* a scroll the UI thread already started. A
wheel notch that arrives while the UI thread is busy waits for it. The UI thread
is busy often and for reasons that keep changing: on-thread M4 restyle 50–90 ms
per tick on lenta.ru (THREAD-10), `apply_relayout_result`, stalls of 55–80 ms in
the `js` phase, `build: chrome` ≈ 1.8 s on ria.ru's first frame (THREAD-12),
blocking engine queries (THREAD-9). Each of these was or is being fixed one by
one. THREAD-5 metrics remain below the 0.95 "on time" threshold on lenta/ria in
every M4 mode (journal 2026-10-06, THREAD-10). Chromium meets it because its
compositor thread scrolls without asking the page's main thread. Lumen cannot
reach "not worse than Chromium under a busy engine" (THREAD-5/6/10 criterion)
while scroll waits for the thread that does everything else.

ADR-016 rejected "move the winit event loop off main": on Windows and macOS the
OS event loop must run on the process main thread. That constraint stands.

## Decision

Keep the event loop on the main thread and **move the shell state off it**.

| Thread | Owns | Never does |
|---|---|---|
| **Main (winit), thin** | OS event pump; `ActiveEventLoop` operations (create window, exit, control flow) on request; **page-viewport wheel/touchpad routing** against a published scroll snapshot | Style, layout, JS, chrome build, any wait on another thread |
| **Browser thread** (new) | All of today's `Lumen` state: chrome, tabs, navigation, relayout apply, click/keyboard handling, JS scroll events | Pumping OS messages |
| **Render thread** (ADR-029) | **Authoritative page and scroll-container offsets**, notch curve, momentum, clamp, snap, present | Layout, style, JS |
| Engine / JS / raster | unchanged (ADR-016, ADR-023) | — |

Rules:

1. `Lumen` is **constructed on the browser thread** and never crosses threads —
   it does not have to become `Send`. Only `Arc<Window>` (`Send + Sync` in winit
   0.30), channels and `EventLoopProxy` cross.
2. Main thread → browser thread: every `WindowEvent` except page-viewport wheel,
   forwarded in order. Browser thread → main thread: `ActiveEventLoop`
   operations as user events through the proxy.
3. The browser thread publishes an immutable `ScrollSnapshot` (`Arc`, latest
   wins) after every relayout: scroll containers with stable ids, clip rects,
   max offsets, scroll chain and `overscroll-behavior`, snap points, and the
   rects where chrome intercepts the wheel. The main thread hit-tests the wheel
   against it and sends the delta straight to the render thread. A wheel over
   chrome goes to the browser thread as today.
4. The render thread owns offsets. Scroll layers carry an id
   (`PushScrollLayer` today is matched by bit-exact `clip_rect`,
   `display_list/scrollbars.rs:277`). Sticky and fixed content stay pinned at
   draw time (overlay ranges, `overlay_partition.rs`).
5. The render thread reports offsets back to the browser thread (latest wins).
   JS `scroll`/`scrollend`, scroll-progress timelines, content-visibility, lazy
   images and IntersectionObserver catch up asynchronously — the last value
   wins, as in Chromium.
6. Rollback lever `LUMEN_NO_BROWSER_THREAD=1` (same idiom as ADR-029) until the
   default has a live measurement.
7. **One writer of the offset — the render thread.** Every other source sends it
   a command instead of writing `scroll_y`: `scroll_to(target, smooth)` /
   `scroll_by(delta, smooth)` tagged with a monotonically increasing epoch.
   Today these sources write `scroll_y` on the UI thread directly:
   - JS `scrollTo`/`scrollBy`/`scrollIntoView` — drained in
     `about_to_wait.rs:1905`;
   - keyboard (`keyboard.rs:597`, vim keys `:144`), scrollbar drag
     (`cursor_moved.rs:195`) and track click (`mouse_input.rs:890`);
   - find-in-page (`find_bar.rs:87`, `find.rs:136`);
   - fragment/anchor navigation and scroll restore (`page_load.rs`), bfcache,
     hibernation, session restore, tab switch;
   - iframes (`frames.rs`);
   - scroll anchoring and `scroll-initial-target` (BUG-524, BUG-944);
   - automation: MCP `AutomationCommand::Scroll` (`about_to_wait.rs:966`) and
     `InputCommand::Scroll` (`:1347`).

   The browser thread updates its own copy optimistically when it issues a
   command, so `scrollY` read right after `scrollTo` returns the new value
   (CSSOM View; BUG-949 already shows ~500 ms lag). Feedback carrying an older
   epoch is dropped and never overwrites a newer programmatic target.
8. Scroll-chain, snap and `overscroll-behavior` resolution
   (`resolve_scroll_chain_target`, `lumen/scrolling.rs:31`, `:511`) become pure
   functions over `ScrollSnapshot`, so the main thread (wheel) and the browser
   thread (keyboard, programmatic scroll) use the same code and give the same
   answers.
9. `ScrollSnapshot` reserves a field for regions with a non-passive `wheel`
   listener (BUG-865). JS `wheel` events are not dispatched today. Once they are,
   a wheel over such a region goes to the browser thread and waits for
   `preventDefault`, as in Chromium; everywhere else it stays on the fast path.
10. `background-attachment: fixed` keeps today's fallback: the band compositor
    renders directly (`CAPABILITIES.md` §backgrounds). It is not pinned by rule 4.

Work item: `ROADMAP.md` THREAD-13. Slices, in order:

1. **Probe**: how `position:fixed` stays pinned today (paint treats
   `BeginFixedLayer` as a no-op, `renderer.rs:3438`); inventory of
   `ActiveEventLoop` uses (8 files in `crates/shell/src`) and of other
   main-thread affinities (clipboard, IME, cursor, dialogs, DPI change).
2. **Browser thread, behaviour-identical**: main thread forwards all events,
   wheel included; `ActiveEventLoop` operations go through the proxy.
3. **Page scroll on the render thread**: `ScrollSnapshot`, wheel routing on the
   main thread, offset feedback to the browser thread.
4. **Overflow containers**: scroll-layer ids, per-container offsets on the
   render thread.
5. **Asynchronous consumers** of the offset (rule 5).
6. **Measurement and default**: THREAD-5 metrics with a busy engine against
   Chromium on ria/lenta/rbc.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Shrink UI-thread work only (M4 budget ≈ 8 ms, cheaper/parallel restyle, THREAD-7/9/12) | Each fix removes one cause of stalls; the next heavy site brings another. None reaches 0.95 on lenta/ria (THREAD-10). Still worth doing for click/input latency, which this ADR does not touch. |
| Event loop on a secondary thread (`with_any_thread`, Windows-only) | Works on Windows, not on macOS; leaves two event-loop topologies to maintain. The thin-main design gives the same result on every platform. |
| Win32 hook / subclassing to catch `WM_MOUSEWHEEL` before winit | Platform-specific, fights winit's wndproc ownership, and still needs the scroll decision state off the UI thread — i.e. this ADR plus a hack. |
| Hit-test the wheel on the render thread instead of the main thread | Rejected in THREAD-6 slice 8: the event arrives on the main thread anyway. Once the main thread is thin it is as fast as the render thread, and keeping routing there avoids a second hop. |
| Separate input process | Process isolation is orthogonal to smoothness; far larger than needed. |

## Consequences

- **Positive:** wheel and touchpad scroll stay smooth whatever the browser or
  engine thread is doing — restyle, relayout apply, JS, first-frame chrome
  build. The 2026-10-06 M4 routing debate (THREAD-10) stops affecting scroll.
  The model matches Chromium's browser / renderer-main / compositor split and
  is the natural partner of THREAD-11 (tile raster ahead of the scroll).
- **Negative / trade-offs:** scroll-linked effects driven by JS (`scroll`
  events, scroll timelines) lag by at least one frame. A wheel over a
  container added since the last snapshot scrolls its ancestor for up to one
  commit. Every `ActiveEventLoop` operation becomes an asynchronous request.
  Debugging spans one more thread. Offsets keyed by scroll-layer id must
  survive a relayout; a fresh layout tree that resets offsets to 0 is a known
  failure mode (BUG-1215). Once slice 3 lands, the MCP `scroll` round-trip used
  by BUG-935 and by `input_perf.py`/`scroll_perf.py`/`mt_stall_bench.py`
  measures the command path, not the user's wheel. The wheel metric is THREAD-5
  (`SendInput`).
- **Amends:** the ADR-016 thread table — chrome UI state moves from Main to the
  browser thread.
- **Future:** close THREAD-13 when THREAD-5 metrics with a busy engine are not
  worse than Chromium on ria/lenta/rbc and idle CPU stays ~0% (ADR-016
  invariant 6). Then remove `LUMEN_NO_BROWSER_THREAD` after one release, as
  with ADR-029.
