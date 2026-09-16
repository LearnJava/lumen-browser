//! BUG-602 — obsolete-but-conforming `align` IDL attribute missing on every
//! interface the living standard gives it: `.align` read `undefined` and
//! threw on `.toLowerCase()` even though `getAttribute('align')` worked fine
//! (only the reflection-table row was absent).

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

/// The exact failure from `legend-align-justify-self.html`: reading `.align`
/// used to throw `TypeError: Cannot read properties of undefined` on the
/// `.toLowerCase()` call the test makes.
#[test]
fn legend_align_reflects_content_attribute() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var l = document.createElement('legend'); l.setAttribute('align', 'left');")
        .unwrap();
    assert!(is_true(&rt, "l.align === 'left'"));
    assert!(is_true(&rt, "l.align.toLowerCase() === 'left'"));
}

/// One reflection-table row is meant to cover every affected interface at
/// once (HTML LS obsolete features): `<div>`/`<table>`/`<tr>`/`<td>`/`<th>`/
/// `<thead>`/`<tbody>`/`<tfoot>`/`<caption>`/`<hr>`/`<img>`/`<iframe>`/`<p>`/
/// heading elements, alongside `<legend>` above.
#[test]
fn align_reflects_on_every_affected_interface() {
    let rt = v8_runtime_with_dom(make_doc());
    for tag in [
        "div", "table", "tr", "td", "th", "thead", "tbody", "tfoot", "caption", "hr", "img",
        "iframe", "p", "h1",
    ] {
        rt.eval(&format!(
            "var _e = document.createElement('{tag}'); \
             _e.setAttribute('align', 'center');"
        ))
        .unwrap();
        assert!(
            is_true(&rt, "_e.align === 'center'"),
            "<{tag}>.align did not reflect the content attribute"
        );
    }
}

/// Plain string reflection (HTML LS §obsolete `align`), not a keyword-limited
/// `enum` — an invalid value round-trips verbatim instead of falling back to
/// a default, and the setter writes the attribute back.
#[test]
fn align_setter_writes_attribute_verbatim() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var d = document.createElement('div');").unwrap();
    assert!(is_true(&rt, "d.align === ''"));
    rt.eval("d.align = 'not-a-real-keyword';").unwrap();
    assert!(is_true(&rt, "d.align === 'not-a-real-keyword'"));
    assert!(is_true(
        &rt,
        "d.getAttribute('align') === 'not-a-real-keyword'"
    ));
}

/// `<col>`/`<colgroup>` deliberately keep no `align` reflection — the living
/// standard's obsolete-attributes table does not give it to
/// `HTMLTableColElement`, unlike its `width` neighbor.
#[test]
fn table_col_element_has_no_align_reflection() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(
        &rt,
        "document.createElement('col').align === undefined"
    ));
}
