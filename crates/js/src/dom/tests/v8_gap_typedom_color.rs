//! GAP-TYPEDOM срез 4 — the `CSSColorValue` family (`CSSRGB`/`CSSHSL`/
//! `CSSHWB`/`CSSLab`/`CSSLCH`/`CSSOKLab`/`CSSOKLCH`/`CSSColor`, CSS Typed OM
//! L1 §5), the remainder left after срезы 1-3 (unit factories, `CSSMathValue`,
//! `parse`/`parseAll`, `StylePropertyMap.set`/`.append`, `CSSTransformValue`).

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt_with_dom() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn s(rt: &V8JsRuntime, code: &str) -> String {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::String(v) => v,
        other => panic!("{code}: expected a string, got {other:?}"),
    }
}

fn b(rt: &V8JsRuntime, code: &str) -> bool {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::Bool(v) => v,
        other => panic!("{code}: expected a bool, got {other:?}"),
    }
}

/// A bare number normalises to a percentage for r/g/b; omitting alpha
/// defaults it to 100%, which the serialiser then drops from `cssText`.
#[test]
fn rgb_number_normalises_to_percent_and_defaults_alpha() {
    let rt = rt_with_dom();
    assert_eq!(s(&rt, "new CSSRGB(0.5, 0.2, 0.8).r.toString()"), "50%");
    assert_eq!(s(&rt, "new CSSRGB(0.5, 0.2, 0.8).alpha.toString()"), "100%");
    assert_eq!(s(&rt, "new CSSRGB(0.5, 0.2, 0.8).toString()"), "rgb(50% 20% 80%)");
}

/// `CSS.number(x)` is preserved as a `<number>` channel (not folded into a
/// percentage), and a non-opaque alpha stays in the serialisation.
#[test]
fn rgb_preserves_explicit_number_channel_and_shows_alpha() {
    let rt = rt_with_dom();
    assert_eq!(s(&rt, "new CSSRGB(0, CSS.number(73), 0).g.toString()"), "73");
    assert_eq!(
        s(&rt, "new CSSRGB(0, 0, 0, CSS.percent(40)).toString()"),
        "rgb(0% 0% 0% / 40%)"
    );
}

/// An angle is neither `<number>` nor `<percentage>` — invalid for an RGB
/// channel, at construction and through the setter.
#[test]
fn rgb_rejects_an_angle_channel() {
    let rt = rt_with_dom();
    assert!(b(
        &rt,
        r#"
        var threw = false;
        try { new CSSRGB(CSS.deg(1), 0, 0); } catch (e) { threw = e instanceof TypeError; }
        threw
        "#
    ));
    assert!(b(
        &rt,
        r#"
        var c = new CSSRGB(0, 0, 0);
        var threw = false;
        try { c.r = CSS.deg(1); } catch (e) { threw = e instanceof TypeError; }
        threw
        "#
    ));
}

/// The alpha channel is strictly `<percentage>` — a `<number>` (`CSS.number`)
/// is rejected even though it is fine for r/g/b.
#[test]
fn rgb_alpha_rejects_a_number() {
    let rt = rt_with_dom();
    assert!(b(
        &rt,
        r#"
        var threw = false;
        try { new CSSRGB(0, 0, 0, CSS.number(1)); } catch (e) { threw = e instanceof TypeError; }
        threw
        "#
    ));
}

/// A `CSSMathSum`/`CSSMathProduct` of percentages is accepted as a
/// `<percentage>`-typed channel (the leaf-wise approximation documented in
/// the file's doc comment).
#[test]
fn rgb_accepts_percentage_math_trees() {
    let rt = rt_with_dom();
    assert!(b(
        &rt,
        "new CSSRGB(new CSSMathSum([CSS.percent(10), CSS.percent(20)]), 0, 0) instanceof CSSRGB"
    ));
    assert!(b(
        &rt,
        "new CSSRGB(new CSSMathProduct([CSS.percent(10), CSS.number(2)]), 0, 0) instanceof CSSRGB"
    ));
}

/// Hue takes a bare number as degrees (not a percentage) for HSL/HWB.
#[test]
fn hsl_hwb_hue_number_means_degrees() {
    let rt = rt_with_dom();
    assert_eq!(s(&rt, "new CSSHSL(120, CSS.percent(50), CSS.percent(50)).h.toString()"), "120deg");
    assert_eq!(s(&rt, "new CSSHSL(120, 0.5, 0.5).toString()"), "hsl(120deg 50% 50%)");
    assert_eq!(s(&rt, "new CSSHWB(30, 0.1, 0.1).toString()"), "hwb(30deg 10% 10%)");
}

/// `CSSLCH`/`CSSOKLCH` route their third channel through the angle
/// normaliser like HSL/HWB; `CSSLab`/`CSSOKLab` keep all three numeric.
#[test]
fn lch_and_lab_serialise() {
    let rt = rt_with_dom();
    assert_eq!(s(&rt, "new CSSLCH(50, 40, 270).toString()"), "lch(50 40 270deg)");
    assert_eq!(s(&rt, "new CSSOKLCH(0.5, 0.1, 180).toString()"), "oklch(0.5 0.1 180deg)");
    assert_eq!(s(&rt, "new CSSLab(50, 40, -20).toString()"), "lab(50 40 -20)");
    assert!(b(&rt, "new CSSOKLab(0.5, 0.1, -0.1) instanceof CSSColorValue"));
}

/// `CSSColor` validates its colour-space name and serialises the channel
/// list positionally.
#[test]
fn css_color_validates_space_and_serialises_channels() {
    let rt = rt_with_dom();
    assert_eq!(
        s(&rt, "new CSSColor('srgb', [1, 0, 0.5]).toString()"),
        "color(srgb 1 0 0.5)"
    );
    assert!(b(
        &rt,
        r#"
        var threw = false;
        try { new CSSColor('not-a-space', [1, 0, 0]); } catch (e) { threw = e instanceof TypeError; }
        threw
        "#
    ));
}

/// Every subclass in the family is an instance of the shared `CSSColorValue`
/// base and of `CSSStyleValue` further up the chain.
#[test]
fn color_family_shares_base_class() {
    let rt = rt_with_dom();
    assert!(b(&rt, "new CSSRGB(0, 0, 0) instanceof CSSColorValue"));
    assert!(b(&rt, "new CSSHSL(0, CSS.percent(0), CSS.percent(0)) instanceof CSSColorValue"));
    assert!(b(&rt, "new CSSColorValue() instanceof CSSStyleValue"));
}
