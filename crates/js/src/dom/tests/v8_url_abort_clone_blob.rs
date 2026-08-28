//! Sixteenth porting slice: URL static methods (`canParse`/`parse`),
//! `AbortSignal`/`AbortController` (incl. `.any`/`.timeout`), fetch abort
//! rejection, `structuredClone`, `btoa`/`atob`, `Blob`, `File`, `FileReader`.
//! Async assertions tightened per the S12b-2 lesson: QuickJS originals either
//! drained microtasks explicitly (`fetch_rejects_on_aborted_signal`) or
//! tolerated a not-yet-flushed `Null` result (`blob_text_promise`,
//! `blob_array_buffer_promise`, `file_reader_read_as_data_url`) because
//! QuickJS's `eval()` never drained the microtask queue. V8 auto-checkpoints
//! microtasks after each script (the
//! `queue_microtask_callback_runs_after_sync_tail` precedent in
//! `v8_perf_observers`), so these are split into a setup `eval()` and a
//! second `eval()` that deterministically reads the settled state.
//!
//! Trusted Types (originally scoped into this slice) was ported later, in its
//! own follow-up slice (`S12b-24-trusted-types`, `mod v8_trusted_types` above).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

#[test]
fn url_can_parse_static_method() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "URL.canParse('https://example.com') === true && \
                 URL.canParse('not a url') === false && \
                 URL.canParse('https://foo.com/path', 'https://base.com') === true"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn url_parse_static_method() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var u = URL.parse('https://example.com/test');
                 var bad = URL.parse('not valid');
                 (u instanceof URL) && u.hostname === 'example.com' && bad === null"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn abort_signal_timeout_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "typeof AbortSignal.timeout === 'function' && \
                 typeof AbortSignal.any === 'function'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn abort_signal_timeout_returns_signal() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var sig = AbortSignal.timeout(5000);
                 sig instanceof AbortSignal && !sig.aborted"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn abort_signal_any_already_aborted() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var ctrl = new AbortController(); ctrl.abort();
                 var combined = AbortSignal.any([ctrl.signal]);
                 combined.aborted === true"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn abort_signal_any_propagates_source_reason() {
    let rt = v8_runtime_with_dom(make_doc());
    // Race decided after construction: combined signal must adopt the
    // aborting source's reason, not a generic AbortError.
    let r = rt.eval(
        "var c1 = new AbortController(); var c2 = new AbortController();
                 var combined = AbortSignal.any([c1.signal, c2.signal]);
                 c2.abort('custom-reason');
                 combined.aborted === true && combined.reason === 'custom-reason'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    // Already-aborted source at construction time: reason copied too.
    let r2 = rt.eval(
        "var pre = AbortSignal.abort('pre-reason');
                 var combined2 = AbortSignal.any([new AbortController().signal, pre]);
                 combined2.aborted === true && combined2.reason === 'pre-reason'"
    ).unwrap();
    assert_eq!(r2, lumen_core::JsValue::Bool(true));
}

#[test]
fn abort_signal_static_abort_and_onabort() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var s = AbortSignal.abort();
                 s.aborted === true && s.reason instanceof DOMException && s.reason.name === 'AbortError'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    // onabort handler fires alongside addEventListener listeners.
    let r2 = rt.eval(
        "var hits = [];
                 var ctrl = new AbortController();
                 ctrl.signal.onabort = function(e) { hits.push('on:' + e.type); };
                 ctrl.signal.addEventListener('abort', function(e) { hits.push('ls:' + e.type); });
                 ctrl.abort();
                 hits.join(',') === 'on:abort,ls:abort'"
    ).unwrap();
    assert_eq!(r2, lumen_core::JsValue::Bool(true));
}

#[test]
fn fetch_rejects_on_aborted_signal() {
    let rt = v8_runtime_with_dom(make_doc());
    // Aborted signal short-circuits fetch before any network call; the
    // rejection reason is the signal's reason. V8 settles the `.catch`
    // microtask after this script finishes, so read `got` in a second
    // `eval()` rather than draining explicitly (no primitive for that
    // under V8, see the module doc comment).
    rt.eval(
        "var got = '';
                 var ctrl = new AbortController();
                 ctrl.abort(new DOMException('user cancelled', 'AbortError'));
                 fetch('http://example.test/', { signal: ctrl.signal })
                     .catch(function(e) { got = e.name + ':' + e.message; });"
    ).unwrap();
    let r = rt.eval("got").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("AbortError:user cancelled".to_string()));
}

