//! BUG-504 part 10 — regression coverage for [`crate::v8_runtime::style_flush`]'s
//! scroll-offset reapplication: [`FlushHandles::maybe_flush`] rebuilds a
//! throwaway layout tree from scratch (every scroll container starts at
//! `scroll_x`/`scroll_y == 0.0`, the box-tree construction default), so
//! before this slice any same-tick style mutation elsewhere on the page —
//! triggering the CSSOM-4/BUG-493 flush for an unrelated `getComputedStyle`/
//! `getBoundingClientRect` read — silently reset every scrolled container's
//! JS-visible `scrollLeft`/`scrollTop` to 0 as a side effect. See
//! `bugs/BUG-504-OPEN.md` part 10 for the full investigation, including why
//! this alone does not close `overflow-clip-clamps-and-ignores-scroll-
//! offsets-vertical-rl.html` (a separate, deeper gap: `scrollTo()`/
//! `scrollBy()`/direct `scrollLeft=`/`scrollTop=` assignment queue into
//! `pending_scrolls` and are never visible to a same-tick read at all,
//! flush or no flush — filed as its own bug).

use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_dom::{Document, NodeData, QualName};
use std::collections::HashMap;

/// `#main` (overflow: auto, genuinely larger content — a real scroll
/// container) containing `.mover` (an unrelated sibling whose style the
/// test mutates to trigger `maybe_flush` without touching `#main` at all).
/// Returns `(doc, main_nid)`.
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
    let mover = doc.create_element(QualName::html("div"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(mover).data {
        attrs.push(lumen_dom::Attribute {
            name: QualName::html("id"),
            value: "mover".into(),
        });
    }
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    doc.append_child(body, main);
    doc.append_child(main, content);
    doc.append_child(body, mover);
    let main_nid = main.index() as u32;
    (Arc::new(Mutex::new(doc)), main_nid)
}

fn v8_runtime_with_scroll_flush(doc: Arc<Mutex<Document>>, main_nid: u32) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(
        "#main { width: 20px; height: 20px; overflow: auto; } \
         .content { width: 200px; height: 200px; }",
    )));
    rt.update_viewport_size(800.0, 600.0);
    // Mirrors what a real relayout's `collect_scroll_containers_for_js_state`
    // push would have left behind after an earlier, ordinary (not same-tick)
    // `scrollTo` settled.
    let mut states = HashMap::new();
    states.insert(main_nid, [15.0f32, 12.0, 200.0, 200.0]);
    rt.update_scroll_states(states);
    rt
}

/// Pre-fix, this returned `0` — the same-tick flush triggered by the
/// unrelated `#mover` mutation rebuilt the layout tree from scratch and fed
/// its all-zero scroll offsets straight into `layout_rects`/`computed_styles`
/// without ever touching (or preserving) `scroll_states`, which stayed at
/// its pre-flush value — this test instead exercises the getter path, which
/// reads the map `maybe_flush` now DOES touch after this slice.
#[test]
fn scroll_left_survives_unrelated_same_tick_style_flush() {
    let (doc, main_nid) = make_scroll_doc();
    let rt = v8_runtime_with_scroll_flush(doc, main_nid);
    let r = rt
        .eval(
            "(function() {
                var mover = document.getElementById('mover');
                mover.style.color = 'blue';
                return document.getElementById('main').scrollLeft;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(15.0));
}

#[test]
fn scroll_top_survives_unrelated_same_tick_style_flush() {
    let (doc, main_nid) = make_scroll_doc();
    let rt = v8_runtime_with_scroll_flush(doc, main_nid);
    let r = rt
        .eval(
            "(function() {
                var mover = document.getElementById('mover');
                mover.style.color = 'blue';
                return document.getElementById('main').scrollTop;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(12.0));
}
