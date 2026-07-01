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

## Deferred

- `InProcessSession::screenshot()` — returns Err; use `screenshot_cpu_rgba/png` (feature `cpu-render`) for deterministic CPU snapshots. GPU readback path planned for 8A.5+.
- `InProcessSession`'s own `click/type_text/scroll/wait/eval` remain no-op/Err stubs — no persistent JS runtime wired there (unlike `WinitSession`, above).
- `WinitSession::eval` without `--features quickjs` still errors.
- Native OS-level input dispatch (isTrusted mouse/keyboard events) for click/type — that's the live shell window's job (SDC-1b), not this headless session.
- Full auto-wait (`WaitCondition::Visible/Stable/NetworkIdle/JsIdle`) beyond `WinitSession::wait`'s existing poll loop — task 8D refinements.
- Remote transport (BiDi over WebSocket) — task 8H.
- CSS selector: descendant/child combinators, pseudo-classes — when needed.

## Invariants

- `InProcessSession` is single-threaded (`!Send`-interior `FontMeasurer` lifetime).
- `InProcessSession::screenshot()` always returns Err — use `screenshot_cpu_rgba/png` (feature `cpu-render`). `WinitSession::screenshot()` is a separate implementation and does return real PNG bytes (headless GPU-path renderer).
- No winit/wgpu dependency in this crate — keeps it usable in CI without GPU.
- `navigate()` clears `net_log` and `con_log` — callers must read logs before next navigate.
- `screenshot_cpu_rgba/png` (feature `cpu-render`): renders through the deterministic tiny-skia CPU path for cross-OS pixel-identical snapshots.
- `driver/tests/snapshot_cpu.rs` (feature `cpu-render`): pixel-compares all 57 graphic_tests pages against committed references in `graphic_tests/snapshots/cpu/`. Regenerate: `SAVE_CPU_SNAPSHOTS=1 cargo test -p lumen-driver --features cpu-render`.
- `driver/tests/test_00..49.rs`: 50 structural-assert integration tests.

## Test counts

12 unit tests in `crates/driver/src/session.rs`; 50 structural integration tests `test_00..49.rs`; 1 snapshot gate `snapshot_cpu` covering 57 pages; 5 (+2 under `--features quickjs`) `WinitSession` automation-command tests in `test_automation_commands.rs`.
