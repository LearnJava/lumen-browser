//! BUG-1121 — `document.referrer` was absent from the live `document`
//! (`typeof` → `'undefined'`), so analytics that call `.indexOf`/`.search`
//! on it unguarded (mixpanel on imgur, Yahoo Rapid, fandom's tracking
//! module) threw at top level. HTML LS §3.1.2: the getter returns the
//! document's referrer or `''` when there is none.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn runtime() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(
        make_doc(),
        "https://example.com/page.html",
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
    )
    .unwrap();
    rt
}

fn string_of(rt: &V8JsRuntime, code: &str) -> String {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("{code}: expected a string, got {other:?}"),
    }
}

/// The report's repro: present, a string, and string methods work on it.
#[test]
fn live_document_referrer_is_an_empty_string() {
    let rt = runtime();
    assert_eq!(string_of(&rt, "String('referrer' in document)"), "true");
    assert_eq!(string_of(&rt, "typeof document.referrer"), "string");
    assert_eq!(string_of(&rt, "document.referrer"), "");
    assert_eq!(string_of(&rt, "String(document.referrer.search(/google/))"), "-1");
    assert_eq!(string_of(&rt, "String(document.referrer.indexOf('yahoo'))"), "-1");
}

/// Readonly attribute: a sloppy-mode assignment is ignored, not stored.
#[test]
fn live_document_referrer_is_readonly() {
    let rt = runtime();
    rt.eval("document.referrer = 'https://evil.example/'").unwrap();
    assert_eq!(string_of(&rt, "document.referrer"), "");
}

/// A document created by script was never fetched — no referrer.
#[test]
fn created_document_referrer_is_an_empty_string() {
    let rt = runtime();
    assert_eq!(
        string_of(&rt, "typeof document.implementation.createHTMLDocument('t').referrer"),
        "string"
    );
    assert_eq!(string_of(&rt, "document.implementation.createHTMLDocument('t').referrer"), "");
}
