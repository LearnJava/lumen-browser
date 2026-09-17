//! Screen Orientation API (W3C Screen Orientation §3-4).
//!
//! Installs `screen.orientation` (a real `EventTarget` subclass, per W3C Screen
//! Orientation §4) with W3C-compliant orientation type/angle, `.lock(orientation)`
//! and `.unlock()` methods, and `onchange` event support. `.lock()` resolves the
//! requested orientation to a concrete type (`'landscape'` → `'landscape-primary'`,
//! `'any'` keeps the current type), assigns it to `type`/`angle`, and fires `change`.
//! Phase 0: `.lock()` requires a natively bound `_lumen_set_fullscreen` to integrate
//! with shell; there is no real hardware/device orientation source yet, so the type
//! only ever changes via `.lock()` or the internal `_fireChangeEvent`.

/// V8 port of the former rquickjs `install_screen_orientation_bindings` (Ph3 V8 migration
/// S5-S7, rquickjs side removed in S12b-B5): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_screen_orientation_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SCREEN_ORIENTATION_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the Screen Orientation API.
#[cfg(feature = "v8-backend")]
const SCREEN_ORIENTATION_SHIM: &str = r#"(function() {
  'use strict';
  if (typeof screen === 'undefined') return;

  // ── ScreenOrientationEvent ──────────────────────────────────────────────────
  // Event fired when the screen orientation changes.
  function ScreenOrientationEvent(type, init) {
    Event.call(this, type || 'change', init);
  }
  ScreenOrientationEvent.prototype = Object.create(Event.prototype);
  ScreenOrientationEvent.prototype.constructor = ScreenOrientationEvent;
  globalThis.ScreenOrientationEvent = ScreenOrientationEvent;
  if (typeof window !== 'undefined') window.ScreenOrientationEvent = ScreenOrientationEvent;

  // Map a (possibly bare) lock argument to the concrete orientation type it
  // resolves to. 'any' keeps whatever the current type already is; 'portrait'/
  // 'landscape' resolve to their '-primary' variant, matching the fix note in
  // BUG-668 (bare values are not valid values of `type` itself).
  function resolveLockedType(orientation, currentType) {
    if (orientation === 'any') return currentType;
    if (orientation === 'portrait') return 'portrait-primary';
    if (orientation === 'landscape') return 'landscape-primary';
    return orientation;
  }

  var ANGLE_BY_TYPE = {
    'portrait-primary':    0,
    'portrait-secondary':  180,
    'landscape-primary':   90,
    'landscape-secondary': 270
  };

  // ── ScreenOrientation ───────────────────────────────────────────────────────
  // Represents the screen orientation state. `interface ScreenOrientation :
  // EventTarget` (W3C Screen Orientation §4) — inherits real dispatchEvent/
  // addEventListener/removeEventListener instead of a hand-rolled pub-sub.
  function ScreenOrientation() {
    EventTarget.call(this);
    this.type              = 'portrait-primary';
    this.angle             = 0;
    this._lockOrientation  = null;
    this.onchange          = null;
  }
  ScreenOrientation.prototype = Object.create(EventTarget.prototype);
  ScreenOrientation.prototype.constructor = ScreenOrientation;

  /// Lock the screen orientation. Phase 0: resolves after calling the native
  /// binding `_lumen_set_fullscreen` (if available) and updating type/angle to
  /// the resolved orientation, firing `change`. Actual fullscreen permission
  /// and hardware orientation enforcement is a shell concern.
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

      var resolvedType = resolveLockedType(orientation, self.type);
      self._fireChangeEvent(resolvedType, ANGLE_BY_TYPE[resolvedType]);

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

  /// Fire a change event (internal use by shell). When P3 integrates real
  /// device orientation, this will also be invoked via a native binding.
  ScreenOrientation.prototype._fireChangeEvent = function(newType, newAngle) {
    this.type  = newType || this.type;
    this.angle = newAngle !== undefined ? newAngle : this.angle;
    var evt = new ScreenOrientationEvent('change');
    this.dispatchEvent(evt);
  };

  // Instantiate and attach to screen object.
  var screenOrientation = new ScreenOrientation();
  screen.orientation = screenOrientation;

  globalThis.ScreenOrientation = ScreenOrientation;
  if (typeof window !== 'undefined') window.ScreenOrientation = ScreenOrientation;
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

    fn with_screen_orientation(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(crate::dom::EVENT_TARGET_SHIM).unwrap();
        rt.eval(
            "var screen = { __proto__: {} }; \
             function Event(type, init) { \
               this.type = type; \
               this.defaultPrevented = false; \
               this.cancelable = !!(init && init.cancelable); \
             }",
        )
        .unwrap();
        install_screen_orientation_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn screen_orientation_initial_state() {
        with_screen_orientation(|rt| {
            let result = rt
                .eval(
                    "screen.orientation.type === 'portrait-primary' && screen.orientation.angle === 0",
                )
                .unwrap();
            assert_eq!(result, JsValue::Bool(true), "Initial orientation should be portrait-primary at angle 0");
        });
    }

    #[test]
    fn screen_orientation_has_lock_method() {
        with_screen_orientation(|rt| {
            let has_lock = rt.eval("typeof screen.orientation.lock === 'function'").unwrap();
            assert_eq!(has_lock, JsValue::Bool(true), "lock method should exist");
        });
    }

    #[test]
    fn screen_orientation_has_unlock_method() {
        with_screen_orientation(|rt| {
            let has_unlock = rt
                .eval("typeof screen.orientation.unlock === 'function'")
                .unwrap();
            assert_eq!(has_unlock, JsValue::Bool(true), "unlock method should exist");
        });
    }

    #[test]
    fn screen_orientation_lock_returns_promise() {
        with_screen_orientation(|rt| {
            let is_promise = rt
                .eval(
                    "screen.orientation.lock('portrait-primary') instanceof Promise",
                )
                .unwrap();
            assert_eq!(is_promise, JsValue::Bool(true), "lock should return a Promise");
        });
    }

    #[test]
    fn screen_orientation_event_listener() {
        with_screen_orientation(|rt| {
            let fired = rt
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
            assert_eq!(fired, JsValue::Bool(true), "change event listener should fire");
        });
    }

    #[test]
    fn screen_orientation_onchange_handler() {
        with_screen_orientation(|rt| {
            let fired = rt
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
            assert_eq!(fired, JsValue::Bool(true), "onchange handler should be called");
        });
    }

    #[test]
    fn screen_orientation_updates_on_event() {
        with_screen_orientation(|rt| {
            let type_ok = rt
                .eval(
                    "screen.orientation._fireChangeEvent('landscape-primary', 90); screen.orientation.type === 'landscape-primary'"
                )
                .unwrap();
            let angle_ok = rt
                .eval("screen.orientation.angle === 90")
                .unwrap();
            assert_eq!(type_ok, JsValue::Bool(true), "type should update");
            assert_eq!(angle_ok, JsValue::Bool(true), "angle should update");
        });
    }

    #[test]
    fn screen_orientation_class_exported() {
        with_screen_orientation(|rt| {
            let exists = rt.eval("typeof ScreenOrientation === 'function'").unwrap();
            assert_eq!(exists, JsValue::Bool(true), "ScreenOrientation class should be exported to globalThis");
        });
    }

    #[test]
    fn screen_orientation_is_event_target() {
        with_screen_orientation(|rt| {
            let is_event_target = rt
                .eval(
                    "screen.orientation instanceof EventTarget && \
                     typeof screen.orientation.dispatchEvent === 'function'",
                )
                .unwrap();
            assert_eq!(is_event_target, JsValue::Bool(true), "ScreenOrientation should inherit EventTarget");
        });
    }

    #[test]
    fn screen_orientation_dispatch_event_fires_change_listener() {
        with_screen_orientation(|rt| {
            let fired = rt
                .eval(
                    r#"
                      var event_fired = false;
                      screen.orientation.addEventListener('change', function(e) {
                        event_fired = true;
                      });
                      screen.orientation.dispatchEvent(new ScreenOrientationEvent('change'));
                      event_fired
                    "#,
                )
                .unwrap();
            assert_eq!(fired, JsValue::Bool(true), "real dispatchEvent should reach a change listener");
        });
    }

    #[test]
    fn screen_orientation_lock_updates_type_and_angle() {
        with_screen_orientation(|rt| {
            rt.eval(
                r#"
                  var seen = null;
                  screen.orientation.addEventListener('change', function() { seen = screen.orientation.type; });
                  screen.orientation.lock('landscape-primary');
                "#,
            )
            .unwrap();
            // V8 runs a full microtask checkpoint after every top-level eval(), so
            // `lock()`'s `.then()` callback has already run by the time this returns.
            let type_ok = rt.eval("screen.orientation.type === 'landscape-primary'").unwrap();
            let angle_ok = rt.eval("screen.orientation.angle === 90").unwrap();
            let seen_ok = rt.eval("seen === 'landscape-primary'").unwrap();
            assert_eq!(type_ok, JsValue::Bool(true), "lock() should update type to the resolved orientation");
            assert_eq!(angle_ok, JsValue::Bool(true), "lock() should update angle to match the resolved type");
            assert_eq!(seen_ok, JsValue::Bool(true), "lock() should fire change before resolving");
        });
    }

    #[test]
    fn screen_orientation_lock_any_keeps_current_type() {
        with_screen_orientation(|rt| {
            rt.eval("screen.orientation.lock('any');").unwrap();
            let type_ok = rt.eval("screen.orientation.type === 'portrait-primary'").unwrap();
            assert_eq!(type_ok, JsValue::Bool(true), "lock('any') should not change the current type");
        });
    }
}
