//! Тесты `v8_whatwg_streams`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

use super::*;
use crate::v8_runtime::V8JsRuntime;

// V8 twin of the (removed) QuickJS `runtime_with_dom` helper.
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

// Mock fetch provider that records calls to fetch_with_body_sync.
// V8 twin of the (removed) QuickJS `CaptureFetch` mock: after porting
// `fetch_response_body_getreader_yields_correct_bytes`, no QuickJS-region
// test used the original any longer, so it was deleted rather than kept dead.
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

// V8 twin of the (removed) QuickJS `runtime_with_fetch` helper.
fn v8_runtime_with_fetch(provider: Arc<CaptureFetch>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let p: Arc<dyn lumen_core::ext::JsFetchProvider> = provider;
    rt.install_dom(make_doc(), "https://example.com/", Some(p), None, None, None, None, None, None, None, false).unwrap();
    rt
}

/// BUG-749: провайдер, записывающий author-заголовки запроса в
/// `name:value;…`. Читать результат через `r.text()` нельзя — промис
/// fetch-а резолвится микротаской, а `eval` её не прокручивает; запись
/// в Mutex видна сразу, потому что сам запрос синхронный.
struct CaptureHeadersFetch {
    seen: std::sync::Mutex<String>,
}
impl lumen_core::ext::JsFetchProvider for CaptureHeadersFetch {
    fn fetch_sync(&self, _url: &str, _method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        unreachable!("fetch_request перекрыт — сюда попадать нечему")
    }
    fn fetch_request(&self, req: &lumen_core::ext::JsFetchRequest<'_>) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        let mut out = self.seen.lock().unwrap();
        for (name, value) in req.headers {
            out.push_str(name);
            out.push(':');
            out.push_str(value);
            out.push(';');
        }
        Ok(lumen_core::ext::JsFetchResult {
            status: 200,
            status_text: "OK".into(),
            headers: vec![],
            body: b"ok".to_vec(),
        })
    }
}

/// Прочитать записанные заголовки, дождавшись рабочего потока.
///
/// `fetch(new Request(...))` уходит по async-пути: у `Request` всегда
/// есть `signal`, а живой сигнал переводит запрос на фоновый поток
/// (`_lumen_fetch_async_start`). Синхронные формы записывают заголовки
/// ещё внутри `eval`, поэтому первая же итерация их и увидит.
fn captured_headers(capture: &CaptureHeadersFetch) -> String {
    for _ in 0..300 {
        let seen = capture.seen.lock().unwrap().clone();
        if !seen.is_empty() {
            return seen;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    capture.seen.lock().unwrap().clone()
}

/// Runtime, чей fetch-провайдер записывает author-заголовки запроса.
fn v8_runtime_with_header_capture() -> (V8JsRuntime, Arc<CaptureHeadersFetch>) {
    let capture = Arc::new(CaptureHeadersFetch { seen: std::sync::Mutex::new(String::new()) });
    let rt = V8JsRuntime::new().unwrap();
    let p: Arc<dyn lumen_core::ext::JsFetchProvider> = Arc::clone(&capture) as _;
    rt.install_dom(make_doc(), "https://example.com/", Some(p), None, None, None, None, None, None, None, false).unwrap();
    (rt, capture)
}

/// Все три спековых способа задать заголовки (запись, `Headers`,
/// `Request`) обязаны доехать до провайдера. До BUG-749 не доезжал ни
/// один: у моста не было параметра под заголовки вовсе.
#[test]
fn fetch_author_headers_reach_the_provider() {
    for expr in [
        "fetch('/api', { headers: { 'X-Probe': '1' } })",
        "fetch('/api', { headers: new Headers([['X-Probe', '1']]) })",
        "fetch(new Request('/api', { headers: { 'X-Probe': '1' } }))",
    ] {
        let (rt, capture) = v8_runtime_with_header_capture();
        rt.eval(expr).unwrap();
        assert_eq!(
            captured_headers(&capture),
            "x-probe:1;",
            "форма задания заголовков `{expr}` не доехала до провайдера"
        );
    }
}

/// `init.headers` вытесняет список самого `Request`-а целиком
/// (Fetch §5.5 шаг 32-33), а не дополняет его.
#[test]
fn fetch_init_headers_replace_request_headers() {
    let (rt, capture) = v8_runtime_with_header_capture();
    rt.eval(
        "fetch(new Request('/api', { headers: { 'X-Old': 'a' } }), { headers: { 'X-New': 'b' } });",
    )
    .unwrap();
    assert_eq!(captured_headers(&capture), "x-new:b;");
}

/// Guard 'request' применяется даже к готовому `Headers`: страница
/// могла собрать его конструктором, где guard === 'none', и тогда
/// `Host`/`Cookie` прошли бы в мост.
#[test]
fn fetch_forbidden_header_never_reaches_the_provider() {
    let (rt, capture) = v8_runtime_with_header_capture();
    rt.eval(
        "var h = new Headers(); h.append('Host', 'evil.example'); h.append('X-Ok', '1'); \
                 fetch('/api', { headers: h });",
    )
    .unwrap();
    assert_eq!(captured_headers(&capture), "x-ok:1;");
}

/// `XMLHttpRequest.setRequestHeader` пишет в свой объект мимо guard-а
/// `Headers` — до BUG-749 из всего списка на провод уходил только
/// Content-Type, и то лишь при наличии тела.
#[test]
fn xhr_set_request_header_reaches_the_provider() {
    let (rt, capture) = v8_runtime_with_header_capture();
    rt.eval(
        "var x = new XMLHttpRequest(); x.open('GET', '/api'); \
                 x.setRequestHeader('X-Probe', '1'); x.send();",
    )
    .unwrap();
    assert_eq!(captured_headers(&capture), "x-probe:1;");
}

/// Mock provider whose body is the URL's last path segment, so two
/// responses in flight at once can be told apart by their bodies.
struct EchoUrlFetch;
impl lumen_core::ext::JsFetchProvider for EchoUrlFetch {
    fn fetch_sync(&self, url: &str, _method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        let tail = url.rsplit('/').next().unwrap_or("");
        Ok(lumen_core::ext::JsFetchResult {
            status: 200,
            status_text: "OK".into(),
            headers: vec![],
            body: format!("body-{tail}").into_bytes(),
        })
    }
    fn fetch_with_body_sync(&self, url: &str, method: &str, _content_type: &str, _body: &[u8]) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        self.fetch_sync(url, method)
    }
}

/// Runtime whose fetch provider echoes the requested URL back as the body.
fn v8_runtime_with_echo_fetch() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let p: Arc<dyn lumen_core::ext::JsFetchProvider> = Arc::new(EchoUrlFetch);
    rt.install_dom(make_doc(), "https://example.com/", Some(p), None, None, None, None, None, None, None, false).unwrap();
    rt
}

