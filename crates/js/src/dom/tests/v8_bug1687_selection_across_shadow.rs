//! BUG-1687 — `Selection.setBaseAndExtent` keeps anchor and focus that live in different tree
//! scopes when both belong to the flattened tree (`selection/shadow-dom/cross-shadow-boundary-*`).
//! A point outside it still collapses onto the anchor (BUG-966).

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

#[test]
fn focus_inside_a_shadow_tree_is_kept() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, r#"(function() {
        var host = document.createElement('div');
        document.body.appendChild(host);
        var root = host.attachShadow({mode: 'open'});
        root.innerHTML = 'inner';
        var s = getSelection();
        s.setBaseAndExtent(document.body, 0, root.firstChild, 3);
        return s.anchorNode === document.body && s.focusNode === root.firstChild && s.focusOffset === 3;
    })()"#));
}

#[test]
fn document_is_a_valid_anchor() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, r#"(function() {
        var host = document.createElement('div');
        document.body.appendChild(host);
        var root = host.attachShadow({mode: 'open'});
        root.innerHTML = 'CONTENT';
        var s = getSelection();
        s.setBaseAndExtent(document, 0, root.firstChild, 2);
        return s.anchorNode === document && s.focusNode === root.firstChild && s.focusOffset === 2;
    })()"#));
}

#[test]
fn point_outside_the_flat_tree_collapses_onto_the_anchor() {
    let rt = v8_runtime_with_dom(make_doc());
    // A detached shadow tree is not part of the document's flattened tree.
    assert!(is_true(&rt, r#"(function() {
        var host = document.createElement('div');
        var root = host.attachShadow({mode: 'open'});
        root.innerHTML = 'inner';
        var s = getSelection();
        s.setBaseAndExtent(document.body, 0, root.firstChild, 3);
        return s.focusNode === document.body && s.focusOffset === 0;
    })()"#));
}

#[test]
fn nested_video_anchor_still_collapses() {
    let rt = v8_runtime_with_dom(make_doc());
    // `selection/selection-nested-video.html`: a media element's light children are not rendered.
    assert!(is_true(&rt, r#"(function() {
        var a = document.createElement('div');
        var v = document.createElement('video');
        var b = document.createElement('video');
        v.appendChild(b);
        document.body.appendChild(a);
        document.body.appendChild(v);
        var c = a.attachShadow({mode: 'open'});
        var s = getSelection();
        s.setBaseAndExtent(b, 0, c, 0);
        return s.anchorNode === b && s.focusNode === b && s.focusOffset === 0;
    })()"#));
}
