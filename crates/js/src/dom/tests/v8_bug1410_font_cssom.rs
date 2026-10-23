//! BUG-1410: CSSOM `font-weight` / `font-style` / `font-stretch` в `element.style` и
//! `getComputedStyle`: дробный вес, `calc()`, угол `oblique`, таблица `lighter`/`bolder`.

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

/// `style[prop] = val` на свежем элементе; результат чтения обратно.
fn assign(rt: &V8JsRuntime, prop: &str, val: &str) -> String {
    eval_str(
        rt,
        &format!("var d = document.createElement('div'); d.style[{prop:?}] = {val:?}; d.style[{prop:?}]"),
    )
}

#[test]
fn out_of_range_and_invalid_values_are_rejected() {
    let rt = rt();
    for (prop, val) in [
        ("fontWeight", "0"),
        ("fontWeight", "1001"),
        ("fontWeight", "-1"),
        ("fontWeight", "auto"),
        ("fontStyle", "oblique 91deg"),
        ("fontStyle", "italic oblique"),
        ("fontStretch", "0.5"),
        ("fontStretch", "-10%"),
    ] {
        assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно быть отброшено");
    }
}

#[test]
fn valid_values_keep_their_canonical_form() {
    let rt = rt();
    for (prop, val, want) in [
        ("fontWeight", "150.25", "150.25"),
        ("fontWeight", "calc(100 + 100)", "calc(200)"),
        ("fontWeight", "bold", "bold"),
        ("fontStyle", "oblique 0deg", "normal"),
        ("fontStyle", "oblique 10deg", "oblique 10deg"),
        ("fontStyle", "oblique calc(30deg * 2)", "oblique calc(60deg)"),
        ("fontStretch", "50%", "50%"),
    ] {
        assert_eq!(assign(&rt, prop, val), want, "{prop} = {val:?}");
    }
}

#[test]
fn computed_font_weight_and_style() {
    let rt = rt();
    let computed = |style: &str| {
        eval_str(
            &rt,
            &format!(
                "var e = document.createElement('div'); document.body.appendChild(e); e.style.cssText = {style:?}; \
                 var c = getComputedStyle(e); c.fontWeight + '|' + c.fontStyle"
            ),
        )
    };
    assert_eq!(computed("font-weight: 150.25"), "150.25|normal");
    assert_eq!(computed("font-weight: calc(100 + 200)"), "300|normal");
    assert_eq!(computed("font-style: oblique 45deg"), "400|oblique 45deg");
    assert_eq!(computed("font-style: oblique 0deg"), "400|normal");
}
