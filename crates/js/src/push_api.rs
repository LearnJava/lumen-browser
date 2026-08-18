//! Push API stub (W3C Push API L1).
//!
//! Implements `registration.pushManager` with:
//! - `subscribe(options)` — subscribe to push notifications
//! - `getSubscription()` — get active subscription
//! - `permissionState()` — check permission status
//! - `PushSubscription` with endpoint and getKey() method
//!
//! Phase 0: Push subscriptions are stored in-memory per registration.
//! The actual endpoint is static and placeholder.

/// V8 port of the former rquickjs `init_push_api` (Ph3 V8 migration S12b-G3,
/// rquickjs side removed in the same batch): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// Defines `PushManager` class on ServiceWorkerRegistration.prototype.
/// Must be called **after** worker registration is set up.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_push_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(PUSH_API_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C Push API L1 (Phase 0).
#[cfg(feature = "v8-backend")]
const PUSH_API_SHIM: &str = r#"(function() {
  // PushSubscription implementation
  var PushSubscription = function(endpoint, keys) {
    this.endpoint = endpoint;
    this.expirationTime = null;
    this._keys = keys || {};
  };

  // getKey(name) -> ArrayBuffer | null
  // Phase 0: returns mock keys
  PushSubscription.prototype.getKey = function(name) {
    if (!name || typeof name !== 'string') {
      return null;
    }
    // Return mock ArrayBuffer for p256dh and auth keys
    if (this._keys[name]) {
      return this._keys[name];
    }
    return null;
  };

  // toJSON() -> object
  PushSubscription.prototype.toJSON = function() {
    return {
      endpoint: this.endpoint,
      expirationTime: this.expirationTime,
      keys: this._keys
    };
  };

  // unsubscribe() -> Promise<boolean>
  // Phase 0: immediately resolves with true
  PushSubscription.prototype.unsubscribe = function() {
    var self = this;
    if (typeof _lumen_push_unsubscribe === 'function') {
      _lumen_push_unsubscribe(this.endpoint);
    }
    return Promise.resolve(true);
  };

  // PushManager implementation
  var PushManager = function(registration) {
    this.registration = registration;
    this.subscription = null;  // In-memory subscription storage
  };

  // subscribe(options) -> Promise<PushSubscription>
  // Phase 0: creates static subscription with generated endpoint
  PushManager.prototype.subscribe = function(options) {
    var self = this;
    options = options || {};

    if (!options.userVisibleOnly && options.userVisibleOnly !== undefined) {
      return Promise.reject(new TypeError('userVisibleOnly must be true or omitted'));
    }

    // Validate applicationServerKey if provided
    if (options.applicationServerKey !== undefined &&
        options.applicationServerKey !== null &&
        !(options.applicationServerKey instanceof ArrayBuffer)) {
      return Promise.reject(new TypeError('applicationServerKey must be an ArrayBuffer'));
    }

    // Generate static endpoint (Phase 0)
    var endpoint = 'https://push.lumen.local/v1/subscription/' + Math.random().toString(36).substr(2, 9);

    // Generate mock keys
    var keys = {
      'p256dh': new ArrayBuffer(65),
      'auth': new ArrayBuffer(16)
    };

    // Create subscription
    self.subscription = new PushSubscription(endpoint, keys);

    // Call native binding for registration (Phase 1: persistence)
    if (typeof _lumen_push_subscribe === 'function') {
      _lumen_push_subscribe(endpoint, options.userVisibleOnly !== false);
    }

    return Promise.resolve(self.subscription);
  };

  // getSubscription() -> Promise<PushSubscription | null>
  // Phase 0: returns in-memory subscription or null
  PushManager.prototype.getSubscription = function() {
    var sub = this.subscription;
    return Promise.resolve(sub || null);
  };

  // permissionState() -> Promise<'granted'|'denied'|'prompt'>
  // Phase 0: always returns 'granted'
  PushManager.prototype.permissionState = function() {
    return Promise.resolve('granted');
  };

  // Attach PushManager to ServiceWorkerRegistration.prototype
  if (typeof ServiceWorkerRegistration !== 'undefined') {
    ServiceWorkerRegistration.prototype.pushManager = null;  // Lazy-initialize
    Object.defineProperty(ServiceWorkerRegistration.prototype, 'pushManager', {
      get: function() {
        if (!this._pushManager) {
          this._pushManager = new PushManager(this);
        }
        return this._pushManager;
      },
      configurable: true
    });
  }

  // Export PushSubscription and PushManager for tests
  globalThis.PushSubscription = PushSubscription;
  globalThis.PushManager = PushManager;
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

    fn with_push_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            "var ServiceWorkerRegistration = function() {}; \
             var _lumen_push_subscribe = function() {}; \
             var _lumen_push_unsubscribe = function() {};",
        )
        .unwrap();
        install_push_api_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn test_push_manager_exists() {
        with_push_api(|rt| {
            let result = rt
                .eval("typeof PushManager === 'function' ? 'exists' : 'missing'")
                .unwrap();
            assert_eq!(result, JsValue::String("exists".to_string()));
        });
    }

    #[test]
    fn test_push_subscription_exists() {
        with_push_api(|rt| {
            let result = rt
                .eval("typeof PushSubscription === 'function' ? 'exists' : 'missing'")
                .unwrap();
            assert_eq!(result, JsValue::String("exists".to_string()));
        });
    }

    #[test]
    fn test_subscribe_returns_promise() {
        with_push_api(|rt| {
            let result = rt
                .eval(
                    "var pm = new PushManager({}); \
                     typeof pm.subscribe({userVisibleOnly: true}) === 'object' ? 'promise' : 'not_promise'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("promise".to_string()));
        });
    }

    #[test]
    fn test_get_subscription_returns_promise() {
        with_push_api(|rt| {
            let result = rt
                .eval(
                    "var pm = new PushManager({}); \
                     typeof pm.getSubscription() === 'object' ? 'promise' : 'not_promise'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("promise".to_string()));
        });
    }

    #[test]
    fn test_permission_state_returns_promise() {
        with_push_api(|rt| {
            let result = rt
                .eval(
                    "var pm = new PushManager({}); \
                     typeof pm.permissionState() === 'object' ? 'promise' : 'not_promise'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("promise".to_string()));
        });
    }

    #[test]
    fn test_push_subscription_get_key() {
        with_push_api(|rt| {
            let result = rt
                .eval(
                    "var sub = new PushSubscription('https://test', {'p256dh': new ArrayBuffer(65)}); \
                     var key = sub.getKey('p256dh'); \
                     key instanceof ArrayBuffer ? 'buffer' : 'not_buffer'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("buffer".to_string()));
        });
    }

    #[test]
    fn test_service_worker_registration_has_push_manager() {
        with_push_api(|rt| {
            let result = rt
                .eval(
                    "var reg = new ServiceWorkerRegistration(); \
                     typeof reg.pushManager === 'object' ? 'yes' : 'no'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("yes".to_string()));
        });
    }
}
