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
- `WinitSession::click/type_text/eval` (SDC-1a, 8A.7 Ф4 driver-side, `crates/driver/src/winit_session.rs`):
  headless DOM-level semantics, distinct from `InProcessSession`'s stubs below.
  `click` resolves the `Target` to a `NodeId` and follows a real `<a href>` (via
  `navigate()`, `is_navigable_href` excludes `#`/`javascript:`/`mailto:`/`tel:`) or
  toggles `checked` on a checkbox/radio; `type_text` writes the `value` attribute
  (overwrite, not append — errors on non-input/textarea targets); `eval(js)` builds a
  one-shot `QuickJsRuntime` (`--features quickjs`) bound via `install_dom` to a
  **clone** of the current DOM (mutations from `eval` do not feed back into the
  session's own layout/paint state — that bidirectional wiring is the larger 8A.8
  migration) and returns the result as a JSON string
  (`lumen_core::ext::JsValue::to_json_string`). `AutomationCommand`/`AutomationReply`
  (`crates/driver/src/types.rs`) are the published contract shell integrates against
  (SDC-1b). Tests: `crates/driver/tests/cases/test_automation_commands.rs`.
- `AutomationHandle`/`AutomationRequest` (`crates/driver/src/automation.rs`, SDC-2): the
  request/reply half SDC-1b was missing — `Lumen`'s automation channel used to fan every
  reply out through one `Sender<AutomationReply>` whose receiver was immediately dropped
  (`let (automation_reply_tx, _) = channel()` in `main.rs`), so callers outside the shell
  process had no way to read a result. `AutomationRequest = (AutomationCommand,
  Sender<AutomationReply>)` carries its own one-shot reply channel per call;
  `AutomationHandle::execute(cmd, timeout)` sends it and blocks on `recv_timeout`. `main()`
  now builds the channel before dispatching to any CLI mode (not inside `run_window_mode`),
  so `--bidi-port`/`--mcp-live-port` front-ends spawned earlier already hold a valid handle.
  `execute()` also invokes an optional wake callback (`set_wake`, `WakeFn = Arc<dyn Fn() +
  Send + Sync>`, shared via `Arc<Mutex<..>>` so it's visible to clones handed out before the
  callback is attached) after enqueueing — without it, a command from a BiDi/MCP thread has
  no way to interrupt a parked `winit::ControlFlow::Wait` event loop (an `mpsc` send isn't an
  OS event/timer/redraw), so it could sit undrained indefinitely. The shell attaches the real
  callback (`EventLoopProxy::send_event(LoadEvent::AutomationWake)`) once its event loop
  exists — see `subsystems/shell.md` SDC-3 entry.
- `LiveWindowSession` (`crates/driver/src/live_session.rs`, SDC-2): a full `BrowserSession`
  impl over `AutomationHandle` — same trait `InProcessSession`/`WinitSession` implement, so
  `lumen-bidi-server` and `lumen-mcp` drive a real window with no protocol-specific glue.
  Real round-trips: `navigate`/`click`/`type_text`/`scroll`/`wait`/`eval`/`screenshot`/
  `query`/`a11y_tree`/`console_log` (+ `query_a11y`/`query_a11y_all`, composed locally from
  `a11y_tree()`). `AutomationCommand` gained `Query(String)`/`A11yTree`/`ConsoleLog` variants
  for this (shell-side handlers reuse `lumen_layout::selector_query::find_all_by_selector` and
  `lumen_a11y::build_ax_tree`, same helpers `resolve_automation_target`/
  `update_platform_ax_tree` already used; `ConsoleLog`, DEVX-1, reads the shell's DevTools
  console-panel buffer — `devtools::console_panel::ConsolePanel::messages()` — cleared on
  every `Navigate` so `console_log()` reflects only the current page). `current_url()` changed
  from `&str` to owned `String` across the whole `BrowserSession` trait (all implementors
  updated) — the old borrow-tied signature would have forced `LiveWindowSession` to leak
  memory on every read to satisfy the lifetime. Layout/computed-style/network-log/
  fingerprint-isolation methods are local stub defaults (documented per-method) — the
  automation channel doesn't carry those commands yet.

