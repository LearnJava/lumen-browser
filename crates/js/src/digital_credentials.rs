//! Digital Credentials API Phase 0 (W3C Digital Credentials Level 1).
//!
//! Enables websites to request digital identity credentials (e.g., mobile driver's
//! license, ISO/IEC 18013-5 mDL) via `navigator.credentials.get({digital: ...})`.
//!
//! Phase 0: rejects with `NotSupportedError` — Lumen does not have an identity
//! wallet integration. The classes and rejection path are present for feature
//! detection (`'DigitalCredential' in window`).
//!
//! Phase 1: native binding `_lumen_digital_credential_get(requestJson)` for
//! OS wallet integration (Android Credential Manager / iOS AuthenticationServices).

/// V8 port of the former rquickjs `install_digital_credentials_api` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-3): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_digital_credentials_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(DIGITAL_CREDENTIALS_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const DIGITAL_CREDENTIALS_SHIM: &str = r#"(function() {
  'use strict';

  // ── DigitalCredential class (§4.3) ────────────────────────────────────────

  if (typeof DigitalCredential === 'undefined') {
    function DigitalCredential() {
      // Constructor is not directly callable by web content (§4.3.1).
      throw new TypeError('DigitalCredential constructor is not directly accessible');
    }
    DigitalCredential.prototype.type = 'digital';
    globalThis.DigitalCredential = DigitalCredential;
  }

  // ── navigator.credentials.get({digital: ...}) hook (§5.1) ────────────────
  // Intercept options.digital inside the existing credentials container shim.

  if (typeof navigator !== 'undefined' && navigator.credentials &&
      typeof navigator.credentials._get_original === 'undefined') {
    var _orig = navigator.credentials.get;
    navigator.credentials._get_original = _orig;
    navigator.credentials.get = function(options) {
      if (options && options.digital != null) {
        // Phase 0: reject — no digital wallet integration
        return Promise.reject(
          new DOMException('Digital credential requests are not supported in this browser', 'NotSupportedError')
        );
      }
      return _orig.apply(this, arguments);
    };
  }

  // ── _lumen_digital_credential_get ────────────────────────────────────────
  // Phase 1 native binding stub (no-op until OS wallet integration).
  if (typeof globalThis._lumen_digital_credential_get === 'undefined') {
    globalThis._lumen_digital_credential_get = function(_requestJson) {
      return null; // Phase 1: return JSON response from OS wallet
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

    fn with_digital_credentials(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            r#"
            if (typeof DOMException === 'undefined') {
                function DOMException(msg, name) {
                    var e = new Error(msg); e.name = name || 'DOMException'; return e;
                }
                globalThis.DOMException = DOMException;
            }
            if (typeof navigator === 'undefined') {
                var navigator = {};
            }
            if (!navigator.credentials) {
                navigator.credentials = {
                    get: function(opts) {
                        return Promise.reject(new DOMException('Not supported', 'NotSupportedError'));
                    }
                };
            }
            "#,
        )
        .unwrap();
        install_digital_credentials_api_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn digital_credential_class_exists() {
        with_digital_credentials(|rt| {
            let ok = rt.eval("typeof DigitalCredential === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn digital_credential_constructor_throws() {
        with_digital_credentials(|rt| {
            let ok = rt
                .eval(
                    r#"
                    (function() {
                        var threw = false;
                        try { new DigitalCredential(); } catch(e) { threw = e instanceof TypeError; }
                        return threw;
                    })()
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn credentials_get_digital_returns_rejected_promise() {
        with_digital_credentials(|rt| {
            // The returned value must be a Promise (which is pre-rejected)
            let ok = rt
                .eval(
                    r#"
                    (function() {
                        var p = navigator.credentials.get({ digital: { providers: [] } });
                        return p instanceof Promise;
                    })()
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn digital_credential_get_binding_exists() {
        with_digital_credentials(|rt| {
            let ok = rt
                .eval("typeof _lumen_digital_credential_get === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
