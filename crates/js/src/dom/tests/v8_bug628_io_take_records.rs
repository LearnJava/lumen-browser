//! BUG-628 — `IntersectionObserver.prototype.takeRecords()` and the readonly
//! `root`/`rootMargin`/`scrollMargin`/`thresholds` attributes (Intersection
//! Observer §2.2). Mirrors WPT `intersection-observer/observer-attributes.html`
//! and the `takeRecords()` boilerplate that 49 files of that category call.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

fn body_rt(rect: [f32; 4]) -> V8JsRuntime {
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    let rt = v8_runtime_with_dom(doc_arc);
    rt.update_layout_rects([(nid, rect)].into_iter().collect());
    rt.update_viewport_size(1024.0, 720.0);
    rt
}

#[test]
fn attributes_have_spec_defaults() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var o = new IntersectionObserver(function() {}, {});").unwrap();
    assert!(is_true(&rt, "o.root === null"));
    assert!(is_true(&rt, "o.rootMargin === '0px 0px 0px 0px'"));
    assert!(is_true(&rt, "o.scrollMargin === '0px 0px 0px 0px'"));
    assert!(is_true(&rt, "Array.isArray(o.thresholds) && o.thresholds.length === 1 && o.thresholds[0] === 0"));
    // No options object at all takes the same defaults.
    assert!(is_true(&rt, "new IntersectionObserver(function() {}).rootMargin === '0px 0px 0px 0px'"));
}

#[test]
fn attributes_reflect_and_normalize_options() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
        var rootDiv = document.createElement('div');
        var o = new IntersectionObserver(function() {}, {
            root: rootDiv, threshold: [1.0, 0, 0.5, 0.25], rootMargin: '10% 20px'
        });
        var e = new IntersectionObserver(function() {}, { rootMargin: ' ', threshold: [] });
        var n = new IntersectionObserver(function() {}, { threshold: 0.5, rootMargin: '5px 6px 7px' });
    "#).unwrap();
    assert!(is_true(&rt, "o.root === rootDiv"));
    assert!(is_true(&rt, "o.rootMargin === '10% 20px 10% 20px'"));
    assert!(is_true(&rt, "o.thresholds.join(',') === '0,0.25,0.5,1'"));
    assert!(is_true(&rt, "e.rootMargin === '0px 0px 0px 0px'"));
    assert!(is_true(&rt, "e.thresholds.length === 1 && e.thresholds[0] === 0"));
    assert!(is_true(&rt, "n.thresholds.length === 1 && n.thresholds[0] === 0.5"));
    assert!(is_true(&rt, "n.rootMargin === '5px 6px 7px 6px'"));
}

#[test]
fn attributes_are_readonly_prototype_accessors() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var o = new IntersectionObserver(function() {});").unwrap();
    for name in ["root", "rootMargin", "scrollMargin", "thresholds"] {
        let code = format!(
            "(function() {{ var d = Object.getOwnPropertyDescriptor(IntersectionObserver.prototype, '{name}'); \
             return !!d && typeof d.get === 'function' && d.set === undefined && !o.hasOwnProperty('{name}'); }})()"
        );
        assert!(is_true(&rt, &code), "{name} must be a getter-only prototype accessor");
    }
    // FrozenArray: the same frozen object on every read.
    assert!(is_true(&rt, "o.thresholds === o.thresholds && Object.isFrozen(o.thresholds)"));
    assert!(is_true(&rt, "typeof IntersectionObserver.prototype.takeRecords === 'function'"));
}

#[test]
fn take_records_drains_the_queue_before_the_callback() {
    let rt = body_rt([0.0, 0.0, 100.0, 50.0]);
    rt.eval(r#"
        var _cb_calls = 0;
        var io = new IntersectionObserver(function() { _cb_calls++; });
        var _empty = io.takeRecords();
        io.observe(document.body);
    "#).unwrap();
    assert!(is_true(&rt, "Array.isArray(_empty) && _empty.length === 0"));
    // Put the queue in the state the spec's "update intersection
    // observations" step leaves it in before the notify task runs: capture
    // one pass's entries and queue them back without delivering.
    rt.eval(r#"
        var _real_cb = io._cb, _captured = null;
        io._cb = function(entries) { _captured = entries; };
        _lumen_deliver_intersection_observers();
        io._queuedEntries = _captured;
        io._cb = _real_cb;
        var _taken = io.takeRecords();
        var _again = io.takeRecords();
    "#).unwrap();
    assert!(is_true(&rt, "_taken.length === 1 && _taken[0].isIntersecting === true"));
    assert!(is_true(&rt, "_again.length === 0"));
    // Nothing is left for the next notify step to deliver.
    rt.eval("_lumen_deliver_intersection_observers();").unwrap();
    assert!(is_true(&rt, "_cb_calls === 0"));
}

#[test]
fn take_records_from_another_callback_prevents_double_delivery() {
    let rt = body_rt([0.0, 0.0, 100.0, 50.0]);
    rt.eval(r#"
        var _a_seen = 0, _b_seen = 0, _b_taken = -1;
        var ioB;
        var ioA = new IntersectionObserver(function(entries) {
            _a_seen += entries.length;
            _b_taken = ioB.takeRecords().length;
        });
        ioB = new IntersectionObserver(function(entries) { _b_seen += entries.length; });
        ioA.observe(document.body);
        ioB.observe(document.body);
        _lumen_deliver_intersection_observers();
    "#).unwrap();
    // B's entry was queued before A's callback ran, taken there, and so not
    // delivered to B's own callback afterwards.
    assert!(is_true(&rt, "_a_seen === 1"));
    assert!(is_true(&rt, "_b_taken === 1"));
    assert!(is_true(&rt, "_b_seen === 0"));
}

#[test]
fn percentage_root_margin_resolves_against_viewport() {
    // Target 10px below a 720px viewport; 10% bottom margin = 72px of slack.
    let rt = body_rt([0.0, 730.0, 100.0, 50.0]);
    rt.eval(r#"
        var _pm = null;
        var io = new IntersectionObserver(function(e) { _pm = e[0]; }, { rootMargin: '0px 0px 10% 0px' });
        io.observe(document.body);
        _lumen_deliver_intersection_observers();
    "#).unwrap();
    assert!(is_true(&rt, "_pm !== null && _pm.isIntersecting === true"));
    assert!(is_true(&rt, "_pm.rootBounds.height === 792"));
}

#[test]
fn observe_after_disconnect_delivers_again() {
    let rt = body_rt([0.0, 0.0, 100.0, 50.0]);
    rt.eval(r#"
        var _cnt = 0;
        var t = document.body;
        var io = new IntersectionObserver(function() { _cnt++; });
        io.observe(t);
        io.disconnect();
        io.observe(t);
        _lumen_deliver_intersection_observers();
    "#).unwrap();
    assert!(is_true(&rt, "_cnt === 1"));
}
