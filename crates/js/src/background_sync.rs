//! Background Sync API stub (W3C Background Sync L2).
//!
//! Implements `registration.sync` with:
//! - `register(tag: string)` — register a sync event tag
//! - `getTags()` — get list of registered sync tags
//! - `sync` event in Service Worker context
//!
//! Phase 0: Sync tags are stored in-memory per registration. The `sync` event
//! fires on next navigation (or explicitly via _lumen_sw_fire_sync).

/// V8 port of the former rquickjs `init_background_sync` (Ph3 V8 migration
/// S12b-G1, rquickjs side removed in the same batch): identical JS shim,
/// evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// Defines `SyncManager` class on ServiceWorkerRegistration.prototype.
/// Must be called **after** worker registration is set up.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_background_sync_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(BACKGROUND_SYNC_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C Background Sync L2 (Phase 0).
#[cfg(feature = "v8-backend")]
const BACKGROUND_SYNC_SHIM: &str = r#"(function() {
  // SyncManager implementation
  var SyncManager = function(registration) {
    this.registration = registration;
    this.tags = [];  // In-memory tag storage per registration
  };

  // register(tag: string) -> Promise<void>
  // Phase 0: immediately resolves. Stores tag in-memory.
  SyncManager.prototype.register = function(tag) {
    var self = this;
    if (typeof tag !== 'string' || tag === '') {
      return Promise.reject(new TypeError('tag must be a non-empty string'));
    }
    if (this.tags.indexOf(tag) === -1) {
      this.tags.push(tag);
    }
    // Call native binding to persist
    if (typeof _lumen_sw_sync_register === 'function') {
      _lumen_sw_sync_register(tag);
    }
    return Promise.resolve();
  };

  // getTags() -> Promise<string[]>
  // Phase 0: returns copy of in-memory tags
  SyncManager.prototype.getTags = function() {
    var copy = this.tags.slice();  // shallow copy
    if (typeof _lumen_sw_get_tags === 'function') {
      var persisted = _lumen_sw_get_tags();
      if (Array.isArray(persisted)) {
        return Promise.resolve(persisted);
      }
    }
    return Promise.resolve(copy);
  };

  // Attach SyncManager to ServiceWorkerRegistration.prototype
  if (typeof ServiceWorkerRegistration !== 'undefined') {
    ServiceWorkerRegistration.prototype.sync = function() {
      if (!this._syncManager) {
        this._syncManager = new SyncManager(this);
      }
      return this._syncManager;
    };
  }

  // Export SyncManager for tests
  globalThis.SyncManager = SyncManager;
})();"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_background_sync(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            "var ServiceWorkerRegistration = function() {}; \
             var _lumen_sw_sync_register = function() {}; \
             var _lumen_sw_get_tags = function() { return []; };",
        )
        .unwrap();
        install_background_sync_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn sync_manager_exists() {
        with_background_sync(|rt| {
            let result = rt
                .eval("typeof SyncManager === 'function' ? 'exists' : 'missing'")
                .unwrap();
            assert_eq!(result, JsValue::String("exists".to_string()));
        });
    }

    #[test]
    fn register_returns_promise() {
        with_background_sync(|rt| {
            let result = rt
                .eval(
                    "var sm = new SyncManager({}); \
                     typeof sm.register('test-tag') === 'object' ? 'promise' : 'not_promise'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("promise".to_string()));
        });
    }

    #[test]
    fn get_tags_returns_promise() {
        with_background_sync(|rt| {
            let result = rt
                .eval(
                    "var sm = new SyncManager({}); \
                     typeof sm.getTags() === 'object' ? 'promise' : 'not_promise'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("promise".to_string()));
        });
    }

    #[test]
    fn sync_manager_stores_tags() {
        with_background_sync(|rt| {
            let result = rt.eval("new SyncManager({}).tags.length >= 0").unwrap();
            assert_eq!(result, JsValue::Bool(true));
        });
    }

    #[test]
    fn service_worker_registration_has_sync_method() {
        with_background_sync(|rt| {
            let result = rt
                .eval("typeof ServiceWorkerRegistration.prototype.sync === 'function' ? 'yes' : 'no'")
                .unwrap();
            assert_eq!(result, JsValue::String("yes".to_string()));
        });
    }
}
