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

### Slice 1 probe results (2026-10-06)

**How `position:fixed` stays pinned today.** Nobody relayouts per scroll step.
The display list is scroll-independent: `walk` brackets a fixed box with
`BeginFixedLayer`/`EndFixedLayer` (`display_list/walk.rs`), `overlay_partition`
splits those ranges into the `overlay` list, and the renderer applies the page
offset only to the `content` list (`renderer.rs:1363`: `is_overlay` → `(0, 0)`,
else `(-scroll_y, -scroll_x)`; the offset seed is dropped at the content|overlay
boundary, `renderer.rs:1329`). The markers themselves are no-ops
(`renderer.rs:3438`) — they are partition metadata, so the comment in
`commands.rs:453` ("already in viewport coordinates") means "coordinates of the
overlay list, which is never shifted". Consequence for rule 4: the render thread
only needs the offset; fixed/sticky pinning already happens at draw time from
the brackets. A fixed box under a transform/filter ancestor is deliberately not
bracketed (page content). Rule 4 relies on balanced brackets: BUG-1037 (early
`return` of an invisible fixed/sticky replaced element) is fixed in this slice
(`close_position_layers`, regression test in `tests/fixed_cb_scroll.rs`).

**`ActiveEventLoop` uses** (all of them; everything else in the 14 files that
mention the type only passes it through):

| Use | Where | Plan for slice 2 |
|---|---|---|
| `exit()` | `app/about_to_wait.rs:36`, `app/mod.rs:213`, `app/resumed.rs:39,137`, `lumen/keyboard.rs:621`, `lumen/tabs_cmd.rs:212`, `update_ui.rs:360` | `BrowserRequest::Exit` through the proxy |
| `create_window` | `app/resumed.rs:35` (main window), `lumen/pip.rs:85,144,266` (PiP, document PiP) | Main window is created on the main thread *before* the browser thread starts and handed over as `Arc<Window>`; PiP windows: request → main thread creates → `Arc<Window>` returned by channel |
| `set_control_flow` | `app/about_to_wait.rs:543,551,1314,1316` | Browser thread computes the wake-up and sends it; main thread applies `WaitUntil`. Cheapest form: browser thread owns its own timer and the main thread stays in `Wait` |
| `create_proxy` | `main.rs`, `window_mode.rs` | Cloned to the browser thread at spawn |
| `run_app` | `window_mode.rs:679` | Stays on the main thread; `ApplicationHandler` becomes a forwarder |

**Other main-thread affinities** (all reached through `Arc<Window>` or free
functions, none through `ActiveEventLoop`):

| Affinity | Where | Verdict |
|---|---|---|
| Clipboard | `platform/clipboard.rs` — raw Win32 `OpenClipboard`, `pbcopy`, `wl-copy` | Not tied to the UI thread on any platform (no `arboard`); safe on the browser thread |
| File dialog | `platform/file_dialog.rs` — Win32 call | Blocking; moves with the browser thread, which is better than blocking the pump |
| IME | `lumen/text_input.rs:433-465` (`Ime::*` events in, `set_ime_*` out) | Events arrive on main and are forwarded in order (rule 2); `Window::set_ime_*` is `Send + Sync` in winit 0.30 |
| Cursor icon / grab (pointer lock) | `lumen/cursor.rs`, `app/about_to_wait.rs:1662`, `lumen/keyboard.rs:214` | `Window` methods; callable from the browser thread. **Unverified on macOS** (winit dispatches to the main thread internally — cost to be measured, not assumed) |
| DPI change | `app/mod.rs:143,195,260` (`ScaleFactorChanged`) | Event forwarded; `scale_factor` reads from `Arc<Window>` |
| `request_redraw`, `set_title`, `set_fullscreen`, `drag_window`, … | ~40 files, all via `Arc<Window>` | Direct calls from the browser thread; no proxy needed |
| `EventLoopProxy::send_event` (`LoadEvent`) | `page_load.rs`, `frames.rs`, `dynamic_image_hook.rs`, `app/user_event.rs` | Already thread-safe; `user_event` handler moves to the browser thread |
| Raw window handle (GPU surface, startup trace) | `app/resumed.rs`, `renderer_process.rs` | Created on main at window creation, owned by the render thread already (ADR-029) |
| Automation / BiDi / MCP | `lumen/automation.rs`, `about_to_wait.rs:966,1347` | Polled from `about_to_wait` today; polling moves to the browser thread unchanged |

**Plan for slice 2.**
1. `main` creates the window in `resumed` as today, then spawns the browser
   thread, which constructs `Lumen` from `Arc<Window>`, a `Receiver<UiMsg>` and a
   proxy clone (rule 1).
2. `ApplicationHandler::window_event`/`device_event` on main only wrap and send
   `UiMsg::Window(event)` (wheel included in this slice — behaviour-identical).
