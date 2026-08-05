# BUG-622: `document.defaultView` is missing entirely (should return `window`)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — live `document` object literal, ~`dom.rs:6989+`)
**Найден:** P2, WPT-VENDOR-inert, 2026-08-04

## Симптом

Confirmed live (`--mcp-live-port`):

```js
typeof document.defaultView       // → "undefined" (should be "object")
document.defaultView === window   // → false        (should be true)
```

`grep -c defaultView crates/js/src/dom.rs` is 0 — not a broken getter,
the property doesn't exist at all on the live `document` object (same
diagnostic pattern as [[reference_shim_dual_document_split]]: check
`'prop' in document`, not `document.prop !== undefined`, to tell "no
property" from "property that evaluates to undefined").

## Масштаб

Found via `inert-does-not-match-disabled-selector.html`:
`document.defaultView.getComputedStyle(button)` throws `Cannot read
properties of undefined (reading 'getComputedStyle')` — the test uses
`document.defaultView.getComputedStyle` instead of the equivalent global
`getComputedStyle`, a common defensive-coding idiom in WPT tests (works
in an iframe/detached-document context where the global `getComputedStyle`
isn't guaranteed to resolve to the right window). 1 file directly hit in
this category; likely a wider-impact gap since `defaultView` is one of the
most basic `Document` properties (WHATWG DOM §3.5) and a common pattern in
test helper libraries (`elem.ownerDocument.defaultView`) for reaching
"the window a node belongs to" without assuming `window` is in scope.

Fix shape: add `get defaultView() { return window; }` to the live
`document` object literal — same one-line pattern as other single-value
accessors on that object.

## Реконфирмация 2026-08-05 (WPT-VENDOR-pointerevents)

The predicted "wider-impact" played out: after fixing the separate
vendoring gap [BUG-654](BUG-654-FIXED.md) (`test_driver.Actions` was
undefined, masking everything downstream of it), this became the single
largest failure cluster in the corrected `pointerevents` run — dozens of
subtests across `setPointerCapture`/boundary-event/predicted-list tests,
each surfacing as `Error: Browsing context for element was detached`
(misleading text, same root cause) from
`tools/wptrunner/wptrunner/testdriver-extra.js::get_context`'s
`element.ownerDocument.defaultView` check. Confirms this is worth
prioritizing — it silently degrades signal quality for every WPT category
whose tests route through `test_driver`'s element-targeted helpers
(`send_keys`, `action_sequence`, `get_computed_label`, etc.), not just the
one file already on record.
