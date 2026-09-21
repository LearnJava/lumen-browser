//! GAP-TYPEDOM срез 5 — `CSSNumericValue.to()`/`.equals()` (§7.1.4-§7.1.5),
//! the remainder left after срезы 1-4 (unit factories, `CSSMathValue`
//! construction, `parse`/`parseAll`, `StylePropertyMap.set`/`.append`,
//! `CSSTransformValue`, `CSSColorValue`). `to()` resolves a math tree by
//! evaluation, not the full §8.5 numeric-type algorithm; `equals()` is
//! structural tree comparison and needs no resolution context.

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

/// `to()` on a bare `CSSUnitValue` still works the same as before this slice
/// (it now dispatches through `CSSNumericValue.prototype.to`, not its own
/// override).
#[test]
fn unit_value_to_converts_within_a_group() {
    let rt = rt_with_dom();
    assert_eq!(s(&rt, "CSS.cm(1).to('px').toString()"), "37.79527559055118px");
    assert!(b(
        &rt,
        r#"
        var threw = false;
        try { CSS.px(1).to('percent'); } catch (e) { threw = e instanceof TypeError; }
        threw
        "#
    ));
}

/// A `CSSMathSum` of same-group units resolves to a single `CSSUnitValue` in
/// the requested unit.
#[test]
fn math_sum_to_resolves_mixed_same_group_units() {
    let rt = rt_with_dom();
    assert_eq!(s(&rt, "CSS.px(10).add(CSS.cm(1)).to('px').toString()"), "47.79527559055118px");
    assert!(b(&rt, "CSS.px(10).add(CSS.cm(1)).to('px') instanceof CSSUnitValue"));
}

/// Summing incompatible unit groups (px + deg) has no single-unit
/// resolution — `to()` throws rather than guessing.
#[test]
fn math_sum_to_rejects_incompatible_groups() {
    let rt = rt_with_dom();
    assert!(b(
        &rt,
        r#"
        var threw = false;
        try { CSS.px(1).add(CSS.deg(1)).to('px'); } catch (e) { threw = e instanceof TypeError; }
        threw
        "#
    ));
}

/// A product with exactly one non-`<number>` operand resolves — the
/// `<number>` operand acts as a scalar multiplier.
#[test]
fn math_product_to_resolves_single_non_number_operand() {
    let rt = rt_with_dom();
    assert_eq!(s(&rt, "CSS.px(10).mul(3).to('px').toString()"), "30px");
}

/// A product with two non-`<number>` operands (px * px) has no single-unit
/// representation — throws instead of inventing a compound unit.
#[test]
fn math_product_to_rejects_two_non_number_operands() {
    let rt = rt_with_dom();
    assert!(b(
        &rt,
        r#"
        var threw = false;
        try { CSS.px(2).mul(CSS.px(3)).to('px'); } catch (e) { threw = e instanceof TypeError; }
        threw
        "#
    ));
}

/// `min()`/`max()` resolve like `sum()` — convert every operand to a shared
/// unit, then pick the extremum.
#[test]
fn math_min_max_to_resolve() {
    let rt = rt_with_dom();
    assert_eq!(s(&rt, "CSS.px(10).min(CSS.px(4)).to('px').toString()"), "4px");
    assert_eq!(s(&rt, "CSS.px(10).max(CSS.px(4)).to('px').toString()"), "10px");
}

/// `negate()` flips the sign; `invert()` only resolves for a `<number>`
/// operand (inverting a length has no single-unit representation here).
#[test]
fn math_negate_and_invert_to() {
    let rt = rt_with_dom();
    assert_eq!(s(&rt, "CSS.px(5).negate().to('px').toString()"), "-5px");
    assert_eq!(s(&rt, "CSS.number(4).invert().to('number').toString()"), "0.25");
    assert!(b(
        &rt,
        r#"
        var threw = false;
        try { CSS.px(5).invert().to('px'); } catch (e) { threw = e instanceof TypeError; }
        threw
        "#
    ));
}

/// `equals()` is structural: two `CSSUnitValue`s with the same unit/value are
/// equal, a different unit or value is not, and the check works across an
/// n-ary argument list.
#[test]
fn equals_compares_unit_values_structurally() {
    let rt = rt_with_dom();
    assert!(b(&rt, "CSS.px(1).equals(CSS.px(1))"));
    assert!(!b(&rt, "CSS.px(1).equals(CSS.px(2))"));
    assert!(!b(&rt, "CSS.px(1).equals(CSS.percent(1))"));
    assert!(b(&rt, "CSS.px(1).equals(CSS.px(1), CSS.px(1))"));
    assert!(!b(&rt, "CSS.px(1).equals(CSS.px(1), CSS.px(2))"));
}

/// `equals()` on a math tree compares operator and operand order — it does
/// not resolve units, so `1cm + 1px` and an equivalent-valued `1px` sum are
/// not equal despite `to()` agreeing on their resolved value.
#[test]
fn equals_compares_math_trees_structurally() {
    let rt = rt_with_dom();
    assert!(b(&rt, "CSS.px(1).add(CSS.px(2)).equals(CSS.px(1).add(CSS.px(2)))"));
    assert!(!b(&rt, "CSS.px(1).add(CSS.px(2)).equals(CSS.px(2).add(CSS.px(1)))"));
    assert!(!b(&rt, "CSS.px(1).add(CSS.px(2)).equals(CSS.px(3))"));
}
