//! BUG-1408: CSSOM `clip-path`/`clip` в `element.style` — грамматика и канон
//! (`_LUMEN_MASK_PROPERTIES`, `_lumen_css_canonical_mask` → `style::values::clip_cssom`):
//! basic-shapes (`inset`/`circle`/`ellipse`/`polygon`/`path`/`rect`/`xywh`), `<geometry-box>`,
//! `url()` и CSS 2.1 `clip: rect()`.

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
fn invalid_clip_values_are_rejected() {
    let rt = rt();
    for (prop, val) in [
        ("clipPath", "auto"),
        ("clipPath", "bogus"),
        ("clipPath", "ray(0deg)"),
        ("clipPath", "inset()"),
        ("clipPath", "circle(123)"),
        ("clipPath", "circle(10% 20%)"),
        ("clipPath", "ellipse(closest-side)"),
        ("clipPath", "polygon(1%)"),
        ("clipPath", "polygon(round 1%, 1px 2px)"),
        ("clipPath", "unknown-box"),
        ("clipPath", "xywh(0px 1%)"),
        ("clipPath", "path(\" \")"),
        ("clip", "none"),
        ("clip", "rect(10%, 20%, auto, auto)"),
        ("clip", "rect(10px 20px, 30px 40px)"),
    ] {
        assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно отклоняться");
    }
}

#[test]
fn valid_clip_values_are_canonical() {
    let rt = rt();
    for (prop, val, want) in [
        ("clipPath", "none", "none"),
        ("clipPath", "inset(0 1px)", "inset(0px 1px)"),
        ("clipPath", "circle(closest-side)", "circle()"),
        ("clipPath", "circle(4% at top right)", "circle(4% at right top)"),
        ("clipPath", "ellipse(closest-side closest-side at 10% 20%)", "ellipse(at 10% 20%)"),
        ("clipPath", "polygon(nonzero, 1px 2px, 3em 4em)", "polygon(1px 2px, 3em 4em)"),
        ("clipPath", "polygon(round 0px, 1px 2px)", "polygon(1px 2px)"),
        ("clipPath", "xywh(0 1% 2px 3em)", "xywh(0px 1% 2px 3em)"),
        ("clipPath", "rect(0 100% 200px 4em)", "rect(0px 100% 200px 4em)"),
        ("clipPath", "circle(7% at 8% 9%) border-box", "circle(7% at 8% 9%)"),
        (
            "clipPath",
            "path(nonzero, \"M20,20h60 v60 h-60z\")",
            "path(\"M 20 20 h 60 v 60 h -60 Z\")",
        ),
        ("clipPath", "url(#a)", "url(#a)"),
        ("clip", "auto", "auto"),
        ("clip", "rect(10px, -20px, auto, auto)", "rect(10px, -20px, auto, auto)"),
    ] {
        assert_eq!(assign(&rt, prop, val), want, "{prop} = {val:?}");
    }
}
