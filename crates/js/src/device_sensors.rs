//! Device Orientation Event and Device Motion Event APIs (W3C Device Orientation L2 & L3)
//!
//! Phase 0 stub: DeviceOrientationEvent and DeviceMotionEvent with default values.
//! Registering a listener schedules one all-zero reading, dispatched on `window`.
//! requestPermission() always resolves to 'granted'.

/// V8 port of the former rquickjs `install_device_sensors_bindings` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-12): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_device_sensors_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(DEVICE_SENSORS_SHIM)?;
    Ok(())
}

/// JavaScript shim: Device Orientation & Motion APIs (Phase 0 - default values)
#[cfg(feature = "v8-backend")]
const DEVICE_SENSORS_SHIM: &str = r#"
(function() {
  // DeviceOrientationEvent class
  class DeviceOrientationEvent extends Event {
    constructor(type, init) {
      super(type, init);
      this.alpha = init?.alpha ?? 0;
      this.beta = init?.beta ?? 0;
      this.gamma = init?.gamma ?? 0;
      this.absolute = init?.absolute ?? false;
    }

    static async requestPermission() {
      // Phase 0: Always grant permission
      return 'granted';
    }
  }

  // DeviceMotionEvent class
  class DeviceMotionEvent extends Event {
    constructor(type, init) {
      super(type, init);
      const defaultAccel = { x: 0, y: 0, z: 0 };
      const defaultRotRate = { alpha: 0, beta: 0, gamma: 0 };
      this.acceleration = init?.acceleration ?? defaultAccel;
      this.accelerationIncludingGravity = init?.accelerationIncludingGravity ?? defaultAccel;
      this.rotationRate = init?.rotationRate ?? defaultRotRate;
      this.interval = init?.interval ?? 0;
    }

    static async requestPermission() {
      // Phase 0: Always grant permission
      return 'granted';
    }
  }

  // EventTarget mixin for device orientation events
  if (typeof window !== 'undefined') {
    const originalAddEventListener = window.addEventListener;
    const originalRemoveEventListener = window.removeEventListener;

    // Listeners already registered per type. A reading is scheduled only
    // for a listener not seen before, so a handler that re-adds an already
    // registered listener (a no-op in the DOM) cannot reschedule readings
    // forever. `once` listeners are not remembered — the DOM drops them after
    // the first delivery, so re-adding one is a genuinely new registration.
    const deviceOrientationListeners = new Set();
    const deviceMotionListeners = new Set();

    function makeReading(type) {
      if (type === 'deviceorientation') {
        return new DeviceOrientationEvent('deviceorientation', {
          alpha: 0, beta: 0, gamma: 0, absolute: false
        });
      }
      return new DeviceMotionEvent('devicemotion', {
        acceleration: { x: 0, y: 0, z: 0 },
        accelerationIncludingGravity: { x: 0, y: 0, z: 0 },
        rotationRate: { alpha: 0, beta: 0, gamma: 0 },
        interval: 0
      });
    }

    // One pending reading per type (BUG-643: a global "fired once" flag used
    // to deliver it to the very first listener ever registered and to no one
    // else). Every registration made before the reading is delivered shares
    // it; a registration made later — including from inside a handler —
    // schedules the next one. Delivery goes through `dispatchEvent`, so the
    // reading gets exactly the semantics of any other window event (whose
    // own gaps — `handleEvent`, `once`, `event.target` — are BUG-1172).
    const pendingReading = { deviceorientation: false, devicemotion: false };
    function scheduleReading(target, type) {
      if (pendingReading[type]) return;
      pendingReading[type] = true;
      setTimeout(() => {
        pendingReading[type] = false;
        target.dispatchEvent(makeReading(type));
      }, 0);
    }

    window.addEventListener = function(type, listener, options) {
      const result = originalAddEventListener.call(this, type, listener, options);
      const known = type === 'deviceorientation' ? deviceOrientationListeners
        : type === 'devicemotion' ? deviceMotionListeners : null;
      if (known && listener && !known.has(listener)) {
        const once = typeof options === 'object' && options !== null && options.once;
        const signal = typeof options === 'object' && options !== null ? options.signal : undefined;
        if (!(signal && signal.aborted)) {
          if (!once) {
            known.add(listener);
            if (signal) signal.addEventListener('abort', () => known.delete(listener));
          }
          scheduleReading(this, type);
        }
      }
      return result;
    };

    window.removeEventListener = function(type, listener, options) {
      if (type === 'deviceorientation') {
        deviceOrientationListeners.delete(listener);
      } else if (type === 'devicemotion') {
        deviceMotionListeners.delete(listener);
      }
      return originalRemoveEventListener.call(this, type, listener, options);
    };

    // Export classes to global scope
    window.DeviceOrientationEvent = DeviceOrientationEvent;
    window.DeviceMotionEvent = DeviceMotionEvent;
  }

  if (typeof globalThis !== 'undefined') {
    globalThis.DeviceOrientationEvent = DeviceOrientationEvent;
    globalThis.DeviceMotionEvent = DeviceMotionEvent;
  }
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex};

    fn with_device_sensors(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "about:blank", None, None, None, None, None, None, None, None, None, false)
            .unwrap();
        f(&rt);
    }

    #[test]
    fn device_orientation_event_class_exists() {
        with_device_sensors(|rt| {
            let ok = rt.eval("typeof DeviceOrientationEvent === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn device_motion_event_class_exists() {
        with_device_sensors(|rt| {
            let ok = rt.eval("typeof DeviceMotionEvent === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn device_orientation_has_default_values() {
        with_device_sensors(|rt| {
            let ok = rt
                .eval(
                    r#"const evt = new DeviceOrientationEvent('deviceorientation', {});
                       evt.alpha === 0 && evt.beta === 0 && evt.gamma === 0 && evt.absolute === false"#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn device_motion_has_default_values() {
        with_device_sensors(|rt| {
            let ok = rt
                .eval(
                    r#"const evt = new DeviceMotionEvent('devicemotion', {});
                       evt.acceleration && evt.accelerationIncludingGravity && evt.rotationRate &&
                       evt.acceleration.x === 0 && evt.interval === 0"#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    /// BUG-643: every listener gets the synthetic reading, not only the
    /// first one ever registered — including one added from inside a handler
    /// after the first reading was already delivered.
    #[test]
    fn every_sensor_listener_receives_the_synthetic_reading() {
        with_device_sensors(|rt| {
            for ty in ["deviceorientation", "devicemotion"] {
                rt.eval(&format!(
                    r#"var a = 0, b = 0, c = 0;
                       function late() {{ c++; }}
                       addEventListener('{ty}', function() {{ a++; addEventListener('{ty}', late); }});
                       addEventListener('{ty}', function() {{ b++; }});"#
                ))
                .unwrap();
                for _ in 0..4 {
                    rt.eval("_lumen_tick_timers()").unwrap();
                }
                // Two readings: one shared by the two listeners registered
                // together, one scheduled by the late registration.
                let ok = rt.eval("a === 2 && b === 2 && c === 1").unwrap();
                assert_eq!(ok, JsValue::Bool(true), "{ty}: every listener must fire");
            }
        });
    }

    /// A listener removed before the reading is delivered never sees it, and
    /// re-adding an already registered listener from its own handler does not
    /// keep rescheduling the reading forever.
    #[test]
    fn removed_listener_is_skipped_and_readd_does_not_loop() {
        with_device_sensors(|rt| {
            rt.eval(
                r#"var gone = 0, self = 0;
                   function g() { gone++; }
                   function s() { self++; addEventListener('deviceorientation', s); }
                   addEventListener('deviceorientation', g);
                   removeEventListener('deviceorientation', g);
                   addEventListener('deviceorientation', s);"#,
            )
            .unwrap();
            for _ in 0..6 {
                rt.eval("_lumen_tick_timers()").unwrap();
            }
            let ok = rt.eval("gone === 0 && self === 1").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn device_orientation_has_request_permission() {
        with_device_sensors(|rt| {
            let ok = rt
                .eval("typeof DeviceOrientationEvent.requestPermission === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn device_motion_has_request_permission() {
        with_device_sensors(|rt| {
            let ok = rt
                .eval("typeof DeviceMotionEvent.requestPermission === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
