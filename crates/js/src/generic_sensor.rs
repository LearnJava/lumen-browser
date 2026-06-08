//! W3C Generic Sensor API — Phase 0 stub
//!
//! Implements the base `Sensor` class and all concrete sensor types:
//! Accelerometer, Gyroscope, LinearAccelerationSensor, GravitySensor,
//! AbsoluteOrientationSensor, RelativeOrientationSensor, Magnetometer,
//! AmbientLightSensor.
//!
//! Phase 0: `start()` activates the sensor but never fires `onreading` — no
//! real hardware access. All reading values are `null` until hardware is
//! connected in a future phase. Native binding `_lumen_sensor_read(type)`
//! is prepared for Phase 1 OS integration.

use rquickjs::Ctx;

/// Install Generic Sensor API bindings into the JS context.
pub fn install_generic_sensor_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(GENERIC_SENSOR_SHIM)?;
    Ok(())
}

const GENERIC_SENSOR_SHIM: &str = r#"
(function() {
  // ── Minimal EventTarget mixin ──────────────────────────────────────────────
  //
  // Used as base for Sensor classes. Avoids depending on a global EventTarget
  // constructor (QuickJS doesn't expose one in all environments).
  function _SensorEventTarget() {
    this._listeners = {};
  }
  _SensorEventTarget.prototype.addEventListener = function(type, listener) {
    if (!this._listeners[type]) this._listeners[type] = [];
    if (!this._listeners[type].includes(listener)) {
      this._listeners[type].push(listener);
    }
  };
  _SensorEventTarget.prototype.removeEventListener = function(type, listener) {
    if (!this._listeners[type]) return;
    this._listeners[type] = this._listeners[type].filter(function(l) { return l !== listener; });
  };
  _SensorEventTarget.prototype.dispatchEvent = function(evt) {
    const listeners = this._listeners[evt.type] || [];
    listeners.forEach(function(l) { l(evt); });
    return true;
  };

  // ── SensorErrorEvent (W3C Generic Sensor §11) ────────────────────────────
  function SensorErrorEvent(type, init) {
    this.type = type;
    this.error = (init && init.error) ? init.error : null;
  }

  // ── Sensor base class (W3C Generic Sensor §8) ─────────────────────────────
  //
  // Phase 0: start() activates sensor; readings are null (no hardware).
  // Native binding _lumen_sensor_deliver_reading(type, payload) reserved for Phase 1.
  function Sensor(options) {
    _SensorEventTarget.call(this);
    this._frequency = (options && options.frequency) ? options.frequency : null;
    this._referenceFrame = (options && options.referenceFrame) ? options.referenceFrame : 'device';
    this._activated = false;
    this._hasReading = false;
    this._timestamp = null;
    this._timerId = null;
    this.onreading = null;
    this.onerror = null;
    this.onactivate = null;
  }
  Sensor.prototype = Object.create(_SensorEventTarget.prototype);
  Sensor.prototype.constructor = Sensor;

  Object.defineProperty(Sensor.prototype, 'activated', {
    get: function() { return this._activated; }
  });
  Object.defineProperty(Sensor.prototype, 'hasReading', {
    get: function() { return this._hasReading; }
  });
  Object.defineProperty(Sensor.prototype, 'timestamp', {
    get: function() { return this._timestamp; }
  });

  /** Start sensor polling. Phase 0: activates but never fires onreading. */
  Sensor.prototype.start = function() {
    if (this._activated) return;
    this._activated = true;
    var self = this;
    // Fire 'activate' event asynchronously per spec §8.10.
    Promise.resolve().then(function() {
      var evt = { type: 'activate' };
      if (typeof self.onactivate === 'function') self.onactivate(evt);
      self.dispatchEvent(evt);
    });
  };

  /** Stop sensor polling. */
  Sensor.prototype.stop = function() {
    if (!this._activated) return;
    this._activated = false;
    if (this._timerId !== null) {
      clearInterval(this._timerId);
      this._timerId = null;
    }
  };

  // ── Helper: make XYZ motion sensor constructor ───────────────────────────
  function _makeXyzSensor(name) {
    var ctor = function(options) { Sensor.call(this, options); };
    ctor.prototype = Object.create(Sensor.prototype);
    ctor.prototype.constructor = ctor;
    ctor.prototype._x = 0;
    ctor.prototype._y = 0;
    ctor.prototype._z = 0;
    Object.defineProperty(ctor.prototype, 'x', { get: function() { return this._hasReading ? this._x : null; } });
    Object.defineProperty(ctor.prototype, 'y', { get: function() { return this._hasReading ? this._y : null; } });
    Object.defineProperty(ctor.prototype, 'z', { get: function() { return this._hasReading ? this._z : null; } });
    Object.defineProperty(ctor, 'name', { value: name });
    return ctor;
  }

  // ── Accelerometer (W3C Accelerometer §5) ──────────────────────────────────
  // Measures acceleration of device including gravity, in m/s².
  var Accelerometer = _makeXyzSensor('Accelerometer');

  // ── LinearAccelerationSensor (W3C Accelerometer §7) ───────────────────────
  // Measures acceleration excluding gravity component.
  var LinearAccelerationSensor = _makeXyzSensor('LinearAccelerationSensor');

  // ── GravitySensor (W3C Accelerometer §8) ──────────────────────────────────
  // Measures gravity component of device acceleration.
  var GravitySensor = _makeXyzSensor('GravitySensor');

  // ── Gyroscope (W3C Gyroscope §5) ──────────────────────────────────────────
  // Measures angular velocity around each axis in rad/s.
  var Gyroscope = _makeXyzSensor('Gyroscope');

  // ── Magnetometer (W3C Magnetometer §5) ────────────────────────────────────
  // Measures the magnetic field intensity in microteslas.
  var Magnetometer = _makeXyzSensor('Magnetometer');

  // ── AmbientLightSensor (W3C Ambient Light Sensor §4) ──────────────────────
  // Measures ambient illuminance in lux.
  function AmbientLightSensor(options) { Sensor.call(this, options); }
  AmbientLightSensor.prototype = Object.create(Sensor.prototype);
  AmbientLightSensor.prototype.constructor = AmbientLightSensor;
  AmbientLightSensor.prototype._illuminance = 0;
  Object.defineProperty(AmbientLightSensor.prototype, 'illuminance', {
    get: function() { return this._hasReading ? this._illuminance : null; }
  });

  // ── OrientationSensor base (W3C Orientation Sensor §6) ────────────────────
  function OrientationSensor(options) { Sensor.call(this, options); }
  OrientationSensor.prototype = Object.create(Sensor.prototype);
  OrientationSensor.prototype.constructor = OrientationSensor;
  OrientationSensor.prototype._quaternion = null;
  Object.defineProperty(OrientationSensor.prototype, 'quaternion', {
    get: function() { return this._hasReading ? this._quaternion : null; }
  });

  /** Populate a rotation matrix from the current quaternion reading. */
  OrientationSensor.prototype.populateMatrix = function(targetMatrix) {
    var q = this._quaternion;
    if (!q) return;
    var x = q[0], y = q[1], z = q[2], w = q[3];
    var x2 = x + x, y2 = y + y, z2 = z + z;
    var xx = x * x2, xy = x * y2, xz = x * z2;
    var yy = y * y2, yz = y * z2, zz = z * z2;
    var wx = w * x2, wy = w * y2, wz = w * z2;
    var m = [
      1 - (yy + zz), xy + wz,       xz - wy,       0,
      xy - wz,       1 - (xx + zz), yz + wx,       0,
      xz + wy,       yz - wx,       1 - (xx + yy), 0,
      0,             0,             0,             1,
    ];
    if (typeof Float32Array !== 'undefined' && targetMatrix instanceof Float32Array) {
      for (var i = 0; i < 16; i++) targetMatrix[i] = m[i];
    } else if (typeof Float64Array !== 'undefined' && targetMatrix instanceof Float64Array) {
      for (var j = 0; j < 16; j++) targetMatrix[j] = m[j];
    } else if (targetMatrix && typeof targetMatrix === 'object') {
      var fields = [
        'm11','m21','m31','m41','m12','m22','m32','m42',
        'm13','m23','m33','m43','m14','m24','m34','m44',
      ];
      for (var k = 0; k < 16; k++) targetMatrix[fields[k]] = m[k];
    }
  };

  // ── AbsoluteOrientationSensor (W3C Orientation Sensor §8) ─────────────────
  // Orientation relative to Earth's reference frame.
  function AbsoluteOrientationSensor(options) { OrientationSensor.call(this, options); }
  AbsoluteOrientationSensor.prototype = Object.create(OrientationSensor.prototype);
  AbsoluteOrientationSensor.prototype.constructor = AbsoluteOrientationSensor;

  // ── RelativeOrientationSensor (W3C Orientation Sensor §9) ─────────────────
  // Orientation relative to device's initial position.
  function RelativeOrientationSensor(options) { OrientationSensor.call(this, options); }
  RelativeOrientationSensor.prototype = Object.create(OrientationSensor.prototype);
  RelativeOrientationSensor.prototype.constructor = RelativeOrientationSensor;

  // ── Phase 1 native stub ────────────────────────────────────────────────────
  //
  // Shell calls this to deliver a sensor reading from OS APIs (CoreMotion,
  // Android SensorManager, Windows SensorAPI, etc.).
  // type: 'accelerometer' | 'gyroscope' | 'magnetometer' | 'ambient-light' |
  //       'absolute-orientation' | 'relative-orientation'
  // payload: object with {x,y,z} or {quaternion:[x,y,z,w]} or {illuminance}
  globalThis._lumen_sensor_deliver_reading = function(type, payload) {
    // Reserved for Phase 1 shell integration.
    // Future: iterate active sensor instances matching `type`, apply payload,
    // set _hasReading=true, _timestamp=performance.now(), fire 'reading' event.
    void type; void payload;
  };

  // ── Export to global scope ─────────────────────────────────────────────────
  var _exports = {
    SensorErrorEvent: SensorErrorEvent,
    Sensor: Sensor,
    Accelerometer: Accelerometer,
    LinearAccelerationSensor: LinearAccelerationSensor,
    GravitySensor: GravitySensor,
    Gyroscope: Gyroscope,
    Magnetometer: Magnetometer,
    AmbientLightSensor: AmbientLightSensor,
    OrientationSensor: OrientationSensor,
    AbsoluteOrientationSensor: AbsoluteOrientationSensor,
    RelativeOrientationSensor: RelativeOrientationSensor,
  };
  Object.assign(globalThis, _exports);
  if (typeof window !== 'undefined') Object.assign(window, _exports);
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

    fn check(rt: &crate::QuickJsRuntime, expr: &str) {
        match rt.eval(expr) {
            Ok(lumen_core::JsValue::Bool(true)) => (),
            other => panic!("assertion failed for `{expr}`: {other:?}"),
        }
    }

    #[test]
    fn accelerometer_class_exists() {
        let rt = make_rt();
        check(&rt, "typeof Accelerometer === 'function'");
    }

    #[test]
    fn gyroscope_class_exists() {
        let rt = make_rt();
        check(&rt, "typeof Gyroscope === 'function'");
    }

    #[test]
    fn sensor_start_sets_activated() {
        let rt = make_rt();
        check(
            &rt,
            r#"const s = new Accelerometer(); s.start(); s.activated === true"#,
        );
    }

    #[test]
    fn sensor_stop_clears_activated() {
        let rt = make_rt();
        check(
            &rt,
            r#"const s = new Gyroscope(); s.start(); s.stop(); s.activated === false"#,
        );
    }

    #[test]
    fn accelerometer_readings_null_before_start() {
        let rt = make_rt();
        check(
            &rt,
            r#"const s = new Accelerometer(); s.x === null && s.y === null && s.z === null"#,
        );
    }

    #[test]
    fn linear_acceleration_sensor_exists() {
        let rt = make_rt();
        check(&rt, "typeof LinearAccelerationSensor === 'function'");
    }

    #[test]
    fn gravity_sensor_exists() {
        let rt = make_rt();
        check(&rt, "typeof GravitySensor === 'function'");
    }

    #[test]
    fn magnetometer_exists() {
        let rt = make_rt();
        check(&rt, "typeof Magnetometer === 'function'");
    }

    #[test]
    fn ambient_light_sensor_exists() {
        let rt = make_rt();
        check(&rt, "typeof AmbientLightSensor === 'function'");
    }

    #[test]
    fn absolute_orientation_sensor_exists() {
        let rt = make_rt();
        check(&rt, "typeof AbsoluteOrientationSensor === 'function'");
    }

    #[test]
    fn relative_orientation_sensor_exists() {
        let rt = make_rt();
        check(&rt, "typeof RelativeOrientationSensor === 'function'");
    }

    #[test]
    fn orientation_sensor_quaternion_null_before_reading() {
        let rt = make_rt();
        check(
            &rt,
            r#"const s = new AbsoluteOrientationSensor(); s.quaternion === null"#,
        );
    }

    #[test]
    fn sensor_error_event_class_exists() {
        let rt = make_rt();
        check(&rt, "typeof SensorErrorEvent === 'function'");
    }

    #[test]
    fn sensor_has_reading_false_initially() {
        let rt = make_rt();
        check(
            &rt,
            r#"const s = new Accelerometer({frequency: 60}); s.hasReading === false && s.timestamp === null"#,
        );
    }

    #[test]
    fn populate_matrix_with_float32_array() {
        let rt = make_rt();
        check(
            &rt,
            r#"
            const s = new AbsoluteOrientationSensor();
            s._hasReading = true;
            s._quaternion = [0, 0, 0, 1];
            const m = new Float32Array(16);
            s.populateMatrix(m);
            m[0] === 1 && m[5] === 1 && m[10] === 1 && m[15] === 1
            "#,
        );
    }

    #[test]
    fn lumen_sensor_deliver_reading_is_function() {
        let rt = make_rt();
        check(
            &rt,
            "typeof globalThis._lumen_sensor_deliver_reading === 'function'",
        );
    }
}
