//! BUG-1424 — `el.style` read-back of an alpha colour uses the shortest 8-bit round-tripping
//! decimal (`0.5`, not `0.502`), and a percentage `opacity` is stored as the number it denotes.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn eval_str(js: &str) -> String {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    match rt.eval(js).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("unexpected value: {other:?}"),
    }
}

#[test]
fn style_color_with_half_alpha_reads_back_as_point_five() {
    let js = "(function() {
        var d = document.createElement('div');
        var out = [];
        ['rgb(0 128 0 / 0.5)', 'hsl(120 100% 25% / 0.5)', '#00800080'].forEach(function(v) {
            d.style.color = v;
            out.push(d.style.color);
        });
        return out.join('|');
    })()";
    assert_eq!(
        eval_str(js),
        "rgba(0, 128, 0, 0.5)|rgba(0, 128, 0, 0.5)|rgba(0, 128, 0, 0.5)"
    );
}

#[test]
fn style_opacity_percentage_is_stored_as_number() {
    let js = "(function() {
        var d = document.createElement('div');
        var out = [];
        ['50%', '-100%', '300%', '0.5', '12.5%'].forEach(function(v) {
            d.style.opacity = v;
            out.push(d.style.opacity);
        });
        return out.join('|');
    })()";
    assert_eq!(eval_str(js), "0.5|-1|3|0.5|0.125");
}
