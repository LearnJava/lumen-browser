//! Gamepad API (W3C Gamepad Level 2 §4).
//!
//! Installs `navigator.getGamepads()` and associated interfaces so that
//! game-oriented pages can probe for connected controllers without JS errors.
//!
//! Phase 0: no hardware polling — all four gamepad slots return `null`
//! (no gamepad connected). The API surface is complete so that feature-detection
//! code (`navigator.getGamepads` existence checks, `GamepadButton` interface,
//! `gamepadconnected` event listener) works without errors.
//!
//! Installed interfaces:
//! - `navigator.getGamepads()` → sparse array of null (4 slots)
//! - `Gamepad` class — id/index/connected/timestamp/mapping/axes/buttons/vibrationActuator
//! - `GamepadButton` class — pressed/touched/value
//! - `GamepadHapticActuator` stub — type/playEffect/reset
//! - `GamepadEvent` class — gamepad property
//! - `window.Gamepad`, `window.GamepadButton`, `window.GamepadHapticActuator`,
//!   `window.GamepadEvent` exported as globals

/// V8 port of the former rquickjs `install_gamepad_bindings` (Ph3 V8 migration S5-S7):
/// identical JS shim, evaluated via [`lumen_core::ext::JsRuntime::eval`].
///
/// Adds `navigator.getGamepads()` and all W3C Gamepad §4 interfaces.
/// Phase 0: returns 4 null slots (no hardware polling). The event infrastructure
/// (`gamepadconnected`/`gamepaddisconnected`) is present but never fires
/// until a future shell integration polls actual hardware.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_gamepad_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(GAMEPAD_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the Gamepad API (W3C Gamepad Level 2 §4).
#[cfg(feature = "v8-backend")]
const GAMEPAD_SHIM: &str = r#"(function() {
  'use strict';
  if (typeof navigator === 'undefined') return;

  // ── GamepadButton ─────────────────────────────────────────────────────────
  // W3C Gamepad §4.3: represents a single button on a gamepad.
  function GamepadButton(pressed, touched, value) {
    this.pressed = pressed === true;
    this.touched = touched === true;
    this.value   = typeof value === 'number' ? value : 0;
  }
  GamepadButton.prototype.toString = function() {
    return '[object GamepadButton]';
  };

  // ── GamepadHapticActuator ─────────────────────────────────────────────────
  // W3C Gamepad §4.4: vibration/haptic feedback stub.
  function GamepadHapticActuator(type) {
    this.type = type || 'vibration';
  }
  GamepadHapticActuator.prototype.playEffect = function(_type, _params) {
    // Phase 0: no haptic hardware — resolve immediately with "complete".
    return Promise.resolve('complete');
  };
  GamepadHapticActuator.prototype.reset = function() {
    return Promise.resolve('complete');
  };

  // ── Gamepad ───────────────────────────────────────────────────────────────
  // W3C Gamepad §4.1: represents a gamepad / joystick device.
  function Gamepad(id, index, connected, mapping) {
    this.id        = id        || '';
    this.index     = typeof index === 'number' ? index : 0;
    this.connected = connected === true;
    this.timestamp = 0;
    this.mapping   = mapping || 'standard';
    // Standard mapping: 4 axes, 17 buttons (W3C Gamepad §4.5).
    this.axes    = [0, 0, 0, 0];
    this.buttons = [];
    for (var i = 0; i < 17; i++) {
      this.buttons.push(new GamepadButton(false, false, 0));
    }
    this.vibrationActuator = new GamepadHapticActuator('vibration');
    // Legacy plural (some sites use .hapticActuators)
    this.hapticActuators   = [this.vibrationActuator];
  }
  Gamepad.prototype.toString = function() {
    return '[object Gamepad]';
  };

  // ── GamepadEvent ──────────────────────────────────────────────────────────
  // W3C Gamepad §4.6: fired when a gamepad is connected or disconnected.
  function GamepadEvent(type, init) {
    var base = new Event(type, init);
    // Copy Event properties
    Object.defineProperty(this, '_base', { value: base, enumerable: false });
    this.type       = base.type;
    this.bubbles    = base.bubbles;
    this.cancelable = base.cancelable;
    this.gamepad    = (init && init.gamepad) ? init.gamepad : null;
  }
  GamepadEvent.prototype = Object.create(Event.prototype);
  GamepadEvent.prototype.constructor = GamepadEvent;

  // ── Internal gamepad list ─────────────────────────────────────────────────
  // Phase 0: all 4 slots are null (no device connected).
  var _gamepads = [null, null, null, null];

  // ── navigator.getGamepads ─────────────────────────────────────────────────
  // W3C Gamepad §5.1: returns a snapshot of the current gamepad state.
  // Returns a sparse array (Array-like object with numeric indices + length).
  // Phase 0: all entries null.
  navigator.getGamepads = function() {
    var out = [];
    for (var i = 0; i < _gamepads.length; i++) {
      out[i] = _gamepads[i];
    }
    return out;
  };

  // ── Internal helper: connect / disconnect a gamepad slot ─────────────────
  // Called by future shell integration (P3) to deliver real hardware events.
  // _lumen_gamepad_connect(index, id, mapping) → fires 'gamepadconnected'.
  // _lumen_gamepad_disconnect(index)           → fires 'gamepaddisconnected'.
  globalThis._lumen_gamepad_connect = function(index, id, mapping) {
    var gp = new Gamepad(id || '', index, true, mapping || 'standard');
    gp.timestamp = typeof performance !== 'undefined' ? performance.now() : 0;
    _gamepads[index] = gp;
    var evt = new GamepadEvent('gamepadconnected', { gamepad: gp, bubbles: false, cancelable: false });
    window.dispatchEvent(evt);
  };

  globalThis._lumen_gamepad_disconnect = function(index) {
    var gp = _gamepads[index];
    _gamepads[index] = null;
    if (gp) {
      var evt = new GamepadEvent('gamepaddisconnected', { gamepad: gp, bubbles: false, cancelable: false });
      window.dispatchEvent(evt);
    }
  };

  // ── Global exports ────────────────────────────────────────────────────────
  try { window.Gamepad              = Gamepad;              } catch(_) {}
  try { window.GamepadButton        = GamepadButton;        } catch(_) {}
  try { window.GamepadHapticActuator = GamepadHapticActuator; } catch(_) {}
  try { window.GamepadEvent         = GamepadEvent;         } catch(_) {}
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_gamepad_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        // Minimal stubs so the shim doesn't fail.
        rt.eval(
            r#"
            var window = globalThis;
            var navigator = { getGamepads: function(){ return [null,null,null,null]; } };
            globalThis.navigator = navigator;
            function Event(t,i){ this.type=t; this.bubbles=(i&&i.bubbles)||false; this.cancelable=(i&&i.cancelable)||false; }
            Event.prototype.constructor = Event;
            window.dispatchEvent = function(){};
            "#,
        )
        .unwrap();
        super::install_gamepad_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn gamepad_api_installed() {
        with_gamepad_api(|rt| {
            let ok = rt
                .eval("typeof navigator.getGamepads === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn get_gamepads_returns_four_slots() {
        with_gamepad_api(|rt| {
            let len = rt.eval("navigator.getGamepads().length").unwrap();
            assert_eq!(len, JsValue::Number(4.0));
        });
    }

    #[test]
    fn get_gamepads_all_null_initially() {
        with_gamepad_api(|rt| {
            let all_null = rt
                .eval("navigator.getGamepads().every(function(g){ return g === null; })")
                .unwrap();
            assert_eq!(all_null, JsValue::Bool(true));
        });
    }

    #[test]
    fn gamepad_class_exists() {
        with_gamepad_api(|rt| {
            let ok = rt.eval("typeof window.Gamepad === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn gamepad_button_class_exists() {
        with_gamepad_api(|rt| {
            let ok = rt
                .eval("typeof window.GamepadButton === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn gamepad_haptic_actuator_class_exists() {
        with_gamepad_api(|rt| {
            let ok = rt
                .eval("typeof window.GamepadHapticActuator === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn gamepad_event_class_exists() {
        with_gamepad_api(|rt| {
            let ok = rt
                .eval("typeof window.GamepadEvent === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn gamepad_button_shape() {
        with_gamepad_api(|rt| {
            let pressed = rt
                .eval("new window.GamepadButton(false, false, 0).pressed")
                .unwrap();
            assert_eq!(pressed, JsValue::Bool(false));
            let value = rt
                .eval("new window.GamepadButton(true, false, 0.75).value")
                .unwrap();
            assert_eq!(value, JsValue::Number(0.75));
        });
    }

    #[test]
    fn gamepad_haptic_actuator_play_effect_returns_promise() {
        with_gamepad_api(|rt| {
            let ok = rt
                .eval("new window.GamepadHapticActuator('vibration').playEffect('dual-rumble', {}) instanceof Promise")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn gamepad_connect_helper_exists() {
        with_gamepad_api(|rt| {
            let ok = rt
                .eval("typeof globalThis._lumen_gamepad_connect === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn gamepad_disconnect_helper_exists() {
        with_gamepad_api(|rt| {
            let ok = rt
                .eval("typeof globalThis._lumen_gamepad_disconnect === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn gamepad_connect_fills_slot() {
        with_gamepad_api(|rt| {
            rt.eval(
                "globalThis._lumen_gamepad_connect(0, 'Xbox Controller (STANDARD GAMEPAD)', 'standard');",
            )
            .unwrap();
            let connected = rt
                .eval("navigator.getGamepads()[0] !== null && navigator.getGamepads()[0].connected === true")
                .unwrap();
            assert_eq!(connected, JsValue::Bool(true));
        });
    }

    #[test]
    fn gamepad_disconnect_clears_slot() {
        with_gamepad_api(|rt| {
            rt.eval("globalThis._lumen_gamepad_connect(1, 'TestPad', 'standard');")
                .unwrap();
            rt.eval("globalThis._lumen_gamepad_disconnect(1);").unwrap();
            let null_slot = rt.eval("navigator.getGamepads()[1] === null").unwrap();
            assert_eq!(null_slot, JsValue::Bool(true));
        });
    }

    #[test]
    fn gamepad_has_17_buttons() {
        with_gamepad_api(|rt| {
            rt.eval("globalThis._lumen_gamepad_connect(0, 'TestPad', 'standard');")
                .unwrap();
            let count = rt.eval("navigator.getGamepads()[0].buttons.length").unwrap();
            assert_eq!(count, JsValue::Number(17.0));
        });
    }

    #[test]
    fn gamepad_has_four_axes() {
        with_gamepad_api(|rt| {
            rt.eval("globalThis._lumen_gamepad_connect(0, 'TestPad', 'standard');")
                .unwrap();
            let count = rt.eval("navigator.getGamepads()[0].axes.length").unwrap();
            assert_eq!(count, JsValue::Number(4.0));
        });
    }

    #[test]
    fn gamepad_vibration_actuator_present() {
        with_gamepad_api(|rt| {
            rt.eval("globalThis._lumen_gamepad_connect(0, 'TestPad', 'standard');")
                .unwrap();
            let has_actuator = rt
                .eval("navigator.getGamepads()[0].vibrationActuator !== null")
                .unwrap();
            assert_eq!(has_actuator, JsValue::Bool(true));
        });
    }
}
