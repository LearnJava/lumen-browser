# BUG-480: `<iframe>` has no separate browsing context — `contentWindow`/`contentDocument` are absent from the JS shim entirely

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs`)
**Найден:** WPT-RUN-3 срез 4 (`ROADMAP.md`) — массовый прогон `css/cssom-view`

## Симптом

`grep -n "contentWindow\|contentDocument" crates/js/src/dom.rs` — zero
matches. Any test that creates an `<iframe>` and reaches into it via
`iframe.contentWindow`/`iframe.contentDocument` gets `undefined`, so the next
property access throws or the test hangs waiting on a promise/event that can
never fire inside a document that doesn't exist from JS's point of view.

## Уже отмечалось походя, но не заводилось отдельно

* [BUG-311](BUG-311-FIXED.md) (fixed): `Node.isConnected` — its own test note
  says "iframes remain FAIL — nested sub-documents through `contentDocument`
  aren't modeled", but the bug itself only covers `isConnected`.
* WPT-VENDOR-focus session (2026-07-28, not filed): "~13 subtests die because
  `<iframe>` has no browsing context at all, `contentWindow`/`contentDocument`
  = `null`" — noted in passing, never turned into its own `BUG-NNN`.

This entry is the first dedicated bug for the underlying gap itself.

## Масштаб находки (в этом срезе)

`tests/wpt/css/cssom-view/resources/matchMedia.js`'s `createIFrame()` helper
(used by all `MediaQueryList-*`/`matchMedia*` tests that need a resizable
sub-document to observe media-query changes in) awaits the iframe's `load`
event and then does `iframe.contentDocument.body.offsetWidth` — six
`MediaQueryList-*`/`MediaQueryListEvent.html` files TIMEOUT outright on this.
Also implicated: `elementsFromPoint-iframes.html`,
`scrollIntoView-iframes.html`, `scroll-behavior-subframe-root.html`,
`scroll-behavior-subframe-window.html`, `matchMedia-display-none-iframe.html`
— all TIMEOUT.

## Что нужно

A real nested browsing context: a second `Document`/`Window` pair per
`<iframe>`, `contentWindow` returning that `Window`, `contentDocument`
returning that `Document` (same-origin only, per HTML LS — cross-origin must
throw/return `null` for `contentDocument` while still returning a `Window`
for `contentWindow`). Large — likely its own multi-slice task, not a single
`BUG-NNN` fix; this entry documents the gap and its WPT blast radius so a
future task doesn't have to rediscover it from scratch.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/cssom-view/` for files whose
dominant/sole cause is this gap, `expected: TIMEOUT`/`FAIL`/`NOTRUN` per the
actual run.

## Срез 25 (`css/css-properties-values-api`, 2026-08-03)

`at-property-viewport-units.html` and `at-property-viewport-units-dynamic.html`
both build their entire test body inside `<iframe id=iframe srcdoc="...">` —
same gap, srcdoc-based sub-document never runs. Both file-level `TIMEOUT`
(zero subtests registered). `.ini` under
`tests/wpt/metadata/css/css-properties-values-api/` for both files.

## Срез 26 (`css/css-highlight-api`, 2026-08-03)

`HighlightRegistry-highlightsFromPoint.html`'s "returns empty array when
called on a display:none iframe document" subtest does
`iframe.contentWindow.document` — `contentWindow` is `null`, so the read
throws `Cannot read properties of null (reading 'document')` before the
test ever reaches `highlightsFromPoint()` (the other 6 subtests in the same
file fail on the unrelated [BUG-534](BUG-534-OPEN.md)). 1 subtest. `.ini`
under `tests/wpt/metadata/css/css-highlight-api/`.
