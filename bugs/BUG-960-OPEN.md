# BUG-960: `scrollWidth`/`scrollHeight` don't compute the true CSS Overflow scrollable-overflow-area for non-scroll-container elements

**Статус:** OPEN
**Дата:** 2026-09-02
**Компонент:** layout (`crates/engine/layout/src/lib.rs::collect_scroll_containers`,
`content_width`/`content_height`), js (`crates/js/src/shim/web_api_shim_mid.js`
— `scrollWidth`/`scrollHeight` getters)
**Найден:** P3 2026-09-02, while closing [BUG-475](BUG-475-FIXED.md)

## Симптом

[BUG-475](BUG-475-FIXED.md) fixed `scrollWidth`/`scrollHeight` returning a
hard `0` for any element that isn't a designated `overflow: scroll`/`auto`
container, by falling back to the element's border-box size. That satisfies
the spec's floor ("at least padding-box size") but not the exact value the
spec requires when the element's content actually overflows its own padding
box without the element being independently scrollable — e.g. a child with
negative margins, an absolutely positioned descendant, or flex/grid content
overflow.

`tests/wpt/css/cssom-view/scrollWidthHeight-negative-margin-002.html`'s
`.wrapper` (`display: flow-root; overflow: visible`) contains `.inner`
(`margin: -100px; width: 300px; height: 300px`), which overflows the
wrapper's padding box by design. Per CSSOM View, `wrapper.scrollWidth` must
equal a precise computed number (204 or 216 minus padding, depending on
direction/writing-mode) derived from the union of the overflowing content's
border boxes — not just the wrapper's own border-box size. The BUG-475 fix
returns the wrapper's own border-box width (154 in this fixture), which
satisfies `assert_greater_than_equal(scrollWidth, paddingBox.width)` but
fails the subsequent `assert_equals(scrollWidth, expectedExact)` in the same
`test()` block, so the WPT subtest remains FAIL (with a different assertion
message than before).

## Причина

`collect_scroll_containers` (`layout/src/lib.rs:1131`) only computes
`content_width`/`content_height` (the "how far does the content extend"
measurement) for boxes that are designated scroll containers
(`overflow_x`/`overflow_y` is `Scroll`/`Auto`) — `content_width`/
`content_height` themselves (`layout/src/lib.rs:1208-1224`) walk only
**direct children**' rects, which is already a simplification (doesn't
recurse through a child that itself doesn't clip). For every other box the
JS getter now falls back to the border-box size (BUG-475), which is correct
only when the box has no overflowing content — the common case, but not the
one this WPT test specifically constructs.

## Масштаб находки

Confirmed affected by construction: `scrollWidthHeight-negative-margin-001.html`,
`scrollWidthHeight-negative-margin-002.html`,
`scrollWidthHeight-child-border-within-padding.tentative.html`,
`scrollWidthHeight-flex-column-padding-001.html` — all four exercise a
non-scroll-container element whose content overflows its own box on purpose.
Not yet checked whether `elementScroll.html`/`elementScroll-002.html`/
`outer-svg.html`/`client-props-input.html` (the other four `.ini` files that
reference BUG-475) depend on this same exact-value gap or are already
satisfied by the border-box floor — needs a fresh `tests/wpt/run_report.py`
pass to tell apart.

## Что нужно

Implement the CSS Overflow §Scrollable Overflow Region algorithm (or a
reasonable approximation) for every box, not just designated scroll
containers: the union of border boxes of everything in the box's flow root
that isn't clipped away, clamped/expanded per the box's own `overflow`
value. This is materially bigger than a JS-side fallback — it likely needs a
new layout-side collector (sibling to `collect_scroll_containers`) that
walks the full subtree (not just direct children) and accounts for
absolutely/relatively positioned descendants and negative margins, then
publishes the result the same way `collect_scroll_containers` does today.

## .ini

Not yet updated — the 8 files under `tests/wpt/metadata/css/cssom-view/`
that reference BUG-475 need a fresh `run_report.py` run to see the actual
PASS/FAIL split after the BUG-475 fix before touching any `.ini`.
