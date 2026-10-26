//! SMOOTH-SCROLL — CSSOM View «smooth scrolling» of the scroll methods.
//!
//! The engine's scroll requests are instant; a smooth scroll is stepped from
//! the shim (`_lumen_smooth_scroll` / `_lumen_smooth_tick`,
//! `shim/web_api_shim_head.js`). These tests drive the stepper by hand: the
//! animation clock is rewound through the animation record and `_lumen_smooth_tick`
//! is called in place of a `requestAnimationFrame` turn.

use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_core::JsValue;
use lumen_dom::{Document, NodeData, QualName};
use std::collections::HashMap;

/// `html > body > #main > .content`; `#main` is a 20x20 scroll container over
/// 200x200 content. Returns `(doc, main_nid)`.
fn make_doc() -> (Arc<Mutex<Document>>, u32) {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let body = doc.create_element(QualName::html("body"));
    let main = doc.create_element(QualName::html("div"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(main).data {
        attrs.push(lumen_dom::Attribute { name: QualName::html("id"), value: "main".into() });
    }
    let content = doc.create_element(QualName::html("div"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(content).data {
        attrs.push(lumen_dom::Attribute { name: QualName::html("class"), value: "content".into() });
    }
    let target = doc.create_element(QualName::html("span"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(target).data {
        attrs.push(lumen_dom::Attribute { name: QualName::html("id"), value: "target".into() });
    }
    let free = doc.create_element(QualName::html("span"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(free).data {
        attrs.push(lumen_dom::Attribute { name: QualName::html("id"), value: "free".into() });
    }
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    doc.append_child(body, main);
    doc.append_child(main, content);
    doc.append_child(main, target);
    doc.append_child(body, free);
    let main_nid = main.index() as u32;
    (Arc::new(Mutex::new(doc)), main_nid)
}

fn nid_of(rt: &V8JsRuntime, id: &str) -> u32 {
    num(rt, &format!("document.getElementById('{id}').__nid__")) as u32
}

fn runtime(main_style: &str) -> (V8JsRuntime, u32) {
    let (doc, main_nid) = make_doc();
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(&format!(
        "#main {{ width: 20px; height: 20px; overflow: auto; {main_style} }} \
         .content {{ width: 200px; height: 200px; }}"
    ))));
    rt.update_viewport_size(800.0, 600.0);
    // Settle the pending style/layout flush now: the first geometry read would otherwise
    // run it later and replace the rects and scroll states the tests inject below.
    rt.eval("['main', 'target', 'free'].forEach(function(i) { getComputedStyle(document.getElementById(i)).display; })").unwrap();
    let mut states = HashMap::new();
    states.insert(main_nid, [0.0, 0.0, 200.0, 200.0]);
    rt.update_scroll_states(states);
    (rt, main_nid)
}

fn num(rt: &V8JsRuntime, script: &str) -> f64 {
    match rt.eval(script) {
        Ok(JsValue::Number(n)) => n,
        other => panic!("expected number, got {other:?}: {script}"),
    }
}

/// Runs `n` stepper turns with the animation clock `ms` milliseconds past its start.
const TICK_AT: &str = "(function(ms, n) {
    for (var k in _lumen_smooth_anims) { _lumen_smooth_anims[k].t0 = performance.now() - ms; }
    for (var i = 0; i < n; i++) _lumen_smooth_tick();
})";

#[test]
fn smooth_behavior_does_not_move_at_the_call() {
    let (rt, _) = runtime("");
    assert_eq!(
        num(&rt, "var m = document.getElementById('main'); m.scrollTo({top: 100, behavior: 'smooth'}); m.scrollTop"),
        0.0
    );
    assert!(rt.take_scroll_requests().is_empty(), "the call itself queues no instant request");
}

#[test]
fn instant_behavior_beats_css_smooth() {
    let (rt, _) = runtime("scroll-behavior: smooth;");
    assert_eq!(
        num(&rt, "var m = document.getElementById('main'); m.scrollTo({top: 100, behavior: 'instant'}); m.scrollTop"),
        100.0
    );
    assert_eq!(rt.take_scroll_requests().len(), 1);
}

#[test]
fn auto_behavior_follows_the_boxs_own_scroll_behavior() {
    let (rt, _) = runtime("scroll-behavior: smooth;");
    assert_eq!(num(&rt, "var m = document.getElementById('main'); m.scrollTo(0, 100); m.scrollTop"), 0.0);
    let (rt, _) = runtime("");
    assert_eq!(num(&rt, "var m = document.getElementById('main'); m.scrollTo(0, 100); m.scrollTop"), 100.0);
}

#[test]
fn scroll_top_setter_uses_auto_behavior() {
    let (rt, _) = runtime("scroll-behavior: smooth;");
    assert_eq!(num(&rt, "var m = document.getElementById('main'); m.scrollTop = 60; m.scrollTop"), 0.0);
}

#[test]
fn stepper_walks_to_the_target_and_pays_one_scrollend() {
    let (rt, nid) = runtime("");
    rt.eval(
        "var ends = 0; var m = document.getElementById('main');
         m.addEventListener('scrollend', function() { ends++; });
         m.scrollTo({top: 100, behavior: 'smooth'});",
    )
    .unwrap();
    // Halfway: moved, but the native `scrollend` of the step is held back.
    rt.eval(&format!("{TICK_AT}(100, 1)")).unwrap();
    let mid = num(&rt, "m.scrollTop");
    assert!(mid > 0.0 && mid < 100.0, "mid-flight position {mid}");
    rt.eval(&format!("_lumen_fire_scrollend_on_element({nid})")).unwrap();
    assert_eq!(num(&rt, "ends"), 0.0, "native scrollend of an intermediate step is suppressed");
    // Past the end: the last step lands on the target exactly.
    rt.eval(&format!("{TICK_AT}(10000, 1)")).unwrap();
    assert_eq!(num(&rt, "m.scrollTop"), 100.0);
    assert_eq!(num(&rt, "ends"), 0.0, "scrollend waits for the shell to apply the last step");
    rt.eval("_lumen_smooth_tick(); _lumen_smooth_tick();").unwrap();
    assert_eq!(num(&rt, "ends"), 1.0, "exactly one scrollend at the end of the sequence");
    rt.eval("_lumen_smooth_tick(); _lumen_smooth_tick();").unwrap();
    assert_eq!(num(&rt, "ends"), 1.0);
    assert_eq!(num(&rt, "Object.keys(_lumen_smooth_anims).length"), 0.0);
}

#[test]
fn same_target_does_not_restart_the_curve() {
    let (rt, _) = runtime("");
    rt.eval("var m = document.getElementById('main'); m.scrollTo({top: 100, behavior: 'smooth'});").unwrap();
    rt.eval(&format!("{TICK_AT}(120, 1)")).unwrap();
    let before = num(&rt, "m.scrollTop");
    let t0 = num(&rt, "_lumen_smooth_anims[m.__nid__].t0");
    // A caller re-issuing the same scroll every timer tick (WPT smooth-scroll-nonstop).
    rt.eval("m.scrollTo({top: 100, behavior: 'smooth'}); m.scrollTo({top: 100.4, behavior: 'smooth'});").unwrap();
    assert_eq!(num(&rt, "_lumen_smooth_anims[m.__nid__].t0"), t0, "still the same animation");
    assert_eq!(num(&rt, "m.scrollTop"), before);
}

#[test]
fn new_target_restarts_from_the_current_position() {
    let (rt, _) = runtime("");
    rt.eval("var m = document.getElementById('main'); m.scrollTo({top: 100, behavior: 'smooth'});").unwrap();
    rt.eval(&format!("{TICK_AT}(100, 1)")).unwrap();
    let mid = num(&rt, "m.scrollTop");
    rt.eval("m.scrollTo({top: 20, behavior: 'smooth'});").unwrap();
    assert_eq!(num(&rt, "_lumen_smooth_anims[m.__nid__].sy"), mid);
    assert_eq!(num(&rt, "_lumen_smooth_anims[m.__nid__].ty"), 20.0);
}

#[test]
fn instant_request_cancels_the_animation() {
    let (rt, _) = runtime("");
    rt.eval("var m = document.getElementById('main'); m.scrollTo({top: 100, behavior: 'smooth'});").unwrap();
    assert_eq!(num(&rt, "Object.keys(_lumen_smooth_anims).length"), 1.0);
    assert_eq!(num(&rt, "m.scrollTo({top: 10, behavior: 'instant'}); m.scrollTop"), 10.0);
    assert_eq!(num(&rt, "Object.keys(_lumen_smooth_anims).length"), 0.0);
}

#[test]
fn someone_elses_movement_ends_the_animation_in_place() {
    let (rt, nid) = runtime("");
    rt.eval(
        "var ends = 0; var m = document.getElementById('main');
         m.addEventListener('scrollend', function() { ends++; });
         m.scrollTo({top: 100, behavior: 'smooth'});",
    )
    .unwrap();
    rt.eval(&format!("{TICK_AT}(60, 1)")).unwrap();
    // A wheel step lands somewhere the stepper never asked for.
    let mut states = HashMap::new();
    states.insert(nid, [0.0, 77.0, 200.0, 200.0]);
    rt.update_scroll_states(states);
    rt.eval("_lumen_smooth_tick(); _lumen_smooth_tick(); _lumen_smooth_tick();").unwrap();
    assert_eq!(num(&rt, "m.scrollTop"), 77.0, "the stepper stops pushing the container");
    assert_eq!(num(&rt, "ends"), 1.0);
}

#[test]
fn invalid_behavior_is_a_type_error() {
    let (rt, _) = runtime("");
    let r = rt.eval("try { document.getElementById('main').scrollTo({top: 1, behavior: 'fast'}); 'no' } catch (e) { e.name }");
    assert_eq!(r.unwrap(), JsValue::String("TypeError".into()));
}

#[test]
fn omitted_axis_keeps_its_position() {
    let (rt, _) = runtime("");
    assert_eq!(
        num(&rt, "var m = document.getElementById('main'); m.scrollTo(40, 30); m.scrollTo({top: 70}); m.scrollLeft"),
        40.0
    );
}

#[test]
fn computed_scroll_behavior_is_exposed_and_not_inherited() {
    let (rt, _) = runtime("scroll-behavior: smooth;");
    assert_eq!(
        rt.eval("getComputedStyle(document.getElementById('main')).scrollBehavior").unwrap(),
        JsValue::String("smooth".into())
    );
    assert_eq!(
        rt.eval("getComputedStyle(document.querySelector('.content')).scrollBehavior").unwrap(),
        JsValue::String("auto".into())
    );
}

#[test]
fn page_scroll_x_round_trips() {
    let (rt, _) = runtime("");
    rt.set_page_scroll_x(30.0);
    assert_eq!(num(&rt, "window.scrollX"), 30.0);
    assert_eq!(num(&rt, "window.pageXOffset"), 30.0);
    rt.eval("window.scrollTo(40, 50)").unwrap();
    assert_eq!(num(&rt, "window.scrollX"), 40.0, "an instant request is visible at once");
    assert_eq!(rt.take_page_scroll_x_request(), Some(40.0));
    assert_eq!(rt.take_page_scroll_requests(), vec![(50.0, false)]);
    assert_eq!(rt.take_page_scroll_x_request(), None);
    assert_eq!(num(&rt, "window.scrollX"), 30.0, "drained: back to the applied position");
    assert!(rt.set_page_scroll_x(35.0));
    assert!(!rt.set_page_scroll_x(35.0));
}

#[test]
fn window_scroll_to_keeps_an_omitted_axis() {
    let (rt, _) = runtime("");
    rt.set_page_scroll_x(30.0);
    rt.set_page_scroll_y(70.0);
    rt.eval("window.scrollTo({top: 90})").unwrap();
    assert_eq!(rt.take_page_scroll_x_request(), Some(30.0));
    assert_eq!(rt.take_page_scroll_requests(), vec![(90.0, false)]);
}

#[test]
fn root_scroller_is_the_viewport() {
    let (rt, _) = runtime("");
    rt.set_page_scroll_x(12.0);
    rt.set_page_scroll_y(34.0);
    assert_eq!(num(&rt, "document.scrollingElement.scrollLeft"), 12.0);
    assert_eq!(num(&rt, "document.scrollingElement.scrollTop"), 34.0);
    assert_eq!(num(&rt, "document.scrollingElement.clientWidth"), 800.0);
    assert_eq!(num(&rt, "document.scrollingElement.clientHeight"), 600.0);
    rt.eval("document.scrollingElement.scrollTo(5, 6)").unwrap();
    assert_eq!(rt.take_page_scroll_x_request(), Some(5.0));
    assert_eq!(rt.take_page_scroll_requests(), vec![(6.0, false)]);
}

/// `_lumen_get_bounding_rect` answers in document coordinates with no ancestor
/// scroll applied (BUG-1166): the difference of two rects is the target's
/// offset in the container's unscrolled content, whatever the container's
/// current scroll is.
#[test]
fn scroll_into_view_in_a_scrolled_container_does_not_drift() {
    let (rt, main_nid) = runtime("");
    let target = nid_of(&rt, "target");
    rt.update_layout_rects(
        [(main_nid, [0.0, 0.0, 20.0, 20.0]), (target, [0.0, 100.0, 10.0, 10.0])].into_iter().collect(),
    );
    // Already scrolled to 50: the answer for `start` is still 100, not 150.
    let mut states = HashMap::new();
    states.insert(main_nid, [0.0, 50.0, 200.0, 200.0]);
    rt.update_scroll_states(states);
    rt.eval("document.getElementById('target').scrollIntoView({block: 'start', behavior: 'instant'})").unwrap();
    assert_eq!(num(&rt, "document.getElementById('main').scrollTop"), 100.0);
    assert_eq!(rt.take_scroll_requests(), vec![(main_nid, 0.0, 100.0)]);
}

#[test]
fn smooth_scroll_into_view_retargets_to_the_same_place() {
    let (rt, main_nid) = runtime("");
    let target = nid_of(&rt, "target");
    rt.update_layout_rects(
        [(main_nid, [0.0, 0.0, 20.0, 20.0]), (target, [0.0, 100.0, 10.0, 10.0])].into_iter().collect(),
    );
    rt.eval(
        "var t = document.getElementById('target'), m = document.getElementById('main');
         t.scrollIntoView({block: 'start', behavior: 'smooth'});",
    )
    .unwrap();
    rt.eval(&format!("{TICK_AT}(100, 1)")).unwrap();
    let t0 = num(&rt, "_lumen_smooth_anims[m.__nid__].t0");
    // The caller asks again mid-flight (WPT smooth-scroll-nonstop, scrollIntoView half).
    rt.eval("t.scrollIntoView({block: 'start', behavior: 'smooth'})").unwrap();
    assert_eq!(num(&rt, "_lumen_smooth_anims[m.__nid__].t0"), t0, "same target: the curve is not restarted");
    assert_eq!(num(&rt, "_lumen_smooth_anims[m.__nid__].ty"), 100.0);
}

#[test]
fn page_scroll_into_view_uses_document_coordinates_on_both_axes() {
    let (rt, main_nid) = runtime("");
    let free = nid_of(&rt, "free");
    rt.update_layout_rects(
        [(main_nid, [0.0, 0.0, 20.0, 20.0]), (free, [30.0, 500.0, 10.0, 10.0])].into_iter().collect(),
    );
    rt.set_page_scroll_y(200.0);
    rt.eval("document.getElementById('free').scrollIntoView({block: 'start', inline: 'start', behavior: 'instant'})").unwrap();
    assert_eq!(rt.take_page_scroll_x_request(), Some(30.0));
    assert_eq!(rt.take_page_scroll_requests(), vec![(500.0, false)]);
}

#[test]
fn scroll_on_an_element_without_a_scrolling_box_does_nothing() {
    let (rt, _) = runtime("");
    assert_eq!(num(&rt, "var f = document.getElementById('free'); f.scrollTo(1, 2); f.scrollTop"), 0.0);
    assert_eq!(num(&rt, "f.scrollTop = 5; f.scrollLeft"), 0.0);
    assert!(rt.take_scroll_requests().is_empty());
}

// ── BUG-1166 / ANCHOR-SCROLL-2: scroll-aware `getBoundingClientRect` ──────────────────────────

fn scrolled_main(rt: &V8JsRuntime, main_nid: u32, y: f32) {
    let target = nid_of(rt, "target");
    let free = nid_of(rt, "free");
    rt.update_layout_rects(
        [
            (main_nid, [0.0, 10.0, 20.0, 20.0]),
            (target, [0.0, 110.0, 10.0, 10.0]),
            (free, [30.0, 200.0, 10.0, 10.0]),
        ]
        .into_iter()
        .collect(),
    );
    rt.update_client_rects([(target, vec![[0.0, 110.0, 10.0, 10.0]])].into_iter().collect());
    rt.update_scroll_states([(main_nid, [0.0, y, 200.0, 200.0])].into_iter().collect());
}

#[test]
fn bounding_rect_of_a_descendant_follows_the_scroll_of_its_container() {
    let (rt, main_nid) = runtime("");
    scrolled_main(&rt, main_nid, 50.0);
    let top = |id: &str| num(&rt, &format!("document.getElementById('{id}').getBoundingClientRect().top"));
    assert_eq!(top("target"), 60.0, "110 in the document, 50 scrolled away");
    assert_eq!(top("main"), 10.0, "the container itself does not move");
    assert_eq!(top("free"), 200.0, "a box outside the container does not move");
    assert_eq!(num(&rt, "document.getElementById('target').getClientRects()[0].top"), 60.0);
}

#[test]
fn offset_top_ignores_the_scroll_of_the_container() {
    let (rt, main_nid) = runtime("position: relative;");
    scrolled_main(&rt, main_nid, 50.0);
    assert_eq!(num(&rt, "document.getElementById('target').offsetTop"), 100.0);
}

#[test]
fn an_anchor_positioned_box_follows_the_scroll_of_its_anchors_container() {
    let (rt, main_nid) = runtime("");
    scrolled_main(&rt, main_nid, 50.0);
    let free = nid_of(&rt, "free");
    // `#free` is positioned against an anchor inside `#main`, vertical axis only.
    rt.update_anchor_scrollers([(free, vec![(main_nid, false, true)])].into_iter().collect());
    assert_eq!(num(&rt, "document.getElementById('free').getBoundingClientRect().top"), 150.0);
    assert_eq!(num(&rt, "document.getElementById('free').getBoundingClientRect().left"), 30.0);
    assert_eq!(num(&rt, "document.getElementById('free').offsetTop"), 150.0, "the anchor's scroll is layout offset");
    rt.update_anchor_scrollers([(free, vec![(main_nid, true, false)])].into_iter().collect());
    assert_eq!(num(&rt, "document.getElementById('free').getBoundingClientRect().top"), 200.0, "no vertical axis");
}
