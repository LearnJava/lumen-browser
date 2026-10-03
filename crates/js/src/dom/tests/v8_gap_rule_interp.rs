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

#[test]
fn transition_of_non_interpolable_pair_flips_at_half_with_allow_discrete() {
    let rt = rt();
    let script = format!(
        "{TR_PRELUDE}
        __vals['transition-property'] = 'row-rule-color';
        __vals['transition-behavior'] = 'allow-discrete';
        __vals['row-rule-color'] = 'red, repeat(auto, red)'; __read('row-rule-color');
        __vals['row-rule-color'] = 'blue, blue, repeat(auto, blue)';
        var a = __read('row-rule-color');
        __now = 4000; var b = __read('row-rule-color');
        __now = 6000; var c = __read('row-rule-color');
        [a, b, c].join('|')"
    );
    // До 50% — `from`, после — `to`; обе стороны в computed-форме (`rgb()`), форма
    // списка с `repeat(auto, …)` сохранена.
    assert_eq!(
        eval_str(&rt, &script),
        concat!(
            "rgb(255, 0, 0), repeat(auto, rgb(255, 0, 0))|",
            "rgb(255, 0, 0), repeat(auto, rgb(255, 0, 0))|",
            "rgb(0, 0, 255), rgb(0, 0, 255), repeat(auto, rgb(0, 0, 255))"
        )
    );
}

#[test]
fn transition_of_non_interpolable_pair_stays_idle_with_normal_behavior() {
    let rt = rt();
    let script = format!(
        "{TR_PRELUDE}
        __vals['transition-property'] = 'row-rule-color';
        __vals['transition-behavior'] = 'normal';
        __vals['row-rule-color'] = 'red, repeat(auto, red)'; __read('row-rule-color');
        __vals['row-rule-color'] = 'blue, blue, repeat(auto, blue)'; __read('row-rule-color')"
    );
    assert_eq!(eval_str(&rt, &script), "blue, blue, repeat(auto, blue)");
}

#[test]
fn flipped_value_is_serialized_as_computed() {
    let rt = rt();
    assert_eq!(
        eval_str(&rt, "_wa_interp_prop('columnRuleColor', 'red, repeat(auto, red)', 'blue, blue, repeat(auto, blue)', 0.3)"),
        "rgb(255, 0, 0), repeat(auto, rgb(255, 0, 0))"
    );
    assert_eq!(
        eval_str(&rt, "_lumen_css_canonical_gap_rule('column-rule-width', 'thin, repeat(2, 20px)')"),
        "1px, repeat(2, 20px)"
    );
}

/// CSS Animations L1 для `*-rule-*`: `_wa_gap_an_value` читает `animation-*` из вычисленного
/// стиля и `@keyframes` из `_wa_gap_keyframes_json`; обе функции и часы подменены.
const AN_PRELUDE: &str = "
var __vals = {}, __now = 0, __kf = {};
_lumen_computed_property = function(nid, name) { return __vals[name] || ''; };
_wa_gap_keyframes_json = function(name) { return __kf[name] === undefined ? null : JSON.stringify(__kf[name]); };
_lumen_make_element = function(nid) { return null; };
performance.now = function() { return __now; };
function __an(base) { _wa_gap_tr_clock_ms = null; return _wa_gap_an_value(1, 'row-rule-width', base, base); }
__vals['animation-name'] = 'a';
__vals['animation-duration'] = '10s';
__vals['animation-delay'] = '0s';
__vals['animation-timing-function'] = 'linear';
__vals['animation-iteration-count'] = '1';
__vals['animation-direction'] = 'normal';
__vals['animation-fill-mode'] = 'none';
__vals['animation-play-state'] = 'running';
__kf['a'] = [
  { offset: 0, decls: [['row-rule-width', '10px']] },
  { offset: 1, decls: [['row-rule-width', '30px']] },
];
";

#[test]
fn keyframes_animation_interpolates_between_frames_over_time() {
    let rt = rt();
    let script = format!(
        "{AN_PRELUDE}
        var r = [__an('3px')];
        __now = 5000; r.push(__an('3px'));
        __now = 10000; r.push(__an('3px'));
        r.join(' ')"
    );
    // t = 0 → 10px, t = 5 с → 20px, после конца без fill-mode — установившееся значение.
    assert_eq!(eval_str(&rt, &script), "10px 20px 3px");
}

#[test]
fn keyframes_animation_missing_end_is_the_neutral_keyframe() {
    let rt = rt();
    let script = format!(
        "{AN_PRELUDE}
        __kf['a'] = [{{ offset: 0, decls: [['row-rule-width', '10px']] }}];
        __an('50px'); __now = 5000; __an('50px')"
    );
    assert_eq!(eval_str(&rt, &script), "30px");
}

