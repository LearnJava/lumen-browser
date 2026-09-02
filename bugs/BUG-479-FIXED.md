# BUG-479: `Element`/`Window` scroll API is a narrow x/y-only stub — no `scroll()` alias, no options object, no Promise, `scrollIntoView` ignores its argument

**Статус:** FIXED 2026-09-02
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs:6206-6227` element scroll methods,
`dom.rs:11619-11632` `window.scrollTo`/`scroll`/`scrollBy`)
**Найден:** WPT-RUN-3 срез 4 (`ROADMAP.md`) — массовый прогон `css/cssom-view`

## Симптом

Three distinct, code-confirmed gaps in the same small block of methods:

```js
scrollTo: function(x, y) { ... _lumen_request_scroll(nid, +x, +y); },
scrollBy: function(x, y) { ... },
scrollIntoView: function() {
    // Scroll the nearest ancestor scroll container to make this element visible.
    var r = _lumen_get_bounding_rect(nid);
    ...
},
```

1. **No `scroll()` alias.** CSSOM View defines `Element.scroll()` as a
   synonym of `scrollTo()`; only `scrollTo`/`scrollBy`/`scrollIntoView` are
   defined here — `element.scroll is not a function` on any test that uses
   the alias (`dom-element-scroll.html`, `elementScroll.html`,
   `element-scroll-arguments.html`).
2. **No `ScrollToOptions` support beyond reading `x.left`/`x.top`.**
   `scrollTo`/`scrollBy` read `x.left`/`x.top` out of an options object but
   drop `behavior` entirely (always an instant jump —
   `_lumen_request_scroll` takes no behavior flag); `scrollIntoView` takes
   **no parameter at all**, so `{block, inline, behavior}` and legacy
   boolean `alignToTop` are silently ignored — explains
   `scrollIntoView-scrollMargin.html`, `scrollIntoView-multiple-nested.html`,
   `scroll-behavior-*` clusters (`scrollX`/`scrollTop` staying at the
   pre-call value in several of these, consistent with instant-jump-only
   behavior not matching what the test set up to observe async/smooth
   completion).
3. **No `Promise` return value.** A newer revision of the CSSOM View
   scroll-behavior spec has `scrollTo`/`scrollBy`/`scrollIntoView` return a
   `Promise` that resolves once the (possibly smooth/async) scroll finishes;
   here all three return `undefined`, so
   `element.scrollTo(...).then(...)` throws `Cannot read properties of
   undefined (reading 'then')` — `element-scroll-promises.html`,
   `window-scroll-promises.html`, `element-scroll-promise-interruption.html`
   (24+18+14 NOTRUN/TIMEOUT subtests, the largest single non-clustered block
   in this slice after BUG-475/476).

## Что нужно

Not a single fix — three separable pieces of work: (1) `scroll` = alias for
`scrollTo`, cheap; (2) thread a `behavior` flag through
`_lumen_request_scroll` and give `scrollIntoView` a real options parameter
(`block`/`inline`/`behavior`, plus `scroll-margin` per CSS Scroll Anchoring/
`scrollIntoView`'s own algorithm); (3) return a `Promise` from all three,
resolved by the shell once the corresponding scroll actually lands (needs a
completion signal from the scroll-animation driver, not just fire-and-forget
`_lumen_request_scroll`).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/cssom-view/` for files whose
dominant/sole cause is one of these three gaps, `expected: FAIL`/`TIMEOUT`/
`NOTRUN` per the actual run.

## Fixed 2026-09-02 (P3)

All three pieces closed in one session (shim text moved to `crates/js/src/
shim/*.js` since this bug was filed — `web_api_shim_mid.js` for `Element`,
`web_api_shim_tail_mc.js` for `window`, plus a new shared block in
`web_api_shim_head.js`):

1. **`scroll()` alias.** `Element.prototype.scroll` now delegates to
   `scrollTo` (`window.scroll` already existed).
2. **Options.** `scrollTo`/`scrollBy` (`Element` and `window`) parse
   `behavior` (container scrolling stays instant either way — the spec
   allows a UA-defined `auto`/`smooth`, and there is no per-container
   animation driver to hook into). `scrollIntoView(arg)` now parses the
   legacy boolean and `{block, inline, behavior}` through
   `_lumen_parse_scroll_into_view_opts` (`web_api_shim_head.js`) — an
   unrecognised enum member throws `TypeError`, matching WebIDL enum
   coercion — and actually drives the alignment on both axes via a new
   `_lumen_align_scroll(contentPos, targetSize, clientSize, curScroll,
   align)` helper implementing CSSOM View §6 step 3's start/center/end/
   nearest maths, for the nearest scrollable-ancestor branch. Fixing this
   surfaced a latent geometry bug in the OLD single-branch formula: it
   computed `r[axis] - pr[axis]` (target's CURRENT on-screen offset within
   the container) and used that directly as the new absolute scroll value,
   which only produces the intended "start" alignment when the container's
   scroll offset happens to be 0 — at any other scroll position it was off
   by exactly that offset. The new code folds the container's current
   scroll back in (`contentPos = (r[axis]-pr[axis]) + curScroll[axis]`)
   before applying the alignment formula, so `'start'` (and the other three)
   are now correct regardless of where the container was scrolled to when
   the call was made.
3. **Promise.** All six affected methods (`Element.scrollTo`/`scroll`/
   `scrollBy`, `window.scrollTo`/`scroll`/`scrollBy`, plus
   `scrollIntoView` on both paths) now return a `Promise`, settled through
   one shared helper (`_lumen_scroll_settle_promise`,
   `web_api_shim_head.js`): it resolves on the queued request's own
   `scrollend`, or — for the case where the request never moves anything at
   all (the page-scroll path drops `scroll`/`scrollend` entirely for a
   no-op `window.scrollTo`; an element/container scroll dispatches the
   event pair unconditionally whenever the target node is found in the
   layout tree, which a truly detached node never is) — after one round
   trip of two nested `requestAnimationFrame` calls, comparing the sampled
   scroll position before/after. No native (Rust) change was needed: the
   frame ordering already guarantees the drain-and-dispatch step for a
   queued request runs before that round trip's second `rAF` callback (see
   `on_redraw_requested`'s documented step order and the element-scroll
   drain in `about_to_wait.rs`), and once the position has moved even once
   the fallback stops checking and leaves resolution entirely to the real
   `scrollend` — so a long smooth animation is never resolved early.

8 new unit tests (`crates/js/src/dom/tests/v8_elem_geometry_scroll.rs`,
`v8_dragdrop_scroll_pointer.rs`): the alias, `instanceof Promise` on both
`Element` and `window`, resolution via `scrollend` (element and window),
resolution via the rAF fallback (element and window), `block: 'end'`
alignment landing at the expected absolute scroll_y, and the `TypeError` on
an invalid `block` enum member.

**Residual, filed separately as [BUG-961](bugs/BUG-961-OPEN.md):**
`scroll-margin-*` is not folded into `scrollIntoView`'s alignment target —
there is no JS-facing accessor for an element's resolved scroll-margin at
all today, so `scrollIntoView-scrollMargin.html` still fails. Also
untouched: inline (horizontal) alignment for the page-level fallback branch
(an element with no scrollable ancestor) — `window.scrollX` is hardcoded to
0 and there is no tracked horizontal page scroll at all, a separate,
previously undocumented gap outside this bug's three named pieces.

Gates: `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` clean; `cargo test -p lumen-js --features v8-backend`
3488/3488 (whole crate, including the 8 new tests).
