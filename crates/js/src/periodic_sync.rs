//! Periodic Background Sync API stub (W3C Periodic Background Sync).
//!
//! Implements `registration.periodicSync` with:
//! - `register(tag, {minInterval})` — register a periodic sync tag
//! - `unregister(tag)` — unregister a periodic sync tag
//! - `getTags()` → Promise<string[]> — list registered tags
//!
//! Phase 0: Tags stored in-memory per registration. Native bindings
//! `_lumen_periodic_sync_register(tag, minInterval)` and
//! `_lumen_periodic_sync_unregister(tag)` are no-ops prepared for
//! shell Phase 1 (OS task scheduler integration).

/// V8 port of the former rquickjs `init_periodic_sync` (Ph3 V8 migration
/// S12b-G2, rquickjs side removed in the same batch): identical JS shim,
/// evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// Defines `PeriodicSyncManager` class on `ServiceWorkerRegistration.prototype`.
/// Must be called after worker registration and Promise are available.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_periodic_sync_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(PERIODIC_SYNC_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C Periodic Background Sync (Phase 0).
#[cfg(feature = "v8-backend")]
const PERIODIC_SYNC_SHIM: &str = r#"(function() {
  // PeriodicSyncManager — manages periodic sync registrations per ServiceWorkerRegistration.
  var PeriodicSyncManager = function(registration) {
    this._registration = registration;
    // In-memory map: tag -> {minInterval}
    this._tags = Object.create(null);
  };

  // register(tag, options?) -> Promise<void>
  // options.minInterval: minimum interval in milliseconds (ignored in Phase 0).
  PeriodicSyncManager.prototype.register = function(tag, options) {
    if (typeof tag !== 'string' || tag === '') {
      return Promise.reject(new TypeError('tag must be a non-empty string'));
    }
    var minInterval = (options && typeof options.minInterval === 'number')
      ? options.minInterval
      : 0;
    this._tags[tag] = { minInterval: minInterval };
    // Native binding hook for shell Phase 1 (OS task scheduler).
    if (typeof _lumen_periodic_sync_register === 'function') {
      _lumen_periodic_sync_register(tag, minInterval);
    }
    return Promise.resolve();
  };

  // unregister(tag) -> Promise<void>
  PeriodicSyncManager.prototype.unregister = function(tag) {
    if (typeof tag !== 'string') {
      return Promise.reject(new TypeError('tag must be a string'));
    }
    delete this._tags[tag];
    if (typeof _lumen_periodic_sync_unregister === 'function') {
      _lumen_periodic_sync_unregister(tag);
    }
    return Promise.resolve();
  };

  // getTags() -> Promise<string[]>
  PeriodicSyncManager.prototype.getTags = function() {
    var tags = Object.keys(this._tags);
    return Promise.resolve(tags);
  };

  // Attach PeriodicSyncManager to ServiceWorkerRegistration.prototype as lazy getter.
  if (typeof ServiceWorkerRegistration !== 'undefined') {
    Object.defineProperty(ServiceWorkerRegistration.prototype, 'periodicSync', {
      get: function() {
        if (!this._periodicSyncManager) {
          this._periodicSyncManager = new PeriodicSyncManager(this);
        }
        return this._periodicSyncManager;
      },
      configurable: true
    });
  }

  globalThis.PeriodicSyncManager = PeriodicSyncManager;
})();"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_periodic_sync(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("var ServiceWorkerRegistration = function() {};")
            .unwrap();
        install_periodic_sync_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn periodic_sync_manager_exists() {
        with_periodic_sync(|rt| {
            let result = rt
                .eval("typeof PeriodicSyncManager === 'function' ? 'exists' : 'missing'")
                .unwrap();
            assert_eq!(result, JsValue::String("exists".to_string()));
        });
    }

    #[test]
    fn register_returns_promise() {
        with_periodic_sync(|rt| {
            let result = rt
                .eval(
                    "var pm = new PeriodicSyncManager({}); \
                     typeof pm.register('news', {minInterval: 86400000}) === 'object' \
                       ? 'promise' : 'not_promise'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("promise".to_string()));
        });
    }

    #[test]
    fn unregister_returns_promise() {
        with_periodic_sync(|rt| {
            let result = rt
                .eval(
                    "var pm = new PeriodicSyncManager({}); \
                     pm.register('news', {minInterval: 3600000}); \
                     typeof pm.unregister('news') === 'object' ? 'promise' : 'not_promise'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("promise".to_string()));
        });
    }

    #[test]
    fn get_tags_returns_promise() {
        with_periodic_sync(|rt| {
            let result = rt
                .eval(
                    "var pm = new PeriodicSyncManager({}); \
                     typeof pm.getTags() === 'object' ? 'promise' : 'not_promise'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("promise".to_string()));
        });
    }
}
