//! BUG-1504: Web Animations — интерполяция зарегистрированных пользовательских свойств.

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

/// Как `animation_test` из WPT `css-properties-values-api/resources/utils.js`: регистрирует
/// `--p` с `syntax`/`initial`, анимирует `kf` (3 итерации по 1000 мс), встаёт на середину
/// третьей и читает `el.style.getPropertyValue('--p')`. `getComputedStyle` в юнит-раннере
/// без движка стилей пуст, поэтому он подменён и отдаёт `initial` как подлежащее значение.
/// `opts` — добавка к опциям `animate()` (`composite`, `iterationComposite`).
fn at_mid(syntax: &str, initial: &str, kf: &str, opts: &str) -> String {
    let rt = rt();
    let script = format!(
        "CSS.registerProperty({{name: '--p', syntax: {syntax:?}, inherits: false, initialValue: {initial:?}}});
         getComputedStyle = function() {{ return {{getPropertyValue: function() {{ return {initial:?}; }}}}; }};
         var el = document.createElement('div'); document.body.appendChild(el);
         var a = el.animate({{'--p': {kf}}}, {{duration: 1000, iterations: 3, {opts}}});
         a.pause(); a.currentTime = 2500; String(el.style.getPropertyValue('--p'))"
    );
    eval_str(&rt, &script)
}

#[test]
fn length_lists_interpolate_elementwise() {
    assert_eq!(at_mid("<length>+", "0px", "['100px 150px', '200px 250px']", ""), "150px 200px");
    assert_eq!(at_mid("<length>#", "0px", "['100px, 150px', '200px, 250px']", ""), "150px, 200px");
    // Разная длина списков — дискретно: на середине берётся конечный.
    assert_eq!(at_mid("<length>+", "0px", "['10px 20px', '30px']", ""), "30px");
}

#[test]
fn length_list_composites() {
    assert_eq!(
        at_mid("<length>+", "50px 100px", "['150px 200px', '250px 300px']", "composite: 'add'"),
        "250px 350px"
    );
    assert_eq!(
        at_mid("<length>+", "0px 0px", "['0px 50px', '100px 100px']", "iterationComposite: 'accumulate'"),
        "250px 275px"
    );
}

#[test]
fn colors_interpolate_and_composite() {
    let kf = "['rgb(100, 100, 100)', 'rgb(200, 200, 200)']";
    assert_eq!(at_mid("<color>", "black", kf, ""), "rgb(150, 150, 150)");
    assert_eq!(
        at_mid("<color>#", "black", "['rgb(100, 100, 100), rgb(150, 150, 150)', 'rgb(200, 200, 200), rgb(250, 250, 250)']", ""),
        "rgb(150, 150, 150), rgb(200, 200, 200)"
    );
    assert_eq!(
        at_mid("<color>", "rgb(50, 50, 50)", "['rgb(50, 50, 50)', 'rgb(150, 150, 150)']", "composite: 'add'"),
        "rgb(150, 150, 150)"
    );
    assert_eq!(
        at_mid("<color>", "black", "['rgb(0, 0, 0)', 'rgb(100, 100, 100)']", "iterationComposite: 'accumulate'"),
        "rgb(250, 250, 250)"
    );
}

#[test]
fn transforms_interpolate_and_composite() {
    assert_eq!(
        at_mid("<transform-function>", "translateX(0px)", "['translateX(100px)', 'translateX(200px)']", ""),
        "translateX(150px)"
    );
    assert_eq!(
        at_mid("<transform-function>", "translateX(100px)", "['translateX(200px)', 'translateX(300px)']", "composite: 'add'"),
        "translateX(350px)"
    );
    assert_eq!(
        at_mid("<transform-list>", "translateX(0px)", "['translateX(100px) scale(2)', 'translateX(200px) scale(4)']", ""),
        "translateX(150px) scale(3)"
    );
    assert_eq!(
        at_mid("<transform-list>", "translateX(100px) scale(2)", "['translateX(200px) scale(3)', 'translateX(300px) scale(4)']", "composite: 'add'"),
        "translateX(100px) scale(2) translateX(250px) scale(3.5)"
    );
    assert_eq!(
        at_mid("<transform-list>", "translateX(0px)", "['translateX(0px) scale(3)', 'translateX(100px) scale(4)']", "iterationComposite: 'accumulate'"),
        "translateX(250px) scale(9.5)"
    );
}
