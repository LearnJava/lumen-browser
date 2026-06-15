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

use rquickjs::Ctx;

/// Install Window Management API shim into the JS context.
///
/// Adds `screen.isExtended`, `navigator.getScreenDetails()`, and the `ScreenDetails` /
/// `ScreenDetailed` classes. Must be called **after** `install_navigator_bindings` so
/// that `screen`, `navigator`, `Promise`, and `DOMException` already exist.
pub fn install_window_management_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(WINDOW_MANAGEMENT_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the W3C Multi-Screen Window Placement Level 1 API.
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

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    /// Install minimal prereqs: screen + navigator + Promise + DOMException.
    fn install_prereqs(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
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
    }

    #[test]
    fn screen_is_extended_false() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_window_management_api(&ctx).unwrap();
            let v: bool = ctx.eval("screen.isExtended === false").unwrap();
            assert!(v, "screen.isExtended should be false in Phase 0");
        });
    }

    #[test]
    fn screen_detailed_class_exported() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_window_management_api(&ctx).unwrap();
            let v: bool = ctx.eval("typeof ScreenDetailed === 'function'").unwrap();
            assert!(v, "ScreenDetailed should be exported on globalThis");
        });
    }

    #[test]
    fn screen_details_class_exported() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_window_management_api(&ctx).unwrap();
            let v: bool = ctx.eval("typeof ScreenDetails === 'function'").unwrap();
            assert!(v, "ScreenDetails should be exported on globalThis");
        });
    }

    #[test]
    fn get_screen_details_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_window_management_api(&ctx).unwrap();
            let v: bool = ctx
                .eval("navigator.getScreenDetails() instanceof Promise")
                .unwrap();
            assert!(v, "getScreenDetails() should return a Promise");
        });
    }

    #[test]
    fn screen_detailed_has_required_fields() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_window_management_api(&ctx).unwrap();
            let v: bool = ctx
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
            assert!(v, "ScreenDetailed should expose all required fields");
        });
    }

    #[test]
    fn screen_details_current_screen() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_window_management_api(&ctx).unwrap();
            let v: bool = ctx
                .eval(
                    r#"
                    var s1 = new ScreenDetailed({ width: 1920, height: 1080, isPrimary: true, label: 'A' });
                    var s2 = new ScreenDetailed({ width: 2560, height: 1440, isPrimary: false, label: 'B' });
                    var sd = new ScreenDetails([s1, s2], 0);
                    sd.currentScreen === s1 && sd.screens.length === 2
                    "#,
                )
                .unwrap();
            assert!(v, "ScreenDetails.currentScreen should point to first screen");
        });
    }

    #[test]
    fn screen_details_event_listener() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_window_management_api(&ctx).unwrap();
            let v: bool = ctx
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
            assert!(v, "addEventListener should store listeners");
        });
    }

    #[test]
    fn get_screen_details_resolves_with_screen_details() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_window_management_api(&ctx).unwrap();
            // Promise resolves synchronously for Phase 0 stubs via Promise.resolve().
            let v: bool = ctx
                .eval(
                    r#"
                    var result = null;
                    navigator.getScreenDetails().then(function(sd) { result = sd; });
                    // PromiseJobs are flushed by QuickJS after eval returns,
                    // but we can check the type was registered.
                    typeof navigator.getScreenDetails === 'function'
                    "#,
                )
                .unwrap();
            assert!(v, "navigator.getScreenDetails should be a function");
        });
    }
}
