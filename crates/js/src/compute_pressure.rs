//! Compute Pressure API stub (W3C Compute Pressure Level 1)
//!
//! Phase 0: PressureObserver registers callback but never fires it — no actual CPU sampling.
//! `PressureRecord {source, state:'nominal', time}` class is exposed.
//! `PressureObserver.knownSources()` returns `['cpu']`.

/// V8 port of the former rquickjs `install_compute_pressure_bindings` (Ph3 V8 migration
/// S5-S7, rquickjs side removed in S12b-8): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_compute_pressure_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(COMPUTE_PRESSURE_SHIM)?;
    Ok(())
}

/// JavaScript shim: Compute Pressure API (Phase 0 - PressureObserver never fires callback)
#[cfg(feature = "v8-backend")]
const COMPUTE_PRESSURE_SHIM: &str = r#"
(function() {
  // PressureRecord — immutable snapshot of a single pressure reading
  class PressureRecord {
    constructor(source, state, time) {
      this.source = source;
      this.state = state;
      this.time = time;
    }

    toJSON() {
      return { source: this.source, state: this.state, time: this.time };
    }
  }
  window.PressureRecord = PressureRecord;

  // PressureObserver — observes compute pressure sources (Phase 0: no actual sampling)
  class PressureObserver {
    constructor(callback) {
      if (typeof callback !== 'function') {
        throw new TypeError('PressureObserver callback must be a function');
      }
      this._callback = callback;
      this._observed = new Set();
    }

    // Returns a Promise that resolves when observation begins.
    // Phase 0: always succeeds for 'cpu'; rejects for unknown sources.
    observe(source) {
      const known = PressureObserver.knownSources();
      if (!known.includes(source)) {
        return Promise.reject(new TypeError('Unknown pressure source: ' + source));
      }
      this._observed.add(source);
      return Promise.resolve();
    }

    // Remove a source from observation; no-op if not observed.
    unobserve(source) {
      this._observed.delete(source);
    }

    // Stop all observation and discard pending records.
    disconnect() {
      this._observed.clear();
    }

    // W3C §4.1: static list of supported sources.
    static knownSources() {
      return ['cpu'];
    }
  }
  window.PressureObserver = PressureObserver;
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_compute_pressure(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("var window = globalThis;").unwrap();
        install_compute_pressure_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn pressure_observer_class_exists() {
        with_compute_pressure(|rt| {
            let ok = rt
                .eval("typeof window.PressureObserver === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn pressure_observer_known_sources_returns_cpu() {
        with_compute_pressure(|rt| {
            let ok = rt
                .eval("JSON.stringify(PressureObserver.knownSources()) === JSON.stringify(['cpu'])")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn pressure_observer_observe_returns_promise() {
        with_compute_pressure(|rt| {
            let ok = rt
                .eval("new PressureObserver(function(){}).observe('cpu') instanceof Promise")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn pressure_observer_disconnect_removes_sources() {
        with_compute_pressure(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var obs = new PressureObserver(function(){});
                    obs.observe('cpu');
                    obs.disconnect();
                    obs._observed.size === 0
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn pressure_record_class_exists() {
        with_compute_pressure(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var r = new PressureRecord('cpu', 'nominal', 0);
                    r.source === 'cpu' && r.state === 'nominal' && r.time === 0
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
