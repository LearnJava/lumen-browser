//! BUG-1437 — анимации и переходы свойств CSS Text (`tab-size`, `text-indent`, `letter-spacing`,
//! `word-spacing`): CSS-wide keyword в ключевых кадрах, сложение `<length-percentage>` и
//! модификаторы `text-indent` (`web_api_shim_tail_b.js`).

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
fn css_wide_keywords_resolve_to_computed_values() {
    let rt = rt();
    let script = "[
        _wa_wide_resolve(null, 'tabSize', 'initial'),
        _wa_wide_resolve(null, 'textIndent', 'initial'),
        _wa_wide_resolve(null, 'tabSize', 'unset'),
        _wa_wide_resolve(null, 'lineBreak', 'INITIAL'),
        _wa_wide_resolve(null, 'tabSize', '12'),
        _wa_wide_resolve(null, 'tabSize', 'revert')
    ].join('|')";
    assert_eq!(eval_str(&rt, script), "8|0px|8|auto|12|revert");
}

#[test]
fn inherit_takes_the_parent_computed_value() {
    let rt = rt();
    let script = "getComputedStyle = function(el) { return { textIndent: '70px' }; };
        _wa_wide_resolve({ parentElement: {} }, 'textIndent', 'inherit')";
    assert_eq!(eval_str(&rt, script), "70px");
}

#[test]
fn tab_size_number_and_length_do_not_mix_and_never_go_negative() {
    let rt = rt();
    let script = "[
        _wa_interp_prop('tabSize', '1', '10px', 0.3),
        _wa_interp_prop('tabSize', '1', '10px', 0.5),
        _wa_interp_prop('tabSize', '1px', '10px', -0.3),
        _wa_motion_interp('tabSize', '0', '10', -0.3),
        _wa_motion_interp('tabSize', '4', '10', 0.3)
    ].join('|')";
    assert_eq!(eval_str(&rt, script), "1|10px|0px|0|5.8");
}

#[test]
fn text_indent_modifiers_are_discrete_and_keep_their_canonical_order() {
    let rt = rt();
    let script = "[
        _wa_motion_interp('textIndent', '0px hanging each-line', '50px each-line hanging', 0.3),
        _wa_motion_interp('textIndent', '0px each-line', '50px hanging', 0.3),
        _wa_motion_interp('textIndent', '0px', '50px each-line hanging', 0.6),
        _wa_interp_prop('textIndent', '0px hanging', '50px hanging', 0.6)
    ].join('|')";
    assert_eq!(eval_str(&rt, script), "15px hanging each-line|0px each-line|50px hanging each-line|30px hanging");
}

#[test]
fn text_indent_percentage_plus_length_is_a_calc_sum() {
    let rt = rt();
    let script = "[
        _wa_composite_value('textIndent', '100%', '50px', 'add'),
        _wa_composite_value('textIndent', '250px', '50px', 'add'),
        _wa_composite_value('textIndent', '250px', '50px each-line hanging', 'add'),
        _wa_composite_value('textIndent', '250px each-line', '50px each-line', 'add'),
        _wa_interp_prop('textIndent', 'calc(100% + 50px)', 'calc(100% + 150px)', 0.3)
    ].join('|')";
    assert_eq!(
        eval_str(&rt, script),
        "calc(100% + 50px)|300px|50px hanging each-line|300px each-line|calc(100% + 80px)"
    );
}

#[test]
fn normal_spacing_is_zero_between_the_ends_and_kept_at_progress_zero_for_letters() {
    let rt = rt();
    let script = "[
        _wa_motion_interp('letterSpacing', 'normal', '10px', 0),
        _wa_motion_interp('letterSpacing', 'normal', '10px', 0.3),
        _wa_motion_interp('wordSpacing', 'normal', '10px', 0),
        _wa_motion_interp('wordSpacing', 'normal', '10px', 0.3)
    ].join('|')";
    assert_eq!(eval_str(&rt, script), "normal|3px|0px|3px");
}
