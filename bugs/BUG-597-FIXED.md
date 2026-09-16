# BUG-597: Drag-and-drop surface incomplete -- `DragEvent` constructor defaults/validates wrong, `draggable` ignores `<a>`/`<img>` default-true, `window`/`document` missing `ondrag*` handlers

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` -- `DragEvent` constructor, `HTMLElement.prototype` `draggable` getter; `crates/js/src/shim/web_api_shim_mid_b.js` -- `window`/`document` `ondrag*`)
**Найден:** P2, WPT-VENDOR-html-editing, 2026-08-04

## Симптом

```
FAIL DragEvent constructor with null as the dataTransfer parameter should be able to fire the event - assert_true: expected true got false
FAIL DragEvent constructor with undefined as the dataTransfer parameter should be able to fire the event - assert_true: expected true got false
FAIL DragEvent constructor with custom object as the dataTransfer parameter should throw TypeError - assert_throws_js: ... did not throw
FAIL an <a> element should be draggable by default - assert_true: expected true got false
FAIL an <img> element should be draggable by default - assert_true: expected true got false
FAIL ondragstart in window - assert_true: expected true got false
FAIL ondragstart in document - assert_true: expected true got false
(same pattern for ondrag/ondragover/ondragenter/ondragleave/ondrop/ondragend, both window and document -- 14 subtests)
```
(`dnd/synthetic/001.html` -- `dataTransfer` default/validation; `dnd/dom/specials.html` -- `window`/`document` `ondrag*` handlers; `dnd/dom/draggable.html` -- `<a>`/`<img>` default-draggable)

## Причина

Three independent gaps in the same feature area, all confirmed by direct
code read:

1. **`DragEvent` constructor** (`dom.rs:781-790`): when `init.dataTransfer`
   is `null`/`undefined`/omitted, the constructor synthesizes a *new*
   `DataTransfer()` instead of leaving `this.dataTransfer` as `null` (the
   `DragEventInit` dictionary's spec default). The doc comment even states
   the intent ("If no DataTransfer provided, create a fresh one for new drag
   operations") -- correct for the internal `_lumen_dispatch_drag_event`
   helper (`dom.rs:794-810`, which always passes an explicit `dataTransfer`)
   but wrong for the public constructor called directly from page script.
   There is also no type check: `new DragEvent('x', {dataTransfer: {}})`
   silently accepts the plain object instead of throwing `TypeError` per
   WebIDL (`DataTransfer?` is a nullable interface type, not `any`).

2. **`draggable` default** (`dom.rs:2776-2780`): the getter returns `false`
   whenever the `draggable` content attribute is absent, for every tag.
   HTML LS §9.10 gives `<a href>` and `<img>` (and a couple of other cases)
   a *default-true* draggable state absent the attribute -- the getter never
   branches on `nid`'s tag name to apply that default.

3. **`window`/`document` `ondrag*` handlers** (`dom.rs:7639` object literal):
   `window` only lists `onpopstate`/`onhashchange`/`onmessage`/`onpageshow`/
   `onpagehide`/`onload` -- none of the seven `GlobalEventHandlers` drag
   properties (`ondragstart`/`ondrag`/`ondragend`/`ondragenter`/`ondragover`/
   `ondragleave`/`ondrop`) that HTML LS's `WindowEventHandlers`/
   `GlobalEventHandlers` mixin requires on both `Window` and `Document`.
   `Element.prototype` already has all seven (`dom.rs:2784-2790`), so this is
   specifically a `window`/`document` gap, not a missing feature overall --
   worth checking whether the same object literal is missing other
   `GlobalEventHandlers` members beyond the drag set (not verified here,
   out of scope for this slice).

## Масштаб

`dnd/synthetic/001.html`: 15/16 unexpected (shared with BUG-596, the
`dataTransfer`-specific subtests are the ones attributable here). Draggable-
default (`dnd/dom/draggable.html`) and window/document handler gaps
(`dnd/dom/specials.html`, 14 subtests: 7 handlers x window+document) each
confirmed via dedicated subtests in the two files above.

## Исправлено (2026-09-16, P3)

`dom.rs` was long since split (`crates/js/src/dom/`); the three gaps live in
the shared shim (`crates/js/src/shim/*.js`), not in the Rust file the report
named:

1. **`DragEvent` constructor** (`web_api_shim_mid.js`): now defaults
   `dataTransfer` to `null` when `init.dataTransfer` is absent/`null`/
   `undefined` (spec default of the nullable `DragEventInit.dataTransfer`
   member), instead of synthesizing a fresh `DataTransfer`. A non-`null`
   value that isn't a `DataTransfer` instance now throws `TypeError`, per
   WebIDL. The internal `_lumen_dispatch_drag_event` helper is unaffected --
   it always passes an explicit `DataTransfer` instance.
2. **`draggable` default** (`web_api_shim_mid.js`, `HTMLElement.prototype`
   getter): when the `draggable` content attribute is absent, the getter now
   checks the tag name and returns `true` for `<img>` and `<a href>` (HTML LS
   §9.10.1's "auto" state), `false` otherwise. An explicit attribute value
   still wins.
3. **`window`/`document` `ondrag*` handlers**: already fixed as a side effect
   of BUG-874 (`_LUMEN_EVENT_HANDLER_ATTRS` already lists all seven
   `ondrag*` names and is looped over both `window` and `document` to declare
   any name from the curated list not already present) -- confirmed still
   correct, not re-implemented.

New/updated tests in `dom::tests::v8_dragdrop_scroll_pointer`:
`drag_event_data_transfer_defaults_to_null_not_a_fresh_instance`,
`drag_event_data_transfer_null_and_undefined_both_resolve_to_null`,
`drag_event_data_transfer_rejects_non_data_transfer_object`,
`drag_event_data_transfer_accepts_real_data_transfer`,
`draggable_defaults_true_for_img_and_a_with_href`,
`draggable_explicit_attribute_overrides_default`,
`ondrag_handlers_are_detectable_on_window_and_document` (regression guard for
point 3). `cargo test -p lumen-js --features v8-backend` green (3713/3713),
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
clean.
