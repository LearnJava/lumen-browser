# BUG-596: Legacy `initEvent`/`initUIEvent`/`initMouseEvent` missing entirely on `Event`/`UIEvent`/`MouseEvent`

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_head.js` -- `Event.prototype`; `crates/js/src/shim/web_api_shim_mid.js` -- `UIEvent.prototype`/`MouseEvent.prototype`)
**Найден:** P2, WPT-VENDOR-html-editing, 2026-08-04

## Симптом

```
FAIL DragEvent should have all of the inherited init*Event methods - assert_true: initMouseEvent expected true got true
  (evt.initMouseEvent is not a function)
FAIL initMouseEvent should not throw - evt.initMouseEvent is not a function
FAIL initUIEvent should not throw - evt.initUIEvent is not a function
FAIL initEvent should not throw - evt.initEvent is not a function
FAIL initMouseEvent should be able to fire the event - evt.initMouseEvent is not a function
FAIL initUIEvent should be able to fire the event - evt.initUIEvent is not a function
FAIL initEvent should be able to fire the event - evt.initEvent is not a function
FAIL initMouseEvent should give null as the dataTransfer - evt.initMouseEvent is not a function
FAIL initUIEvent should give null as the dataTransfer - evt.initUIEvent is not a function
FAIL initEvent should give null as the dataTransfer - evt.initEvent is not a function
```
(`dnd/synthetic/001.html` -- fully self-contained, no `testdriver`, no
cross-file dependency; 1/16 subtests pass, the other 15 all trace back to
this one gap)

## Причина

DOM Standard §2.5 ("Legacy") and UI Events §3.6/§4.5 still require
`Event.prototype.initEvent(type, bubbles, cancelable)`,
`UIEvent.prototype.initUIEvent(type, bubbles, cancelable, view, detail)`, and
`MouseEvent.prototype.initMouseEvent(...)` -- deprecated in favor of the
constructor form but not removed, and still exercised by legacy code and by
this exact WPT suite. None of the three exist anywhere in the codebase; only
the modern `new Event(...)`/`new MouseEvent(...)` constructor path is
implemented, so any script that mutates an already-constructed event via the
legacy `init*` methods (a still-common pattern for synthesizing events in
place, e.g. before re-dispatching a cloned/reused event object) throws
`TypeError: ... is not a function`.

## Масштаб

Confirmed via one fully self-contained file (`dnd/synthetic/001.html`, zero
external dependencies): 15 of 16 subtests fail, all downstream of these three
missing methods. Any WPT file anywhere in the vendored tree that uses the
legacy init pattern inherits the same failure -- not specific to drag events.

## Исправлено (2026-09-16, P3)

`Event.prototype.initEvent` was already present in
`web_api_shim_head.js` by the time this session picked up the bug -- the
"zero-hit" grep in the original triage was stale, not a live gap.
`UIEvent.prototype.initUIEvent` and `MouseEvent.prototype.initMouseEvent`
were genuinely missing; added to `web_api_shim_mid.js`, each delegating to
its parent's legacy init method and then overwriting only the
fields the spec adds at that level (`initUIEvent`: `view`/`detail`;
`initMouseEvent`: `screenX`/`screenY`/`clientX`/`clientY`/`ctrlKey`/`altKey`/
`shiftKey`/`metaKey`/`button`/`relatedTarget`, plus the `x`/`y` legacy
aliases). Living on the shared prototype chain, both are inherited for free
by every subclass (`DragEvent`, `WheelEvent`, `PointerEvent`, …) -- exactly
what `dnd/synthetic/001.html`'s `DragEvent` case exercises. New test:
`dom::tests::v8_event_classes::legacy_init_event_methods_reinitialize_and_dispatch`.
`cargo test -p lumen-js --lib --features v8-backend` green (3707/3707),
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
clean.
