//! Battery Status API — deterministic `BatteryManager` (ADR-007 Layer 4, 9D.4).
//!
//! The Battery Status API (`navigator.getBattery()`) is a high-entropy
//! fingerprinting source: battery level, charging state, and charge/discharge
//! time together form a near-unique device signature. Per ADR-007 the engine
//! never reads real power state: the manager answers the desktop no-battery
//! constants (`charging: true`, `level: 1`, `chargingTime: 0`,
//! `dischargingTime: Infinity`). The state is constant, so the spec's
//! `chargingchange`/`levelchange`/`chargingtimechange`/`dischargingtimechange`
//! events never fire — `addEventListener`/`on*` still work, there is just
//! nothing to report. This replaces the former rejected-promise stub
//! (BUG-1791): the Yandex.Player SDK calls `getBattery()` unguarded in its
//! constructor, and a rejection killed page script on ya.ru.
//!
//! Window scope only (same as the stub it replaces); `WorkerNavigator` has no
//! `getBattery` yet. Must be called **after** `v8_runtime.rs::install_dom`'s
//! `WEB_API_SHIM` (requires `navigator` and the shim's `EventTarget`); the
//! finalize pass (`navigator_bindings.rs`) then moves the function onto
//! `Navigator.prototype`.

/// V8 port of the former rquickjs `install_battery_bindings` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-4): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_battery_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(BATTERY_SHIM)?;
    Ok(())
}

