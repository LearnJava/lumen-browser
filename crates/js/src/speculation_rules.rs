//! Speculation Rules API Phase 0 (W3C Speculation Rules §3).
//!
//! Exposes:
//! - `document.prerendering` — boolean, `false` in Phase 0 (no prerendering support).
//! - `prerenderingchange` event on document — fired (never in Phase 0).
//! - `document.getSpeculationRules()` — returns `[]` Phase 0.
//!
//! The `<script type="speculationrules">` JSON block is not consumed by the engine
//! in Phase 0 (no background prefetch/prerender). The shell may hook into the
//! `_lumen_deliver_speculation_rules(json)` native binding for Phase 1 to implement
//! resource hints.

/// V8 port of the former rquickjs `install_speculation_rules_api` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-6): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_speculation_rules_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SPECULATION_RULES_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const SPECULATION_RULES_SHIM: &str = r#"(function() {
  'use strict';

  // document.prerendering (Speculation Rules §2.3):
  // true if the current document is being prerendered (not yet activated).
  // Phase 0: always false — Lumen does not yet prerender pages.
  if (typeof document !== 'undefined') {
    if (!Object.getOwnPropertyDescriptor(document, 'prerendering')) {
      Object.defineProperty(document, 'prerendering', {
        configurable: true,
        enumerable: true,
        get: function() { return false; }
      });
    }

    // document.getSpeculationRules() → [] Phase 0.
    // Phase 1: return parsed speculation rules from <script type="speculationrules">.
    if (typeof document.getSpeculationRules !== 'function') {
      document.getSpeculationRules = function() { return []; };
    }

    // Expose onprerenderingchange as a document event handler (§2.3.4).
    if (!Object.getOwnPropertyDescriptor(document, 'onprerenderingchange')) {
      Object.defineProperty(document, 'onprerenderingchange', {
        configurable: true,
        enumerable: true,
        get: function() { return document._onprerenderingchange || null; },
        set: function(fn) { document._onprerenderingchange = typeof fn === 'function' ? fn : null; }
      });
    }
  }

  // _lumen_deliver_speculation_rules(rulesJson):
  // Shell Phase 1 hook — called after HTML parsing when a
  // <script type="speculationrules"> block is encountered.
  // Phase 0: no-op. Phase 1: parse JSON, schedule prefetch/prerender hints.
  if (typeof globalThis._lumen_deliver_speculation_rules === 'undefined') {
    globalThis._lumen_deliver_speculation_rules = function(_rulesJson) {
      // Phase 0 no-op
    };
  }

})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_speculation_rules_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            r#"
            if (typeof document === 'undefined') {
                var document = { _listeners: {} };
                document.addEventListener = function(t, fn) {
                    (this._listeners[t] = this._listeners[t] || []).push(fn);
                };
            }
            "#,
        )
        .unwrap();
        install_speculation_rules_api_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn document_prerendering_is_false() {
        with_speculation_rules_api(|rt| {
            let ok = rt.eval("document.prerendering === false").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn document_get_speculation_rules_returns_empty() {
        with_speculation_rules_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    typeof document.getSpeculationRules === 'function'
                      && Array.isArray(document.getSpeculationRules())
                      && document.getSpeculationRules().length === 0
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn deliver_speculation_rules_is_noop() {
        with_speculation_rules_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    typeof globalThis._lumen_deliver_speculation_rules === 'function'
                      && (globalThis._lumen_deliver_speculation_rules('{}'), true)
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn onprerenderingchange_property() {
        with_speculation_rules_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    document.onprerenderingchange === null
                      && (document.onprerenderingchange = function(){}, true)
                      && typeof document.onprerenderingchange === 'function'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
