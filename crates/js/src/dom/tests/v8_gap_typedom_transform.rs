//! GAP-TYPEDOM срез 3 — the `CSSTransformComponent` family
//! (`CSSTranslate`/`CSSRotate`/`CSSScale`/`CSSSkew`/`CSSSkewX`/`CSSSkewY`/
//! `CSSPerspective`/`CSSMatrixComponent`) and `CSSTransformValue` (CSS Typed
//! OM L1 §11), the largest still-unimplemented slice of GAP-TYPEDOM/BUG-554
//! after срезы 1-2 (unit factories, `CSSMathValue`, `parse`/`parseAll`,
//! `StylePropertyMap.set`/`.append`).

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt_with_dom() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, false)
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

fn n(rt: &V8JsRuntime, code: &str) -> f64 {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::Number(v) => v,
        other => panic!("{code}: expected a number, got {other:?}"),
    }
}

/// A 2-arg `CSSTranslate` is 2D and serialises with `translate(...)`; the
/// 3-arg form is 3D even when `z` is `0px`, per spec §11.3.
#[test]
fn translate_arity_controls_is2d() {
    let rt = rt_with_dom();
    assert!(b(&rt, "new CSSTranslate(CSS.px(1), CSS.px(2)).is2D"));
    assert_eq!(s(&rt, "new CSSTranslate(CSS.px(1), CSS.px(2)).toString()"), "translate(1px, 2px)");
    assert!(!b(&rt, "new CSSTranslate(CSS.px(1), CSS.px(2), CSS.px(0)).is2D"));
    assert_eq!(
        s(&rt, "new CSSTranslate(CSS.px(1), CSS.px(2), CSS.px(0)).toString()"),
        "translate3d(1px, 2px, 0px)"
    );
}

/// A plain number is not a `CSSNumericValue` — `CSSTranslate`'s x/y are
/// strictly typed per the IDL (unlike `CSSScale`'s `(double or
/// CSSNumericValue)` union below).
#[test]
fn translate_rejects_a_plain_number() {
    let rt = rt_with_dom();
    assert!(b(
        &rt,
        r#"
        var threw = false;
        try { new CSSTranslate(1, 2); } catch (e) { threw = e instanceof TypeError; }
        threw
        "#
    ));
}

/// `CSSRotate(angle)` is the 2D single-axis form; `CSSRotate(x,y,z,angle)`
/// is the 3D axis-angle form. Both serialise per §11.4.
#[test]
fn rotate_one_arg_vs_four_arg() {
    let rt = rt_with_dom();
    assert!(b(&rt, "new CSSRotate(CSS.deg(45)).is2D"));
    assert_eq!(s(&rt, "new CSSRotate(CSS.deg(45)).toString()"), "rotate(45deg)");
    assert!(!b(&rt, "new CSSRotate(1, 0, 0, CSS.deg(45)).is2D"));
    assert_eq!(s(&rt, "new CSSRotate(1, 0, 0, CSS.deg(45)).toString()"), "rotate3d(1, 0, 0, 45deg)");
}

/// `CSSScale`'s x/y/z accept plain numbers (the union type in the IDL),
/// unlike `CSSTranslate`/`CSSRotate`'s strictly-`CSSNumericValue` operands.
/// Two-arg form is 2D; the `z` default is `1`, not `0`.
#[test]
fn scale_accepts_plain_numbers_and_defaults_z_to_one() {
    let rt = rt_with_dom();
    assert!(b(&rt, "new CSSScale(2, 3).is2D"));
    assert_eq!(s(&rt, "new CSSScale(2, 3).toString()"), "scale(2, 3)");
    assert!(!b(&rt, "new CSSScale(2, 3, 4).is2D"));
    assert_eq!(s(&rt, "new CSSScale(2, 3, 4).toString()"), "scale3d(2, 3, 4)");
}

/// `skew(ax, ay)` vs `skewX`/`skewY` — all three are inherently 2D.
#[test]
fn skew_family_serialisation() {
    let rt = rt_with_dom();
    assert_eq!(s(&rt, "new CSSSkew(CSS.deg(10), CSS.deg(20)).toString()"), "skew(10deg, 20deg)");
    assert!(b(&rt, "new CSSSkew(CSS.deg(10), CSS.deg(20)).is2D"));
    assert_eq!(s(&rt, "new CSSSkewX(CSS.deg(10)).toString()"), "skewX(10deg)");
    assert_eq!(s(&rt, "new CSSSkewY(CSS.deg(10)).toString()"), "skewY(10deg)");
}

