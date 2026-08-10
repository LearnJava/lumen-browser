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
