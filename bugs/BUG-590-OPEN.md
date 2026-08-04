# BUG-590: `document.createEvent` missing entirely; `beforeunload` dispatched via `dispatchEvent()` doesn't invoke the handler

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — grep for `createEvent` returns zero hits; `onbeforeunload`/`beforeunload` listener dispatch path)
**Найден:** P2, WPT-VENDOR-html-browsers, 2026-08-04

## Симптом

```
document.createEvent is not a function
FAIL Returning a string must not cancel the event: CustomEvent, non-cancelable - assert_true: CustomEvent must be able to trigger the event handler expected true got false
```

`html/browsers/browsing-the-web/unloading-documents/beforeunload-canceling.html`
— 3 subtests fail because `window.dispatchEvent(new CustomEvent("beforeunload"))`
never runs the `onbeforeunload`/listener handler; 1 more fails because
`document.createEvent("BeforeUnloadEvent")` throws `TypeError: … is not a
function`.

## Причина

Two small, independent gaps: (1) `document.createEvent` (legacy DOM3 Events
factory, still required by spec for compatibility) is not implemented at
all — no method on `Document.prototype`. (2) manually dispatching a
`beforeunload` event through the generic `dispatchEvent()` path doesn't
reach whatever internal wiring calls the page's `onbeforeunload`
handler/listeners — either `beforeunload` is special-cased to only fire from
the engine's own navigation-unload sequence, or the generic dispatch path
doesn't route to it at all.

## Масштаб

Both gaps are local to this one test file's 4 failing subtests; the rest of
the file (checking that returning a string from the handler does or doesn't
cancel the navigation) wasn't reached because the handler never fired.
