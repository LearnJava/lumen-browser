# BUG-338: no automation surface can scroll a nested `overflow:auto` container (only the top-level page)

**Статус:** OPEN
**Компонент:** driver/shell (`lumen-driver::WinitSession::scroll`, `lumen-shell::navigate_fragment`) — DEVX/automation
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

## Suggested fix direction

- `WinitSession::scroll()`: resolve `target` (when given) to a `NodeId` via the existing
  selector-query machinery (already used by `layout_box_by_selector`), then find its
  nearest scrolling ancestor (mirrors `find_scroll_container_at`, but by node instead of by
  point) and mutate *that* scroll node instead of always the root. Keep root-scroll as the
  no-`target`/no-match fallback (current behavior) for backward compatibility with existing
  callers (`scroll_perf.py` etc., which scroll `body`).
- `navigate_fragment()`: after finding the target's box, walk its ancestor chain and, for
  each scrollable ancestor whose scrollport doesn't already contain the target, adjust that
  ancestor's own scroll offset too (bringing the target into view relative to *that*
  container) before falling through to the existing page-level `scroll_to`/smooth-scroll
  path — i.e. implement the ancestor-walk part of `scrollIntoView` that fragment nav is
  supposed to invoke.
