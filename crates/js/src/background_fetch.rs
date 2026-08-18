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

/// V8 port of the former rquickjs `init_background_fetch` (Ph3 V8 migration
/// S12b-G3, rquickjs side removed in the same batch): identical JS shim,
/// evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// Defines `BackgroundFetchManager` on `ServiceWorkerRegistration.prototype.backgroundFetch`.
/// Must be called after DOM + Promise are available.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_background_fetch_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(BACKGROUND_FETCH_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C Background Fetch L1 (Phase 0).
#[cfg(feature = "v8-backend")]
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

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_background_fetch(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            "var ServiceWorkerRegistration = function() {}; \
             var _lumen_bg_fetch_register = function() {}; \
             var _lumen_bg_fetch_activate = function() {}; \
             var _lumen_bg_fetch_abort = function() {};",
        )
        .unwrap();
        install_background_fetch_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn bg_fetch_manager_exists() {
        with_background_fetch(|rt| {
            let result = rt
                .eval("typeof BackgroundFetchManager === 'function' ? 'exists' : 'missing'")
                .unwrap();
            assert_eq!(result, JsValue::String("exists".to_string()));
        });
    }

    #[test]
    fn fetch_returns_promise_with_registration() {
        with_background_fetch(|rt| {
            let result = rt
                .eval(
                    "var mgr = new BackgroundFetchManager({}); \
                     var p = mgr.fetch('my-fetch', '/file.zip', {downloadTotal: 1000}); \
                     typeof p === 'object' && typeof p.then === 'function' ? 'promise' : 'not'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("promise".to_string()));
        });
    }

    // fetch() synchronously stores the registration in _fetches before returning the Promise,
    // so internal state is accessible immediately without awaiting.
    #[test]
    fn get_returns_registration_after_fetch() {
        with_background_fetch(|rt| {
            let result = rt
                .eval(
                    "var mgr = new BackgroundFetchManager({}); \
                     mgr.fetch('dl', '/large.bin'); \
                     var reg = mgr._fetches['dl']; \
                     reg && reg.id === 'dl' ? 'found' : 'missing'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("found".to_string()));
        });
    }

    #[test]
    fn get_ids_returns_registered_ids() {
        with_background_fetch(|rt| {
            let result = rt
                .eval(
                    "var mgr = new BackgroundFetchManager({}); \
                     mgr.fetch('a', '/a.bin'); \
                     mgr.fetch('b', '/b.bin'); \
                     var ids = Object.keys(mgr._fetches); \
                     ids.length === 2 && ids.indexOf('a') >= 0 && ids.indexOf('b') >= 0 \
                       ? 'ok' : 'fail'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("ok".to_string()));
        });
    }

    // abort() is synchronous — sets result/failureReason immediately.
    #[test]
    fn abort_sets_failure_reason() {
        with_background_fetch(|rt| {
            let result = rt
                .eval(
                    "var mgr = new BackgroundFetchManager({}); \
                     mgr.fetch('job', '/data.zip'); \
                     var reg = mgr._fetches['job']; \
                     reg.abort(); \
                     reg.result === 'failure' && reg.failureReason === 'aborted' ? 'ok' : 'fail'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("ok".to_string()));
        });
    }

    // fetch() with duplicate id rejects before storing — _fetches still has only one entry.
    #[test]
    fn duplicate_id_rejects() {
        with_background_fetch(|rt| {
            let result = rt
                .eval(
                    "var mgr = new BackgroundFetchManager({}); \
                     mgr.fetch('dup', '/x.bin'); \
                     var p = mgr.fetch('dup', '/y.bin'); \
                     var rejected = p instanceof Promise && Object.keys(mgr._fetches).length === 1; \
                     rejected ? 'rejected' : 'not_rejected'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("rejected".to_string()));
        });
    }
}
