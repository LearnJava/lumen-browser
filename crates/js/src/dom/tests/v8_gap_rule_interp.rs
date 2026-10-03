//! CSS Gap Decorations L1 §4.7 — интерполяция `*-rule-width/-color/-inset-*` в Web Animations
//! (нативный `_lumen_css_interpolate_gap_rule` + ветка `_wa_gap_prop_re` в `_wa_interp_prop`).

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

#[test]
fn native_interpolates_width_lists_to_lcm() {
    let rt = rt();
    assert_eq!(
        eval_str(&rt, "_lumen_css_interpolate_gap_rule('column-rule-width', '10px, 20px', '30px', 0.5)"),
        "20px, 25px"
    );
}

#[test]
fn native_returns_null_for_mismatched_auto_shapes() {
    let rt = rt();
    assert_eq!(
        rt.eval(
            "_lumen_css_interpolate_gap_rule('row-rule-width', '1px, repeat(auto, 2px)', \
             '1px, 1px, repeat(auto, 2px)', 0.5)"
        )
        .unwrap(),
        lumen_core::JsValue::Null
    );
}

#[test]
fn wa_interp_prop_routes_gap_properties_and_flips_when_not_interpolable() {
    let rt = rt();
    assert_eq!(eval_str(&rt, "_wa_interp_prop('rowRuleColor', 'black', 'red', 0.5)"), "rgb(128, 0, 0)");
    assert_eq!(
        eval_str(&rt, "_wa_interp_prop('columnRuleInsetCapStart', '-100%', '1px', 0.3)"),
        "calc(-70% + 0.3px)"
    );
    // `overlap-join` не интерполируется — перелом на 50%.
    assert_eq!(
        eval_str(&rt, "_wa_interp_prop('rowRuleInsetJunctionEnd', 'overlap-join', '4px', 0.2)"),
        "overlap-join"
    );
    assert_eq!(
        eval_str(&rt, "_wa_interp_prop('rowRuleInsetJunctionEnd', 'overlap-join', '4px', 0.7)"),
        "4px"
    );
}
