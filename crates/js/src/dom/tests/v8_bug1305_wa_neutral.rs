//! BUG-1305: Web Animations — неявный («neutral») ключевой кадр, шорткоды `border-width`/
//! `border-radius`, `background-size` и списки теней интерполируются
//! (`_wa_compute_at_p`, `_wa_expand_shorthand`, `_wa_lerp_bg_size`, `_wa_lerp_shadow_list`).

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

/// `el.animate(kf, {duration: 100, fill: 'both', easing: 'linear'})`, пауза на 30 мс, затем
/// значение `prop` из `el.style`. `computed` подменяет `getComputedStyle()` (источник
/// «underlying»-значения неявного кадра).
fn sample(rt: &V8JsRuntime, computed: &str, kf: &str, prop: &str) -> String {
    eval_str(
        rt,
        &format!(
            "getComputedStyle = function() {{ return {computed}; }}; \
             var el = document.createElement('div'); document.body.appendChild(el); \
             var a = el.animate({kf}, {{duration: 100, fill: 'both', easing: 'linear'}}); \
             a.pause(); a.currentTime = 30; String(el.style[{prop:?}])"
        ),
    )
}

#[test]
fn bg_size_interpolates_per_component() {
    let rt = rt();
    assert_eq!(eval_str(&rt, "_wa_interp_prop('backgroundSize', '10px 10px', '20px 20px', 0.3)"), "13px 13px");
    // Недостающая высота — `auto`.
    assert_eq!(eval_str(&rt, "_wa_interp_prop('backgroundSize', '10px', '20px', 0.5)"), "15px auto");
    // Списки слоёв повторяются до НОК.
    assert_eq!(
        eval_str(&rt, "_wa_interp_prop('backgroundSize', '10px, 20px', '30px', 0.5)"),
        "20px auto, 25px auto"
    );
    // `auto` против длины и ключевые слова не интерполируются — перелом на 50%.
    assert_eq!(eval_str(&rt, "_wa_interp_prop('backgroundSize', 'auto', '20px', 0.2)"), "auto");
    assert_eq!(eval_str(&rt, "_wa_interp_prop('backgroundSize', 'cover', '20px', 0.7)"), "20px");
}

#[test]
fn box_shadow_lists_interpolate() {
    let rt = rt();
    assert_eq!(
        eval_str(&rt, "_wa_interp_prop('boxShadow', '0px 0px 0px 0px #000', '100px 0px 0px 0px #000', 0.3)"),
        "rgb(0, 0, 0) 30px 0px 0px 0px"
    );
    // Короткий список добирается прозрачной нулевой тенью.
    assert_eq!(
        eval_str(&rt, "_wa_interp_prop('boxShadow', 'none', '10px 10px rgb(255, 0, 0)', 0.5)"),
        "rgba(255, 0, 0, 0.5) 5px 5px 0px 0px"
    );
    // `inset` обязан совпасть, иначе перелом.
    assert_eq!(
        eval_str(&rt, "_wa_interp_prop('boxShadow', '1px 1px red', 'inset 3px 3px red', 0.2)"),
        "1px 1px red"
    );
    assert_eq!(
        eval_str(&rt, "_wa_interp_prop('boxShadow', 'inset 2px 2px #000', 'inset 4px 4px #000', 0.5)"),
        "rgb(0, 0, 0) 3px 3px 0px 0px inset"
    );
}

#[test]
fn border_width_shorthand_animates_longhands() {
    let rt = rt();
    assert_eq!(
        sample(
            &rt,
            "{}",
            "[{borderWidth: '20px 40px 60px 80px'}, {borderWidth: '30px 50px 70px 90px'}]",
            "borderTopWidth"
        ),
        "23px"
    );
    assert_eq!(
        sample(&rt, "{}", "[{borderRadius: '10px / 20px'}, {borderRadius: '20px / 40px'}]", "borderTopLeftRadius"),
        "13px 26px"
    );
}

#[test]
fn missing_end_keyframe_is_the_underlying_value() {
    let rt = rt();
    assert_eq!(
        sample(
            &rt,
            "{backgroundColor: 'rgb(0, 0, 0)'}",
            "[{backgroundColor: 'rgb(100, 100, 100)'}]",
            "backgroundColor"
        ),
        "rgb(30, 30, 30)"
    );
    assert_eq!(
        sample(&rt, "{letterSpacing: '10px'}", "[{letterSpacing: '20px'}]", "letterSpacing"),
        "13px"
    );
    // Только 0%-кадр: конец — вычисленное значение.
    assert_eq!(
        sample(&rt, "{letterSpacing: '10px'}", "[{letterSpacing: '20px', offset: 0}]", "letterSpacing"),
        "17px"
    );
}

#[test]
fn properties_follow_their_own_keyframes() {
    let rt = rt();
    // `b` назван только в среднем кадре: между 0% и 50% он идёт от вычисленного значения.
    assert_eq!(
        sample(
            &rt,
            "{letterSpacing: '0px', wordSpacing: '0px'}",
            "[{letterSpacing: '0px', offset: 0}, {wordSpacing: '100px', offset: 0.5}, {letterSpacing: '10px', offset: 1}]",
            "wordSpacing"
        ),
        "60px"
    );
}

#[test]
fn box_shadow_and_bg_size_reach_the_style() {
    let rt = rt();
    assert_eq!(
        sample(
            &rt,
            "{}",
            "[{boxShadow: '0 0 0 0 #000'}, {boxShadow: '100px 0 0 0 #000'}]",
            "boxShadow"
        ),
        "rgb(0, 0, 0) 30px 0px 0px 0px"
    );
    assert_eq!(
        sample(&rt, "{}", "[{backgroundSize: '10px 10px'}, {backgroundSize: '20px 20px'}]", "backgroundSize"),
        "13px 13px"
    );
}

#[test]
fn named_colours_and_normal_spacing_interpolate() {
    let rt = rt();
    assert_eq!(eval_str(&rt, "_wa_interp_prop('borderTopColor', 'rgb(0, 0, 0)', 'green', 0.3)"), "rgb(0,38,0)");
    assert_eq!(
        eval_str(&rt, "_wa_interp_prop('boxShadow', '1px 1px red', '3px 3px blue', 0.5)"),
        "rgb(128, 0, 128) 2px 2px 0px 0px"
    );
    assert_eq!(eval_str(&rt, "_wa_interp_prop('letterSpacing', 'normal', '20px', 0.3)"), "6px");
}
