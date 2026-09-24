//! Тесты `v8_webworker`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    rt.eval(script).unwrap() == lumen_core::JsValue::Bool(true)
}

// ── Web Worker tests (WHATWG Web Workers §4) ─────────────────────────────

#[test]
fn worker_class_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof Worker === 'function'"));
}

#[test]
fn window_worker_class_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof window.Worker === 'function'"));
}

#[test]
fn worker_constructor_returns_instance() {
    let rt = v8_runtime_with_dom(make_doc());
    // Use a data: URL so no network fetch is needed.
    assert!(bool_eval(
        &rt,
        "var w = new Worker('data:text/javascript,'); w instanceof Worker"
    ));
}

#[test]
fn worker_has_post_message() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var w = new Worker('data:text/javascript,'); typeof w.postMessage === 'function'"
    ));
}

#[test]
fn worker_has_terminate() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var w = new Worker('data:text/javascript,'); typeof w.terminate === 'function'"
    ));
}

#[test]
fn worker_has_add_event_listener() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var w = new Worker('data:text/javascript,'); typeof w.addEventListener === 'function'"
    ));
}

#[test]
fn worker_onmessage_is_null_by_default() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var w = new Worker('data:text/javascript,'); w.onmessage === null"
    ));
}

#[test]
fn worker_onmessage_setter_and_getter() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var w = new Worker('data:text/javascript,'); \
                 var fn = function(e){}; \
                 w.onmessage = fn; \
                 w.onmessage === fn"
    ));
}

#[test]
fn worker_terminate_removes_from_registry() {
    let rt = v8_runtime_with_dom(make_doc());
    // terminate() should not throw and the worker object still exists.
    assert!(bool_eval(
        &rt,
        "var w = new Worker('data:text/javascript,'); \
                 w.terminate(); \
                 w instanceof Worker"
    ));
}

#[test]
fn worker_roundtrip_message_via_pump() {
    use std::time::Duration;
    let rt = v8_runtime_with_dom(make_doc());
    // Worker script: echo back any message with a 'reply' wrapper.
    let script = "data:text/javascript,onmessage%20%3D%20function(e)%7BpostMessage(%7Breply%3Ae.data%7D)%3B%7D";
    rt.eval(&format!("var w = new Worker('{}'); var received = null; w.onmessage = function(e){{received=e.data.reply;}}; w.postMessage(42);", script)).unwrap();
    // Give the worker thread time to process the message.
    std::thread::sleep(Duration::from_millis(150));
    rt.pump_workers();
    let result = rt.eval("received").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(42.0));
}

#[test]
fn worker_import_scripts_preserves_strict_global_declarations() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        r#"
        var importScopeResult = null;
        var helper = new Blob([
            "'use strict'; self.helperRan = true; function importedHelper() { return 41; } " +
            "var importedVar = 1; let importedLexical = 2;"
        ], {type: 'text/javascript'});
        var helperUrl = URL.createObjectURL(helper);
        var workerSource = "importScripts(" + JSON.stringify(helperUrl) + ");" +
            "postMessage([self.helperRan, typeof importedHelper, typeof importedVar, " +
            "typeof importedLexical].join('|'));";
        var workerUrl = URL.createObjectURL(new Blob([workerSource], {type: 'text/javascript'}));
        var importWorker = new Worker(workerUrl);
        importWorker.onmessage = function(e) { importScopeResult = e.data; };
        importWorker.onerror = function(e) { importScopeResult = 'ERROR: ' + e.message; };
        "#,
    )
    .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        rt.pump_workers();
        if rt.eval("importScopeResult !== null").unwrap() == lumen_core::JsValue::Bool(true) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        rt.eval("importScopeResult").unwrap(),
        lumen_core::JsValue::String("true|function|number|number".into())
    );
}

#[test]
fn worker_add_event_listener_fires_on_pump() {
    use std::time::Duration;
    let rt = v8_runtime_with_dom(make_doc());
    let script = "data:text/javascript,onmessage%20%3D%20function(e)%7BpostMessage(e.data%20*%202)%3B%7D";
    rt.eval(&format!(
        "var w = new Worker('{}'); \
                 var got = null; \
                 w.addEventListener('message', function(e){{got=e.data;}}); \
                 w.postMessage(7);",
        script
    ))
    .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    rt.pump_workers();
    let result = rt.eval("got").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(14.0));
}

// ── BUG-868 GAP-WORKERSCOPE срез 2: MessagePort transfer across the
// page↔worker boundary ────────────────────────────────────────────────────

