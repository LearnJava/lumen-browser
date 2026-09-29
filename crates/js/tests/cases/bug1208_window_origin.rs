//! BUG-1208 — `window.origin`/`self.origin` (WindowOrWorkerGlobalScope
//! mixin, HTML LS §7.4.1) is missing from the global object shim, and
//! `location.origin` for a non-sandboxed `about:blank`/`about:srcdoc`
//! sub-document must be the PARENT's origin (inherited), not the opaque
//! `about:`-address one.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn make_rt(page_url: &str, origin_inherit_from: Option<&str>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(
        doc,
        page_url,
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
        origin_inherit_from,
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

/// A normal `https:` page: `window.origin`/`self.origin` are the page's own
/// tuple origin, and equal `location.origin`.
#[test]
fn window_and_self_origin_match_location_origin_on_a_normal_page() {
    let rt = make_rt("https://example.com:1234/doc", None);
    assert_eq!(
        str_eval(&rt, "window.origin"),
        "https://example.com:1234"
    );
    assert_eq!(str_eval(&rt, "self.origin"), "https://example.com:1234");
    assert_eq!(str_eval(&rt, "location.origin"), "https://example.com:1234");
}

/// A `data:` document (or any other opaque-origin address) reports the
/// literal string `"null"` for `window.origin`/`location.origin` — the HTML
/// LS §7.1.1 ASCII serialization of an opaque origin — not the empty string.
#[test]
fn window_origin_is_the_literal_string_null_for_an_opaque_address() {
    let rt = make_rt("data:text/html,hi", None);
    assert_eq!(str_eval(&rt, "window.origin"), "null");
    assert_eq!(str_eval(&rt, "self.origin"), "null");
    assert_eq!(str_eval(&rt, "location.origin"), "null");
}

/// The core of BUG-1208: a non-sandboxed `about:srcdoc` sub-document's own
/// realm origin — `window.origin`/`self.origin` — is the PARENT's origin
/// (HTML LS §7.4.1's initialise-the-Document's-origin algorithm for
/// `about:` addresses), even though `location.href`/`location.origin` still
/// report the opaque `about:srcdoc` address itself.
#[test]
fn about_srcdoc_window_origin_inherits_the_parent_but_location_origin_does_not() {
    let rt = make_rt("about:srcdoc", Some("https://parent.example"));
    assert_eq!(str_eval(&rt, "window.origin"), "https://parent.example");
    assert_eq!(str_eval(&rt, "self.origin"), "https://parent.example");
    // `location.origin` is the literal address's own (opaque) origin — it
    // never inherits, unlike `window.origin`.
    assert_eq!(str_eval(&rt, "location.origin"), "null");
}

/// Same as above for `about:blank`.
#[test]
fn about_blank_window_origin_inherits_the_parent() {
    let rt = make_rt("about:blank", Some("https://parent.example:8080"));
    assert_eq!(
        str_eval(&rt, "window.origin"),
        "https://parent.example:8080"
    );
    assert_eq!(str_eval(&rt, "location.origin"), "null");
}

/// A sandboxed (or otherwise parent-less) `about:blank`/`about:srcdoc`
/// document has no inherited origin to fall back on, so it stays opaque —
/// same "null" as any other opaque address.
#[test]
fn about_blank_without_an_inherited_origin_stays_opaque() {
    let rt = make_rt("about:blank", None);
    assert_eq!(str_eval(&rt, "window.origin"), "null");
    assert_eq!(str_eval(&rt, "location.origin"), "null");
}