- DEVX-5: `InProcessSession::click/type_text/scroll/eval/screenshot` (cargo features
  `v8-backend`/`cpu-render`, both on by default in `lumen-shell`) — the headless MCP
  (`--mcp`/`--mcp-port`) stubs from the table above. `SessionState.doc` changed from a plain
  `Document` to `Arc<Mutex<Document>>` (`crates/driver/src/session.rs`) so it can be shared
  with a **persistent** `lumen_js::v8_runtime::V8JsRuntime` installed via `install_dom` once
  per `navigate()`/`run_pipeline()` call and stored on `SessionState.js` (cfg-gated on
  `v8-backend`) — every method that previously borrowed `&state.doc` now locks the mutex
  first. `click`/`type_text` port `WinitSession`'s DOM-level semantics verbatim (own copies of
  `resolve_target_node`/`set_attr`/`remove_attr`/`resolve_href`/`is_navigable_href` — same
  per-session-type duplication convention as the rest of this file, not shared with
  `winit_session.rs`); `scroll` now calls the existing `scroll_page_by` instead of a no-op.
  `eval(js)` is the key difference from `WinitSession::eval`'s **one-shot** runtime: because
  `state.js` is created once and reused for every `eval()` call in the same navigation, and it
  shares the same `Arc<Mutex<Document>>` that `click`/`type_text` mutate, a DOM mutation from
  one `eval()` call (or a prior `click`/`type_text`) is visible to the next `eval()` call —
  proven by `eval_mutations_persist_across_calls` in
  `crates/driver/tests/cases/test_devx5_headless_automation.rs`. `screenshot()` is cfg-gated:
  with `cpu-render` it calls `screenshot_cpu_png()` (no GPU adapter needed — this is what makes
  headless MCP `screenshot` work without a window); without it, the old GPU `Renderer::new_headless`
  path is unchanged. None of this triggers a relayout — `layout_root`/`flat_tree` stay fixed
  from `navigate()` time, so `layout_snapshot`/`computed_style*`/`screenshot` don't reflect
  DOM mutations made via `click`/`type_text`/`eval` (matches `WinitSession`'s existing
  limitation; full interactive-JS-driven relayout is a separate, much larger slice).
  `crates/shell/Cargo.toml`: `v8`/`quickjs` features now also enable
  `lumen-driver/v8-backend`/`lumen-driver/quickjs` respectively, and `lumen-driver` always
  gets `cpu-render` — so a plain `cargo build -p lumen-shell` (default features) ships a
  headless MCP that Just Works.

- DEVX-2: non-pixel golden regression layer (`crates/driver/tests/cases/test_devx2_golden.rs`),
  modeled on `graphic_tests` but asserted through `BrowserSession` (`layout_box_by_selector`,
  `computed_style_snapshot`, `query_a11y`/`query_a11y_all`) instead of pixel diffing — runs via
  `cargo test -p lumen-driver`, no GPU/Edge. Fixtures: `crates/driver/tests/fixtures/golden-*.html`
  (container/flex geometry, cascade specificity + inheritance, form-control accessible roles).
  Surfaced [BUG-294](../bugs/BUG-294-OPEN.md) (flex-item `margin-left` double-applied in
  `lay_out_flex`'s row branch) — the fixture uses `gap` instead of per-item `margin` to avoid
  baking that bug into the golden baseline.

## Deferred

- `WinitSession::eval` without `--features quickjs` still errors.
- Native OS-level input dispatch (isTrusted mouse/keyboard events) for click/type — that's the live shell window's job (SDC-1b), not this headless session.
- Full auto-wait (`WaitCondition::Visible/Stable/NetworkIdle/JsIdle`) beyond `WinitSession::wait`'s existing poll loop — task 8D refinements.
- `LiveWindowSession`'s `layout_snapshot`/`computed_style(_snapshot)`/`layout_box_by_selector`/
  `all_layout_boxes_by_selector`/`network_log`/fingerprint-isolation methods — local stub
  defaults, not yet threaded through `AutomationCommand` (SDC-2 MVP scope). `console_log` is
  real as of DEVX-1 (see above) — no longer in this list.
- Remote transport (BiDi over WebSocket) — live navigate/eval/captureScreenshot/input done
  (SDC-2); network interception, cookie/storage events, `domContentLoaded` remain 8H.3.
- CSS selector: descendant/child combinators, pseudo-classes — when needed.

## Invariants

- `InProcessSession` is single-threaded (`!Send`-interior `FontMeasurer` lifetime).
- `InProcessSession::screenshot()`: with `cpu-render` (default in `lumen-shell`) calls `screenshot_cpu_png()` — deterministic, no GPU adapter needed; without it, falls back to the GPU `Renderer::new_headless` path (needs a real adapter — fine for `cargo test -p lumen-driver` with no extra features, since that path is exercised there today).
- `InProcessSession` never opens a winit window or wgpu surface — keeps it usable in CI without GPU (with `cpu-render` on; without it, `screenshot()` still needs a wgpu adapter, see above).
- `navigate()` clears `net_log` and `con_log` — callers must read logs before next navigate.
- `screenshot_cpu_rgba/png` (feature `cpu-render`): renders through the deterministic tiny-skia CPU path for cross-OS pixel-identical snapshots.
- `SessionState.doc: Arc<Mutex<Document>>` (DEVX-5) — every read/write site locks it; the lock is never held across a `navigate()`/`self.` call that could re-enter (`resolve_target_node` and friends take `&Document` directly, not `&SessionState`, so they can run under an already-held lock without deadlocking).
- `driver/tests/snapshot_cpu.rs` (feature `cpu-render`): pixel-compares all 57 graphic_tests pages against committed references in `graphic_tests/snapshots/cpu/`. Regenerate: `SAVE_CPU_SNAPSHOTS=1 cargo test -p lumen-driver --features cpu-render`.
- `driver/tests/test_00..49.rs`: 50 structural-assert integration tests.

## Test counts

12 unit tests in `crates/driver/src/session.rs`; 50 structural integration tests `test_00..49.rs`; 1 snapshot gate `snapshot_cpu` covering 57 pages; 5 (+2 under `--features quickjs`) `WinitSession` automation-command tests in `test_automation_commands.rs`; 6 (+3 under `--features v8-backend`, +1 under its absence) `InProcessSession` automation-command tests in `test_devx5_headless_automation.rs`.
