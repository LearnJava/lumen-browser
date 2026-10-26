//! BUG-1529: CSSOM свойств CSS Inline в `element.style` — `line-height`, `baseline-shift`,
//! грамматика `vertical-align`, `initial-letter`; их `getComputedStyle` проверяет слой layout.

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
fn invalid_values_are_rejected() {
    let rt = rt();
    for (prop, val) in [
        ("lineHeight", "auto"),
        ("lineHeight", "-2"),
        ("lineHeight", "-10px"),
        ("lineHeight", "-200%"),
        ("lineHeight", "2 10px"),
        ("baselineShift", "5"),
        ("baselineShift", "-10px 20%"),
        ("baselineShift", "sub super"),
        ("baselineShift", "20px sub"),
        ("verticalAlign", "auto"),
        ("verticalAlign", "none"),
        ("verticalAlign", "first last"),
        ("verticalAlign", "sub super"),
        ("verticalAlign", "20% sub"),
        ("verticalAlign", "baseline middle"),
        ("verticalAlign", "text-top, bottom"),
        ("initialLetter", "drop"),
        ("initialLetter", "0.5"),
        ("initialLetter", "1 0"),
        ("initialLetter", "1 1.5"),
        ("initialLetter", "1 drop 3"),
        ("initialLetter", "drop 1 2"),
    ] {
        assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно быть отброшено");
    }
}

#[test]
fn valid_values_keep_their_canonical_form() {
    let rt = rt();
    for (prop, val, want) in [
        ("lineHeight", "normal", "normal"),
        ("lineHeight", "2", "2"),
        ("lineHeight", "0%", "0%"),
        ("baselineShift", "0", "0px"),
        ("baselineShift", "center", "center"),
        ("verticalAlign", "0", "baseline"),
        ("verticalAlign", "baseline 0", "baseline"),
        ("verticalAlign", "first baseline", "first"),
        ("verticalAlign", "last baseline sub", "last sub"),
        ("verticalAlign", "super middle first", "first middle super"),
        ("verticalAlign", "text-top first 10%", "first text-top 10%"),
        ("initialLetter", "normal", "normal"),
        ("initialLetter", "1.23 456", "1.23 456"),
        ("initialLetter", "drop 1.23", "1.23 drop"),
        ("initialLetter", "raise 1.23", "1.23 raise"),
        ("initialLetter", "calc(sign(1rem - 1px) * 1.5)", "calc(sign(1rem - 1px) * 1.5)"),
    ] {
        assert_eq!(assign(&rt, prop, val), want, "{prop} = {val:?}");
    }
}

#[test]
fn supports_initial_letter() {
    let rt = rt();
    assert_eq!(eval_str(&rt, "String(CSS.supports('initial-letter', '3 2'))"), "true");
}
