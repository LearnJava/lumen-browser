//! Screen Orientation API (W3C Screen Orientation §3-4).
//!
//! Installs `screen.orientation` with W3C-compliant orientation type/angle,
//! `.lock(orientation)` and `.unlock()` methods, and `onchange` event support.
//! Phase 0: `.lock()` requires a natively bound `_lumen_set_fullscreen` to integrate
//! with shell; orientation type is static 'portrait-primary' for now.

use rquickjs::Ctx;

/// Install Screen Orientation API shim into the JS context.
///
/// Adds `screen.orientation` object with `type`, `angle`, `lock()`, `unlock()`,
/// and `onchange` event support. All orientation types (portrait-primary,
/// landscape-primary, etc.) map to simple stubs. Lock/unlock methods return
/// Promises; actual fullscreen integration is handled by shell bindings.
///
/// Must be called **after** `install_dom_api` so that `screen`, `Promise`,
/// `DOMException`, and `EventTarget` already exist.
pub fn install_screen_orientation_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(SCREEN_ORIENTATION_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the Screen Orientation API.
const SCREEN_ORIENTATION_SHIM: &str = r#"(function() {
  'use strict';
  if (typeof screen === 'undefined') return;

  // ── ScreenOrientationEvent ──────────────────────────────────────────────────
  // Event fired when the screen orientation changes.
  function ScreenOrientationEvent(type, init) {
    this.type      = type || 'change';
    this.bubbles   = false;
    this.cancelable = false;
    this.target    = null;
  }
  ScreenOrientationEvent.prototype = Object.create(Event.prototype);
  ScreenOrientationEvent.prototype.constructor = ScreenOrientationEvent;
  globalThis.ScreenOrientationEvent = ScreenOrientationEvent;
  if (typeof window !== 'undefined') window.ScreenOrientationEvent = ScreenOrientationEvent;

  // ── ScreenOrientation ───────────────────────────────────────────────────────
  // Represents the screen orientation state.
  function ScreenOrientation() {
    this.type              = 'portrait-primary';
    this.angle             = 0;
    this._lockOrientation  = null;
    this._listeners        = {};
    this.onchange          = null;
  }

  /// Lock the screen orientation. Phase 0: resolves immediately after calling
  /// the native binding `_lumen_set_fullscreen` (if available). actual fullscreen
  /// permission and orientation enforcement is a shell concern.
  ScreenOrientation.prototype.lock = function(orientation) {
    var self = this;
    return Promise.resolve().then(function() {
      // Validate orientation string per WHATWG Screen Orientation spec.
      var validOrientations = [
        'portrait-primary',
        'portrait-secondary',
        'portrait',
        'landscape-primary',
        'landscape-secondary',
        'landscape',
        'any'
      ];
      if (validOrientations.indexOf(orientation) === -1) {
        return Promise.reject(new TypeError('Invalid orientation: ' + orientation));
      }
      self._lockOrientation = orientation;

      // Call native binding if available (shell integration point).
      if (typeof _lumen_set_fullscreen === 'function') {
        try {
          _lumen_set_fullscreen(true);
        } catch(e) {
          // Silently ignore if binding unavailable.
        }
      }

      return self;
    });
  };

  /// Unlock the screen orientation. Phase 0: resolves immediately.
  ScreenOrientation.prototype.unlock = function() {
    this._lockOrientation = null;
    if (typeof _lumen_set_fullscreen === 'function') {
      try {
        _lumen_set_fullscreen(false);
      } catch(e) {
        // Silently ignore if binding unavailable.
      }
    }
    return Promise.resolve();
  };

  /// Event listener support.
  ScreenOrientation.prototype.addEventListener = function(type, fn) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
  };

  ScreenOrientation.prototype.removeEventListener = function(type, fn) {
    if (!this._listeners[type]) return;
    this._listeners[type] = this._listeners[type].filter(function(f) { return f !== fn; });
  };

  /// Fire a change event (internal use by shell). Phase 0: not called yet.
  /// When P3 integrates device orientation, this will be invoked via a native binding.
  ScreenOrientation.prototype._fireChangeEvent = function(newType, newAngle) {
    this.type  = newType || this.type;
    this.angle = newAngle !== undefined ? newAngle : this.angle;
    var evt = new ScreenOrientationEvent('change');
    evt.target = this;

    var fns = this._listeners['change'] || [];
    fns.forEach(function(f) { try { f(evt); } catch(e) {} });

    if (this.onchange) {
      try { this.onchange(evt); } catch(e) {}
    }
  };

  // Instantiate and attach to screen object.
  var screenOrientation = new ScreenOrientation();
  screen.orientation = screenOrientation;

  globalThis.ScreenOrientation = ScreenOrientation;
  if (typeof window !== 'undefined') window.ScreenOrientation = ScreenOrientation;
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Runtime, Context};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn install_prereqs(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
            "var screen = { __proto__: {} }; \
             function Event(type) { this.type = type; }",
        )
        .unwrap();
    }

    #[test]
    fn screen_orientation_initial_state() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_screen_orientation_bindings(&ctx).unwrap();

            let result = ctx
                .eval::<bool, _>(
                    "screen.orientation.type === 'portrait-primary' && screen.orientation.angle === 0",
                )
                .unwrap();
            assert!(result, "Initial orientation should be portrait-primary at angle 0");
        });
    }

    #[test]
    fn screen_orientation_has_lock_method() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_screen_orientation_bindings(&ctx).unwrap();

            let has_lock: bool = ctx.eval("typeof screen.orientation.lock === 'function'").unwrap();
            assert!(has_lock, "lock method should exist");
        });
    }

    #[test]
    fn screen_orientation_has_unlock_method() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_screen_orientation_bindings(&ctx).unwrap();

            let has_unlock: bool = ctx
                .eval("typeof screen.orientation.unlock === 'function'")
                .unwrap();
            assert!(has_unlock, "unlock method should exist");
        });
    }

    #[test]
    fn screen_orientation_lock_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_screen_orientation_bindings(&ctx).unwrap();

            let is_promise: bool = ctx
                .eval(
                    "screen.orientation.lock('portrait-primary') instanceof Promise",
                )
                .unwrap();
            assert!(is_promise, "lock should return a Promise");
        });
    }

    #[test]
    fn screen_orientation_event_listener() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_screen_orientation_bindings(&ctx).unwrap();

            let fired: bool = ctx
                .eval(
                    r#"
                      var event_fired = false;
                      screen.orientation.addEventListener('change', function(e) {
                        event_fired = true;
                      });
                      screen.orientation._fireChangeEvent('landscape-primary', 90);
                      event_fired
                    "#,
                )
                .unwrap();
            assert!(fired, "change event listener should fire");
        });
    }

    #[test]
    fn screen_orientation_onchange_handler() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_screen_orientation_bindings(&ctx).unwrap();

            let fired: bool = ctx
                .eval(
                    r#"
                      var onchange_fired = false;
                      screen.orientation.onchange = function(e) {
                        onchange_fired = true;
                      };
                      screen.orientation._fireChangeEvent('landscape-primary', 90);
                      onchange_fired
                    "#,
                )
                .unwrap();
            assert!(fired, "onchange handler should be called");
        });
    }

    #[test]
    fn screen_orientation_updates_on_event() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_screen_orientation_bindings(&ctx).unwrap();

            let type_ok: bool = ctx
                .eval(
                    "screen.orientation._fireChangeEvent('landscape-primary', 90); screen.orientation.type === 'landscape-primary'"
                )
                .unwrap();
            let angle_ok: bool = ctx
                .eval("screen.orientation.angle === 90")
                .unwrap();
            assert!(type_ok, "type should update");
            assert!(angle_ok, "angle should update");
        });
    }

    #[test]
    fn screen_orientation_class_exported() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_screen_orientation_bindings(&ctx).unwrap();

            let exists: bool = ctx.eval("typeof ScreenOrientation === 'function'").unwrap();
            assert!(exists, "ScreenOrientation class should be exported to globalThis");
        });
    }
}
