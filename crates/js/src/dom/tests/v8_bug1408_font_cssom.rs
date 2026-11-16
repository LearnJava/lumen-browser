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

#[test]
fn font_variant_shorthand_round_trips_through_longhands() {
    let rt = rt();
    for (val, want) in [
        ("normal", "normal"),
        ("none", "none"),
        ("All-Small-Caps", "all-small-caps"),
        (
            "super proportional-width jis83 stacked-fractions tabular-nums historical-forms \
             all-small-caps no-contextual",
            "no-contextual all-small-caps historical-forms tabular-nums stacked-fractions \
             jis83 proportional-width super",
        ),
        (
            "annotation(flowing) ornaments(flowing) swash(flowing) character-variant(flowing) \
             styleset(flowing) historical-forms stylistic(flowing)",
            "stylistic(flowing) historical-forms styleset(flowing) character-variant(flowing) \
             swash(flowing) ornaments(flowing) annotation(flowing)",
        ),
        ("small-caps tabular-nums", "small-caps tabular-nums"),
    ] {
        assert_eq!(assign(&rt, "fontVariant", val), want, "font-variant = {val:?}");
    }
    // Лонгхенды читаются после присваивания шорткода, шорткод собирается из них же.
    assert_eq!(
        eval_str(&rt, "var d = document.createElement('div'); d.style.fontVariant = 'small-caps'; d.style.fontVariantCaps"),
        "small-caps"
    );
    assert_eq!(
        eval_str(&rt, "var d = document.createElement('div'); d.style.fontVariantNumeric = 'tabular-nums'; d.style.fontVariant"),
        "tabular-nums"
    );
    assert_eq!(
        eval_str(&rt, "var d = document.createElement('div'); d.style.fontVariant = 'small-caps'; d.style.cssText"),
        "font-variant: small-caps;"
    );
    for val in ["normal none", "small-caps normal", "sub super", "bogus"] {
        assert_eq!(assign(&rt, "fontVariant", val), "", "font-variant = {val:?} должен отклониться");
    }
}

#[test]
fn font_tag_lists_and_alternates_in_inline_style() {
    let rt = rt();
    for (prop, val, want) in [
        ("fontFeatureSettings", "\"dlig\" 1, \"smcp\" on", "\"dlig\", \"smcp\""),
        ("fontFeatureSettings", "\"liga\" off", "\"liga\" 0"),
        ("fontFeatureSettings", "\"\\22\\7d\\2f\\2a\"", "\"\\\"}/*\""),
        ("fontVariationSettings", "'wght' 1e3, 'slnt' -450.0e-1", "\"wght\" 1000, \"slnt\" -45"),
        ("fontVariantAlternates", "character-variant(more,than,one) styleset(x)",
            "styleset(x) character-variant(more, than, one)"),
        ("fontVariantAlternates", "historical-forms", "historical-forms"),
    ] {
        assert_eq!(assign(&rt, prop, val), want, "{prop} = {val:?}");
    }
    for (prop, val) in [
        ("fontFeatureSettings", "\"silly\" off"),
        ("fontVariationSettings", "'wght' 100px"),
        ("fontVariantAlternates", "swash(one) swash(two)"),
        ("fontVariantAlternates", "annotation()"),
        ("fontVariantAlternates", "historical-forms(argument)"),
    ] {
        assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно отклоняться");
    }
}
