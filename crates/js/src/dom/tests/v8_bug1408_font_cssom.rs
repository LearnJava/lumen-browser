//! BUG-1408: CSSOM ключевых свойств CSS Fonts в `element.style` — одиночные ключевые слова,
//! `font-palette` с `<dashed-ident>`, `font-synthesis` и списки `font-variant-*`
//! (`_LUMEN_FONT_PROPERTIES`, `_lumen_css_canonical_font`).

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

fn assign(rt: &V8JsRuntime, prop: &str, val: &str) -> String {
    eval_str(
        rt,
        &format!("var d = document.createElement('div'); d.style[{prop:?}] = {val:?}; d.style[{prop:?}]"),
    )
}

#[test]
fn invalid_font_values_are_rejected() {
    let rt = rt();
    for (prop, val) in [
        ("fontKerning", "normal auto"),
        ("fontOpticalSizing", "normal"),
        ("fontPalette", "none"),
        ("fontVariantCaps", "auto"),
        ("fontVariantLigatures", "common-ligatures no-common-ligatures"),
        ("fontVariantNumeric", "lining-nums oldstyle-nums"),
        ("fontVariantEastAsian", "jis78 jis83"),
        ("fontVariantPosition", "super sub"),
        ("fontVariantEmoji", "normal text"),
        ("fontSynthesis", "none weight"),
        ("fontSynthesisStyle", "auto none"),
    ] {
        assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно отклоняться");
    }
}

#[test]
fn valid_font_values_are_canonical() {
    let rt = rt();
    for (prop, val, want) in [
        ("fontKerning", "AUTO", "auto"),
        ("fontOpticalSizing", "none", "none"),
        ("fontPalette", "--Pitchfork", "--Pitchfork"),
        ("fontPalette", "light", "light"),
        ("fontVariantCaps", "All-Small-Caps", "all-small-caps"),
        ("fontVariantLigatures", "no-contextual common-ligatures", "common-ligatures no-contextual"),
        ("fontVariantNumeric", "tabular-nums oldstyle-nums", "oldstyle-nums tabular-nums"),
        ("fontVariantEastAsian", "ruby full-width simplified", "simplified full-width ruby"),
        ("fontVariantPosition", "sub", "sub"),
        ("fontVariantEmoji", "unicode", "unicode"),
        ("fontSynthesis", "position weight small-caps style", "weight style small-caps position"),
        ("fontSynthesisWeight", "none", "none"),
        ("fontSynthesisStyle", "oblique-only", "oblique-only"),
    ] {
        assert_eq!(assign(&rt, prop, val), want, "{prop} = {val:?}");
    }
}
