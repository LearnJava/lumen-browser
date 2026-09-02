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

/// BUG-476: `offsetLeft`/`offsetTop` must be relative to the nearest
/// positioned ancestor (`offsetParent`), not the viewport — measured from
/// that ancestor's *padding* edge (its border-box origin plus its own border
/// widths), CSSOM View §5. `div#main` (the `offsetParent`, `position:
/// relative`, 6px left / 3px top border) sits at viewport (8, 8); its padding
/// edge is therefore at (14, 11). `span.highlight` (the target, an ordinary
/// static descendant) sits at viewport (24, 20) — offsetLeft/offsetTop must
/// report the difference: 10 and 9.
#[test]
fn offset_left_top_relative_to_positioned_ancestor() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let (div_nid, span_nid) = {
        let doc = doc_arc.lock().unwrap();
        (
            super::super::find_element_by_tag(&doc, "div").unwrap().index() as u32,
            super::super::find_element_by_tag(&doc, "span").unwrap().index() as u32,
        )
    };
    rt.update_layout_rects(
        [(div_nid, [8.0, 8.0, 300.0, 200.0]), (span_nid, [24.0, 20.0, 50.0, 20.0])]
            .into_iter()
            .collect(),
    );
    rt.update_computed_styles(
        [(
            div_nid,
            [
                ("position".to_string(), "relative".to_string()),
                ("border-left-width".to_string(), "6px".to_string()),
                ("border-top-width".to_string(), "3px".to_string()),
            ]
            .into_iter()
            .collect(),
        )]
        .into_iter()
        .collect(),
    );
    let left = rt.eval("document.getElementsByClassName('highlight')[0].offsetLeft").unwrap();
    assert_eq!(left, lumen_core::JsValue::Number(10.0));
    let top = rt.eval("document.getElementsByClassName('highlight')[0].offsetTop").unwrap();
    assert_eq!(top, lumen_core::JsValue::Number(9.0));
}

/// The same fixture with no `position` set anywhere: `div#main` is skipped
/// (statically positioned) and the walk keeps going up to `<body>`, which the
/// algorithm always accepts as an `offsetParent` — CSSOM View §5 does not
/// require a positioned `<body>`. With `<body>` at the viewport origin,
/// `offsetLeft`/`offsetTop` on the target reduce to its own viewport
/// position.
#[test]
fn offset_left_top_falls_back_to_body_when_no_positioned_ancestor() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let (body_nid, span_nid) = {
        let doc = doc_arc.lock().unwrap();
        (
            super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32,
            super::super::find_element_by_tag(&doc, "span").unwrap().index() as u32,
        )
    };
    rt.update_layout_rects(
        [(body_nid, [0.0, 0.0, 800.0, 600.0]), (span_nid, [5.0, 7.0, 50.0, 20.0])]
            .into_iter()
            .collect(),
    );
    let left = rt.eval("document.getElementsByClassName('highlight')[0].offsetLeft").unwrap();
    assert_eq!(left, lumen_core::JsValue::Number(5.0));
    let top = rt.eval("document.getElementsByClassName('highlight')[0].offsetTop").unwrap();
    assert_eq!(top, lumen_core::JsValue::Number(7.0));
}

/// CSSOM View §5 step 1: `<body>` itself is a special case, always zero
/// regardless of its own `offsetParent` walk.
#[test]
fn offset_left_top_zero_for_body_itself() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.update_layout_rects([(nid, [50.0, 60.0, 800.0, 600.0])].into_iter().collect());
    let left = rt.eval("document.body.offsetLeft").unwrap();
    assert_eq!(left, lumen_core::JsValue::Number(0.0));
    let top = rt.eval("document.body.offsetTop").unwrap();
    assert_eq!(top, lumen_core::JsValue::Number(0.0));
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

// ── BUG-479: `scroll()` alias, `scrollIntoView` options, Promise return ────

#[test]
fn element_scroll_is_alias_for_scroll_to() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.update_scroll_states([(nid, [0.0, 0.0, 800.0, 2000.0])].into_iter().collect());
    rt.eval("document.body.scroll(100, 200)").unwrap();
    let reqs = rt.take_scroll_requests();
    assert_eq!(reqs.len(), 1);
    assert!((reqs[0].1 - 100.0).abs() < 0.1);
    assert!((reqs[0].2 - 200.0).abs() < 0.1);
}

