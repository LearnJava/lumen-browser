# BUG-711: WebGL context has no `WebGLRenderingContext`/`WebGL2RenderingContext` identity, `getContext('webgl2')` is byte-for-byte identical to `getContext('webgl')`, and several core WebGL1 methods (`compressedTexImage2D`/`compressedTexSubImage2D`, `uniformMatrix2fv`) plus every compressed-texture extension are entirely absent

**Статус:** OPEN
**Компонент:** js (`crates/js/src/webgl_canvas.rs` — `WEBGL_SHIM`)
**Найден:** WPT-VENDOR-webgl (`ROADMAP.md`)

## Симптом

`WPT-VENDOR-webgl` run (`run_report.py --all --root webgl --recursive`,
0:38, 8 selected ids): **7/8 harness OK, 7/15 subtests passed** — a small
category but every non-TIMEOUT failure traces to the same root cause.

* `bufferSubData.html` — `ReferenceError: WebGLRenderingContext is not
  defined`. The test does
  `assert_true(gl instanceof WebGLRenderingContext)`-style setup via a
  direct reference to the global constructor before touching `bufferData`;
  the global simply doesn't exist.
* `compressedTexImage2D.html` (2/3 subtests FAIL) and
  `compressedTexSubImage2D.html` (2/3 subtests FAIL) —
  `TypeError: gl.compressedTexImage2D is not a function` /
  `gl.compressedTexSubImage2D is not a function`. Neither method is
  defined on the context object at all (not even as a no-op stub, unlike
  most of the rest of the shim).
* `compressed-tex-sub-image-2d-without-base-image.html` — FAIL:
  `assert_true: A supported compressed texture format must be available
  expected true got false`. The test calls
  `gl.getExtension("WEBGL_compressed_texture_s3tc")` and
  `gl.getExtension("WEBGL_compressed_texture_etc1")`, both of which return
  `null` — `getSupportedExtensions()` only ever returns
  `['WEBGL_debug_renderer_info', 'WEBGL_lose_context']`, so no compressed
  texture format is reachable from script under any name.