#[test]
fn readable_stream_constructor_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.ReadableStream === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn writable_stream_constructor_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.WritableStream === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn transform_stream_constructor_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.TransformStream === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn readable_stream_locked_initially_false() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var rs = new ReadableStream(); rs.locked === false"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn readable_stream_get_reader_locks_stream() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var rs = new ReadableStream({ start: function(c) { c.close(); } }); \
                 var reader = rs.getReader(); \
                 rs.locked === true"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn readable_stream_read_delivers_chunk_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var done = false; \
                 var rs = new ReadableStream({ \
                   start: function(c) { c.enqueue('hello'); c.close(); } \
                 }); \
                 var reader = rs.getReader(); \
                 reader.read().then(function(r) { done = (r.value === 'hello' && r.done === false); });"
    ).unwrap();
    let r = rt.eval("done").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn readable_stream_read_done_after_close() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var got = []; \
                 var rs = new ReadableStream({ \
                   start: function(c) { c.enqueue(1); c.enqueue(2); c.close(); } \
                 }); \
                 var reader = rs.getReader(); \
                 reader.read().then(function(r) { got.push(r.value); }); \
                 reader.read().then(function(r) { got.push(r.value); }); \
                 reader.read().then(function(r) { got.push(r.done ? 'done' : 'nodone'); });"
    ).unwrap();
    let r = rt.eval(
        "got.length === 3 && got[0] === 1 && got[1] === 2 && got[2] === 'done'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn readable_stream_release_lock_unlocks() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var rs = new ReadableStream({ start: function(c) { c.close(); } }); \
                 var reader = rs.getReader(); \
                 reader.releaseLock(); \
                 rs.locked === false"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn readable_stream_tee_produces_two_streams() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var rs = new ReadableStream({ \
                   start: function(c) { c.enqueue(42); c.close(); } \
                 }); \
                 var pair = rs.tee(); \
                 pair.length === 2 && pair[0] instanceof ReadableStream && pair[1] instanceof ReadableStream"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn readable_stream_tee_both_clones_have_data() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var rs = new ReadableStream({ \
                   start: function(c) { c.enqueue(99); c.close(); } \
                 }); \
                 var pair = rs.tee(); \
                 var v1, v2; \
                 pair[0].getReader().read().then(function(r) { v1 = r.value; }); \
                 pair[1].getReader().read().then(function(r) { v2 = r.value; });"
    ).unwrap();
    let r = rt.eval("v1 === 99 && v2 === 99").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Streams §4.8.3: the sink is never called from the same turn as `write()`
