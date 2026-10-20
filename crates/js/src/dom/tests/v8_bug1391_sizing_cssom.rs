//! BUG-1391: `element.style.{min,max}-{width,height}` и `contain-intrinsic-*` валидируются и
//! канонизируются (`_lumen_css_canonical_min_max_size` / `_lumen_css_canonical_contain_intrinsic`).

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
fn min_max_keywords_are_side_specific() {
    let rt = rt();
    for (prop, val) in [
        ("maxWidth", "auto"),
        ("maxHeight", "auto"),
        ("minWidth", "none"),
        ("minHeight", "none"),
        ("maxWidth", "min-content max-content"),
        ("maxWidth", "-10%"),
    ] {
        assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно отклоняться");
    }
    assert_eq!(assign(&rt, "maxWidth", "none"), "none");
    assert_eq!(assign(&rt, "minWidth", "auto"), "auto");
}

#[test]
fn min_max_zero_canonicalizes_to_px() {
    let rt = rt();
    for prop in ["maxHeight", "minWidth", "maxWidth", "minHeight"] {
        assert_eq!(assign(&rt, prop, "0"), "0px", "{prop}");
    }
    assert_eq!(assign(&rt, "minWidth", "fit-content(calc(10% - 0.5em))"), "fit-content(calc(10% - 0.5em))");
}

#[test]
fn contain_intrinsic_size_validates_and_collapses() {
    let rt = rt();
    for (prop, val) in [
        ("containIntrinsicSize", "legacy"),
        ("containIntrinsicSize", "auto 1px 1px auto"),
        ("containIntrinsicWidth", "1px none"),
        ("containIntrinsicHeight", "20%"),
        ("containIntrinsicBlockSize", "none auto"),
    ] {
        assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно отклоняться");
    }
    assert_eq!(assign(&rt, "containIntrinsicSize", "5px 5px"), "5px");
    assert_eq!(assign(&rt, "containIntrinsicSize", "auto 1px auto 1px"), "auto 1px");
    assert_eq!(assign(&rt, "containIntrinsicSize", "2em 3px"), "2em 3px");
    assert_eq!(assign(&rt, "containIntrinsicBlockSize", "auto none"), "auto none");
}
