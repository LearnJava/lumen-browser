//! BUG-568: `document.write()` — where the written text lands and which
//! written `<script>`s run. The shell parses the whole page before any script
//! runs and executes each parser script between `_lumen_push_current_script`
//! and `_lumen_pop_current_script` (`crates/shell/src/scripts.rs`); the
//! `run_parser_script` helper below does exactly that.

use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_core::JsValue;

/// Serves a fixed script body per URL and counts requests.
struct ScriptServer {
    bodies: Vec<(&'static str, &'static str)>,
    fetches: std::sync::atomic::AtomicUsize,
    /// `Some(n)` — behave as `script-src 'nonce-<n>'` for inline text.
    inline_nonce: Option<&'static str>,
}
impl lumen_core::ext::JsFetchProvider for ScriptServer {
    fn fetch_sync(&self, url: &str, _method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        self.fetches.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let body = self.bodies.iter().find(|(u, _)| *u == url).map(|(_, b)| *b);
        match body {
            Some(b) => Ok(lumen_core::ext::JsFetchResult {
                status: 200,
                status_text: "OK".into(),
                headers: vec![],
                body: b.as_bytes().to_vec(),
                url: url.to_string(),
            }),
            None => Ok(lumen_core::ext::JsFetchResult {
                status: 404,
                status_text: "Not Found".into(),
                headers: vec![],
                body: Vec::new(),
                url: url.to_string(),
            }),
        }
    }
    fn fetch_with_body_sync(&self, _url: &str, _method: &str, _content_type: &str, _body: &[u8]) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
        Err(lumen_core::error::Error::Network("no body expected".into()))
    }
    fn check_inline_script(&self, nonce: &str, _body: &str) -> lumen_core::error::Result<()> {
        match self.inline_nonce {
            None => return Ok(()),
            Some(n) if n == nonce => return Ok(()),
            Some(_) => {}
        }
        Err(lumen_core::error::Error::CspElementSrcBlocked {
            directive: "script-src-elem".into(),
            blocked_uri: "inline".into(),
            original_policy: "script-src 'nonce-ok'".into(),
        })
    }
}

fn setup(html: &str, bodies: Vec<(&'static str, &'static str)>) -> (V8JsRuntime, Arc<ScriptServer>) {
    setup_with_policy(html, bodies, None)
}

fn setup_with_policy(
    html: &str,
    bodies: Vec<(&'static str, &'static str)>,
    inline_nonce: Option<&'static str>,
) -> (V8JsRuntime, Arc<ScriptServer>) {
    let server = Arc::new(ScriptServer { bodies, fetches: std::sync::atomic::AtomicUsize::new(0), inline_nonce });
    let doc = Arc::new(Mutex::new(lumen_html_parser::parse(html)));
    let rt = V8JsRuntime::new().unwrap();
    let p: Arc<dyn lumen_core::ext::JsFetchProvider> = server.clone();
    rt.install_dom(doc, "https://example.com/", Some(p), None, None, None, None, None, None, None, None, false)
        .unwrap();
    (rt, server)
}

/// Run the markup's `<script id=…>` the way the shell's page-load loop does.
fn run_parser_script(rt: &V8JsRuntime, id: &str) {
    rt.eval(&format!(
        "(function() {{ var s = document.getElementById('{id}'); \
           _lumen_push_current_script(s.__nid__); \
           try {{ (0, eval)(s.textContent); }} finally {{ _lumen_pop_current_script(); }} }})()"
    ))
    .unwrap();
}

