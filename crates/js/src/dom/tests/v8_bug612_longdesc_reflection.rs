//! BUG-612 — obsolete-but-conforming `longdesc` content attribute has no IDL
//! reflection: `img.longdesc` reads `undefined` even though
//! `getAttribute('longdesc')`/`hasAttribute('longdesc')` already work (only
//! the reflection-table row was absent, same class as BUG-602's `align`).

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

#[test]
fn img_longdesc_reflects_content_attribute() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var i = document.createElement('img'); \
         i.setAttribute('longdesc', 'fail.html');",
    )
    .unwrap();
    assert!(is_true(&rt, "i.longDesc === 'fail.html'"));
}

#[test]
fn iframe_longdesc_reflects_content_attribute() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var f = document.createElement('iframe'); \
         f.setAttribute('longdesc', 'fail.html');",
    )
    .unwrap();
    assert!(is_true(&rt, "f.longDesc === 'fail.html'"));
}

/// `url`-kind reflection: the setter writes the attribute back verbatim
/// (URL resolution happens on read via the browsing context, not in the
/// reflection layer itself).
#[test]
fn longdesc_setter_writes_attribute() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var i = document.createElement('img');").unwrap();
    assert!(is_true(&rt, "i.longDesc === ''"));
    rt.eval("i.longDesc = 'desc.html';").unwrap();
    assert!(is_true(&rt, "i.getAttribute('longdesc') === 'desc.html'"));
}