/// — the controller only starts advancing its queue once the promise
/// wrapping `start()` settles. So the assertion has to live in a second
/// `eval()`, after the microtask checkpoint (BUG-823 made this shim honour
/// that ordering; before, `write()` called the sink synchronously).
#[test]
fn writable_stream_get_writer_and_write() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var written = []; \
                 var ws = new WritableStream({ \
                   write: function(chunk) { written.push(chunk); } \
                 }); \
                 var writer = ws.getWriter(); \
                 writer.write('a'); writer.write('b');"
    ).unwrap();
    let r = rt.eval(
        "written.length === 2 && written[0] === 'a' && written[1] === 'b'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn writable_stream_locked_when_writer_held() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var ws = new WritableStream(); \
                 var w = ws.getWriter(); \
                 ws.locked === true"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn writable_stream_close_resolves() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var closed = false, closePromiseSettled = false; \
                 var ws = new WritableStream({ close: function() { closed = true; } }); \
                 var w = ws.getWriter(); \
                 w.close().then(function() { closePromiseSettled = true; });"
    ).unwrap();
    // The sink runs on the checkpoint, and the promise `close()` returned
    // settles with it — BUG-823 left the second half hanging.
    let r = rt.eval("closed && closePromiseSettled").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── BUG-823: every state transition settles the promises waiting on it ──
//
// Each of these used to leave one promise pending forever, and because
// `testharness.js` runs `promise_test`s in sequence, a single such promise
// took the rest of the WPT file with it — hence a TIMEOUT of the whole
// `.any.html` instead of one failing subtest.

