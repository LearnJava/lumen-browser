# BUG-342: V8 native-function trampoline silently drops Array/Object arguments — `Vec<u8>`-consuming natives always see empty input

**Статус:** OPEN
**Компонент:** js (`crates/js/src/v8_compat.rs::v8_to_jsvalue`, used by `native_fn_trampoline`)
**Найден:** P2, WPT-VENDOR-compression 2026-07-25 (`run_report.py --all --root compression --recursive`, vendored `compression/compression-multiple-chunks.any.html` and siblings)

## Симптом

Every WPT compress/decompress round-trip subtest that isn't zero-byte input fails:
`compression-multiple-chunks.any.html` (0/60), `compression-including-empty-chunk.any.html`
(0/12), `decompression-buffersource.any.html` (0/48), `decompression-split-chunk.any.html`
(0/60), `decompression-correct-input.any.html`/`decompression-uint8array-output.any.html`/
`decompression-extra-input.any.html` (TIMEOUT). The compressed/decompressed output is
always exactly what an **empty** input would produce (e.g. `CompressionStream('gzip')` fed
`"Hello"` returns the 20-byte gzip-of-nothing stream `[31,139,8,0,0,0,0,0,0,255,3,0,0,0,0,0,0,0,0,0]`
instead of a stream that decodes back to `"Hello"`).

## Root cause

Isolated via a live `--bidi-port` `script.evaluate` diagnostic (bypassing the Streams API
entirely): calling `_lumen_sha_digest('SHA-256', [72,101,108,108,111])` (native taking
`(algo: String, data: Vec<u8>)`) returns the SHA-256 digest **of the empty string**
(`e3b0c442...`), not of `"Hello"` — reproduced identically whether the array argument is a
plain array literal, `Array.from(uint8Array)`, or a real `Uint8Array`. Same for
`_lumen_compress_bytes(Array.from(u8), 'gzip')` (native taking `(data: Vec<u8>, format:
String)` — `Vec<u8>` in the *other* argument position). The sibling `String` argument is
always received correctly (confirmed by algorithm/format selection working); only the
array-shaped argument is lost, regardless of its position.

Root cause: `native_fn_trampoline` (`v8_compat.rs:774`) converts each V8 call argument to
`JsValue` via `v8_to_jsvalue` (`v8_compat.rs:711`):

```rust
pub(crate) fn v8_to_jsvalue(scope: &v8::PinScope<'_, '_>, val: v8::Local<'_, v8::Value>) -> JsValue {
    if val.is_null() || val.is_undefined() { return JsValue::Null; }
    if val.is_boolean() { return JsValue::Bool(val.boolean_value(scope)); }
    if val.is_number() { return JsValue::Number(val.number_value(scope).unwrap_or(f64::NAN)); }
    if val.is_string() { return val.to_string(scope).map(|s| JsValue::String(s.to_rust_string_lossy(scope))).unwrap_or(JsValue::Null); }
    // Arrays, objects, functions → Null (sufficient for compat-layer natives)
    JsValue::Null
}
```

The trailing comment's assumption is false: any JS `Array` (or plain object) argument
falls through to `JsValue::Null`. `array_from_js_value` (`v8_compat.rs:116`, backing
`impl FromJsValue for Vec<u8>`/`Vec<u32>`/`Vec<String>`/`Vec<f64>`) maps `JsValue::Null` →
`Ok(Vec::new())`, so the native silently receives an empty vec instead of erroring —
no exception, no log, nothing to signal the argument was dropped. A second, *separate*
conversion function in the same crate, `from_v8` (`v8_runtime.rs:4400`), does handle
`val.is_array()` correctly (recursively converts each element) — it's used elsewhere in
the V8 runtime but not by the native-function-argument path, which is why this went
unnoticed: anything exercised through `from_v8`'s callers works, anything reaching a
native via `native_fn_trampoline`/`v8_to_jsvalue` with an array/object argument does not.

## Impact

Any `_lumen_*` native registered via `reg!`/`into_v8_fnN` that takes a `Vec<u8>`
(or `Vec<u32>`/`Vec<String>`/`Vec<f64>`, or presumably a plain object) argument silently
operates on empty/default data under the V8 backend — the **default engine since ADR-018**.
Confirmed affected: `_lumen_compress_bytes`, `_lumen_decompress_bytes` (Compression
Streams — this WPT category), `_lumen_sha_digest` (`crypto.subtle.digest`, WebCryptoAPI —
likely explains some of that category's unexplored TIMEOUT/ERROR results too, not yet
cross-checked). Any other native with an array-shaped parameter should be treated as
suspect until re-audited. Existing `cargo test -p lumen-js` coverage did **not** catch
this because `dom.rs`'s unit tests run through `QuickJsRuntime` (rquickjs), never the V8
path (`v8-backend` is an optional, non-default Cargo feature) — see
`decompression_stream_multi_chunk_matches_single_chunk` (`dom.rs:26856`), which exercises
this exact multi-write scenario and passes under QuickJS while the equivalent live-browser
V8 case (verified via a `script.evaluate` repro) fails.

## Suspected fix direction

Make `v8_to_jsvalue` handle `is_array()`/`is_object()` the same way `from_v8` already
does (recursively convert array elements / object entries instead of falling through to
`Null`) — ideally by deduplicating the two conversion functions rather than patching both
independently, since they diverging silently is exactly how this bug was introduced.
After fixing, re-run `_lumen_sha_digest`/`_lumen_compress_bytes`/`_lumen_decompress_bytes`
through the live V8 diagnostic to confirm, then re-run
`run_report.py --all --root compression --recursive` and `--root WebCryptoAPI` to check
for newly-passing subtests.
