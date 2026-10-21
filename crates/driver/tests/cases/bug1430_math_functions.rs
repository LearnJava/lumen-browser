//! BUG-1430 — математические функции в нечисловых контекстах (`<number>`, `<integer>`,
//! `<time>`, `<angle>`): computed-значение и запись `el.style[prop]`.

#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

const PAGE: &str = r#"<html><body><div id="c" style="font-size:20px;width:100px"><div id="t">x</div></div></body></html>"#;

fn ev(s: &mut InProcessSession, js: &str) -> String {
    s.eval(js).expect("eval").trim_matches('"').to_owned()
}

fn computed(s: &mut InProcessSession, prop: &str, value: &str) -> String {
    ev(
        s,
        &format!(
            "(function(){{ var t=document.getElementById('t'); t.style.cssText='position:absolute'; \
             t.style.setProperty('{prop}','{value}'); return getComputedStyle(t).getPropertyValue('{prop}'); }})()"
        ),
    )
}

fn specified(s: &mut InProcessSession, prop: &str, value: &str) -> String {
    ev(
        s,
        &format!(
            "(function(){{ var t=document.getElementById('t'); t.style.cssText='';              t.style.setProperty('{prop}','{value}'); return t.style.getPropertyValue('{prop}'); }})()"
        ),
    )
}

#[test]
fn number_integer_time_angle_contexts_resolve_at_computed_time() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    let cases: &[(&str, &str, &str)] = &[
        ("z-index", "calc(1 + 2)", "3"),
        ("z-index", "calc(-3.5 / 2)", "-2"),
        ("order", "calc(1 + 2)", "3"),
        ("opacity", "calc(0.5 + 0.25)", "0.75"),
        ("flex-grow", "calc(1 + 2)", "3"),
        ("tab-size", "calc(1 + 2)", "3"),
        ("transition-delay", "calc(1s + 1s)", "2s"),
        ("transition-delay", "calc(500ms + 1s)", "1.5s"),
        ("animation-duration", "calc(1s + 1s)", "2s"),
        ("rotate", "calc(1deg + 1deg)", "2deg"),
        ("scale", "calc(1 + 1)", "2"),
        ("scale", "sign(-1px)", "-1"),
        ("scale", "calc(infinity)", "3.40282e+38"),
        ("scale", "calc(NaN)", "0"),
        ("scale", "round(nearest, -0, infinity)", "-0"),
        ("scale", "calc(10px / 5px)", "2"),
        ("scale", "calc(pi)", "3.14159"),
        ("scale", "progress(5px, 0px, 10px)", "0.5"),
        ("margin-left", "calc(100px * progress(5px, 0px, 10px))", "50px"),
        ("margin-left", "calc(10px*pi)", "31.41593px"),
    ];
    for (prop, value, want) in cases {
        let got = computed(&mut s, prop, value);
        assert_eq!(&got, want, "{prop}: {value}");
    }
}

#[test]
fn individual_transform_properties_have_a_computed_value() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    assert_eq!(computed(&mut s, "rotate", "45deg"), "45deg");
    assert_eq!(computed(&mut s, "rotate", "none"), "none");
    assert_eq!(computed(&mut s, "scale", "2"), "2");
    assert_eq!(computed(&mut s, "scale", "2 3"), "2 3");
    assert_eq!(computed(&mut s, "translate", "5px"), "5px");
    assert_eq!(computed(&mut s, "translate", "5px 7px"), "5px 7px");
}

#[test]
fn transform_function_argument_math_is_evaluated() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    let a = computed(&mut s, "transform", "rotate(calc(1deg + 1deg))");
    let b = computed(&mut s, "transform", "rotate(2deg)");
    assert_ne!(a, "none");
    assert_eq!(a, b);
}

#[test]
fn specified_value_of_a_math_function_is_canonical_calc_or_dropped() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    let cases: &[(&str, &str, &str)] = &[
        ("opacity", "min(.1, .2)", "calc(0.1)"),
        ("opacity", "calc(NaN)", "calc(NaN)"),
        ("opacity", "calc(1 * -infinity)", "calc(-infinity)"),
        ("z-index", "calc(1 + 2)", "calc(3)"),
        ("transition-delay", "calc(1s + 1s)", "calc(2s)"),
        ("rotate", "calc(1deg + 1deg)", "calc(2deg)"),
        ("margin-left", "calc(10px*pi)", "calc(31.41593px)"),
        ("margin-left", "calc(1px*NaN)", "calc(NaN * 1px)"),
        ("width", "calc(1px*NaN)", "calc(NaN * 1px)"),
        // Type or syntax errors drop the assignment.
        ("z-index", "abs()", ""),
        ("z-index", "calc(1px)", ""),
        ("opacity", "calc(1 + )", ""),
        ("rotate", "sin(1)", ""),
    ];
    for (prop, value, want) in cases {
        let got = specified(&mut s, prop, value);
        assert_eq!(&got, want, "{prop}: {value}");
    }
}
