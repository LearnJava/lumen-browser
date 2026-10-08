# BUG-1215: headless `InProcessSession::scroll()` offset is lost on the next relayout

**Статус:** FIXED 2026-10-07 — `layout_and_commit` принимает `prev_scroll`, `relayout` переносит смещения контейнеров на свежее дерево
**Дата:** 2026-09-29
**Компонент:** driver (`crates/driver/src/session.rs::layout_and_commit`/`relayout`/`scroll`)
**Найден:** P6, side discovery while verifying [BUG-965](BUG-965-FIXED.md)'s fix

## Механизм

`InProcessSession::scroll()` (the native driver API behind the headless
`--mcp-port`/`--mcp` `scroll` command) routes to a nested overflow ancestor
via `lumen_layout::set_scroll_position(&mut state.layout_root, ...)`, which
mutates the current `LayoutBox` tree's `scroll_x`/`scroll_y` fields in place —
no relayout involved, by design (cheap, off-main-thread-style update).

The very next relayout — `eval()`'s mandatory post-run `self.relayout()`, or
the one `click()`/`type_text()` already run after dispatching their DOM event
— calls `layout_and_commit`, which calls
`lumen_layout::layout_measured_with_counters(&doc_guard, sheet, ...)`. That
function builds a **brand-new** `LayoutBox` tree from scratch: every
`scroll_x`/`scroll_y` starts back at `0.0`. Nothing reapplies the
previously-set scroll offset onto the fresh tree.

Contrast with the JS-side `FlushHandles::maybe_flush`
(`crates/js/src/v8_runtime/style_flush.rs`), which explicitly captures
`prev_scroll` from its own cache before rebuilding layout and calls
`set_scroll_position` on the fresh tree to restore it (this is what makes
[BUG-965](BUG-965-FIXED.md)'s JS-driven `scrollTo()` scenario work correctly).
`InProcessSession::layout_and_commit` has no equivalent step for the scroll
offset the *driver's own* `scroll()` API just set.

## Симптом

Any headless script that calls the `scroll` MCP command and then triggers
*any* further interaction that causes a relayout (`eval`, `click`,
`type_text`) sees the scroll silently reset to `0`, both on the Rust side
(`collect_scroll_containers(&state.layout_root)`) and on the JS side
(`Element.scrollTop`/`scrollLeft`, once a relayout has run) — confirmed with a
unit test: `scroll()` → `eval("1")` (no-op, only to force a relayout) →
`document.getElementById(...).scrollTop` reads `"0"`, not the previously-set
offset.

## Масштаб

Affects any headless (`InProcessSession`) automation that scrolls a
container and then reads back geometry, takes a screenshot, or otherwise
triggers a relayout before reading the scroll position again. Does not affect
`WinitSession` (live window, shell's own relayout path, out of scope here —
not audited for the same gap).

## Что нужно

`InProcessSession::layout_and_commit` (or `commit_layout`) needs to capture
the previous `layout_root`'s scroll offsets (keyed by `NodeId`) before
rebuilding, then reapply them onto the freshly built tree via
`lumen_layout::set_scroll_position`, mirroring what
`FlushHandles::maybe_flush` already does for the JS-side cache.
