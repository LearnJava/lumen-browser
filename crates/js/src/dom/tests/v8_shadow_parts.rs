//! CSS Shadow Parts L1: `Element.part` (a `DOMTokenList` over the `part` attribute,
//! `[PutForwards=value]`) and `CSS.supports("selector(::part(...))")`. BUG-1578.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn runtime() -> V8JsRuntime {
    let doc = Arc::new(Mutex::new(lumen_html_parser::parse(
        r#"<div id="host"></div><span id="a" part="x y"></span>"#,
    )));
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    rt
}

fn eval_str(rt: &V8JsRuntime, script: &str) -> String {
    match rt.eval(script).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

#[test]
fn element_part_is_a_token_list_over_the_part_attribute() {
    let rt = runtime();
    assert_eq!(eval_str(&rt, "var a = document.getElementById('a'); a.part instanceof DOMTokenList ? a.part.value : 'no'"), "x y");
    assert_eq!(eval_str(&rt, "a.part.add('z'); a.getAttribute('part')"), "x y z");
    assert_eq!(eval_str(&rt, "a.part.remove('x'); a.part.contains('x') + ',' + a.part.length"), "false,2");
    assert_eq!(eval_str(&rt, "String(a.part === a.part)"), "true");
}

#[test]
fn element_part_assignment_forwards_to_value() {
    let rt = runtime();
    assert_eq!(eval_str(&rt, "var a = document.getElementById('a'); a.part = 'p q'; a.getAttribute('part')"), "p q");
    assert_eq!(eval_str(&rt, "var h = document.getElementById('host'); h.part.length + ':' + h.part.value"), "0:");
}

#[test]
fn css_supports_accepts_part_selectors() {
    let rt = runtime();
    for sel in ["::part(a)", "::part(a b)", "x-foo::part(a):hover", "::part(a)::before", "::part(a)::selection"] {
        assert_eq!(
            eval_str(&rt, &format!("String(CSS.supports('selector({sel})'))")),
            "true",
            "{sel}"
        );
    }
    for sel in ["::part()", "::part(a)::part(b)", "::part(a)::slotted(b)", "::part(a b,c)"] {
        assert_eq!(
            eval_str(&rt, &format!("String(CSS.supports('selector({sel})'))")),
            "false",
            "{sel}"
        );
    }
}
