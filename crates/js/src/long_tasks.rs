//! W3C Long Tasks API — JS shim.
//!
//! Installs `PerformanceLongTaskTiming` and `TaskAttributionTiming` classes plus
//! the `_lumen_deliver_longtask_entry(...)` delivery binding.
//!
//! Detection lives in `crates/shell`, not here: [`crate::v8_runtime::V8JsRuntime`]
//! has no notion of a "task" boundary on its own, so the shell times every
//! `PersistentJs::eval_js` dispatch (timer/rAF/event-listener/classic-script
//! callback — every one of those is a discrete HTML-spec task) and calls
//! `_lumen_deliver_longtask_entry` when a dispatch runs over the spec's 50ms
//! threshold (`crates/shell/src/persistent_js.rs`). This module is only the
//! JS-visible entry shape.
//!
//! Attribution is deliberately the single "unknown culprit in this window"
//! shape (`tests/wpt/longtask-timing/longtask-attributes.html`'s expectation)
//! — no cross-frame attribution model exists in the engine yet, so every task
//! attributes to the top-level window itself.
//!
//! Spec: <https://w3c.github.io/longtasks/>

/// Install Long Tasks API into the JS context.
///
/// Evaluated via [`lumen_core::ext::JsRuntime::eval`]. Must be called after DOM
/// install so that `PerformanceObserver`, `_perf_entries`, and
/// `_perf_observer_notify` are already in scope.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_long_tasks_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(LONGTASK_SHIM)?;
    Ok(())
}

