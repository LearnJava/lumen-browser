//! BUG-873: DOM §2.9 «dispatching events» — one event path walked in three
//! phases. Before the fix there were two independent and both-incomplete
//! walks: the script one (`_lumen_dispatch`, a single node) and the native
//! one (`_lumen_dispatch_bubble`, ancestors plus `document`); neither ran a
//! capture phase, neither reached `window`, and `eventPhase` was `undefined`.
//!
//! The shape these tests guard is the one the E2E track measured on a live
//! React 18 build: a framework hangs its listeners on the hydration ROOT and
//! routes them to components itself, so a `dispatchEvent` that stops at the
//! target element is indistinguishable from no input at all.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

/// `outer > inner`, both in the live document, plus a `log` array every
/// listener below appends to. `window.log.join(',')` is then the whole
/// observable order of one dispatch in a single string.
const NEST: &str = "window.log = []; \
     document.body.innerHTML = '<div id=\"outer\"><div id=\"inner\"></div></div>'; \
     var outer = document.getElementById('outer'); \
     var inner = document.getElementById('inner'); \
     function mark(tag) { return function(e) { window.log.push(tag); }; } ";

fn log_of(rt: &V8JsRuntime) -> String {
    match rt.eval("window.log.join(',')").unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected a string log, got {other:?}"),
    }
}

#[test]
fn dispatch_event_bubbles_to_every_ancestor() {
    // The core of BUG-873: `inner.dispatchEvent(new Event(t, {bubbles: true}))`
    // used to be heard by `inner` alone.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         inner.addEventListener('t', mark('inner')); \
         outer.addEventListener('t', mark('outer')); \
         document.body.addEventListener('t', mark('body')); \
         document.addEventListener('t', mark('document')); \
         window.addEventListener('t', mark('window')); \
         inner.dispatchEvent(new Event('t', {{bubbles: true}}));"
    ))
    .unwrap();
    assert_eq!(log_of(&rt), "inner,outer,body,document,window");
}

#[test]
fn capture_listeners_run_root_first_before_the_target() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         window.addEventListener('t', mark('win-cap'), true); \
         document.addEventListener('t', mark('doc-cap'), true); \
         outer.addEventListener('t', mark('outer-cap'), {{capture: true}}); \
         inner.addEventListener('t', mark('inner')); \
         outer.addEventListener('t', mark('outer-bubble')); \
         inner.dispatchEvent(new Event('t', {{bubbles: true}}));"
    ))
    .unwrap();
    assert_eq!(
        log_of(&rt),
        "win-cap,doc-cap,outer-cap,inner,outer-bubble"
    );
}

#[test]
fn capture_reaches_ancestors_even_for_a_non_bubbling_event() {
    // DOM §2.9: the capture phase does not consult `bubbles`; only the walk
    // back up does.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         outer.addEventListener('t', mark('outer-cap'), true); \
         outer.addEventListener('t', mark('outer-bubble')); \
         inner.addEventListener('t', mark('inner')); \
         inner.dispatchEvent(new Event('t'));"
    ))
    .unwrap();
    assert_eq!(log_of(&rt), "outer-cap,inner");
}

#[test]
fn event_phase_is_reported_at_each_hop() {
    // `e.eventPhase` was `undefined` at every hop before BUG-873.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         outer.addEventListener('t', function(e) {{ window.log.push('cap' + e.eventPhase); }}, true); \
         inner.addEventListener('t', function(e) {{ window.log.push('at' + e.eventPhase); }}); \
         outer.addEventListener('t', function(e) {{ window.log.push('bub' + e.eventPhase); }}); \
         inner.dispatchEvent(new Event('t', {{bubbles: true}}));"
    ))
    .unwrap();
    assert_eq!(log_of(&rt), "cap1,at2,bub3");
    // NONE again once the dispatch is over.
    let after = rt
        .eval("var e = new Event('t'); inner.dispatchEvent(e); e.eventPhase")
        .unwrap();
    assert_eq!(after, lumen_core::JsValue::Number(0.0));
}

#[test]
fn current_target_follows_the_path_while_target_stays_put() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         function probe(e) {{ \
             window.log.push((e.currentTarget === window ? 'window' : e.currentTarget.id || 'document') \
                 + '/' + e.target.id); \
         }} \
         inner.addEventListener('t', probe); \
         outer.addEventListener('t', probe); \
         document.addEventListener('t', probe); \
         window.addEventListener('t', probe); \
         inner.dispatchEvent(new Event('t', {{bubbles: true}}));"
    ))
    .unwrap();
    assert_eq!(
        log_of(&rt),
        "inner/inner,outer/inner,document/inner,window/inner"
    );
    // …and is cleared when the dispatch ends.
    let after = rt
        .eval("var e = new Event('t'); inner.dispatchEvent(e); e.currentTarget")
        .unwrap();
    assert_eq!(after, lumen_core::JsValue::Null);
}

#[test]
fn stop_propagation_ends_the_path_after_the_current_object() {
    // Both listeners on `outer` must still run — the spec stops the path
    // between objects, not between listeners of one object.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         inner.addEventListener('t', mark('inner')); \
         outer.addEventListener('t', function(e) {{ window.log.push('outer-a'); e.stopPropagation(); }}); \
         outer.addEventListener('t', mark('outer-b')); \
         document.addEventListener('t', mark('document')); \
         inner.dispatchEvent(new Event('t', {{bubbles: true}}));"
    ))
    .unwrap();
    assert_eq!(log_of(&rt), "inner,outer-a,outer-b");
}

