//! SVG DOM API stubs (W3C SVG 2 §3, §10, §11)
//! Phase 0: SVGElement/SVGSVGElement class hierarchy, getBBox() → DOMRect(zeros),
//! document.createElementNS('http://www.w3.org/2000/svg', ...) patched to return
//! typed SVG element instances. SVGRect/SVGPoint/SVGLength/SVGAnimatedLength types.

/// Install SVG DOM API bindings into a V8 runtime (Ph3 V8 migration S5-S7;
/// the rquickjs twin was removed in S12b-B25).
#[cfg(feature = "v8-backend")]
pub(crate) fn install_svg_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SVG_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const SVG_SHIM: &str = concat!(
  include_str!("shim/svg_shim_head.js"),
  "
  const _IDL = ",
  include_str!("shim/svg_idl_table.js"),
  ";
",
  include_str!("shim/svg_idl_shape.js"),
  r#"})();
"#
);

#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8;
