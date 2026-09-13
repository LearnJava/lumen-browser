//! MathML DOM API stub (MathML Core §2.2 `MathMLElement`).
//! GAP-XMLDOC срез 7 (BUG-685): unlike SVG, MathML Core defines exactly one
//! interface — `MathMLElement` — for every element in the namespace, no
//! per-tag subclasses. This closes the gap slice 6 left open: parser-built
//! `<math>` subtrees (and `document.createElementNS('http://www.w3.org/1998/Math/MathML', ...)`)
//! get a typed prototype (`instanceof MathMLElement`) instead of the bare
//! `Element.prototype` — mirrors slice 4's SVG prototype wiring, but has no
//! typed-method surface to add: MathML Core leaves layout/rendering members
//! (if any) off this interface entirely.

/// Install MathML DOM API bindings into a V8 runtime.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_mathml_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(MATHML_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const MATHML_SHIM: &str = r#"
(function() {
  'use strict';

  // MathMLElement (MathML Core §2.2) — the single interface every MathML
  // element implements, script-constructed or parser-built alike. No
  // `focus`/`blur` stub is needed the way `SVGElement` carries one: MathML
  // elements are not focusable per spec, and the shared wrapper factory
  // (`_lumen_build_element`, web_api_shim_mid.js) already provides
  // `dataset`/`style`/event-target members generically — this class exists
  // only so `_lumen_element_prototype_for` has a typed prototype to hand out.
  class MathMLElement extends (typeof Element !== 'undefined' ? Element : Object) {}
  window.MathMLElement = MathMLElement;

  window.MATHML_NAMESPACE = 'http://www.w3.org/1998/Math/MathML';
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    use crate::v8_runtime::V8JsRuntime;

    /// Install a minimal `Element` stub then MathML bindings.
    fn with_mathml() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(r#"
            var window = globalThis;
            class Element {}
            window.Element = Element;
        "#).unwrap();
        super::install_mathml_bindings_v8(&rt).unwrap();
        rt
    }

    fn bool_eval(rt: &V8JsRuntime, expr: &str) -> bool {
        matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
    }

    #[test]
    fn mathml_element_class_exists() {
        let rt = with_mathml();
        assert!(bool_eval(&rt, "typeof window.MathMLElement === 'function'"));
    }

    #[test]
    fn mathml_element_extends_element() {
        let rt = with_mathml();
        assert!(bool_eval(&rt, "new window.MathMLElement() instanceof window.Element"));
    }

    #[test]
    fn mathml_namespace_constant_is_set() {
        let rt = with_mathml();
        assert!(bool_eval(&rt, "window.MATHML_NAMESPACE === 'http://www.w3.org/1998/Math/MathML'"));
    }
}
