//! Тесты `v8_formdata`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

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

// ── FormData API tests ────────────────────────────────────────────────────

#[test]
fn formdata_class_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof FormData === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn formdata_window_constructor_exposed() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.FormData === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn formdata_append_and_get() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var fd = new FormData(); fd.append('name', 'alice'); fd.get('name')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("alice".into()));
}

#[test]
fn formdata_get_missing_returns_null() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var fd = new FormData(); fd.get('nope') === null").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn formdata_has_returns_true_when_present() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var fd = new FormData(); fd.append('k', 'v'); fd.has('k')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn formdata_has_returns_false_when_absent() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var fd = new FormData(); fd.has('nope')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

#[test]
fn formdata_delete_removes_entries() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('x', '1'); fd.append('x', '2'); \
                 fd.delete('x'); fd.has('x')"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

#[test]
fn formdata_get_all_returns_all_values() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('k', 'a'); fd.append('k', 'b'); \
                 fd.getAll('k').join(',')"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("a,b".into()));
}

#[test]
fn formdata_set_replaces_first_occurrence() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('k', 'old1'); fd.append('k', 'old2'); \
                 fd.set('k', 'new'); fd.getAll('k').length + ':' + fd.get('k')"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("1:new".into()));
}

#[test]
fn formdata_to_url_encoded_basic() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('a', '1'); fd.append('b', '2'); \
                 fd._toUrlEncoded()"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("a=1&b=2".into()));
}

#[test]
fn formdata_to_url_encoded_percent_encodes_spaces() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('full name', 'hello world'); \
                 fd._toUrlEncoded()"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("full%20name=hello%20world".into()));
}

#[test]
fn formdata_to_url_encoded_percent_encodes_ampersand() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('q', 'a&b=c'); \
                 fd._toUrlEncoded()"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("q=a%26b%3Dc".into()));
}

#[test]
fn formdata_keys_iterator() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('x', '1'); fd.append('y', '2'); \
                 var keys = []; var it = fd.keys(); var n; \
                 while (!(n = it.next()).done) { keys.push(n.value); } \
                 keys.join(',')"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("x,y".into()));
}

#[test]
fn formdata_values_iterator() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('x', 'p'); fd.append('y', 'q'); \
                 var vals = []; var it = fd.values(); var n; \
                 while (!(n = it.next()).done) { vals.push(n.value); } \
                 vals.join(',')"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("p,q".into()));
}

#[test]
fn formdata_foreach_iterates_value_name() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('a', '1'); fd.append('b', '2'); \
                 var out = []; \
                 fd.forEach(function(v, k) { out.push(k + '=' + v); }); \
                 out.join('&')"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("a=1&b=2".into()));
}

#[test]
fn formdata_symbol_iterator_same_as_entries() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('k', 'v'); \
                 var it = fd[Symbol.iterator](); var n = it.next(); \
                 n.value[0] + '=' + n.value[1]"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("k=v".into()));
}

#[test]
fn formdata_to_multipart_contains_boundary() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('name', 'alice'); \
                 var bnd = 'test-boundary'; \
                 var bytes = fd._toMultipart(bnd); \
                 var s = ''; for (var i = 0; i < bytes.length; i++) { s += String.fromCharCode(bytes[i]); } \
                 s.indexOf('--test-boundary') >= 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn formdata_to_multipart_contains_field_name_and_value() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('email', 'user@example.com'); \
                 var bytes = fd._toMultipart('bnd'); \
                 var s = ''; for (var i = 0; i < bytes.length; i++) { s += String.fromCharCode(bytes[i]); } \
                 s.indexOf('name=\"email\"') >= 0 && s.indexOf('user@example.com') >= 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn formdata_to_multipart_ends_with_closing_delimiter() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('x', '1'); \
                 var bytes = fd._toMultipart('B'); \
                 var s = ''; for (var i = 0; i < bytes.length; i++) { s += String.fromCharCode(bytes[i]); } \
                 s.indexOf('--B--') >= 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn formdata_to_multipart_empty_entries_yields_only_closing_boundary() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); \
                 var bytes = fd._toMultipart('B'); \
                 var s = ''; for (var i = 0; i < bytes.length; i++) { s += String.fromCharCode(bytes[i]); } \
                 s.trim()"
    ).unwrap();
    // Empty FormData → just --B--\r\n
    assert_eq!(r, lumen_core::JsValue::String("--B--".into()));
}

