//! BUG-930 — Canvas 2D `fillStyle`/`strokeStyle`/`shadowColor` must keep a
//! `color(<space> …)` value (CSS Color L4 §10.1) in its functional form when
//! read back, instead of gamut-mapping it to `#rrggbb`. `CanvasColor` now
//! remembers the parsed `ColorFloat` (`crates/engine/canvas/src/color.rs`)
//! specifically so `to_css_string` can serialize it that way.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_canvas() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt.eval(
        "(function() {
            var c = document.createElement('canvas');
            c.id = 'c';
            document.body.appendChild(c);
        })()",
    )
    .unwrap();
    rt
}

#[test]
fn fill_style_display_p3_round_trips_in_functional_form() {
    let rt = v8_runtime_with_canvas();
    let r = rt
        .eval(
            "(function() {
                var ctx = document.getElementById('c').getContext('2d');
                ctx.fillStyle = 'color(display-p3 0 1 0)';
                return ctx.fillStyle;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("color(display-p3 0 1 0)".to_string()));
}

#[test]
fn stroke_style_srgb_color_function_round_trips_in_functional_form() {
    let rt = v8_runtime_with_canvas();
    let r = rt
        .eval(
            "(function() {
                var ctx = document.getElementById('c').getContext('2d');
                ctx.strokeStyle = 'color(srgb 0.5 0 0.5)';
                return ctx.strokeStyle;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("color(srgb 0.5 0 0.5)".to_string()));
}

/// A plain `#rrggbb`/named color must keep its existing hex serialization —
/// only `color()`-authored input gets the functional-form treatment.
#[test]
fn fill_style_plain_hex_color_is_unaffected() {
    let rt = v8_runtime_with_canvas();
    let r = rt
        .eval(
            "(function() {
                var ctx = document.getElementById('c').getContext('2d');
                ctx.fillStyle = '#ff0000';
                return ctx.fillStyle;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("#ff0000".to_string()));
}