#[test]
fn worker_message_port_transfer_page_to_worker_round_trip() {
    use std::time::Duration;
    let rt = v8_runtime_with_dom(make_doc());
    // The worker receives one transferred port on its first message,
    // wires up a reply on it, and posts back once its own onmessage sees
    // `e.ports.length === 1` — the exact idiom BUG-868 originally reported
    // as staying at `0` forever.
    let worker_src = "onmessage = function(e) {\
        if (e.data === 'init' && e.ports.length === 1) {\
            var p = e.ports[0];\
            p.onmessage = function(e2) { p.postMessage('pong:' + e2.data); };\
            postMessage('got-port');\
        }\
    };";
    rt.eval(&format!(
        "var workerUrl = URL.createObjectURL(new Blob([\"{worker_src}\"], {{type: 'text/javascript'}})); \
         var w = new Worker(workerUrl); \
         var log = []; \
         w.onmessage = function(e) {{ log.push(e.data); }}; \
         var ch = new MessageChannel(); \
         ch.port2.onmessage = function(e) {{ log.push(e.data); }}; \
         w.postMessage('init', [ch.port1]);"
    ))
    .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        rt.pump_workers();
        if rt.eval("log.length").unwrap() == lumen_core::JsValue::Number(1.0) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(rt.eval("log[0]").unwrap(), lumen_core::JsValue::String("got-port".into()));

    // The page's surviving `ch.port2` now talks to the worker's reconstructed
    // proxy for the transferred `ch.port1` — round-trip through the worker.
    rt.eval("ch.port2.postMessage('ping');").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        rt.pump_workers();
        if rt.eval("log.length").unwrap() == lumen_core::JsValue::Number(2.0) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        rt.eval("log[1]").unwrap(),
        lumen_core::JsValue::String("pong:ping".into())
    );
}

#[test]
fn worker_message_port_transfer_worker_to_page_round_trip() {
    use std::time::Duration;
    let rt = v8_runtime_with_dom(make_doc());
    // The worker creates its OWN channel and transfers one port out through
    // its own `postMessage` — the other reported half of BUG-868
    // (`self.postMessage('made-port', [p])` leaving `e.ports === undefined`
    // on the page).
    let worker_src = "var ch = new MessageChannel(); \
        ch.port1.onmessage = function(e) { ch.port1.postMessage('worker-pong:' + e.data); }; \
        postMessage('made-port', [ch.port2]);";
    rt.eval(&format!(
        "var workerUrl = URL.createObjectURL(new Blob([\"{worker_src}\"], {{type: 'text/javascript'}})); \
         var w = new Worker(workerUrl); \
         var pagePort = null; \
         var log = []; \
         w.onmessage = function(e) {{ \
             if (e.data === 'made-port' && e.ports.length === 1) {{ \
                 pagePort = e.ports[0]; \
                 pagePort.onmessage = function(e2) {{ log.push(e2.data); }}; \
                 log.push('made-port'); \
             }} \
         }};"
    ))
    .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        rt.pump_workers();
        if rt.eval("log.length").unwrap() == lumen_core::JsValue::Number(1.0) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(rt.eval("log[0]").unwrap(), lumen_core::JsValue::String("made-port".into()));

    rt.eval("pagePort.postMessage('hi');").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        rt.pump_workers();
        if rt.eval("log.length").unwrap() == lumen_core::JsValue::Number(2.0) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        rt.eval("log[1]").unwrap(),
        lumen_core::JsValue::String("worker-pong:hi".into())
    );
}

// ── BUG-591: worker parent-side reporting ────────────────────────────────

#[test]
fn worker_top_level_exception_fires_parent_onerror() {
    use std::time::Duration;
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var w = new Worker('data:text/javascript,throw new Error(\"boom\")'); \
                 var errEvent = null; \
                 w.onerror = function(e){ errEvent = e; };",
    )
    .unwrap();
    // Give the worker thread time to run its top-level script and fail.
    std::thread::sleep(Duration::from_millis(150));
    rt.pump_workers();
    assert!(bool_eval(&rt, "errEvent !== null"));
    assert!(bool_eval(&rt, "errEvent.type === 'error'"));
    assert!(bool_eval(&rt, "errEvent.message === 'boom'"));
}

#[test]
fn worker_onmessage_handler_exception_fires_parent_onerror() {
    use std::time::Duration;
    let rt = v8_runtime_with_dom(make_doc());
    let script = "data:text/javascript,onmessage%20%3D%20function(e)%7Bthrow%20new%20Error(%27boom2%27)%3B%7D";
    rt.eval(&format!(
        "var w = new Worker('{}'); \
                 var errEvent = null; \
                 w.onerror = function(e){{ errEvent = e; }}; \
                 w.postMessage(1);",
        script
    ))
    .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    rt.pump_workers();
    assert!(bool_eval(&rt, "errEvent !== null"));
    assert!(bool_eval(&rt, "errEvent.message === 'boom2'"));
}

#[test]
fn worker_data_url_base64_script() {
    use std::time::Duration;
    // base64("postMessage('hello');") = "cG9zdE1lc3NhZ2UoJ2hlbGxvJyk7"
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var w = new Worker('data:text/javascript;base64,cG9zdE1lc3NhZ2UoJ2hlbGxvJyk7'); \
                 var got = null; \
                 w.onmessage = function(e){ got = e.data; };",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    rt.pump_workers();
    let result = rt.eval("got").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("hello".into()));
}

