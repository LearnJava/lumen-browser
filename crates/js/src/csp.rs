//! Content Security Policy Level 3 JS bindings.
//! <https://www.w3.org/TR/CSP3/#violation-events>
//!
//! Phase 0: `SecurityPolicyViolationEvent` class and a native binding that
//! dispatches it on `document`.  No enforcement — the shell wires actual
//! blocking in Phase 1 via `_lumen_fire_csp_violation`.

use rquickjs::Ctx;

/// Install CSP JS bindings: `SecurityPolicyViolationEvent` class and
/// `_lumen_fire_csp_violation` native dispatch helper.
pub fn install_csp_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(CSP_SHIM)?;
    Ok(())
}

/// JavaScript shim: SecurityPolicyViolationEvent + fire helper.
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

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn with_csp_api(f: impl FnOnce(&rquickjs::Ctx)) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            // Minimal DOM stub
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;
                var location = { href: 'https://example.com/' };
                var _dispatched = [];
                var document = {
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
            install_csp_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn security_policy_violation_event_class_exists() {
        with_csp_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.SecurityPolicyViolationEvent === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn event_has_correct_type() {
        with_csp_api(|ctx| {
            let ok: bool = ctx
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
            assert!(ok);
        });
    }

    #[test]
    fn event_has_violated_directive() {
        with_csp_api(|ctx| {
            let ok: bool = ctx
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
            assert!(ok);
        });
    }

    #[test]
    fn event_disposition_defaults_to_enforce() {
        with_csp_api(|ctx| {
            let ok: bool = ctx
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
            assert!(ok);
        });
    }

    #[test]
    fn dispatch_helper_exists() {
        with_csp_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window._lumen_dispatch_csp_violation === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn dispatch_helper_fires_event_on_document() {
        with_csp_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    _lumen_dispatch_csp_violation('script-src', 'inline', "script-src 'none'", 'enforce');
                    _dispatched.length === 1 && _dispatched[0].violatedDirective === 'script-src'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
