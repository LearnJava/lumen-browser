# BUG-479: `Element`/`Window` scroll API is a narrow x/y-only stub — no `scroll()` alias, no options object, no Promise, `scrollIntoView` ignores its argument

**Статус:** OPEN
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
