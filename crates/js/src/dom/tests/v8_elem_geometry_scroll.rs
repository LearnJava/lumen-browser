//! S12b-24-elem-geometry-scroll: Element geometry API, scroll events, CSS Scroll
//! Snap L2 snapchanging/snapchanged — QuickJS copies above removed after porting.

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

// ── Element geometry API ─────────────────────────────────────────────────

#[test]
fn get_bounding_client_rect_method_on_element() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.update_layout_rects([(nid, [5.0, 10.0, 200.0, 100.0])].into_iter().collect());
    let x = rt.eval("document.body.getBoundingClientRect().x").unwrap();
    assert_eq!(x, lumen_core::JsValue::Number(5.0));
    let w = rt.eval("document.body.getBoundingClientRect().width").unwrap();
    assert_eq!(w, lumen_core::JsValue::Number(200.0));
    let bottom = rt.eval("document.body.getBoundingClientRect().bottom").unwrap();
    assert_eq!(bottom, lumen_core::JsValue::Number(110.0));
}

#[test]
fn offset_width_height_on_element() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.update_layout_rects([(nid, [0.0, 0.0, 320.0, 240.0])].into_iter().collect());
    let ow = rt.eval("document.body.offsetWidth").unwrap();
    assert_eq!(ow, lumen_core::JsValue::Number(320.0));
    let oh = rt.eval("document.body.offsetHeight").unwrap();
    assert_eq!(oh, lumen_core::JsValue::Number(240.0));
}

#[test]
fn scroll_top_left_via_update_scroll_states() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.update_scroll_states([(nid, [42.0, 17.0, 800.0, 2000.0])].into_iter().collect());
    let sl = rt.eval("document.body.scrollLeft").unwrap();
    assert_eq!(sl, lumen_core::JsValue::Number(42.0));
    let st = rt.eval("document.body.scrollTop").unwrap();
    assert_eq!(st, lumen_core::JsValue::Number(17.0));
    let sw = rt.eval("document.body.scrollWidth").unwrap();
    assert_eq!(sw, lumen_core::JsValue::Number(800.0));
    let sh = rt.eval("document.body.scrollHeight").unwrap();
    assert_eq!(sh, lumen_core::JsValue::Number(2000.0));
}

/// BUG-475: CSSOM View defines `scrollWidth`/`scrollHeight` for every element,
/// not just designated `overflow: scroll`/`auto` containers. An element with
/// no entry in the scroll-state map (never a scroll container) must still
/// answer at least its padding/border-box size, not 0.
#[test]
fn scroll_width_height_fall_back_to_bounding_rect_for_non_scroll_container() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.update_layout_rects([(nid, [0.0, 0.0, 100.0, 60.0])].into_iter().collect());
    let sw = rt.eval("document.body.scrollWidth").unwrap();
    assert_eq!(sw, lumen_core::JsValue::Number(100.0));
    let sh = rt.eval("document.body.scrollHeight").unwrap();
    assert_eq!(sh, lumen_core::JsValue::Number(60.0));
}

#[test]
fn scroll_to_queues_request() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.update_scroll_states([(nid, [0.0, 0.0, 800.0, 2000.0])].into_iter().collect());
    rt.eval("document.body.scrollTo(100, 200)").unwrap();
    let reqs = rt.take_scroll_requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].0, nid);
    assert!((reqs[0].1 - 100.0).abs() < 0.1);
    assert!((reqs[0].2 - 200.0).abs() < 0.1);
}

#[test]
fn scroll_by_adds_to_current_position() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.update_scroll_states([(nid, [50.0, 100.0, 800.0, 2000.0])].into_iter().collect());
    rt.eval("document.body.scrollBy(10, -20)").unwrap();
    let reqs = rt.take_scroll_requests();
    assert_eq!(reqs.len(), 1);
    assert!((reqs[0].1 - 60.0).abs() < 0.1);
    assert!((reqs[0].2 - 80.0).abs() < 0.1);
}

// ── scroll events ─────────────────────────────────────────────────────────

#[test]
fn fire_element_scroll_dispatches_event() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.eval(&format!(
        "var fired = false; \
                 var el = document.body || _lumen_make_element({nid}); \
                 el.addEventListener('scroll', function() {{ fired = true; }});"
    )).unwrap();
    rt.fire_element_scroll(nid);
    let result = rt.eval("fired").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true), "scroll event should fire on element");
}

#[test]
fn fire_element_scroll_event_is_non_bubbling() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.eval(
        "var doc_fired = false; \
                 document.addEventListener('scroll', function() { doc_fired = true; });"
    ).unwrap();
    rt.fire_element_scroll(nid);
    let result = rt.eval("doc_fired").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(false), "scroll event must not bubble to document");
}

#[test]
fn fire_window_scroll_dispatches_event() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var win_fired = false; window.addEventListener('scroll', function() { win_fired = true; });").unwrap();
    rt.fire_window_scroll();
    let result = rt.eval("win_fired").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true), "window scroll event should fire");
}

// ── CSS Scroll Snap L2 snapchanging/snapchanged events ─────────────────────

#[test]
fn fire_snap_changing_dispatches_event() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.eval(&format!(
        "var snap_type = ''; \
                 var el = document.body || _lumen_make_element({nid}); \
                 el.addEventListener('snapchanging', function(e) {{ snap_type = e.type; }});"
    )).unwrap();
    rt.fire_snap_changing(nid, None, None);
    let result = rt.eval("snap_type").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("snapchanging".into()));
}

#[test]
fn fire_snap_changed_exposes_snap_targets() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.eval(&format!(
        "var block_ok = false; var inline_null = false; \
                 var el = document.body || _lumen_make_element({nid}); \
                 el.addEventListener('snapchanged', function(e) {{ \
                     block_ok = (e.snapTargetBlock !== null && e.snapTargetBlock !== undefined); \
                     inline_null = (e.snapTargetInline === null); \
                 }});"
    )).unwrap();
    rt.fire_snap_changed(nid, Some(nid), None);
    assert_eq!(rt.eval("block_ok").unwrap(), lumen_core::JsValue::Bool(true));
    assert_eq!(rt.eval("inline_null").unwrap(), lumen_core::JsValue::Bool(true));
}
