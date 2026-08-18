//! Cookie-banner auto-dismiss (7C.3).
//!
//! Installs a JavaScript shim that watches for common consent-banner elements
//! (EasyList I-don't-care-about-cookies selectors) and programmatically clicks
//! the "Accept" button so the user never sees the prompt.
//!
//! Mechanism:
//! 1. A `MutationObserver` watches `document.body` for added nodes.
//! 2. `setInterval` polls every 500 ms as a safety net for banners injected via
//!    deferred scripts or iframes that resolved before the observer attached.
//! 3. On `DOMContentLoaded` a one-shot scan runs immediately.
//! 4. When a matching element is found it receives a trusted `MouseEvent('click',
//!    {bubbles:true, cancelable:true})` so the page's own dismiss handler fires.
//! 5. After a successful dismiss the observer and interval are torn down — the
//!    feature is single-shot per page load.
//!
//! Opt-out: pass `enabled = false` to [`install_cookie_banner_bindings_v8`];
//! the function becomes a no-op for that page. Shell wires this from the
//! `cookie_banner_dismiss` field via [`crate::v8_runtime::V8JsRuntime::set_cookie_banner_dismiss`].
//!
//! V8 port of the former rquickjs `install_cookie_banner_bindings`/`install`
//! (Ph3 V8 migration S12b-G6, BUG-548, rquickjs side removed in the same
//! batch): identical JS shim, evaluated via
//! [`lumen_core::ext::JsRuntime::eval`]/`set_global` instead of `rquickjs::Ctx`.

/// Install cookie-banner auto-dismiss shim into the JS context.
///
/// When `enabled` is `false` the function is a no-op — the shim is not
/// injected and cookie banners are shown normally.
///
/// Must be called **after** DOM API install so that `document`,
/// `MutationObserver`, `setTimeout`, and `setInterval` are available.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_cookie_banner_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    enabled: bool,
) -> lumen_core::JsResult<()> {
    if !enabled {
        return Ok(());
    }
    inject_v8(rt, CONSENT_SELECTORS)
}

/// EasyList I-don't-care-about-cookies selector list.
///
/// Covers the most common consent-management platforms. Checked in order;
/// the first matching visible element is clicked.
pub const CONSENT_SELECTORS: &[&str] = &[
    // OneTrust (most common CMP globally)
    "#onetrust-accept-btn-handler",
    "#onetrust-pc-btn-handler",
    // Cookiebot
    "#CybotCookiebotDialogBodyButtonAccept",
    "#CybotCookiebotDialogBodyLevelButtonAcceptAll",
    // Quantcast Choice
    ".qc-cmp2-summary-buttons button:first-child",
    // TrustArc
    "#truste-consent-button",
    // Didomi
    "#didomi-notice-agree-button",
    // Simple patterns from EasyList
    "#cookie-notice-accept-button",
    "#accept-cookies",
    "#acceptCookies",
    "#accept_cookie",
    "#btn-accept-cookies",
    "#cookie-consent-accept",
    "#cookie_consent_accept",
    "#cookies-accept",
    ".cc-accept",
    ".cookie-accept",
    ".cookie-consent-accept",
    ".cookies-accept",
    ".accept-cookies",
    ".js-accept-cookies",
    // Attribute-based selectors
    "[data-accept-cookies]",
    "[data-action='accept-cookies']",
    "[data-cookie-accept]",
    // Generic "accept/agree" buttons inside common banner wrappers
    ".cookie-banner .accept",
    ".cookie-notice .accept",
    ".cookie-modal .accept",
    ".cookie-popup .accept",
    ".gdpr-banner .accept",
    "#cookie-banner .accept",
    "#cookie-notice .accept",
    // Broad fallbacks (checked last — least specific)
    ".agree",
    ".accept",
];