#[test]
fn worker_blob_url_script() {
    use std::time::Duration;
    let rt = v8_runtime_with_dom(make_doc());
    // Create a blob URL from a JS Blob and use it as the worker script.
    rt.eval(
        "var blob = new Blob([\"onmessage=function(e){postMessage(e.data+1);}\"], \
                  {type:'text/javascript'}); \
                 var url = URL.createObjectURL(blob); \
                 var w = new Worker(url); \
                 var res = null; \
                 w.onmessage = function(e){ res = e.data; }; \
                 w.postMessage(10);",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    rt.pump_workers();
    let result = rt.eval("res").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(11.0));
}

// ── BUG-364: external (http/https) Worker/SharedWorker script URLs ──────

/// Fetch provider stub for BUG-364 tests: returns a fixed status/body for
/// every request, regardless of URL or method.
struct FixedFetch {
    status: u16,
    body: &'static str,
}
impl lumen_core::ext::JsFetchProvider for FixedFetch {
    fn fetch_sync(
        &self,
        url: &str,
        _method: &str,
    ) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        Ok(lumen_core::ext::JsFetchResult {
            status: self.status,
            status_text: "".into(),
            headers: vec![],
            body: self.body.as_bytes().to_vec(),
            url: url.to_string(),
        })
    }
}

/// Mock provider standing in for an HTTP redirect: `fetch_with_redirect`
/// (`lumen-network`) already follows redirects and reports the final URL in
/// `JsFetchResult::url` — this double mimics that by answering with a URL
/// different from the one it was asked for.
struct RedirectingScriptFetch {
    body: &'static str,
    final_url: &'static str,
}
impl lumen_core::ext::JsFetchProvider for RedirectingScriptFetch {
    fn fetch_sync(
        &self,
        _url: &str,
        _method: &str,
    ) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        Ok(lumen_core::ext::JsFetchResult {
            status: 200,
            status_text: "".into(),
            headers: vec![],
            body: self.body.as_bytes().to_vec(),
            url: self.final_url.to_string(),
        })
    }
}

fn v8_runtime_with_dom_and_fetch(
    doc: Arc<Mutex<Document>>,
    provider: Arc<dyn lumen_core::ext::JsFetchProvider>,
) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(
        doc,
        "https://example.com/page.html",
        Some(provider),
        None, None, None, None, None, None, None, None, false)
    .unwrap();
    rt
}

// ── BUG-571: external `<script src>` inserted by page script ───────────

