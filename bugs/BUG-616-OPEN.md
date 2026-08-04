# BUG-616: `createImageBitmap()` rejects `HTMLCanvasElement`/`HTMLImageElement`-cropped sources — only OffscreenCanvas/ImageData/whole-HTMLImageElement/Blob are handled

**Статус:** OPEN
**Компонент:** js (`crates/js/src/offscreen_canvas.rs`, `createImageBitmap` global installed near line 646)
**Найден:** P2, WPT-VENDOR-imagebitmap-renderingcontext, 2026-08-04

## Симптом

`imagebitmap-renderingcontext`'s run: **12/14 harness OK, 1/20 subtests
passed**. 15 of the 19 unexpected subtests fail with the identical error:

```
FAIL ... - promise_test: Unhandled rejection with value: object
"TypeError: createImageBitmap: unsupported source type"
```

(`context-creation.html`, `context-creation-with-alpha.html`,
`context-creation-offscreen-with-alpha.html`, both
`transferFromImageBitmap-ToBlob-*.html`, both
`transferFromImageBitmap-TransferToImageBitmap-*.html`,
`transferFromImageBitmap-detached.html`, both
`transferFromImageBitmap-null*.html`). One more,
`bitmaprenderer-as-imagesource.html`, hits the same rejection but TIMEOUTs
instead of FAILing — it uses `async_test` with a bare
`createImageBitmap(canvas).then(t.step_func(...))` and no `.catch`, so the
unhandled rejection never calls `t.done()` and the test hangs to the
harness timeout instead of failing fast; same root cause, different
surface symptom. The 14th file, `toBlob-origin-clean-offscreen.sub.html`,
also TIMEOUTs but its source is a cross-origin `<img>` (not a canvas) —
the log shows no JS error at all before the timeout, so it is **not**
attributed to this bug; likely blocked on the cross-origin `img.onload`
(subdomain `{{domains[www1]}}`) never firing, not traced further here.

Every failing call is `createImageBitmap(srcCanvas)` where `srcCanvas` is
an on-page `<canvas>` element (`document.createElement('canvas')`,
`.getContext('2d')` already used to draw into it) — e.g.
`context-creation-with-alpha.html:35`:

```js
var srcCanvas = document.createElement('canvas');
// ... ctx.fillRect(...) ...
createImageBitmap(srcCanvas).then(...)
```

Confirmed live (`--mcp-live-port`):

```js
var c = document.createElement('canvas'); c.width = c.height = 10;
createImageBitmap(c) // rejects with TypeError: createImageBitmap: unsupported source type
```

## Причина

`crates/js/src/offscreen_canvas.rs`'s `createImageBitmap` shim tests the
source against four shapes in order — `ImageData` (`.data`+`.width`+
`.height`), `OffscreenCanvas` (`typeof source.__canvas_id__ === 'number'`),
`HTMLImageElement` (`.__nid__` + tag `IMG`), `Blob` (`._bytes instanceof
Uint8Array`) — and falls through to `reject(new TypeError('createImageBitmap:
unsupported source type'))` for anything else. An on-page `<canvas>`
(`HTMLCanvasElement`) matches none of these: it carries neither
`__canvas_id__` (that property is JS-shim-internal to the `OffscreenCanvas`
class in the same file, never copied onto DOM canvas elements — 2D-context
pixels for a DOM canvas are tracked purely by native `nid`, with no JS-side
handle) nor `__nid__`+tag-`IMG`. Per the HTML LS, `createImageBitmap()`'s
first argument is `CanvasImageSource` — `HTMLImageElement | SVGImageElement
| HTMLVideoElement | HTMLCanvasElement | ImageBitmap | OffscreenCanvas |
VideoFrame` — plus the `ImageBitmapSource` extension (`Blob | ImageData`).
`HTMLCanvasElement` (and by the same gap `HTMLVideoElement`/`ImageBitmap`
itself as a re-croppable source) is simply missing a branch.

## Масштаб

Every WPT test in this category that exercises the `2d`-canvas →
`createImageBitmap` → `bitmaprenderer.transferFromImageBitmap` pipeline
fails at the first step — this is the dominant failure across the run (15
direct FAILs + 1 TIMEOUT of the 19 unexpected results). Of the remaining 3
unexpected results, 2 are [[BUG-617]] (`ImageBitmapRenderingContext` has no
global constructor) and 1 (`toBlob-origin-clean-offscreen.sub.html`) is
unattributed, see Симптом. Fix: add an `HTMLCanvasElement` branch to
`createImageBitmap` that reads the canvas's current pixels the same way
the existing OffscreenCanvas branch does (`_lumen_canvas2d_get_image_data`-
style native, keyed by `nid` instead of `__canvas_id__`) before falling
through to the `unsupported source type` rejection. `HTMLVideoElement` as a
source is out of scope for this bug (no test in this category exercises
it) but shares the same gap and is worth checking when this is fixed.
