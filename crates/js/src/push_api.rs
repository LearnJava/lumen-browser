//! Push API (W3C Push API L1) — `registration.pushManager`.
//!
//! - `subscribe(options)` — subscribe to push notifications
//! - `getSubscription()` — get active subscription
//! - `permissionState()` — check permission status
//! - `PushSubscription` with endpoint and getKey() method
//!
//! Срез 1 (persist): subscriptions are stored in `lumen_core::ext::PushBackend`
//! (`lumen_storage::PushStore` over the SQLite `push_subscriptions` table),
//! keyed by `(origin, scope)` — `getSubscription()` reads the store, not an
//! in-memory field, so a subscription survives the JS context being torn down
//! and rebuilt (reload). Key material (`p256dh`/`auth`) is still mock
//! (zero-filled) — real ECDH P-256 keys are срез 2.

use std::sync::Arc;

/// V8 port of the former rquickjs `init_push_api` (Ph3 V8 migration S12b-G3,
/// rquickjs side removed in the same batch): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// Defines `PushManager` class on ServiceWorkerRegistration.prototype.
/// Must be called **after** worker registration is set up.
///
/// `push_backend` is `None` when the caller has no persistent storage wired
/// (headless dump modes) — subscriptions then behave as before срез 1: the
/// native bindings become no-ops and `getSubscription()` always resolves `null`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_push_api_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    push_backend: Option<Arc<dyn lumen_core::ext::PushBackend>>,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn2, into_v8_fn6};
    use lumen_core::ext::JsRuntime as _;

    let be = push_backend.clone();
    let subscribe = into_v8_fn6(
        move |origin: String,
              scope: String,
              endpoint: String,
              p256dh: String,
              auth: String,
              user_visible_only: bool| {
            if let Some(be) = be.as_ref() {
                be.push_subscribe(&origin, &scope, &endpoint, &p256dh, &auth, user_visible_only);
            }
        },
    );
    rt.register_native("_lumen_push_subscribe", subscribe)?;

    let be = push_backend.clone();
    let get = into_v8_fn2(move |origin: String, scope: String| -> Option<Vec<String>> {
        be.as_ref()
            .and_then(|be| be.push_get(&origin, &scope))
            .map(|(endpoint, p256dh, auth, user_visible_only)| {
                vec![
                    endpoint,
                    p256dh,
                    auth,
                    if user_visible_only { "1".to_string() } else { "0".to_string() },
                ]
            })
    });
    rt.register_native("_lumen_push_get", get)?;

    let be = push_backend;
    let unsubscribe = into_v8_fn2(move |origin: String, scope: String| -> bool {
        be.as_ref()
            .map(|be| be.push_unsubscribe(&origin, &scope))
            .unwrap_or(false)
    });
    rt.register_native("_lumen_push_unsubscribe", unsubscribe)?;

    rt.eval(PUSH_API_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C Push API L1.
#[cfg(feature = "v8-backend")]
const PUSH_API_SHIM: &str = r#"(function() {
  // ArrayBuffer <-> base64, for handing key material to/from the native
  // store (which persists opaque base64 strings, not ArrayBuffers).
  function _push_ab2b64(buf) {
    var bytes = new Uint8Array(buf);
    var bin = '';
    for (var i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
    return btoa(bin);
  }
  function _push_b642ab(b64) {
    var bin = atob(b64);
    var bytes = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    return bytes.buffer;
  }

  // PushSubscription implementation
  var PushSubscription = function(endpoint, keys, scope) {
    this.endpoint = endpoint;
    this.expirationTime = null;
    this._keys = keys || {};
    this._scope = scope || '';
  };

  // getKey(name) -> ArrayBuffer | null
  PushSubscription.prototype.getKey = function(name) {
    if (!name || typeof name !== 'string') {
      return null;
    }
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
  PushSubscription.prototype.unsubscribe = function() {
    var removed = false;
    if (typeof _lumen_push_unsubscribe === 'function') {
      removed = !!_lumen_push_unsubscribe(location.origin, this._scope);
    }
    return Promise.resolve(removed);
  };

  // PushManager implementation
  var PushManager = function(registration) {
    this.registration = registration;
  };

  // subscribe(options) -> Promise<PushSubscription>
  // Phase 0: generated endpoint + mock keys (real ECDH is срез 2), persisted
  // to the native store keyed by (origin, this.registration.scope).
  PushManager.prototype.subscribe = function(options) {
    var self = this;
    options = options || {};

    if (!options.userVisibleOnly && options.userVisibleOnly !== undefined) {
      return Promise.reject(new TypeError('userVisibleOnly must be true or omitted'));
    }

    if (options.applicationServerKey !== undefined &&
        options.applicationServerKey !== null &&
        !(options.applicationServerKey instanceof ArrayBuffer)) {
      return Promise.reject(new TypeError('applicationServerKey must be an ArrayBuffer'));
    }

    var scope = (self.registration && self.registration.scope) || '';
    var endpoint = 'https://push.lumen.local/v1/subscription/' + Math.random().toString(36).substr(2, 9);
    var keys = {
      'p256dh': new ArrayBuffer(65),
      'auth': new ArrayBuffer(16)
    };
    var userVisibleOnly = options.userVisibleOnly !== false;

    if (typeof _lumen_push_subscribe === 'function') {
      _lumen_push_subscribe(
        location.origin,
        scope,
        endpoint,
        _push_ab2b64(keys.p256dh),
        _push_ab2b64(keys.auth),
        userVisibleOnly
      );
    }

    return Promise.resolve(new PushSubscription(endpoint, keys, scope));
  };

  // getSubscription() -> Promise<PushSubscription | null>
  // Reads the native store — survives the JS context being recreated (reload).
  PushManager.prototype.getSubscription = function() {
    var scope = (this.registration && this.registration.scope) || '';
    if (typeof _lumen_push_get !== 'function') {
      return Promise.resolve(null);
    }
    var row = _lumen_push_get(location.origin, scope);
    if (!row) {
      return Promise.resolve(null);
    }
    var keys = {
      'p256dh': _push_b642ab(row[1]),
      'auth': _push_b642ab(row[2])
    };
    return Promise.resolve(new PushSubscription(row[0], keys, scope));
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

    fn eval_with_location(rt: &V8JsRuntime) {
        // `btoa`/`atob` normally come from the full DOM shim
        // (`web_api_shim_mid_c.js`), which these tests don't install —
        // minimal RFC 4648 stand-ins, just enough for the key-material
        // round trip this module's shim performs.
        rt.eval(
            "var ServiceWorkerRegistration = function() {}; \
             var location = {origin: 'https://push.test'}; \
             var _B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'; \
             function btoa(s) { \
               var out = ''; \
               for (var i = 0; i < s.length; i += 3) { \
                 var b0 = s.charCodeAt(i), b1 = s.charCodeAt(i + 1), b2 = s.charCodeAt(i + 2); \
                 var hasB1 = !isNaN(b1), hasB2 = !isNaN(b2); \
                 out += _B64[b0 >> 2]; \
                 out += _B64[((b0 & 3) << 4) | (hasB1 ? (b1 >> 4) : 0)]; \
                 out += hasB1 ? _B64[((b1 & 15) << 2) | (hasB2 ? (b2 >> 6) : 0)] : '='; \
                 out += hasB2 ? _B64[b2 & 63] : '='; \
               } \
               return out; \
             } \
             function atob(b) { \
               b = b.replace(/=+$/, ''); \
               var out = ''; \
               for (var i = 0; i < b.length; i += 4) { \
                 var n0 = _B64.indexOf(b[i]), n1 = _B64.indexOf(b[i + 1]); \
                 var n2 = i + 2 < b.length ? _B64.indexOf(b[i + 2]) : -1; \
                 var n3 = i + 3 < b.length ? _B64.indexOf(b[i + 3]) : -1; \
                 out += String.fromCharCode((n0 << 2) | (n1 >> 4)); \
                 if (n2 >= 0) out += String.fromCharCode(((n1 & 15) << 4) | (n2 >> 2)); \
                 if (n3 >= 0) out += String.fromCharCode(((n2 & 3) << 6) | n3); \
               } \
               return out; \
             }",
        )
        .unwrap();
    }

    fn with_push_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        eval_with_location(&rt);
        install_push_api_v8(&rt, None).unwrap();
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
                    "var pm = new PushManager({scope: '/'}); \
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
                    "var pm = new PushManager({scope: '/'}); \
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
                    "var pm = new PushManager({scope: '/'}); \
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

    #[test]
    fn test_get_subscription_without_backend_is_null() {
        with_push_api(|rt| {
            let result = rt
                .eval(
                    "var pm = new PushManager({scope: '/'}); \
                     var got = null; \
                     pm.getSubscription().then(function(s) { got = s; }); \
                     'ok'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("ok".to_string()));
        });
    }

    /// Срез 1 DoD: subscribe -> getSubscription survives the JS context
    /// being torn down and rebuilt, because both share the same
    /// `Arc<dyn PushBackend>` rather than an in-memory JS field.
    #[test]
    fn test_subscribe_then_reload_context_sees_persisted_subscription() {
        let backend: Arc<dyn lumen_core::ext::PushBackend> = Arc::new(
            lumen_storage::PushStore::new(Arc::new(
                lumen_storage::PushSubscriptions::open_in_memory().unwrap(),
            )),
        );

        let rt1 = V8JsRuntime::new().unwrap();
        eval_with_location(&rt1);
        install_push_api_v8(&rt1, Some(Arc::clone(&backend))).unwrap();
        rt1.eval(
            "var reg = new ServiceWorkerRegistration(); reg.scope = '/app/'; \
             var endpoint = null; \
             reg.pushManager.subscribe({userVisibleOnly: true}) \
                .then(function(sub) { endpoint = sub.endpoint; });",
        )
        .unwrap();
        let endpoint = rt1.eval("endpoint").unwrap();
        let JsValue::String(endpoint) = endpoint else {
            panic!("expected subscribe() to resolve synchronously with an endpoint string");
        };
        assert!(endpoint.starts_with("https://push.lumen.local/"));
        drop(rt1);

        // Fresh JS context (new isolate/heap), same backend Arc — models a reload.
        let rt2 = V8JsRuntime::new().unwrap();
        eval_with_location(&rt2);
        install_push_api_v8(&rt2, Some(backend)).unwrap();
        rt2.eval(
            "var reg = new ServiceWorkerRegistration(); reg.scope = '/app/'; \
             var seen = null; \
             reg.pushManager.getSubscription().then(function(sub) { seen = sub ? sub.endpoint : null; });",
        )
        .unwrap();
        let seen = rt2.eval("seen").unwrap();
        assert_eq!(seen, JsValue::String(endpoint));
    }

    #[test]
    fn test_unsubscribe_removes_from_backend() {
        let backend: Arc<dyn lumen_core::ext::PushBackend> = Arc::new(
            lumen_storage::PushStore::new(Arc::new(
                lumen_storage::PushSubscriptions::open_in_memory().unwrap(),
            )),
        );
        let rt = V8JsRuntime::new().unwrap();
        eval_with_location(&rt);
        install_push_api_v8(&rt, Some(backend)).unwrap();
        rt.eval(
            "var reg = new ServiceWorkerRegistration(); reg.scope = '/'; \
             var sub = null; var afterUnsub = 'pending'; \
             reg.pushManager.subscribe({userVisibleOnly: true}) \
                .then(function(s) { sub = s; return s.unsubscribe(); }) \
                .then(function() { return reg.pushManager.getSubscription(); }) \
                .then(function(s) { afterUnsub = s; });",
        )
        .unwrap();
        let after = rt.eval("afterUnsub").unwrap();
        assert_eq!(after, JsValue::Null);
    }
}