/// The bug's own repro (`writable-streams/close.any.js`): a sink that throws
/// from `close()` rejected `close()` and left `writer.closed` hanging.
#[test]
fn writer_closed_rejects_when_sink_close_throws() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var log = []; \
                 var ws = new WritableStream({ close: function() { throw new Error('boom'); } }); \
                 var w = ws.getWriter(); \
                 w.write('y').then(function() { log.push('write-ok'); }, \
                                   function() { log.push('write-rejected'); }); \
                 w.close().then(function() { log.push('close-ok'); }, \
                                function(e) { log.push('close-rejected ' + e.message); }); \
                 w.closed.then(function() { log.push('closed-ok'); }, \
                               function(e) { log.push('closed-rejected ' + e.message); });"
    ).unwrap();
    let r = rt.eval(
        "log.indexOf('write-ok') >= 0 \
                 && log.indexOf('close-rejected boom') >= 0 \
                 && log.indexOf('closed-rejected boom') >= 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A throwing `write()` errors the stream, so both the write promise and
/// `writer.closed` have to hear about it.
#[test]
fn writer_closed_rejects_when_sink_write_throws() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var log = []; \
                 var ws = new WritableStream({ write: function() { throw new Error('wboom'); } }); \
                 var w = ws.getWriter(); \
                 w.write('a').then(function() { log.push('write-ok'); }, \
                                   function(e) { log.push('write-rejected ' + e.message); }); \
                 w.closed.then(function() { log.push('closed-ok'); }, \
                               function(e) { log.push('closed-rejected ' + e.message); });"
    ).unwrap();
    let r = rt.eval(
        "log.indexOf('write-rejected wboom') >= 0 \
                 && log.indexOf('closed-rejected wboom') >= 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// The sink's `abort()` had no call site at all before BUG-823.
#[test]
fn writable_stream_abort_calls_sink_abort() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var log = [], aborted = null; \
                 var ws = new WritableStream({ abort: function(reason) { aborted = reason; } }); \
                 var w = ws.getWriter(); \
                 w.abort('why').then(function() { log.push('abort-resolved'); }, \
                                     function() { log.push('abort-rejected'); }); \
                 w.closed.then(function() { log.push('closed-ok'); }, \
                               function(e) { log.push('closed-rejected ' + e); });"
    ).unwrap();
    let r = rt.eval(
        "aborted === 'why' \
                 && log.indexOf('abort-resolved') >= 0 \
                 && log.indexOf('closed-rejected why') >= 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A thenable from `start()` used to be discarded, so «start returned a
/// rejection» never reached the stream.
#[test]
fn writable_stream_start_rejection_errors_the_stream() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = ''; \
                 var ws = new WritableStream({ \
                   start: function() { return Promise.reject(new Error('nope')); } \
                 }); \
                 var w = ws.getWriter(); \
                 w.closed.then(function() { out = 'closed-ok'; }, \
                               function(e) { out = 'closed-rejected ' + e.message; });"
    ).unwrap();
    let r = rt.eval("out === 'closed-rejected nope'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// The writable half of a pipe going down has to reject the pipe promise
/// rather than leave the page waiting on it.
#[test]
fn pipe_to_rejects_when_destination_write_throws() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = ''; \
                 var rs = new ReadableStream({ start: function(c) { c.enqueue('a'); c.close(); } }); \
                 var ws = new WritableStream({ write: function() { throw new Error('dead'); } }); \
                 rs.pipeTo(ws).then(function() { out = 'resolved'; }, \
                                    function(e) { out = 'rejected ' + e.message; });"
    ).unwrap();
    let r = rt.eval("out === 'rejected dead'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `controller.error()` reached only the read requests already standing;
/// `reader.closed` heard nothing.
#[test]
fn readable_stream_error_rejects_reader_closed() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = [], ctrl = null; \
                 var rs = new ReadableStream({ start: function(c) { ctrl = c; } }); \
                 var reader = rs.getReader(); \
                 reader.closed.then(function() { out.push('closed-ok'); }, \
                                    function(e) { out.push('closed-rejected ' + e.message); }); \
                 reader.read().then(function() { out.push('read-ok'); }, \
                                    function(e) { out.push('read-rejected ' + e.message); }); \
                 ctrl.error(new Error('bang'));"
    ).unwrap();
    let r = rt.eval(
        "out.indexOf('closed-rejected bang') >= 0 && out.indexOf('read-rejected bang') >= 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Same gap on the readable side: a rejected `start()` has to error the
/// stream, which is what «start should be able to return a promise and
/// reject it» asks for.
#[test]
fn readable_stream_start_rejection_errors_the_stream() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = ''; \
                 var rs = new ReadableStream({ \
                   start: function() { return Promise.reject(new Error('late')); } \
                 }); \
                 var reader = rs.getReader(); \
                 reader.read().then(function() { out = 'read-ok'; }, \
                                    function(e) { out = 'read-rejected ' + e.message; });"
    ).unwrap();
    let r = rt.eval("out === 'read-rejected late'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `cancel()` swallowed whatever the source's own `cancel()` did.
#[test]
fn readable_stream_cancel_surfaces_source_rejection() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = ''; \
                 var rs = new ReadableStream({ \
                   cancel: function() { return Promise.reject(new Error('cboom')); } \
                 }); \
                 rs.cancel('r').then(function() { out = 'resolved'; }, \
                                     function(e) { out = 'rejected ' + e.message; });"
    ).unwrap();
    let r = rt.eval("out === 'rejected cboom'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// «errors thrown in transform put the writable and readable in an errored
/// state» — the two halves are wired to each other now.
#[test]
fn transform_stream_transform_error_reaches_both_sides() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = []; \
                 var ts = new TransformStream({ \
                   transform: function() { throw new Error('tboom'); } \
                 }); \
                 var writer = ts.writable.getWriter(); \
                 var reader = ts.readable.getReader(); \
                 writer.write('x').then(function() { out.push('write-ok'); }, \
                                        function(e) { out.push('write-rejected ' + e.message); }); \
                 writer.closed.then(function() { out.push('writer-closed-ok'); }, \
                                    function(e) { out.push('writer-closed-rejected ' + e.message); }); \
                 reader.closed.then(function() { out.push('reader-closed-ok'); }, \
                                    function(e) { out.push('reader-closed-rejected ' + e.message); });"
    ).unwrap();
    let r = rt.eval(
        "out.indexOf('write-rejected tboom') >= 0 \
                 && out.indexOf('writer-closed-rejected tboom') >= 0 \
                 && out.indexOf('reader-closed-rejected tboom') >= 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Cancelling the readable end of a transform errors the writable one, so a
/// writer parked on `closed` is not left there (`transform-streams/backpressure`).
#[test]
fn transform_stream_readable_cancel_errors_writable() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = ''; \
                 var ts = new TransformStream(); \
                 var writer = ts.writable.getWriter(); \
                 writer.closed.then(function() { out = 'closed-ok'; }, \
                                    function() { out = 'closed-rejected'; }); \
                 ts.readable.cancel('done');"
    ).unwrap();
    let r = rt.eval("out === 'closed-rejected'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── BUG-824: tee / BYOB / async iteration / a stream as a body ─────────
//
// Four surfaces that a test could only hang on, because each of them was
// either absent or silently substituted by something with different
// semantics. The fifth surface the bug listed — `TextDecoderStream` never
// closing its readable side — turned out to have been fixed as a side
// effect of BUG-823's writable-side rewrite; `text_decoder_stream_closes_
// its_readable_side` below is the guard that keeps it that way.

/// `tee()` used to snapshot the controller's queue into two stubs and close
/// the source, so the source reported `locked === false` and everything it
/// enqueued after the call went nowhere.
#[test]
fn readable_stream_tee_locks_source_and_forwards_later_chunks() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = [], ctrl = null; \
                 var rs = new ReadableStream({ start: function(c) { ctrl = c; c.enqueue('early'); } }); \
                 var pair = rs.tee(); \
                 out.push('locked=' + rs.locked); \
                 ctrl.enqueue('late'); \
                 var r0 = pair[0].getReader(), r1 = pair[1].getReader(); \
                 r0.read().then(function(r) { out.push('a0=' + r.value); return r0.read(); }) \
                          .then(function(r) { out.push('a1=' + r.value); }); \
                 r1.read().then(function(r) { out.push('b0=' + r.value); return r1.read(); }) \
                          .then(function(r) { out.push('b1=' + r.value); });"
    ).unwrap();
    for _ in 0..8 {
        rt.eval("0").unwrap();
    }
    let r = rt.eval("out.join(',')").unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("locked=true,a0=early,b0=early,a1=late,b1=late".into())
    );
}

