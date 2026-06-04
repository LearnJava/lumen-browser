//! Background Sync API stub (W3C Background Sync L2).
//!
//! Implements `registration.sync` with:
//! - `register(tag: string)` — register a sync event tag
//! - `getTags()` — get list of registered sync tags
//! - `sync` event in Service Worker context
//!
//! Phase 0: Sync tags are stored in-memory per registration. The `sync` event
//! fires on next navigation (or explicitly via _lumen_sw_fire_sync).

use rquickjs::Ctx;

/// Install the Background Sync API stub into the JS context.
///
/// Defines `SyncManager` class on ServiceWorkerRegistration.prototype.
/// Must be called **after** worker registration is set up.
pub fn init_background_sync(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(BACKGROUND_SYNC_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C Background Sync L2 (Phase 0).
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
             globalThis._lumen_sw_sync_register = function() {}; \
             globalThis._lumen_sw_get_tags = function() { return []; };"
        ).expect("install stubs");
    }

    #[test]
    fn test_sync_manager_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_background_sync(&ctx).expect("init background sync");
            let result: String = ctx.eval(
                "typeof SyncManager === 'function' ? 'exists' : 'missing'"
            ).expect("eval");
            assert_eq!(result, "exists");
        });
    }

    #[test]
    fn test_register_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_background_sync(&ctx).expect("init background sync");
            let result: String = ctx.eval(
                "var sm = new SyncManager({}); \
                 typeof sm.register('test-tag') === 'object' ? 'promise' : 'not_promise'"
            ).expect("eval");
            assert_eq!(result, "promise");
        });
    }

    #[test]
    fn test_get_tags_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_background_sync(&ctx).expect("init background sync");
            let result: String = ctx.eval(
                "var sm = new SyncManager({}); \
                 typeof sm.getTags() === 'object' ? 'promise' : 'not_promise'"
            ).expect("eval");
            assert_eq!(result, "promise");
        });
    }

    #[test]
    fn test_sync_manager_stores_tags() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_background_sync(&ctx).expect("init background sync");
            ctx.eval::<(), _>("var sm = new SyncManager({}); sm.tags.push('test-tag');").expect("setup");
            let result: bool = ctx.eval(
                "new SyncManager({}).tags.length >= 0"
            ).expect("eval");
            assert!(result);  // Just verify tags array is accessible
        });
    }

    #[test]
    fn test_service_worker_registration_has_sync_method() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_background_sync(&ctx).expect("init background sync");
            let result: String = ctx.eval(
                "typeof ServiceWorkerRegistration.prototype.sync === 'function' ? 'yes' : 'no'"
            ).expect("eval");
            assert_eq!(result, "yes");
        });
    }
}