fn eval_str(rt: &V8JsRuntime, expr: &str) -> String {
    match rt.eval(expr).unwrap() {
        JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

/// Child element ids of `<body>` in order (`#` for an element without one).
const BODY_IDS: &str = "Array.prototype.map.call(document.body.children, function(e) { \
                        return e.id || ('#' + e.localName); }).join(',')";

/// The text lands right after the writing script, not at the end of `<body>`.
#[test]
fn write_lands_after_the_running_script() {
    let (rt, _) = setup(
        "<body><p id=a></p><script id=s>document.write('<b id=w></b>')</script><p id=z></p></body>",
        vec![],
    );
    run_parser_script(&rt, "s");
    assert_eq!(eval_str(&rt, BODY_IDS), "a,s,w,z");
}

/// Several writes from one script keep their order, and a tag split over two
/// calls is parsed once it is complete.
#[test]
fn split_tag_across_writes_is_held_back() {
    let (rt, _) = setup(
        "<body><script id=s>document.write('<i id=');document.write(\"'x'>1</i>\");\
         document.write('<u id=y></u>')</script><p id=z></p></body>",
        vec![],
    );
    run_parser_script(&rt, "s");
    assert_eq!(eval_str(&rt, BODY_IDS), "s,x,y,z");
    assert_eq!(eval_str(&rt, "document.getElementById('x').textContent"), "1");
}

/// A written inline script runs inside the `write()` call, with
/// `document.currentScript` set to itself; its own write lands after it.
#[test]
fn written_inline_script_runs_synchronously() {
    let (rt, _) = setup(
        "<body><script id=s>globalThis.log = [];\
         document.write('<script id=w>log.push(document.currentScript.id);\
         document.write(\"<b id=ww></b>\")<\\/script>');\
         log.push('after:' + document.currentScript.id)</script><p id=z></p></body>",
        vec![],
    );
    run_parser_script(&rt, "s");
    assert_eq!(eval_str(&rt, "log.join(',')"), "w,after:s");
    assert_eq!(eval_str(&rt, BODY_IDS), "s,w,ww,z");
}

/// A written `<script src>` blocks the parser: it runs once the writing
/// script returns, and an inline script written after it waits for it.
#[test]
fn written_external_script_blocks_later_written_scripts() {
    let (rt, server) = setup(
        "<body><script id=s>globalThis.log = [];\
         document.write('<script id=e src=\"/ext.js\"><\\/script>');\
         document.write('<script id=i>log.push(\"inline\")<\\/script>');\
         log.push('writer-end')</script><p id=z></p></body>",
        vec![("https://example.com/ext.js", "log.push('ext:' + document.currentScript.id)")],
    );
    run_parser_script(&rt, "s");
    assert_eq!(eval_str(&rt, "log.join(',')"), "writer-end,ext:e,inline");
    assert_eq!(server.fetches.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(eval_str(&rt, BODY_IDS), "s,e,i,z");
}

/// A written external script fires `load`; a 404 fires `error` and does not
/// stall what follows it.
#[test]
fn written_external_script_load_and_error_events() {
    let (rt, _) = setup(
        "<body><script id=s>globalThis.log = [];\
         document.write('<script src=\"/ok.js\" onload=\"log.push(\\'load\\')\"><\\/script>');\
         document.write('<script src=\"/missing.js\" onerror=\"log.push(\\'error\\')\"><\\/script>');\
         document.write('<script>log.push(\"tail\")<\\/script>')</script></body>",
        vec![("https://example.com/ok.js", "log.push('ok')")],
    );
    run_parser_script(&rt, "s");
    rt.eval("_lumen_tick_timers(); _lumen_tick_timers();").unwrap();
    assert_eq!(eval_str(&rt, "log.join(',')"), "ok,load,error,tail");
}

/// A write from a `<head>` script keeps a script in `<head>` and moves the
/// first body-only element (and everything after it) to the start of `<body>`.
#[test]
fn write_from_head_splits_head_and_body_content() {
    let (rt, _) = setup(
        "<html><head><script id=s>document.write('<meta id=m><div id=d></div><span id=t></span>')</script>\
         </head><body><p id=z></p></body></html>",
        vec![],
    );
    run_parser_script(&rt, "s");
    assert_eq!(eval_str(&rt, "document.getElementById('m').parentNode.localName"), "head");
    assert_eq!(eval_str(&rt, BODY_IDS), "d,t,z");
}

/// `defer` on a written external script moves it to the end of parsing.
#[test]
fn written_defer_script_runs_at_interactive() {
    let (rt, _) = setup(
        "<body><script id=s>globalThis.log = [];\
         document.addEventListener('DOMContentLoaded', function() { log.push('dcl'); });\
         document.write('<script defer src=\"/d.js\"><\\/script>');\
         log.push('writer-end')</script></body>",
        vec![("https://example.com/d.js", "log.push('deferred')")],
    );
    run_parser_script(&rt, "s");
    assert_eq!(eval_str(&rt, "log.join(',')"), "writer-end");
    rt.eval("_lumen_apply_ready_state('interactive')").unwrap();
    let log = eval_str(&rt, "log.join(',')");
    assert!(log.starts_with("writer-end,deferred"), "{log}");
}

/// `script-src` judges a written inline script: without the nonce it does not
/// run and a `securitypolicyviolation` names `inline`; with it, it runs.
#[test]
fn written_inline_script_is_checked_against_script_src() {
    let (rt, _) = setup_with_policy(
        "<body><script id=s>globalThis.log = [];\
         document.addEventListener('securitypolicyviolation', function(e) { log.push('csp:' + e.blockedURI); });\
         document.write('<script>log.push(\"bad\")<\\/script>');\
         document.write('<script nonce=ok>log.push(\"good\")<\\/script>')</script></body>",
        vec![],
        Some("ok"),
    );
    run_parser_script(&rt, "s");
    rt.eval("_lumen_tick_timers();").unwrap();
    let log = eval_str(&rt, "log.join(',')");
    assert!(!log.contains("bad"), "{log}");
    assert!(log.contains("csp:inline"), "{log}");
    assert!(log.contains("good"), "{log}");
}

/// Outside any script (a timer, an event handler) the text still goes to the
/// end of `<body>`; after the document has loaded `write()` does nothing.
#[test]
fn write_without_a_script_appends_and_after_load_is_ignored() {
    let (rt, _) = setup("<body><p id=a></p></body>", vec![]);
    rt.eval("document.write('<b id=w></b>')").unwrap();
    assert_eq!(eval_str(&rt, BODY_IDS), "a,w");
    rt.eval("_lumen_apply_ready_state('interactive'); _lumen_apply_ready_state('complete'); \
             document.write('<b id=late></b>')")
        .unwrap();
    assert_eq!(eval_str(&rt, BODY_IDS), "a,w");
}
