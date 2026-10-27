//! BUG-1535: CSSOM ключевых свойств табличной модели (`border-collapse`, `caption-side`,
//! `empty-cells`, `table-layout`) и грамматика `border-spacing` в `element.style`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn assign(rt: &V8JsRuntime, prop: &str, val: &str) -> String {
    let script = format!("var d = document.createElement('div'); d.style[{prop:?}] = {val:?}; d.style[{prop:?}]");
    match rt.eval(&script).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

#[test]
fn invalid_values_are_rejected() {
    let rt = rt();
    for (prop, val) in [
        ("borderCollapse", "none"),
        ("borderCollapse", "separate collapse"),
        ("captionSide", "auto"),
        ("captionSide", "left"),
        ("captionSide", "top bottom"),
        ("emptyCells", "auto"),
        ("emptyCells", "show hide"),
        ("tableLayout", "none"),
        ("tableLayout", "auto fixed"),
        ("borderSpacing", "10%"),
        ("borderSpacing", "-20px"),
        ("borderSpacing", "30"),
        ("borderSpacing", "40px 50px 60px"),
        ("borderSpacing", "10px 5%"),
    ] {
        assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно быть отброшено");
    }
}

#[test]
fn valid_values_keep_their_canonical_form() {
    let rt = rt();
    for (prop, val, want) in [
        ("borderCollapse", "collapse", "collapse"),
        ("captionSide", "bottom", "bottom"),
        ("emptyCells", "hide", "hide"),
        ("tableLayout", "fixed", "fixed"),
        ("borderSpacing", "0px", "0px"),
        ("borderSpacing", "0", "0px"),
        ("borderSpacing", "10px 20px", "10px 20px"),
        ("borderSpacing", "10px 10px", "10px"),
        (
            "borderSpacing",
            "calc(10px + 0.5em) calc(10px - 0.5em)",
            "calc(0.5em + 10px) calc(-0.5em + 10px)",
        ),
    ] {
        assert_eq!(assign(&rt, prop, val), want, "{prop} = {val:?}");
    }
}
