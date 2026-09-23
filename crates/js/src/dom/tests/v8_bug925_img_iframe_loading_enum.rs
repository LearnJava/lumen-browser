//! BUG-925 — `<img>`/`<iframe>` `.loading` was reflected as a plain
//! `'string'`, not an enum: `getAttribute('loading')` worked, but an invalid
//! content attribute (e.g. `loading="BOGUS"`) read back through the IDL
//! getter dead verbatim instead of normalizing to `'eager'` per HTML LS
//! §4.8.11/§2.6.6.9 — same shape `fetchPriority`/`referrerPolicy` already
//! avoid. `<audio>`/`<video>` coverage for the rest of this bug (the media
//! lazy-loading behaviour itself) lives in `audio_element.rs`/
//! `video_bindings.rs`, which have no reflection-table row to test here.

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

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

/// Default (missing attribute) and invalid values both fall back to
/// `'eager'` -- `loading` is a limited-to-known-values enum, not a plain
/// string, even though the content attribute keeps the raw invalid value.
#[test]
fn loading_defaults_to_eager_and_normalizes_invalid_values() {
    let rt = v8_runtime_with_dom(make_doc());
    for tag in ["img", "iframe"] {
        rt.eval(&format!("var _e = document.createElement('{tag}');"))
            .unwrap();
        assert!(
            is_true(&rt, "_e.loading === 'eager'"),
            "<{tag}>.loading did not default to 'eager'"
        );
        rt.eval("_e.setAttribute('loading', 'BOGUS');").unwrap();
        assert!(
            is_true(&rt, "_e.loading === 'eager'"),
            "<{tag}>.loading did not reject an invalid content attribute"
        );
        assert!(
            is_true(&rt, "_e.getAttribute('loading') === 'BOGUS'"),
            "<{tag}>'s content attribute must keep the raw invalid value"
        );
    }
}

/// The `lazy` keyword reflects both ways: setting the IDL property writes
/// the content attribute, and reading it back through either the attribute
/// or the property agrees.
#[test]
fn loading_reflects_lazy_both_ways() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var i = document.createElement('img'); i.setAttribute('loading', 'lazy');")
        .unwrap();
    assert!(is_true(&rt, "i.loading === 'lazy'"));

    rt.eval("var f = document.createElement('iframe'); f.loading = 'lazy';")
        .unwrap();
    assert!(is_true(&rt, "f.loading === 'lazy'"));
    assert!(is_true(&rt, "f.getAttribute('loading') === 'lazy'"));
}
