# BUG-489: `getComputedStyle()` returns nothing for a `display: contents` element itself

**Статус:** FIXED 2026-09-02
**Дата:** 2026-08-02
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs::flatten_contents`,
`crates/engine/layout/src/lib.rs::collect_computed_styles`/
`collect_layout_rects`)
**Найден:** WPT-RUN-3 срез 7 (`ROADMAP.md`) — массовый прогон `css/css-display`

## Механизм

`display: contents` (CSS Display L3 §7.2) is spec'd to eliminate the
element's own *box*, not the element or its style — `getComputedStyle()`
must still resolve real values (cssom-1 §resolved-values doesn't carve out
an exception). Lumen's box builder does create a `BoxKind::Contents` box
for such an element during `build_box`, but `flatten_contents`
(`box_tree.rs:4599-4618`) then **removes it from the tree entirely**,
splicing its children in its place (`children.remove(i)` at `:4605`, no
child re-inserted with the original element's own `NodeId`). Since
`collect_computed_styles`/`collect_layout_rects` (`lib.rs:1227-1276`) are
pure `LayoutBox`-tree walkers keyed by `b.node.index()`, an element with no
surviving box in the tree can never get an entry in either map — every
property read through `getComputedStyle()` and every geometry read through
`getBoundingClientRect()` falls through the `.unwrap_or_default()` in
`_lumen_get_computed_style` (`dom.rs:2525`) to `""`/all-zero, same shape as
"element doesn't exist".

Confirmed live via `--mcp-port`:

```
<div id="t1" style="display:contents"></div>
getComputedStyle(document.getElementById('t1')).display  →  ""   (spec: "contents")
document.getElementById('t1').getBoundingClientRect().width → 0  (spec-correct, box is eliminated — not itself a bug)
```

## Симптом

`display-contents-computed-style.html` (`css/css-display`) queries a
`display: contents` element's own computed style directly (not through a
descendant) in 3 of its 5 subtests: serialization of `display` itself
("contents"), resolved-vs-used value of `width`/`height`/`margin-left`/
`padding-top` on a `display:contents` element, and blockification of the
root `<html>` element (which the UA stylesheet in this test also sets to
`display: contents`, expecting it to compute to `block` per the "root
elements are blockified" rule) — all three read back `""` instead of a
real value. The other 2 subtests of the same file query a **descendant** of
a `display:contents` container instead ([BUG-488](BUG-488-FIXED.md) — plain
inline elements, unrelated mechanism, isolated separately).

`display-contents-parsing-001.html` and `display-contents-focusable-001.html`
hit the identical pattern on their own single `display:contents` target
element.

## Масштаб находки

3 files in this slice. Likely recurs anywhere a WPT test queries
`getComputedStyle()`/`getBoundingClientRect()` directly on a
`display:contents` element rather than one of its children — a
`grep -rl 'display:\s*contents' tests/wpt/css` count would over-estimate
scope (most such tests only check rendering/children, not the contents
element's own resolved style).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-display/` for the
attributed subtests in `display-contents-computed-style.html`,
`display-contents-parsing-001.html`, `display-contents-focusable-001.html`.

## Fix (2026-09-02)

Unlike [BUG-488](BUG-488-FIXED.md) (a plain inline element — the nearest
nested segment's style is a usable approximation), a `display: contents`
element has neither a surviving box nor a descendant whose style would
match: it can own non-inherited properties directly (`width`,
`margin-left`, …) and wrap block-level children, so approximation from a
child is not applicable — the real cascade result is needed.

That result is already cached: `precompute_counters` builds a
`CounterMap` entry for every element regardless of `display`. The fix
threads that map to the collector instead of recomputing anything:
`layout_measured_with_counters`/`layout_measured_hyp_with_counters` hand
the fresh `CounterMap` back to the caller alongside the `LayoutBox`, and
`collect_computed_styles` takes a new `counters: Option<&CounterMap>`
parameter — when `Some`, every element whose counter-cached style has
`display: contents` and no existing map entry gets one built from that
cached style. Call sites that reuse an already-built `LayoutBox` without a
matching fresh `CounterMap` (`relayout_scoped`'s incremental path,
hibernation, `page_load.rs`/`page_pipeline.rs`/`relayout.rs`) pass `None`
and keep today's behaviour for this one element shape — no regression,
same scope as before.

Separately: CSS Display L3 §2.7 blockifies the root element — its own box
can never be eliminated (nothing above it to splice children into), so
`display: contents` on the document element now computes to `block` in
`cascade.rs` (after the declaration loop, so it sees the final `display`),
matching what already happened implicitly at the box-tree level (the root
element's `BoxKind::Contents` was never processed by `flatten_contents`,
which only runs over a parent's children list).

New end-to-end test `crates/driver/tests/cases/bug489_display_contents_computed_style.rs`
reproduces the exact repro shape from the vendored
`css/css-display/display-contents-computed-style.html`. New unit tests
`snapshot_collectors_cover_display_contents_elements` and
`root_element_display_contents_is_blockified` in `lumen-layout::lib.rs`.

Gates: `cargo test -p lumen-layout` 3660/3660, `cargo test -p lumen-driver`
282/282 (whole crate), `cargo test -p lumen-shell` 1717/1717, `cargo clippy
-p lumen-layout -p lumen-driver -p lumen-shell --all-targets -- -D
warnings` clean.