* `uniformMatrixNfv.html` — FAIL: `gl[f] is not a function` for
  `f = 'uniformMatrix2fv'`. Only `uniformMatrix3fv`/`uniformMatrix4fv`
  exist (and `uniformMatrix3fv` is itself a silent no-op, "mat3 not
  tracked" per its own comment) — `uniformMatrix2fv` was never added, and
  none of the WebGL2 non-square variants
  (`uniformMatrix2x3fv`/`uniformMatrix3x2fv`/`uniformMatrix2x4fv`/
  `uniformMatrix4x2fv`/`uniformMatrix3x4fv`/`uniformMatrix4x3fv`) exist
  either.
* `texImage2D.html` — FAIL: `assert_throws_js: function "function() {
    gl.texImage2D(0, 0, 0, 0, 0, window);
  }" did not throw`. The test calls the WebIDL 6-argument
  `texImage2D(target, level, internalformat, format, type, source)`
  overload (the `TexImageSource` form, for `<img>`/`<canvas>`/`ImageData`/
  `ImageBitmap`) and expects a `TypeError` for a non-`TexImageSource`
  `source` (a `Window`, then an object whose `width`/`height` getters
  throw). `gl.texImage2D` (line 281) only implements the 9-argument raw
  `ArrayBufferView` overload
  (`target, level, internalformat, width, height, border, format, type,
  pixels`); called with 6 args the trailing 3 params
  (`format`/`type`/`pixels`) are `undefined`, `pixels == null` is true, and
  the function returns silently — the 6-arg overload doesn't exist at all,
  so nothing ever gets a chance to throw.
* `idlharness.any.html` — TIMEOUT, but only because the shared
  `/resources/WebIDLParser.js`+`/resources/idlharness.js` helpers aren't
  vendored (same documented survey gap as `FileAPI`/`animation-worklet`/
  every other category using `idlharness.any.js`); not itself new
  signal, but consistent with the finding below — an idlharness run would
  fail hard on the missing constructors regardless.

## Root cause

`crates/js/src/webgl_canvas.rs::WEBGL_SHIM` (`_makeContext`, line 82)
builds the WebGL context as a **plain object literal** assigned method by
method — there is no `WebGLRenderingContext` (or `WebGL2RenderingContext`)
constructor function anywhere in the JS shim or the V8 install path
(confirmed: `grep -rn "WebGLRenderingContext" crates/js/src/` — zero
hits), so `gl` has no prototype chain a page can name, `instanceof` always
throws `ReferenceError` before it can even evaluate to `false`, and
`window.WebGLRenderingContext` is `undefined` rather than a function.

`_addCanvasStubs` (line 320) branches on
`t === 'webgl' || t === 'webgl2' || t === 'experimental-webgl'` and calls
the *same* `_makeContext(cid)` for all three — `getContext('webgl2')`
returns an object with zero WebGL2-only surface (no `uniformMatrix3x2fv`
family, no `UNIFORM_BUFFER`/`createVertexArray`/`drawBuffers`/texture-3D
methods, etc.), i.e. WebGL2 is presently WebGL1 wearing a different
context-type string. `CAPABILITIES.md`'s "software WebGL 1.0" framing
(`crates/js/src/webgl_canvas.rs:3` doc comment, `CAPABILITIES.md:110/148`)
is accurate for WebGL1's happy path but doesn't flag that WebGL2 is an
unlabeled alias.

Within WebGL1 itself, the method table is inconsistent: most unimplemented
GL functionality is a documented no-op (`gl.bufferSubData = function()
{}`, `gl.drawElements = function() {}`, `gl.texParameteri = function()
{}` — silently accepted, matching this shim's stated "software stub"
design), but `compressedTexImage2D`/`compressedTexSubImage2D` are missing
*outright* — calling them throws a `TypeError` instead of silently
no-opping like their neighbors, a different (and more surprising) failure
mode than the rest of the shim, and `getSupportedExtensions()` doesn't
even advertise a compressed-texture extension a page could feature-detect
around before calling them.

## Дальше

1. Add `WebGLRenderingContext` and `WebGL2RenderingContext` as real
   (even if minimal) constructor functions, set `gl.__proto__ =
   WebGLRenderingContext.prototype` (or `WebGL2RenderingContext.prototype`
   for the `'webgl2'` branch) in `_makeContext`, so `instanceof` and
   `gl.constructor.name` resolve correctly — this is the cheapest fix and
   unblocks every idlharness-style WebIDL-shape check for the category,
   not just the specific `bufferSubData.html` symptom above.
2. Differentiate `getContext('webgl2')` from `getContext('webgl')` — at
   minimum use the different prototype from (1); a fuller fix adds the
   WebGL2-only method surface, but that's new API surface, likely a
   separate follow-up given the software-rasterizer scope.
3. Either implement `compressedTexImage2D`/`compressedTexSubImage2D` as
   real no-ops (matching the rest of the shim's "accepted, does nothing"
   convention) or drop them from `getSupportedExtensions()`'s implied
   contract consistently — right now a page that correctly feature-detects
   via `getExtension` never reaches the missing-function crash, but one
   that doesn't (most WPT conformance tests, and likely most real pages
   using S3TC/ETC1 textures unconditionally) gets an uncaught `TypeError`
   instead of the spec's `INVALID_OPERATION`-via-`getError()` path.
4. Add `uniformMatrix2fv` (same pattern as the existing `uniformMatrix3fv`
   no-op) for parity with `uniformMatrix3fv`/`uniformMatrix4fv`.
5. Add the 6-argument `TexImageSource` overload of `texImage2D` (dispatch
   on `arguments.length` the way real engines do), including the WebIDL
   type coercion that makes an invalid `source` throw `TypeError` rather
   than silently no-op.
