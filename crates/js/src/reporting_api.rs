//! Reporting API (W3C Reporting API Level 1).
//!
//! Phase 0: observer infrastructure + report delivery binding.
//! - `new ReportingObserver(callback, opts?)` — observe report types
//! - `.observe()` / `.disconnect()` / `.takeRecords()`
//! - `Report {type, url, body}` — report object
//! - `_lumen_deliver_report(type, url, body_json)` — shell binding to inject reports
//!
//! Phase 1: integration with CSP, deprecation, intervention, crash reports from shell.

/// V8 port of the former rquickjs `install_reporting_api_bindings` (Ph3 V8 migration
/// S5-S7, rquickjs side removed in S12b-B3): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_reporting_api_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(REPORTING_API_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const REPORTING_API_SHIM: &str = r#"
(function() {
  'use strict';
  // Global observer registry — active ReportingObserver instances.
  var _reporting_observers = [];
  // BUG-629: per-instance state lives in closure WeakMaps, not on `this`
  // — no web-visible `_callback`/`_queue`/... and nothing to leak onto
  // globalThis.
  var _reportData = new WeakMap();
  var _observerState = new WeakMap();

  // WebIDL: interface objects are {writable, !enumerable, configurable}.
  function _defineGlobal(name, value) {
    Object.defineProperty(globalThis, name, {
      value: value, writable: true, enumerable: false, configurable: true
    });
  }

  // --- Report interface (W3C Reporting API §2.3) ---
  // No constructor in the IDL: page script must get `Illegal constructor`
  // both with and without `new` (BUG-629 finding 3). Instances are made
  // only by `_makeReport` below.
  function Report() { throw new TypeError('Illegal constructor'); }

  function _reportField(obj, key) {
    var d = _reportData.get(obj);
    if (!d) throw new TypeError('Illegal invocation');
    return d[key];
  }

  // readonly attributes: accessors on the prototype, enumerable per WebIDL.
  ['type', 'url', 'body'].forEach(function(key) {
    Object.defineProperty(Report.prototype, key, {
      get: { [key]: function() { return _reportField(this, key); } }[key],
      enumerable: true, configurable: true
    });
  });

  // [Default] object toJSON() — an operation, hence non-enumerable.
  Object.defineProperty(Report.prototype, 'toJSON', {
    value: function toJSON() {
      return { type: _reportField(this, 'type'), url: _reportField(this, 'url'), body: _reportField(this, 'body') };
    },
    writable: true, enumerable: false, configurable: true
  });
  Object.defineProperty(Report.prototype, Symbol.toStringTag, {
    value: 'Report', writable: false, enumerable: false, configurable: true
  });

  function _makeReport(type, url, body) {
    var r = Object.create(Report.prototype);
    _reportData.set(r, { type: String(type), url: String(url), body: body === undefined ? null : body });
    return r;
  }

  _defineGlobal('Report', Report);

  // --- ReportingObserver interface (§3.1) ---
  function _accepts(st, report) {
    if (!st.types) return true;
    return st.types.indexOf(_reportData.get(report).type) !== -1;
  }

  function _state(obj) {
    var st = _observerState.get(obj);
    if (!st) throw new TypeError('Illegal invocation');
    return st;
  }

  function _invoke(st, obs, records) {
    try { st.callback.call(obs, records, obs); } catch (e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); }
  }

  // `class` gives the WebIDL shape for free: calling without `new` throws
  // (BUG-629 findings 1-2) and methods are non-enumerable (finding 4).
  class ReportingObserver {
    // opts: { types?: string[], buffered?: boolean }
    constructor(callback, opts) {
      if (typeof callback !== 'function') {
        throw new TypeError("Failed to construct 'ReportingObserver': parameter 1 is not of type 'Function'.");
      }
      _observerState.set(this, {
        callback: callback,
        types: (opts && opts.types != null) ? Array.from(opts.types, String) : null,
        buffered: !!(opts && opts.buffered),
        queue: [],
        observing: false
      });
    }

    // §3.1: observe() — start receiving reports.
    observe() {
      var st = _state(this);
      if (st.observing) return;
      st.observing = true;
      _reporting_observers.push(this);
      // If buffered, replay already-generated reports.
      if (st.buffered) {
        _buffered_reports.forEach(function(r) {
          if (_accepts(st, r)) st.queue.push(r);
        });
        if (st.queue.length > 0) _invoke(st, this, st.queue.splice(0));
      }
    }

    // §3.1: disconnect() — stop receiving reports.
    disconnect() {
      var st = _state(this);
      st.observing = false;
      var idx = _reporting_observers.indexOf(this);
      if (idx !== -1) _reporting_observers.splice(idx, 1);
      st.queue = [];
    }

    // §3.1: takeRecords() — return queued reports and clear the queue.
    takeRecords() {
      return _state(this).queue.splice(0);
    }
  }
  Object.defineProperty(ReportingObserver.prototype, Symbol.toStringTag, {
    value: 'ReportingObserver', writable: false, enumerable: false, configurable: true
  });

  _defineGlobal('ReportingObserver', ReportingObserver);

  // Buffered store — holds the last 100 reports for buffered observers.
  var _buffered_reports = [];
  var _BUFFER_LIMIT = 100;

  // Deliver a report to all matching active observers.
  function _deliver(report) {
    _buffered_reports.push(report);
    if (_buffered_reports.length > _BUFFER_LIMIT) {
      _buffered_reports.shift();
    }
    _reporting_observers.slice().forEach(function(obs) {
      var st = _observerState.get(obs);
      if (!st || !_accepts(st, report)) return;
      st.queue.push(report);
      _invoke(st, obs, st.queue.splice(0));
    });
  }

  // Native binding — called by shell or other browser subsystems to deliver reports.
  // type: string (e.g. 'csp-violation', 'deprecation', 'intervention', 'crash')
  // url: string — page URL at time of report
  // body_json: string — JSON-serialised report body (optional)
  globalThis._lumen_deliver_report = function(type, url, body_json) {
    var body = null;
    if (body_json) {
      try { body = JSON.parse(body_json); } catch (_) { body = body_json; }
    }
    _deliver(_makeReport(type, url, body));
  };
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        install_reporting_api_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn reporting_observer_exists() {
        with_api(|rt| {
            let ok = rt.eval("typeof ReportingObserver === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn report_class_exists() {
        with_api(|rt| {
            let ok = rt.eval("typeof Report === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn observe_disconnect_take_records() {
        with_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var received = [];
                    var obs = new ReportingObserver(function(reports) {
                        received = received.concat(reports);
                    }, { types: ['csp-violation'] });
                    obs.observe();
                    _lumen_deliver_report('csp-violation', 'https://example.com', '{"effectiveDirective":"script-src"}');
                    obs.disconnect();
                    received.length === 1 &&
                    received[0].type === 'csp-violation' &&
                    received[0].url === 'https://example.com'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn take_records_empty_when_nothing_queued() {
        with_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var obs = new ReportingObserver(function() {}, { buffered: true });
                    var empty = obs.takeRecords();
                    obs.disconnect();
                    Array.isArray(empty) && empty.length === 0
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn type_filter_excludes_unmatched() {
        with_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var received = [];
                    var obs = new ReportingObserver(function(reports) {
                        received = received.concat(reports);
                    }, { types: ['deprecation'] });
                    obs.observe();
                    _lumen_deliver_report('csp-violation', 'https://example.com', null);
                    obs.disconnect();
                    received.length === 0
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn deliver_report_binding_exists() {
        with_api(|rt| {
            let ok = rt.eval("typeof _lumen_deliver_report === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    // ── BUG-629: WebIDL-форма Report / ReportingObserver ─────────────────────

    fn eval_bool(src: &str) -> bool {
        let rt = V8JsRuntime::new().unwrap();
        install_reporting_api_bindings_v8(&rt).unwrap();
        rt.eval(src).unwrap() == JsValue::Bool(true)
    }

    /// Вызов без `new` бросает TypeError и не пишет состояние в globalThis.
    #[test]
    fn bug629_observer_without_new_throws_and_does_not_pollute_global() {
        assert!(eval_bool(
            "var t = false; try { ReportingObserver(function(){}); } catch (e) { t = e instanceof TypeError; }              t && !('_callback' in globalThis) && !('_queue' in globalThis) && !('_types' in globalThis)"
        ));
    }

    /// У `Report` нет конструктора: и с `new`, и без — `Illegal constructor`,
    /// глобальные `type`/`url` не появляются.
    #[test]
    fn bug629_report_is_not_constructible() {
        assert!(eval_bool(
            "var a = false, b = false;              try { new Report('csp-violation', 'https://x/', {}); } catch (e) { a = e instanceof TypeError; }              try { Report('t', 'u', null); } catch (e) { b = e instanceof TypeError; }              a && b && !('type' in globalThis) && !('url' in globalThis)"
        ));
    }

    /// Операции прототипа неперечислимы, внутреннего `_accepts` нет,
    /// у экземпляра нет собственных свойств.
    #[test]
    fn bug629_prototype_operations_non_enumerable() {
        assert!(eval_bool(
            "var p = ReportingObserver.prototype;              var ok = ['observe', 'disconnect', 'takeRecords'].every(function(m) {                var d = Object.getOwnPropertyDescriptor(p, m);                return d && typeof d.value === 'function' && !d.enumerable && d.writable && d.configurable; });              var o = new ReportingObserver(function(){}); var keys = []; for (var k in o) keys.push(k);              ok && !('_accepts' in p) && keys.length === 0 && Object.getOwnPropertyNames(o).length === 0 &&              !Object.getOwnPropertyDescriptor(Report.prototype, 'toJSON').enumerable &&              !Object.getOwnPropertyDescriptor(globalThis, 'Report').enumerable &&              !Object.getOwnPropertyDescriptor(globalThis, 'ReportingObserver').enumerable"
        ));
    }

    /// Доставленный отчёт — настоящий `Report`: readonly-аксессоры на
    /// прототипе, `toJSON`, `instanceof`; подделать через прототип нельзя.
    #[test]
    fn bug629_delivered_report_shape() {
        assert!(eval_bool(
            "var got = null;              var o = new ReportingObserver(function(r) { got = r[0]; }); o.observe();              _lumen_deliver_report('deprecation', 'https://a.example/', '{\"id\":\"x\"}'); o.disconnect();              var d = Object.getOwnPropertyDescriptor(Report.prototype, 'type');              var forged = false; try { Object.create(Report.prototype).type; } catch (e) { forged = e instanceof TypeError; }              got instanceof Report && got.type === 'deprecation' && got.url === 'https://a.example/' &&              got.body.id === 'x' && typeof d.get === 'function' && d.set === undefined && d.enumerable &&              JSON.stringify(got) === '{\"type\":\"deprecation\",\"url\":\"https://a.example/\",\"body\":{\"id\":\"x\"}}' &&              Object.prototype.toString.call(got) === '[object Report]' && forged"
        ));
    }

    /// buffered: true переигрывает уже выпущенные отчёты при observe().
    #[test]
    fn bug629_buffered_replay_still_works() {
        assert!(eval_bool(
            "_lumen_deliver_report('intervention', 'https://b/', null);              var got = []; var o = new ReportingObserver(function(r) { got = got.concat(r); }, { buffered: true, types: ['intervention'] });              o.observe(); o.disconnect(); got.length === 1 && got[0].type === 'intervention' && got[0].body === null"
        ));
    }
}
