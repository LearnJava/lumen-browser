//! BUG-688 — Touch Events L2 `Touch`/`TouchList`/`TouchEvent`. The three
//! globals were missing outright (`Touch is not defined`), while elements
//! alone answered `'ontouchstart' in el` — a touch-device signal `window` and
//! `document` never gave.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

/// Evaluates `expr`; an exception comes back as `THROW:<name>`.
fn s(rt: &V8JsRuntime, expr: &str) -> String {
    let wrapped = format!(
        "String((function(){{ try {{ return {expr}; }} \
         catch (e) {{ return 'THROW:' + e.name; }} }})())"
    );
    match rt.eval(&wrapped) {
        Ok(lumen_core::JsValue::String(v)) => v,
        other => format!("{other:?}"),
    }
}

#[test]
fn touch_constructor_requires_identifier_and_event_target() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var el = document.getElementById('main');").unwrap();
    for bad in [
        "new Touch()",
        "new Touch(null)",
        "new Touch({})",
        "new Touch({identifier: 0})",
        "new Touch({target: el})",
        "new Touch({identifier: 0, target: null})",
        "new Touch({identifier: 0, target: location})",
        "new Touch({identifier: 0, target: el, clientX: NaN})",
        "new Touch({identifier: 0, target: el, touchType: 'finger'})",
        "Touch({identifier: 0, target: el})",
    ] {
        assert_eq!(s(&rt, bad), "THROW:TypeError", "{bad}");
    }
}

#[test]
fn touch_carries_init_values_and_defaults() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var el = document.getElementById('main'); \
         var t = new Touch({identifier: 42, target: el, pageX: 15, clientY: 5.5});",
    )
    .unwrap();
    assert_eq!(
        s(&rt, "[t.identifier, t.target === el, t.pageX, t.clientY, t.screenX, t.force, t.touchType].join()"),
        "42,true,15,5.5,0,0,direct"
    );
    assert_eq!(s(&rt, "Object.prototype.toString.call(t)"), "[object Touch]");
    // Attributes live on the prototype, and no legacy `webkit*` alias exists.
    assert_eq!(s(&rt, "t.hasOwnProperty('identifier')"), "false");
    assert_eq!(s(&rt, "'webkitForce' in t"), "false");
    assert_eq!(s(&rt, "Object.getOwnPropertyDescriptor(Touch.prototype, 'force').get.call({})"), "THROW:TypeError");
    // A document target is an EventTarget too.
    assert_eq!(s(&rt, "new Touch({identifier: 1, target: document}).target === document"), "true");
}

#[test]
fn touch_event_builds_touch_lists() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var el = document.getElementById('main'); \
         var a = new Touch({identifier: 1, target: el}); \
         var b = new Touch({identifier: 2, target: el}); \
         var ev = new TouchEvent('touchstart', {touches: [a, b], targetTouches: [a], altKey: true, bubbles: true});",
    )
    .unwrap();
    assert_eq!(
        s(&rt, "[ev.type, ev.bubbles, ev.touches.length, ev.touches[1] === b, ev.targetTouches.item(0) === a, \
                ev.changedTouches.length, ev.touches.item(2), ev.altKey, ev.ctrlKey].join()"),
        "touchstart,true,2,true,true,0,,true,false"
    );
    assert_eq!(s(&rt, "Object.prototype.toString.call(ev.touches)"), "[object TouchList]");
    assert_eq!(s(&rt, "[...ev.touches].length"), "2");
    assert_eq!(s(&rt, "[ev.getModifierState('Alt'), ev.getModifierState('Shift')].join()"), "true,false");
    assert_eq!(s(&rt, "ev.getModifierState()"), "THROW:TypeError");
    assert_eq!(s(&rt, "ev instanceof UIEvent && ev instanceof Event"), "true");
    assert_eq!(s(&rt, "new TouchEvent('touchend').touches.length"), "0");
    assert_eq!(s(&rt, "new TouchEvent('x', {touches: [{}]})"), "THROW:TypeError");
    assert_eq!(s(&rt, "new TouchEvent()"), "THROW:TypeError");
    assert_eq!(s(&rt, "new TouchList()"), "THROW:TypeError");
    assert_eq!(s(&rt, "'initTouchEvent' in ev || 'identifiedTouch' in ev.touches"), "false");
}

#[test]
fn touch_globals_are_non_enumerable_and_handlers_stay_hidden() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(&rt, "['Touch', 'TouchList', 'TouchEvent'].map(function(n) { \
                return Object.getOwnPropertyDescriptor(globalThis, n).enumerable; }).join()"),
        "false,false,false"
    );
    // "Expose legacy touch event APIs" is off: no `ontouch*` anywhere.
    assert_eq!(
        s(&rt, "['ontouchstart' in window, 'ontouchstart' in document, \
                'ontouchend' in document.getElementById('main')].join()"),
        "false,false,false"
    );
}
