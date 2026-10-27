//! BUG-1541: `element.style.{filter,backdropFilter}` — каноническая запись, отказ от
//! недопустимых значений (Filter Effects L1 §3).

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

/// `style[prop] = val` на свежем элементе; результат чтения обратно.
fn assign(rt: &V8JsRuntime, prop: &str, val: &str) -> String {
    match rt
        .eval(&format!("var d = document.createElement('div'); d.style[{prop:?}] = {val:?}; d.style[{prop:?}]"))
        .unwrap()
    {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

#[test]
fn filter_valid_values_are_canonical() {
    let rt = rt();
    for prop in ["filter", "backdropFilter"] {
        for (val, want) in [
            ("none", "none"),
            ("blur(100px)", "blur(100px)"),
            ("blur(0)", "blur(0px)"),
            ("blur()", "blur(0px)"),
            ("brightness(300%)", "brightness(300%)"),
            ("brightness()", "brightness(1)"),
            ("grayscale(300%)", "grayscale(100%)"),
            ("opacity(2)", "opacity(1)"),
            ("sepia()", "sepia(1)"),
            ("hue-rotate(0)", "hue-rotate(0deg)"),
            ("hue-rotate(90deg)", "hue-rotate(90deg)"),
            ("drop-shadow(1px 2px)", "drop-shadow(1px 2px)"),
            ("drop-shadow(0 0 0)", "drop-shadow(0px 0px 0px)"),
            ("drop-shadow(1px 2px rgb(4, 5, 6))", "drop-shadow(rgb(4, 5, 6) 1px 2px)"),
            ("blur(10px) contrast(20) brightness(30)", "blur(10px) contrast(20) brightness(30)"),
            ("brightness(calc(3))", "brightness(calc(3))"),
        ] {
            assert_eq!(assign(&rt, prop, val), want, "{prop} = {val:?}");
        }
    }
}

#[test]
fn filter_invalid_values_are_rejected() {
    let rt = rt();
    for prop in ["filter", "backdropFilter"] {
        for val in [
            "auto", "none hue-rotate(0deg)", "blur(10)", "blur(-100px)", "blur(5px 6px)",
            "brightness(-20)", "brightness(30px)", "contrast(-20)", "drop-shadow(10 20)",
            "drop-shadow(10% 20%)", "drop-shadow(1px)", "drop-shadow(1px 2px 3px 4px)",
            "drop-shadow(rgb(4, 5, 6))", "drop-shadow()", "grayscale(-20)", "hue-rotate(90)",
            "invert(30px)", "blur(10%)", "opacity(-20)", "saturate(-20)", "sepia(30px)", "blur(1px) bogus(2)", "",
        ] {
            assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно отклоняться");
        }
    }
}
