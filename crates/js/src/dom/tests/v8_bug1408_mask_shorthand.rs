//! BUG-1408 срез 7: шорткод `mask` в `element.style` — значение проверяется по грамматике
//! `<mask-layer>#`, раскладывается по лонгхендам слоёв (`_lumen_css_expand_mask`) и читается
//! обратно каноном из них (`_lumen_css_mask_shorthand`).

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

/// `style.mask = val` на свежем элементе; результат чтения обратно.
fn assign(rt: &V8JsRuntime, val: &str) -> String {
    eval_str(
        rt,
        &format!("var d = document.createElement('div'); d.style.mask = {val:?}; d.style.mask"),
    )
}

#[test]
fn mask_shorthand_is_canonical() {
    let rt = rt();
    for (val, want) in [
        ("none", "none"),
        ("linear-gradient(to left bottom, red, blue)", "linear-gradient(to left bottom, red, blue)"),
        ("linear-gradient(to left bottom, red, blue) luminance", "linear-gradient(to left bottom, red, blue) luminance"),
        ("url(\"https://example.com/\")", "url(\"https://example.com/\")"),
        ("url(\"https://example.com/\") 1px 2px / contain", "url(\"https://example.com/\") 1px 2px / contain"),
        ("repeat-y", "repeat-y"),
        ("border-box", "none"),
        ("content-box", "content-box"),
        ("linear-gradient(to left bottom, red, blue) padding-box", "linear-gradient(to left bottom, red, blue) padding-box"),
        ("view-box", "view-box"),
        ("no-clip", "no-clip"),
        ("url(\"https://example.com/\") add", "url(\"https://example.com/\")"),
        ("subtract", "subtract"),
        ("alpha", "alpha"),
        ("border-box border-box", "none"),
        ("border-box content-box", "border-box content-box"),
        ("border-box no-clip", "no-clip"),
        ("none alpha", "alpha"),
        (
            "intersect no-clip space round 1px 2px / contain stroke-box linear-gradient(to left bottom, red, blue) luminance",
            "linear-gradient(to left bottom, red, blue) 1px 2px / contain space round stroke-box no-clip intersect luminance",
        ),
        (
            "intersect no-clip space round 1px 2px / contain view-box, stroke-box linear-gradient(to left bottom, red, blue) luminance",
            "1px 2px / contain space round view-box no-clip intersect, linear-gradient(to left bottom, red, blue) stroke-box luminance",
        ),
        ("none, linear-gradient(to left bottom, red, blue) padding-box", "none, linear-gradient(to left bottom, red, blue) padding-box"),
    ] {
        assert_eq!(assign(&rt, val), want, "mask = {val:?}");
    }
}

#[test]
fn invalid_mask_shorthand_is_rejected() {
    let rt = rt();
    for val in [
        "none linear-gradient(to 0% 0%, rgb(0, 0, 0), rgb(0, 0, 255))",
        "none alpha luminance",
        "1px 2px 3px",
        "1px 2px 3px 4px",
        "1px 2px / contain / cover",
        "repeat-y repeat-x",
        "stroke-box no-clip view-box",
        "border-box view-box padding-box",
        "no-clip border-box content-box",
        "content-box stroke-box no-clip",
        "margin-box",
        "add intersect",
    ] {
        assert_eq!(assign(&rt, val), "", "mask = {val:?} должно отклоняться");
    }
}

#[test]
fn mask_shorthand_sets_layer_and_border_longhands() {
    let rt = rt();
    let read = eval_str(
        &rt,
        "var d = document.createElement('div'); d.style.mask = 'none, linear-gradient(to left bottom, red, blue) padding-box'; \
         ['mask-image','mask-position','mask-size','mask-repeat','mask-origin','mask-clip','mask-composite','mask-mode', \
          'mask-border-source','mask-border-outset','mask-border-repeat','mask-border-slice','mask-border-width'] \
           .map(function(p) { return d.style.getPropertyValue(p); }).join('|')",
    );
    assert_eq!(
        read,
        "none, linear-gradient(to left bottom, red, blue)|0% 0%, 0% 0%|auto, auto|repeat, repeat|border-box, padding-box|\
         border-box, padding-box|add, add|match-source, match-source|none|0|stretch|0|auto"
    );
}

#[test]
fn removing_mask_shorthand_drops_its_longhands() {
    let rt = rt();
    let read = eval_str(
        &rt,
        "var d = document.createElement('div'); d.style.mask = 'none'; d.style.mask = ''; \
         ['mask-image','mask-border-source'].map(function(p) { return d.style.getPropertyValue(p); }).join('|')",
    );
    assert_eq!(read, "|");
    assert_eq!(eval_str(&rt, "var d = document.createElement('div'); d.style.mask = 'none'; d.style.mask"), "none");
}
