//! BUG-607 — `document.fgColor`/`bgColor`/`linkColor`/`vlinkColor`/
//! `alinkColor` (HTML LS §obsolete) were entirely absent (plain `undefined`),
//! and so were the underlying `HTMLBodyElement` obsolete IDL attributes
//! (`text`/`bgColor`/`link`/`vLink`/`aLink`) they forward to.

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

/// `document.body`'s own obsolete IDL attributes reflect the matching
/// content attribute, same shape as the BUG-602 `align` table.
#[test]
fn body_legacy_color_attrs_reflect_content_attributes() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.body.setAttribute('text', 'blue'); \
         document.body.setAttribute('bgcolor', 'green'); \
         document.body.setAttribute('link', 'red'); \
         document.body.setAttribute('vlink', 'yellow'); \
         document.body.setAttribute('alink', 'silver');",
    )
    .unwrap();
    assert!(is_true(&rt, "document.body.text === 'blue'"));
    assert!(is_true(&rt, "document.body.bgColor === 'green'"));
    assert!(is_true(&rt, "document.body.link === 'red'"));
    assert!(is_true(&rt, "document.body.vLink === 'yellow'"));
    assert!(is_true(&rt, "document.body.aLink === 'silver'"));

    rt.eval("document.body.bgColor = 'orange';").unwrap();
    assert!(is_true(
        &rt,
        "document.body.getAttribute('bgcolor') === 'orange'"
    ));
}

/// `[LegacyNullToEmptyString]`: a bare `null` write becomes `''`, not the
/// literal string `"null"` a plain string reflection would produce.
#[test]
fn body_legacy_color_attr_null_becomes_empty_string() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.body.text = 'blue'; document.body.text = null;")
        .unwrap();
    assert!(is_true(&rt, "document.body.text === ''"));
    assert!(is_true(&rt, "document.body.getAttribute('text') === ''"));
}

/// `document.fgColor`/etc. are a thin forward onto `document.body`'s own
/// properties, both for reading and writing.
#[test]
fn document_color_attrs_forward_to_body() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.fgColor = 'blue';").unwrap();
    assert!(is_true(&rt, "document.fgColor === 'blue'"));
    assert!(is_true(&rt, "document.body.text === 'blue'"));
    assert!(is_true(&rt, "document.body.getAttribute('text') === 'blue'"));

    rt.eval("document.body.link = 'red';").unwrap();
    assert!(is_true(&rt, "document.linkColor === 'red'"));
}

/// HTML LS: with no body element, the getters return `''` and the setters
/// are no-ops.
#[test]
fn document_color_attrs_no_body_are_inert() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.body.remove();").unwrap();
    assert!(is_true(&rt, "document.body === null"));
    assert!(is_true(&rt, "document.fgColor === ''"));
    rt.eval(
        "document.fgColor = 'red'; document.bgColor = 'red'; \
         document.linkColor = 'red'; document.vlinkColor = 'red'; \
         document.alinkColor = 'red';",
    )
    .unwrap();
    assert!(is_true(&rt, "document.fgColor === ''"));
    assert!(is_true(&rt, "document.bgColor === ''"));
    assert!(is_true(&rt, "document.linkColor === ''"));
    assert!(is_true(&rt, "document.vlinkColor === ''"));
    assert!(is_true(&rt, "document.alinkColor === ''"));
}
