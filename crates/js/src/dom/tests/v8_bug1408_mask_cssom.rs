//! BUG-1408: CSSOM свойств CSS Masking в `element.style` — слои `mask-*` по грамматике
//! `background-*`, `mask-{origin,clip,composite,mode}` — списки ключевых слов, `mask-type`/
//! `clip-rule` — одиночные ключевые слова (`_LUMEN_MASK_PROPERTIES`,
//! `_lumen_css_canonical_mask`).

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

/// `style[prop] = val` на свежем элементе; результат чтения обратно.
fn assign(rt: &V8JsRuntime, prop: &str, val: &str) -> String {
    eval_str(
        rt,
        &format!("var d = document.createElement('div'); d.style[{prop:?}] = {val:?}; d.style[{prop:?}]"),
    )
}

#[test]
fn invalid_mask_values_are_rejected() {
    let rt = rt();
    for (prop, val) in [
        ("maskRepeat", "repeat undefined"),
        ("maskSize", "-1px"),
        ("maskPosition", "right top 5px"),
        ("maskPosition", "center left 1px"),
        ("maskOrigin", "margin-box"),
        ("maskClip", "auto"),
        ("maskComposite", "source-over"),
        ("maskMode", "auto"),
        ("maskType", "alpha, luminance"),
        ("clipRule", "auto"),
        ("maskRepeat", "repeat; round"),
    ] {
        assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно отклоняться");
    }
}

#[test]
fn valid_mask_values_are_canonical() {
    let rt = rt();
    for (prop, val, want) in [
        ("maskSize", "1px", "1px auto"),
        ("maskRepeat", "repeat no-repeat", "repeat-x"),
        ("maskPosition", "bottom 10% right 20%", "right 20% bottom 10%"),
        ("maskPosition", "top", "center top"),
        ("maskPosition", "top, center, left", "center top, center center, left center"),
        ("maskOrigin", "BORDER-BOX", "border-box"),
        ("maskClip", "no-clip", "no-clip"),
        ("maskComposite", "add, subtract", "add, subtract"),
        ("maskMode", "luminance", "luminance"),
        ("maskType", "alpha", "alpha"),
        ("clipRule", "evenodd", "evenodd"),
    ] {
        assert_eq!(assign(&rt, prop, val), want, "{prop} = {val:?}");
    }
}
