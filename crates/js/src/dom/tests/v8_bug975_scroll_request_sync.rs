//! BUG-975 — regression coverage for [`crate::v8_runtime::install::platform::install_scroll_state`]'s
//! optimistic `scroll_states` write on `_lumen_request_scroll`.
//!
//! Before this slice, `scrollTo`/`scrollBy`/direct `scrollLeft=`/`scrollTop=`
//! assignment only pushed the request into `pending_scrolls`, a queue the
//! shell drains asynchronously (`about_to_wait`'s idle tick). `_lumen_get_scroll_state`
//! reads exclusively from the `scroll_states` cache, which that queue never
//! fed — a synchronous read in the SAME script turn as the write always saw
//! the stale pre-request value (often 0/0), never the value just requested.
//! See `bugs/BUG-975-OPEN.md` for the full investigation, including why the
//! optimistic write is intentionally unclamped except for the one absolute
//! `overflow: clip` rule.

use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_dom::{Document, NodeData, QualName};
use std::collections::HashMap;

/// `#main` (a real scroll container, content genuinely larger than its box)
/// whose `overflow` the caller controls via the stylesheet in
/// [`v8_runtime_with_scroll_state`]. Returns `(doc, main_nid)`.
fn make_scroll_doc() -> (Arc<Mutex<Document>>, u32) {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let body = doc.create_element(QualName::html("body"));
    let main = doc.create_element(QualName::html("div"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(main).data {
        attrs.push(lumen_dom::Attribute {
            name: QualName::html("id"),
            value: "main".into(),
        });
    }
    let content = doc.create_element(QualName::html("div"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(content).data {
        attrs.push(lumen_dom::Attribute {
            name: QualName::html("class"),
            value: "content".into(),
        });
    }
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    doc.append_child(body, main);
    doc.append_child(main, content);
    let main_nid = main.index() as u32;
    (Arc::new(Mutex::new(doc)), main_nid)
}

fn v8_runtime_with_scroll_state(doc: Arc<Mutex<Document>>, main_nid: u32, overflow: &str, seed: [f32; 4]) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(&format!(
        "#main {{ width: 20px; height: 20px; overflow: {overflow}; }} \
         .content {{ width: 200px; height: 200px; }}"
    ))));
    rt.update_viewport_size(800.0, 600.0);
    // Mirrors what a real relayout's `collect_scroll_containers_for_js_state`
    // push would have left behind — the optimistic write only ever touches an
    // already-known container, so every test seeds one.
    let mut states = HashMap::new();
    states.insert(main_nid, seed);
    rt.update_scroll_states(states);
    rt
}

#[test]
fn scroll_to_is_visible_to_synchronous_read() {
    let (doc, main_nid) = make_scroll_doc();
    let rt = v8_runtime_with_scroll_state(doc, main_nid, "auto", [0.0, 0.0, 200.0, 200.0]);
    let r = rt
        .eval(
            "(function() {
                var main = document.getElementById('main');
                main.scrollTo(20, 30);
                return [main.scrollLeft, main.scrollTop];
            })()",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::Array(vec![
            lumen_core::JsValue::Number(20.0),
            lumen_core::JsValue::Number(30.0),
        ])
    );
}

#[test]
fn scroll_by_is_visible_to_synchronous_read() {
    let (doc, main_nid) = make_scroll_doc();
    let rt = v8_runtime_with_scroll_state(doc, main_nid, "auto", [5.0, 5.0, 200.0, 200.0]);
    let r = rt
        .eval(
            "(function() {
                var main = document.getElementById('main');
                main.scrollBy(10, -2);
                return [main.scrollLeft, main.scrollTop];
            })()",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::Array(vec![
            lumen_core::JsValue::Number(15.0),
            lumen_core::JsValue::Number(3.0),
        ])
    );
}

#[test]
fn direct_scroll_left_top_assignment_is_visible_to_synchronous_read() {
    let (doc, main_nid) = make_scroll_doc();
    let rt = v8_runtime_with_scroll_state(doc, main_nid, "auto", [0.0, 0.0, 200.0, 200.0]);
    let r = rt
        .eval(
            "(function() {
                var main = document.getElementById('main');
                main.scrollLeft = 25;
                main.scrollTop = 35;
                return [main.scrollLeft, main.scrollTop];
            })()",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::Array(vec![
            lumen_core::JsValue::Number(25.0),
            lumen_core::JsValue::Number(35.0),
        ])
    );
}

/// `overflow: clip` (CSS Overflow L3 §3.4) rejects every scroll offset
/// outright — the one clamp rule the optimistic write can honor exactly
/// without live layout data, since it comes straight from `computed_styles`,
/// not from geometry.
#[test]
fn overflow_clip_pins_optimistic_write_to_zero() {
    let (doc, main_nid) = make_scroll_doc();
    let rt = v8_runtime_with_scroll_state(doc, main_nid, "clip", [0.0, 0.0, 200.0, 200.0]);
    let r = rt
        .eval(
            "(function() {
                var main = document.getElementById('main');
                main.scrollTo(20, 30);
                return [main.scrollLeft, main.scrollTop];
            })()",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::Array(vec![
            lumen_core::JsValue::Number(0.0),
            lumen_core::JsValue::Number(0.0),
        ])
    );
}
