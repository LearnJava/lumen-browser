//! BUG-1425 — `getComputedStyle().color` и `el.style.color` схлопывали `lab()`/`oklch()`/
//! `color()`/`color-mix()`/относительные цвета в `rgb()`. Теперь computed-значение хранит
//! исходное цветовое пространство (CSS Color L4 §15), specified — запись как есть.

#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

const PAGE: &str = r#"<html><body><div id="t">x</div></body></html>"#;

fn ev(s: &mut InProcessSession, js: &str) -> String {
    s.eval(js).expect("eval").trim_matches('"').to_owned()
}

fn computed(s: &mut InProcessSession, value: &str) -> String {
    ev(
        s,
        &format!(
            "(function(){{ var t=document.getElementById('t'); t.style.color=''; t.style.color='{value}'; \
             return getComputedStyle(t).color; }})()"
        ),
    )
}

#[test]
fn computed_color_keeps_lab_oklch_and_color_function() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    assert_eq!(computed(&mut s, "lab(46 -50 50)"), "lab(46 -50 50)");
    assert_eq!(computed(&mut s, "oklch(0.5 0.1 90)"), "oklch(0.5 0.1 90)");
    assert_eq!(computed(&mut s, "color(srgb 0 0.5 0)"), "color(srgb 0 0.5 0)");
    assert_eq!(computed(&mut s, "lab(20 0 10/50%)"), "lab(20 0 10 / 0.5)");
    assert_eq!(computed(&mut s, "color-mix(in srgb, red, blue)"), "color(srgb 0.5 0 0.5)");
    assert_eq!(computed(&mut s, "rgb(from rebeccapurple r g b)"), "color(srgb 0.4 0.2 0.6)");
    assert_eq!(computed(&mut s, "hwb(120 0% 50%)"), "rgb(0, 128, 0)");
    assert_eq!(computed(&mut s, "hwb(from rebeccapurple h w b)"), "color(srgb 0.4 0.2 0.6)");
}

#[test]
fn specified_color_keeps_source_form() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    let spec = |s: &mut InProcessSession, v: &str| {
        ev(s, &format!("(function(){{ var t=document.getElementById('t'); t.style.color='{v}'; return t.style.color; }})()"))
    };
    assert_eq!(spec(&mut s, "color-mix(in srgb, red, blue)"), "color-mix(in srgb, red, blue)");
    assert_eq!(spec(&mut s, "rgb(from rebeccapurple r g b)"), "rgb(from rebeccapurple r g b)");
    assert_eq!(spec(&mut s, "lab(46 -50 50)"), "lab(46 -50 50)");
}