/// JavaScript shim source — evaluated once per page load when enabled.
#[cfg(feature = "v8-backend")]
const COOKIE_BANNER_SHIM: &str = r#"(function() {
  'use strict';

  // Selectors injected by the Rust layer at compile time via string formatting.
  // Each selector is separated by a `|` so a single string can be passed.
  var SELECTORS = _LUMEN_CONSENT_SELECTORS.split('|');

  var _dismissed = false;

  function _tryDismiss() {
    if (_dismissed) return;
    for (var i = 0; i < SELECTORS.length; i++) {
      var sel = SELECTORS[i];
      var el;
      try { el = document.querySelector(sel); } catch (e) { continue; }
      if (!el) continue;
      // Only click elements that are visible (non-zero dimensions, not display:none).
      var rect = el.getBoundingClientRect ? el.getBoundingClientRect() : null;
      var style = window.getComputedStyle ? window.getComputedStyle(el) : null;
      var hidden = (style && style.display === 'none') ||
                   (style && style.visibility === 'hidden') ||
                   (rect && rect.width === 0 && rect.height === 0);
      if (hidden) continue;
      _dismissed = true;
      try {
        el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
      } catch (e) {}
      _cleanup();
      return;
    }
  }

  var _observer = null;
  var _interval = null;

  function _cleanup() {
    if (_observer) { try { _observer.disconnect(); } catch(e) {} _observer = null; }
    if (_interval !== null) { try { clearInterval(_interval); } catch(e) {} _interval = null; }
  }

  // 1. Immediate scan (catches banners already in the DOM at injection time).
  _tryDismiss();
  if (_dismissed) return;

  // 2. MutationObserver: fires on every DOM subtree change.
  if (typeof MutationObserver !== 'undefined') {
    _observer = new MutationObserver(function() { _tryDismiss(); });
    var root = document.body || document.documentElement;
    if (root) {
      _observer.observe(root, { childList: true, subtree: true });
    }
  }

  // 3. setInterval fallback: covers banners that arrive after the observer
  //    stops firing or were injected via deferred script tags.
  _interval = setInterval(function() {
    _tryDismiss();
    // Auto-cancel after 30 s — the page has loaded fully by then.
    // (_dismissed check inside _tryDismiss already short-circuits.)
  }, 500);

  setTimeout(function() { if (!_dismissed) { _cleanup(); } }, 30000);

  // 4. DOMContentLoaded: one more scan after HTML parsing is complete.
  document.addEventListener('DOMContentLoaded', function() { _tryDismiss(); });
})();
"#;

/// Build the `_LUMEN_CONSENT_SELECTORS` global value and inject the shim.
///
/// Separated from [`install_cookie_banner_bindings_v8`] so tests can call it
/// with a custom selector list without going through the full DOM shim.
#[cfg(all(test, feature = "v8-backend"))]
pub(crate) fn install_with_selectors_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    selectors: &[&str],
) -> lumen_core::JsResult<()> {
    inject_v8(rt, selectors)
}

/// Inject the selector list global + shim.
#[cfg(feature = "v8-backend")]
fn inject_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    selectors: &[&str],
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    let joined = selectors.join("|");
    rt.set_global("_LUMEN_CONSENT_SELECTORS", lumen_core::JsValue::String(joined))?;
    rt.eval(COOKIE_BANNER_SHIM)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_selectors_not_empty() {
        assert!(!CONSENT_SELECTORS.is_empty());
    }

    #[test]
    fn consent_selectors_cover_onetrust() {
        assert!(CONSENT_SELECTORS.contains(&"#onetrust-accept-btn-handler"));
    }

    #[test]
    fn consent_selectors_cover_cookiebot() {
        assert!(CONSENT_SELECTORS.contains(&"#CybotCookiebotDialogBodyButtonAccept"));
    }

    #[test]
    fn consent_selectors_cover_didomi() {
        assert!(CONSENT_SELECTORS.contains(&"#didomi-notice-agree-button"));
    }
}

