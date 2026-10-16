//! COMPAT-CSS-JS: `webkit*` aliases of desktop Chrome (Mozilla 1170774) —
//! fullscreen, `webkitURL`, `webkitMatchesSelector`, `webkitRequestAnimationFrame`
//! and the prefixed transition/animation event names.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn truthy(rt: &V8JsRuntime, script: &str) -> bool {
    rt.eval(script).unwrap() == lumen_core::JsValue::Bool(true)
}

#[test]
fn document_fullscreen_aliases() {
    let rt = rt();
    assert!(truthy(&rt, "typeof document.webkitExitFullscreen === 'function'"));
    assert!(truthy(&rt, "typeof document.webkitCancelFullScreen === 'function'"));
    assert!(truthy(&rt, "document.webkitFullscreenElement === null"));
    assert!(truthy(&rt, "document.webkitCurrentFullScreenElement === null"));
    assert!(truthy(&rt, "document.webkitFullscreenEnabled === true"));
    assert!(truthy(&rt, "document.webkitIsFullScreen === false"));
    rt.eval("_lumen_dispatch_mouse_event(_lumen_root_nid, 'mousedown', 0, 0, 0, 1, 0);").unwrap();
    rt.eval("document.body.webkitRequestFullscreen();").unwrap();
    assert!(truthy(&rt, "document.webkitFullscreenElement !== null && document.webkitFullscreenElement.tagName === 'BODY'"));
    assert!(truthy(&rt, "document.webkitIsFullScreen === true"));
    rt.eval("document.webkitExitFullscreen();").unwrap();
    assert!(truthy(&rt, "document.fullscreenElement === null"));
}

#[test]
fn element_aliases() {
    let rt = rt();
    assert!(truthy(&rt, "typeof Element.prototype.webkitRequestFullScreen === 'function'"));
    assert!(truthy(&rt, "document.getElementById('main').webkitMatchesSelector('div')"));
    assert!(truthy(&rt, "!document.getElementById('main').webkitMatchesSelector('span')"));
}

#[test]
fn window_aliases() {
    let rt = rt();
    assert!(truthy(&rt, "webkitURL === URL && window.webkitURL === URL"));
    assert!(truthy(&rt, "typeof webkitRequestAnimationFrame === 'function'"));
    assert!(truthy(&rt, "typeof webkitCancelAnimationFrame === 'function'"));
    assert!(truthy(&rt, "webkitRequestAnimationFrame(function(){}) > 0"));
}

#[test]
fn prefixed_transition_event_listener_fires_once() {
    let rt = rt();
    rt.eval(
        "var el = document.getElementById('main'); var a = 0, b = 0; \
         el.addEventListener('webkitTransitionEnd', function() { a++; }); \
         el.dispatchEvent(new TransitionEvent('transitionend', {propertyName: 'opacity'}));",
    )
    .unwrap();
    assert!(truthy(&rt, "a === 1"));
    // With a standard listener present the prefixed one stays silent.
    rt.eval(
        "el.addEventListener('transitionend', function(e) { b++; }); \
         el.dispatchEvent(new TransitionEvent('transitionend', {propertyName: 'opacity'}));",
    )
    .unwrap();
    assert!(truthy(&rt, "a === 1 && b === 1"));
}

#[test]
fn prefixed_animation_event_listener_fires() {
    let rt = rt();
    rt.eval(
        "var el = document.getElementById('main'); var n = 0; \
         el.addEventListener('webkitAnimationEnd', function(e) { n++; }); \
         el.dispatchEvent(new AnimationEvent('animationend', {animationName: 'x'}));",
    )
    .unwrap();
    assert!(truthy(&rt, "n === 1"));
}
