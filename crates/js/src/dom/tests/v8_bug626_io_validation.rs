//! BUG-626 — `IntersectionObserver` constructor and `observe()` argument
//! validation (Intersection Observer §2.2 + WebIDL conversions). Mirrors WPT
//! `intersection-observer/observer-exceptions.html`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true")
        .unwrap();
    rt.install_dom(
        doc,
        "https://example.test/",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
    )
    .unwrap();
    rt
}

/// Runs `code` and returns the thrown value's `name` (or `"no-throw"`).
fn thrown_name(rt: &V8JsRuntime, code: &str) -> String {
    let wrapped = format!(
        "(function() {{ try {{ {code}; return 'no-throw'; }} catch (e) {{ return e.name; }} }})()"
    );
    match rt.eval(&wrapped).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("unexpected result {other:?} for {code}"),
    }
}

#[test]
fn invalid_threshold_throws() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        thrown_name(
            &rt,
            "new IntersectionObserver(function(){}, {threshold: [1.1]})"
        ),
        "RangeError"
    );
    assert_eq!(
        thrown_name(
            &rt,
            "new IntersectionObserver(function(){}, {threshold: -0.1})"
        ),
        "RangeError"
    );
    assert_eq!(
        thrown_name(
            &rt,
            "new IntersectionObserver(function(){}, {threshold: ['foo']})"
        ),
        "TypeError"
    );
    // The WebIDL conversion (TypeError) precedes the range check (RangeError).
    assert_eq!(
        thrown_name(
            &rt,
            "new IntersectionObserver(function(){}, {threshold: [2, 'foo']})"
        ),
        "TypeError"
    );
    // Numeric strings convert like any double; [0, 1] is inclusive.
    assert_eq!(
        thrown_name(
            &rt,
            "new IntersectionObserver(function(){}, {threshold: ['0.5', 0, 1]})"
        ),
        "no-throw"
    );
}

#[test]
fn invalid_root_margin_throws_syntax_error() {
    let rt = v8_runtime_with_dom(make_doc());
    for bad in [
        "1",
        "2em",
        "auto",
        "calc(1px + 2px)",
        "1px !important",
        "1px 1px 1px 1px 1px",
    ] {
        let code = format!("new IntersectionObserver(function(){{}}, {{rootMargin: '{bad}'}})");
        assert_eq!(thrown_name(&rt, &code), "SyntaxError", "rootMargin: {bad}");
        let code = format!("new IntersectionObserver(function(){{}}, {{scrollMargin: '{bad}'}})");
        assert_eq!(
            thrown_name(&rt, &code),
            "SyntaxError",
            "scrollMargin: {bad}"
        );
    }
    let is_dom_exception = rt
        .eval(
            "(function(){ try { new IntersectionObserver(function(){}, {rootMargin: '2em'}); } \
               catch (e) { return e instanceof DOMException && e.code === 12; } return false; })()",
        )
        .unwrap();
    assert_eq!(is_dom_exception, lumen_core::JsValue::Bool(true));
    assert_eq!(
        thrown_name(
            &rt,
            "new IntersectionObserver(function(){}, {rootMargin: '-5% 10px'})"
        ),
        "no-throw"
    );
}

#[test]
fn bad_callback_and_root_throw_type_error() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(thrown_name(&rt, "new IntersectionObserver()"), "TypeError");
    assert_eq!(
        thrown_name(&rt, "new IntersectionObserver({})"),
        "TypeError"
    );
    assert_eq!(
        thrown_name(&rt, "new IntersectionObserver(function(){}, {root: 'foo'})"),
        "TypeError"
    );
    assert_eq!(
        thrown_name(
            &rt,
            "new IntersectionObserver(function(){}, {root: document})"
        ),
        "no-throw"
    );
    assert_eq!(
        thrown_name(
            &rt,
            "new IntersectionObserver(function(){}, {root: document.body})"
        ),
        "no-throw"
    );
}

#[test]
fn observe_non_element_throws_type_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var o = new IntersectionObserver(function(){}, {});")
        .unwrap();
    assert_eq!(thrown_name(&rt, "o.observe('foo')"), "TypeError");
    assert_eq!(thrown_name(&rt, "o.observe(null)"), "TypeError");
    assert_eq!(thrown_name(&rt, "o.observe(document)"), "TypeError");
    assert_eq!(
        thrown_name(&rt, "o.observe(document.createTextNode('x'))"),
        "TypeError"
    );
    assert_eq!(thrown_name(&rt, "o.observe(document.body)"), "no-throw");
}
