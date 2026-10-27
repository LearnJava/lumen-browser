//! `box-decoration-break` / `-webkit-box-decoration-break` (CSS Fragmentation L3 §5.1, Compat §3):
//! `element.style` keeps `slice | clone` and the CSS-wide keywords, rejects everything else.

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

fn assign(rt: &V8JsRuntime, prop: &str, val: &str) -> String {
    eval_str(
        rt,
        &format!("var d = document.createElement('div'); d.style[{prop:?}] = {val:?}; d.style[{prop:?}]"),
    )
}

#[test]
fn keywords_are_kept_and_the_rest_is_rejected() {
    let rt = rt();
    for prop in ["boxDecorationBreak", "webkitBoxDecorationBreak"] {
        for val in ["slice", "clone", "inherit", "initial"] {
            assert_eq!(assign(&rt, prop, val), val, "{prop} = {val:?}");
        }
        for val in ["auto", "slice clone", "none", "1px"] {
            assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно отклоняться");
        }
    }
}

#[test]
fn css_supports_knows_both_names() {
    let rt = rt();
    assert_eq!(eval_str(&rt, "String(CSS.supports('box-decoration-break', 'clone'))"), "true");
    assert_eq!(eval_str(&rt, "String(CSS.supports('-webkit-box-decoration-break', 'slice'))"), "true");
}
