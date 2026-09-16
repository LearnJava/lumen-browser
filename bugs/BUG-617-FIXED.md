# BUG-617: `ImageBitmapRenderingContext` has no global constructor (plain object literal instead of a class) and `OffscreenCanvas.getContext()` ignores `'bitmaprenderer'`/`'webgpu'` entirely

**Статус:** FIXED (2026-09-16, P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js::HTMLCanvasElement.prototype.getContext`, `crates/js/src/offscreen_canvas.rs::getContext`)
**Найден:** P2, WPT-VENDOR-imagebitmap-renderingcontext, 2026-08-04

## Симптом

Two of the category's failures are a distinct `ReferenceError`, not the
`createImageBitmap` rejection from [[BUG-616]]:

```
FAIL Test that canvas.getContext('bitmaprenderer') returns an instance of
ImageBitmapRenderingContext - ImageBitmapRenderingContext is not defined
```

(`context-creation.html`, on-page `<canvas>`; `context-creation-
offscreen.html`, `new OffscreenCanvas(...)` — same error on both).

Confirmed live (`--mcp-live-port`):

```js
typeof window.ImageBitmapRenderingContext
// → "undefined"

var c = document.createElement('canvas');
c.getContext('bitmaprenderer').constructor.name
// → "Object"   (should be "ImageBitmapRenderingContext")

new OffscreenCanvas(10, 10).getContext('bitmaprenderer')
// → null
```

## Причина

Two independent gaps in the same feature, both real bugs on their own:

1. **No global class at all.** `dom.rs`'s `getContext` factory (the branch
   at `t === 'bitmaprenderer'`, ~line 3083) builds the context as a bare
   object literal — `var brctx = { canvas: this, transferFromImageBitmap:
   function(bitmap) {...} };` — with no `ImageBitmapRenderingContext`
   constructor anywhere in `WEB_API_SHIM` for it to be an instance of.
   Every real browser exposes `window.ImageBitmapRenderingContext` as a
   constructible (well, spec says "no constructor operation", but still a
   named interface) global, so `instanceof ImageBitmapRenderingContext` —
   a standard WebIDL-conformance idiom, used here and certain to recur in
   other vendored categories that touch canvas contexts — throws
   `ReferenceError` instead of evaluating to `true`/`false`. This affects
   the on-page-`<canvas>` path (which otherwise works correctly — see
   `context-preserves-canvas.html`, 1/1 passed) equally with the
   `OffscreenCanvas` path.

2. **`OffscreenCanvas.getContext()` only implements `'2d'`.**
   `offscreen_canvas.rs:454-457`:
   ```js
   getContext(contextType, options) {
     if (contextType !== '2d') {
       return null;
     }
     ...
   ```
   unconditionally returns `null` for `'bitmaprenderer'` and `'webgpu'`,
   even though the on-page `<canvas>` factory in `dom.rs` (reused by
   `webgl_canvas.rs::_addCanvasStubs`, see its own comment "the element
   factory has already installed the working '2d'/'bitmaprenderer'/
   'webgpu' accessor") handles both. `OffscreenCanvas` is a separate
   hand-written `class` in `offscreen_canvas.rs` with its own `getContext`
   method — it never delegates to `dom.rs`'s factory, so the
   `'bitmaprenderer'` branch that exists for on-page canvases was never
   ported over when `OffscreenCanvas` was implemented. This means
   `new OffscreenCanvas(...).getContext('bitmaprenderer')` cannot work at
   all today, independent of gap 1 above.

## Масштаб

Both gaps are hit by this category's `context-creation*.html` tests (2 of
the 19 unexpected results — the rest are [[BUG-616]]). Gap 1 is the
broader risk: any future WPT category doing `x instanceof
<SomeContextInterface>` on a canvas-family context will hit the same
`ReferenceError` pattern if that context is likewise built as a plain
object literal — worth an audit of `dom.rs`'s other `getContext` branches
(`'2d'`, `'webgpu'`) for the same missing-constructor gap when picked up.
Not investigated here (out of scope for this category's vendoring pass).

## Фикс (2026-09-16)

**Gap 1.** `web_api_shim_mid.js`'s `CanvasRenderingContext2D` section gained
an `ImageBitmapRenderingContext` global next to it (same "throws on direct
`new`, tagged via `_lumen_idl_tag`" pattern). The `'bitmaprenderer'` branch of
`HTMLCanvasElement.prototype.getContext` now builds its context object via
`Object.create(ImageBitmapRenderingContext.prototype)` instead of a bare
`{...}` literal, so `instanceof`/`constructor.name`/`Symbol.toStringTag`
all resolve correctly for the on-page `<canvas>` path.

**Gap 2.** `OffscreenCanvas.getContext()` (`offscreen_canvas.rs`) gained a
`'bitmaprenderer'` branch mirroring the on-page one. Its own JS shim
(`OFFSCREEN_CANVAS_SHIM`) defines `ImageBitmapRenderingContext`
self-containedly — reusing the page's global when present (same
reuse-or-define-locally shape already used there for
`CanvasGradient`/`CanvasPattern`/`TextMetrics`/`ImageData`), since this
module's own bindings can in principle be installed without the page shim
having run first (its own V8 unit-test harness does exactly that, and
`OffscreenCanvas` is not yet wired into real Worker threads at all —
`worker.rs::run_worker_thread_v8`'s doc comment). `transferFromImageBitmap`
on the offscreen side is backed by a new native,
`_lumen_offscreen_bitmaprenderer_transfer_from_image_bitmap`
(`offscreen_canvas.rs`), which replaces the target `OffscreenCanvas`'s whole
backing `Context2D` with the source bitmap's pixels — the offscreen-side
counterpart of `canvas2d.rs::bitmaprenderer_transfer_native`, which presents
onto a page `<canvas>` by `nid` instead.

`'webgpu'` on `OffscreenCanvas` stays unimplemented — real feature work
(a `GPUCanvasContext` bound to an off-DOM backing store), not this bug's
missing-constructor/missing-branch shape, and not exercised by this
category's tests.

Tests: `offscreen_canvas.rs::tests_v8` (context is a real class instance,
cached, `transferFromImageBitmap` round-trips pixels dst←src, `null`
clears, invalid-argument throws) and
`dom/tests/v8_core/canvas_interface_membership.rs`
(`bitmaprenderer_context_is_an_instance_of_its_global_interface`, on-page
path). Gate: `cargo clippy --workspace --all-targets -- -D warnings` чист;
`scripts/scoped-test.sh` — единственный красный,
`cases::snapshot_cpu::cpu_snapshots_match_references`, посторонний дрейф
(BUG-1008, побайтово та же сигнатура 7 файлов).
