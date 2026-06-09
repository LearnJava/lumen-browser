/// Launch Handler API (WICG Web App Launch Handler).
///
/// Phase 0: in-memory queue + consumer callback infrastructure.
/// - `window.launchQueue` — `LaunchQueue` singleton
/// - `LaunchQueue.setConsumer(callback)` — registers a handler for launch params
/// - `LaunchParams` — `{targetURL, files[]}` object delivered to the consumer
///
/// Native bindings:
/// - `_lumen_deliver_launch_params(targetURL, files_json)` — called by shell to
///   deliver a new launch (Phase 1: actual OS file associations / URL activation).
use rquickjs::Ctx;

/// Install Launch Handler API bindings into the JS context.
pub fn install_launch_handler_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(LAUNCH_HANDLER_SHIM)?;
    Ok(())
}

const LAUNCH_HANDLER_SHIM: &str = r#"
(function() {
  'use strict';

  // Phase 0 native hook — no-op; shell delivers real params in Phase 1.
  if (typeof _lumen_deliver_launch_params === 'undefined') {
    globalThis._lumen_deliver_launch_params = function(_url, _filesJson) {};
  }

  // WICG Launch Handler §3.1 — LaunchParams.
  function LaunchParams(targetURL, files) {
    this.targetURL = targetURL || null;
    this.files = files || [];
  }

  // WICG Launch Handler §3.2 — LaunchQueue.
  function LaunchQueue() {
    this._consumer = null;
    // Unconsumed params queued before setConsumer() is called.
    this._pending = [];
  }

  // §3.2.1: setConsumer(consumer) — register a callback.
  // Immediately drains any pending LaunchParams already in the queue.
  LaunchQueue.prototype.setConsumer = function(consumer) {
    if (typeof consumer !== 'function') {
      throw new TypeError('LaunchQueue.setConsumer: consumer must be a function');
    }
    this._consumer = consumer;
    // Drain any params that arrived before the consumer was set.
    var pending = this._pending.splice(0);
    for (var i = 0; i < pending.length; i++) {
      try { consumer(pending[i]); } catch (_) {}
    }
  };

  // Internal: deliver a LaunchParams to the consumer (or enqueue if no consumer yet).
  LaunchQueue.prototype._deliver = function(params) {
    if (this._consumer) {
      try { this._consumer(params); } catch (_) {}
    } else {
      this._pending.push(params);
    }
  };

  // Install singleton on window.
  var launchQueue = new LaunchQueue();
  if (typeof globalThis !== 'undefined') {
    Object.defineProperty(globalThis, 'launchQueue', {
      value: launchQueue,
      writable: false,
      configurable: true,
      enumerable: true,
    });
  }
  if (typeof window !== 'undefined' && window !== globalThis) {
    Object.defineProperty(window, 'launchQueue', {
      value: launchQueue,
      writable: false,
      configurable: true,
      enumerable: true,
    });
  }

  // Export LaunchParams constructor for feature detection.
  globalThis.LaunchParams = LaunchParams;

  // §3.3: _lumen_deliver_launch_params(targetURL, filesJson) — called by shell.
  // filesJson is a JSON array of file name strings (Phase 0: names only).
  globalThis._lumen_deliver_launch_params = function(targetURL, filesJson) {
    var files = [];
    if (filesJson) {
      try {
        var names = JSON.parse(filesJson);
        for (var i = 0; i < names.length; i++) {
          files.push({ name: names[i], kind: 'file' });
        }
      } catch (_) {}
    }
    launchQueue._deliver(new LaunchParams(targetURL, files));
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

    fn with_lh(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>("var window = globalThis;").unwrap();
            install_launch_handler_api(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn launch_queue_exists() {
        with_lh(|ctx| {
            let ok: bool = ctx
                .eval("typeof launchQueue === 'object' && launchQueue !== null")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn set_consumer_is_function() {
        with_lh(|ctx| {
            let ok: bool = ctx
                .eval("typeof launchQueue.setConsumer === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn launch_params_constructor_exported() {
        with_lh(|ctx| {
            let ok: bool = ctx
                .eval("typeof LaunchParams === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn set_consumer_rejects_non_function() {
        with_lh(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var threw = false;
                    try { launchQueue.setConsumer('not a function'); }
                    catch(e) { threw = e instanceof TypeError; }
                    threw
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn consumer_receives_delivered_params() {
        with_lh(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var received = null;
                    launchQueue.setConsumer(function(p) { received = p; });
                    _lumen_deliver_launch_params('https://example.com/app', '[]');
                    received !== null && received.targetURL === 'https://example.com/app'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn params_queued_before_consumer_drained_on_set() {
        with_lh(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    _lumen_deliver_launch_params('https://example.com/early', '[]');
                    var received = null;
                    launchQueue.setConsumer(function(p) { received = p; });
                    received !== null && received.targetURL === 'https://example.com/early'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn files_parsed_from_json() {
        with_lh(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var received = null;
                    launchQueue.setConsumer(function(p) { received = p; });
                    _lumen_deliver_launch_params('https://x.com/', '["doc.pdf","img.png"]');
                    received.files.length === 2 && received.files[0].name === 'doc.pdf'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn null_target_url_when_omitted() {
        with_lh(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var received = null;
                    launchQueue.setConsumer(function(p) { received = p; });
                    _lumen_deliver_launch_params(null, '[]');
                    received !== null && received.targetURL === null
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn window_alias_works() {
        with_lh(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.launchQueue === 'object'")
                .unwrap();
            assert!(ok);
        });
    }
}
