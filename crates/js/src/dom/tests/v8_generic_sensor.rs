//! BUG-394 — `Sensor` and its subclasses against the real install.
//!
//! `generic_sensor.rs`'s own tests stub `Event`/`EventTarget` (plain V8 has
//! neither), so they cannot show the one thing the bug is about: that the
//! base really is `WEB_API_SHIM`'s `EventTarget`, with its listener
//! registry, its options and its `on<type>` step — not a look-alike.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    rt.eval(script).unwrap() == lumen_core::JsValue::Bool(true)
}

/// The prototype chain and the listener implementation both have to be
/// the engine's own, not a private mixin's.
#[test]
fn sensors_are_real_event_targets() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "Sensor.prototype instanceof EventTarget"));
    assert!(bool_eval(&rt, "(new Accelerometer()) instanceof EventTarget"));
    assert!(bool_eval(
        &rt,
        "(new AmbientLightSensor()) instanceof EventTarget"
    ));
    assert!(bool_eval(
        &rt,
        "(new Magnetometer()).addEventListener === EventTarget.prototype.addEventListener"
    ));
}

/// A `Sensor` has no constructor operation in the IDL; the concrete
/// subclasses do.
#[test]
fn direct_sensor_construction_throws_type_error() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var threw = false; \
                 try { new Sensor(); } catch (e) { threw = e instanceof TypeError; } \
                 threw"
    ));
    assert!(bool_eval(
        &rt,
        "var threw = false; \
                 try { new OrientationSensor(); } catch (e) { threw = e instanceof TypeError; } \
                 threw"
    ));
    assert!(bool_eval(
        &rt,
        "(new Gyroscope()) instanceof Gyroscope && (new Gyroscope()) instanceof Sensor"
    ));
}

/// Options the private mixin never supported, now free from the shared
/// base: `once` drops the listener, `removeEventListener` honours the
/// capture flag it was registered with.
#[test]
fn sensor_listeners_support_event_target_options() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var s = new Accelerometer(); var hits = 0; \
                 s.addEventListener('reading', function() { hits++; }, {once: true}); \
                 s.dispatchEvent(new Event('reading')); \
                 s.dispatchEvent(new Event('reading')); \
                 hits === 1"
    ));
    assert!(bool_eval(
        &rt,
        "var s = new Accelerometer(); var hits = 0; \
                 var cb = function() { hits++; }; \
                 s.addEventListener('reading', cb, true); \
                 s.removeEventListener('reading', cb); \
                 s.dispatchEvent(new Event('reading')); \
                 hits === 1"
    ));
}

/// `start()` fires `activate` asynchronously, once per channel, with a
/// real `Event` — `dispatchEvent` itself performs the `on<type>` call.
#[test]
fn start_fires_activate_once_per_channel() {
    let rt = v8_runtime_with_dom(make_doc());
    // The event is queued on a microtask (spec §8.10), which runs when
    // this `eval` returns — hence the separate assertion call.
    rt.eval(
        "var s = new Gyroscope(); \
                 var viaHandler = 0, viaListener = 0, evt = null; \
                 s.onactivate = function(e) { viaHandler++; evt = e; }; \
                 s.addEventListener('activate', function() { viaListener++; }); \
                 s.start();"
    )
    .unwrap();
    assert!(bool_eval(
        &rt,
        "viaHandler === 1 && viaListener === 1 \
                 && (evt instanceof Event) && evt.type === 'activate' && evt.target === s"
    ));
}
