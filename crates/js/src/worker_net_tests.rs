//! Tests for [`super`] — a worker scope's `fetch()`/`XMLHttpRequest` over the
//! page's own `Response`/`Request` (WORKER-1 срез 5).
// Хелперы тестового модуля: исключение из clippy.toml покрывает только тело
// `#[test]` (docs/lint-policy.md §10).
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use lumen_core::JsValue;
use lumen_core::ext::JsRuntime as _;

use crate::v8_runtime::V8JsRuntime;

/// `(url, method, body, content type)` of one request that reached [`Net`].
type Seen = (String, String, Vec<u8>, String);

/// Records what reached the network and answers every URL with the same
/// canned response.
struct Net {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    final_url: Option<String>,
    seen: Mutex<Vec<Seen>>,
}

impl Net {
    fn new(status: u16, headers: &[(&str, &str)], body: &[u8]) -> Arc<Self> {
        Arc::new(Self {
            status,
            headers: headers.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect(),
            body: body.to_vec(),
            final_url: None,
            seen: Mutex::new(Vec::new()),
        })
    }
}

impl lumen_core::ext::JsFetchProvider for Net {
    fn fetch_sync(&self, url: &str, method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        self.fetch_request(&lumen_core::ext::JsFetchRequest { url, method, headers: &[], body: None, token: None })
    }

    fn fetch_request(
        &self,
        req: &lumen_core::ext::JsFetchRequest<'_>,
    ) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        let (bytes, ctype) = req.body.as_ref().map_or((Vec::new(), String::new()), |b| (b.bytes.to_vec(), b.content_type.to_string()));
        self.seen.lock().unwrap().push((req.url.to_string(), req.method.to_string(), bytes, ctype));
        Ok(lumen_core::ext::JsFetchResult {
            status: self.status,
            status_text: "OK".into(),
            headers: self.headers.clone(),
            body: self.body.clone(),
            url: self.final_url.clone().unwrap_or_else(|| req.url.to_string()),
        })
    }
}

/// A worker scope as the dedicated/shared flavours build it: the
/// `[Exposed=Worker]` shim, a base URL, then the network surface.
fn scope(net: Option<Arc<Net>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    crate::worker::install_worker_scope_globals_v8(&rt).unwrap();
    rt.eval(crate::worker::WORKER_TIMERS_SHIM).unwrap();
    rt.set_global("_lumen_worker_base_url", JsValue::String("https://w.test/dir/worker.js".into())).unwrap();
    let provider = net.map(|n| n as Arc<dyn lumen_core::ext::JsFetchProvider>);
    super::install_worker_net_v8(&rt, provider).unwrap();
    rt
}

fn text(rt: &V8JsRuntime, expr: &str) -> String {
    match rt.eval(expr).unwrap() {
        JsValue::String(s) => s,
        other => panic!("{expr} → {other:?}"),
    }
}

/// `fetch()` resolves with the page's `Response` class — real accessors, the
/// final URL, headers off the wire and the body's bytes intact (non-UTF-8
/// ones included, which the old base64-in-a-string transport mangled into
/// text first).
#[test]
fn fetch_resolves_with_the_page_response_class() {
    let mut n = Net::new(200, &[("content-type", "application/octet-stream"), ("x-a", "1")], &[0, 255, 128, 7]);
    Arc::get_mut(&mut n).unwrap().final_url = Some("https://w.test/moved".into());
    let rt = scope(Some(n));
    rt.eval(
        "globalThis.out = 'pending';\
         fetch('data.bin').then(function(r) {\
           var head = [r instanceof Response, r.status, r.ok, r.url, r.redirected, r.headers.get('x-a'),\
                       Object.getOwnPropertyNames(r).length].join(',');\
           return r.arrayBuffer().then(function(b) { out = head + '|' + Array.from(new Uint8Array(b)).join(' '); });\
         }, function(e) { out = 'rejected: ' + e; });",
    )
    .unwrap();
    assert_eq!(text(&rt, "out"), "true,200,true,https://w.test/moved,true,1,0|0 255 128 7");
}

/// The request goes through the `Request` constructor: a relative URL
/// resolves against the worker's script URL, and a POST body arrives with
/// the Content-Type it implies.
#[test]
fn fetch_sends_resolved_url_and_body() {
    let n = Net::new(200, &[], b"ok");
    let rt = scope(Some(Arc::clone(&n)));
    rt.eval(
        "globalThis.out = 'pending';\
         fetch('../api', { method: 'post', body: new URLSearchParams('a=1&b=2') })\
           .then(function(r) { return r.text(); }).then(function(t) { out = t; });",
    )
    .unwrap();
    assert_eq!(text(&rt, "out"), "ok");
    let seen = n.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "https://w.test/api");
    assert_eq!(seen[0].1, "POST");
    assert_eq!(seen[0].2, b"a=1&b=2");
    assert_eq!(seen[0].3, "application/x-www-form-urlencoded;charset=UTF-8");
}

/// A provider-less scope (headless dump modes) rejects with `TypeError`
/// rather than throwing synchronously.
#[test]
fn fetch_without_provider_rejects_with_type_error() {
    let rt = scope(None);
    rt.eval(
        "globalThis.out = 'pending';\
         fetch('https://x.test/').then(function() { out = 'resolved'; }, function(e) { out = e.name; });",
    )
    .unwrap();
    assert_eq!(text(&rt, "out"), "TypeError");
}

