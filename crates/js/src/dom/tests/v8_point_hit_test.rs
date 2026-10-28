//! `document.elementFromPoint`/`elementsFromPoint` (CSSOM View §3,
//! BUG-464/BUG-477) — real `LayoutBox` tree pushed via `update_hit_test_tree`,
//! not a canned rect map, since the native runs the actual stacking-aware
//! hit test (`lumen_paint::hit_test`/`hit_test_all`) against it.
//!
//! Inline elements (`<a>`, `<span>`) are hit through `HitTestResult::inline_source` (BUG-1557).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`] (same shape as
/// `v8_elem_geometry_scroll.rs`'s own).
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt.update_viewport_size(800.0, 600.0);
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
fn element_from_point_before_any_layout_returns_root() {
    // No `layout_and_push` call — `hit_test_tree` is still `None`. Must not
    // panic (no unwrap on a missing tree); a viewport point over no box is the root element.
    let doc_arc = make_hit_test_doc();
    let rt = v8_runtime_with_dom(doc_arc);
    let el = rt.eval("document.elementFromPoint(2, 2).tagName").unwrap();
    assert_eq!(el, lumen_core::JsValue::String("HTML".into()));
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

/// BUG-1202: a node inserted in the same tick is found by the hit test (the flush's own tree is
/// used, not the stale embedder snapshot).
#[test]
fn element_from_point_finds_node_inserted_this_tick() {
    let doc_arc = make_hit_test_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc_arc));
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse("body { margin: 0; } button { display: block; width: 80px; height: 20px; }")));
    rt.update_viewport_size(800.0, 600.0);
    layout_and_push(&rt, &doc_arc);
    let r = rt.eval(
        "var b = document.createElement('button'); document.body.appendChild(b);
         var r = b.getClientRects()[0];
         var e = document.elementFromPoint(r.left + 2, r.top + 2);
         (e ? e.localName : 'null') + '|' + document.elementsFromPoint(r.left + 2, r.top + 2).map(function(x){return x.localName;}).join('>')"
    ).unwrap();
    match r {
        lumen_core::JsValue::String(s) => assert_eq!(s, "button|button>body>html"),
        o => panic!("{o:?}"),
    }
}

/// BUG-1557: inline elements are hit (not just their block container), the chain runs from the
/// innermost inline element up; empty space right of the text still belongs to the block.
#[test]
fn hit_test_reports_inline_elements() {
    let doc_arc = Arc::new(Mutex::new(lumen_html_parser::parse(
        r#"<html><body><div id="c">aaa <a id="a">link <b id="b">bold</b></a></div></body></html>"#,
    )));
    let rt = v8_runtime_with_dom(Arc::clone(&doc_arc));
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse("body{margin:0} div{font:16px monospace}")));
    let sheet = lumen_css_parser::parse("body{margin:0} div{font:16px monospace}");
    let root = {
        let doc = doc_arc.lock().unwrap();
        lumen_layout::layout(&doc, &sheet, lumen_core::geom::Size::new(800.0, 600.0))
    };
    rt.update_hit_test_tree(Arc::new(root));
    let r = rt.eval(
        "var ids = function(l) { return l.map(function(e) { return e.id || e.tagName; }).join('>'); };
         var rb = document.getElementById('b').getClientRects()[0];
         var x = rb.left + 2, y = rb.top + 2;
         var past = document.elementFromPoint(700, y);
         [document.elementFromPoint(x, y).id, ids(document.elementsFromPoint(x, y)), past && past.id].join('|')",
    )
    .unwrap();
    match r {
        lumen_core::JsValue::String(s) => assert_eq!(s, "b|b>a>c>BODY>HTML|c"),
        o => panic!("{o:?}"),
    }
}

/// BUG-1557: CSSOM View §3 — a point inside the viewport over no box hits the root element;
/// outside the viewport nothing; missing / non-finite coordinates are a TypeError.
#[test]
fn hit_test_viewport_root_and_argument_checks() {
    let doc_arc = make_hit_test_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc_arc));
    layout_and_push(&rt, &doc_arc);
    let r = rt.eval(
        "var thrown = function(f) { try { f(); return 'no'; } catch (e) { return e instanceof TypeError ? 'TE' : 'other'; } };
         [document.elementFromPoint(500, 500).tagName,
          document.elementsFromPoint(500, 500).map(function(e) { return e.tagName; }).join('>'),
          String(document.elementFromPoint(900, 10)),
          document.elementsFromPoint(-1, 10).length,
          thrown(function() { document.elementFromPoint(); }),
          thrown(function() { document.elementsFromPoint(1); }),
          thrown(function() { document.elementFromPoint(0, Infinity); }),
          thrown(function() { document.elementsFromPoint(NaN, 0); })].join('|')",
    )
    .unwrap();
    match r {
        lumen_core::JsValue::String(s) => assert_eq!(s, "HTML|HTML|null|0|TE|TE|TE|TE"),
        o => panic!("{o:?}"),
    }
}
