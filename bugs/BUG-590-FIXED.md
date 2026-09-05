# BUG-590: `document.createEvent` missing entirely; `beforeunload` dispatched via `dispatchEvent()` doesn't invoke the handler

**Статус:** FIXED 2026-09-05
**Компонент:** js (`crates/js/src/shim/web_api_shim_head.js`, `web_api_shim_mid.js`)
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

**WPT-RUN-6 срез 38 (2026-09-02):** classifies one more TIMEOUT id,
`uievents/legacy-domevents-tests/approved/dispatchEvent.click.checkbox.html`.
That test's `TestEvent` handler calls `document.createEvent("MouseEvent")`
*inside* a `test()` callback (so `testharness.js` catches the `TypeError`
and fails only that subtest) but then calls `TARGET.dispatchEvent(e)`
*outside* the callback, in the raw `TestEvent` function body — a
native-event listener invoked by `BUTTON.click()`. Measured live
(`--mcp-live-port` + `tests/wpt/serve_wpt_like.py`, reduced to a
`document.createEvent` call inside a native `click` listener): the
`TypeError` prints nowhere (no `script error:`, no `[JS error]`) and script
execution resumes on the next top-level statement — the same swallow
[BUG-871](bugs/BUG-871-OPEN.md) already describes for `message` listeners,
here for a native click dispatch instead. `TARGET.dispatchEvent(e)` with `e`
left `undefined` never runs, `TARGET` never gets its `click`, and the
harness never reaches `done()`. `legacy-create-event-missing` marker added
to `tests/wpt/timeout_audit.py::SOURCE_MARKERS`; script —
`tests/wpt/verify_slice38_gaps.py`.

## Причина устранена

`document.createEvent(interface)` added (DOM §2.2 legacy factory) — a
lower-case interface-name switch (`_lumen_legacy_event_ctor` in
`web_api_shim_head.js`) covering `Event`/`CustomEvent`/`UIEvent`/
`MouseEvent`/`KeyboardEvent`/`FocusEvent`/`HashChangeEvent`/`StorageEvent`/
`MessageEvent`/`DragEvent`/`CompositionEvent`/`BeforeUnloadEvent`, throwing a
`NotSupportedError` `DOMException` for anything else. The returned event
starts with an empty type; `Event.prototype.initEvent(type, bubbles,
cancelable)` (also new) fills it in, per spec forcing `isTrusted` back to
`false`.

Gap (2) from the original report — "`window.dispatchEvent(new
CustomEvent("beforeunload"))` never runs the handler" — did not reproduce
under a live-window probe (`--mcp-live-port`): the generic
`window.dispatchEvent` `on<type>` branch (BUG-392) already invokes
`onbeforeunload` for any event type, `beforeunload` included, and no code
path sets `defaultPrevented` from a returned string unless
`_lumen_fire_beforeunload` (the real navigation-unload path) is the caller —
which a script-authored `CustomEvent` never is. All 4 non-iframe subtests
of `beforeunload-canceling.html` pass once `createEvent` alone is fixed.

`legacy-create-event-missing`'s other id (`dispatchEvent.click.checkbox.html`,
srez 38 above) is unaffected by this fix for its own TIMEOUT reason: the
`document.createEvent("MouseEvent")` call itself no longer throws, but the
exception-swallow this file's listener depends on for signalling failure is
[BUG-871](bugs/BUG-871-OPEN.md), a separate open defect.

Tests: `crates/driver/tests/cases/bug590_create_event_beforeunload.rs`
(`InProcessSession::eval`, 6 cases — interface mapping, unknown-interface
`NotSupportedError`, `initEvent` retargeting, the three CustomEvent
subtests, and a regression guard on `_lumen_fire_beforeunload`, which this
fix does not touch).
