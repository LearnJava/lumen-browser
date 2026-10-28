//! BUG-1561: `element.style.scrollBehavior` принимает только `auto | smooth`;
//! карта computed — в `layout::selector_query::tests`.

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

fn assign(rt: &V8JsRuntime, val: &str) -> String {
    eval_str(
        rt,
        &format!(
            "var d = document.createElement('div'); d.style.scrollBehavior = {val:?}; d.style.scrollBehavior"
        ),
    )
}

#[test]
fn scroll_behavior_accepts_auto_and_smooth() {
    let rt = rt();
    assert_eq!(assign(&rt, "smooth"), "smooth");
    assert_eq!(assign(&rt, "AUTO"), "auto");
}

#[test]
fn scroll_behavior_rejects_normal_and_garbage() {
    let rt = rt();
    for val in ["normal", "instant", "smooth smooth", "10px"] {
        assert_eq!(assign(&rt, val), "", "scroll-behavior = {val:?} должно отклоняться");
    }
}
