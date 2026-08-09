# BUG-712: `navigator.gpu` has no `GPU` interface identity — `globalThis.GPU` doesn't exist, so `instanceof`/WebIDL-shape checks on the top-level WebGPU entry point are impossible

**Статус:** OPEN
**Компонент:** js (`crates/js/src/webgpu.rs` — `WEBGPU_SHIM`)
**Найден:** WPT-VENDOR-webgpu (`ROADMAP.md`)

## Симптом

`WPT-VENDOR-webgpu` has no test content to run: the upstream WPT `webgpu/`
directory is intentionally empty (`README.md`: "browsers should pin
('vendor') a specific version of the gpuweb/cts repository... checked into
the browser repository as non-exported wpt tests" — the CTS itself is not
in web-platform-tests). `run_report.py --all --root webgpu --recursive`
correctly reports `no tests selected` (0 files, 0 ids) — confirmed 2026-08-09,
matches the `eyedropper`/`webauthn` precedent that a category can still be
worth a live probe even with zero WPT signal
([[reference_wpt_run_report_invocation_recipe]]).

Live probe (`--mcp-live-port`, `crates/js/src/webgpu.rs::WEBGPU_SHIM`) on
the default V8 build (no `webgpu` Cargo feature — `navigator.gpu` stays the
pure Phase 0 JS shim per the module doc comment, `webgpu.rs:900`):

```json
{
  "gpu_typeof": "object",
  "GPU_global": "undefined",
  "gpu_instanceof_GPU": "ERR:GPU is not defined",
  "gpu_ctor_name": "Object",
  "gpu_proto_is_object_proto": true,
  "toStringTag": "[object Object]",
  "adapter_ctor_name": "function",
  "canvas_getContext_webgpu": {
    "typeof": "object",
    "ctorName": "GPUCanvasContext",
    "instanceofGPUCanvasContext": true
  },
  "bufferUsage_frozen": false
}
```

* `globalThis.GPU` is `undefined` — no `GPU` interface constructor exists
  anywhere in the shim, so `navigator.gpu instanceof GPU` throws
  `ReferenceError: GPU is not defined` instead of evaluating (any
  WebIDL-shape/idlharness-style check on the entry point fails before it
  can even assert `false`).
* `navigator.gpu.constructor.name` is `"Object"` and
  `Object.getPrototypeOf(navigator.gpu) === Object.prototype` — the object
  Firefox/Chrome expose as `navigator.gpu` is an instance of `GPU`, per
  spec (`[Exposed=(Window, DedicatedWorker), SecureContext] interface GPU`);
  here it's a bare object literal, same class of defect as
  [BUG-711](BUG-711-OPEN.md) (WebGL context has no
  `WebGLRenderingContext` identity either) but on the entry point of a
  *different* subsystem, so not a duplicate.
* By contrast, every other WebGPU interface *is* constructor/prototype-based
  and correct: `GPUAdapter`, `GPUDevice`, `GPUBuffer`, `GPUTexture`,
  `GPUCommandEncoder`, `GPUCanvasContext` (confirmed live:
  `canvas.getContext('webgpu') instanceof GPUCanvasContext === true`) all
  use real `function Ctor() {...}` + `Ctor.prototype.method = ...` —
  `navigator.gpu` itself (`_gpu = {...}` at `webgpu.rs:821`) is the one
  object built as a plain literal, not a smaller variant of the WebGL bug.
* Secondary, lower-severity finding: `GPUBufferUsage`/`GPUTextureUsage`/
  `GPUShaderStage`/`GPUMapMode`/`GPUColorWrite` (`webgpu.rs:849-883`) are
  plain mutable objects (`Object.isFrozen(GPUBufferUsage) === false`) —
  real engines expose these as read-only constant namespaces; a page can
  currently do `GPUBufferUsage.VERTEX = 0` and corrupt every subsequent
  `createBuffer({usage: GPUBufferUsage.VERTEX})` call process-wide. Worth
  fixing alongside the constructor issue since it's the same file/area, not
  worth its own bug number.

## Root cause

`crates/js/src/webgpu.rs:821` builds the `navigator.gpu` value as
`var _gpu = { requestAdapter: ..., getPreferredCanvasFormat: ...,
wgslLanguageFeatures: ... };` — a plain object literal — then installs it
via `Object.defineProperty(navigator, 'gpu', { get: function() { return
_gpu; } })` (`webgpu.rs:840`). Unlike every other interface in the same
file (`GPUAdapter` at line 744, `GPUDevice` at line 704,
`GPUCanvasContext` at line 775, etc.), there is no `function GPU() {...}`
constructor and no `globalThis.GPU = GPU` — confirmed
(`grep -n "instanceof GPU\b\|globalThis\.GPU\b\|class GPU\b"
crates/js/src/webgpu.rs` → zero hits, only `GPUAdapter`/`GPUDevice`/etc.
patterns match). This is a straightforward omission, not a design
decision — the file's own doc comment (`webgpu.rs:3`) describes
`navigator.gpu` as "a `GPU` object" as if the type already existed.

## Дальше

1. Add `function GPU() {}` + `globalThis.GPU = GPU;` next to the other
   interface constructors, set `_gpu.__proto__ = GPU.prototype` (or
   construct `_gpu` via `new GPU()` and attach the existing methods to
   `GPU.prototype`) so `navigator.gpu instanceof GPU` is `true` and
   `navigator.gpu.constructor.name === 'GPU'`, matching the pattern already
   used correctly by `GPUAdapter`/`GPUDevice`/`GPUCanvasContext` in the
   same file.
2. Optionally freeze the `GPU*Usage`/`GPUShaderStage`/`GPUMapMode`/
   `GPUColorWrite` constant objects (`Object.freeze(...)` after
   `globalThis.GPUBufferUsage = {...}` etc.) to match spec-mandated
   read-only namespace semantics — cheap, same commit as (1).
3. No WPT test content exists to gate this fix against (the upstream
   `webgpu/` WPT directory is intentionally empty — see Симптом); validate
   with a small inline Rust `#[test]` alongside the existing
   `bool_eval`-based tests already in this file (e.g. near `webgpu.rs:1314`)
   asserting `navigator.gpu instanceof GPU`.