/// «canceling both branches should aggregate the cancel reasons» — the first
/// subtest of `readable-streams/tee.any.js` this used to hang on. One branch
/// cancelling must leave the source alone.
#[test]
fn readable_stream_tee_aggregates_cancel_reasons() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var seen = null, settled = 0; \
                 var rs = new ReadableStream({ cancel: function(reason) { seen = reason; } }); \
                 var pair = rs.tee(); \
                 pair[0].cancel('a').then(function() { settled++; }); \
                 var afterFirst = seen; \
                 pair[1].cancel('b').then(function() { settled++; });"
    ).unwrap();
    let r = rt.eval(
        "afterFirst === null && Array.isArray(seen) \
                 && seen[0] === 'a' && seen[1] === 'b' && settled === 2"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `type: 'bytes'` was ignored, so `getReader({mode:'byob'})` handed back a
/// default reader and a whole `Uint8Array` chunk — a silent change of
/// semantics rather than an error.
#[test]
fn readable_stream_byob_reader_fills_the_callers_view() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = []; \
                 var bs = new ReadableStream({ type: 'bytes', \
                   start: function(c) { c.enqueue(new Uint8Array([7, 8, 9])); c.close(); } }); \
                 var r = bs.getReader({ mode: 'byob' }); \
                 out.push('ctor=' + r.constructor.name); \
                 r.read(new Uint8Array(2)) \
                  .then(function(x) { out.push('one=' + Array.from(x.value) + '/' + x.done); return r.read(new Uint8Array(2)); }) \
                  .then(function(x) { out.push('two=' + Array.from(x.value) + '/' + x.done); return r.read(new Uint8Array(2)); }) \
                  .then(function(x) { out.push('end=' + x.value.length + '/' + x.done); });"
    ).unwrap();
    for _ in 0..8 {
        rt.eval("0").unwrap();
    }
    let r = rt.eval("out.join(' ')").unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "ctor=ReadableStreamBYOBReader one=7,8/false two=9/false end=0/true".into()
        )
    );
}

/// The source side of BYOB: `controller.byobRequest` hands the source the
/// caller's own buffer, and `respond(n)` delivers exactly those bytes.
#[test]
fn readable_byte_stream_controller_exposes_byob_request() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = []; \
                 var bs = new ReadableStream({ type: 'bytes', pull: function(c) { \
                   var req = c.byobRequest; \
                   if (!req) { out.push('no-request'); return; } \
                   out.push('view=' + req.view.byteLength); \
                   new Uint8Array(req.view.buffer, req.view.byteOffset, req.view.byteLength)[0] = 42; \
                   req.respond(1); \
                 } }); \
                 var r = bs.getReader({ mode: 'byob' }); \
                 r.read(new Uint8Array(4)).then(function(x) { out.push('got=' + Array.from(x.value)); });"
    ).unwrap();
    let r = rt.eval(
        "out.indexOf('view=4') >= 0 && out.indexOf('got=42') >= 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A `byob` reader on a stream that is not a byte stream is an error, not a
/// quiet downgrade; so is an unknown `type`.
#[test]
fn readable_stream_byob_mode_requires_a_byte_stream() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var threwMode = false, threwType = false; \
                 try { new ReadableStream().getReader({ mode: 'byob' }); } catch (e) { threwMode = e instanceof TypeError; } \
                 try { new ReadableStream({ type: 'chars' }); } catch (e) { threwType = e instanceof TypeError; } \
                 threwMode && threwType"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `for await (const chunk of stream)` — the most common way modern code
/// reads a stream — used to throw «rs is not async iterable».
#[test]
fn readable_stream_async_iteration_yields_every_chunk() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = ''; \
                 (async function() { \
                    var acc = []; \
                    for await (var c of ReadableStream.from(['a', 'b', 'c'])) acc.push(c); \
                    out = acc.join(''); \
                  })();"
    ).unwrap();
    let r = rt.eval("out").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("abc".into()));
}

