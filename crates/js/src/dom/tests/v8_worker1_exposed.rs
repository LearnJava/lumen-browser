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

/// WORKER-1 срез 4: `AbortController`/`AbortSignal` exist in a worker and
/// behave like the page's — `abort()` fires the listener with an
/// `AbortError` reason, `AbortSignal.any` follows its source, and a throwing
/// listener neither escapes `abort()` nor stops the next one (the page-only
/// `_lumen_report_exception` is absent here; the slice must not reach for it).
#[test]
fn worker_scope_has_abort_controller() {
    let rt = worker_scope();
    let out = rt
        .eval(
            r#"var r = [typeof AbortController, typeof AbortSignal];
               var c = new AbortController();
               var any = AbortSignal.any([c.signal]);
               c.signal.addEventListener('abort', function() { throw new Error('boom'); });
               c.signal.addEventListener('abort', function(e) { r.push(e.type); });
               c.abort();
               r.push(c.signal.aborted, c.signal.reason.name, any.aborted);
               try { c.signal.throwIfAborted(); r.push('none'); } catch (e) { r.push(e.name); }
               r.push(AbortSignal.abort('why').reason);
               r.join(',')"#,
        )
        .unwrap();
    assert_eq!(
        out,
        lumen_core::JsValue::String("function,function,abort,true,AbortError,true,AbortError,why".into())
    );
}

/// WORKER-1 срез 4: `Blob`/`File` exist in a worker, carry their bytes and
/// type, slice, and read back through `text()`/`arrayBuffer()`/`stream()` —
/// the last one proving the slice meets the worker's Streams.
#[test]
fn worker_scope_has_blob_and_file() {
    let rt = worker_scope();
    rt.eval(
        r#"globalThis.__out = 'pending';
           var b = new Blob(['Привет', new Uint8Array([0x21])], { type: 'Text/Plain' });
           var f = new File([b], 'a.txt', { lastModified: 7 });
           var head = [typeof FileReader, b.size, b.type, f.name, f.lastModified, f.size,
                       f instanceof Blob, b.slice(0, 2).size].join(',');
           var reader = b.stream().getReader();
           Promise.all([b.slice(12).text(), f.arrayBuffer(), reader.read()]).then(function(v) {
               globalThis.__out = head + '|' + v[0] + '|' + v[1].byteLength + '|' + v[2].value.length;
           }, function(e) { globalThis.__out = 'error: ' + e; });"#,
    )
    .unwrap();
    assert_eq!(
        rt.eval("globalThis.__out").unwrap(),
        lumen_core::JsValue::String("function,13,text/plain,a.txt,7,13,true,2|!|13|13".into())
    );
}

/// WORKER-1 срез 4: `FormData` exists in a worker; built without a form it
/// keeps its entries in order and serializes to multipart through the
/// worker's `TextEncoder`.
#[test]
fn worker_scope_has_form_data() {
    let rt = worker_scope();
    let out = rt
        .eval(
            r#"var fd = new FormData();
               fd.append('a', '1'); fd.append('b', '2'); fd.append('a', '3'); fd.set('b', '4');
               var body = new TextDecoder().decode(fd._toMultipart('X'));
               [fd.getAll('a').join('+'), fd.get('b'), Array.from(fd.keys()).join(''),
                body.indexOf('name="b"') > 0, body.slice(-5)].join(',')"#,
        )
        .unwrap();
    assert_eq!(out, lumen_core::JsValue::String("1+3,4,aba,true,X--\r\n".into()));
}

/// The worker runs the page's own Abort/File API/FormData classes: each slice
/// is spliced into both programs.
#[test]
fn abort_file_api_and_form_data_slices_are_shared_by_page_and_worker() {
    for slice in [ABORT_SHIM, FILE_API_SHIM, FORM_DATA_SHIM] {
        assert!(web_api_shim().contains(slice));
        assert!(worker_exposed_shim().contains(slice));
    }
}
