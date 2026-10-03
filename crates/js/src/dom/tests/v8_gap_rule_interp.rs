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

/// CSS Transitions L1 для `*-rule-*`: `_wa_gap_tr_value` ведёт переход по чтениям
/// `getComputedStyle()`. `_lumen_computed_property` подменяется таблицей, `performance.now` —
/// управляемыми часами, чтобы не зависеть от реального времени.
const TR_PRELUDE: &str = "
var __vals = {}, __now = 0;
_lumen_computed_property = function(nid, name) { return __vals[name] || ''; };
performance.now = function() { return __now; };
function __read(name) { _wa_gap_tr_clock_ms = null; return _wa_gap_tr_value(1, name, __vals[name]); }
__vals['transition-property'] = 'row-rule-width';
__vals['transition-duration'] = '10s';
__vals['transition-delay'] = '0s';
__vals['transition-timing-function'] = 'linear';
";

#[test]
fn transition_covering_tokens_include_shorthands() {
    let rt = rt();
    assert_eq!(
        eval_str(&rt, "_wa_gap_tr_tokens('column-rule-inset-cap-end').join(' ')"),
        // `column-rule`/`rule` покрывают только width/style/color — inset у них свои шортхенды.
        "all column-rule-inset column-rule-inset-cap column-rule-inset-end \
         column-rule-inset-cap-end rule-inset rule-inset-cap rule-inset-end rule-inset-cap-end"
    );
    assert_eq!(
        eval_str(&rt, "_wa_gap_tr_tokens('row-rule-width').join(' ')"),
        "all row-rule row-rule-width rule rule-width"
    );
}

#[test]
fn transition_runs_between_two_reads() {
    let rt = rt();
    let script = format!(
        "{TR_PRELUDE}
        __vals['row-rule-width'] = '10px'; var a = __read('row-rule-width');
        __vals['row-rule-width'] = '20px'; var b = __read('row-rule-width');
        __now = 5000; var c = __read('row-rule-width');
        __now = 10000; var d = __read('row-rule-width');
        [a, b, c, d].join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "10px 10px 15px 20px");
}

#[test]
fn transition_needs_a_duration_and_a_listed_property() {
    let rt = rt();
    let no_duration = format!(
        "{TR_PRELUDE}
        __vals['transition-duration'] = '0s';
        __vals['row-rule-width'] = '10px'; __read('row-rule-width');
        __vals['row-rule-width'] = '20px'; __read('row-rule-width')"
    );
    assert_eq!(eval_str(&rt, &no_duration), "20px");
    let unlisted = format!(
        "{TR_PRELUDE}
        __vals['transition-property'] = 'opacity';
        __vals['row-rule-width'] = '10px'; __read('row-rule-width');
        __vals['row-rule-width'] = '20px'; __read('row-rule-width')"
    );
    assert_eq!(eval_str(&rt, &unlisted), "20px");
}

#[test]
fn transition_honours_delay_and_restarts_from_the_current_value() {
    let rt = rt();
    let script = format!(
        "{TR_PRELUDE}
        __vals['transition-delay'] = '2s';
        __vals['row-rule-width'] = '10px'; __read('row-rule-width');
        __vals['row-rule-width'] = '20px'; var a = __read('row-rule-width');
        __now = 7000; var b = __read('row-rule-width');
        __vals['transition-delay'] = '0s';
        __vals['row-rule-width'] = '40px'; var c = __read('row-rule-width');
        [a, b, c].join(' ')"
    );
    // t = 0: в задержке — значение `from`; через 7 с (5 с после задержки) — 15px;
    // прерывание на 15px начинает новый переход от него же.
    assert_eq!(eval_str(&rt, &script), "10px 15px 15px");
}

#[test]
fn transition_does_not_start_for_non_interpolable_pair() {
    let rt = rt();
    let script = format!(
        "{TR_PRELUDE}
        __vals['transition-property'] = 'row-rule-inset-cap-start';
        __vals['row-rule-inset-cap-start'] = 'overlap-join'; __read('row-rule-inset-cap-start');
        __vals['row-rule-inset-cap-start'] = '4px'; __read('row-rule-inset-cap-start')"
    );
    assert_eq!(eval_str(&rt, &script), "4px");
}
