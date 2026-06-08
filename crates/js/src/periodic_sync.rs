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

use rquickjs::Ctx;

/// Install the Periodic Background Sync API stub into the JS context.
///
/// Defines `PeriodicSyncManager` class on `ServiceWorkerRegistration.prototype`.
/// Must be called after worker registration and Promise are available.
pub fn init_periodic_sync(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(PERIODIC_SYNC_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C Periodic Background Sync (Phase 0).
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
             globalThis.TypeError = TypeError;",
        )
        .expect("install stubs");
    }

    #[test]
    fn test_periodic_sync_manager_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_periodic_sync(&ctx).expect("init");
            let result: String = ctx
                .eval("typeof PeriodicSyncManager === 'function' ? 'exists' : 'missing'")
                .expect("eval");
            assert_eq!(result, "exists");
        });
    }

    #[test]
    fn test_register_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_periodic_sync(&ctx).expect("init");
            let result: String = ctx
                .eval(
                    "var pm = new PeriodicSyncManager({}); \
                     typeof pm.register('news', {minInterval: 86400000}) === 'object' \
                       ? 'promise' : 'not_promise'",
                )
                .expect("eval");
            assert_eq!(result, "promise");
        });
    }

    #[test]
    fn test_unregister_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_periodic_sync(&ctx).expect("init");
            let result: String = ctx
                .eval(
                    "var pm = new PeriodicSyncManager({}); \
                     pm.register('news', {minInterval: 3600000}); \
                     typeof pm.unregister('news') === 'object' ? 'promise' : 'not_promise'",
                )
                .expect("eval");
            assert_eq!(result, "promise");
        });
    }

    #[test]
    fn test_get_tags_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_periodic_sync(&ctx).expect("init");
            let result: String = ctx
                .eval(
                    "var pm = new PeriodicSyncManager({}); \
                     typeof pm.getTags() === 'object' ? 'promise' : 'not_promise'",
                )
                .expect("eval");
            assert_eq!(result, "promise");
        });
    }
}
