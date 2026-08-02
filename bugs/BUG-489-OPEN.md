# BUG-489: `getComputedStyle()` returns nothing for a `display: contents` element itself

**Статус:** OPEN
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
a `display:contents` container instead ([BUG-488](BUG-488-OPEN.md) — plain
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