#[test]
fn keyframes_animation_honours_delay_fill_direction_and_pause() {
    let rt = rt();
    let delayed = format!(
        "{AN_PRELUDE}
        __vals['animation-delay'] = '2s'; __vals['animation-fill-mode'] = 'both';
        var r = [__an('3px')];
        __now = 7000; r.push(__an('3px'));
        __now = 60000; r.push(__an('3px'));
        r.join(' ')"
    );
    assert_eq!(eval_str(&rt, &delayed), "10px 20px 30px");
    let alternate = format!(
        "{AN_PRELUDE}
        __vals['animation-iteration-count'] = '2'; __vals['animation-direction'] = 'alternate';
        __an('3px'); __now = 12500; __an('3px')"
    );
    // Вторая итерация идёт назад: на её четверти — 25 % от конца = 25px.
    assert_eq!(eval_str(&rt, &alternate), "25px");
    let paused = format!(
        "{AN_PRELUDE}
        __an('3px'); __now = 5000; __an('3px');
        __vals['animation-play-state'] = 'paused'; __now = 5000; var a = __an('3px');
        __now = 9000; var b = __an('3px');
        __vals['animation-play-state'] = 'running'; __now = 9000; __an('3px');
        __now = 11000; var c = __an('3px');
        [a, b, c].join(' ')"
    );
    // На паузе значение стоит на 20px; после возобновления часы идут с замороженной точки.
    assert_eq!(eval_str(&rt, &paused), "20px 20px 24px");
}

#[test]
fn keyframes_animation_ignores_other_properties_and_none() {
    let rt = rt();
    let script = format!(
        "{AN_PRELUDE}
        __kf['a'] = [{{ offset: 0, decls: [['opacity', '0']] }}, {{ offset: 1, decls: [['opacity', '1']] }}];
        var r = [__an('3px')];
        __kf['a'] = [{{ offset: 0, decls: [['rule-width', '10px']] }}, {{ offset: 1, decls: [['rule-width', '30px']] }}];
        __now = 5000; r.push(__an('3px'));
        __vals['animation-name'] = 'none'; r.push(__an('3px'));
        r.join(' ')"
    );
    // `rule-width` задаёт обе оси; `opacity` ключевого кадра значения не меняет.
    assert_eq!(eval_str(&rt, &script), "3px 20px 3px");
}

/// `element.getAnimations()` для `@keyframes` этих свойств: `_wa_gap_an_sync` заводит
/// `Animation` с таймингом из `animation-*`, а его `currentTime` читает и сдвигает те же часы,
/// по которым `getComputedStyle()` считает значение.
#[test]
fn keyframes_animation_is_listed_and_seekable() {
    let rt = rt();
    let script = format!(
        "{AN_PRELUDE}
        __vals['animation-play-state'] = 'paused';
        var tgt = {{ __nid__: 1 }};
        _wa_gap_tr_clock_ms = null; _wa_gap_an_sync(tgt);
        var a = _wa_animations.filter(function(x) {{ return x.id === 'a'; }});
        var r = [a.length, a[0].playState, a[0].currentTime, a[0].effect.getComputedTiming().duration];
        __now = 3000; _wa_gap_tr_clock_ms = null;
        a[0].currentTime = 5000;
        r.push(__an('3px'));
        r.push(a[0].currentTime);
        _wa_gap_an_sync(tgt);
        r.push(_wa_animations.filter(function(x) {{ return x.id === 'a'; }}).length);
        r.join(' ')"
    );
    // Один элемент списка, на паузе в нуле; после seek на 5 с значение 20px и часы стоят на 5 с.
    assert_eq!(eval_str(&rt, &script), "1 paused 0 10000 20px 5000 1");
}

#[test]
fn keyframes_animation_leaves_the_list_after_its_end_without_forwards_fill() {
    let rt = rt();
    let script = format!(
        "{AN_PRELUDE}
        var tgt = {{ __nid__: 1 }};
        _wa_gap_tr_clock_ms = null; _wa_gap_an_sync(tgt);
        var n = function() {{ return _wa_animations.filter(function(x) {{ return x.id === 'a'; }}).length; }};
        var r = [n()];
        __now = 11000; _wa_gap_tr_clock_ms = null; _wa_gap_an_sync(tgt);
        r.push(n());
        r.join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "1 0");
}

/// CSS Syntax §5.4.7: конец значения закрывает открытую функцию — `setProperty` с
/// `repeat(auto, …` без `)` сохраняет закрытый список, а не отбрасывает значение.
#[test]
fn set_property_closes_unclosed_function_of_gap_rule_list() {
    let rt = rt();
    let script = "(function() {
        var d = document.createElement('div');
        d.style.setProperty('column-rule-color', 'repeat(2, black, red)');
        d.style.setProperty('column-rule-color', 'repeat(auto, rgb(0, 0, 255), rgb(255, 0, 0)');
        var a = d.style.getPropertyValue('column-rule-color');
        return a;
    })()";
    assert_eq!(
        eval_str(&rt, script),
        "repeat(auto, rgb(0, 0, 255), rgb(255, 0, 0))"
    );
}
