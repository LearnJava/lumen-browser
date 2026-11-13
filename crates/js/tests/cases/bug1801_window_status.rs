//! BUG-1801 — `window.status` (HTML LS, the window's status text) is missing
//! from the global object shim: a page script reading the bare `status`
//! identifier (ebay.com) gets `Uncaught ReferenceError: status is not
//! defined` instead of the string attribute every real browser defines.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn make_rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(
        doc,
        "https://example.com/doc",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        None,
    )
    .unwrap();
    rt
}

fn str_eval(rt: &V8JsRuntime, script: &str) -> String {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::String(s)) => s,
        Ok(other) => panic!("expected a string from script, got {other:?}: {script}"),
        Err(e) => panic!("eval error: {e} for {script}"),
    }
}

/// The core of BUG-1801: the bare `status` identifier resolves on the global
/// (no ReferenceError) to a string, initially empty — and it is `window`'s
/// own property, as `Object.defineProperty`-shaped WebIDL attributes are.
#[test]
fn bare_status_resolves_to_an_initially_empty_string() {
    let rt = make_rt();
    assert_eq!(str_eval(&rt, "typeof status"), "string");
    assert_eq!(str_eval(&rt, "status"), "");
    assert_eq!(
        str_eval(&rt, "String(Object.getOwnPropertyNames(window).includes('status'))"),
        "true"
    );
}

/// Setter stores the value, getter reads it back, through both the bare
/// identifier and `window.status` (one accessor on one object).
#[test]
fn assignment_round_trips_through_the_bare_identifier_and_window() {
    let rt = make_rt();
    assert_eq!(str_eval(&rt, "status = 'done'; status"), "done");
    assert_eq!(str_eval(&rt, "window.status"), "done");
    assert_eq!(str_eval(&rt, "window.status = 'x'; status"), "x");
}

/// `status` is a DOMString attribute: values coerce with ToString, not by
/// keeping the raw type (WebIDL §3.6.9).
#[test]
fn assignment_coerces_the_value_to_a_string() {
    let rt = make_rt();
    assert_eq!(str_eval(&rt, "status = 123; status"), "123");
    assert_eq!(str_eval(&rt, "status = null; status"), "null");
}