/// Leaving the loop early runs the iterator's `return()`, which cancels the
/// stream and releases the lock — otherwise the stream would stay locked for
/// good.
#[test]
fn readable_stream_async_iteration_break_cancels_the_stream() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var cancelled = null, locked = null; \
                 var rs = new ReadableStream({ \
                   start: function(c) { c.enqueue(1); c.enqueue(2); }, \
                   cancel: function(reason) { cancelled = reason === undefined ? 'yes' : reason; } \
                 }); \
                 (async function() { \
                    for await (var c of rs) break; \
                    locked = rs.locked; \
                  })();"
    ).unwrap();
    let r = rt.eval("cancelled === 'yes' && locked === false").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A `Response`/`Request` built over a ReadableStream used to substitute an
/// empty body: the constructor built a *fresh* stream and dropped the one it
/// was handed, so `arrayBuffer()` resolved with zero bytes.
#[test]
fn response_over_a_stream_body_keeps_its_bytes() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = ''; \
                 var rs = new ReadableStream({ start: function(c) { \
                   c.enqueue(new Uint8Array([1, 2, 3])); c.enqueue(new Uint8Array([4])); c.close(); } }); \
                 var resp = new Response(rs); \
                 var sameStream = resp.body === rs; \
                 resp.arrayBuffer().then(function(b) { \
                   out = sameStream + ':' + Array.from(new Uint8Array(b)).join(','); });"
    ).unwrap();
    for _ in 0..8 {
        rt.eval("0").unwrap();
    }
    let r = rt.eval("out").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("true:1,2,3,4".into()));
}

/// Fetch §2.3 «clone a body» tees the stream — the canonical reason to want
/// a working `tee()` in the first place (cache + parse from one response).
#[test]
fn response_clone_over_a_stream_body_tees_it() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var a = '', b = ''; \
                 var rs = new ReadableStream({ start: function(c) { \
                   c.enqueue(new Uint8Array([9, 9])); c.close(); } }); \
                 var first = new Response(rs); \
                 var second = first.clone(); \
                 first.text().then(function(t) { a = String(t.length); }); \
                 second.arrayBuffer().then(function(x) { b = Array.from(new Uint8Array(x)).join(','); });"
    ).unwrap();
    for _ in 0..8 {
        rt.eval("0").unwrap();
    }
    let r = rt.eval("a + '|' + b").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("2|9,9".into()));
}

/// Regression guard for the fifth surface BUG-824 listed: closing the
/// writable side of a `TextDecoderStream` must reach its readable side, or
/// `readableStreamToArray` — the helper the whole `encoding/streams`
/// category is built on — never sees `done`.
#[test]
fn text_decoder_stream_closes_its_readable_side() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = []; \
                 var tds = new TextDecoderStream(); \
                 var w = tds.writable.getWriter(), r = tds.readable.getReader(); \
                 w.write(new Uint8Array([104, 105])); w.close(); \
                 r.read().then(function(x) { out.push(x.value + '/' + x.done); return r.read(); }) \
                         .then(function(x) { out.push(x.value + '/' + x.done); });"
    ).unwrap();
    for _ in 0..8 {
        rt.eval("0").unwrap();
    }
    let r = rt.eval("out.join(' ')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("hi/false undefined/true".into()));
}

#[test]
fn transform_stream_has_readable_and_writable() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var ts = new TransformStream(); \
                 ts.readable instanceof ReadableStream && ts.writable instanceof WritableStream"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn transform_stream_passthrough() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var received = []; \
                 var ts = new TransformStream(); \
                 var writer = ts.writable.getWriter(); \
                 var reader = ts.readable.getReader(); \
                 writer.write('x'); \
                 reader.read().then(function(r) { received.push(r.value); });"
    ).unwrap();
    let r = rt.eval("received.length === 1 && received[0] === 'x'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn transform_stream_custom_transformer() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = []; \
                 var ts = new TransformStream({ \
                   transform: function(chunk, ctrl) { ctrl.enqueue(chunk * 2); } \
                 }); \
                 var writer = ts.writable.getWriter(); \
                 var reader = ts.readable.getReader(); \
                 writer.write(5); \
                 reader.read().then(function(r) { out.push(r.value); });"
    ).unwrap();
    let r = rt.eval("out.length === 1 && out[0] === 10").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn pipe_to_writable_stream() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var collected = []; \
                 var rs = new ReadableStream({ \
                   start: function(c) { c.enqueue('a'); c.enqueue('b'); c.close(); } \
                 }); \
                 var ws = new WritableStream({ write: function(ch) { collected.push(ch); } }); \
                 var done = false; \
                 rs.pipeTo(ws).then(function() { done = true; });"
    ).unwrap();
    let r = rt.eval(
        "done && collected.length === 2 && collected[0] === 'a' && collected[1] === 'b'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn pipe_through_transform_stream() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = []; \
                 var rs = new ReadableStream({ \
                   start: function(c) { c.enqueue(3); c.close(); } \
                 }); \
                 var ts = new TransformStream({ \
                   transform: function(chunk, ctrl) { ctrl.enqueue(chunk + 10); } \
                 }); \
                 var dest = rs.pipeThrough(ts); \
                 dest.getReader().read().then(function(r) { out.push(r.value); });"
    ).unwrap();
    let r = rt.eval("out.length === 1 && out[0] === 13").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn blob_stream_returns_readable_stream() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "new Blob(['hello']).stream() instanceof ReadableStream"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn blob_stream_delivers_bytes() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var got = null; \
                 var blob = new Blob(['hi']); \
                 var reader = blob.stream().getReader(); \
                 reader.read().then(function(r) { got = r.value instanceof Uint8Array ? r.value.length : -1; });"
    ).unwrap();
    let r = rt.eval("got === 2").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn response_body_is_readable_stream() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "new Response('hello').body instanceof ReadableStream"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn response_body_used_starts_false() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "new Response('data').bodyUsed === false"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn response_body_used_after_text() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var resp = new Response('x'); \
                 resp.text().then(function() {}); \
                 resp.bodyUsed === true"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── K-3: Fetch streaming body tests ──────────────────────────────────────