#[test]
fn stop_immediate_propagation_ends_the_current_object_too() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         outer.addEventListener('t', function(e) {{ window.log.push('outer-a'); e.stopImmediatePropagation(); }}); \
         outer.addEventListener('t', mark('outer-b')); \
         document.addEventListener('t', mark('document')); \
         inner.dispatchEvent(new Event('t', {{bubbles: true}}));"
    ))
    .unwrap();
    assert_eq!(log_of(&rt), "outer-a");
}

#[test]
fn stop_propagation_during_capture_never_reaches_the_target() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         outer.addEventListener('t', function(e) {{ window.log.push('cap'); e.stopPropagation(); }}, true); \
         inner.addEventListener('t', mark('inner')); \
         inner.dispatchEvent(new Event('t', {{bubbles: true}}));"
    ))
    .unwrap();
    assert_eq!(log_of(&rt), "cap");
}

#[test]
fn event_dispatched_at_the_document_reaches_window_and_carries_a_target() {
    // BUG-873's `bubble-to-window` variant reported both halves: the event
    // never arrived at `window`, and `e.target` read back as `null`.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         document.addEventListener('t', function(e) {{ window.log.push('doc/' + (e.target === document)); }}); \
         window.addEventListener('t', mark('window')); \
         document.dispatchEvent(new Event('t', {{bubbles: true}}));"
    ))
    .unwrap();
    assert_eq!(log_of(&rt), "doc/true,window");
}

#[test]
fn native_input_path_reaches_window_as_well() {
    // `_lumen_dispatch_bubble` is what the shell drives for a real click; it
    // used to end at `document`.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         document.addEventListener('click', mark('document')); \
         window.addEventListener('click', mark('window')); \
         _lumen_dispatch_bubble(inner.__nid__, 'click');"
    ))
    .unwrap();
    assert_eq!(log_of(&rt), "document,window");
}

#[test]
fn non_bubbling_event_does_not_reach_document_listeners() {
    // The other half of the same defect: the old walk ran the document's
    // listeners unconditionally, so `bubbles: false` still reached them.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         inner.addEventListener('t', mark('inner')); \
         document.addEventListener('t', mark('document')); \
         window.addEventListener('t', mark('window')); \
         inner.dispatchEvent(new Event('t'));"
    ))
    .unwrap();
    assert_eq!(log_of(&rt), "inner");
}

#[test]
fn detached_subtree_does_not_reach_the_document() {
    // DOM §2.9 builds the path out of the target's own root, which for a node
    // the page has not inserted is that detached node itself.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         var loose = document.createElement('div'); \
         loose.addEventListener('t', mark('loose')); \
         document.addEventListener('t', mark('document')); \
         loose.dispatchEvent(new Event('t', {{bubbles: true}}));"
    ))
    .unwrap();
    assert_eq!(log_of(&rt), "loose");
}

#[test]
fn on_handler_at_an_ancestor_fires_on_a_script_dispatch() {
    // BUG-360's `on<type>` half must ride the same path — a delegated
    // `outer.onclick` is as common as `addEventListener`.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         outer.onclick = mark('outer-onclick'); \
         inner.dispatchEvent(new MouseEvent('click', {{bubbles: true}}));"
    ))
    .unwrap();
    assert_eq!(log_of(&rt), "outer-onclick");
}

#[test]
fn composed_path_lists_the_whole_chain_and_is_empty_outside_dispatch() {
    // BUG-577: `composedPath()` did not exist at all. It is the same path this
    // dispatch already builds, which is why it lands here and not separately.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         inner.addEventListener('t', function(e) {{ \
             window.log.push(e.composedPath().map(function(o) {{ \
                 return o === window ? 'window' : (o === document ? 'document' : (o.id || o.tagName)); \
             }}).join('>')); \
         }}); \
         window.loose = new Event('t'); \
         window.log.push('outside:' + window.loose.composedPath().length); \
         inner.dispatchEvent(new Event('t', {{bubbles: true}}));"
    ))
    .unwrap();
    assert_eq!(
        log_of(&rt),
        "outside:0,inner>outer>BODY>HTML>document>window"
    );
}

#[test]
fn event_phase_constants_are_exposed() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "[Event.NONE, Event.CAPTURING_PHASE, Event.AT_TARGET, Event.BUBBLING_PHASE, \
              new Event('t').AT_TARGET].join(',')",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::String("0,1,2,3,2".into()));
}

#[test]
fn remove_event_listener_honours_the_capture_flag() {
    // A capture listener and a bubble listener of the same callback are two
    // distinct registrations; removing one must not touch the other.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         var fn = mark('outer'); \
         outer.addEventListener('t', fn, true); \
         outer.addEventListener('t', fn); \
         outer.removeEventListener('t', fn); \
         inner.dispatchEvent(new Event('t', {{bubbles: true}}));"
    ))
    .unwrap();
    assert_eq!(log_of(&rt), "outer");
}

#[test]
fn gc_collect_clears_capture_listeners_too() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(&format!(
        "{NEST} \
         outer.addEventListener('t', mark('outer-cap'), true); \
         _lumen_gc_collect([outer.__nid__]); \
         inner.dispatchEvent(new Event('t', {{bubbles: true}}));"
    ))
    .unwrap();
    assert_eq!(log_of(&rt), "");
}
