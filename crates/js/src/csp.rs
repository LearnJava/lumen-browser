//! Content Security Policy Level 3 JS bindings.
//! <https://www.w3.org/TR/CSP3/#violation-events>
//!
//! Phase 0: `SecurityPolicyViolationEvent` class and a native binding that
//! dispatches it on `document`.  No enforcement — the shell wires actual
//! blocking in Phase 1 via `_lumen_fire_csp_violation`.

/// Install CSP JS bindings: `SecurityPolicyViolationEvent` class and
/// `_lumen_dispatch_csp_violation` native dispatch helper.
///
/// Must run after the DOM shim so that `Event`, `window` and `document` are
/// already defined. Evaluates the JS shim via
/// [`lumen_core::ext::JsRuntime::eval`] on the default (V8) engine.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_csp_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(CSP_SHIM)?;
    Ok(())
}

/// JavaScript shim: SecurityPolicyViolationEvent + fire helper.
#[cfg(feature = "v8-backend")]
const CSP_SHIM: &str = r#"
(function() {
  // ── SecurityPolicyViolationEvent (CSP3 §7.8) ────────────────────────────
  // Extends Event; carries all properties defined in the violation report.
  class SecurityPolicyViolationEvent extends Event {
    constructor(type, init) {
      super(type || 'securitypolicyviolation', {
        bubbles:    true,
        composed:   true,
        cancelable: false
      });
      var i = init || {};
      this.documentURI        = i.documentURI       || (typeof location !== 'undefined' ? location.href : '');
      this.referrer           = i.referrer           || (typeof document !== 'undefined' ? document.referrer : '');
      this.blockedURI         = i.blockedURI         || '';
      this.violatedDirective  = i.violatedDirective  || '';
      this.effectiveDirective = i.effectiveDirective || i.violatedDirective || '';
      this.originalPolicy     = i.originalPolicy     || '';
      this.disposition        = i.disposition        || 'enforce';
      this.statusCode         = i.statusCode         !== undefined ? i.statusCode : 0;
      this.sample             = i.sample             || '';
      this.sourceFile         = i.sourceFile         || '';
      this.lineNumber         = i.lineNumber         || 0;
      this.columnNumber       = i.columnNumber       || 0;
    }
  }
  window.SecurityPolicyViolationEvent = SecurityPolicyViolationEvent;

  // ── _lumen_fire_csp_violation (native binding hook) ─────────────────────
  // Called by the Rust shell (Phase 1) when it detects a policy violation.
  // directive      — violated directive name, e.g. "script-src"
  // blockedUri     — blocked URI, e.g. "inline" for inline scripts
  // originalPolicy — full serialised policy string
  // disposition    — "enforce" | "report"
  //
  // Phase 0: this JS helper is defined so the event class is available;
  // the Rust binding `_lumen_fire_csp_violation` will forward here in Phase 1.
  window._lumen_dispatch_csp_violation = function(directive, blockedUri, originalPolicy, disposition) {
    if (typeof document === 'undefined') { return; }
    var evt = new SecurityPolicyViolationEvent('securitypolicyviolation', {
      blockedURI:         blockedUri,
      violatedDirective:  directive,
      effectiveDirective: directive,
      originalPolicy:     originalPolicy,
      disposition:        disposition || 'enforce',
      statusCode:         0
    });
    document.dispatchEvent(evt);
  };
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Set up a minimal DOM stub (`window`, `Event`, `document`, `location`) plus
    /// the CSP shim on a bare V8 runtime — the shim only needs `Event`, `window`
    /// and `document` defined, so no full `install_dom` is required. Evals on one
    /// runtime share global state, so `_dispatched` persists across `eval` calls.
    fn with_csp_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            r#"
            globalThis.window = globalThis;
            globalThis.location = { href: 'https://example.com/' };
            globalThis._dispatched = [];
            globalThis.document = {
              referrer: '',
              dispatchEvent: function(e) { _dispatched.push(e); }
            };
            function Event(type, init) {
              this.type = type;
              this.bubbles    = (init && init.bubbles)    || false;
              this.composed   = (init && init.composed)   || false;
              this.cancelable = (init && init.cancelable) || false;
            }
            globalThis.Event = Event;
            "#,
        )
        .unwrap();
        install_csp_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn security_policy_violation_event_class_exists() {
        with_csp_api(|rt| {
            let ok = rt
                .eval("typeof window.SecurityPolicyViolationEvent === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn event_has_correct_type() {
        with_csp_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var e = new SecurityPolicyViolationEvent('securitypolicyviolation', {
                      blockedURI: 'inline',
                      violatedDirective: 'script-src',
                      originalPolicy: "script-src 'none'",
                      disposition: 'enforce'
                    });
                    e.type === 'securitypolicyviolation'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn event_has_violated_directive() {
        with_csp_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var e = new SecurityPolicyViolationEvent('securitypolicyviolation', {
                      violatedDirective: 'script-src',
                      blockedURI: 'inline',
                      originalPolicy: "script-src 'none'"
                    });
                    e.violatedDirective === 'script-src' &&
                    e.effectiveDirective === 'script-src' &&
                    e.blockedURI === 'inline'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn event_disposition_defaults_to_enforce() {
        with_csp_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var e = new SecurityPolicyViolationEvent('securitypolicyviolation', {
                      violatedDirective: 'img-src',
                      originalPolicy: "img-src 'none'"
                    });
                    e.disposition === 'enforce'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn dispatch_helper_exists() {
        with_csp_api(|rt| {
            let ok = rt
                .eval("typeof window._lumen_dispatch_csp_violation === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn dispatch_helper_fires_event_on_document() {
        with_csp_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    _lumen_dispatch_csp_violation('script-src', 'inline', "script-src 'none'", 'enforce');
                    _dispatched.length === 1 && _dispatched[0].violatedDirective === 'script-src'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
