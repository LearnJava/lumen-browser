# BUG-960: `scrollWidth`/`scrollHeight` don't compute the true CSS Overflow scrollable-overflow-area for non-scroll-container elements

**Статус:** FIXED 2026-09-28 (P6)
**Дата:** 2026-09-02
**Компонент:** layout (`crates/engine/layout/src/lib.rs::collect_scroll_containers_inner`,
`scrollable_extent_x`/`scrollable_extent_y`), js (`crates/js/src/shim/web_api_shim_mid.js`
— `scrollWidth`/`scrollHeight` getters, unchanged — they already read whatever
`_lumen_get_scroll_state` publishes)
**Найден:** P3 2026-09-02, while closing [BUG-475](BUG-475-FIXED.md)
**Исправлен:** P6 2026-09-28

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
border boxes — not just the wrapper's own border-box size.

## Причина

Two independent gaps, both fixed here:

1. `scrollable_extent_x`/`scrollable_extent_y` (the `content_width`/
   `content_height` fold) only walked **direct children**' rects — a
   grandchild's overflow (e.g. through an intermediate `overflow: visible`
   wrapper) never reached the outer scroll container's `scrollWidth` at all.
2. `collect_scroll_containers_inner` only ran that fold for boxes that are
   designated scroll containers (`overflow-x`/`overflow-y` is
   `Scroll`/`Auto`). Every other box's JS getter fell back to the border-box
   size (BUG-475's floor), which is correct only when the box has no
   overflowing content — not the case this WPT test specifically constructs.

## Фикс

`crates/engine/layout/src/lib.rs`:

- `scrollable_extent_x`/`scrollable_extent_y` now recurse through every
  descendant that doesn't itself establish a clip boundary
  (`box_clips_own_overflow`: any `overflow-x`/`overflow-y` other than
  `visible`). A clipping descendant's own border box still contributes to
  the ancestor's scrollable-overflow region (it's a normal in-flow/abspos
  box), but *its* descendants don't roll up any further — they're already
  contained and reported separately by that box's own
  `scrollWidth`/`scrollHeight`.
- `collect_scroll_containers_inner` (the `for_js_state` variant used to feed
  `update_scroll_states`) now also publishes an entry for plain
  `overflow: visible` boxes whose content-width/content-height exceeds their
  own padding-box size — i.e. exactly the boxes CSSOM View defines an exact
  `scrollWidth`/`scrollHeight` for beyond the border-box floor. Boxes that
  don't overflow are deliberately left unpublished (the JS shim's
  border-box fallback is already correct and cheaper for the common case).

`crates/js/src/shim/web_api_shim_mid.js`: no logic change — `scrollWidth`/
`scrollHeight` already read `_lumen_get_scroll_state` first and fall back to
the border-box rect, so the new layout-side entries flow straight through.
Only the explanatory comment was updated.

## Тесты

5 new unit tests in
`crates/engine/layout/src/tests/scroll_interaction_misc.rs` (all passing):

- `scroll_width_nested_overflow_visible_reaches_outer_container` — a
  grandchild's overflow now reaches the outer scroll container through a
  non-clipping intermediate box.
- `scroll_width_nested_overflow_stops_at_clipping_descendant` — an
  intermediate `overflow: hidden` box still contributes its own border box,
  but its overflowing grandchild does not roll up any further.
- `scroll_width_overflow_visible_reports_exact_overflow_not_just_border_box`
  — a plain `overflow: visible` box with overflowing content is now
  JS-visible with the exact scrollable-overflow magnitude.
- `scroll_width_overflow_visible_non_overflowing_absent_from_js_state` — the
  common non-overflowing case stays off the published list.
- Full `lumen-layout` unit suite: 4112 passed, 0 failed (no regressions).

`cargo clippy -p lumen-layout --all-targets -- -D warnings`: clean.

## .ini

Still not updated — the 8 files under `tests/wpt/metadata/css/cssom-view/`
that reference BUG-475/BUG-960 need a live `tests/wpt/run_report.py` pass
(built browser + `wss` server, hours) to confirm the exact PASS/FAIL split
against this fix before touching any `.ini`. The fix targets the scenarios
`BUG-960`'s own investigation identified by construction
(`scrollWidthHeight-negative-margin-{001,002}.html`,
`scrollWidthHeight-child-border-within-padding.tentative.html`,
`scrollWidthHeight-flex-column-padding-001.html`); `elementScroll.html`/
`elementScroll-002.html`/`outer-svg.html`/`client-props-input.html` were
never confirmed to depend on this exact-value gap in the first place and
still need the same fresh run to tell apart.
