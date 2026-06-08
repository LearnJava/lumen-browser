/// Reporting API (W3C Reporting API Level 1).
///
/// Phase 0: observer infrastructure + report delivery binding.
/// - `new ReportingObserver(callback, opts?)` — observe report types
/// - `.observe()` / `.disconnect()` / `.takeRecords()`
/// - `Report {type, url, body}` — report object
/// - `_lumen_deliver_report(type, url, body_json)` — shell binding to inject reports
///
/// Phase 1: integration with CSP, deprecation, intervention, crash reports from shell.
use rquickjs::Ctx;

/// Install Reporting API bindings into the JS context.
pub fn install_reporting_api_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(REPORTING_API_SHIM)?;
    Ok(())
}

const REPORTING_API_SHIM: &str = r#"
(function() {
  // Global observer registry — list of active {callback, types, queue} objects.
  var _reporting_observers = [];

  // --- Report class ---

  // W3C Reporting API §5: Report interface.
  function Report(type, url, body) {
    this.type = type;
    this.url = url;
    this.body = body || null;
  }

  Report.prototype.toJSON = function() {
    return { type: this.type, url: this.url, body: this.body };
  };

  globalThis.Report = Report;

  // --- ReportingObserver class ---

  // W3C Reporting API §6.1: ReportingObserver constructor.
  // opts: { types?: string[], buffered?: boolean }
  function ReportingObserver(callback, opts) {
    if (typeof callback !== 'function') {
      throw new TypeError('ReportingObserver: callback must be a function');
    }
    this._callback = callback;
    this._types = (opts && Array.isArray(opts.types)) ? opts.types.slice() : null;
    this._buffered = (opts && opts.buffered === true);
    this._queue = [];
    this._observing = false;
  }

  // §6.1: observe() — start receiving reports.
  ReportingObserver.prototype.observe = function() {
    if (this._observing) return;
    this._observing = true;
    _reporting_observers.push(this);
    // §6.1: if buffered, replay already-queued global reports.
    if (this._buffered) {
      var self = this;
      _buffered_reports.forEach(function(r) {
        if (self._accepts(r)) {
          self._queue.push(r);
        }
      });
      if (self._queue.length > 0) {
        var records = self._queue.splice(0);
        try { self._callback(records, self); } catch (_) {}
      }
    }
  };

  // §6.1: disconnect() — stop receiving reports.
  ReportingObserver.prototype.disconnect = function() {
    this._observing = false;
    var idx = _reporting_observers.indexOf(this);
    if (idx !== -1) _reporting_observers.splice(idx, 1);
    this._queue = [];
  };

  // §6.1: takeRecords() — return queued reports and clear the queue.
  ReportingObserver.prototype.takeRecords = function() {
    return this._queue.splice(0);
  };

  // Internal: check whether this observer accepts a given report type.
  ReportingObserver.prototype._accepts = function(report) {
    if (!this._types) return true;
    return this._types.indexOf(report.type) !== -1;
  };

  globalThis.ReportingObserver = ReportingObserver;

  // Buffered store — holds the last 100 reports for buffered observers.
  var _buffered_reports = [];
  var _BUFFER_LIMIT = 100;

  // §6.2: deliver a report to all matching active observers.
  function _deliver(report) {
    _buffered_reports.push(report);
    if (_buffered_reports.length > _BUFFER_LIMIT) {
      _buffered_reports.shift();
    }
    _reporting_observers.forEach(function(obs) {
      if (!obs._accepts(report)) return;
      obs._queue.push(report);
      var records = obs._queue.splice(0);
      try { obs._callback(records, obs); } catch (_) {}
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
    _deliver(new Report(type, url, body));
  };
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

    fn with_api(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_reporting_api_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn reporting_observer_exists() {
        with_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof ReportingObserver === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn report_class_exists() {
        with_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof Report === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn observe_disconnect_take_records() {
        with_api(|ctx| {
            let ok: bool = ctx
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
            assert!(ok);
        });
    }

    #[test]
    fn take_records_returns_queued() {
        with_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var obs = new ReportingObserver(function() {}, {});
                    obs.observe();
                    obs._queue.push(new Report('deprecation', 'https://a.com', null));
                    var recs = obs.takeRecords();
                    obs.disconnect();
                    recs.length === 1 && recs[0].type === 'deprecation'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn type_filter_excludes_unmatched() {
        with_api(|ctx| {
            let ok: bool = ctx
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
            assert!(ok);
        });
    }

    #[test]
    fn deliver_report_binding_exists() {
        with_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof _lumen_deliver_report === 'function'")
                .unwrap();
            assert!(ok);
        });
    }
}