#[test]
fn formdata_to_multipart_escapes_quotes_in_name() {
    // Use \" in the JS string to pass a double-quote character as field name.
    // In the Rust string, \" is a literal " (Rust escape); V8 then evaluates
    // 'ev\"il' in single-quoted JS string, where \" is interpreted as ".
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('ev\\\"il', 'val'); \
                 var bytes = fd._toMultipart('B'); \
                 var s = ''; for (var i = 0; i < bytes.length; i++) { s += String.fromCharCode(bytes[i]); } \
                 s.indexOf('ev%22il') >= 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn formdata_to_multipart_multiple_fields_ordered() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var fd = new FormData(); fd.append('a', '1'); fd.append('b', '2'); \
                 var bytes = fd._toMultipart('X'); \
                 var s = ''; for (var i = 0; i < bytes.length; i++) { s += String.fromCharCode(bytes[i]); } \
                 s.indexOf('name=\"a\"') < s.indexOf('name=\"b\"')"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// Mock fetch provider that records calls to fetch_with_body_sync.
type FetchCall = (String, String, String, Vec<u8>);
struct CaptureFetch {
    calls: std::sync::Mutex<Vec<FetchCall>>,
}
impl CaptureFetch {
    fn new() -> Arc<Self> {
        Arc::new(Self { calls: std::sync::Mutex::new(vec![]) })
    }
}
impl lumen_core::ext::JsFetchProvider for CaptureFetch {
    fn fetch_sync(&self, url: &str, method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        self.calls.lock().unwrap().push((url.into(), method.into(), String::new(), vec![]));
        Ok(lumen_core::ext::JsFetchResult { status: 200, status_text: "OK".into(), headers: vec![], body: b"ok".to_vec() })
    }
    fn fetch_with_body_sync(&self, url: &str, method: &str, content_type: &str, body: &[u8]) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        self.calls.lock().unwrap().push((url.into(), method.into(), content_type.into(), body.to_vec()));
        Ok(lumen_core::ext::JsFetchResult { status: 200, status_text: "OK".into(), headers: vec![], body: b"ok".to_vec() })
    }
}

fn v8_runtime_with_fetch(provider: Arc<CaptureFetch>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let p: Arc<dyn lumen_core::ext::JsFetchProvider> = provider;
    rt.install_dom(make_doc(), "https://example.com/", Some(p), None, None, None, None, None, None, None, false).unwrap();
    rt
}

// Mock fetch provider whose cancellable variants always report an abort,
// exercising the _lumen_fetch_cancellable* bridge → code 2 path.
struct AbortFetch;
impl lumen_core::ext::JsFetchProvider for AbortFetch {
    fn fetch_sync(&self, _url: &str, _method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        Ok(lumen_core::ext::JsFetchResult { status: 200, status_text: "OK".into(), headers: vec![], body: b"ok".to_vec() })
    }
    fn fetch_with_body_sync(&self, _url: &str, _method: &str, _content_type: &str, _body: &[u8]) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        Ok(lumen_core::ext::JsFetchResult { status: 200, status_text: "OK".into(), headers: vec![], body: b"ok".to_vec() })
    }
    fn fetch_cancellable(&self, _url: &str, _method: &str, _token: &lumen_core::ext::AbortToken) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        Err(lumen_core::error::Error::Aborted("aborted".into()))
    }
    fn fetch_with_body_cancellable(&self, _url: &str, _method: &str, _content_type: &str, _body: &[u8], _token: &lumen_core::ext::AbortToken) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        Err(lumen_core::error::Error::Aborted("aborted".into()))
    }
}
impl AbortFetch {
    fn new() -> Arc<Self> { Arc::new(AbortFetch) }
}

