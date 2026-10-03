//! CSS Gap Decorations L1 §3–§4 в inline-`style` CSSOM (BUG-553): `rule*` / `{column,row}-rule*`
//! хранятся лонгхендами (`_lumen_css_expand_gap_rule`), шортхенд собирается при чтении.

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

const DIV: &str = "var d = document.createElement('div');";

#[test]
fn rule_shorthand_sets_three_longhand_lists() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            &format!(
                "{DIV} d.style.columnRule = 'repeat(auto, blue 6px, 5px solid red)';                  [d.style.columnRuleWidth, d.style.columnRuleStyle, d.style.columnRuleColor].join('|')"
            )
        ),
        "repeat(auto, 6px, 5px)|repeat(auto, none, solid)|repeat(auto, blue, red)"
    );
}

#[test]
fn rule_shorthand_reads_back_from_longhands() {
    let rt = rt();
    assert_eq!(
        eval_str(&rt, &format!("{DIV} d.style.columnRule = 'blue 6px'; d.style.columnRule")),
        "6px blue"
    );
    assert_eq!(
        eval_str(&rt, &format!("{DIV} d.style.rule = 'double'; d.style.rule + '|' + d.style.rowRuleStyle")),
        "double|double"
    );
}

#[test]
fn invalid_value_is_dropped() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            &format!(
                "{DIV} d.style.columnRule = '5px solid red'; d.style.columnRule = 'red 5px solid red';                  d.style.columnRuleWidth = '30%'; d.style.columnRule"
            )
        ),
        "5px solid red"
    );
    assert_eq!(eval_str(&rt, &format!("{DIV} d.style.ruleBreak = 'auto'; d.style.ruleBreak")), "");
}

#[test]
fn inset_shorthand_canonical_forms() {
    let rt = rt();
    assert_eq!(
        eval_str(&rt, &format!("{DIV} d.style.columnRuleInset = '10px 20px / -5px'; d.style.columnRuleInset")),
        "10px 20px / -5px -5px"
    );
    assert_eq!(
        eval_str(&rt, &format!("{DIV} d.style.ruleInsetCap = '0'; d.style.columnRuleInsetCapStart")),
        "0px"
    );
    assert_eq!(
        eval_str(&rt, &format!("{DIV} d.style.ruleInsetStart = '5px'; d.style.rowRuleInsetJunctionStart")),
        "5px"
    );
}

#[test]
fn keyword_properties_fan_out_to_both_axes() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            &format!("{DIV} d.style.ruleVisibilityItems = 'around'; d.style.columnRuleVisibilityItems + d.style.rowRuleVisibilityItems")
        ),
        "aroundaround"
    );
}

#[test]
fn style_attribute_round_trips_and_remove_property_clears_longhands() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            &format!("{DIV} d.setAttribute('style', 'column-rule: 5px solid red !important'); d.style.columnRuleStyle")
        ),
        "solid"
    );
    assert_eq!(
        eval_str(
            &rt,
            &format!(
                "{DIV} d.style.columnRule = '5px solid red'; var old = d.style.removeProperty('column-rule');                  old + '|' + d.style.columnRuleWidth + '|' + d.style.length"
            )
        ),
        "5px solid red||"
    );
}
