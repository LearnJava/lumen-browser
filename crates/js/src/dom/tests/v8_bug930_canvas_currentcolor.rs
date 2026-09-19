//! BUG-930 — Canvas 2D `fillStyle`/`strokeStyle`/`shadowColor` must resolve the
//! `currentColor` keyword (bare or nested inside `color-mix()`) to the canvas
//! element's own computed `color` (HTML LS §4.12.5.1.3), rather than rejecting
//! it as an unparseable value. `CanvasColor::from_css_str` has no notion of an
//! element, so the substitution happens at the JS/native boundary in
//! `web_api_shim_mid.js`'s `_lumen_c2d_resolve_current_color` before the string
//! reaches the Rust color parser.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 runtime with a `<canvas id="c">` appended to `<body>`, returning the
/// runtime and the canvas's `nid` (needed to key `update_computed_styles`).
fn v8_runtime_with_canvas() -> (V8JsRuntime, u32) {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    let nid = match rt
        .eval(
            "(function() {
                var c = document.createElement('canvas');
                c.id = 'c';
                document.body.appendChild(c);
                return c.__nid__;
            })()",
        )
        .unwrap()
    {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("unexpected nid: {other:?}"),
    };
    (rt, nid)
}

#[test]
fn fill_style_current_color_resolves_to_element_computed_color() {
    let (rt, nid) = v8_runtime_with_canvas();
    let mut styles = std::collections::HashMap::new();
    styles.insert("color".to_string(), "rgb(255, 0, 255)".to_string());
    let mut outer = std::collections::HashMap::new();
    outer.insert(nid, styles);
    rt.update_computed_styles(outer);

    let r = rt
        .eval(
            "(function() {
                var ctx = document.getElementById('c').getContext('2d');
                ctx.fillStyle = 'currentColor';
                return ctx.fillStyle;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("#ff00ff".to_string()));
}

#[test]
fn stroke_style_current_color_resolves_to_element_computed_color() {
    let (rt, nid) = v8_runtime_with_canvas();
    let mut styles = std::collections::HashMap::new();
    styles.insert("color".to_string(), "rgb(0, 255, 0)".to_string());
    let mut outer = std::collections::HashMap::new();
    outer.insert(nid, styles);
    rt.update_computed_styles(outer);

    let r = rt
        .eval(
            "(function() {
                var ctx = document.getElementById('c').getContext('2d');
                ctx.strokeStyle = 'currentColor';
                return ctx.strokeStyle;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("#00ff00".to_string()));
}

#[test]
fn shadow_color_current_color_resolves_to_element_computed_color() {
    let (rt, nid) = v8_runtime_with_canvas();
    let mut styles = std::collections::HashMap::new();
    styles.insert("color".to_string(), "rgb(0, 0, 255)".to_string());
    let mut outer = std::collections::HashMap::new();
    outer.insert(nid, styles);
    rt.update_computed_styles(outer);

    let r = rt
        .eval(
            "(function() {
                var ctx = document.getElementById('c').getContext('2d');
                ctx.shadowColor = 'currentColor';
                return ctx.shadowColor;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("#0000ff".to_string()));
}

/// A keyword nested inside a function, not just the bare keyword — the same
/// substitution has to hit `color-mix(in srgb, black, currentcolor)` since the
/// text replacement runs before the whole string reaches the parser.
#[test]
fn fill_style_color_mix_with_nested_current_color_is_accepted() {
    let (rt, nid) = v8_runtime_with_canvas();
    let mut styles = std::collections::HashMap::new();
    styles.insert("color".to_string(), "rgb(255, 255, 255)".to_string());
    let mut outer = std::collections::HashMap::new();
    outer.insert(nid, styles);
    rt.update_computed_styles(outer);

    let r = rt
        .eval(
            "(function() {
                var ctx = document.getElementById('c').getContext('2d');
                ctx.fillStyle = 'color-mix(in srgb, black, currentcolor)';
                return ctx.fillStyle;
            })()",
        )
        .unwrap();
    // Midpoint of black and white — no longer rejected as an unparseable value.
    assert_eq!(r, lumen_core::JsValue::String("#808080".to_string()));
}

/// No computed style populated yet (page can write `fillStyle` before first
/// layout) — resolves to opaque black rather than being dropped as invalid.
#[test]
fn fill_style_current_color_without_computed_style_falls_back_to_black() {
    let (rt, _nid) = v8_runtime_with_canvas();
    let r = rt
        .eval(
            "(function() {
                var ctx = document.getElementById('c').getContext('2d');
                ctx.fillStyle = '#ffffff';
                ctx.fillStyle = 'currentColor';
                return ctx.fillStyle;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("#000000".to_string()));
}