/// The chunk-loader shape every bundler emits: create a script, set
/// `src`, hook `onload`, append. The body must be fetched, executed in
/// global scope, and `load` fired — one task after the insertion.
#[test]
fn dynamic_external_script_executes_and_fires_load() {
    let provider = Arc::new(FixedFetch { status: 200, body: "globalThis.__b571_ext = 7;" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b571_ext_load = false;
                   var s = document.createElement('script');
                   s.src = 'chunk.js';
                   s.onload = function() { globalThis.__b571_ext_load = true; };
                   document.head.appendChild(s);
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    // PERF-14: fetch() runs on a worker thread; settle it as headless does.
    rt.settle_pending_fetches(std::time::Duration::from_secs(5));
    let r = rt
        .eval("globalThis.__b571_ext === 7 && globalThis.__b571_ext_load === true")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A failed fetch must fire `error` rather than leave the loader's
/// promise pending forever — the exact hang behind BUG-703.
#[test]
fn dynamic_external_script_fires_error_on_http_failure() {
    let provider = Arc::new(FixedFetch { status: 404, body: "nope" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b571_ext_err = false;
                   var s = document.createElement('script');
                   s.src = 'missing.js';
                   s.addEventListener('error', function() { globalThis.__b571_ext_err = true; });
                   document.head.appendChild(s);
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    // PERF-14: fetch() runs on a worker thread; settle it as headless does.
    rt.settle_pending_fetches(std::time::Duration::from_secs(5));
    let r = rt.eval("globalThis.__b571_ext_err === true").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── BUG-878: `<script src>` appended into a shadow root ─────────────────

/// The same chunk-loader shape as `dynamic_external_script_executes_and_fires_load`,
/// but the script is appended to an open shadow root instead of `document.head`.
/// `_lumen_resource_is_connected`'s plain-parent walk used to dead-end at the
/// `ShadowRoot` node (never a DOM child of its host), so the element stayed
/// "pending" forever and the fetch never started.
#[test]
fn dynamic_external_script_in_shadow_root_executes_and_fires_load() {
    let provider = Arc::new(FixedFetch { status: 200, body: "globalThis.__b878_ext = 7;" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b878_ext_load = false;
                   var host = document.createElement('div');
                   document.body.appendChild(host);
                   var root = host.attachShadow({mode: 'open'});
                   var s = document.createElement('script');
                   s.src = 'chunk.js';
                   s.onload = function() { globalThis.__b878_ext_load = true; };
                   root.appendChild(s);
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    // PERF-14: fetch() runs on a worker thread; settle it as headless does.
    rt.settle_pending_fetches(std::time::Duration::from_secs(5));
    let r = rt
        .eval("globalThis.__b878_ext === 7 && globalThis.__b878_ext_load === true")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `isConnected` must also see through the shadow-root boundary — the same
/// `_lumen_get_parent` dead-end this bug's connectivity check hit.
#[test]
fn element_in_connected_shadow_root_is_connected() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        r#"var host = document.createElement('div');
                   document.body.appendChild(host);
                   var root = host.attachShadow({mode: 'open'});
                   var p = document.createElement('p');
                   root.appendChild(p);"#,
    )
    .unwrap();
    assert!(bool_eval(&rt, "p.isConnected === true"));
}

// ── BUG-703: dynamic `<link rel=stylesheet>` load/error ────────────────

/// The shape behind the tbank.ru hang: a block loader appends a
/// stylesheet link and awaits its `load` before rendering. Both the IDL
/// attribute and an addEventListener listener must be reached.
#[test]
fn dynamic_stylesheet_link_fires_load() {
    let provider = Arc::new(FixedFetch { status: 200, body: "#a{color:red}" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b703_load = 0;
                   var l = document.createElement('link');
                   l.rel = 'stylesheet';
                   l.href = 'block.css';
                   l.onload = function() { globalThis.__b703_load++; };
                   l.addEventListener('load', function() { globalThis.__b703_load++; });
                   document.head.appendChild(l);
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    // PERF-14: fetch() runs on a worker thread; settle it as headless does.
    rt.settle_pending_fetches(std::time::Duration::from_secs(5));
    let r = rt.eval("globalThis.__b703_load").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(2.0));
}

/// A failed sheet must fire `error`, not hang — the loader in the field
/// races `onload` against `onerror` and settles on whichever arrives.
#[test]
fn dynamic_stylesheet_link_fires_error_on_http_failure() {
    let provider = Arc::new(FixedFetch { status: 404, body: "nope" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b703_err = false;
                   var l = document.createElement('link');
                   l.rel = 'stylesheet';
                   l.href = 'missing.css';
                   l.onerror = function() { globalThis.__b703_err = true; };
                   document.head.appendChild(l);
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    // PERF-14: fetch() runs on a worker thread; settle it as headless does.
    rt.settle_pending_fetches(std::time::Duration::from_secs(5));
    let r = rt.eval("globalThis.__b703_err === true").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── BUG-826: <link rel=preload|modulepreload|prefetch> ────────────────
//
// The predecessor of this block asserted the *absence* of the feature
// («a rel=preload link must not be fetched behind the page's back — no
// event either way»), which is what BUG-826 turned out to be: the hint
// reached a stderr logger in the shell and nothing else.

/// A `rel=preload` with a valid `as` fetches and reports `load`.
#[test]
fn dynamic_preload_link_fetches_and_fires_load() {
    let provider = Arc::new(FixedFetch { status: 200, body: "x" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b826_load = 0;
                   globalThis.__b826_err = 0;
                   var l = document.createElement('link');
                   l.rel = 'preload';
                   l.as = 'font';
                   l.href = 'thing.woff2';
                   l.onload = function() { globalThis.__b826_load++; };
                   l.onerror = function() { globalThis.__b826_err++; };
                   document.head.appendChild(l);
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    let r = rt.eval("globalThis.__b826_load === 1 && globalThis.__b826_err === 0").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// An HTTP failure is reported as `error` — the `*_deny` half of the
/// WPT families used to hang exactly like the `*_allow` half.
#[test]
fn preload_link_fires_error_on_http_failure() {
    let provider = Arc::new(FixedFetch { status: 404, body: "nope" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b826_e = false;
                   var l = document.createElement('link');
                   l.rel = 'preload';
                   l.as = 'script';
                   l.href = 'missing.js';
                   l.onerror = function() { globalThis.__b826_e = true; };
                   document.head.appendChild(l);
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    let r = rt.eval("globalThis.__b826_e === true").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `as` in no state — absent, or a keyword that is not a destination —
/// obtains no resource, and the element then reports *nothing*: WPT's
/// `preload/onload-event.html` asserts both the missing-`as` and the
/// `as=foobarxmlthing` link stay silent.
#[test]
fn preload_link_without_valid_as_fires_nothing() {
    let provider = Arc::new(FixedFetch { status: 200, body: "x" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b826_any = 0;
                   function mk(as) {
                       var l = document.createElement('link');
                       l.rel = 'preload';
                       if (as !== null) l.as = as;
                       l.href = 'thing.bin?' + as;
                       l.onload = function() { globalThis.__b826_any++; };
                       l.onerror = function() { globalThis.__b826_any++; };
                       document.head.appendChild(l);
                   }
                   mk(null);
                   mk('foobarxmlthing');
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    let r = rt.eval("globalThis.__b826_any === 0").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A `type` the destination cannot consume is the same silent refusal.
#[test]
fn preload_link_with_wrong_type_fires_nothing() {
    let provider = Arc::new(FixedFetch { status: 200, body: "x" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b826_t = 0;
                   var l = document.createElement('link');
                   l.rel = 'preload';
                   l.as = 'style';
                   l.type = 'text/html';
                   l.href = 'sheet.css';
                   l.onload = function() { globalThis.__b826_t++; };
                   l.onerror = function() { globalThis.__b826_t++; };
                   document.head.appendChild(l);
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    let r = rt.eval("globalThis.__b826_t === 0").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `rel=prefetch` is destination-agnostic: no `as` is needed and the
/// element still reports `load`.
#[test]
fn dynamic_prefetch_link_fires_load() {
    let provider = Arc::new(FixedFetch { status: 200, body: "x" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b826_pf = false;
                   var l = document.createElement('link');
                   l.rel = 'prefetch';
                   l.href = 'next-page.html';
                   l.onload = function() { globalThis.__b826_pf = true; };
                   document.head.appendChild(l);
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    let r = rt.eval("globalThis.__b826_pf === true").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// BUG-848: `rel=icon` is not a resource hint in §4.6.7's taxonomy but
/// fetches and reports through the same shape, and both spellings of it
/// must — `rel="shortcut icon"` is the historic form still in the wild,
/// and the token split hands the dispatcher the plain `icon` from it.
#[test]
fn icon_link_fetches_and_fires_load_in_both_spellings() {
    let provider = Arc::new(FixedFetch { status: 200, body: "GIF89a" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b848_icon = 0;
                   function mkIcon(rel, href) {
                       var l = document.createElement('link');
                       l.rel = rel;
                       l.href = href;
                       l.onload = function() { globalThis.__b848_icon++; };
                       document.head.appendChild(l);
                   }
                   mkIcon('icon', 'favicon.ico');
                   mkIcon('shortcut icon', 'legacy.ico');
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    let r = rt.eval("globalThis.__b848_icon").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(2.0));
}

/// An icon the server does not have reports `error`, the same half of
/// the pair every other hint type already had.
#[test]
fn icon_link_fires_error_on_http_failure() {
    let provider = Arc::new(FixedFetch { status: 404, body: "nope" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b848_ie = false;
                   var l = document.createElement('link');
                   l.rel = 'icon';
                   l.href = 'missing.ico';
                   l.onerror = function() { globalThis.__b848_ie = true; };
                   document.head.appendChild(l);
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    let r = rt.eval("globalThis.__b848_ie === true").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `rel=modulepreload` with no `as` defaults to the «script»
/// destination and loads; a valid but non-script-like destination is
/// the one case that fires `error` instead of staying silent.
#[test]
fn modulepreload_link_load_and_bad_destination_error() {
    let provider = Arc::new(FixedFetch { status: 200, body: "export const a = 1;" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b826_m = '';
                   var ok = document.createElement('link');
                   ok.rel = 'modulepreload';
                   ok.href = 'mod.js';
                   ok.onload = function() { globalThis.__b826_m += 'L'; };
                   ok.onerror = function() { globalThis.__b826_m += 'E'; };
                   document.head.appendChild(ok);
                   var bad = document.createElement('link');
                   bad.rel = 'modulepreload';
                   bad.as = 'image';
                   bad.href = 'mod2.js';
                   bad.onload = function() { globalThis.__b826_m += 'l'; };
                   bad.onerror = function() { globalThis.__b826_m += 'e'; };
                   document.head.appendChild(bad);
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    // Order is not asserted: the two paths settle through different
    // queues (a plain task for the refusal, a fetch promise for the
    // load), and the spec fixes neither against the other.
    let r = rt
        .eval(
            "globalThis.__b826_m.length === 2                      && globalThis.__b826_m.indexOf('L') >= 0                      && globalThis.__b826_m.indexOf('e') >= 0",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Build a document whose `<head>` already holds a `<link rel=preload>`,
/// the way the HTML parser leaves it — no `createElement`, so the
/// insertion hook never sees the element.
fn make_doc_with_parser_preload_link() -> Arc<Mutex<Document>> {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let head = doc.create_element(QualName::html("head"));
    let body = doc.create_element(QualName::html("body"));
    let link = doc.create_element(QualName::html("link"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(link).data {
        for (name, value) in [
            ("rel", "preload"),
            ("as", "script"),
            ("href", "parsed.js"),
            ("onload", "globalThis.__b826_p++"),
        ] {
            attrs.push(lumen_dom::Attribute {
                name: QualName::html(name),
                value: value.into(),
            });
        }
    }
    doc.append_child(doc.root(), html);
    doc.append_child(html, head);
    doc.append_child(head, link);
    doc.append_child(html, body);
    Arc::new(Mutex::new(doc))
}

/// A hint the *parser* wrote never passes through the insertion hook,
/// so it is picked up by the document pass at «interactive» — and the
/// inline `onload=` content attribute must be the one that fires.
#[test]
fn parser_written_preload_link_fetches_at_interactive() {
    let provider = Arc::new(FixedFetch { status: 200, body: "x" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc_with_parser_preload_link(), provider);
    rt.eval("globalThis.__b826_p = 0; _lumen_tick_timers();").unwrap();
    // Nothing has run yet: the element was never inserted through the
    // DOM API, so only the document pass can reach it.
    let before = rt.eval("globalThis.__b826_p").unwrap();
    assert_eq!(before, lumen_core::JsValue::Number(0.0));
    rt.eval("_lumen_apply_ready_state('interactive'); _lumen_tick_timers();").unwrap();
    let after = rt.eval("globalThis.__b826_p").unwrap();
    assert_eq!(after, lumen_core::JsValue::Number(1.0));
}

/// The two paths overlap for a link a head script appends before the
/// document pass runs; the per-node guard keeps it at one fetch.
#[test]
fn preload_link_is_not_fetched_twice_by_the_document_pass() {
    let provider = Arc::new(FixedFetch { status: 200, body: "x" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b826_n = 0;
                   var l = document.createElement('link');
                   l.rel = 'preload';
                   l.as = 'script';
                   l.href = 'once.js';
                   l.onload = function() { globalThis.__b826_n++; };
                   document.head.appendChild(l);
                   _lumen_tick_timers();
                   _lumen_apply_ready_state('interactive');
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    let r = rt.eval("globalThis.__b826_n").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
}

/// `link.relList.supports('preload')` is the first line of WPT's own
/// preload helper — without it every test in the family threw before
/// reaching its subject.
#[test]
fn link_rel_list_supports_and_reflects() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        r#"globalThis.__b826_rl = (function() {
                       var l = document.createElement('link');
                       l.rel = 'preload';
                       var ok = l.relList.supports('preload')
                             && l.relList.supports('modulepreload')
                             && !l.relList.supports('nonsense')
                             && l.relList.contains('preload')
                             && l.relList === l.relList;
                       l.relList.add('prefetch');
                       return ok && l.rel === 'preload prefetch';
                   })();"#,
    )
    .unwrap();
    let r = rt.eval("globalThis.__b826_rl === true").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A link built detached and only later connected still loads, and a
/// link that never enters the document must not fetch at all.
#[test]
fn detached_stylesheet_link_loads_only_once_connected() {
    let provider = Arc::new(FixedFetch { status: 200, body: "#a{color:red}" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        r#"globalThis.__b703_d = 0;
                   var d = document.createElement('div');
                   var l = document.createElement('link');
                   l.rel = 'stylesheet';
                   l.href = 'block.css';
                   l.onload = function() { globalThis.__b703_d++; };
                   d.appendChild(l);
                   _lumen_tick_timers();"#,
    )
    .unwrap();
    // PERF-14: fetch() runs on a worker thread; settle it as headless does.
    rt.settle_pending_fetches(std::time::Duration::from_secs(5));
    let before = rt.eval("globalThis.__b703_d").unwrap();
    assert_eq!(before, lumen_core::JsValue::Number(0.0));
    rt.eval("document.body.appendChild(d); _lumen_tick_timers();").unwrap();
    // PERF-14: fetch() runs on a worker thread; settle it as headless does.
    rt.settle_pending_fetches(std::time::Duration::from_secs(5));
    let after = rt.eval("globalThis.__b703_d").unwrap();
    assert_eq!(after, lumen_core::JsValue::Number(1.0));
}

#[test]
fn worker_external_url_fetches_and_runs_script() {
    use std::time::Duration;
    let provider = Arc::new(FixedFetch { status: 200, body: "postMessage('remote');" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    // Relative URL — must resolve against the page URL before fetching.
    rt.eval(
        "var w = new Worker('worker.js'); \
                 var got = null; \
                 w.onmessage = function(e){ got = e.data; };",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    rt.pump_workers();
    let result = rt.eval("got").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("remote".into()));
}

/// BUG-984: a classic `Worker`'s own `location` must report the final URL
/// after redirects, not the constructor URL the script was fetched from.
#[test]
fn worker_external_url_redirect_updates_its_own_location() {
    use std::time::Duration;
    let provider = Arc::new(RedirectingScriptFetch {
        body: "postMessage(location.href);",
        final_url: "https://example.com/final.js",
    });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        "var w = new Worker('https://example.com/start.js'); \
                 var got = null; \
                 w.onmessage = function(e){ got = e.data; };",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    rt.pump_workers();
    let result = rt.eval("got").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("https://example.com/final.js".into()));
}

#[test]
fn worker_external_url_fetch_failure_fires_onerror() {
    let provider = Arc::new(FixedFetch { status: 404, body: "not found" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        "var w = new Worker('https://example.com/missing.js'); \
                 var errEvent = null; \
                 w.onerror = function(e){ errEvent = e; }; \
                 w.postMessage('ignored'); \
                 w.terminate();",
    )
    .unwrap();
    // `error` is queued via setTimeout(fn, 0) — drive the timer wheel.
    rt.eval("_lumen_tick_timers()").unwrap();
    assert!(bool_eval(&rt, "errEvent !== null"));
    assert!(bool_eval(&rt, "errEvent.type === 'error'"));
    assert!(bool_eval(
        &rt,
        "errEvent.message.indexOf('https://example.com/missing.js') !== -1"
    ));
}

#[test]
fn shared_worker_external_url_connects_and_echoes() {
    use std::time::Duration;
    let provider = Arc::new(FixedFetch {
        status: 200,
        body: "onconnect = function(e){ e.ports[0].onmessage = function(ev){ e.ports[0].postMessage(ev.data + 1); }; };",
    });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        "var sw = new SharedWorker('https://example.com/sw.js'); \
                 var got = null; \
                 sw.port.onmessage = function(e){ got = e.data; }; \
                 sw.port.postMessage(41);",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    rt.pump_shared_workers();
    let result = rt.eval("got").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(42.0));
}

/// BUG-984: the exact case measured in the bug report — a `SharedWorker`'s
/// own `location` must report the final URL after redirects, not the
/// constructor URL the script was fetched from.
#[test]
fn shared_worker_external_url_redirect_updates_its_own_location() {
    use std::time::Duration;
    let provider = Arc::new(RedirectingScriptFetch {
        body: "onconnect = function(e){ e.ports[0].postMessage(location.href); };",
        final_url: "https://example.com/final-sw.js",
    });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        "var sw = new SharedWorker('https://example.com/start-sw.js'); \
                 var got = null; \
                 sw.port.onmessage = function(e){ got = e.data; };",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    rt.pump_shared_workers();
    let result = rt.eval("got").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("https://example.com/final-sw.js".into()));
}

#[test]
fn shared_worker_external_url_fetch_failure_fires_onerror() {
    let provider = Arc::new(FixedFetch { status: 500, body: "server error" });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        "var sw = new SharedWorker('https://example.com/missing-sw.js'); \
                 var errEvent = null; \
                 sw.onerror = function(e){ errEvent = e; }; \
                 sw.port.postMessage('ignored');",
    )
    .unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    assert!(bool_eval(&rt, "errEvent !== null"));
    assert!(bool_eval(
        &rt,
        "errEvent.message.indexOf('https://example.com/missing-sw.js') !== -1"
    ));
}

// ── BUG-905: SharedWorker runtime errors stop at the worker's own scope ──
// Unlike a dedicated Worker (one owning client), HTML LS §10.2.6 gives a
// shared worker no single owner to forward a *runtime* exception to — it
// fires `error` at the worker's own global scope and stops there, regardless
// of how many clients are connected or which one triggered it. This used to
// broadcast to every connected client instead (the original, incorrect,
// BUG-591 shape) — see `shared_worker.rs`'s `broadcast_shared_worker_error`,
// now reserved for a top-level parse/load failure only.

#[test]
fn shared_worker_onconnect_exception_does_not_reach_client_onerror() {
    use std::time::Duration;
    let provider = Arc::new(FixedFetch {
        status: 200,
        body: "onconnect = function(e) { throw new Error('boom-connect'); };",
    });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        "var sw = new SharedWorker('https://example.com/sw-onconnect.js'); \
                 var errEvent = null; \
                 sw.onerror = function(e){ errEvent = e; };",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    rt.pump_shared_workers();
    assert!(bool_eval(&rt, "errEvent === null"));
}

#[test]
fn shared_worker_port_onmessage_exception_does_not_reach_any_client() {
    use std::time::Duration;
    let provider = Arc::new(FixedFetch {
        status: 200,
        body: "onconnect = function(e) { var p = e.ports[0]; \
                       p.onmessage = function(ev) { throw new Error('boom-message'); }; };",
    });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        "var swA = new SharedWorker('https://example.com/sw-broadcast.js', 'bcast'); \
                 var swB = new SharedWorker('https://example.com/sw-broadcast.js', 'bcast'); \
                 var errA = null, errB = null; \
                 swA.onerror = function(e){ errA = e; }; \
                 swB.onerror = function(e){ errB = e; }; \
                 swA.port.postMessage('x');",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    rt.pump_shared_workers();
    assert!(bool_eval(&rt, "errA === null"), "client A (sender) must not see a runtime error");
    assert!(bool_eval(&rt, "errB === null"), "client B (bystander) must not see it either");
}

#[test]
fn shared_worker_error_addeventlistener_does_not_fire_for_runtime_error() {
    use std::time::Duration;
    let provider = Arc::new(FixedFetch {
        status: 200,
        body: "onconnect = function(e) { throw new Error('boom-listener'); };",
    });
    let rt = v8_runtime_with_dom_and_fetch(make_doc(), provider);
    rt.eval(
        "var sw = new SharedWorker('https://example.com/sw-listener.js'); \
                 var gotViaListener = null; \
                 sw.addEventListener('error', function(e){ gotViaListener = e; });",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    rt.pump_shared_workers();
    assert!(bool_eval(&rt, "gotViaListener === null"));
}

// ── GAP-CSPENF срез 13: worker-src против new Worker()/new SharedWorker() ──

/// Mock provider whose `check_worker_src` always refuses with
/// `Error::CspWorkerSrcBlocked`, the way `HttpClient::check_worker_src`
/// (срез 13) does when the document's `worker-src` forbids the worker's
/// script URL — proves `_lumen_worker_fetch_script`/`_lumen_sw_fetch_script`
/// run the check BEFORE ever reaching `fetch_sync` (this provider errors
/// instead of panicking so a regression that skips the check shows up as a
/// wrong message/empty side channel rather than a test-harness abort).
struct CspBlockedWorkerProvider;
impl lumen_core::ext::JsFetchProvider for CspBlockedWorkerProvider {
    fn fetch_sync(&self, _url: &str, _method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        Err(lumen_core::error::Error::Network("fetch_sync must not be reached — check_worker_src should short-circuit first".into()))
    }
    fn check_worker_src(&self, _url: &str) -> lumen_core::error::Result<()> {
        Err(lumen_core::error::Error::CspWorkerSrcBlocked {
            blocked_uri: "https://blocked.example/worker.js".into(),
            original_policy: "worker-src 'none'".into(),
        })
    }
}

fn v8_runtime_with_csp_blocked_worker(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let p: Arc<dyn lumen_core::ext::JsFetchProvider> = Arc::new(CspBlockedWorkerProvider);
    rt.install_dom(doc, "", Some(p), None, None, None, None, None, None, None, None, false).unwrap();
    rt
}

#[test]
fn worker_src_block_reaches_native_side_channel() {
    let rt = v8_runtime_with_csp_blocked_worker(make_doc());
    let r = rt
        .eval("_lumen_worker_fetch_script('https://blocked.example/worker.js'); _lumen_worker_last_csp_block()")
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr.len(), 2);
            assert_eq!(arr[0], lumen_core::JsValue::String("https://blocked.example/worker.js".into()));
            assert_eq!(arr[1], lumen_core::JsValue::String("worker-src 'none'".into()));
        }
        other => panic!("expected [uri, policy], got {other:?}"),
    }
}

#[test]
fn worker_src_block_never_starts_worker_and_fires_onerror() {
    let rt = v8_runtime_with_csp_blocked_worker(make_doc());
    rt.eval(
        "var w = new Worker('https://blocked.example/worker.js'); \
                 var errEvent = null; \
                 w.onerror = function(e){ errEvent = e; };",
    )
    .unwrap();
    // `_id` stays null — same "worker never started" state a network failure
    // leaves it in (HTML LS §10.2.6.1).
    assert!(bool_eval(&rt, "w._id === null"));
    rt.eval("_lumen_tick_timers()").unwrap();
    assert!(bool_eval(&rt, "errEvent !== null"));
    assert!(bool_eval(&rt, "errEvent.type === 'error'"));
    assert!(bool_eval(
        &rt,
        "errEvent.message.indexOf('https://blocked.example/worker.js') !== -1"
    ));
}

#[test]
fn worker_src_block_fires_security_policy_violation_event() {
    let rt = v8_runtime_with_csp_blocked_worker(make_doc());
    rt.eval(
        "var seen = null; \
         document.addEventListener('securitypolicyviolation', function(e) { \
             seen = [e.violatedDirective, e.blockedURI, e.originalPolicy].join('|'); \
         }); \
         var w = new Worker('https://blocked.example/worker.js'); \
         _lumen_tick_timers();",
    )
    .unwrap();
    assert_eq!(
        rt.eval("seen").unwrap(),
        lumen_core::JsValue::String(
            "worker-src|https://blocked.example/worker.js|worker-src 'none'".into()
        )
    );
}

#[test]
fn shared_worker_src_block_reaches_native_side_channel() {
    let rt = v8_runtime_with_csp_blocked_worker(make_doc());
    let r = rt
        .eval("_lumen_sw_fetch_script('https://blocked.example/worker.js'); _lumen_sw_last_csp_block()")
        .unwrap();
    match r {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr.len(), 2);
            assert_eq!(arr[0], lumen_core::JsValue::String("https://blocked.example/worker.js".into()));
            assert_eq!(arr[1], lumen_core::JsValue::String("worker-src 'none'".into()));
        }
        other => panic!("expected [uri, policy], got {other:?}"),
    }
}

#[test]
fn shared_worker_src_block_fires_onerror() {
    let rt = v8_runtime_with_csp_blocked_worker(make_doc());
    rt.eval(
        "var sw = new SharedWorker('https://blocked.example/worker.js'); \
                 var errEvent = null; \
                 sw.onerror = function(e){ errEvent = e; };",
    )
    .unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    assert!(bool_eval(&rt, "errEvent !== null"));
    assert!(bool_eval(
        &rt,
        "errEvent.message.indexOf('https://blocked.example/worker.js') !== -1"
    ));
}

#[test]
fn shared_worker_src_block_fires_security_policy_violation_event() {
    let rt = v8_runtime_with_csp_blocked_worker(make_doc());
    rt.eval(
        "var seen = null; \
         document.addEventListener('securitypolicyviolation', function(e) { \
             seen = [e.violatedDirective, e.blockedURI, e.originalPolicy].join('|'); \
         }); \
         var sw = new SharedWorker('https://blocked.example/worker.js'); \
         _lumen_tick_timers();",
    )
    .unwrap();
    assert_eq!(
        rt.eval("seen").unwrap(),
        lumen_core::JsValue::String(
            "worker-src|https://blocked.example/worker.js|worker-src 'none'".into()
        )
    );
}
