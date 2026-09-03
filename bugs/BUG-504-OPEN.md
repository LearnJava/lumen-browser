# BUG-504: scrollable-overflow computation (`content_width`/`content_height`)
ignores CSS `transform` on children entirely, and is wrong in several other
css-overflow scenarios (abspos, clip-margin, RTL/logical axes)

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** layout (`crates/engine/layout/src/lib.rs::content_width`/`content_height`)
**Найден:** WPT-RUN-3 срез 11 (`ROADMAP.md`) — массовый прогон `css/css-overflow`

## Механизм

`content_width`/`content_height` (`crates/engine/layout/src/lib.rs:1200-1216`)
compute `scrollWidth`/`scrollHeight` as `max(own rect size, farthest child
edge)`, where "child edge" is read straight off each child's post-layout
`LayoutBox::rect` — the box's static in-flow geometry:

```rust
fn content_width(b: &LayoutBox) -> f32 {
    b.children.iter().fold(b.rect.width, |acc, c| {
        let c_right = c.rect.x + c.rect.width - b.rect.x;
        acc.max(c_right)
    })
}
```

`rect` never reflects a CSS `transform` — Lumen (correctly) treats `transform`
as a paint-time-only visual effect that doesn't move boxes for the purpose of
layout flow. But per the CSS Overflow spec (`css-overflow-3/#scrollable`),
`transform` **does** contribute to the *scrollable* overflow rectangle
specifically — a translated/rotated/scaled child's painted (post-transform)
bounding box is what determines whether a scroll container needs to grow its
scroll range, even though the child's own flow position and the size it
occupies in normal layout are unaffected. `content_width`/`content_height`
never apply the child's `transform` when computing this box, so any content
that only extends beyond the container via a transform is invisible to
`scrollWidth`/`scrollHeight` and reports plain `0` (no overflow) instead of
the transformed extent.

Confirmed directly:

```html
<div style="width:100px;height:100px;overflow:auto" id=c>
  <div style="width:50px;height:50px;transform:translateX(200px)"></div>
</div>
<script>c.scrollWidth /* spec: 250, Lumen: 100 (no overflow detected) */</script>
```

Live `--mcp-live-port` probe on this exact snippet returns `scrollWidth: 0`
above the container's own size — i.e. `content_width` falls back to its
`b.rect.width` floor because the untransformed child fits entirely inside the
100×100 box; the transform is simply never consulted.

## Масштаб находки

20 files in `css/css-overflow`, split into two groups by confidence:

**Verified root cause (transform):** `overflow-abpos-transform.html`,
`overflow-inline-transform-relative.html`,
`single-axis-scroll-apis-dynamic.html`,
`scrollable-overflow-transform-unreachable-region.html`,
`scrollable-overflow-transform-dynamic-{001..006}.html`,
`scrollable-overflow-height-with-flex-item-margin-inline-end{,-rtl}.html`,
`scrollable-overflow-with-{flex,grid}-item-margin-inline-end.html` — all
report `scrollWidth`/`scrollHeight` (or a derived `container`/`container1`/
`container2` reading, once [BUG-384](BUG-384-FIXED.md) is separately fixed) as
`0` where a positive value is expected, and every one of these tests exercises
a `transform` on the overflowing child.

**Same symptom, not yet individually root-caused (list for follow-up, may be
this bug or a sibling one — abspos-without-transform, clip-margin RTL,
scrollbar-gutter space reservation, single-axis clamping all return the same
"`scrollWidth`/`scrollHeight` reads `0`/`undefined` where a positive number is
expected" shape but haven't each been traced to a specific missing
contribution):
`overflow-clip-clamps-and-ignores-scroll-offsets-vertical-rl.html`,
`overflow-outside-padding.html`, `overflow-clip-scroll-size.html`,
`overflow-rtl-scroll-left.html`, `single-axis-scroll-into-view{,-rtl}.html`,
`single-axis-scroll-apis-programmatic.html`,
`scrollbar-gutter-{001,rtl-001,vertical-lr-001,vertical-rl-001}.html`,
`scrollbar-gutter-propagation-{001,002,003,007}.html`.

Once `scrollable-overflow-transform-*`/`scrollable-overflow-with-nested-
elements-*` (currently masked by [BUG-360](BUG-360-FIXED.md), body `onload`
never firing) start actually running `checkLayout`, most of that family will
also land on this bug.

## Что нужно

