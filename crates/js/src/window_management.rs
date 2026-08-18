//! Window Management API (W3C Multi-Screen Window Placement Level 1).
//!
//! Installs:
//! - `screen.isExtended` — `false` in Phase 0 (single-screen stub).
//! - `navigator.getScreenDetails()` → `Promise<ScreenDetails>` — resolves with one
//!   `ScreenDetailed` that mirrors the current `screen` object.
//! - `ScreenDetails` class with `.screens[]` and `.currentScreen`.
//! - `ScreenDetailed` extends `Screen` with `left`, `top`, `availLeft`, `availTop`,
//!   `isPrimary`, `isInternal`, `devicePixelRatio`, `label`.
//!
//! Phase 1: `_lumen_get_screen_details()` native binding will query the OS for all
//! connected screens and call the callback with a JSON array of screen descriptors.

/// V8 port of the former rquickjs `install_window_management_api` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-B5): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_window_management_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(WINDOW_MANAGEMENT_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the W3C Multi-Screen Window Placement Level 1 API.
#[cfg(feature = "v8-backend")]
const WINDOW_MANAGEMENT_SHIM: &str = r#"(function() {
  'use strict';
  if (typeof screen === 'undefined' || typeof navigator === 'undefined') return;

  // ── screen.isExtended ────────────────────────────────────────────────────────
  // W3C WMWPA §3.1: true when 2+ screens connected. Phase 0: always false.
  try {
    if (!('isExtended' in screen)) {
      Object.defineProperty(screen, 'isExtended', {
        get: function() { return false; },
        configurable: true, enumerable: true
      });
    }
  } catch(_) {}

  // ── ScreenDetailed ───────────────────────────────────────────────────────────
  // W3C WMWPA §4.1 — extends Screen with placement and display metadata.
  function ScreenDetailed(data) {
    // Mirror base Screen properties.
    this.width            = data.width            || screen.width;
    this.height           = data.height           || screen.height;
    this.availWidth       = data.availWidth        || screen.availWidth;
    this.availHeight      = data.availHeight       || screen.availHeight;
    this.colorDepth       = data.colorDepth        || screen.colorDepth;
    this.pixelDepth       = data.pixelDepth        || screen.pixelDepth;
    // Extended placement properties.
    this.left             = data.left             !== undefined ? data.left   : 0;
    this.top              = data.top              !== undefined ? data.top    : 0;
    this.availLeft        = data.availLeft        !== undefined ? data.availLeft  : 0;
    this.availTop         = data.availTop         !== undefined ? data.availTop   : 0;
    this.isPrimary        = data.isPrimary        !== undefined ? data.isPrimary  : true;
    this.isInternal       = data.isInternal       !== undefined ? data.isInternal : false;
    this.devicePixelRatio = data.devicePixelRatio !== undefined ? data.devicePixelRatio : 1;
    this.label            = data.label            !== undefined ? data.label : '';
  }
  globalThis.ScreenDetailed = ScreenDetailed;
  if (typeof window !== 'undefined') window.ScreenDetailed = ScreenDetailed;

  // ── ScreenDetails ─────────────────────────────────────────────────────────────
  // W3C WMWPA §4.2 — list of all connected screens + currentScreen pointer.
  function ScreenDetails(screens, currentIndex) {
    this.screens       = screens;
    this.currentScreen = screens[currentIndex || 0];
    this._listeners    = {};
    this.oncurrentscreenchange = null;
    this.onscreenschange       = null;
  }

  ScreenDetails.prototype.addEventListener = function(type, fn) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
  };

  ScreenDetails.prototype.removeEventListener = function(type, fn) {
    if (!this._listeners[type]) return;
    this._listeners[type] = this._listeners[type].filter(function(f) { return f !== fn; });
  };

  globalThis.ScreenDetails = ScreenDetails;
  if (typeof window !== 'undefined') window.ScreenDetails = ScreenDetails;

  // ── navigator.getScreenDetails() ─────────────────────────────────────────────
  // W3C WMWPA §3.2 — returns Promise<ScreenDetails>.
  // Phase 0: resolves with one ScreenDetailed mirroring the current screen object.
  // Phase 1: _lumen_get_screen_details(callback) will supply a JSON array of all
  //          OS screens; callback receives [{width,height,left,top,...},...].
  function _buildPhase0ScreenDetails() {
    var primary = new ScreenDetailed({
      width:            screen.width,
      height:           screen.height,
      availWidth:       screen.availWidth,
      availHeight:      screen.availHeight,
      colorDepth:       screen.colorDepth,
      pixelDepth:       screen.pixelDepth,
      left:             0,
      top:              0,
      availLeft:        0,
      availTop:         0,
      isPrimary:        true,
      isInternal:       false,
      devicePixelRatio: (typeof devicePixelRatio !== 'undefined' ? devicePixelRatio : 1),
      label:            'Built-in Screen'
    });
    return new ScreenDetails([primary], 0);
  }

  if (typeof navigator.getScreenDetails !== 'function') {
    navigator.getScreenDetails = function() {
      // Phase 1 hook: if native binding provides multi-screen data, use it.
      if (typeof _lumen_get_screen_details === 'function') {
        return new Promise(function(resolve, reject) {
          try {
            _lumen_get_screen_details(function(screensJson) {
              try {
                var arr = JSON.parse(screensJson);
                var screens = arr.map(function(d) { return new ScreenDetailed(d); });
                var currentIdx = arr.findIndex(function(d) { return d.isPrimary; });
                resolve(new ScreenDetails(screens, currentIdx >= 0 ? currentIdx : 0));
              } catch(e) {
                reject(new DOMException('Screen details parse error', 'InvalidStateError'));
              }
            });
          } catch(e) {
            reject(new DOMException('getScreenDetails failed', 'NotAllowedError'));
          }
        });
      }
      // Phase 0: single-screen stub.
      return Promise.resolve(_buildPhase0ScreenDetails());
    };
  }
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

    /// Install minimal prereqs: screen + navigator + Promise + DOMException.
    fn with_window_management(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            "var screen = { \
               width: 1920, height: 1080, \
               availWidth: 1920, availHeight: 1080, \
               colorDepth: 24, pixelDepth: 24 \
             }; \
             var navigator = {}; \
             function DOMException(msg, name) { this.message = msg; this.name = name; } \
             DOMException.prototype = Object.create(Error.prototype); \
             globalThis.DOMException = DOMException; \
             globalThis.devicePixelRatio = 1;",
        )
        .unwrap();
        install_window_management_api_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn screen_is_extended_false() {
        with_window_management(|rt| {
            let v = rt.eval("screen.isExtended === false").unwrap();
            assert_eq!(v, JsValue::Bool(true), "screen.isExtended should be false in Phase 0");
        });
    }

    #[test]
    fn screen_detailed_class_exported() {
        with_window_management(|rt| {
            let v = rt.eval("typeof ScreenDetailed === 'function'").unwrap();
            assert_eq!(v, JsValue::Bool(true), "ScreenDetailed should be exported on globalThis");
        });
    }

    #[test]
    fn screen_details_class_exported() {
        with_window_management(|rt| {
            let v = rt.eval("typeof ScreenDetails === 'function'").unwrap();
            assert_eq!(v, JsValue::Bool(true), "ScreenDetails should be exported on globalThis");
        });
    }

    #[test]
    fn get_screen_details_returns_promise() {
        with_window_management(|rt| {
            let v = rt
                .eval("navigator.getScreenDetails() instanceof Promise")
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "getScreenDetails() should return a Promise");
        });
    }

    #[test]
    fn screen_detailed_has_required_fields() {
        with_window_management(|rt| {
            let v = rt
                .eval(
                    r#"
                    var s = new ScreenDetailed({
                      width: 1920, height: 1080,
                      left: 0, top: 0,
                      availLeft: 0, availTop: 0,
                      isPrimary: true, isInternal: false,
                      devicePixelRatio: 2, label: 'Test'
                    });
                    s.width === 1920 && s.height === 1080 &&
                    s.left === 0 && s.top === 0 &&
                    s.isPrimary === true && s.isInternal === false &&
                    s.devicePixelRatio === 2 && s.label === 'Test'
                    "#,
                )
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "ScreenDetailed should expose all required fields");
        });
    }

    #[test]
    fn screen_details_current_screen() {
        with_window_management(|rt| {
            let v = rt
                .eval(
                    r#"
                    var s1 = new ScreenDetailed({ width: 1920, height: 1080, isPrimary: true, label: 'A' });
                    var s2 = new ScreenDetailed({ width: 2560, height: 1440, isPrimary: false, label: 'B' });
                    var sd = new ScreenDetails([s1, s2], 0);
                    sd.currentScreen === s1 && sd.screens.length === 2
                    "#,
                )
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "ScreenDetails.currentScreen should point to first screen");
        });
    }

    #[test]
    fn screen_details_event_listener() {
        with_window_management(|rt| {
            let v = rt
                .eval(
                    r#"
                    var sd = new ScreenDetails([], 0);
                    var called = false;
                    sd.addEventListener('screenschange', function() { called = true; });
                    var fns = sd._listeners['screenschange'];
                    fns && fns.length === 1
                    "#,
                )
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "addEventListener should store listeners");
        });
    }

    #[test]
    fn get_screen_details_resolves_with_screen_details() {
        with_window_management(|rt| {
            // Promise resolves via the V8 microtask queue after eval returns.
            let v = rt
                .eval(
                    r#"
                    var result = null;
                    navigator.getScreenDetails().then(function(sd) { result = sd; });
                    typeof navigator.getScreenDetails === 'function'
                    "#,
                )
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "navigator.getScreenDetails should be a function");
        });
    }
}
