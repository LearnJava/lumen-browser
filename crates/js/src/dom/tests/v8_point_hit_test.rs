//! `document.elementFromPoint`/`elementsFromPoint` (CSSOM View §3,
//! BUG-464/BUG-477) — real `LayoutBox` tree pushed via `update_hit_test_tree`,
//! not a canned rect map, since the native runs the actual stacking-aware
//! hit test (`lumen_paint::hit_test`/`hit_test_all`) against it.
//!
//! The fixture uses two nested EMPTY `<div>`s (no text) rather than inline
//! content: `hit_test_box`'s `InlineRun` handling resolves a text hit to the
//! DOM ancestor that establishes the inline formatting context (documented
//! in `hit_test.rs`'s own module doc — "какой текстовый узел под курсором"
//! is a Phase 0 gap), so a `<span>`-in-text fixture would hit its parent
//! `<div>`, not the span itself. A plain block box IS its own hit target,
//! which is what these tests need to verify.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`] (same shape as
/// `v8_elem_geometry_scroll.rs`'s own).
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

/// Real (HTML-parsed, not hand-built) `html > body > div.outer > div.inner`,
/// `.inner` fully inside `.outer`'s top-left corner.
fn make_hit_test_doc() -> Arc<Mutex<Document>> {
    let doc = lumen_html_parser::parse(
        r#"<html><body><div class="outer"><div class="inner"></div></div></body></html>"#,
    );
    Arc::new(Mutex::new(doc))
}

/// Lays out `make_hit_test_doc()`'s tree and pushes the result as the
/// hit-test tree, so `elementFromPoint` has real geometry to test against —
/// same document the runtime's own DOM natives read from. Resets the UA
/// `body { margin: 8px }` (HTML Rendering §14.3.3) the same way
/// `lumen_layout::lib.rs`'s own tests do (`BODY_RESET`), so a hit at (2, 2)
/// lands in content instead of the body's margin box.
fn layout_and_push(rt: &V8JsRuntime, doc_arc: &Arc<Mutex<Document>>) {
    let sheet = lumen_css_parser::parse(
        "body{margin:0} .outer{height:100px} .inner{height:50px}",
    );
    let root = {
        let doc = doc_arc.lock().unwrap();
        lumen_layout::layout(&doc, &sheet, lumen_core::geom::Size::new(800.0, 600.0))
    };
    rt.update_hit_test_tree(Arc::new(root));
}

#[test]
fn element_from_point_returns_innermost_class() {
    let doc_arc = make_hit_test_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc_arc));
    layout_and_push(&rt, &doc_arc);
    let cls = rt.eval(
        "var el = document.elementFromPoint(2, 2); el ? el.className : null"
    ).unwrap();
    assert_eq!(cls, lumen_core::JsValue::String("inner".into()));
}

#[test]
fn element_from_point_outside_layout_returns_null() {
    let doc_arc = make_hit_test_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc_arc));
    layout_and_push(&rt, &doc_arc);
    let el = rt.eval("document.elementFromPoint(99999, 99999)").unwrap();
    assert_eq!(el, lumen_core::JsValue::Null);
}

#[test]
fn element_from_point_before_any_layout_returns_null() {
    // No `layout_and_push` call — `hit_test_tree` is still `None`. Must not
    // panic (no unwrap on a missing tree) and must behave like a genuine miss.
    let doc_arc = make_hit_test_doc();
    let rt = v8_runtime_with_dom(doc_arc);
    let el = rt.eval("document.elementFromPoint(2, 2)").unwrap();
    assert_eq!(el, lumen_core::JsValue::Null);
}

#[test]
fn elements_from_point_includes_ancestors_topmost_first() {
    let doc_arc = make_hit_test_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc_arc));
    layout_and_push(&rt, &doc_arc);
    let classes = rt.eval(
        "document.elementsFromPoint(2, 2)\
            .map(function(e) { return e.className || e.tagName; })\
            .join(',')"
    ).unwrap();
    let lumen_core::JsValue::String(joined) = classes else {
        panic!("expected a joined string of class/tag names");
    };
    let items: Vec<&str> = joined.split(',').collect();
    assert_eq!(items.first(), Some(&"inner"), "topmost hit must come first: {items:?}");
    assert!(items.contains(&"outer"), "elementsFromPoint must include the ancestor .outer: {items:?}");
    assert!(items.contains(&"BODY"), "elementsFromPoint must include the ancestor <body>: {items:?}");
    assert!(items.contains(&"HTML"), "elementsFromPoint must include the root <html>: {items:?}");
}

#[test]
fn elements_from_point_outside_layout_returns_empty_array() {
    let doc_arc = make_hit_test_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc_arc));
    layout_and_push(&rt, &doc_arc);
    let len = rt.eval("document.elementsFromPoint(99999, 99999).length").unwrap();
    assert_eq!(len, lumen_core::JsValue::Number(0.0));
}

#[test]
fn element_from_point_and_elements_from_point_agree_on_topmost() {
    let doc_arc = make_hit_test_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc_arc));
    layout_and_push(&rt, &doc_arc);
    let agree = rt.eval(
        "document.elementFromPoint(2, 2) === document.elementsFromPoint(2, 2)[0]"
    ).unwrap();
    assert_eq!(agree, lumen_core::JsValue::Bool(true));
}
