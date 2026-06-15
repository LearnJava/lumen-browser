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

use rquickjs::Ctx;

/// Install Digital Credentials API stubs into the JS context.
///
/// Must run after the credentials shim so that the `container.get()` path
/// is already wired (this module monkey-patches the `options.digital` branch).
pub fn install_digital_credentials_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(DIGITAL_CREDENTIALS_SHIM)?;
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn install_prereqs(ctx: &rquickjs::Ctx) {
        // Minimal credentials stub
        ctx.eval::<(), _>(
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
        install_digital_credentials_api(ctx).unwrap();
    }

    #[test]
    fn digital_credential_class_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            let ok: bool = ctx.eval("typeof DigitalCredential === 'function'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn digital_credential_constructor_throws() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var threw = false;
                    try { new DigitalCredential(); } catch(e) { threw = e instanceof TypeError; }
                    threw
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn credentials_get_digital_returns_rejected_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            // The returned value must be a Promise (which is pre-rejected)
            let ok: bool = ctx
                .eval(
                    r#"
                    var p = navigator.credentials.get({ digital: { providers: [] } });
                    p instanceof Promise
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn digital_credential_get_binding_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            let ok: bool = ctx
                .eval("typeof _lumen_digital_credential_get === 'function'")
                .unwrap();
            assert!(ok);
        });
    }
}
