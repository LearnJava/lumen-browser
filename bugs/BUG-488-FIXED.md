# BUG-488: `getComputedStyle()`/`getBoundingClientRect()` return nothing for plain inline-level elements (`<span>`, `<em>`, `<a>`, …)

**Статус:** FIXED 2026-09-02
**Дата:** 2026-08-02
**Компонент:** layout (`crates/engine/layout/src/lib.rs::collect_computed_styles`/
`collect_layout_rects`, `crates/engine/layout/src/box_tree.rs` — inline
formatting: `BoxKind::InlineRun`/`InlineSegment`, `anon_inline_block_row`)
**Найден:** WPT-RUN-3 срез 7 (`ROADMAP.md`) — массовый прогон `css/css-display`

## Механизм

`collect_computed_styles`/`collect_layout_rects` (`lib.rs:1227-1276`) key
their output maps by `b.node.index()`, walking only `LayoutBox.children`
(`Vec<LayoutBox>`). For inline-level content, layout does **not** build one
`LayoutBox` per inline DOM element — inline children are grouped into
`BoxKind::InlineRun`/`InlineBlockRow` boxes whose actual per-run content
lives in `segments: Vec<InlineSegment>` (`box_tree.rs:2126`), a field the
two `collect_*` walkers never look inside (they only recurse `b.children`).
Each `InlineSegment` carries `source_node: NodeId` for the **text node**
that produced it (`box_tree.rs:2154`), not the wrapping inline element — so
even that field can't help. The `InlineRun`/`InlineBlockRow` box itself is
tagged with the *containing block's* own `NodeId`
(`anon_inline_block_row(node, ...)` at `box_tree.rs:3925`, called with the
container's own `id` at the call site `box_tree.rs:5587`), not the inline
element's. Net effect: a plain inline element's own `NodeId` never appears
as a key in either map, under any content (empty, text, mixed) — confirmed
live via `--mcp-port`:

```
<div id="plain"><span id="s" style="background-color:red">x</span></div>
getComputedStyle(document.getElementById('s')).backgroundColor  →  ""
document.getElementById('s').getBoundingClientRect().width      →  0
```

against the same page's sibling `<div>` (block-level), which resolves
correctly (`"rgb(255, 0, 0)"`). Reproduced with an empty `<span>`, a `<span>`
with text, and a `<span>` inside a `<p>` alongside sibling text — same
result in all three; content and position inside the inline formatting
context don't matter, only "is the element itself inline-level".

## Симптом

Any WPT assertion that calls `getComputedStyle()`/`getBoundingClientRect()`
(directly, or via a shared helper like `computed-testcommon.js`) on an
element that is inline-level **at the moment of the query** returns an
empty string / all-zero rect instead of a real value — indistinguishable
from "element doesn't exist". In this slice:

- `display-contents-blockify-dynamic.html`'s *first* failing assertion is a
  plain `<span>` grid item queried before any `display:contents` code runs
  at all (`assert_equals(display(child), "block", ...)` on a `<span>` that
  is still inline at that point) — the isolated repro above was distilled
  from this observation. The file's other two failures involve `<div>`
  containers that should already be block-level and are **not** explained
  by this bug; that file is left without `.ini` this slice (mixed/uncertain
  root cause, compounded by malformed test markup — an unclosed `<span>`
  inside `#grid-child` that relies on HTML5 parse-error recovery).
- The `span`-descendants of `display: contents` containers in
  `display-contents-computed-style.html` (`#t2 span`, `#t3 span`) also hit
  this — a red herring that first looked like a `display:contents`-specific
  gap until isolated with a minimal non-`contents` repro above.

## Масштаб находки

Not scoped to this slice's files — every inline HTML element (`<span>`,
`<a>`, `<em>`, `<strong>`, `<label>`, …) is affected identically, in any
WPT category. `grep -c 'getComputedStyle' tests/wpt/css/**/*.html` is a
poor proxy for scope (most calls target the test's designated block-level
`#target`), but any assertion targeting an inline element specifically will
silently read as "unsupported"/zero rather than fail with a diagnosable
message — worth flagging in future slices' triage as a first check before
attributing a `""`/zero-rect failure to something else.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-display/` for the files
in this slice fully explained by this bug (see slice write-up in
`docs/wpt-status.md`).

## Срез 2026-09-02 (P3) — FIXED

Both collectors now walk DOM ancestry, not just `LayoutBox.children`. A new
helper `inline_element_ancestors(doc, source_node, stop_at)`
(`crates/engine/layout/src/lib.rs`) walks from an `InlineSegment`/`InlineFrag`'s
`source_node` up through `Document` parents to the owning `InlineRun`'s own
`node` (the containing block/inline-block, exclusive), collecting every plain
inline element in between (innermost first).

- `collect_computed_styles_rec` publishes, for every such ancestor, the full
  property map of the nearest nested segment's already-cascaded style
  (`computed_style_to_map(&seg.style)`) — exact for every inherited property
  and for `display` (always `inline`, or the DOM element would not have been
  flattened into a segment at all), approximate only when an element strictly
  between the ancestor and the segment overrides a non-inherited property.
- `collect_layout_rects_rec` publishes, for every such ancestor, the union of
  every laid-out `InlineFrag` nested inside it across all lines — line
  y-position via the same `font_size * line_height` uniform-line-height model
  `selection.rs` already uses.

Both `collect_computed_styles`/`collect_layout_rects` now take `&Document` —
propagated through every call site in `lumen-shell` (`frames.rs`,
`hibernation.rs`, `page_load.rs`, `page_pipeline.rs`, `relayout.rs`) and
`lumen-driver` (`session.rs`), each of which already held (or now takes) the
document lock at the call site.

New regression test `snapshot_collectors_cover_plain_inline_elements`
(`lumen-layout::lib.rs` test module) — laid out with a real (`Fixed8`)
measurer, since the no-measurer `layout()` test helper never assigns text a
nonzero width and would pass trivially for the wrong reason.

`lumen-driver`'s pre-existing `inline_element_has_no_snapshot_entry_but_its_text_node_does`
asserted the old (buggy) absence of an entry as a "regression guard for the
bridge itself" — updated to `inline_element_and_its_text_node_report_the_same_style`,
asserting the element and its text node now agree instead of the element
having nothing.

Pixel-neutral by construction: the fix only adds keys to the two JS-snapshot
maps consumed by `getComputedStyle()`/`getBoundingClientRect()` — it does not
touch `LayoutBox`, the box tree, or anything the paint/display-list path
reads.
