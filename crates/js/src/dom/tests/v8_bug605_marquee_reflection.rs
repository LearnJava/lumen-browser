//! BUG-605 — `<marquee>` had no dedicated `HTMLMarqueeElement` interface:
//! `loop`/`scrollAmount`/`scrollDelay` all read `undefined` instead of
//! reflecting their content attributes with spec defaults, and
//! `HTMLMarqueeElement` was not even a global constructor.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

#[test]
fn marquee_has_global_constructor_with_no_event_handlers() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "typeof HTMLMarqueeElement === 'function'"));
    assert!(is_true(
        &rt,
        "document.createElement('marquee') instanceof HTMLMarqueeElement"
    ));
    assert!(is_true(&rt, "!('onstart' in HTMLMarqueeElement.prototype)"));
    assert!(is_true(&rt, "!('onfinish' in HTMLMarqueeElement.prototype)"));
    assert!(is_true(&rt, "!('onbounce' in HTMLMarqueeElement.prototype)"));
}

/// `loop` falls back to -1 for a non-numeric attribute, a negative-but-not
/// `-1` value, and reads through a normal positive value verbatim.
#[test]
fn marquee_loop_reflects_with_minus_one_default() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var m1 = document.createElement('marquee'); m1.setAttribute('loop', 'a');")
        .unwrap();
    assert!(is_true(&rt, "m1.loop === -1"));
    rt.eval("var m2 = document.createElement('marquee'); m2.setAttribute('loop', '-2');")
        .unwrap();
    assert!(is_true(&rt, "m2.loop === -1"));
    rt.eval("var m3 = document.createElement('marquee'); m3.setAttribute('loop', '2');")
        .unwrap();
    assert!(is_true(&rt, "m3.loop === 2"));
}

/// `scrollAmount`/`scrollDelay` are plain `unsigned long` reflections
/// (default 6/85, negative or unparseable falls back to the default).
#[test]
fn marquee_scroll_amount_and_delay_reflect_with_defaults() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var m = document.createElement('marquee');").unwrap();
    assert!(is_true(&rt, "m.scrollAmount === 6"));
    assert!(is_true(&rt, "m.scrollDelay === 85"));

    rt.eval("m.setAttribute('scrollamount', 'aa');").unwrap();
    assert!(is_true(&rt, "m.scrollAmount === 6"));
    rt.eval("m.setAttribute('scrollamount', '-1');").unwrap();
    assert!(is_true(&rt, "m.scrollAmount === 6"));
    rt.eval("m.setAttribute('scrollamount', '10');").unwrap();
    assert!(is_true(&rt, "m.scrollAmount === 10"));

    rt.eval("m.setAttribute('scrolldelay', '-1');").unwrap();
    assert!(is_true(&rt, "m.scrollDelay === 85"));
    rt.eval("m.setAttribute('scrolldelay', '100');").unwrap();
    assert!(is_true(&rt, "m.scrollDelay === 100"));
}
