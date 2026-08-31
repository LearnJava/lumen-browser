# BUG-933 — `ImageBitmap` (offscreen canvas side) is still a duck-typed literal, not a class

**Статус:** OPEN
**Компонент:** js (`crates/js/src/offscreen_canvas.rs` — `transferToImageBitmap`/`createImageBitmap`)
**Найден:** 2026-08-31 (P3), при закрытии [BUG-932](BUG-932-FIXED.md)

## Симптом

`OffscreenCanvas.prototype.transferToImageBitmap()` and `createImageBitmap()`
both mint a plain object literal — `{width, height, __canvas_id__, close:
function() {...}}` — instead of a real `ImageBitmap` instance:

```
bitmap.constructor.name === 'Object'
bitmap instanceof ImageBitmap   // ReferenceError — no such global
```

Same class of defect BUG-932 fixed for the 2D context, its gradients,
patterns, text metrics and image data — but not this bug's scope: no
`ImageBitmap` global exists anywhere in the codebase (`grep -rn "function
ImageBitmap\|class ImageBitmap"` across `crates/js/src/**` is empty), so
this is not a page/offscreen inconsistency to reconcile, just a missing
class outright.

## Почему отделён от BUG-932

`__canvas_id__` is read as an own, enumerable field — a cross-module
duck-typing marker meaning "this is a canvas-like object with a native
canvas id" — in **four** files: `canvas2d.rs`, `offscreen_canvas.rs`,
`worker.rs` (structured-clone transfer of an `OffscreenCanvas`/bitmap
across the worker boundary) and `web_api_shim_mid.js` (the element
context's own `drawImage`/`createPattern` source resolution, which also
accepts an offscreen bitmap as a source). Turning `ImageBitmap` into a
class means either:

- keeping `__canvas_id__` readable as a prototype getter (behaviorally
  equivalent for `typeof x.__canvas_id__`/property reads, but untested
  against all four consumers — `worker.rs`'s transfer/reconstruction path
  in particular serializes objects to JSON for the postMessage bridge,
  where a getter-backed value round-trips differently than an own data
  property unless the serializer already normalizes through `JSON.stringify`
  semantics, which read getters transparently but were not verified here), or
- auditing and updating all four call sites in the same commit.

Bigger blast radius than the context/gradient/pattern/textmetrics/imagedata
fix BUG-932 shipped (which stayed self-contained inside
`offscreen_canvas.rs`'s own IIFE, with zero cross-file readers of the
internal shape it changed) — split out rather than risk it under the same
session budget.

## Направление починки

Same recipe as BUG-932: `function ImageBitmap() { throw new TypeError('Illegal
constructor'); }`, `_offscreen_idl_tag`/`Symbol.toStringTag`, a mint helper
(`_offscreen_make_image_bitmap(cid, w, h)` via `Object.create(prototype)` +
non-enumerable slot), `close()` on the prototype reading/mutating the slot
(idempotent — a second `close()` must not double-free the native canvas id).
`width`/`height` and — for backward compatibility with the four `__canvas_id__`
readers above — `__canvas_id__` itself as **enumerable prototype getters**
reading the slot, so existing duck-typing call sites keep working unchanged.
Before landing: grep all four files for `__canvas_id__` reads against a
bitmap-shaped value specifically (not just any object) and verify each one
still gets a plain `number` — a live probe through `worker.rs`'s structured-clone
path (`v8_serialize_with_offscreen_canvas_transfer_embeds_sentinel` and
neighboring tests) is the one most likely to need re-checking, since it goes
through JSON serialization rather than a direct property read.

## Данные

Not separately measured — no dedicated WPT/probe run isolating this from
BUG-932's or BUG-456's slices. Functional behavior (pixels, `close()`
freeing the native canvas) is correct on every measured path; the defect is
purely in the object model, same as BUG-932 was.
