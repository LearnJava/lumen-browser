//! BUG-1293 — CSS-переходы и анимации стартуют при чтении стиля: оверлей `getComputedStyle()`
//! для любого анимируемого свойства (`_wa_motion_*` в `web_api_shim_tail_b.js`) и натив
//! `_lumen_css_interpolate_transform`.

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

/// Вычисленный стиль и часы подменены: `__vals` — снимок, `__now` — `performance.now()`.
const PRELUDE: &str = "
var __vals = {}, __now = 0, __kf = {};
_lumen_computed_property = function(nid, name) { return __vals[name] || ''; };
_wa_gap_keyframes_json = function(name) { return __kf[name] === undefined ? null : JSON.stringify(__kf[name]); };
_lumen_make_element = function(nid) { return null; };
performance.now = function() { return __now; };
function __read(name) { _wa_gap_tr_clock_ms = null; return _wa_gap_tr_value(1, name, __vals[name]); }
function __an(name, base) { _wa_gap_tr_clock_ms = null; return _wa_gap_an_value(1, name, base, base); }
__vals['transition-property'] = 'all';
__vals['transition-duration'] = '10s';
__vals['transition-delay'] = '0s';
__vals['transition-timing-function'] = 'linear';
";

#[test]
fn a_transition_starts_on_the_read_after_the_change() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __vals['margin-left'] = '10px'; var a = __read('margin-left');
        __vals['margin-left'] = '30px'; var b = __read('margin-left');
        __now = 5000; var c = __read('margin-left');
        __now = 10000; var d = __read('margin-left');
        [a, b, c, d].join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "10px 10px 20px 30px");
}

#[test]
fn a_negative_delay_lands_mid_transition_at_once() {
    // interpolation-testcommon.js: `transition-delay: -50s` over 100 s starts at 50 %.
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __vals['transition-duration'] = '100s'; __vals['transition-delay'] = '-50s';
        __vals['opacity'] = '0'; __read('opacity');
        __vals['opacity'] = '1'; __read('opacity')"
    );
    assert_eq!(eval_str(&rt, &script), "0.5");
}

#[test]
fn a_zero_duration_or_an_unlisted_property_never_transitions() {
    let rt = rt();
    let zero = format!(
        "{PRELUDE}
        __vals['transition-duration'] = '0s';
        __vals['width'] = '10px'; __read('width');
        __vals['width'] = '20px'; __read('width')"
    );
    assert_eq!(eval_str(&rt, &zero), "20px");
    let unlisted = format!(
        "{PRELUDE}
        __vals['transition-property'] = 'opacity';
        __vals['width'] = '10px'; __read('width');
        __vals['width'] = '20px'; __read('width')"
    );
    assert_eq!(eval_str(&rt, &unlisted), "20px");
}

#[test]
fn shorthands_cover_their_longhands() {
    let rt = rt();
    assert_eq!(
        eval_str(&rt, "_wa_motion_cover('margin-left').join(' ')"),
        "margin margin-left all"
    );
    assert_eq!(
        eval_str(&rt, "_wa_motion_cover('border-top-color').join(' ')"),
        "border-color border border-top border-top-color all"
    );
    assert_eq!(
        eval_str(&rt, "_wa_motion_cover('border-top-left-radius').join(' ')"),
        "border-radius border-top-left-radius all"
    );
    let shorthand = format!(
        "{PRELUDE}
        __vals['transition-property'] = 'margin';
        __vals['margin-left'] = '10px'; __read('margin-left');
        __vals['margin-left'] = '30px'; __read('margin-left');
        __now = 5000; __read('margin-left')"
    );
    assert_eq!(eval_str(&rt, &shorthand), "20px");
}

#[test]
fn colours_interpolate_premultiplied_in_srgb() {
    let rt = rt();
    assert_eq!(
        eval_str(&rt, "_wa_motion_interp('backgroundColor', 'rgb(0, 0, 0)', 'rgb(100, 100, 100)', 0.5)"),
        "rgb(50, 50, 50)"
    );
    // Прозрачный конец не тянет цвет к чёрному: `rgba(0,0,0,0)` → `rgb(200,0,0)`.
    assert_eq!(
        eval_str(&rt, "_wa_motion_interp('color', 'rgba(0, 0, 0, 0)', 'rgb(200, 0, 0)', 0.5)"),
        "rgba(200, 0, 0, 0.5)"
    );
}

#[test]
fn lists_of_lengths_interpolate_per_component_and_mismatched_units_flip() {
    let rt = rt();
    assert_eq!(eval_str(&rt, "_wa_motion_interp('perspectiveOrigin', '0px 10px', '10px 20px', 0.5)"), "5px 15px");
    assert_eq!(eval_str(&rt, "_wa_motion_interp('marginLeft', '10px', '50%', 0.25)"), "10px");
    assert_eq!(eval_str(&rt, "_wa_motion_interp('marginLeft', '10px', '50%', 0.75)"), "50%");
    assert!(!matches!(
        rt.eval("_wa_motion_interpolable('marginLeft', 'margin-left', '10px', '50%')").unwrap(),
        lumen_core::JsValue::Bool(true)
    ));
}

