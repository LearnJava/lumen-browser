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

/// V8 port of the former rquickjs `install_generic_sensor_bindings` (Ph3 V8 migration S5-S7):
/// identical JS shim, evaluated via [`lumen_core::ext::JsRuntime::eval`].
#[cfg(feature = "v8-backend")]
pub(crate) fn install_generic_sensor_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(GENERIC_SENSOR_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const GENERIC_SENSOR_SHIM: &str = r#"
(function() {
  // ── Base classes: the engine's own EventTarget/Event ──────────────────────
  //
  // W3C Generic Sensor §8 is `interface Sensor : EventTarget`, so the base has
  // to be the engine's real `EventTarget` — the one `WEB_API_SHIM` installs
  // before this module (plain V8 has none) — and not a private look-alike, as
  // this shim used to define: `sensor instanceof EventTarget` has to hold, and
  // listeners have to live in the same registry that gives every other event
  // source `capture`/`once` and the `on<type>` handler call (BUG-394).
  //
  // Without those globals there is no `Sensor` interface to install, so bail
  // rather than export a twin whose `addEventListener` silently differs: a
  // missing `Accelerometer` fails feature detection closed, a divergent one
  // does not (the `navigator.permissions` precedent, `permissions.rs`). On a
  // real page the branch is unreachable — `install_dom` evaluates
  // `WEB_API_SHIM` before every `install_v8!` module — it only bites a
  // standalone install such as a bare-V8 unit test.
  var EventTargetBase = globalThis.EventTarget;
  var EventBase = globalThis.Event;
  if (typeof EventTargetBase !== 'function' || typeof EventBase !== 'function') return;

  // ── SensorErrorEvent (W3C Generic Sensor §11) ────────────────────────────
  //
  // `SensorErrorEventInit` declares `error` as a *required* dictionary member,
  // which under WebIDL makes the init argument itself mandatory too: a missing
  // second argument, a non-object one, or an absent/`undefined` `error` must
  // each throw `TypeError` instead of silently defaulting to `null` (BUG-393).
  function SensorErrorEvent(type, init) {
    if (arguments.length < 2) {
      throw new TypeError(
        'Failed to construct SensorErrorEvent: 2 arguments required, but only ' +
        arguments.length + ' present.'
      );
    }
    if (init === null || init === undefined || (typeof init !== 'object' && typeof init !== 'function')) {
      throw new TypeError(
        'Failed to construct SensorErrorEvent: argument 2 is not of type SensorErrorEventInit.'
      );
    }
    var error = init.error;
    if (error === undefined) {
      throw new TypeError(
        'Failed to construct SensorErrorEvent: required member error is undefined.'
      );
    }
    // WebIDL interface-type conversion: anything that is not a DOMException is
    // a TypeError. Guarded on the global existing, since the shim is also
    // installed standalone (unit tests) where the DOMException polyfill may not
    // have run.
    if (typeof globalThis.DOMException === 'function' && !(error instanceof globalThis.DOMException)) {
      throw new TypeError(
        'Failed to construct SensorErrorEvent: member error is not of type DOMException.'
      );
    }
    this.type = String(type);
    this.error = error;
  }

  // ── Sensor base class (W3C Generic Sensor §8) ─────────────────────────────
  //
  // Phase 0: start() activates sensor; readings are null (no hardware).
  // Native binding _lumen_sensor_deliver_reading(type, payload) reserved for Phase 1.
  function Sensor(options) {
    // WebIDL: the `Sensor` interface declares no constructor operation, so it
    // is reachable only through a concrete subclass — a direct `new Sensor()`
    // is an illegal constructor. Subclasses enter this body through
    // `Sensor.call(this, options)`, where `new.target` is undefined (BUG-394).
    if (new.target === Sensor) {
      throw new TypeError('Illegal constructor');
    }
    EventTargetBase.call(this);
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
  Sensor.prototype = Object.create(EventTargetBase.prototype);
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
    // Fire 'activate' event asynchronously per spec §8.10. `dispatchEvent`
    // invokes `onactivate` itself (the `on<type>` step every EventTarget
    // performs), so the explicit call this line used to carry would now
    // deliver the event twice.
    Promise.resolve().then(function() {
      self.dispatchEvent(new EventBase('activate'));
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
  // Abstract like `Sensor`: the IDL gives a constructor only to the two
  // concrete subclasses below, so `new OrientationSensor()` is illegal too
  // (same WebIDL rule as BUG-394's `new Sensor()`, not named in that report).
  function OrientationSensor(options) {
    if (new.target === OrientationSensor) {
      throw new TypeError('Illegal constructor');
    }
    Sensor.call(this, options);
  }
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

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// `Event`/`EventTarget` stubs, copied from `permissions.rs`'s `STUBS`.
    ///
    /// The shim now refuses to install without both globals (`Sensor` is an
    /// `EventTarget` subclass), and plain V8 has neither — on a page they come
    /// from `WEB_API_SHIM`, which these tests cannot evaluate (it needs the
    /// native registrations of a full `install_dom`). The copies match the
    /// contract this module depends on: a per-type listener map plus the
    /// `on<type>` handler call inside `dispatchEvent`. What a stub cannot
    /// prove — that the base really is the engine's own `EventTarget` — is
    /// covered by the `generic_sensor_*` tests in `dom.rs`, which run against
    /// the real install.
    const STUBS: &str = r#"
        function Event(type) { this.type = String(type); this.target = null; }
        globalThis.Event = Event;
        function EventTarget() {
            Object.defineProperty(this, '_listeners', { value: Object.create(null), writable: true });
        }
        EventTarget.prototype.addEventListener = function(type, cb) {
            if (!cb) return;
            type = String(type);
            (this._listeners[type] || (this._listeners[type] = [])).push(cb);
        };
        EventTarget.prototype.removeEventListener = function(type, cb) {
            var list = this._listeners[String(type)];
            if (!list) return;
            var i = list.indexOf(cb);
            if (i >= 0) list.splice(i, 1);
        };
        EventTarget.prototype.dispatchEvent = function(event) {
            var list = (this._listeners[String(event.type)] || []).slice();
            event.target = this;
            for (var i = 0; i < list.length; i++) { list[i].call(this, event); }
            var on = this['on' + event.type];
            if (typeof on === 'function') on.call(this, event);
            return true;
        };
        globalThis.EventTarget = EventTarget;
    "#;

    fn with_generic_sensor(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(STUBS).unwrap();
        super::install_generic_sensor_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    /// Same, plus the engine's own `DOMException` constructor — the WebIDL
    /// `error` member is typed `DOMException`, and a hand-written twin would
    /// prove nothing about the real one (see `DOM_EXCEPTION_POLYFILL` docs).
    fn with_generic_sensor_and_dom_exception(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(STUBS).unwrap();
        rt.eval(crate::v8_runtime::DOM_EXCEPTION_POLYFILL).unwrap();
        super::install_generic_sensor_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    /// Evaluate `expr` and assert it threw a `TypeError`.
    fn check_throws_type_error(rt: &V8JsRuntime, expr: &str) {
        let probe = format!(
            "(function() {{ try {{ {expr}; return 'no throw'; }} \
             catch (e) {{ return (e instanceof TypeError) ? 'TypeError' : ('other: ' + e); }} }})()"
        );
        assert_eq!(
            rt.eval(&probe).unwrap(),
            JsValue::String("TypeError".to_string()),
            "expected TypeError from `{expr}`"
        );
    }

    fn check(rt: &V8JsRuntime, expr: &str) {
        assert_eq!(rt.eval(expr).unwrap(), JsValue::Bool(true), "assertion failed for `{expr}`");
    }

    #[test]
    fn accelerometer_class_exists() {
        with_generic_sensor(|rt| check(rt, "typeof Accelerometer === 'function'"));
    }

    #[test]
    fn gyroscope_class_exists() {
        with_generic_sensor(|rt| check(rt, "typeof Gyroscope === 'function'"));
    }

    #[test]
    fn sensor_start_sets_activated() {
        with_generic_sensor(|rt| {
            check(
                rt,
                "var s = new Accelerometer(); s.start(); s.activated === true",
            );
        });
    }

    #[test]
    fn sensor_stop_clears_activated() {
        with_generic_sensor(|rt| {
            check(
                rt,
                "var s = new Gyroscope(); s.start(); s.stop(); s.activated === false",
            );
        });
    }

    #[test]
    fn accelerometer_readings_null_before_start() {
        with_generic_sensor(|rt| {
            check(
                rt,
                "var s = new Accelerometer(); s.x === null && s.y === null && s.z === null",
            );
        });
    }

    #[test]
    fn linear_acceleration_sensor_exists() {
        with_generic_sensor(|rt| check(rt, "typeof LinearAccelerationSensor === 'function'"));
    }

    #[test]
    fn gravity_sensor_exists() {
        with_generic_sensor(|rt| check(rt, "typeof GravitySensor === 'function'"));
    }

    #[test]
    fn magnetometer_exists() {
        with_generic_sensor(|rt| check(rt, "typeof Magnetometer === 'function'"));
    }

    #[test]
    fn ambient_light_sensor_exists() {
        with_generic_sensor(|rt| check(rt, "typeof AmbientLightSensor === 'function'"));
    }

    #[test]
    fn absolute_orientation_sensor_exists() {
        with_generic_sensor(|rt| check(rt, "typeof AbsoluteOrientationSensor === 'function'"));
    }

    #[test]
    fn relative_orientation_sensor_exists() {
        with_generic_sensor(|rt| check(rt, "typeof RelativeOrientationSensor === 'function'"));
    }

    #[test]
    fn orientation_sensor_quaternion_null_before_reading() {
        with_generic_sensor(|rt| {
            check(
                rt,
                "var s = new AbsoluteOrientationSensor(); s.quaternion === null",
            );
        });
    }

    #[test]
    fn sensor_error_event_class_exists() {
        with_generic_sensor(|rt| check(rt, "typeof SensorErrorEvent === 'function'"));
    }

    #[test]
    fn sensor_error_event_arity_is_two() {
        with_generic_sensor(|rt| check(rt, "SensorErrorEvent.length === 2"));
    }

    #[test]
    fn sensor_error_event_without_init_dict_throws() {
        with_generic_sensor(|rt| check_throws_type_error(rt, "new SensorErrorEvent('error')"));
    }

    #[test]
    fn sensor_error_event_with_non_object_init_throws() {
        with_generic_sensor(|rt| {
            check_throws_type_error(rt, "new SensorErrorEvent('error', 42)");
            check_throws_type_error(rt, "new SensorErrorEvent('error', null)");
            check_throws_type_error(rt, "new SensorErrorEvent('error', undefined)");
        });
    }

    #[test]
    fn sensor_error_event_without_required_error_member_throws() {
        with_generic_sensor(|rt| {
            check_throws_type_error(rt, "new SensorErrorEvent('error', {})");
            check_throws_type_error(rt, "new SensorErrorEvent('error', {error: undefined})");
        });
    }

    #[test]
    fn sensor_error_event_with_non_dom_exception_error_throws() {
        with_generic_sensor_and_dom_exception(|rt| {
            check_throws_type_error(rt, "new SensorErrorEvent('error', {error: {}})");
            check_throws_type_error(rt, "new SensorErrorEvent('error', {error: 'boom'})");
        });
    }

    #[test]
    fn sensor_error_event_with_init_dict_keeps_error() {
        with_generic_sensor_and_dom_exception(|rt| {
            check(
                rt,
                "var err = new DOMException(); \
                 var evt = new SensorErrorEvent('type', {error: err}); \
                 evt.type === 'type' && evt.error === err",
            );
        });
    }

    #[test]
    fn sensor_has_reading_false_initially() {
        with_generic_sensor(|rt| {
            check(
                rt,
                "var s = new Accelerometer({frequency: 60}); s.hasReading === false && s.timestamp === null",
            );
        });
    }

    #[test]
    fn populate_matrix_with_float32_array() {
        with_generic_sensor(|rt| {
            check(
                rt,
                r#"
                var s = new AbsoluteOrientationSensor();
                s._hasReading = true;
                s._quaternion = [0, 0, 0, 1];
                var m = new Float32Array(16);
                s.populateMatrix(m);
                m[0] === 1 && m[5] === 1 && m[10] === 1 && m[15] === 1
                "#,
            );
        });
    }

    /// BUG-394: `interface Sensor : EventTarget` — the base has to be the
    /// global `EventTarget`, not a private mixin, so both the prototype chain
    /// and the listener implementation are the platform's own.
    #[test]
    fn sensor_subclasses_inherit_global_event_target() {
        with_generic_sensor(|rt| {
            check(rt, "Sensor.prototype instanceof EventTarget");
            check(rt, "(new Accelerometer()) instanceof EventTarget");
            check(rt, "(new AbsoluteOrientationSensor()) instanceof EventTarget");
            check(
                rt,
                "(new Gyroscope()).addEventListener === EventTarget.prototype.addEventListener",
            );
            check(
                rt,
                "(new Gyroscope()).dispatchEvent === EventTarget.prototype.dispatchEvent",
            );
        });
    }

    /// BUG-394: `Sensor` has no constructor operation in the IDL, so only its
    /// concrete subclasses are constructible.
    #[test]
    fn direct_sensor_construction_throws() {
        with_generic_sensor(|rt| {
            check_throws_type_error(rt, "new Sensor()");
            check_throws_type_error(rt, "new Sensor({frequency: 60})");
            // The abstract orientation base is the same case (W3C Orientation
            // Sensor §6 declares no constructor either).
            check_throws_type_error(rt, "new OrientationSensor()");
        });
    }

    /// The guard keys on `new.target`, so it must not fire for the subclasses
    /// that reach the same body through `Sensor.call(this, options)`.
    #[test]
    fn subclass_construction_still_works() {
        with_generic_sensor(|rt| {
            check(rt, "(new Accelerometer()) instanceof Sensor");
            check(rt, "(new AmbientLightSensor()) instanceof Sensor");
            check(
                rt,
                "var s = new RelativeOrientationSensor({frequency: 30}); \
                 (s instanceof OrientationSensor) && (s instanceof Sensor)",
            );
        });
    }

    /// `start()` must deliver `activate` exactly once to each of the two
    /// subscription channels: `dispatchEvent` already calls the `on<type>`
    /// handler, so the shim must not call `onactivate` a second time itself.
    #[test]
    fn activate_event_fires_once_per_channel() {
        with_generic_sensor(|rt| {
            // The event is queued on a microtask (spec §8.10), which runs when
            // this `eval` returns — hence the separate assertion call.
            rt.eval(
                "var s = new Accelerometer(); \
                 var viaHandler = 0, viaListener = 0, evt = null; \
                 s.onactivate = function(e) { viaHandler++; evt = e; }; \
                 s.addEventListener('activate', function() { viaListener++; }); \
                 s.start();",
            )
            .unwrap();
            check(
                rt,
                "viaHandler === 1 && viaListener === 1 && evt instanceof Event \
                 && evt.type === 'activate' && evt.target === s",
            );
        });
    }

    #[test]
    fn lumen_sensor_deliver_reading_is_function() {
        with_generic_sensor(|rt| {
            check(rt, "typeof globalThis._lumen_sensor_deliver_reading === 'function'");
        });
    }
}