fn v8_runtime_with_abort_fetch() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let p: Arc<dyn lumen_core::ext::JsFetchProvider> = AbortFetch::new();
    rt.install_dom(make_doc(), "https://example.com/", Some(p), None, None, None, None, None, None, None, false).unwrap();
    rt
}

// Mock provider whose cancellable variants BLOCK until the AbortToken is flipped,
// then report an abort — simulates a slow in-flight request cancelled mid-stream.
struct BlockingFetch;
impl lumen_core::ext::JsFetchProvider for BlockingFetch {
    fn fetch_sync(&self, _url: &str, _method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        Ok(lumen_core::ext::JsFetchResult { status: 200, status_text: "OK".into(), headers: vec![], body: b"ok".to_vec() })
    }
    fn fetch_with_body_sync(&self, _url: &str, _method: &str, _content_type: &str, _body: &[u8]) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        Ok(lumen_core::ext::JsFetchResult { status: 200, status_text: "OK".into(), headers: vec![], body: b"ok".to_vec() })
    }
    fn fetch_cancellable(&self, _url: &str, _method: &str, token: &lumen_core::ext::AbortToken) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        while !token.is_aborted() { std::thread::sleep(std::time::Duration::from_millis(5)); }
        Err(lumen_core::error::Error::Aborted("aborted".into()))
    }
    fn fetch_with_body_cancellable(&self, _url: &str, _method: &str, _content_type: &str, _body: &[u8], token: &lumen_core::ext::AbortToken) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        while !token.is_aborted() { std::thread::sleep(std::time::Duration::from_millis(5)); }
        Err(lumen_core::error::Error::Aborted("aborted".into()))
    }
}
impl BlockingFetch {
    fn new() -> Arc<Self> { Arc::new(BlockingFetch) }
}

fn v8_runtime_with_blocking_fetch() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let p: Arc<dyn lumen_core::ext::JsFetchProvider> = BlockingFetch::new();
    rt.install_dom(make_doc(), "https://example.com/", Some(p), None, None, None, None, None, None, None, false).unwrap();
    rt
}

// A generic AbortController.abort() fired *during* an in-flight async fetch must
// reject the promise with an AbortError (not only the timeout path).
#[test]
fn fetch_inflight_abort_rejects_with_abort_error() {
    let rt = v8_runtime_with_blocking_fetch();
    rt.eval("var c = new AbortController(); globalThis.__st='pending'; fetch('https://example.com/slow', {signal: c.signal}).then(function(){__st='resolved';}).catch(function(e){__st=e && e.name ? e.name : 'error';}); c.abort();").unwrap();
    for _ in 0..400 {
        let _ = rt.eval("_lumen_tick_timers();");
        let _ = rt.eval("_lumen_drain_microtasks();");
        std::thread::sleep(std::time::Duration::from_millis(5));
        if rt.eval("__st").unwrap() != lumen_core::JsValue::String("pending".into()) { break; }
    }
    assert_eq!(rt.eval("__st").unwrap(), lumen_core::JsValue::String("AbortError".into()));
}

#[test]
fn abort_signal_timeout_records_deadline() {
    let rt = v8_runtime_with_fetch(CaptureFetch::new());
    assert_eq!(rt.eval("AbortSignal.timeout(50)._timeoutMs === 50").unwrap(), lumen_core::JsValue::Bool(true));
    assert_eq!(rt.eval("AbortSignal.timeout(0)._timeoutMs === 0").unwrap(), lumen_core::JsValue::Bool(true));
}

