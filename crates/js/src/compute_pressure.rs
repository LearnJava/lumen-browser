/// Compute Pressure API stub (W3C Compute Pressure Level 1)
/// Phase 0: PressureObserver registers callback but never fires it — no actual CPU sampling.
/// `PressureRecord {source, state:'nominal', time}` class is exposed.
/// `PressureObserver.knownSources()` returns `['cpu']`.
use rquickjs::Ctx;

/// Install Compute Pressure API bindings into the JS context.
pub fn install_compute_pressure_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(COMPUTE_PRESSURE_SHIM)?;
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn with_compute_pressure_api(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;
                function DOMException(message, name) {
                  Error.call(this, message);
                  this.message = message;
                  this.name = name || 'Error';
                }
                DOMException.prototype = Object.create(Error.prototype);
                DOMException.prototype.constructor = DOMException;
                globalThis.DOMException = DOMException;
                "#,
            )
            .unwrap();
            install_compute_pressure_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn pressure_observer_class_exists() {
        with_compute_pressure_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.PressureObserver === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn pressure_observer_known_sources_returns_cpu() {
        with_compute_pressure_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    "JSON.stringify(PressureObserver.knownSources()) === JSON.stringify(['cpu'])",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn pressure_observer_observe_returns_promise() {
        with_compute_pressure_api(|ctx| {
            let ok: bool = ctx
                .eval("new PressureObserver(function(){}).observe('cpu') instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn pressure_observer_disconnect_removes_sources() {
        with_compute_pressure_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var obs = new PressureObserver(function(){});
                    obs.observe('cpu');
                    obs.disconnect();
                    obs._observed.size === 0
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn pressure_record_class_exists() {
        with_compute_pressure_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var r = new PressureRecord('cpu', 'nominal', 0);
                    r.source === 'cpu' && r.state === 'nominal' && r.time === 0
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