#[test]
fn response_body_reader_reads_first_chunk() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = null; \
                 var reader = new Response('hello').body.getReader(); \
                 reader.read().then(function(r) { out = r; });"
    ).unwrap();
    let r = rt.eval(
        "out !== null && !out.done && out.value instanceof Uint8Array && out.value.length === 5"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn response_body_reader_done_after_all_chunks() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var done = false; \
                 var reader = new Response('hi').body.getReader(); \
                 reader.read().then(function() { return reader.read(); }) \
                       .then(function(r) { done = r.done; });"
    ).unwrap();
    let r = rt.eval("done === true").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn response_body_getreader_marks_body_used() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var resp = new Response('data'); \
                 resp.body.getReader(); \
                 resp.bodyUsed === true"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn response_body_text_rejects_after_getreader() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var resp = new Response('abc'); \
                 resp.body.getReader(); \
                 var rejected = false; \
                 resp.text().then(null, function() { rejected = true; });"
    ).unwrap();
    let r = rt.eval("rejected === true").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn response_body_getreader_rejects_if_already_used() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var resp = new Response('x'); \
                 resp.text().then(function() {}); \
                 var threw = false; \
                 try { resp.body.getReader(); } catch(e) { threw = true; } \
                 threw === true"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn fetch_body_chunk_binding_returns_slice() {
    // _lumen_fetch_body_length / _lumen_fetch_body_chunk work when no cache is set.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "_lumen_fetch_body_length() === 0 && _lumen_fetch_body_chunk(0, 10).length === 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn fetch_cache_response_reads_own_stream_queue_not_global_slot() {
    // BUG-703: a body up to _RS_CHUNK is drained into the stream's own
    // queue by the eager pull in the ReadableStream constructor, which
    // frees the per-response slot. Consuming the body must then read that
    // queue; falling through to the process-wide FetchCache slot handed
    // the response the body of whatever request finished last. Two fetches
    // are issued before either body is read — with the fallback in play the
    // first response reports the second one's body.
    let rt = v8_runtime_with_echo_fetch();
    rt.eval(
        "var first = null, second = null; \
                 var a = fetch('https://example.com/one'); \
                 var b = fetch('https://example.com/two'); \
                 a.then(function(r) { return r.text(); }).then(function(t) { first = t; }); \
                 b.then(function(r) { return r.text(); }).then(function(t) { second = t; });",
    )
    .unwrap();
    let r = rt.eval("first + '|' + second").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("body-one|body-two".into()));
}

#[test]
fn stream_slot_alloc_returns_zero_when_no_cache() {
    // _lumen_stream_alloc returns 0 when FetchCache is empty (no prior fetch).
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("_lumen_stream_alloc() === 0").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn fetch_response_body_getreader_yields_correct_bytes() {
    // fetch() via mock provider → response.body.getReader().read() delivers body bytes.
    let capture = CaptureFetch::new();
    let rt = v8_runtime_with_fetch(Arc::clone(&capture));
    rt.eval(
        "var out = null; \
                 fetch('https://example.com/').then(function(resp) { \
                     return resp.body.getReader().read(); \
                 }).then(function(r) { out = r; });"
    ).unwrap();
    let r = rt.eval(
        "out !== null && !out.done && out.value instanceof Uint8Array \
                 && out.value[0] === 111 && out.value[1] === 107"  // 'ok' = [111, 107]
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn fetch_resolves_relative_url_against_document_base() {
    // BUG-347: fetch('resources/x.js') on a page served at
    // https://example.com/ must resolve to the absolute URL before
    // reaching the native binding, not fail as "missing scheme".
    let capture = CaptureFetch::new();
    let rt = v8_runtime_with_fetch(Arc::clone(&capture));
    rt.eval("fetch('resources/x.js');").unwrap();
    let calls = capture.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "https://example.com/resources/x.js");
}

#[test]
fn fetch_resolves_root_relative_url_against_document_origin() {
    let capture = CaptureFetch::new();
    let rt = v8_runtime_with_fetch(Arc::clone(&capture));
    rt.eval("fetch('/common/blank.html');").unwrap();
    let calls = capture.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "https://example.com/common/blank.html");
}

#[test]
fn request_constructor_absolutizes_relative_url() {
    let rt = v8_runtime_with_fetch(CaptureFetch::new());
    let r = rt.eval("new Request('resources/x.js').url").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("https://example.com/resources/x.js".into()));
}

