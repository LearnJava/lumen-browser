//! Device Orientation Event and Device Motion Event APIs (W3C Device Orientation L2 & L3)
//!
//! Phase 0 stub: DeviceOrientationEvent and DeviceMotionEvent with default values.
//! requestPermission() always resolves to 'granted'.

use rquickjs::Ctx;

pub fn install_device_sensors_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(DEVICE_SENSORS_SHIM)?;
    Ok(())
}

/// JavaScript shim: Device Orientation & Motion APIs (Phase 0 - default values)
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

    // Store listeners
    const deviceOrientationListeners = new Set();
    const deviceMotionListeners = new Set();

    // Fire a single deviceorientation event with default values on first listener add
    let firedOrientationEvent = false;
    let firedMotionEvent = false;

    window.addEventListener = function(type, listener, options) {
      if (type === 'deviceorientation' && !firedOrientationEvent) {
        firedOrientationEvent = true;
        // Fire event with default values {0, 0, 0, false} after listener registration
        setTimeout(() => {
          if (deviceOrientationListeners.has(listener)) {
            const evt = new DeviceOrientationEvent('deviceorientation', {
              alpha: 0,
              beta: 0,
              gamma: 0,
              absolute: false
            });
            listener(evt);
          }
        }, 0);
        deviceOrientationListeners.add(listener);
      } else if (type === 'devicemotion' && !firedMotionEvent) {
        firedMotionEvent = true;
        // Fire event with default values after listener registration
        setTimeout(() => {
          if (deviceMotionListeners.has(listener)) {
            const evt = new DeviceMotionEvent('devicemotion', {
              acceleration: { x: 0, y: 0, z: 0 },
              accelerationIncludingGravity: { x: 0, y: 0, z: 0 },
              rotationRate: { alpha: 0, beta: 0, gamma: 0 },
              interval: 0
            });
            listener(evt);
          }
        }, 0);
        deviceMotionListeners.add(listener);
      }
      return originalAddEventListener.call(this, type, listener, options);
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

#[cfg(test)]
mod tests {
    use lumen_core::JsRuntime as _;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex};

    fn make_rt() -> crate::QuickJsRuntime {
        let rt = crate::QuickJsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "about:blank", None, None, None, None, None, None)
            .unwrap();
        rt
    }

    #[test]
    fn device_orientation_event_class_exists() {
        let rt = make_rt();
        match rt.eval("typeof DeviceOrientationEvent === 'function'") {
            Ok(lumen_core::JsValue::Bool(true)) => (),
            other => panic!("DeviceOrientationEvent class check failed: {other:?}"),
        }
    }

    #[test]
    fn device_motion_event_class_exists() {
        let rt = make_rt();
        match rt.eval("typeof DeviceMotionEvent === 'function'") {
            Ok(lumen_core::JsValue::Bool(true)) => (),
            other => panic!("DeviceMotionEvent class check failed: {other:?}"),
        }
    }

    #[test]
    fn device_orientation_has_default_values() {
        let rt = make_rt();
        match rt.eval(
            r#"const evt = new DeviceOrientationEvent('deviceorientation', {});
               evt.alpha === 0 && evt.beta === 0 && evt.gamma === 0 && evt.absolute === false"#,
        ) {
            Ok(lumen_core::JsValue::Bool(true)) => (),
            other => panic!("DeviceOrientationEvent defaults check failed: {other:?}"),
        }
    }

    #[test]
    fn device_motion_has_default_values() {
        let rt = make_rt();
        match rt.eval(
            r#"const evt = new DeviceMotionEvent('devicemotion', {});
               evt.acceleration && evt.accelerationIncludingGravity && evt.rotationRate &&
               evt.acceleration.x === 0 && evt.interval === 0"#,
        ) {
            Ok(lumen_core::JsValue::Bool(true)) => (),
            other => panic!("DeviceMotionEvent defaults check failed: {other:?}"),
        }
    }

    #[test]
    fn device_orientation_has_request_permission() {
        let rt = make_rt();
        match rt.eval("typeof DeviceOrientationEvent.requestPermission === 'function'") {
            Ok(lumen_core::JsValue::Bool(true)) => (),
            other => panic!("requestPermission method check failed: {other:?}"),
        }
    }

    #[test]
    fn device_motion_has_request_permission() {
        let rt = make_rt();
        match rt.eval("typeof DeviceMotionEvent.requestPermission === 'function'") {
            Ok(lumen_core::JsValue::Bool(true)) => (),
            other => panic!("requestPermission method check failed: {other:?}"),
        }
    }
}