#[test]
fn scroll_to_and_scroll_by_return_a_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.update_scroll_states([(nid, [0.0, 0.0, 800.0, 2000.0])].into_iter().collect());
    let v = rt
        .eval("document.body.scrollTo(1, 1) instanceof Promise")
        .unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
    let v = rt
        .eval("document.body.scrollBy(1, 1) instanceof Promise")
        .unwrap();
    assert_eq!(v, lumen_core::JsValue::Bool(true));
}

/// The promise settles once the queued request's own `scrollend` lands —
/// not before (BUG-479: previously there was no promise to observe at all).
#[test]
fn element_scroll_to_promise_resolves_on_scrollend() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    rt.update_scroll_states([(nid, [0.0, 0.0, 800.0, 2000.0])].into_iter().collect());
    rt.eval(
        "var __r = 'pending'; \
         document.body.scrollTo(100, 200).then(function() { __r = 'resolved'; });",
    )
    .unwrap();
    assert_eq!(rt.eval("__r").unwrap(), lumen_core::JsValue::String("pending".into()));
    rt.fire_element_scrollend(nid);
    assert_eq!(rt.eval("__r").unwrap(), lumen_core::JsValue::String("resolved".into()));
}

/// A no-op scroll (nothing to move — no scroll container at all here) must
/// not hang its promise forever: native only fires `scroll`/`scrollend` for
/// an actual position change, so the returned promise falls back to
/// resolving once it observes the sampled position hasn't moved across one
/// rendering-update round trip (`_lumen_scroll_settle_promise`).
#[test]
fn element_scroll_to_promise_resolves_via_raf_fallback_when_nothing_moves() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var __r = 'pending'; \
         document.body.scrollTo(0, 0).then(function() { __r = 'resolved'; });",
    )
    .unwrap();
    assert_eq!(rt.eval("__r").unwrap(), lumen_core::JsValue::String("pending".into()));
    rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
    assert_eq!(rt.eval("__r").unwrap(), lumen_core::JsValue::String("pending".into()), "one frame is not enough");
    rt.eval("_lumen_run_raf_callbacks(16)").unwrap();
    assert_eq!(rt.eval("__r").unwrap(), lumen_core::JsValue::String("resolved".into()));
}

#[test]
fn scroll_into_view_block_end_aligns_bottom_edge() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = match rt.eval("document.getElementById('main').__nid__").unwrap() {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("expected a numeric nid, got {other:?}"),
    };
    let body_nid = {
        let doc = make_doc();
        let doc = doc.lock().unwrap();
        super::super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    // Container (body) viewport-relative box: 0,0 400x300, currently at
    // scroll (0, 50). Target sits at container-relative (0, 500), 20px tall
    // — content-space Y = 500 + 50 = 550, so 'end' should land the container
    // at scroll_y = 550 + 20 - 300 = 270.
    rt.update_scroll_states([(body_nid, [0.0, 50.0, 400.0, 3000.0])].into_iter().collect());
    let mut rects = std::collections::HashMap::new();
    rects.insert(body_nid, [0.0_f32, 0.0, 400.0, 300.0]);
    rects.insert(nid, [0.0_f32, 500.0, 100.0, 20.0]);
    rt.update_layout_rects(rects);
    rt.eval("document.getElementById('main').scrollIntoView({block: 'end'})").unwrap();
    let reqs = rt.take_scroll_requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].0, body_nid);
    assert!((reqs[0].2 - 270.0).abs() < 0.1, "expected scroll_y 270, got {}", reqs[0].2);
}

#[test]
fn scroll_into_view_invalid_block_throws_type_error() {
    let rt = v8_runtime_with_dom(make_doc());
    let err = rt
        .eval("document.getElementById('main').scrollIntoView({block: 'bogus'})")
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("not a valid enum value"),
        "expected the ScrollLogicalPosition enum-validation error, got {err:?}"
    );
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
