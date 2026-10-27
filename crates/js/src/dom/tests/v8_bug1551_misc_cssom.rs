//! BUG-1551: каноническая запись `content`, `font-family`, `font-size`, `list-style-image`,
//! `outline-width`/`outline-color` в `element.style` (`_LUMEN_MISC_PROPERTIES`,
//! `_lumen_css_canonical_misc`).

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn eval_str(rt: &V8JsRuntime, script: &str) -> String {
    match rt.eval(script).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

/// Значение из атрибута `style=""` читается обратно как `style[prop]`.
fn attr(rt: &V8JsRuntime, decl: &str, prop: &str) -> String {
    eval_str(
        rt,
        &format!("var d = document.createElement('div'); d.setAttribute('style', {decl:?}); d.style[{prop:?}]"),
    )
}

/// `style[prop] = val` на свежем элементе; результат чтения обратно.
fn assign(rt: &V8JsRuntime, prop: &str, val: &str) -> String {
    eval_str(
        rt,
        &format!("var d = document.createElement('div'); d.style[{prop:?}] = {val:?}; d.style[{prop:?}]"),
    )
}

#[test]
fn values_are_serialized_canonically() {
    let rt = rt();
    assert_eq!(attr(&rt, "content: 'string'", "content"), "\"string\"");
    assert_eq!(attr(&rt, "content: counter(par-num, decimal)", "content"), "counter(par-num)");
    assert_eq!(attr(&rt, "list-style-image: url(http://localhost/)", "listStyleImage"), "url(\"http://localhost/\")");
    assert_eq!(attr(&rt, "font-family: 'Lucida Grande'", "fontFamily"), "Lucida Grande");
    assert_eq!(attr(&rt, "font-family: '34J', \"serif\"", "fontFamily"), "\"34J\", \"serif\"");
    assert_eq!(attr(&rt, "font-size: .5%", "fontSize"), "0.5%");
    assert_eq!(attr(&rt, "font-size: 10pt", "fontSize"), "10pt");
    assert_eq!(attr(&rt, "line-height: -0", "lineHeight"), "0");
    assert_eq!(attr(&rt, "outline-width: .1em", "outlineWidth"), "0.1em");
    assert_eq!(attr(&rt, "outline-color: invert", "outlineColor"), "invert");
}

#[test]
fn assignment_goes_through_the_same_canon() {
    let rt = rt();
    assert_eq!(assign(&rt, "content", "'a'"), "\"a\"");
    assert_eq!(assign(&rt, "fontFamily", "'Twisty Tie'"), "Twisty Tie");
    assert_eq!(assign(&rt, "fontSize", ".1em"), "0.1em");
    assert_eq!(assign(&rt, "fontSize", "-1px"), "");
    assert_eq!(assign(&rt, "backgroundImage", "url(a.png)"), "url(\"a.png\")");
}
