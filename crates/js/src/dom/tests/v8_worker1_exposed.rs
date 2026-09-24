//! WORKER-1: `[Exposed=Worker]` members reach a worker scope from the same
//! shim slices the page evaluates, together with the natives those slices call.

use super::*;

/// A bare worker scope, installed the way every worker flavour installs its
/// shared part (`worker::install_worker_scope_globals_v8` — dedicated, shared
/// and service workers all go through it).
fn worker_scope() -> crate::v8_runtime::V8JsRuntime {
    let rt = crate::v8_runtime::V8JsRuntime::new().unwrap();
    crate::worker::install_worker_scope_globals_v8(&rt).unwrap();
    rt
}

/// BUG-1080 (encoding half): `TextEncoder`/`TextDecoder` exist in a worker
/// and round-trip non-ASCII text — which proves the natives behind
/// `decode()` are registered, not just the constructors defined.
#[test]
fn worker_scope_has_text_encoder_and_decoder() {
    let rt = worker_scope();
    let out = rt
        .eval(
            "var b = new TextEncoder().encode('Привет, 😀');\
             new TextDecoder().decode(b) + '|' + b.length",
        )
        .unwrap();
    assert_eq!(out, lumen_core::JsValue::String("Привет, 😀|18".into()));
}

/// The decoder's label table and fatal mode are the page's, not a stub:
/// `windows-1251` decodes, an unknown label throws `RangeError`, and malformed
/// UTF-8 under `fatal` throws `TypeError`.
#[test]
fn worker_text_decoder_labels_and_fatal_mode() {
    let rt = worker_scope();
    let out = rt
        .eval(
            "var r = [];\
             r.push(new TextDecoder('windows-1251').decode(new Uint8Array([0xCF, 0xF0, 0xE8])));\
             r.push(new TextDecoder('cp1251').encoding);\
             try { new TextDecoder('no-such-label'); r.push('none'); } catch (e) { r.push(e.name); }\
             try { new TextDecoder('utf-8', {fatal: true}).decode(new Uint8Array([0xFF])); r.push('none'); }\
             catch (e) { r.push(e.name); }\
             r.join(',')",
        )
        .unwrap();
    assert_eq!(
        out,
        lumen_core::JsValue::String("При,windows-1251,RangeError,TypeError".into())
    );
}

/// The worker gets the page's own classes, not a look-alike: the same slice
/// text is in both programs.
#[test]
fn text_encoding_slice_is_shared_by_page_and_worker() {
    assert!(web_api_shim().contains(TEXT_ENCODING_SHIM));
    assert!(worker_exposed_shim().contains(TEXT_ENCODING_SHIM));
}

/// BUG-1066: `DOMException` is `[Exposed=*]`; the shared part every worker
/// flavour installs provides it (it used to reach only the dedicated worker,
/// through that flavour's own `atob` wiring).
#[test]
fn worker_scope_has_dom_exception() {
    let rt = worker_scope();
    let out = rt
        .eval(
            "var e = new DOMException('m', 'AbortError');\
             [e instanceof Error, e.name, e.code, DOMException.DATA_CLONE_ERR].join(',')",
        )
        .unwrap();
    assert_eq!(out, lumen_core::JsValue::String("true,AbortError,20,25".into()));
}

/// BUG-649: `navigator` exists in the shared worker scope (WorkerNavigator,
/// BUG-776) — the reported `ReferenceError` is gone.
#[test]
fn worker_scope_has_navigator() {
    let rt = worker_scope();
    assert_eq!(
        rt.eval("typeof navigator + ',' + (navigator instanceof WorkerNavigator)").unwrap(),
        lumen_core::JsValue::String("object,true".into())
    );
}

/// BUG-1080 (streams half): the Streams classes and the two families built
/// on them exist in a worker and actually move data — `TextDecoderStream`
/// decodes through a `pipeThrough`, a `ReadableStream` read settles.
#[test]
fn worker_scope_has_streams() {
    let rt = worker_scope();
    rt.eval(
        "globalThis.__out = [typeof ReadableStream, typeof WritableStream, typeof TransformStream,             typeof TextDecoderStream, typeof TextEncoderStream, typeof CompressionStream,             typeof DecompressionStream, typeof ByteLengthQueuingStrategy,             typeof CountQueuingStrategy].join(',');         var rs = new ReadableStream({ start: function(c) {             c.enqueue(new Uint8Array([0xD0, 0x9F])); c.enqueue(new Uint8Array([0xD1, 0x80])); c.close(); } });         var reader = rs.pipeThrough(new TextDecoderStream()).getReader();         var text = '';         function pump() { return reader.read().then(function(r) {             if (r.done) { globalThis.__out += '|' + text; return; }             text += r.value; return pump(); }); }         pump();",
    )
    .unwrap();
    assert_eq!(
        rt.eval("globalThis.__out").unwrap(),
        lumen_core::JsValue::String(
            "function,function,function,function,function,function,function,function,function|Пр".into()
        )
    );
}

/// `CompressionStream` needs the `_lumen_cs_*` codec natives; a gzip round
/// trip inside the worker proves they are registered there, not only in the
/// page runtime.
#[test]
fn worker_compression_stream_round_trips() {
    let rt = worker_scope();
    rt.eval(
        "globalThis.__out = 'pending';         var src = new TextEncoder().encode('lumen lumen lumen');         var rs = new ReadableStream({ start: function(c) { c.enqueue(src); c.close(); } });         var reader = rs.pipeThrough(new CompressionStream('gzip'))             .pipeThrough(new DecompressionStream('gzip'))             .pipeThrough(new TextDecoderStream()).getReader();         var text = '';         function pump() { return reader.read().then(function(r) {             if (r.done) { globalThis.__out = text; return; }             text += r.value; return pump(); }); }         pump().catch(function(e) { globalThis.__out = 'error: ' + e; });",
    )
    .unwrap();
    assert_eq!(
        rt.eval("globalThis.__out").unwrap(),
        lumen_core::JsValue::String("lumen lumen lumen".into())
    );
}

/// The worker runs the page's own Streams, not a look-alike.
#[test]
fn streams_slice_is_shared_by_page_and_worker() {
    assert!(web_api_shim().contains(STREAMS_SHIM));
    assert!(worker_exposed_shim().contains(STREAMS_SHIM));
}
