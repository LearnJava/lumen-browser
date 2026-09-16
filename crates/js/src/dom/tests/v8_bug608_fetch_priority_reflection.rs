//! BUG-608 — `fetchPriority` IDL attribute (HTML LS §2.5.3) missing entirely
//! on `<img>`/`<script>`/`<link>`/`<iframe>`: `.fetchPriority` read
//! `undefined` even though `getAttribute('fetchpriority')` worked fine (only
//! the reflection-table row was absent, same shape as BUG-602's `align`).

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

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

/// Default (missing attribute) and invalid values both fall back to `'auto'`
/// -- `fetchPriority` is a limited-to-known-values enum, not a plain string.
#[test]
fn fetch_priority_defaults_to_auto_and_rejects_invalid_values() {
    let rt = v8_runtime_with_dom(make_doc());
    for tag in ["img", "script", "link", "iframe"] {
        rt.eval(&format!("var _e = document.createElement('{tag}');"))
            .unwrap();
        assert!(
            is_true(&rt, "_e.fetchPriority === 'auto'"),
            "<{tag}>.fetchPriority did not default to 'auto'"
        );
        rt.eval("_e.setAttribute('fetchpriority', 'not-a-real-keyword');")
            .unwrap();
        assert!(
            is_true(&rt, "_e.fetchPriority === 'auto'"),
            "<{tag}>.fetchPriority did not reject an invalid content attribute"
        );
    }
}

/// Valid keywords (`high`/`low`) reflect the content attribute, and the
/// setter writes it back.
#[test]
fn fetch_priority_reflects_valid_keywords_both_ways() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var i = document.createElement('img'); i.setAttribute('fetchpriority', 'high');")
        .unwrap();
    assert!(is_true(&rt, "i.fetchPriority === 'high'"));

    rt.eval("var s = document.createElement('script'); s.fetchPriority = 'low';")
        .unwrap();
    assert!(is_true(&rt, "s.fetchPriority === 'low'"));
    assert!(is_true(&rt, "s.getAttribute('fetchpriority') === 'low'"));
}
