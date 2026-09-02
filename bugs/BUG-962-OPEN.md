# BUG-962: `scrollIntoView` alignment ignores `scroll-margin-*`

**Статус:** OPEN
**Дата:** 2026-09-02
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js::Element.scrollIntoView`,
`_lumen_align_scroll`/`_lumen_get_bounding_rect` — `web_api_shim_head.js`) +
possibly a new native binding in `crates/js/src/v8_runtime/install/platform.rs`
**Найден:** P3 2026-09-02, residual of [BUG-479](bugs/BUG-479-FIXED.md)

## Симптом

[BUG-479](bugs/BUG-479-FIXED.md) gave `scrollIntoView` real `block`/`inline`/
`behavior` handling and a `Promise` return value, computing each axis's
target scroll offset from the target element's and container's
`getBoundingClientRect()`s alone. CSS `scroll-margin-top`/`-right`/`-bottom`/
`-left` (CSS Scroll Snap L1 §4, already computed into `ComputedStyle` —
`crates/engine/layout/src/style/computed.rs:488-492`, wired into CSS Scroll
Snap's own snap-area math in `crates/engine/layout/src/lib.rs`) is not folded
into the alignment target at all: `_lumen_align_scroll`'s `contentPos` is the
element's raw border-box position, so a target styled with e.g.
`scroll-margin-top: 20px` lands flush against the container's edge instead of
20px clear of it. `scrollIntoView-scrollMargin.html` (named directly in
BUG-479's original symptom list) exercises exactly this and is expected to
still fail.

## Что нужно

`getBoundingClientRect()`/`layout_rects` carry border-box geometry only —
there is no JS-facing accessor for an element's resolved `scroll-margin-*`
today. Needs either:
1. A new native binding (`_lumen_get_scroll_margin(nid) -> [top,right,bottom,left]`
   or similar) reading `ComputedStyle::scroll_margin_*`, threaded through the
   shell the same way `layout_rects` is (`update_layout_rects`-style push
   after relayout) — the "own new geometry channel" option; or
2. Exposing `scroll-margin-*` through the existing `getComputedStyle()` map
   (`computed_style_to_map`) and reading it from there in the shim — cheaper
   if BUG-472 (`getComputedStyle` is a fixed hand-written property list) is
   being touched anyway, dead weight otherwise.

Once available, `_lumen_align_scroll`'s `contentPos`/`clientSize` need the
margin folded in per CSS Scroll Snap L1 §4's scroll-margin definition (the
margin expands the target's effective box on `'start'`/`'end'`/`'center'`
the same way it already does for scroll-snap's snap area in
`crates/engine/layout/src/lib.rs`).
