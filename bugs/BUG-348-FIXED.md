# BUG-348 — `canvas.getContext('2d')` returns `null` unconditionally under the V8 default build: WebGL shim clobbers the 2D context accessor

**Статус:** FIXED 2026-07-29 (P3)
**Компонент:** js (`crates/js/src/webgl_canvas.rs`, `crates/js/src/v8_runtime.rs`)
**Найден:** P2, WPT-VENDOR-density-size-correction (2026-07-26), while triaging a `TypeError: Cannot set properties of null (setting 'fillColor')` and a TIMEOUT in the vendored `density-size-correction` category — both tests do nothing more exotic than `document.createElement('canvas').getContext('2d')`.

## Симптом

`HTMLCanvasElement.getContext('2d')` returns `null` for **every** canvas, appended or
detached — not a WPT/BiDi-specific quirk. Confirmed three ways:

1. Two `density-size-correction` WPT tests fail/hang on the very first
   `canvas.getContext('2d')` call (`resources/exify.js`'s `createImageWithMetadata`,
   `density-corrected-image-in-canvas.html`'s `createCanvasWithImage`).
2. An isolated repro over plain WebDriver BiDi (`--bidi-port`, no wptrunner/wptserve
   involved at all) against a bare `file://` page: both a detached and a
   `document.body`-appended canvas return `ctx === null`, while
   `_lumen_get_tag_name`/`_lumen_canvas_is_transferred`/`_lumen_canvas_dims` all report
   correct values (`tag: "canvas"`, `transferred: false`, `dims: [10, 20]`) for the same
   node id in the same synchronous eval — ruling out both of `getContext`'s own
   null-return guards (`crates/js/src/dom.rs:5963-5964`).
3. `lumen --screenshot .tmp/canvas2d-check.png graphic_tests/57-canvas-2d.html`
   (CPU renderer, **not** BiDi) renders all six canvases as blank UA-background
   rectangles — no `fillRect`/`arc`/path/`strokeRect` content at all. So this is not
   headless/BiDi-specific; it's the current state of `main`'s default (V8) build.

## Причина

`crates/js/src/v8_runtime.rs:3858-3865` installs canvas-related JS shims in this order
(comment: "Mirrors `lib.rs::install_dom`'s ordering (webgl before canvas2d)"):

```rust
install_webgl_canvas_v8(self, &fingerprint)?;   // webgl_canvas.rs
install_v8!(canvas2d::install_canvas2d_bindings_v8);
```

`webgl_canvas.rs`'s `WEBGL_SHIM` (used by both the QuickJS and V8 ports) monkey-patches
`document.createElement`:

```js
// webgl_canvas.rs:484-493
var _origCreate = document.createElement.bind(document);
document.createElement = function(tag) {
  var el = _origCreate(tag);
  if (typeof tag === 'string' && tag.toLowerCase() === 'canvas') {
    _addCanvasStubs(el);   // <-- overwrites el.getContext unconditionally
  }
  return el;
};
```

and `_addCanvasStubs` (`webgl_canvas.rs:462-478`) sets `el.getContext` to a function that
only handles `'webgl'/'webgl2'/'experimental-webgl'` and **returns `null` for every other
`contextType`, including `'2d'`**:

```js
el.getContext = function(contextType) {
  var t = ('' + (contextType || '')).toLowerCase();
  if (t === 'webgl' || t === 'webgl2' || t === 'experimental-webgl') { ... return _ctx; }
  return null;
};
```

The real, functional `'2d'`/`'bitmaprenderer'`/`'webgpu'` implementation lives in
`crates/js/src/dom.rs:5959-6013`, installed as part of the base element-wrapper factory
(`WEB_API_SHIM`, ordinary `document.createElement`) — i.e. *before* `webgl_canvas`'s
wrapper runs. `canvas2d.rs`'s own V8 install (`install_canvas2d_bindings_v8`) registers
only the native `_lumen_canvas2d_*` bindings the `dom.rs` shim's `getContext('2d')`
branch calls into — it does **not** touch `document.createElement`/`getContext` itself
(confirmed: no match for either in `canvas2d.rs`). So the WebGL wrapper's own-property
`el.getContext` assignment is the *last* write and permanently shadows the working `2d`
accessor for every canvas created after JS-runtime bootstrap — there is nothing
downstream that restores it.

This is a straightforward install-order bug from `P3-v8-s8` (`26309f646`, 2026-07-14,
same day as the V8 default cutover, ADR-018) — the QuickJS rollback path shares the same
`WEBGL_SHIM` source and is presumably equally affected, but V8 is what every graphic
test / WPT run since 2026-07-14 has actually exercised.

## Предлагаемый фикс (not applied — filing only, P2 does not own `crates/js`)

`_addCanvasStubs`'s `getContext` override should fall through to whatever
`document.createElement`'s *original* wrapper already put on `el` for any
`contextType` it doesn't itself handle, instead of unconditionally returning `null`:

```js
el.getContext = function(contextType) {
  var t = ('' + (contextType || '')).toLowerCase();
  if (t === 'webgl' || t === 'webgl2' || t === 'experimental-webgl') { ... return _ctx; }
  return _origGetContext ? _origGetContext.call(el, contextType) : null;
};
```

capturing `el.getContext` (the dom.rs-installed one) as `_origGetContext` before
overwriting it. Needs a matching fix in both the QuickJS (`install_webgl_canvas`) and V8
(`install_webgl_canvas_v8`) shim strings since they share `WEBGL_SHIM` verbatim — and a
regression test/graphic-test rerun of `57-canvas-2d.html` to confirm the fillRect/arc/
path/strokeRect boxes actually render again.

## Масштаб

Breaks `<canvas>` 2D for every canvas built with `document.createElement` on the current
default engine build — not just this WPT category. Canvases that reach JS by any other
route (`getElementById`/`querySelector` over parsed markup) were never affected: the WebGL
wrapper only runs inside `document.createElement`.

## Фикс (2026-07-29, P3)

`_addCanvasStubs` now captures the element factory's own `getContext` before overwriting
it and **delegates** to it (`_origGetContext.apply(this || el, arguments)`) for every
`contextType` the WebGL branch does not handle, instead of returning `null`. Two
neighbouring clobbers of the same kind were closed with it:

- `toDataURL`/`toBlob` are installed **only if absent** — `dom.rs`'s `toDataURL` returns
  a blank 1×1 PNG (ADR-007), a valid image, whereas the WebGL stub's `'data:,'` is not a
  decodable image URL and was winning on every created canvas.
- One context per canvas (HTML LS §4.12.4) is preserved explicitly: once a WebGL context
  has been handed out for an element, other types answer `null` rather than falling
  through. Without this line the delegation would have *added* a new deviation (2D after
  WebGL on the same canvas), since the old blanket `return null` happened to satisfy the
  rule by accident.

Verified end-to-end on the default (V8) `dev-release` build with an instrumented probe
page (`--screenshot`, probe prints its findings into the DOM), before → after:

| probe | before | after |
|---|---|---|
| `createElement('canvas').getContext('2d')` | `NULL` | `object`, `fillRect` present, stable identity, `.canvas` back-ref |
| `createElement('canvas').toDataURL()` | `data:,` | `data:image/p…` (blank PNG) |
| `getContext('webgl')` | `object` | `object` (unchanged: `drawArrays`, `.canvas`, same object on repeat, `webgl2` alias) |
| `getContext('nosuch')`, `<div>.getContext('webgl')` | `null` | `null` |

Regression tests in `webgl_canvas.rs`: `get_context_2d_delegates_to_element_factory`,
`get_context_2d_is_null_after_webgl`, `get_context_unknown_type_returns_null`,
`canvas_privacy_stubs_do_not_clobber_element_factory`. The test helper
`install_minimal_dom` now models `dom.rs`'s factory (its element carries a `getContext`
and a `toDataURL`) — the old helper handed back an element with neither, so the previous
`get_context_2d_returns_null` test asserted the buggy policy *and* would have passed
either way. `non_canvas_has_no_get_context` became `non_canvas_gets_no_webgl_stub` for the
same reason: in production every element has a `getContext`, so the discriminator is the
WebGL context, not the method's presence.

### Не входит в этот фикс

Evidence item 3 of the report (blank `--screenshot` of `graphic_tests/57-canvas-2d.html`)
is **not** this bug: that page reaches its canvases via `getElementById`, which was always
served by the working `dom.rs` accessor. Its blankness is a separate, pre-existing gap in
the headless CPU render path — filed as [BUG-428](BUG-428-OPEN.md). The committed
reference `graphic_tests/snapshots/cpu/57-canvas-2d.png` is blank for the same reason, so
this fix does not change any CPU snapshot.