3. `ActiveEventLoop` call sites listed above are replaced by a small
   `MainRequest` enum sent back through the proxy (`Exit`, `CreatePipWindow`,
   `WakeAt`); `&ActiveEventLoop` parameters in the 14 files disappear from
   signatures and are replaced by a `&MainHandle` (channel + proxy).
4. `about_to_wait` logic runs as the browser thread's loop (`recv_timeout` against
   the wake-up deadline), so idle CPU stays ~0% (ADR-016 invariant 6).
5. `LUMEN_NO_BROWSER_THREAD=1` keeps the old in-place path (rule 6).
6. Risk to measure first inside slice 2: the macOS cost of `Window` calls made
   off the main thread, and event ordering of `RedrawRequested` (must be
   forwarded, not dropped).

### Slice 2 result (2026-10-06)

Landed as planned (`browser_thread.rs`: `UiMsg`, `MainHandle`, `MainForwarder`),
with one correction to the probe: **winit 0.30 on Windows returns
`window_handle()` only on the thread that created the window**, so the GPU
surface cannot be built on the browser thread from `Arc<Window>`. The main
thread now snapshots the raw handles at window creation
(`lumen_paint::SurfaceWindow`, also for PiP windows), and femtovg builds its
surface attributes from them instead of `GlWindow`. Idle CPU on
`01-sanity.html`: 109 ms/10 s with the browser thread vs 234 ms without.
Graphic tests: identical results in both modes (11 failures, all pre-existing).
macOS behaviour of off-main `Window` calls is still unmeasured.

### Slice 3 result (2026-10-06)

Landed (`wheel_scroll.rs`, `lumen/scroll_route.rs`, `render_thread.rs`). Where it
differs from the plan above:

- **Who routes.** `MainForwarder` tracks the cursor and Shift itself (it sees
  `CursorMoved`/`ModifiersChanged` first) and decides per wheel event against the
  published `ScrollSnapshot`; a touchpad gesture keeps one sink from `Started`
  to `Ended`. Wheel over chrome, a panel, split view, a page with
  `scroll-snap`, an overflow container or an iframe is *not* routed — those
  stay on the browser thread until slice 4 (`blockers` in the snapshot, plus a
  reserved empty `wheel_listeners`, rule 9).
- **Who owns the offset.** The render thread keeps `cur_y/cur_x` and a `gen`
  counter while it drives a wheel curve or touchpad momentum (`owned`). Every
  change is posted back (`ScrollShared::post_feedback`, latest wins, one wake-up
  per pending value); the browser thread adopts it
  (`Lumen::adopt_scroll_feedback`) and redraws.
- **Epoch rule (7), in this slice's form.** A frame commit carries `ack_gen`:
  the generation the browser thread had adopted. `ThreadedRenderBackend` derives
  it at commit time; if the offset the browser passes differs from what it
  believes it has (navigation, keyboard, `scrollTo`, MCP `scroll`), the frame is
  tagged `ACK_BROWSER_SET` and wins over the render thread's offset. Frames with
  an older `ack_gen` are drawn at the render thread's offset. So programmatic
  sources still *write* `scroll_y` on the browser thread — the "command with an
  epoch" form of rule 7 for them (and for `AutomationCommand::Scroll` /
  `InputCommand::Scroll`) is the next step, with the async consumers of
  slice 5.
- **Pacing.** The render thread ticks a driven curve itself and presents the
  last frame at the new offset; browser frames with an unchanged display list
  only refresh the overlay while a curve runs (a second present per tick halved
  the cadence). New lists are still presented at once — skipping versions cost
  ~90 ms full repaints on lenta.ru. Ticks follow `render()` (it blocks on
  vsync) with an 8 ms floor; a fixed 16.7 ms timer drifted against vsync and gave
  "3 ms, 30 ms" pairs. A curve started from rest gets an 8 ms head start,
  because `recv_timeout` on Windows quantises to ~15.6 ms (first present was
  23 ms after the click, against 8 ms before).
- **Measured** (`scripts/scroll_smoothness_run.py`, 60 Hz, `--maximized`,
  ad-block off, 30 clicks): a 2000-row local page — on-time 0.92–0.97 vs
  0.91–0.94 with `LUMEN_NO_WHEEL_ROUTE=1`, click→present latency 2–3.5 ms vs
  7–9 ms, no jerks in either. lenta.ru (3×3 runs): routing off presents only
  46 frames for 30 clicks (the busy UI thread eats the curve), routing on
  presents ~140 — the full 200 ms curve per click — at 16 ms cadence; the
  remaining ~90 ms gaps are post-scroll browser frames repainting new content,
  not scroll. Scroll position after wheel, MCP `scroll` and another wheel click
  agrees with the old path (400 → 500 → 540). Graphic tests: 11 failures, same
  as slice 2. Not measured: ria.ru/rbc.ru with an artificially stalled browser
  thread (the sites are too noisy for two runs; slice 6) and a display above
  60 Hz.

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