#[test]
fn fetch_cancellable_bridge_reports_abort() {
    let rt = v8_runtime_with_abort_fetch();
    assert_eq!(rt.eval("_lumen_fetch_cancellable('https://example.com/','GET',0,[]) === 2").unwrap(), lumen_core::JsValue::Bool(true));
    assert_eq!(rt.eval("_lumen_fetch_cancellable_with_body('https://example.com/','POST','text/plain',[104,105],0,[]) === 2").unwrap(), lumen_core::JsValue::Bool(true));
}

#[test]
fn fetch_cancellable_bridge_reports_ok() {
    let rt = v8_runtime_with_fetch(CaptureFetch::new());
    assert_eq!(rt.eval("_lumen_fetch_cancellable('https://example.com/','GET',0,[]) === 0").unwrap(), lumen_core::JsValue::Bool(true));
}

#[test]
fn fetch_post_formdata_sends_multipart_body() {
    // Fetch spec §5.4: FormData body → multipart/form-data (not urlencoded).
    let capture = CaptureFetch::new();
    let rt = v8_runtime_with_fetch(Arc::clone(&capture));
    rt.eval(
        "var fd = new FormData(); fd.append('user', 'bob'); fd.append('age', '30'); \
                 fetch('https://example.com/api', { method: 'POST', body: fd })"
    ).unwrap();
    let calls = capture.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let (url, method, ct, body) = &calls[0];
    assert_eq!(url, "https://example.com/api");
    assert_eq!(method, "POST");
    // Content-Type must start with multipart/form-data and include a boundary.
    assert!(ct.starts_with("multipart/form-data; boundary="),
        "expected multipart/form-data content-type, got: {ct}");
    // Body must contain the field names and values in multipart format.
    let body_str = std::str::from_utf8(body).unwrap();
    assert!(body_str.contains("name=\"user\""), "body should contain field name 'user'");
    assert!(body_str.contains("bob"), "body should contain value 'bob'");
    assert!(body_str.contains("name=\"age\""), "body should contain field name 'age'");
    assert!(body_str.contains("30"), "body should contain value '30'");
    // Body must end with closing boundary --boundary--\r\n
    assert!(body_str.contains("--\r\n"), "body must contain closing boundary");
}

#[test]
fn fetch_post_string_body_sends_text_plain() {
    let capture = CaptureFetch::new();
    let rt = v8_runtime_with_fetch(Arc::clone(&capture));
    rt.eval(
        "fetch('https://example.com/api', { method: 'POST', body: 'hello world' })"
    ).unwrap();
    let calls = capture.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let (_, method, ct, body) = &calls[0];
    assert_eq!(method, "POST");
    assert_eq!(ct, "text/plain;charset=UTF-8");
    assert_eq!(std::str::from_utf8(body).unwrap(), "hello world");
}

#[test]
fn fetch_post_uint8array_body_sends_octet_stream() {
    let capture = CaptureFetch::new();
    let rt = v8_runtime_with_fetch(Arc::clone(&capture));
    rt.eval(
        "fetch('https://example.com/bin', { method: 'PUT', body: new Uint8Array([1, 2, 3]) })"
    ).unwrap();
    let calls = capture.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let (_, method, ct, body) = &calls[0];
    assert_eq!(method, "PUT");
    assert_eq!(ct, "application/octet-stream");
    assert_eq!(body, &[1u8, 2, 3]);
}

#[test]
fn fetch_post_content_type_override() {
    let capture = CaptureFetch::new();
    let rt = v8_runtime_with_fetch(Arc::clone(&capture));
    rt.eval(
        "var fd = new FormData(); fd.append('x', '1'); \
                 fetch('https://example.com/', { method: 'POST', body: fd, \
                   headers: {'Content-Type': 'application/json'} })"
    ).unwrap();
    let calls = capture.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let (_, _, ct, _) = &calls[0];
    assert_eq!(ct, "application/json");
}
