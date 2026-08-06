# BUG-338: no automation surface can scroll a nested `overflow:auto` container (only the top-level page)

**Статус:** FIXED 2026-08-06
**Компонент:** driver/shell (`lumen-driver::WinitSession::scroll`, `lumen-driver::InProcessSession::scroll`,
`lumen-shell::navigate_fragment`) — DEVX/automation
**Найден:** P1, CC-CSS-3 2026-07-24 (while trying to write a scroll-driven repro/graphic-test for BUG-336)

## Симптом

Two candidate ways to drive a page into a *scrolled* state for automated/graphic testing
both only move the top-level page viewport, never a nested scrollable ancestor:

1. **MCP `scroll` tool** (`--mcp-live-port`/`--mcp`, used by `scripts/scroll_perf.py`,
   `scripts/input_perf.py`, `graphic_tests/run.py --live`): accepts a `target: {css: "..."}`
   selector in its params, but `WinitSession::scroll()`
   (`crates/driver/src/winit_session.rs:1014`) takes the target as `_target` — unused —
   and always calls `scroll_page_by()`, which only mutates `scroll.nodes.first_mut()`, the
   **root** scroll node (`crates/driver/src/winit_session.rs:142-155`). There is no path
   from an MCP `scroll` call to any other node in the scroll tree, however deep
   `find_scroll_container_at`/`collect_scroll_containers` (the layout-side machinery that
   *does* route real mouse-wheel events to the innermost container under the cursor,
   CC-CSS-2) would place it.
2. **Fragment navigation** (`#anchor`): `navigate_fragment()`
   (`crates/shell/src/main.rs:9273`) finds the target element's absolute box and calls
   `self.scroll_to(y)` — again the page-level scroll only. A real browser's fragment/
   `Element.scrollIntoView()` walks every scrollable ancestor and scrolls each one just
   enough to bring the target into view; Lumen doesn't.

## Impact

There is currently no way — MCP, `--ipc-server`, `--mcp`, headless `InProcessSession`, or
a static HTML/CSS-only trick (no fragment-into-nested-container support either) — to
produce a deterministic "nested container scrolled to X" state for a graphic test or a
driver-level assertion. This blocked writing an automated regression test for
[BUG-336](BUG-336-FIXED.md) (`position:sticky` inside `overflow:auto` panels, e.g. chrome's
`.dt-panel`/`.net-table th`) — that fix currently has unit-test coverage of its exact
renderer-side math only, not an end-to-end screenshot/assertion proving it in a live page.
Also blocks: any future CC-CSS-* or general CSS work whose DoD needs "scrolled" pixel or
box-model verification of nested scroll containers specifically (sidebar lists, dropdown
panels, DevTools-style resizable panes — all over the chrome asset).

## Fix

Added `lumen_layout::find_scroll_container_for_node(containers, doc, node)`
(`crates/engine/layout/src/lib.rs`) — the by-node twin of the existing by-point
`find_scroll_container_at`: walks the DOM parent chain from `node` (inclusive of `node`
itself) and returns the first ancestor present in `containers`, or `None`.

Wired into the two automation session types that already hold a mutable
`LayoutBox`/`Document` pair:

- **`WinitSession::scroll()`**: resolves `target` to a `NodeId` (reusing the existing
  `resolve_target_node`), finds its nearest scrolling ancestor via the new helper, and — if
  found — clamps and applies the delta to *that* container's own `scroll_x`/`scroll_y`
  (`lumen_layout::set_scroll_position`) instead of the page. No target, no DOM match, or no
  scrolling ancestor (e.g. `body`) falls through unchanged to the pre-fix whole-page
  `scroll_page_by` path — existing callers (`scroll_perf.py` etc., which scroll `body`)
  keep their old behavior.
- **`InProcessSession::scroll()`**: same routing, reusing the existing `resolve_click_target`
  (which additionally resolves `Target::Point` via hit-test, exactly like real mouse-wheel
  routing does) instead of `WinitSession`'s selector/NodeId-only resolver. This is the type
  actually reachable through the standalone `lumen-mcp` binary's `scroll` tool
  (`docs/automation.md`'s `--mcp`/`--mcp-port` row), so this is the change that makes the
  bug's own title true for headless MCP automation, not just the `lumen-driver` test harness
  `WinitSession` runs on.
- **`navigate_fragment()`** (`crates/shell/src/main.rs`): new
  `Lumen::scroll_nested_ancestors_into_view(node, target_rect)` walks the DOM parent chain
  of the fragment target and, for each `ScrollContainer` on that chain whose current
  viewport doesn't already contain `target_rect` (vertical axis only, matching the
  page-level `scroll_to`/`start_smooth_scroll` this runs before), nudges that container's
  own `scroll_y` just enough to bring the nearer edge into view. Content boxes carry
  absolute (unscrolled) layout coordinates — `PushScrollLayer`'s `translate(-scroll_x,
  -scroll_y)` is a paint-time-only effect — so each container's adjustment is independent of
  its ancestors' scroll state and can be computed without needing to compose transforms.

**Scope note — not fixed:** the **live-window** MCP path (`--mcp-live-port`, `LiveSession`
→ `AutomationCommand::Scroll(delta)` → shell's automation-command handler) still drops
`target` — `AutomationCommand::Scroll` doesn't carry one on the wire at all. Fixing that
needs a wire-protocol change (new field/variant) plus a handler on the shell side; out of
scope for this fix, which targeted the two session types the bug's own diagnosis and
suggested-fix section named. `graphic_tests/run.py --live`'s default live-window pipeline
does not currently issue any target-scoped `scroll` calls, so this gap has no live-test
impact today — filed as a follow-up if a future test needs it.

## Tests

- `lumen-layout`: `find_scroll_container_for_node_{walks_dom_ancestors,
  matches_node_itself, none_when_no_scrolling_ancestor}` (`crates/engine/layout/src/lib.rs`).
- `lumen-driver`: `InProcessSession::scroll` unit tests
  (`crates/driver/src/session.rs::tests::scroll_with_*`) — selector into a nested container,
  clamping at content bounds, `Target::Point` over the container, and the page-level
  fallback for a target with no scrolling ancestor. `WinitSession::scroll` integration tests
  (`crates/driver/tests/cases/test_bug338_nested_scroll.rs`) covering the same
  container-vs-page routing through the public `BrowserSession` API.

Merge `p3-bug-338`.