/// JS shim for the W3C Long Tasks API.
#[cfg(feature = "v8-backend")]
const LONGTASK_SHIM: &str = r#"(function() {
  'use strict';

  // W3C Long Tasks API §3.2 — per-container attribution of a long task.
  function TaskAttributionTiming(init) {
    var o = init || {};
    /// Always 'taskattribution' — not independently observable (excluded from
    /// PerformanceObserver.supportedEntryTypes), only reachable via `attribution`.
    this.entryType     = 'taskattribution';
    this.name          = typeof o.name          !== 'undefined' ? String(o.name)          : 'unknown';
    this.startTime     = Number(o.startTime)     || 0;
    this.duration      = Number(o.duration)      || 0;
    /// 'iframe' | 'embed' | 'object' | 'audio' | 'video' | 'track' | 'unknown' | 'window'.
    this.containerType = typeof o.containerType  !== 'undefined' ? String(o.containerType) : 'window';
    this.containerSrc  = typeof o.containerSrc   !== 'undefined' ? String(o.containerSrc)  : '';
    this.containerId   = typeof o.containerId    !== 'undefined' ? String(o.containerId)   : '';
    this.containerName = typeof o.containerName  !== 'undefined' ? String(o.containerName) : '';
  }
  TaskAttributionTiming.prototype.toJSON = function() {
    return {
      entryType: this.entryType, name: this.name,
      startTime: this.startTime, duration: this.duration,
      containerType: this.containerType, containerSrc: this.containerSrc,
      containerId: this.containerId, containerName: this.containerName
    };
  };
  globalThis.TaskAttributionTiming = TaskAttributionTiming;

  // W3C Long Tasks API §3.1 — a single task that blocked the main thread for
  // more than 50ms.
  function PerformanceLongTaskTiming(init) {
    var o = init || {};
    this.entryType = 'longtask';
    /// 'self' when the culprit ran in this window (the only case the engine
    /// can currently attribute — no cross-frame task model exists).
    this.name = typeof o.name !== 'undefined' ? String(o.name) : 'self';
    this.startTime = Number(o.startTime) || 0;
    // Rounded to the nearest millisecond, like every shipping implementation —
    // the spec leaves sub-ms resolution to the UA.
    this.duration = Math.round(Number(o.duration) || 0);
    var rawAttribution = Array.isArray(o.attribution) ? o.attribution : [new TaskAttributionTiming()];
    this.attribution = rawAttribution.map(function(a) {
      return a instanceof TaskAttributionTiming ? a : new TaskAttributionTiming(a);
    });
  }
  PerformanceLongTaskTiming.prototype.toJSON = function() {
    return {
      entryType: this.entryType, name: this.name,
      startTime: this.startTime, duration: this.duration,
      attribution: this.attribution.map(function(a) { return a.toJSON(); })
    };
  };
  globalThis.PerformanceLongTaskTiming = PerformanceLongTaskTiming;

  // Called by the shell after any `eval_js` dispatch measured over the 50ms
  // threshold.
  //
  //   start_ms    — task start (performance.now() equivalent)
  //   duration_ms — task duration; should be >= 50 to qualify
  globalThis._lumen_deliver_longtask_entry = function(start_ms, duration_ms) {
    var entry = new PerformanceLongTaskTiming({
      startTime: Number(start_ms),
      duration:  Number(duration_ms)
    });
    if (typeof _perf_entries !== 'undefined') {
      _perf_entries.push(entry);
    }
    if (typeof _perf_observer_notify === 'function') {
      _perf_observer_notify([entry]);
    }
  };
})();
"#;

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // `expect()` в хелперах тестового модуля: исключение из clippy.toml
    // покрывает только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Minimal performance infrastructure required to test observer delivery.
    const PERF_STUB: &str = r#"
        var _perf_entries = [];
        var _perf_observers = [];
        function _perf_observer_notify(entries) {
            for (var i = 0; i < _perf_observers.length; i++) {
                var obs = _perf_observers[i];
                for (var j = 0; j < entries.length; j++) {
                    if (obs._types.indexOf(entries[j].entryType) !== -1) {
                        var captured = entries;
                        obs._cb({ getEntries: function() { return captured; } }, obs);
                    }
                }
            }
        }
        function PerformanceObserver(cb) { this._cb = cb; this._types = []; }
        PerformanceObserver.prototype.observe = function(opts) {
            this._types = (opts && opts.entryTypes) ? opts.entryTypes : [];
            _perf_observers.push(this);
        };
        PerformanceObserver.prototype.disconnect = function() {
            var i = _perf_observers.indexOf(this);
            if (i !== -1) _perf_observers.splice(i, 1);
        };
    "#;

    fn with_longtasks(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        super::install_long_tasks_bindings_v8(&rt).expect("longtasks install failed");
        f(&rt);
    }

    fn with_longtasks_and_perf(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(PERF_STUB).expect("perf stub install failed");
        super::install_long_tasks_bindings_v8(&rt).expect("longtasks install failed");
        f(&rt);
    }

    #[test]
    fn longtask_timing_class_exists() {
        with_longtasks(|rt| {
            let ok = rt.eval("typeof PerformanceLongTaskTiming === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn task_attribution_class_exists() {
        with_longtasks(|rt| {
            let ok = rt.eval("typeof TaskAttributionTiming === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn deliver_binding_exists() {
        with_longtasks(|rt| {
            let ok = rt.eval("typeof _lumen_deliver_longtask_entry === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn default_attribution_matches_spec_shape() {
        // tests/wpt/longtask-timing/longtask-attributes.html's exact expectation.
        with_longtasks(|rt| {
            rt.eval("var e = new PerformanceLongTaskTiming({startTime: 10, duration: 60.4});")
                .unwrap();
            assert_eq!(rt.eval("e.entryType").unwrap(), JsValue::String("longtask".into()));
            assert_eq!(rt.eval("e.name").unwrap(), JsValue::String("self".into()));
            assert_eq!(rt.eval("e.duration").unwrap(), JsValue::Number(60.0));
            assert_eq!(rt.eval("e.attribution.length").unwrap(), JsValue::Number(1.0));
            assert_eq!(
                rt.eval("e.attribution[0].entryType").unwrap(),
                JsValue::String("taskattribution".into())
            );
            assert_eq!(rt.eval("e.attribution[0].name").unwrap(), JsValue::String("unknown".into()));
            assert_eq!(rt.eval("e.attribution[0].duration").unwrap(), JsValue::Number(0.0));
            assert_eq!(rt.eval("e.attribution[0].startTime").unwrap(), JsValue::Number(0.0));
            assert_eq!(
                rt.eval("e.attribution[0].containerType").unwrap(),
                JsValue::String("window".into())
            );
            assert_eq!(rt.eval("e.attribution[0].containerId").unwrap(), JsValue::String("".into()));
            assert_eq!(rt.eval("e.attribution[0].containerName").unwrap(), JsValue::String("".into()));
            assert_eq!(rt.eval("e.attribution[0].containerSrc").unwrap(), JsValue::String("".into()));
        });
    }

    #[test]
    fn deliver_creates_entry_in_perf_buffer() {
        with_longtasks_and_perf(|rt| {
            rt.eval("_lumen_deliver_longtask_entry(1000, 75);").unwrap();
            assert_eq!(rt.eval("_perf_entries.length").unwrap(), JsValue::Number(1.0));
            assert_eq!(
                rt.eval("_perf_entries[0].entryType").unwrap(),
                JsValue::String("longtask".into())
            );
            assert_eq!(rt.eval("_perf_entries[0].duration").unwrap(), JsValue::Number(75.0));
        });
    }

    #[test]
    fn deliver_notifies_observer() {
        with_longtasks_and_perf(|rt| {
            rt.eval(
                r#"var got = [];
                   var po = new PerformanceObserver(function(list) {
                       got = got.concat(list.getEntries());
                   });
                   po.observe({entryTypes: ['longtask']});
                   _lumen_deliver_longtask_entry(2000, 90);"#,
            )
            .unwrap();
            assert_eq!(rt.eval("got.length").unwrap(), JsValue::Number(1.0), "observer should have received 1 entry");
            assert_eq!(rt.eval("got[0].entryType").unwrap(), JsValue::String("longtask".into()));
        });
    }
}
