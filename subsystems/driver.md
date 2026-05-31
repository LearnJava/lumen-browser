# lumen-driver

Programmatic interface to the Lumen engine. Enables level 2–3 testing (lumen-plan.md §15):
headless pipeline without winit/wgpu/ffmpeg.

## Done

- `BrowserSession` trait: 6 resources (screenshot, a11y_tree, layout_snapshot, computed_style,
  network_log, console_log) + 6 tools (navigate, click, type_text, scroll, wait, eval, query).
- `InProcessSession`: full headless pipeline (encoding → HTML parse → CSS cascade → layout)
  using bundled Inter-Regular. Shares engine crates directly, no IPC.
- Simple CSS selector engine: tag, `#id`, `.class`, `tag.class`, multi-class combinations.
- `A11yNode` tree built from ARIA role mapping of HTML tags.
- `BoxModel` list from layout tree walk (border-box + margin-box).
- `ComputedProperties` map extracted from `ComputedStyle` fields.
- 12 unit tests (session + selector parser).

## Deferred

- `screenshot()` — returns Err until task 8A.5 (tinyskia-cpu-raster CPU renderer).
- `eval(js)` — returns Err until task 8A.7 integrates persistent JS runtime.
- Full auto-wait (`WaitCondition::Visible/Stable/NetworkIdle/JsIdle`) — task 8D.
- Native input injection for click/type — task 8C.
- Remote transport (BiDi over WebSocket, MCP over stdio) — tasks 8B / 8H.
- CSS selector: descendant/child combinators, pseudo-classes — when needed.

## Invariants

- `InProcessSession` is single-threaded (`!Send`-interior `FontMeasurer` lifetime).
- `screenshot()` always returns Err in headless mode — do not change without task 8A.5.
- No winit/wgpu dependency in this crate — keeps it usable in CI without GPU.
- `navigate()` clears `net_log` and `con_log` — callers must read logs before next navigate.

- `screenshot_cpu_rgba/png` (feature `cpu-render`): renders through the deterministic tiny-skia CPU path for cross-OS pixel-identical snapshots.
- `driver/tests/snapshot_cpu.rs` (feature `cpu-render`): pixel-compares 34 geometry pages against committed references in `graphic_tests/snapshots/cpu/`.
- `driver/tests/test_00..49.rs`: 50 structural-assert integration tests.

## Test counts

12 unit tests in `crates/driver/src/session.rs`; 50 structural integration tests `test_00..49.rs`; 1 snapshot gate `snapshot_cpu` covering 34 pages.
