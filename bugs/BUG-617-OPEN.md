# BUG-617: `ImageBitmapRenderingContext` has no global constructor (plain object literal instead of a class) and `OffscreenCanvas.getContext()` ignores `'bitmaprenderer'`/`'webgpu'` entirely

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` `getContext` factory ~line 3083, `crates/js/src/offscreen_canvas.rs::getContext` ~line 454)
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