/// The mini-`Response` this replaced dropped a buffer body on the floor.
#[test]
fn response_constructor_keeps_a_buffer_body() {
    let rt = scope(None);
    rt.eval(
        "globalThis.out = 'pending';\
         new Response(new Uint8Array([1, 2, 3]).buffer, { status: 201 }).arrayBuffer()\
           .then(function(b) { out = b.byteLength + ':' + new Response(null, { status: 204 }).status; });",
    )
    .unwrap();
    assert_eq!(text(&rt, "out"), "3:204");
}

/// BUG-1081: `overrideMimeType` exists and its charset decides how
/// `responseText` is decoded; response headers are readable.
#[test]
fn xhr_override_mime_type_charset_decodes_response_text() {
    // "Привет" in windows-1251.
    let n = Net::new(200, &[("content-type", "text/plain"), ("x-b", "2")], &[0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2]);
    let rt = scope(Some(n));
    let got = text(
        &rt,
        "var x = new XMLHttpRequest();\
         x.open('GET', 't.txt', false);\
         x.overrideMimeType('text/plain; charset=\"windows-1251\"');\
         x.send();\
         [x.readyState, x.status, x.responseText, x.getResponseHeader('X-B'), x.responseURL].join('|')",
    );
    assert_eq!(got, "4|200|Привет|2|https://w.test/dir/t.txt");
}

/// An async request fires its events from a task, so handlers assigned after
/// `send()` — the order `encoding/resources/decoding-helpers.js` uses — run.
#[test]
fn xhr_async_events_reach_handlers_assigned_after_send() {
    let n = Net::new(200, &[], b"abc");
    let rt = scope(Some(n));
    rt.eval(
        "globalThis.log = [];\
         var x = new XMLHttpRequest();\
         x.open('GET', 'a');\
         x.responseType = 'arraybuffer';\
         x.send();\
         x.onreadystatechange = function() { log.push('rs' + x.readyState); };\
         x.onload = function() { log.push('load:' + x.response.byteLength); };\
         x.addEventListener('loadend', function(e) { log.push('loadend:' + e.loaded); });",
    )
    .unwrap();
    assert_eq!(text(&rt, "log.join(',')"), "");
    rt.eval("_lumen_worker_run_tasks()").unwrap();
    assert_eq!(text(&rt, "log.join(',')"), "rs2,rs3,rs4,load:3,loadend:3");
}


/// `overrideMimeType` labels go through the WHATWG table, not TextDecoder's:
/// a replacement label yields one U+FFFD, a UTF-32 label is unknown (UTF-8),
/// and a BOM overrides whatever the label said (Encoding §6 «decode»).
#[test]
fn xhr_text_follows_whatwg_labels_and_bom() {
    let cases: [(&str, &[u8], &str); 4] = [
        ("iso-2022-kr", b"abc", "65533"),
        ("iso-2022-kr", b"", ""),
        ("utf-32", &[0x41, 0x00, 0x00, 0x00], "65 0 0 0"),
        ("windows-1251", &[0xFF, 0xFE, 0x41, 0x00], "65"),
    ];
    for (label, body, want) in cases {
        let rt = scope(Some(Net::new(200, &[], body)));
        let got = text(
            &rt,
            &format!(
                "var x = new XMLHttpRequest(); x.open('GET', 'b', false);\
                 x.overrideMimeType('text/plain; charset={label}'); x.send();\
                 Array.from(x.responseText).map(function(c) {{ return c.codePointAt(0); }}).join(' ')"
            ),
        );
        assert_eq!(got, want, "{label}");
    }
}

/// A request with a body reports its upload on `xhr.upload`: `abort()` from
/// `loadstart` reaches the upload's `abort`/`loadend` first (XHR «request
/// error steps»), and a completed request fires the upload's
/// `loadstart … loadend` before the response's `readystatechange`s.
#[test]
fn xhr_upload_events() {
    let rt = scope(Some(Net::new(200, &[], b"r")));
    rt.eval(
        "globalThis.log = [];\
         var a = new XMLHttpRequest(); a.open('POST', 'u');\
         a.onloadstart = function() { a.abort(); };\
         a.upload.onabort = function(e) { log.push('up-abort:' + (e.target === a.upload)); };\
         a.upload.onloadend = function() { log.push('up-loadend'); };\
         a.onabort = function() { log.push('abort'); };\
         a.send('body');\
         var b = new XMLHttpRequest(); b.open('POST', 'u');\
         ['loadstart', 'progress', 'load', 'loadend'].forEach(function(t) {\
           b.upload.addEventListener(t, function(e) { log.push('up-' + t + ':' + e.loaded); });\
         });\
         b.onreadystatechange = function() { if (b.readyState === 2) log.push('rs2'); };\
         b.send('abcd');",
    )
    .unwrap();
    rt.eval("_lumen_worker_run_tasks()").unwrap();
    assert_eq!(
        text(&rt, "log.join(',')"),
        "up-abort:true,up-loadend,abort,up-loadstart:0,up-progress:4,up-load:4,up-loadend:4,rs2"
    );
}

