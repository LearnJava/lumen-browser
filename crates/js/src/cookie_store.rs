//! Cookie Store API (WHATWG Cookie Store API).
//!
//! Implements the async `cookieStore` global:
//! - `get(name|options)` → Promise<CookieListItem | null>
//! - `getAll(name|options)` → Promise<CookieListItem[]>
//! - `set(name, value | CookieInit)` → Promise<undefined>
//! - `delete(name | CookieDeleteOptions)` → Promise<undefined>
//! - `addEventListener('change', handler)` / `onchange` — `CookieChangeEvent`
//!
//! Phase 0: in-memory cookie store, isolated from `document.cookie` reads.
//! `cookieStore.set()` also writes to `document.cookie` (one-way sync).
//! `CookieStoreManager` on `ServiceWorkerRegistration` — stub (Phase 0).

use rquickjs::Ctx;

/// Install the Cookie Store API into the JS context.
pub fn init_cookie_store(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(COOKIE_STORE_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing WHATWG Cookie Store API (Phase 0).
const COOKIE_STORE_SHIM: &str = r#"(function() {
  // ── CookieChangeEvent ───────────────────────────────────────────────────────
  function CookieChangeEvent(type, init) {
    this.type    = type || 'change';
    this.bubbles = false;
    this.cancelable = false;
    this.changed = (init && init.changed) ? init.changed : [];
    this.deleted = (init && init.deleted) ? init.deleted : [];
  }

  // ── CookieStore ─────────────────────────────────────────────────────────────
  function CookieStore() {
    this._cookies = {};           // name → {name, value, path, domain, expires, secure, sameSite}
    this._listeners = [];         // change event listeners
    this.onchange = null;
  }

  // get(name | {name}) → Promise<CookieListItem | null>
  CookieStore.prototype.get = function(nameOrOptions) {
    var name = typeof nameOrOptions === 'string' ? nameOrOptions
             : (nameOrOptions && nameOrOptions.name) ? nameOrOptions.name
             : null;
    if (name === null) {
      return Promise.reject(new TypeError('cookieStore.get: name required'));
    }
    var entry = this._cookies[name];
    return Promise.resolve(entry ? _makeCookieItem(entry) : null);
  };

  // getAll(name | {name} | undefined) → Promise<CookieListItem[]>
  CookieStore.prototype.getAll = function(nameOrOptions) {
    var self = this;
    var filter = null;
    if (typeof nameOrOptions === 'string') {
      filter = nameOrOptions;
    } else if (nameOrOptions && nameOrOptions.name) {
      filter = nameOrOptions.name;
    }
    var result = [];
    Object.keys(self._cookies).forEach(function(k) {
      if (!filter || k === filter) {
        result.push(_makeCookieItem(self._cookies[k]));
      }
    });
    return Promise.resolve(result);
  };

  // set(name, value) or set({name, value, path?, domain?, expires?, secure?, sameSite?})
  // → Promise<undefined>
  CookieStore.prototype.set = function(nameOrInit, value) {
    var init;
    if (typeof nameOrInit === 'string') {
      if (typeof value !== 'string') {
        return Promise.reject(new TypeError('cookieStore.set: value must be a string'));
      }
      init = { name: nameOrInit, value: value };
    } else if (nameOrInit && typeof nameOrInit === 'object') {
      init = nameOrInit;
      if (typeof init.name !== 'string' || typeof init.value !== 'string') {
        return Promise.reject(new TypeError('cookieStore.set: name and value required'));
      }
    } else {
      return Promise.reject(new TypeError('cookieStore.set: invalid argument'));
    }

    var entry = {
      name:     init.name,
      value:    init.value,
      path:     init.path     || '/',
      domain:   init.domain   || null,
      expires:  init.expires  !== undefined ? init.expires : null,
      secure:   init.secure   === true,
      sameSite: init.sameSite || 'strict',
    };

    var was = this._cookies[entry.name];
    this._cookies[entry.name] = entry;

    // One-way sync to document.cookie (Phase 0)
    if (typeof document !== 'undefined') {
      try {
        var str = encodeURIComponent(entry.name) + '=' + encodeURIComponent(entry.value) + '; path=' + entry.path;
        document.cookie = str;
      } catch(e) { /* ignore */ }
    }

    // Notify native binding (Phase 1: persistence)
    if (typeof _lumen_cookie_store_set === 'function') {
      _lumen_cookie_store_set(entry.name, entry.value, entry.path || '/');
    }

    this._fireChange([_makeCookieItem(entry)], []);
    return Promise.resolve(undefined);
  };

  // delete(name | {name, domain?, path?}) → Promise<undefined>
  CookieStore.prototype.delete = function(nameOrOptions) {
    var name = typeof nameOrOptions === 'string' ? nameOrOptions
             : (nameOrOptions && nameOrOptions.name) ? nameOrOptions.name
             : null;
    if (name === null) {
      return Promise.reject(new TypeError('cookieStore.delete: name required'));
    }

    var was = this._cookies[name];
    if (was) {
      delete this._cookies[name];
      // Also remove from document.cookie
      if (typeof document !== 'undefined') {
        try {
          document.cookie = encodeURIComponent(name) + '=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/';
        } catch(e) { /* ignore */ }
      }
      if (typeof _lumen_cookie_store_delete === 'function') {
        _lumen_cookie_store_delete(name);
      }
      this._fireChange([], [_makeDeletedItem(was)]);
    }
    return Promise.resolve(undefined);
  };

  // addEventListener('change', fn) / removeEventListener
  CookieStore.prototype.addEventListener = function(type, fn) {
    if (type === 'change' && typeof fn === 'function') {
      this._listeners.push(fn);
    }
  };

  CookieStore.prototype.removeEventListener = function(type, fn) {
    if (type !== 'change') return;
    var idx = this._listeners.indexOf(fn);
    if (idx !== -1) this._listeners.splice(idx, 1);
  };

  // dispatchEvent — minimal shim
  CookieStore.prototype.dispatchEvent = function(event) {
    if (event.type === 'change') {
      this._fireChange(event.changed || [], event.deleted || []);
    }
    return true;
  };

  CookieStore.prototype._fireChange = function(changed, deleted) {
    if (!changed.length && !deleted.length) return;
    var evt = new CookieChangeEvent('change', { changed: changed, deleted: deleted });
    if (typeof this.onchange === 'function') {
      try { this.onchange(evt); } catch(e) {}
    }
    var ls = this._listeners.slice();
    for (var i = 0; i < ls.length; i++) {
      try { ls[i](evt); } catch(e) {}
    }
  };

  // ── Helpers ─────────────────────────────────────────────────────────────────
  function _makeCookieItem(entry) {
    return {
      name:     entry.name,
      value:    entry.value,
      path:     entry.path,
      domain:   entry.domain,
      expires:  entry.expires,
      secure:   entry.secure,
      sameSite: entry.sameSite,
    };
  }

  function _makeDeletedItem(entry) {
    return {
      name:   entry.name,
      path:   entry.path,
      domain: entry.domain,
    };
  }

  // ── CookieStoreManager (ServiceWorkerRegistration stub) ─────────────────────
  function CookieStoreManager(registration) {
    this._registration = registration;
    this._subscriptions = [];
  }

  // subscribe(subscriptions) → Promise<undefined>
  CookieStoreManager.prototype.subscribe = function(subscriptions) {
    if (!Array.isArray(subscriptions)) {
      return Promise.reject(new TypeError('subscribe: argument must be an array'));
    }
    this._subscriptions = subscriptions.slice();
    return Promise.resolve(undefined);
  };

  // unsubscribe(subscriptions) → Promise<undefined>
  CookieStoreManager.prototype.unsubscribe = function(subscriptions) {
    return Promise.resolve(undefined);
  };

  // getSubscriptions() → Promise<CookieStoreGetOptions[]>
  CookieStoreManager.prototype.getSubscriptions = function() {
    return Promise.resolve(this._subscriptions.slice());
  };

  // Attach CookieStoreManager to ServiceWorkerRegistration.prototype
  if (typeof ServiceWorkerRegistration !== 'undefined') {
    Object.defineProperty(ServiceWorkerRegistration.prototype, 'cookies', {
      get: function() {
        if (!this._cookieStoreManager) {
          this._cookieStoreManager = new CookieStoreManager(this);
        }
        return this._cookieStoreManager;
      },
      configurable: true,
    });
  }

  // ── Global cookieStore singleton ─────────────────────────────────────────────
  var _cookieStore = new CookieStore();

  globalThis.CookieStore           = CookieStore;
  globalThis.CookieChangeEvent     = CookieChangeEvent;
  globalThis.CookieStoreManager    = CookieStoreManager;
  globalThis.cookieStore           = _cookieStore;
  if (typeof window !== 'undefined') {
    window.CookieStore         = CookieStore;
    window.CookieChangeEvent   = CookieChangeEvent;
    window.CookieStoreManager  = CookieStoreManager;
    window.cookieStore         = _cookieStore;
  }
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
             globalThis._lumen_cookie_store_set = function() {}; \
             globalThis._lumen_cookie_store_delete = function() {};"
        ).expect("install stubs");
    }

    #[test]
    fn cookie_store_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_cookie_store(&ctx).expect("init");
            let r: String = ctx.eval("typeof cookieStore === 'object' ? 'ok' : 'missing'")
                .expect("eval");
            assert_eq!(r, "ok");
        });
    }

    #[test]
    fn cookie_store_get_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_cookie_store(&ctx).expect("init");
            let r: String = ctx.eval(
                "typeof cookieStore.get('x') === 'object' ? 'promise' : 'not_promise'"
            ).expect("eval");
            assert_eq!(r, "promise");
        });
    }

    #[test]
    fn cookie_store_set_and_get_all() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_cookie_store(&ctx).expect("init");
            ctx.eval::<(), _>("cookieStore.set('session', 'abc123');").expect("set");
            // Verify internal store is synchronously updated
            let count: i32 = ctx.eval(
                "Object.keys(cookieStore._cookies).length"
            ).expect("eval");
            assert_eq!(count, 1);
        });
    }

    #[test]
    fn cookie_store_delete_removes_cookie() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_cookie_store(&ctx).expect("init");
            ctx.eval::<(), _>("cookieStore.set('tok', 'xyz');").expect("set");
            ctx.eval::<(), _>("cookieStore.delete('tok');").expect("delete");
            // Verify internal store is synchronously updated
            let count: i32 = ctx.eval(
                "Object.keys(cookieStore._cookies).length"
            ).expect("eval");
            assert_eq!(count, 0);
        });
    }

    #[test]
    fn cookie_change_event_fires_on_set() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_cookie_store(&ctx).expect("init");
            let r: String = ctx.eval(
                "var fired = false; \
                 cookieStore.onchange = function(e) { fired = e.changed.length > 0; }; \
                 cookieStore.set('k', 'v'); \
                 fired ? 'yes' : 'no'"
            ).expect("eval");
            assert_eq!(r, "yes");
        });
    }

    #[test]
    fn cookie_store_manager_on_sw_registration() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_cookie_store(&ctx).expect("init");
            let r: String = ctx.eval(
                "var reg = new ServiceWorkerRegistration(); \
                 typeof reg.cookies === 'object' ? 'ok' : 'missing'"
            ).expect("eval");
            assert_eq!(r, "ok");
        });
    }

    #[test]
    fn cookie_store_get_nonexistent_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_cookie_store(&ctx).expect("init");
            // get on empty store returns a Promise object (null resolves asynchronously)
            let r: String = ctx.eval(
                "typeof cookieStore.get('nonexistent') === 'object' ? 'promise' : 'not_promise'"
            ).expect("eval");
            assert_eq!(r, "promise");
        });
    }

    #[test]
    fn cookie_store_internal_state_after_set() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_cookie_store(&ctx).expect("init");
            ctx.eval::<(), _>("cookieStore.set('foo', 'bar');").expect("set");
            // Value is immediately accessible via internal store
            let v: String = ctx.eval(
                "cookieStore._cookies['foo'] ? cookieStore._cookies['foo'].value : 'missing'"
            ).expect("eval");
            assert_eq!(v, "bar");
        });
    }
}