Extend `content_width`/`content_height` (or their caller) to fold in each
child's rendered/painted bounding box under its accumulated `transform`
(matrix-transform the child's border-box corners, not just translate) when
computing scrollable overflow, while leaving the child's own `rect` (flow
position/size) untouched — the two concerns (flow geometry vs. scrollable
overflow) need to stay separate per spec. The abspos/RTL/clip-margin cluster
needs its own investigation once this lands — some of those may turn out to
be the same fix (e.g. if abspos boxes aren't walked into `content_width` at
all yet), others may be genuinely separate.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-overflow/` for the 20 files
above, `expected: FAIL` per affected subtest.

## Срез 2026-09-03 (P3): transform contribution landed

Implemented exactly the "Что нужно" fix above for the **verified root cause
(transform)** group. `content_width`/`content_height`
(`crates/engine/layout/src/lib.rs`) now consult a new helper,
`child_scrollable_bounds`, which returns a child's border-box corners after
applying its forward transform matrix (`forward_box_transform`, the same
matrix paint uses to emit `PushTransform`) when the child carries one, or the
plain `c.rect` unchanged otherwise (zero-cost fast path for the untransformed
common case — the vast majority of boxes). `LayoutBox::rect` itself is left
untouched, so flow geometry is unaffected; only the scrollable-overflow fold
changes. Confirmed against the exact repro snippet from this file's
"Механизм" section (`translateX(200px)` on a 50×50 child in a 100×100
`overflow:auto` container) via new regression tests
(`collect_scroll_containers_transform_grows_scroll_width`/`_height` in
`crates/engine/layout/src/tests/scroll_interaction_misc.rs`) — `scrollWidth`
now reports `250` (was `0` above the container's own size), matching the
spec value from the snippet's comment. A third regression test
(`collect_scroll_containers_no_transform_unaffected`) guards the
untransformed fast path against regressing to the old plain-`rect` numbers.
`cargo test -p lumen-layout`: 3683/3683 (workspace-wide, not just the new
tests) — no existing geometry test shifted, confirming the fast path is
truly a no-op for untransformed children. This lands via `collect_scroll_containers`
(consumed by both `scrollWidth`/`scrollHeight`'s JS getters via
`_lumen_get_scroll_state` and `set_scroll_position`'s clamp range), so both
read and scroll-clamp surfaces pick up the fix together.

**Not attempted — remains the bug's open scope:** the **"same symptom, not
yet individually root-caused"** group (11 files: abspos-without-transform,
clip-margin RTL, scrollbar-gutter space reservation, single-axis clamping).
None of these involve a child `transform`, so this slice's fix does not
touch them; each still needs its own root-cause pass per the file's original
"Масштаб находки" split. Status remains `OPEN` — only the transform
component of this bug's original 20-file finding is closed.

## Срез 2026-09-03 (P3, часть 2): `overflow-outside-padding.html` root-caused and fixed

Root-caused the first file of the "not yet individually root-caused" group
above. `content_width`/`content_height` (`crates/engine/layout/src/lib.rs`)
had two independent defects, both exposed by this file's asymmetric-border,
abspos-heavy layout:

1. **Floor used the border-box, not the padding box.** `scrollWidth`/
   `scrollHeight` are defined (CSS Overflow L3 §3.3) relative to the padding
   edge, but the floor was plain `b.rect.width`/`height` (border-box,
   `LayoutBox::rect`'s documented contract). A container with a non-zero
   border (this test's `.container` has `border-width: 0 0 50px 80px`) has a
   border-box strictly larger than its padding-box, so `scrollWidth` read
   280 instead of the spec's 200 even with *no* overflowing content at all.
2. **Absolutely/fixed positioned descendants that land wholly outside the
   padding edges still contributed in full**, instead of being excluded per
   CSS Overflow L3 §3.3 ("blocks wholly outside padding edges should not
   contribute to overflow"). The test's six `.target` boxes (`position:
   absolute; width/height: 1000px`) are each pushed via a single physical
   inset (`top`/`right`/`bottom`/`left: -1000px`) to sit just past one edge
   of a 200×200 container — touching it, not inside it. Because
   `content_width`/`content_height` folded in every child's bounds
   unconditionally, each of these boxes blew `scrollWidth` up to ~1000+px.

Fix: a new shared `padding_box(b)` helper (also now used by
`collect_scroll_containers_inner`'s `clip_rect`, replacing its inline
border-subtraction so the floor and the viewport rect can never drift apart
again) replaces the border-box floor and origin in both functions. A new
`contributes_to_scrollable_overflow(child, bounds, padding_box)` gate skips a
child entirely (not just clamps it) when **and only when** it is
`position: absolute`/`fixed` *and* its (transform-adjusted) bounds have zero
overlap with the padding box on the X axis, the Y axis, or both
(`rects_overlap`, strict inequalities — touching at a boundary counts as no
overlap, matching this test's boxes landing exactly on the padding edge).
The gate is conditioned on the child's `position` specifically because CSS
Overflow L3 §3.4 makes the opposite rule for `transform`: an in-flow box
pushed entirely outside by `transform` must still count in full (the
transform-contribution fix from part 1 above, and its two regression tests,
are deliberately left unconditional and unaffected by this slice —
re-verified: both still pass unchanged).

Regression tests added to `crates/engine/layout/src/tests/scroll_interaction_misc.rs`:
`collect_scroll_containers_abspos_wholly_outside_padding_excluded` (mirrors
this file's exact repro: 1000×1000 abspos child at `top: -1000px` inside a
100×100 container — `scrollWidth`/`scrollHeight` must stay at 100, not grow
from the child's horizontal overlap alone), `_abspos_overlapping_child_still_contributes`
(guard: an abspos child overlapping on both axes must still grow
`scrollWidth` normally), and `_scroll_width_floor_is_padding_box_not_border_box`
(200px content + 80px left border → `scrollWidth` must read 200, not 280).
`cargo test -p lumen-layout`: 3686/3686 (workspace's existing 3683 unaffected,
including the two transform-contribution tests from part 1).

**Live WPT verification not performed this slice** — the sandbox this
session ran in refused to bind the `--mcp-port` TCP socket (`os error
10013`, access denied) needed to drive a headless probe, so the fix is
verified by unit tests that reproduce the exact geometry (offsets, border
widths, expected `scrollWidth` values) of the WPT file's six subtests, not
by a live run of the file itself.

**Remaining scope unchanged:** 10 files (abspos-without-transform is now
closed; clip-margin RTL, scrollbar-gutter space reservation, single-axis
clamping remain) — each still needs its own root-cause pass. Status stays
`OPEN`.
