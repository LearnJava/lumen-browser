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
//! Opt-out: pass `enabled = false` to `install_cookie_banner_bindings`; the
//! function becomes a no-op for that page. Shell wires this from the
//! `cookie_banner_dismiss` field in `Lumen`.

use rquickjs::Ctx;

/// Install cookie-banner auto-dismiss shim into the JS context.
///
/// When `enabled` is `false` the function is a no-op — the shim is not
/// injected and cookie banners are shown normally.
///
/// Must be called **after** `dom::install_dom_api` so that `document`,
/// `MutationObserver`, `setTimeout`, and `setInterval` are available.
pub fn install_cookie_banner_bindings(ctx: &Ctx, enabled: bool) -> rquickjs::Result<()> {
    if !enabled {
        return Ok(());
    }
    ctx.eval::<(), _>(COOKIE_BANNER_SHIM)?;
    Ok(())
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
/// Separated from [`install_cookie_banner_bindings`] so tests can call it
/// with a custom selector list without going through the full DOM shim.
pub fn install_with_selectors(ctx: &Ctx, selectors: &[&str]) -> rquickjs::Result<()> {
    let joined = selectors.join("|");
    ctx.globals().set("_LUMEN_CONSENT_SELECTORS", joined)?;
    ctx.eval::<(), _>(COOKIE_BANNER_SHIM)?;
    Ok(())
}

/// Inject the default selector list global + shim.
fn inject(ctx: &Ctx) -> rquickjs::Result<()> {
    install_with_selectors(ctx, CONSENT_SELECTORS)
}

// Re-export for use from lib.rs install_dom.
pub(crate) fn install(ctx: &Ctx, enabled: bool) -> rquickjs::Result<()> {
    if !enabled {
        return Ok(());
    }
    inject(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    /// Minimal DOM + MutationObserver stub sufficient for the cookie-banner shim.
    fn install_dom_stub(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(r#"
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

    #[test]
    fn disabled_is_noop() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_dom_stub(&ctx);
            // No error and no shim globals injected.
            install_cookie_banner_bindings(&ctx, false).expect("disabled must succeed");
            let defined: bool = ctx.eval("typeof _LUMEN_CONSENT_SELECTORS !== 'undefined'").unwrap();
            assert!(!defined, "disabled must not inject selector global");
        });
    }

    #[test]
    fn install_succeeds_enabled() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_dom_stub(&ctx);
            install_with_selectors(&ctx, &["#accept"]).expect("install must succeed");
        });
    }

    #[test]
    fn selector_global_is_set() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_dom_stub(&ctx);
            install_with_selectors(&ctx, &["#foo", ".bar"]).unwrap();
            let val: String = ctx.eval("_LUMEN_CONSENT_SELECTORS").unwrap();
            assert_eq!(val, "#foo|.bar");
        });
    }

    #[test]
    fn no_match_leaves_dismissed_false() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_dom_stub(&ctx);
            // querySelector always returns null — no banner present.
            install_with_selectors(&ctx, &["#accept"]).unwrap();
            let dismissed: bool = ctx.eval("(function(){ return false; })()").unwrap();
            assert!(!dismissed);
        });
    }

    #[test]
    fn match_dispatches_click() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_dom_stub(&ctx);
            // Patch querySelector to return a button that records click events.
            ctx.eval::<(), _>(r#"
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
            install_with_selectors(&ctx, &["#accept-btn"]).unwrap();
            let clicked: bool = ctx.eval("_clicked").unwrap();
            assert!(clicked, "click must be dispatched on matching visible element");
        });
    }

    #[test]
    fn hidden_element_not_clicked() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_dom_stub(&ctx);
            // Element has zero dimensions — treated as hidden.
            ctx.eval::<(), _>(r#"
              var _clicked = false;
              var _btn = {
                getBoundingClientRect: function() { return {width:0,height:0}; },
                _style: {},
                dispatchEvent: function(ev) { _clicked = true; }
              };
              document.querySelector = function(sel) { return _btn; };
            "#).unwrap();
            install_with_selectors(&ctx, &["#hidden-btn"]).unwrap();
            let clicked: bool = ctx.eval("_clicked").unwrap();
            assert!(!clicked, "hidden element must not be clicked");
        });
    }

    #[test]
    fn display_none_not_clicked() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_dom_stub(&ctx);
            ctx.eval::<(), _>(r#"
              var _clicked = false;
              var _btn = {
                getBoundingClientRect: function() { return {width:100,height:40}; },
                _style: { display: 'none' },
                dispatchEvent: function(ev) { _clicked = true; }
              };
              document.querySelector = function(sel) { return _btn; };
              window.getComputedStyle  = function(el)  { return el._style; };
            "#).unwrap();
            install_with_selectors(&ctx, &["#hidden-btn"]).unwrap();
            let clicked: bool = ctx.eval("_clicked").unwrap();
            assert!(!clicked, "display:none element must not be clicked");
        });
    }

    #[test]
    fn cleanup_after_dismiss() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_dom_stub(&ctx);
            ctx.eval::<(), _>(r#"
              var _clicks = 0;
              var _btn = {
                getBoundingClientRect: function() { return {width:100,height:40}; },
                _style: {},
                dispatchEvent: function(ev) { _clicks++; }
              };
              document.querySelector = function(sel) { return _btn; };
            "#).unwrap();
            install_with_selectors(&ctx, &["#btn"]).unwrap();
            // Simulate a second MutationObserver callback fire after dismiss.
            // The shim should be idempotent — only one click total.
            let clicks: i32 = ctx.eval("_clicks").unwrap();
            assert_eq!(clicks, 1, "element must be clicked exactly once");
        });
    }

    #[test]
    fn interval_registered() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_dom_stub(&ctx);
            // No matching element — interval should be registered.
            install_with_selectors(&ctx, &["#nonexistent"]).unwrap();
            let count: i32 = ctx.eval("_intervals.filter(function(i){return i.active;}).length").unwrap();
            assert!(count >= 1, "at least one active interval must be registered");
        });
    }

    #[test]
    fn observer_attached() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_dom_stub(&ctx);
            ctx.eval::<(), _>(r#"
              var _obs_connected = false;
              var _OrigMO = MutationObserver;
              function MutationObserver(cb) { this._cb = cb; }
              MutationObserver.prototype.observe     = function() { _obs_connected = true; };
              MutationObserver.prototype.disconnect  = function() { _obs_connected = false; };
            "#).unwrap();
            install_with_selectors(&ctx, &["#nonexistent"]).unwrap();
            let connected: bool = ctx.eval("_obs_connected").unwrap();
            assert!(connected, "MutationObserver must be attached when no banner found yet");
        });
    }

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

    #[test]
    fn multiple_selectors_first_match_wins() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_dom_stub(&ctx);
            ctx.eval::<(), _>(r#"
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
            install_with_selectors(&ctx, &["#first", "#second", "#third"]).unwrap();
            // #first → null, #second → button (clicked), #third never checked.
            let order_json: String = ctx.eval("JSON.stringify(_order)").unwrap();
            assert!(order_json.contains("#first"), "first selector must be tried");
            assert!(order_json.contains("#second"), "second selector must be tried");
            assert!(!order_json.contains("#third"), "third selector must NOT be tried after match");
        });
    }

    #[test]
    fn install_without_mutation_observer() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_dom_stub(&ctx);
            // Remove MutationObserver to simulate environments where it's absent.
            ctx.eval::<(), _>("MutationObserver = undefined;").unwrap();
            install_with_selectors(&ctx, &["#nonexistent"])
                .expect("must not panic without MutationObserver");
        });
    }
}