/// `CSSPerspective` is inherently 3D regardless of its argument.
#[test]
fn perspective_is_always_3d() {
    let rt = rt_with_dom();
    assert!(!b(&rt, "new CSSPerspective(CSS.px(100)).is2D"));
    assert_eq!(s(&rt, "new CSSPerspective(CSS.px(100)).toString()"), "perspective(100px)");
}

/// `CSSMatrixComponent` wraps a `DOMMatrix`; `is2D` follows the matrix
/// unless `options.is2D` overrides it.
#[test]
fn matrix_component_wraps_dommatrix_and_honours_is2d_override() {
    let rt = rt_with_dom();
    assert!(b(&rt, "new CSSMatrixComponent(new DOMMatrix()).is2D"));
    assert!(!b(&rt, "new CSSMatrixComponent(new DOMMatrix(), { is2D: false }).is2D"));
    assert!(b(&rt, "new CSSMatrixComponent(new DOMMatrix()).matrix instanceof DOMMatrix"));
}

/// `CSSTransformValue.is2D` is true only when every component is 2D, and
/// `toMatrix()` composes them via `DOMMatrix` the same way a
/// `<transform-list>` string would.
#[test]
fn transform_value_is2d_and_to_matrix() {
    let rt = rt_with_dom();
    assert!(b(
        &rt,
        "new CSSTransformValue([new CSSTranslate(CSS.px(1), CSS.px(2))]).is2D"
    ));
    assert!(!b(
        &rt,
        r#"new CSSTransformValue([
            new CSSTranslate(CSS.px(1), CSS.px(2)),
            new CSSPerspective(CSS.px(100))
        ]).is2D"#
    ));
    // translate(10px, 0) then scale(2) — matches the equivalent transform string.
    assert!(b(
        &rt,
        r#"
        var fromComponents = new CSSTransformValue([
            new CSSTranslate(CSS.px(10), CSS.px(0)),
            new CSSScale(2, 2)
        ]).toMatrix();
        var fromString = new DOMMatrix('translate(10px, 0px) scale(2, 2)');
        fromComponents.a === fromString.a && fromComponents.d === fromString.d &&
            fromComponents.e === fromString.e && fromComponents.f === fromString.f
        "#
    ));
}

/// `CSSTransformValue` is indexable and iterable like the spec's
/// `sequence<CSSTransformComponent>` backing.
#[test]
fn transform_value_is_indexable_and_iterable() {
    let rt = rt_with_dom();
    assert_eq!(n(&rt, "new CSSTransformValue([new CSSTranslate(CSS.px(1), CSS.px(2))]).length"), 1.0);
    assert_eq!(
        s(&rt, "new CSSTransformValue([new CSSTranslate(CSS.px(1), CSS.px(2))])[0].toString()"),
        "translate(1px, 2px)"
    );
    assert_eq!(
        s(
            &rt,
            r#"
            var out = [];
            for (var c of new CSSTransformValue([new CSSSkewX(CSS.deg(5)), new CSSSkewY(CSS.deg(6))])) {
                out.push(c.toString());
            }
            out.join('|')
            "#
        ),
        "skewX(5deg)|skewY(6deg)"
    );
}

/// An empty component list is rejected, per §11.1.
#[test]
fn transform_value_rejects_empty_list() {
    let rt = rt_with_dom();
    assert!(b(
        &rt,
        r#"
        var threw = false;
        try { new CSSTransformValue([]); } catch (e) { threw = e instanceof TypeError; }
        threw
        "#
    ));
}

/// A length that needs a resolution context this shim doesn't have (a
/// percentage) throws from `toMatrix()` rather than silently treating it
/// as zero.
#[test]
fn translate_to_matrix_throws_on_unresolvable_percentage() {
    let rt = rt_with_dom();
    assert!(b(
        &rt,
        r#"
        var threw = false;
        try { new CSSTranslate(CSS.percent(50), CSS.px(0)).toMatrix(); }
        catch (e) { threw = e instanceof TypeError; }
        threw
        "#
    ));
}
