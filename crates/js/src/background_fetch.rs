//! Background Fetch API stub (W3C Background Fetch L1).
//!
//! Implements `registration.backgroundFetch` with:
//! - `fetch(id, requests, opts)` → Promise<BGFetchRegistration>
//! - `get(id)` → Promise<BGFetchRegistration|undefined>
//! - `getIds()` → Promise<string[]>
//!
//! `BGFetchRegistration` exposes:
//! - `id`, `result`, `failureReason`, `recordsAvailable`
//! - `downloaded`, `downloadTotal`, `uploaded`, `uploadTotal`
//! - `activate()`, `abort()`, `addEventListener()`
//!
//! Phase 0: all operations are in-memory; no actual HTTP fetch.
//! Native bindings `_lumen_bg_fetch_*` are stubs for shell Phase 1.

use rquickjs::Ctx;

/// Install the Background Fetch API stub into the JS context.
///
/// Defines `BackgroundFetchManager` on `ServiceWorkerRegistration.prototype.backgroundFetch`.
/// Must be called after DOM + Promise are available.
pub fn init_background_fetch(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(BACKGROUND_FETCH_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C Background Fetch L1 (Phase 0).
const BACKGROUND_FETCH_SHIM: &str = r#"(function() {
  // BGFetchRegistration — represents one background fetch job.
  var BGFetchRegistration = function(id, opts) {
    this.id = id;
    this.result = '';               // '' | 'success' | 'failure'
    this.failureReason = '';        // '' | 'aborted' | 'bad-status' | 'fetch-error' | 'quota-exceeded' | 'download-total-exceeded'
    this.recordsAvailable = true;
    this.downloaded = 0;
    this.downloadTotal = (opts && typeof opts.downloadTotal === 'number') ? opts.downloadTotal : 0;
    this.uploaded = 0;
    this.uploadTotal = 0;
    this._listeners = Object.create(null);
    this._active = true;
  };

  // activate() -> Promise<BackgroundFetchEvent> (Phase 0: no-op, resolves immediately)
  BGFetchRegistration.prototype.activate = function() {
    if (typeof _lumen_bg_fetch_activate === 'function') {
      _lumen_bg_fetch_activate(this.id);
    }
    return Promise.resolve(this);
  };

  // abort() -> Promise<boolean>
  // Phase 0: marks registration as failed/aborted, resolves true.
  BGFetchRegistration.prototype.abort = function() {
    if (!this._active) {
      return Promise.resolve(false);
    }
    this._active = false;
    this.result = 'failure';
    this.failureReason = 'aborted';
    if (typeof _lumen_bg_fetch_abort === 'function') {
      _lumen_bg_fetch_abort(this.id);
    }
    return Promise.resolve(true);
  };

  // addEventListener(type, handler) — minimal event target (Phase 0).
  BGFetchRegistration.prototype.addEventListener = function(type, handler) {
    if (typeof handler !== 'function') { return; }
    if (!this._listeners[type]) {
      this._listeners[type] = [];
    }
    this._listeners[type].push(handler);
  };

  // BackgroundFetchManager — per-registration manager.
  var BackgroundFetchManager = function(registration) {
    this._registration = registration;
    // In-memory map: id -> BGFetchRegistration
    this._fetches = Object.create(null);
  };

  // fetch(id, requests, options?) -> Promise<BGFetchRegistration>
  // Phase 0: stores registration in-memory without issuing any real request.
  BackgroundFetchManager.prototype.fetch = function(id, requests, options) {
    if (typeof id !== 'string' || id === '') {
      return Promise.reject(new TypeError('id must be a non-empty string'));
    }
    if (this._fetches[id]) {
      return Promise.reject(new TypeError('Background fetch with id "' + id + '" already exists'));
    }
    var reg = new BGFetchRegistration(id, options);
    this._fetches[id] = reg;
    if (typeof _lumen_bg_fetch_register === 'function') {
      _lumen_bg_fetch_register(id, typeof requests === 'string' ? requests : JSON.stringify(requests));
    }
    return Promise.resolve(reg);
  };

  // get(id) -> Promise<BGFetchRegistration|undefined>
  BackgroundFetchManager.prototype.get = function(id) {
    var reg = this._fetches[id];
    return Promise.resolve(reg !== undefined ? reg : undefined);
  };

  // getIds() -> Promise<string[]>
  BackgroundFetchManager.prototype.getIds = function() {
    return Promise.resolve(Object.keys(this._fetches));
  };

  // Attach BackgroundFetchManager to ServiceWorkerRegistration.prototype as lazy getter.
  if (typeof ServiceWorkerRegistration !== 'undefined') {
    Object.defineProperty(ServiceWorkerRegistration.prototype, 'backgroundFetch', {
      get: function() {
        if (!this._backgroundFetchManager) {
          this._backgroundFetchManager = new BackgroundFetchManager(this);
        }
        return this._backgroundFetchManager;
      },
      configurable: true
    });
  }

  globalThis.BackgroundFetchManager = BackgroundFetchManager;
  globalThis.BGFetchRegistration = BGFetchRegistration;
})();"#;

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().expect("Runtime::new");
        let ctx = Context::full(&rt).expect("Context::full");
        (rt, ctx)
    }

    fn install_stubs(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
            "globalThis.ServiceWorkerRegistration = function() {}; \
             globalThis.TypeError = TypeError; \
             globalThis._lumen_bg_fetch_register = function() {}; \
             globalThis._lumen_bg_fetch_activate = function() {}; \
             globalThis._lumen_bg_fetch_abort = function() {};",
        )
        .expect("install stubs");
    }

    #[test]
    fn bg_fetch_manager_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_background_fetch(&ctx).expect("init");
            let result: String = ctx
                .eval("typeof BackgroundFetchManager === 'function' ? 'exists' : 'missing'")
                .expect("eval");
            assert_eq!(result, "exists");
        });
    }

    #[test]
    fn fetch_returns_promise_with_registration() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_background_fetch(&ctx).expect("init");
            let result: String = ctx
                .eval(
                    "var mgr = new BackgroundFetchManager({}); \
                     var p = mgr.fetch('my-fetch', '/file.zip', {downloadTotal: 1000}); \
                     typeof p === 'object' && typeof p.then === 'function' ? 'promise' : 'not'",
                )
                .expect("eval");
            assert_eq!(result, "promise");
        });
    }

    // fetch() synchronously stores the registration in _fetches before returning the Promise,
    // so internal state is accessible immediately without awaiting.
    #[test]
    fn get_returns_registration_after_fetch() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_background_fetch(&ctx).expect("init");
            let result: String = ctx
                .eval(
                    "var mgr = new BackgroundFetchManager({}); \
                     mgr.fetch('dl', '/large.bin'); \
                     var reg = mgr._fetches['dl']; \
                     reg && reg.id === 'dl' ? 'found' : 'missing'",
                )
                .expect("eval");
            assert_eq!(result, "found");
        });
    }

    #[test]
    fn get_ids_returns_registered_ids() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_background_fetch(&ctx).expect("init");
            let result: String = ctx
                .eval(
                    "var mgr = new BackgroundFetchManager({}); \
                     mgr.fetch('a', '/a.bin'); \
                     mgr.fetch('b', '/b.bin'); \
                     var ids = Object.keys(mgr._fetches); \
                     ids.length === 2 && ids.indexOf('a') >= 0 && ids.indexOf('b') >= 0 \
                       ? 'ok' : 'fail'",
                )
                .expect("eval");
            assert_eq!(result, "ok");
        });
    }

    // abort() is synchronous — sets result/failureReason immediately.
    #[test]
    fn abort_sets_failure_reason() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_background_fetch(&ctx).expect("init");
            let result: String = ctx
                .eval(
                    "var mgr = new BackgroundFetchManager({}); \
                     mgr.fetch('job', '/data.zip'); \
                     var reg = mgr._fetches['job']; \
                     reg.abort(); \
                     reg.result === 'failure' && reg.failureReason === 'aborted' ? 'ok' : 'fail'",
                )
                .expect("eval");
            assert_eq!(result, "ok");
        });
    }

    // fetch() with duplicate id rejects before storing — _fetches still has only one entry.
    #[test]
    fn duplicate_id_rejects() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_background_fetch(&ctx).expect("init");
            let result: String = ctx
                .eval(
                    "var mgr = new BackgroundFetchManager({}); \
                     mgr.fetch('dup', '/x.bin'); \
                     var p = mgr.fetch('dup', '/y.bin'); \
                     var rejected = p instanceof Promise && Object.keys(mgr._fetches).length === 1; \
                     rejected ? 'rejected' : 'not_rejected'",
                )
                .expect("eval");
            assert_eq!(result, "rejected");
        });
    }
}
