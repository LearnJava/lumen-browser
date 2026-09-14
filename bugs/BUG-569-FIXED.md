# BUG-569: `HTMLImageElement.prototype.decode()` does not exist

**Статус:** FIXED 2026-09-14 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_tail_b.js` —
`HTMLImageElement.prototype.decode`)
**Найден:** P2, WPT-VENDOR-html-semantics-embedded-content, 2026-08-04

## Симптом

`img.decode()` throws `TypeError: img.decode is not a function` on every
`HTMLImageElement` instance. All 6 variants of
`html/semantics/embedded-content/the-img-element/*decode*` fail identically
before reaching the promise-resolution assertion they test:

```
FAIL HTMLImageElement.prototype.decode(), attach to DOM before promise resolves.: src cached - img.decode is not a function
FAIL HTMLImageElement.prototype.decode(), attach to DOM before promise resolves.: src in empty picture cached - img.decode is not a function
FAIL HTMLImageElement.prototype.decode(), attach to DOM before promise resolves.: src in empty picture not cached - img.decode is not a function
FAIL HTMLImageElement.prototype.decode(), attach to DOM before promise resolves.: src in picture with source cached - img.decode is not a function
FAIL HTMLImageElement.prototype.decode(), attach to DOM before promise resolves.: src in picture with source not cached - img.decode is not a function
FAIL HTMLImageElement.prototype.decode(), attach to DOM before promise resolves.: src not cached - img.decode is not a function
```

## Причина

`HTMLImageElement.decode()` (HTML LS §4.8.4.4, "Decoding images") is a
Promise-returning method that resolves once the image data has finished
decoding — distinct from waiting on the `load` event, and the mechanism
sites use to avoid painting a not-yet-decoded frame. It has never been added
to the image element wrapper; the image pipeline decodes synchronously
during load today (no async decode task queued), so there is no natural hook
for the method to await yet.

## Масштаб

57 failing subtests in this category alone, all under
`the-img-element/`, all with the identical `TypeError` shape — a single
missing method blocks every `decode()`-based test regardless of what image
state (cached/picture/source) each variant is actually probing.

## Исправлено

`decode()` settles off the same `_lumen_img_state` table BUG-630 already
populates for `complete`/`naturalWidth`/`naturalHeight` — no new async decode
pipeline was needed. Added `HTMLImageElement.prototype.decode` in
`crates/js/src/shim/web_api_shim_tail_b.js`, right after the `complete`/
`naturalWidth`/`naturalHeight` getter block: a node that is already
`complete` (success or failure) resolves/rejects synchronously (rejection
uses the same zero-dimensions shape `_lumen_fire_image_error` writes, an
`EncodingError` `DOMException`, mirroring how the `complete` getter already
treats that shape as failure); a node still in flight waits for the `load`/
`error` event that `_lumen_fire_image_load`/`_lumen_fire_image_error`
dispatch once the shell's decode pipeline settles it, then re-checks the
state the same way.

New test file `crates/js/tests/cases/bug569_img_decode.rs` (5 tests):
`decode` exists and returns a `Promise`; resolves immediately when the state
is already a success; rejects with `EncodingError` when the state is already
a failure; resolves after a `load` event fired while still pending; rejects
after an `error` event fired while still pending.

`cargo test -p lumen-js --features v8-backend` green (121/121, was 116),
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D
warnings` clean.