/// JavaScript shim: `BatteryManager` + cached `getBattery()` promise.
#[cfg(feature = "v8-backend")]
const BATTERY_SHIM: &str = r#"(function() {
  'use strict';
  if (typeof navigator !== 'object' || navigator === null) return;

  // The shim's own EventTarget (`WEB_API_SHIM`), not a V8 built-in — plain V8
  // has none. Bail rather than install a BatteryManager that swallows
  // addEventListener (the permissions.rs precedent).
  var ET = globalThis.EventTarget;
  if (typeof ET !== 'function') return;

  function def(obj, name, value) {
    Object.defineProperty(obj, name, {
      value: value, writable: true, enumerable: false, configurable: true });
  }
  // WebIDL §3.7.6: a readonly attribute is an enumerable, configurable
  // accessor on the interface prototype object.
  function defAttr(obj, name, get, set) {
    Object.defineProperty(obj, name, {
      get: get, set: set, enumerable: true, configurable: true });
  }
  function defTag(obj, tag) {
    if (typeof Symbol === 'undefined' || !Symbol.toStringTag) return;
    Object.defineProperty(obj, Symbol.toStringTag, {
      value: tag, writable: false, enumerable: false, configurable: true });
  }

  function BatteryManager() { throw new TypeError('Illegal constructor'); }
  BatteryManager.prototype = Object.create(ET.prototype);
  def(BatteryManager.prototype, 'constructor', BatteryManager);
  defTag(BatteryManager.prototype, 'BatteryManager');

  // One manager, one deterministic state (ADR-007): a desktop machine on AC
  // power. `slots()` is the WebIDL brand check — getters reject other receivers.
  var manager;
  var S = { charging: true, chargingTime: 0, dischargingTime: Infinity, level: 1,
            onchargingchange: null, onchargingtimechange: null,
            ondischargingtimechange: null, onlevelchange: null };
  function slots(o) {
    if (o !== manager) throw new TypeError('Illegal invocation');
    return S;
  }

  ['charging', 'chargingTime', 'dischargingTime', 'level'].forEach(function(name) {
    defAttr(BatteryManager.prototype, name, function() { return slots(this)[name]; });
  });
  // The state never moves, so these handlers are decoration — but WebIDL-shaped.
  ['chargingchange', 'chargingtimechange', 'dischargingtimechange', 'levelchange']
    .forEach(function(type) {
      var prop = 'on' + type;
      defAttr(BatteryManager.prototype, prop,
        function() { return slots(this)[prop]; },
        function(fn) { slots(this)[prop] = (typeof fn === 'function') ? fn : null; });
    });

  // Reflect.construct runs EventTarget's body (which installs the listener map)
  // against BatteryManager.prototype without going through BatteryManager
  // itself, which is the throwing no-constructor stub.
  try {
    manager = Reflect.construct(ET, [], BatteryManager);
  } catch (e) {
    manager = Object.create(BatteryManager.prototype);
    try { ET.call(manager); } catch (e2) {}
  }

  // [SameObject] (Battery Status §5.2): every call returns the same promise —
  // WPT battery-promise asserts the identity.
  var promise = Promise.resolve(manager);

  navigator.getBattery = function() { return promise; };

  // Interface object on the global, non-enumerable like every other interface.
  Object.defineProperty(globalThis, 'BatteryManager', {
    value: BatteryManager, writable: true, enumerable: false, configurable: true });
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::JsValue;
    use lumen_core::ext::JsRuntime as _;

    /// The page environment the shim needs: the shim's `EventTarget` contract
    /// (a per-type listener map) and a `navigator` object. On a real page both
    /// arrive with `WEB_API_SHIM` before this module installs.
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
            var list = this._listeners[String(event.type)] || [];
            event.target = this;
            for (var i = 0; i < list.slice().length; i++) { list[i].call(this, event); }
            var on = this['on' + event.type];
            if (typeof on === 'function') on.call(this, event);
            return true;
        };
        globalThis.EventTarget = EventTarget;
        var navigator = {};
    "#;

    fn with_battery(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(STUBS).unwrap();
        install_battery_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    fn bool_eval(rt: &V8JsRuntime, expr: &str) -> bool {
        rt.eval(expr).unwrap() == JsValue::Bool(true)
    }

    #[test]
    fn install_succeeds_without_navigator() {
        // Should not panic even if navigator is undefined.
        let rt = V8JsRuntime::new().unwrap();
        install_battery_bindings_v8(&rt).expect("install should succeed");
    }

    #[test]
    fn get_battery_is_function() {
        with_battery(|rt| {
            let ok = rt.eval("typeof navigator.getBattery === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn get_battery_returns_thenable() {
        with_battery(|rt| {
            let ok = rt
                .eval(
                    "(function() { \
                       var p = navigator.getBattery(); \
                       return typeof p === 'object' && typeof p.then === 'function'; \
                     })()",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn get_battery_has_catch_method() {
        with_battery(|rt| {
            let ok = rt
                .eval(
                    "(function() { \
                       var p = navigator.getBattery(); \
                       return typeof p.catch === 'function'; \
                     })()",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn get_battery_returns_same_promise() {
        // [SameObject]: WPT battery-promise asserts the identity.
        with_battery(|rt| {
            assert!(bool_eval(
                rt,
                "navigator.getBattery() === navigator.getBattery()"
            ));
        });
    }

    #[test]
    fn battery_manager_deterministic_state() {
        with_battery(|rt| {
            // ADR-007 desktop constants; the manager reaches the page through
            // the promise, so resolve it into a global first.
            rt.eval(
                "navigator.getBattery().then(function(b) { globalThis.__battery = b; })",
            )
            .unwrap();
            assert!(bool_eval(rt, "__battery instanceof BatteryManager"));
            assert!(bool_eval(rt, "__battery instanceof EventTarget"));
            assert!(bool_eval(rt, "__battery.charging === true"));
            assert!(bool_eval(rt, "__battery.chargingTime === 0"));
            assert!(bool_eval(rt, "__battery.dischargingTime === Infinity"));
            assert!(bool_eval(rt, "__battery.level === 1"));
            assert!(bool_eval(
                rt,
                "Object.prototype.toString.call(__battery) === '[object BatteryManager]'"
            ));
        });
    }

    #[test]
    fn battery_manager_attributes_are_readonly() {
        with_battery(|rt| {
            rt.eval(
                "navigator.getBattery().then(function(b) { globalThis.__battery = b; })",
            )
            .unwrap();
            // A write through the accessor must not move the engine's answer.
            rt.eval("__battery.charging = false; __battery.level = 0.42;").unwrap();
            assert!(bool_eval(rt, "__battery.charging === true"));
            assert!(bool_eval(rt, "__battery.level === 1"));
            // Brand check: a plain object receiver is rejected.
            assert!(bool_eval(
                rt,
                "(function() { try { BatteryManager.prototype.charging.call({}); return false; } catch (e) { return e instanceof TypeError; } })()"
            ));
        });
    }

    #[test]
    fn battery_manager_is_illegal_constructor() {
        with_battery(|rt| {
            assert!(bool_eval(
                rt,
                "(function() { try { new BatteryManager(); return false; } catch (e) { return e instanceof TypeError; } })()"
            ));
        });
    }

    #[test]
    fn battery_manager_on_handlers_round_trip() {
        with_battery(|rt| {
            rt.eval(
                "navigator.getBattery().then(function(b) { globalThis.__battery = b; })",
            )
            .unwrap();
            assert!(bool_eval(rt, "__battery.onlevelchange === null"));
            rt.eval(
                "__battery.onlevelchange = function() {}; \
                 __battery.addEventListener('levelchange', __battery.onlevelchange)",
            )
            .unwrap();
            // The EventTarget machinery works even though the engine never
            // fires a state change.
            assert!(bool_eval(
                rt,
                "(function() { var fired = false; \
                   __battery.addEventListener('chargingchange', function() { fired = true; }); \
                   __battery.dispatchEvent(new Event('chargingchange')); \
                   return fired && typeof __battery.onlevelchange === 'function'; })()"
            ));
        });
    }
}
