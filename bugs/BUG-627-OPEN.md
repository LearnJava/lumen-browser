# BUG-627: `IntersectionObserver`'s `root` option (explicit root element) and `scrollMargin` option are entirely ignored — every observer intersects against the viewport regardless of `options.root`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:7217-7276` —
`_lumen_deliver_intersection_observers`)
**Найден:** P2, WPT-VENDOR-intersection-observer, 2026-08-05

## Симптом

Confirmed live (`--mcp-live-port`, `eval`): `new IntersectionObserver(cb,
{root: someElement}).root` returns `undefined` (also covered by BUG-625 —
no getter at all), but more importantly `_lumen_deliver_intersection_observers`
(`dom.rs:7217-7276`) never reads `obs._options.root` anywhere in its body —
`rootTop`/`rootLeft`/`rootRight`/`rootBottom` (`dom.rs:7226-7227`) are
computed unconditionally from `_lumen_get_viewport_size()`, i.e. the
observer always treats the top-level viewport as the intersection root,
even when constructed with an explicit scrollable-ancestor `root` element.
`options.scrollMargin` (a newer addition to the spec, expanding the
*target's* bounds before intersecting) is likewise never referenced
anywhere in the shim.

## Масштаб

Explains a large share of the category's remaining, non-BUG-625 failures
once `takeRecords()` availability stops masking them:

- `root-*.html` family (`root-margin-root-element.html`,
  `same-document-root.html`, `same-document-with-document-root.html`,
  `unclipped-root.html`, `root-is-table-with-overflow-scroll.html`, …):
  `rootBounds` in delivered entries always reflects the viewport, never
  the configured root element's bounds, and intersection ratios are
  computed against the wrong container entirely for nested-scroller
  cases.
- All `scroll-margin-*` / `*-scroll-margin.html`-style tests
  (14 failures with the signature `assert_equals:
  IntersectionObserverEntryCount expected 1 but got 0`) — `scrollMargin`
  silently has zero effect, so entries that should cross a threshold
  because of the expanded target bounds never do.
- `cross-document-root.html`, `explicit-root-different-document.html`:
  an explicit `root` in a different document should make the observer
  always report non-intersecting; current code can't distinguish this
  case since `root` is never inspected.

## Fix shape

`_lumen_deliver_intersection_observers` needs an `obs._options.root`
branch: when set, resolve the root element's own bounding rect (and, if
it is itself scrollable, its scrollport/clip rect — not just its border
box) via the same native binding used for `rootLeft`/`rootTop`/etc.
instead of `_lumen_get_viewport_size()`, and expand it by the parsed
`rootMargin`. Separately, `scrollMargin` (an array of 1-4 length values
in the same shorthand grammar as `rootMargin`) needs to expand `ex/ey/
ew/eh` (the *target's* rect, `dom.rs:7236`) before the intersection
computation, not the root. Both are independent of BUG-625/BUG-626 but
touch the same delivery function — worth doing together.
