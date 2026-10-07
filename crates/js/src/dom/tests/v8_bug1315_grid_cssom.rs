//! BUG-1315: `element.style.<grid-свойство> = <невалидное значение>` отклоняется (грамматика —
//! `_lumen_css_canonical_grid` → `style::values::grid_cssom`), валидное сохраняется как есть.

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
        &format!(
            "var d = document.createElement('div'); d.style[{prop:?}] = {val:?}; d.style[{prop:?}]"
        ),
    )
}

#[test]
fn invalid_grid_values_are_rejected() {
    let rt = rt();
    for (prop, val) in [
        ("gridTemplateColumns", "-10px"),
        ("gridTemplateColumns", "10px 10pxx"),
        ("gridTemplateRows", "[one]"),
        ("gridAutoFlow", "row row"),
        ("gridRow", "5 / 8 / 3"),
        ("gridColumn", "0 / 5"),
        ("gridArea", "auto / auto / auto / auto / auto"),
        ("gridTemplateAreas", "\"a b\" \"c\""),
        ("gridAutoColumns", "none"),
        ("gridAutoRows", "minmax(5fr, 1px)"),
        ("gridTemplate", "10px"),
        ("grid", "auto-flow / auto-flow"),
        ("flexGrow", "-1"),
        ("flexShrink", "foo"),
        ("flowTolerance", "foo"),
    ] {
        assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно отклоняться");
    }
}

#[test]
fn valid_grid_values_are_kept() {
    let rt = rt();
    for (prop, val) in [
        ("gridTemplateColumns", "repeat(2, 1fr) 10px"),
        ("gridTemplateRows", "[a] minmax(10px, auto) [b]"),
        ("gridAutoFlow", "row dense"),
        ("gridRow", "1 / span 2"),
        ("gridColumn", "a / b"),
        ("gridArea", "1 / 2 / 3 / 4"),
        ("gridTemplateAreas", "\"a b\" \"a c\""),
        ("gridAutoColumns", "10px 1fr"),
        ("gridTemplate", "\"a\" 10px / 1fr"),
        ("grid", "auto-flow dense / 100px"),
        ("flexGrow", "2.5"),
        ("flexShrink", "0"),
        ("flowTolerance", "infinite"),
    ] {
        assert_eq!(assign(&rt, prop, val), val, "{prop} = {val:?} должно сохраняться");
    }
}

#[test]
fn invalid_value_keeps_previous_declaration() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.style.gridRow = '1 / 3'; d.style.gridRow = '5 8'; d.style.gridRow"
        ),
        "1 / 3"
    );
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.style.cssText = 'grid-auto-flow: row row; flex-grow: 2'; d.style.cssText"
        ),
        "flex-grow: 2;"
    );
}

#[test]
fn css_wide_keywords_and_var_still_pass() {
    let rt = rt();
    assert_eq!(assign(&rt, "gridTemplateColumns", "inherit"), "inherit");
    assert_eq!(assign(&rt, "gridRow", "var(--x)"), "var(--x)");
}
