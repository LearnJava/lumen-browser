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

use rquickjs::Ctx;

/// Install the Push API stub into the JS context.
///
/// Defines `PushManager` class on ServiceWorkerRegistration.prototype.
/// Must be called **after** worker registration is set up.
pub fn init_push_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(PUSH_API_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C Push API L1 (Phase 0).
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

#[cfg(test)]
mod tests {
    use rquickjs::{Runtime, Context, Ctx};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().expect("Runtime::new");
        let ctx = Context::full(&rt).expect("Context::full");
        (rt, ctx)
    }

    fn install_stubs(ctx: &Ctx) {
        ctx.eval::<(), _>(
            "globalThis.ServiceWorkerRegistration = function() {}; \
             globalThis.TypeError = TypeError; \
             globalThis.ArrayBuffer = ArrayBuffer; \
             globalThis._lumen_push_subscribe = function() {}; \
             globalThis._lumen_push_unsubscribe = function() {};"
        ).expect("install stubs");
    }

    #[test]
    fn test_push_manager_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_push_api(&ctx).expect("init push api");
            let result: String = ctx.eval(
                "typeof PushManager === 'function' ? 'exists' : 'missing'"
            ).expect("eval");
            assert_eq!(result, "exists");
        });
    }

    #[test]
    fn test_push_subscription_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_push_api(&ctx).expect("init push api");
            let result: String = ctx.eval(
                "typeof PushSubscription === 'function' ? 'exists' : 'missing'"
            ).expect("eval");
            assert_eq!(result, "exists");
        });
    }

    #[test]
    fn test_subscribe_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_push_api(&ctx).expect("init push api");
            let result: String = ctx.eval(
                "var pm = new PushManager({}); \
                 typeof pm.subscribe({userVisibleOnly: true}) === 'object' ? 'promise' : 'not_promise'"
            ).expect("eval");
            assert_eq!(result, "promise");
        });
    }

    #[test]
    fn test_get_subscription_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_push_api(&ctx).expect("init push api");
            let result: String = ctx.eval(
                "var pm = new PushManager({}); \
                 typeof pm.getSubscription() === 'object' ? 'promise' : 'not_promise'"
            ).expect("eval");
            assert_eq!(result, "promise");
        });
    }

    #[test]
    fn test_permission_state_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_push_api(&ctx).expect("init push api");
            let result: String = ctx.eval(
                "var pm = new PushManager({}); \
                 typeof pm.permissionState() === 'object' ? 'promise' : 'not_promise'"
            ).expect("eval");
            assert_eq!(result, "promise");
        });
    }

    #[test]
    fn test_push_subscription_get_key() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_push_api(&ctx).expect("init push api");
            let result: String = ctx.eval(
                "var sub = new PushSubscription('https://test', {'p256dh': new ArrayBuffer(65)}); \
                 var key = sub.getKey('p256dh'); \
                 key instanceof ArrayBuffer ? 'buffer' : 'not_buffer'"
            ).expect("eval");
            assert_eq!(result, "buffer");
        });
    }

    #[test]
    fn test_service_worker_registration_has_push_manager() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_push_api(&ctx).expect("init push api");
            let result: String = ctx.eval(
                "var reg = new ServiceWorkerRegistration(); \
                 typeof reg.pushManager === 'object' ? 'yes' : 'no'"
            ).expect("eval");
            assert_eq!(result, "yes");
        });
    }
}
