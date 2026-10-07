//! BUG-949 — `scrollY` right after `scrollTo()` must reflect the request
//! (CSSOM View), not the position the shell last applied.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/doc", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn num(rt: &V8JsRuntime, script: &str) -> f64 {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Number(n)) => n,
        other => panic!("expected number, got {other:?}: {script}"),
    }
}

#[test]
fn scroll_to_is_visible_to_immediate_read() {
    let rt = rt();
    assert_eq!(num(&rt, "window.scrollTo(0, 150); window.scrollY"), 150.0);
    assert_eq!(num(&rt, "window.scrollBy(0, 50); window.pageYOffset"), 200.0);
}

#[test]
fn smooth_scroll_keeps_committed_value() {
    let rt = rt();
    rt.set_page_scroll_y(10.0);
    assert_eq!(num(&rt, "window.scrollTo({top: 300, behavior: 'smooth'}); window.scrollY"), 10.0);
}

#[test]
fn drained_queue_falls_back_to_committed_position() {
    let rt = rt();
    rt.eval("window.scrollTo(0, 150)").unwrap();
    rt.take_page_scroll_requests();
    rt.set_page_scroll_y(120.0);
    assert_eq!(num(&rt, "window.scrollY"), 120.0);
}