// ─── structuredClone tests ────────────────────────────────────────────

#[test]
fn structured_clone_primitive() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("structuredClone(42) === 42").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    let r2 = rt.eval("structuredClone('hello') === 'hello'").unwrap();
    assert_eq!(r2, lumen_core::JsValue::Bool(true));
    let r3 = rt.eval("structuredClone(null) === null").unwrap();
    assert_eq!(r3, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_deep_object() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var orig = { a: 1, b: { c: [1,2,3] } };
                     var clone = structuredClone(orig);
                     clone.b.c[0] = 99;
                     orig.b.c[0] === 1 && clone.b.c[0] === 99",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_array() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var arr = [1, [2, 3]];
                     var c = structuredClone(arr);
                     c[1][0] = 99;
                     arr[1][0] === 2",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_date() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var d = new Date(1000000);
                     var c = structuredClone(d);
                     c instanceof Date && c.getTime() === 1000000",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn window_structured_clone_alias() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("window.structuredClone === structuredClone").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_map() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var orig = new Map([['a', {x:1}], ['b', [2,3]]]);
                     var clone = structuredClone(orig);
                     clone.get('a').x = 99;
                     orig.get('a').x === 1 && clone instanceof Map && clone.size === 2",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_set() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var orig = new Set([1, 'hello', true]);
                     var clone = structuredClone(orig);
                     clone instanceof Set && clone.size === 3 &&
                     clone.has(1) && clone.has('hello') && clone.has(true)",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_map_nested_objects() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var inner = {v: 42};
                     var orig = new Map([['k', inner]]);
                     var clone = structuredClone(orig);
                     clone.get('k').v = 99;
                     inner.v === 42",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_set_nested_objects() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var orig = new Set([new Date(5000), new RegExp('x', 'i')]);
                     var clone = structuredClone(orig);
                     var items = [];
                     clone.forEach(function(v) { items.push(v); });
                     clone instanceof Set && clone.size === 2 &&
                     items[0] instanceof Date && items[0].getTime() === 5000 &&
                     items[1] instanceof RegExp && items[1].source === 'x'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_regexp() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var orig = /hello/gi;
                     var clone = structuredClone(orig);
                     clone instanceof RegExp && clone.source === 'hello' && clone.flags === 'gi'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_circular_reference() {
    // A self-referential object must not overflow the stack and must
    // preserve the cycle: clone.self === clone.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var o = { name: 'a' };
                     o.self = o;
                     var c = structuredClone(o);
                     c.name === 'a' && c.self === c && c !== o",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_shared_reference_identity() {
    // The same object referenced twice clones to a single shared clone.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var shared = { v: 1 };
                     var orig = { a: shared, b: shared };
                     var c = structuredClone(orig);
                     c.a === c.b && c.a !== shared && c.a.v === 1",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_array_buffer() {
    // ArrayBuffer is deep-copied (independent backing store).
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var buf = new ArrayBuffer(4);
                     new Uint8Array(buf).set([1, 2, 3, 4]);
                     var c = structuredClone(buf);
                     var cv = new Uint8Array(c);
                     c instanceof ArrayBuffer && c !== buf && c.byteLength === 4 &&
                     cv[0] === 1 && cv[3] === 4 &&
                     (cv[0] = 99, new Uint8Array(buf)[0] === 1)",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_typed_array() {
    // Typed array clones its element type, length and values; original
    // stays independent.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var ta = new Uint16Array([10, 20, 30]);
                     var c = structuredClone(ta);
                     c[1] = 999;
                     c instanceof Uint16Array && c.length === 3 &&
                     c[0] === 10 && ta[1] === 20 && c.buffer !== ta.buffer",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_typed_array_shares_buffer_identity() {
    // Two views over one buffer must, after cloning, still share one buffer.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var buf = new ArrayBuffer(8);
                     var a = new Uint8Array(buf);
                     var b = new Uint8Array(buf);
                     var c = structuredClone({ a: a, b: b });
                     c.a.buffer === c.b.buffer && c.a.buffer !== buf",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_function_throws_data_clone_error() {
    // Functions are not serializable → DataCloneError DOMException.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var threw = false, name = '';
                     try { structuredClone(function(){}); }
                     catch (e) { threw = true; name = e.name; }
                     threw && name === 'DataCloneError'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_symbol_throws_data_clone_error() {
    // Symbols are not serializable → DataCloneError DOMException.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var threw = false, name = '';
                     try { structuredClone(Symbol('x')); }
                     catch (e) { threw = true; name = e.name; }
                     threw && name === 'DataCloneError'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn structured_clone_bigint_primitive() {
    // BigInt round-trips as a value.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("structuredClone(9007199254740993n) === 9007199254740993n")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ─── btoa / atob tests ──────────────────────────────────────────────

#[test]
fn btoa_basic_encoding() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("btoa('Man')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("TWFu".into()));
}

#[test]
fn btoa_with_padding() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("btoa('Ma')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("TWE=".into()));
}

#[test]
fn atob_basic_decoding() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("atob('TWFu')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("Man".into()));
}

#[test]
fn btoa_atob_roundtrip() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("atob(btoa('Hello, World!')) === 'Hello, World!'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn btoa_atob_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.btoa === 'function' && typeof window.atob === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ─── Blob tests ─────────────────────────────────────────────────────

#[test]
fn blob_from_string_parts() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var b = new Blob(['hello ', 'world'], {type: 'text/plain'}); \
                 b.size === 11 && b.type === 'text/plain'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn blob_empty() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var b = new Blob(); b.size === 0 && b.type === ''").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn blob_slice() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var b = new Blob(['hello world']); \
                 var s = b.slice(6, 11); \
                 s.size === 5"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn blob_text_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _blob_text_result = null; new Blob(['hello']).text().then(function(t) { _blob_text_result = t; });").unwrap();
    let r = rt.eval("_blob_text_result").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("hello".to_string()));
}

#[test]
fn blob_array_buffer_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _blob_ab_len = null; new Blob(['abc']).arrayBuffer().then(function(ab) { _blob_ab_len = ab.byteLength; });").unwrap();
    let r = rt.eval("_blob_ab_len").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(3.0));
}

#[test]
fn blob_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.Blob === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ─── File tests ─────────────────────────────────────────────────────

#[test]
fn file_name_and_size() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var f = new File(['data'], 'test.txt', {type: 'text/plain'}); \
                 f.name === 'test.txt' && f.size === 4 && f.type === 'text/plain'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn file_last_modified() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var f = new File(['x'], 'a.txt', {lastModified: 12345}); \
                 f.lastModified === 12345"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn file_instanceof_blob() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var f = new File(['x'], 'a.txt'); \
                 f instanceof Blob && f instanceof File"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ─── FileReader tests ───────────────────────────────────────────────

#[test]
fn file_reader_read_as_text() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fr = new FileReader(); \
                 var done = false; \
                 fr.onload = function() { done = true; }; \
                 fr.readAsText(new Blob(['hello'])); \
                 fr.readyState === 1"  // LOADING immediately
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn file_reader_constants() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "FileReader.EMPTY === 0 && FileReader.LOADING === 1 && FileReader.DONE === 2"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn file_reader_read_as_data_url() {
    let rt = v8_runtime_with_dom(make_doc());
    // Encode 'hi' as base64 = 'aGk='
    rt.eval(
        "var fr = new FileReader(); \
                 var result = null; \
                 fr.onload = function(e) { result = e.target.result; }; \
                 fr.readAsDataURL(new Blob(['hi'], {type: 'text/plain'}));"
    ).unwrap();
    let r = rt.eval("result").unwrap();
    match r {
        lumen_core::JsValue::String(s) => {
            assert!(s.starts_with("data:text/plain;base64,"), "got: {s}");
            assert!(s.contains("aGk="), "expected base64 of 'hi', got: {s}");
        }
        other => panic!("expected resolved data URL string, got {other:?}"),
    }
}
