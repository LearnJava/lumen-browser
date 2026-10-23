//! CQ-SCROLL-STATE-2 — `@container scroll-state(scrollable: …)` follows the scroll offset:
//! the queries are answered in the layout pass, which starts every scroller at offset 0, so the
//! same-tick flush hands it the offsets the page set (`scrollTop = …`) or the shell reported
//! (`update_scroll_states`).

use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_dom::{Document, NodeData, QualName};
use std::collections::HashMap;

/// `#scroller` (200×200, 600px of content) with `#target` inside the content.
fn doc() -> (Arc<Mutex<Document>>, u32) {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let body = doc.create_element(QualName::html("body"));
    let mut with_id = |tag: &str, id: &str| {
        let el = doc.create_element(QualName::html(tag));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(el).data {
            attrs.push(lumen_dom::Attribute { name: QualName::html("id"), value: id.into() });
        }
        el
    };
    let scroller = with_id("div", "scroller");
    let filler = with_id("div", "filler");
    let target = with_id("div", "target");
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    doc.append_child(body, scroller);
    doc.append_child(scroller, filler);
    doc.append_child(filler, target);
    let nid = scroller.index() as u32;
    (Arc::new(Mutex::new(doc)), nid)
}

fn runtime() -> (V8JsRuntime, u32) {
    let (doc, nid) = doc();
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(
        "#scroller { width: 200px; height: 200px; container-type: scroll-state; overflow-y: scroll; }
         #filler { height: 600px; }
         #target { --top: no; --bottom: no; }
         @container scroll-state(scrollable: top) { #target { --top: yes; } }
         @container scroll-state(scrollable: bottom) { #target { --bottom: yes; } }",
    )));
    rt.update_viewport_size(800.0, 600.0);
    (rt, nid)
}

const READ: &str = "(function(){ var cs = getComputedStyle(document.getElementById('target')); \
                    return cs.getPropertyValue('--top') + '/' + cs.getPropertyValue('--bottom'); })()";

#[test]
fn scrollable_follows_script_scroll_top() {
    let (rt, _) = runtime();
    let eval = |js: &str| match rt.eval(&format!("(function(){{ {js} }})()")).unwrap() {
        lumen_core::JsValue::String(s) => s.to_string(),
        other => panic!("expected a string, got {other:?}"),
    };
    assert_eq!(eval(&format!("return ({READ});")), "no/yes", "before any scroll");
    assert_eq!(
        eval(&format!("document.getElementById('scroller').scrollTop = 200; return ({READ});")),
        "yes/yes",
        "in the middle"
    );
    assert_eq!(
        eval(&format!("document.getElementById('scroller').scrollTop = 400; return ({READ});")),
        "yes/no",
        "at the end"
    );
    assert_eq!(
        eval(&format!("document.getElementById('scroller').scrollTop = 0; return ({READ});")),
        "no/yes",
        "back at the top"
    );
}

#[test]
fn scrollable_follows_the_shell_scroll_report() {
    let (rt, nid) = runtime();
    let read = |rt: &V8JsRuntime| match rt.eval(&format!("(function(){{ return ({READ}); }})()")).unwrap() {
        lumen_core::JsValue::String(s) => s.to_string(),
        other => panic!("expected a string, got {other:?}"),
    };
    assert_eq!(read(&rt), "no/yes");
    let mut states = HashMap::new();
    states.insert(nid, [0.0f32, 400.0, 200.0, 600.0]);
    rt.update_scroll_states(states);
    assert_eq!(read(&rt), "yes/no", "the shell moved the scroller to the end");
}