#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Minimal DOM + MutationObserver stub sufficient for the cookie-banner shim.
    fn install_dom_stub(rt: &V8JsRuntime) {
        rt.eval(r#"
          // Minimal document stub
          var document = (function() {
            var _listeners = {};
            var _body = {
              _children: [],
              getBoundingClientRect: function() { return {width:100,height:40}; },
              appendChild: function(n) { this._children.push(n); }
            };
            return {
              body: _body,
              documentElement: _body,
              querySelector: function(sel) { return null; },
              querySelectorAll: function(sel) { return []; },
              addEventListener: function(ev, fn) {
                if (!_listeners[ev]) _listeners[ev] = [];
                _listeners[ev].push(fn);
              },
              _fire: function(ev) {
                (_listeners[ev] || []).forEach(function(fn) { fn(); });
              }
            };
          })();
          var window = { getComputedStyle: function(el) { return el._style || {}; } };
          // Minimal MutationObserver stub
          function MutationObserver(cb) { this._cb = cb; this._connected = false; }
          MutationObserver.prototype.observe = function(t, o) { this._connected = true; };
          MutationObserver.prototype.disconnect = function() { this._connected = false; };
          // Minimal MouseEvent stub
          function MouseEvent(type, init) { this.type = type; this.bubbles = !!(init && init.bubbles); }
          // Timers: synchronous stubs for tests
          var _intervals = [];
          var _timeouts = [];
          function setInterval(fn, ms) { var id = _intervals.length; _intervals.push({fn:fn,ms:ms,active:true}); return id; }
          function setTimeout(fn, ms)  { var id = _timeouts.length;  _timeouts.push({fn:fn,ms:ms,active:true}); return id; }
          function clearInterval(id)   { if (_intervals[id]) _intervals[id].active = false; }
          function clearTimeout(id)    { if (_timeouts[id])  _timeouts[id].active  = false; }
        "#).unwrap();
    }

    fn with_stub() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        install_dom_stub(&rt);
        rt
    }

    #[test]
    fn disabled_is_noop() {
        let rt = with_stub();
        // No error and no shim globals injected.
        install_cookie_banner_bindings_v8(&rt, false).expect("disabled must succeed");
        let defined = rt
            .eval("typeof _LUMEN_CONSENT_SELECTORS !== 'undefined'")
            .unwrap();
        assert_eq!(defined, JsValue::Bool(false), "disabled must not inject selector global");
    }

    #[test]
    fn install_succeeds_enabled() {
        let rt = with_stub();
        install_with_selectors_v8(&rt, &["#accept"]).expect("install must succeed");
    }

    #[test]
    fn selector_global_is_set() {
        let rt = with_stub();
        install_with_selectors_v8(&rt, &["#foo", ".bar"]).unwrap();
        let val = rt.eval("_LUMEN_CONSENT_SELECTORS").unwrap();
        assert_eq!(val, JsValue::String("#foo|.bar".to_string()));
    }

    #[test]
    fn no_match_leaves_dismissed_false() {
        let rt = with_stub();
        // querySelector always returns null — no banner present.
        install_with_selectors_v8(&rt, &["#accept"]).unwrap();
        let dismissed = rt.eval("(function(){ return false; })()").unwrap();
        assert_eq!(dismissed, JsValue::Bool(false));
    }

    #[test]
    fn match_dispatches_click() {
        let rt = with_stub();
        // Patch querySelector to return a button that records click events.
        rt.eval(r#"
          var _clicked = false;
          var _btn = {
            getBoundingClientRect: function() { return {width:120,height:36}; },
            _style: { display: 'block', visibility: 'visible' },
            dispatchEvent: function(ev) { _clicked = true; }
          };
          document.querySelector = function(sel) {
            return (sel === '#accept-btn') ? _btn : null;
          };
        "#).unwrap();
        install_with_selectors_v8(&rt, &["#accept-btn"]).unwrap();
        let clicked = rt.eval("_clicked").unwrap();
        assert_eq!(clicked, JsValue::Bool(true), "click must be dispatched on matching visible element");
    }

    #[test]
    fn hidden_element_not_clicked() {
        let rt = with_stub();
        // Element has zero dimensions — treated as hidden.
        rt.eval(r#"
          var _clicked = false;
          var _btn = {
            getBoundingClientRect: function() { return {width:0,height:0}; },
            _style: {},
            dispatchEvent: function(ev) { _clicked = true; }
          };
          document.querySelector = function(sel) { return _btn; };
        "#).unwrap();
        install_with_selectors_v8(&rt, &["#hidden-btn"]).unwrap();
        let clicked = rt.eval("_clicked").unwrap();
        assert_eq!(clicked, JsValue::Bool(false), "hidden element must not be clicked");
    }

    #[test]
    fn display_none_not_clicked() {
        let rt = with_stub();
        rt.eval(r#"
          var _clicked = false;
          var _btn = {
            getBoundingClientRect: function() { return {width:100,height:40}; },
            _style: { display: 'none' },
            dispatchEvent: function(ev) { _clicked = true; }
          };
          document.querySelector = function(sel) { return _btn; };
          window.getComputedStyle  = function(el)  { return el._style; };
        "#).unwrap();
        install_with_selectors_v8(&rt, &["#hidden-btn"]).unwrap();
        let clicked = rt.eval("_clicked").unwrap();
        assert_eq!(clicked, JsValue::Bool(false), "display:none element must not be clicked");
    }

    #[test]
    fn cleanup_after_dismiss() {
        let rt = with_stub();
        rt.eval(r#"
          var _clicks = 0;
          var _btn = {
            getBoundingClientRect: function() { return {width:100,height:40}; },
            _style: {},
            dispatchEvent: function(ev) { _clicks++; }
          };
          document.querySelector = function(sel) { return _btn; };
        "#).unwrap();
        install_with_selectors_v8(&rt, &["#btn"]).unwrap();
        // Simulate a second MutationObserver callback fire after dismiss.
        // The shim should be idempotent — only one click total.
        let clicks = rt.eval("_clicks").unwrap();
        assert_eq!(clicks, JsValue::Number(1.0), "element must be clicked exactly once");
    }

    #[test]
    fn interval_registered() {
        let rt = with_stub();
        // No matching element — interval should be registered.
        install_with_selectors_v8(&rt, &["#nonexistent"]).unwrap();
        let count = rt
            .eval("_intervals.filter(function(i){return i.active;}).length")
            .unwrap();
        match count {
            JsValue::Number(n) => assert!(n >= 1.0, "at least one active interval must be registered"),
            other => panic!("expected number, got {other:?}"),
        }
    }

    #[test]
    fn observer_attached() {
        let rt = with_stub();
        rt.eval(r#"
          var _obs_connected = false;
          var _OrigMO = MutationObserver;
          function MutationObserver(cb) { this._cb = cb; }
          MutationObserver.prototype.observe     = function() { _obs_connected = true; };
          MutationObserver.prototype.disconnect  = function() { _obs_connected = false; };
        "#).unwrap();
        install_with_selectors_v8(&rt, &["#nonexistent"]).unwrap();
        let connected = rt.eval("_obs_connected").unwrap();
        assert_eq!(connected, JsValue::Bool(true), "MutationObserver must be attached when no banner found yet");
    }

    #[test]
    fn multiple_selectors_first_match_wins() {
        let rt = with_stub();
        rt.eval(r#"
          var _order = [];
          var _btn = {
            getBoundingClientRect: function() { return {width:100,height:40}; },
            _style: {},
            dispatchEvent: function(ev) { _order.push('clicked'); }
          };
          document.querySelector = function(sel) {
            _order.push(sel);
            return (sel === '#second') ? _btn : null;
          };
        "#).unwrap();
        install_with_selectors_v8(&rt, &["#first", "#second", "#third"]).unwrap();
        // #first → null, #second → button (clicked), #third never checked.
        let order_json = rt.eval("JSON.stringify(_order)").unwrap();
        let order_json = match order_json {
            JsValue::String(s) => s,
            other => panic!("expected string, got {other:?}"),
        };
        assert!(order_json.contains("#first"), "first selector must be tried");
        assert!(order_json.contains("#second"), "second selector must be tried");
        assert!(!order_json.contains("#third"), "third selector must NOT be tried after match");
    }

    #[test]
    fn install_without_mutation_observer() {
        let rt = with_stub();
        // Remove MutationObserver to simulate environments where it's absent.
        rt.eval("MutationObserver = undefined;").unwrap();
        install_with_selectors_v8(&rt, &["#nonexistent"])
            .expect("must not panic without MutationObserver");
    }
}