#[test]
fn integer_properties_round_the_interpolated_value() {
    let rt = rt();
    assert_eq!(eval_str(&rt, "_wa_motion_interp('zIndex', '-2', '-4', 0.1)"), "-2");
    assert_eq!(eval_str(&rt, "_wa_motion_interp('zIndex', '2', '5', 0.5)"), "4");
}

#[test]
fn transform_interpolates_in_the_native_and_flips_to_a_matrix() {
    let rt = rt();
    assert_eq!(
        eval_str(&rt, "_wa_motion_interp('transform', 'translateX(0px)', 'translateX(100px)', 0.5)"),
        "matrix(1, 0, 0, 1, 50, 0)"
    );
    // `rotate(900deg)` не сворачивается по кратчайшему пути, пока обе стороны — списки.
    assert_eq!(
        eval_str(&rt, "_wa_motion_interp('transform', 'none', 'rotate(900deg)', 0.2)"),
        eval_str(&rt, "_lumen_css_resolve_transform('rotate(180deg)', 0, 0)")
    );
    // Вырожденная пара не интерполируется: перелом, но результат — resolved matrix.
    assert_eq!(
        eval_str(
            &rt,
            "_wa_motion_interp('transform', 'matrix3d(2,0,0,0,0,2,0,0,0,0,0,0,0,0,0,1)', 'translateX(5px)', 0.75)"
        ),
        "matrix(1, 0, 0, 1, 5, 0)"
    );
}

#[test]
fn a_keyframes_animation_of_any_property_runs_on_the_read_clock() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __vals['animation-name'] = 'k'; __vals['animation-duration'] = '10s'; __vals['animation-delay'] = '0s';
        __vals['animation-timing-function'] = 'linear'; __vals['animation-iteration-count'] = '1';
        __vals['animation-direction'] = 'normal'; __vals['animation-fill-mode'] = 'none';
        __vals['animation-play-state'] = 'running';
        __kf['k'] = [
          {{ offset: 0, decls: [['margin-left', '10px']] }},
          {{ offset: 1, decls: [['margin-left', '30px']] }},
        ];
        var r = [__an('margin-left', '3px')];
        __now = 5000; r.push(__an('margin-left', '3px'));
        r.join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "10px 20px");
}

#[test]
fn a_keyframe_with_add_composition_stacks_on_the_underlying_value() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __vals['animation-name'] = 'k'; __vals['animation-duration'] = '10s'; __vals['animation-delay'] = '0s';
        __vals['animation-timing-function'] = 'linear'; __vals['animation-iteration-count'] = '1';
        __vals['animation-direction'] = 'normal'; __vals['animation-fill-mode'] = 'none';
        __vals['animation-play-state'] = 'running';
        __kf['k'] = [
          {{ offset: 0, decls: [['margin-left', '10px'], ['animation-composition', 'add']] }},
          {{ offset: 1, decls: [['margin-left', '30px'], ['animation-composition', 'add']] }},
        ];
        __an('margin-left', '100px'); __now = 5000; __an('margin-left', '100px')"
    );
    assert_eq!(eval_str(&rt, &script), "120px");
}

#[test]
fn a_removed_animation_name_cancels_the_shadow_animation() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        var a = _lumen_css_anim_register('a:', 1, 'k');
        var fin = a.finished, out = [];
        _wa_gap_an[1] = {{ '0:k': {{ start: 0, pausedAt: null, anim: a }} }};
        __vals['animation-name'] = 'none';
        __an('margin-left', '3px');
        fin.then(function() {{ out.push('resolved'); }}, function(e) {{ out.push(e.name); }});
        out.push(a._state);
        out.join(',')"
    );
    // `finished` отклоняется асинхронно; синхронно видно только состояние.
    assert_eq!(eval_str(&rt, &script), "idle");
    assert_eq!(eval_str(&rt, "typeof _wa_gap_an[1]"), "undefined");
}

#[test]
fn overshoot_is_clamped_to_the_property_range() {
    let rt = rt();
    assert_eq!(eval_str(&rt, "_wa_motion_interp('opacity', '0', '1', 1.5)"), "1");
    assert_eq!(eval_str(&rt, "_wa_motion_interp('opacity', '0', '1', -0.5)"), "0");
    assert_eq!(eval_str(&rt, "_wa_motion_interp('width', '10px', '20px', -2)"), "0px");
    // A margin may go negative.
    assert_eq!(eval_str(&rt, "_wa_motion_interp('marginLeft', '10px', '20px', -2)"), "-10px");
}

#[test]
fn display_stays_visible_for_the_whole_run_when_one_end_is_none() {
    let rt = rt();
    assert_eq!(eval_str(&rt, "_wa_motion_interp('display', 'block', 'none', 0.9)"), "block");
    assert_eq!(eval_str(&rt, "_wa_motion_interp('display', 'none', 'block', 0.1)"), "block");
    assert_eq!(eval_str(&rt, "_wa_motion_interp('display', 'block', 'flex', 0.7)"), "flex");
}
