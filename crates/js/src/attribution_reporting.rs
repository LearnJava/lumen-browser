//! Attribution Reporting API stub (Privacy Sandbox).
//!
//! Exposes `window.attributionReporting` and the `attributionsrc` attribute
//! as defined by the WICG Attribution Reporting API proposal.
//!
//! Phase 0 scope (no real attribution measurement):
//! - `window.attributionReporting` object with stub methods:
//!   - `registerSource(sourceData)` → `Promise<undefined>` — no-op.
//!   - `registerTrigger(triggerData)` → `Promise<undefined>` — no-op.
//! - `attributionSrc` IDL attribute on `HTMLAnchorElement` and
//!   `HTMLImageElement` that mirrors the `attributionsrc` content attribute.
//! - `AttributionReportingEligibility` constants object.
//!
//! Phase 1: wire `_lumen_attribution_register_source` and
//! `_lumen_attribution_register_trigger` native hooks to the actual
//! cross-site-measurement reporting pipeline.

/// V8 port of the former rquickjs `install_attribution_reporting_api` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-5): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_attribution_reporting_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(ATTRIBUTION_REPORTING_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const ATTRIBUTION_REPORTING_SHIM: &str = r#"
(function(global) {
  'use strict';

  // AttributionReportingEligibility constants.
  // Spec: https://wicg.github.io/attribution-reporting-api/#attribution-reporting-eligibility
  var AttributionReportingEligibility = Object.freeze({
    EMPTY:            '',
    EVENT_SOURCE:     'event-source',
    NAVIGATION_SOURCE:'navigation-source',
    TRIGGER:          'trigger',
  });
  global.AttributionReportingEligibility = AttributionReportingEligibility;

  // window.attributionReporting — main API object.
  // Phase 0: all mutating methods resolve immediately as no-ops.
  // Phase 1: call _lumen_attribution_register_source / _lumen_attribution_register_trigger.
  var attributionReporting = {
    /**
     * Register an attribution source (impression).
     * @param {object} sourceData
     * @returns {Promise<undefined>}
     */
    registerSource: function registerSource(_sourceData) {
      // Phase 1: _lumen_attribution_register_source(JSON.stringify(_sourceData))
      return Promise.resolve(undefined);
    },

    /**
     * Register an attribution trigger (conversion).
     * @param {object} triggerData
     * @returns {Promise<undefined>}
     */
    registerTrigger: function registerTrigger(_triggerData) {
      // Phase 1: _lumen_attribution_register_trigger(JSON.stringify(_triggerData))
      return Promise.resolve(undefined);
    },
  };

  Object.defineProperty(global, 'attributionReporting', {
    value: attributionReporting,
    writable: false,
    configurable: true,
    enumerable: true,
  });

  // attributionSrc IDL attribute on HTMLAnchorElement and HTMLImageElement.
  // Maps to the "attributionsrc" content attribute.
  // Spec: https://wicg.github.io/attribution-reporting-api/#dom-htmlanchorelement-attributionsrc
  function defineAttributionSrc(proto) {
    if (!proto) return;
    Object.defineProperty(proto, 'attributionSrc', {
      get: function() {
        return this.getAttribute('attributionsrc') || '';
      },
      set: function(val) {
        this.setAttribute('attributionsrc', val);
      },
      configurable: true,
      enumerable: true,
    });
  }

  if (typeof global.HTMLAnchorElement !== 'undefined') {
    defineAttributionSrc(global.HTMLAnchorElement.prototype);
  }
  if (typeof global.HTMLImageElement !== 'undefined') {
    defineAttributionSrc(global.HTMLImageElement.prototype);
  }
  if (typeof global.HTMLScriptElement !== 'undefined') {
    defineAttributionSrc(global.HTMLScriptElement.prototype);
  }

})(typeof globalThis !== 'undefined' ? globalThis : this);
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_attribution_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        // Minimal DOM shim.
        rt.eval(
            r#"
            var window = globalThis;
            if (typeof globalThis.document === 'undefined') {
              globalThis.document = {};
            }
            // Stub HTMLAnchorElement with prototype and getAttribute/setAttribute.
            function makeElement() {
              var attrs = {};
              return {
                getAttribute: function(k) { return attrs[k] !== undefined ? attrs[k] : null; },
                setAttribute: function(k, v) { attrs[k] = v; },
              };
            }
            function HTMLAnchorElement() {}
            HTMLAnchorElement.prototype = makeElement();
            globalThis.HTMLAnchorElement = HTMLAnchorElement;

            function HTMLImageElement() {}
            HTMLImageElement.prototype = makeElement();
            globalThis.HTMLImageElement = HTMLImageElement;

            function HTMLScriptElement() {}
            HTMLScriptElement.prototype = makeElement();
            globalThis.HTMLScriptElement = HTMLScriptElement;
            "#,
        )
        .unwrap();
        install_attribution_reporting_api_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn attribution_reporting_object_exists() {
        with_attribution_api(|rt| {
            let ok = rt
                .eval("typeof attributionReporting === 'object' && attributionReporting !== null")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn register_source_is_function() {
        with_attribution_api(|rt| {
            let ok = rt
                .eval("typeof attributionReporting.registerSource === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn register_trigger_is_function() {
        with_attribution_api(|rt| {
            let ok = rt
                .eval("typeof attributionReporting.registerTrigger === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn register_source_returns_promise_resolving_undefined() {
        with_attribution_api(|rt| {
            rt.eval(
                "var __src = 'pending'; attributionReporting.registerSource({}).then(function(v) { __src = v; });",
            )
            .unwrap();
            let ok = rt.eval("__src === undefined").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn register_trigger_returns_promise_resolving_undefined() {
        with_attribution_api(|rt| {
            rt.eval(
                "var __trig = 'pending'; attributionReporting.registerTrigger({}).then(function(v) { __trig = v; });",
            )
            .unwrap();
            let ok = rt.eval("__trig === undefined").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn attribution_reporting_eligibility_constants_exist() {
        with_attribution_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    typeof AttributionReportingEligibility === 'object' &&
                    AttributionReportingEligibility.EVENT_SOURCE === 'event-source' &&
                    AttributionReportingEligibility.TRIGGER === 'trigger'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn attribution_src_idl_attribute_on_anchor() {
        with_attribution_api(|rt| {
            rt.eval(
                r#"
                var a = new HTMLAnchorElement();
                a.attributionSrc = 'https://example.com/report';
                var __asr = a.attributionSrc;
                var __attr = a.getAttribute('attributionsrc');
                "#,
            )
            .unwrap();
            let ok = rt
                .eval("__asr === 'https://example.com/report' && __attr === 'https://example.com/report'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn attribution_src_idl_attribute_on_image() {
        with_attribution_api(|rt| {
            rt.eval(
                r#"
                var img = new HTMLImageElement();
                img.attributionSrc = 'https://ad.example/pixel';
                var __imgasr = img.attributionSrc;
                "#,
            )
            .unwrap();
            let ok = rt.eval("__imgasr === 'https://ad.example/pixel'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