#[test]
fn text_decoder_stream_decodes_utf8() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = []; \
                 var tds = new TextDecoderStream(); \
                 var writer = tds.writable.getWriter(); \
                 var reader = tds.readable.getReader(); \
                 writer.write(new Uint8Array([72, 101, 108, 108, 111])); \
                 reader.read().then(function(r) { out.push(r.value); });"
    ).unwrap();
    let r = rt.eval("out.length === 1 && out[0] === 'Hello'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn text_decoder_stream_mode_ascii() {
    // {stream: true} with complete ASCII works like normal decode.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var dec = new TextDecoder(); \
                 var s = dec.decode(new Uint8Array([72,101,108,108,111]), {stream: true}); \
                 s === 'Hello'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn text_decoder_stream_mode_buffers_partial_utf8() {
    // Euro sign € = 0xE2 0x82 0xAC (3-byte UTF-8).
    // Sending only the first byte with stream:true must return '' and buffer it.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var dec = new TextDecoder(); \
                 var partial = dec.decode(new Uint8Array([0xE2]), {stream: true}); \
                 partial === ''"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn text_decoder_stream_mode_reassembles_split_multibyte() {
    // Continuation of previous: second chunk provides the rest of €.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var dec = new TextDecoder(); \
                 dec.decode(new Uint8Array([0xE2]), {stream: true}); \
                 var result = dec.decode(new Uint8Array([0x82, 0xAC])); \
                 result === '€'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn text_decoder_stream_mode_final_flush_clears_buffer() {
    // After streaming, final decode() with no args flushes (returns empty or replacement).
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var dec = new TextDecoder(); \
                 dec.decode(new Uint8Array([72]), {stream: true}); \
                 var flushed = dec.decode(); \
                 typeof flushed === 'string'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn text_decoder_no_arg_returns_empty_string() {
    // decode() with no arguments (empty flush) always returns a string.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var dec = new TextDecoder(); \
                 dec.decode() === '' && dec.decode(null) === ''"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn text_decoder_stream_decoder_stream_splits_multibyte() {
    // TextDecoderStream uses {stream:true} internally — writing bytes of €
    // in two chunks must produce the character exactly once.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = []; \
                 var tds = new TextDecoderStream(); \
                 var writer = tds.writable.getWriter(); \
                 var reader = tds.readable.getReader(); \
                 writer.write(new Uint8Array([0xE2])); \
                 reader.read().then(function(r) { if (!r.done) out.push(r.value); });"
    ).unwrap();
    rt.eval(
        "writer.write(new Uint8Array([0x82, 0xAC])); \
                 reader.read().then(function(r) { if (!r.done) out.push(r.value); });"
    ).unwrap();
    let r = rt.eval("out.join('') === '€'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn text_encoder_stream_encodes_string() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var out = []; \
                 var tes = new TextEncoderStream(); \
                 var writer = tes.writable.getWriter(); \
                 var reader = tes.readable.getReader(); \
                 writer.write('Hi'); \
                 reader.read().then(function(r) { out.push(r.value); });"
    ).unwrap();
    let r = rt.eval(
        "out.length === 1 && out[0] instanceof Uint8Array && out[0][0] === 72"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn byte_length_queuing_strategy() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var s = new ByteLengthQueuingStrategy({ highWaterMark: 16 }); \
                 s.highWaterMark === 16 && s.size(new Uint8Array(4)) === 4"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn count_queuing_strategy() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var s = new CountQueuingStrategy({ highWaterMark: 10 }); \
                 s.highWaterMark === 10 && s.size('anything') === 1"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn readable_stream_from_array() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var done = false; \
                 var rs = ReadableStream.from([10, 20, 30]); \
                 var reader = rs.getReader(); \
                 reader.read().then(function(r) { done = r.value === 10 && !r.done; });"
    ).unwrap();
    let r = rt.eval("done").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
