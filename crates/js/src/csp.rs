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
    _lumen_send_csp_reports(originalPolicy, evt);
  };

  // ── report-uri delivery (CSP3 §5.5 "report violation") ──────────────────
  // GAP-CSPENF срез 14: `CspPolicy.report_uri` is parsed on the Rust side
  // (`crates/network/src/csp.rs`) but nothing crosses the Rust/JS boundary to
  // carry it to every enforcement call site — script-src/img-src/style-src
  // live in `crates/shell`, connect-src/worker-src live in `lumen-network`
  // behind a native side channel (срезы 10-13). Every one of them already
  // threads `originalPolicy` (the combined header+meta text) through to here,
  // so re-extracting `report-uri` from that string avoids widening the
  // boundary a sixth time.
  //
  // ── report-to delivery (Reporting API v0) ────────────────────────────────
  // GAP-CSPENF срез 60: unlike `report-uri`, a `report-to <group>` directive
  // carries only a group NAME in the policy text — the URLs live in a
  // separate `Report-To` response header, parsed into `Document::
  // report_to_endpoints` by срез 59 (`page_source::report_to_endpoints`).
  // Reading it via `_lumen_get_report_to_endpoints_json` (one native call per
  // violation, `crates/js/src/v8_runtime/install/dom_core.rs`) keeps the
  // boundary at the same single choke point срез 59 already crosses, instead
  // of widening every `fire_*_violation` call site with a sixth argument the
  // way `originalPolicy` itself is threaded.
  window._lumen_send_csp_reports = function(originalPolicy, evt) {
    if (typeof fetch !== 'function' || typeof URL !== 'function') { return; }
    var base = (typeof document !== 'undefined' && document.baseURI) ||
               (typeof location !== 'undefined' ? location.href : undefined);
    var body = JSON.stringify({
      'csp-report': {
        'document-uri':       evt.documentURI,
        'referrer':            evt.referrer,
        'violated-directive':  evt.violatedDirective,
        'effective-directive': evt.effectiveDirective,
        'original-policy':     originalPolicy,
        'disposition':         evt.disposition,
        'blocked-uri':         evt.blockedURI,
        'status-code':         evt.statusCode
      }
    });
    var send = function(u) {
      var target;
      try { target = new URL(u, base).href; } catch (e) { return; }
      fetch(target, {
        method:  'POST',
        headers: { 'Content-Type': 'application/csp-report' },
        body:    body
      }).catch(function() {});
    };
    var uriMatch = /(?:^|;)\s*report-uri\s+([^;]+)/i.exec(originalPolicy || '');
    if (uriMatch) {
      uriMatch[1].trim().split(/\s+/).filter(Boolean).forEach(send);
    }
    var toMatch = /(?:^|;)\s*report-to\s+(\S+)/i.exec(originalPolicy || '');
    if (toMatch && typeof _lumen_get_report_to_endpoints_json === 'function') {
      var group = toMatch[1];
      var endpoints;
      try { endpoints = JSON.parse(_lumen_get_report_to_endpoints_json()); }
      catch (e) { endpoints = null; }
      if (endpoints && Array.isArray(endpoints[group])) {
        endpoints[group].forEach(send);
      }
    }
  };
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
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

    /// Same rig as [`with_csp_api`] plus a mock `fetch`/`URL` that records
    /// every call into `_reports` instead of touching the network — for the
    /// report-uri delivery tests below. `URL` only resolves absolute and
    /// root-relative (`/path`) inputs, the only shapes the tests use.
    fn with_csp_api_and_report_mock(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            r#"
            globalThis.window = globalThis;
            globalThis.location = { href: 'https://example.com/page' };
            globalThis._dispatched = [];
            globalThis._reports = [];
            globalThis.document = {
              baseURI: 'https://example.com/page',
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
            function URL(u, base) {
              this.href = /^https?:\/\//.test(u) ? u : (base.match(/^(https?:\/\/[^/]+)/)[1] + u);
            }
            globalThis.URL = URL;
            globalThis.fetch = function(target, init) {
              _reports.push({ target: target, init: init });
              return Promise.resolve({ ok: true });
            };
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

    /// GAP-CSPENF срез 14: a policy with `report-uri` POSTs a `csp-report`
    /// JSON body to it, resolved against `document.baseURI`.
    #[test]
    fn report_uri_posts_report_to_endpoint() {
        with_csp_api_and_report_mock(|rt| {
            let ok = rt
                .eval(
                    r#"
                    _lumen_dispatch_csp_violation('script-src', 'inline',
                      "script-src 'none'; report-uri /csp-report", 'enforce');
                    _reports.length === 1 &&
                    _reports[0].target === 'https://example.com/csp-report' &&
                    _reports[0].init.method === 'POST' &&
                    _reports[0].init.headers['Content-Type'] === 'application/csp-report' &&
                    JSON.parse(_reports[0].init.body)['csp-report']['violated-directive'] === 'script-src' &&
                    JSON.parse(_reports[0].init.body)['csp-report']['blocked-uri'] === 'inline' &&
                    JSON.parse(_reports[0].init.body)['csp-report']['original-policy'] ===
                      "script-src 'none'; report-uri /csp-report"
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    /// Multiple whitespace-separated URIs in `report-uri` each get their own
    /// POST.
    #[test]
    fn report_uri_posts_to_every_listed_endpoint() {
        with_csp_api_and_report_mock(|rt| {
            let ok = rt
                .eval(
                    r#"
                    _lumen_dispatch_csp_violation('img-src', 'https://evil.example/x.png',
                      "img-src 'none'; report-uri /a /b", 'enforce');
                    _reports.length === 2 &&
                    _reports[0].target === 'https://example.com/a' &&
                    _reports[1].target === 'https://example.com/b'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    /// No `report-uri` directive in the policy — no reports sent.
    #[test]
    fn no_report_uri_sends_no_reports() {
        with_csp_api_and_report_mock(|rt| {
            let ok = rt
                .eval(
                    r#"
                    _lumen_dispatch_csp_violation('script-src', 'inline', "script-src 'none'", 'enforce');
                    _reports.length === 0
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    /// No `fetch`/`URL` in the runtime (the plain [`with_csp_api`] rig) — the
    /// dispatch helper must not throw even though the policy has
    /// `report-uri`.
    #[test]
    fn report_uri_without_fetch_does_not_throw() {
        with_csp_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    _lumen_dispatch_csp_violation('script-src', 'inline',
                      "script-src 'none'; report-uri /csp-report", 'enforce');
                    _dispatched.length === 1
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    /// Same rig as [`with_csp_api_and_report_mock`] plus a stub
    /// `_lumen_get_report_to_endpoints_json` (in production installed by
    /// `install_document_meta`, `crates/js/src/v8_runtime/install/
    /// dom_core.rs`) returning the given group→URLs JSON map — for the
    /// `report-to` delivery tests (GAP-CSPENF срез 60).
    fn with_report_to_mock(endpoints_json: &str, f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(&format!(
            r#"
            globalThis.window = globalThis;
            globalThis.location = {{ href: 'https://example.com/page' }};
            globalThis._dispatched = [];
            globalThis._reports = [];
            globalThis.document = {{
              baseURI: 'https://example.com/page',
              referrer: '',
              dispatchEvent: function(e) {{ _dispatched.push(e); }}
            }};
            function Event(type, init) {{
              this.type = type;
              this.bubbles    = (init && init.bubbles)    || false;
              this.composed   = (init && init.composed)   || false;
              this.cancelable = (init && init.cancelable) || false;
            }}
            globalThis.Event = Event;
            function URL(u, base) {{
              this.href = /^https?:\/\//.test(u) ? u : (base.match(/^(https?:\/\/[^/]+)/)[1] + u);
            }}
            globalThis.URL = URL;
            globalThis.fetch = function(target, init) {{
              _reports.push({{ target: target, init: init }});
              return Promise.resolve({{ ok: true }});
            }};
            globalThis._lumen_get_report_to_endpoints_json = function() {{
              return {};
            }};
            "#,
            js_string_literal_for_test(endpoints_json),
        ))
        .unwrap();
        install_csp_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    fn js_string_literal_for_test(s: &str) -> String {
        format!("{:?}", s)
    }

    /// GAP-CSPENF срез 60: `report-to <group>` resolves the group name
    /// against the `_lumen_get_report_to_endpoints_json` map and POSTs the
    /// same `csp-report` body to every URL of that group.
    #[test]
    fn report_to_posts_to_named_group_endpoints() {
        with_report_to_mock(
            r#"{"csp-endpoint":["https://example.com/report-collector"]}"#,
            |rt| {
                let ok = rt
                    .eval(
                        r#"
                        _lumen_dispatch_csp_violation('script-src', 'inline',
                          "script-src 'none'; report-to csp-endpoint", 'enforce');
                        _reports.length === 1 &&
                        _reports[0].target === 'https://example.com/report-collector' &&
                        JSON.parse(_reports[0].init.body)['csp-report']['violated-directive'] === 'script-src'
                        "#,
                    )
                    .unwrap();
                assert_eq!(ok, JsValue::Bool(true));
            },
        );
    }

    /// `report-to` names a group absent from the endpoints map — no report
    /// is sent (nothing to send it to), and dispatch still does not throw.
    #[test]
    fn report_to_unknown_group_sends_no_reports() {
        with_report_to_mock(r#"{"other-group":["https://example.com/x"]}"#, |rt| {
            let ok = rt
                .eval(
                    r#"
                    _lumen_dispatch_csp_violation('script-src', 'inline',
                      "script-src 'none'; report-to csp-endpoint", 'enforce');
                    _reports.length === 0 && _dispatched.length === 1
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    /// `report-uri` and `report-to` both present in the same policy — both
    /// deliveries fire independently.
    #[test]
    fn report_uri_and_report_to_both_fire() {
        with_report_to_mock(
            r#"{"csp-endpoint":["https://example.com/collector"]}"#,
            |rt| {
                let ok = rt
                    .eval(
                        r#"
                        _lumen_dispatch_csp_violation('script-src', 'inline',
                          "script-src 'none'; report-uri /csp-report; report-to csp-endpoint", 'enforce');
                        _reports.length === 2 &&
                        _reports.some(function(r) { return r.target === 'https://example.com/csp-report'; }) &&
                        _reports.some(function(r) { return r.target === 'https://example.com/collector'; })
                        "#,
                    )
                    .unwrap();
                assert_eq!(ok, JsValue::Bool(true));
            },
        );
    }
}
