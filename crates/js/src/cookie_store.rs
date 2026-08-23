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

/// V8 port of the former rquickjs `init_cookie_store` (Ph3 V8 migration
/// S12b-G5, rquickjs side removed in the same batch): identical JS shim,
/// evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_cookie_store_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(COOKIE_STORE_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing WHATWG Cookie Store API (Phase 0).
#[cfg(feature = "v8-backend")]
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
      try { this.onchange(evt); } catch (e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); }
    }
    var ls = this._listeners.slice();
    for (var i = 0; i < ls.length; i++) {
      try { ls[i](evt); } catch (e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); }
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

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_cookie_store(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            "globalThis.ServiceWorkerRegistration = function() {}; \
             globalThis._lumen_cookie_store_set = function() {}; \
             globalThis._lumen_cookie_store_delete = function() {};",
        )
        .unwrap();
        install_cookie_store_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn cookie_store_exists() {
        with_cookie_store(|rt| {
            let r = rt
                .eval("typeof cookieStore === 'object' ? 'ok' : 'missing'")
                .unwrap();
            assert_eq!(r, JsValue::String("ok".to_string()));
        });
    }

    #[test]
    fn cookie_store_get_returns_promise() {
        with_cookie_store(|rt| {
            let r = rt
                .eval("typeof cookieStore.get('x') === 'object' ? 'promise' : 'not_promise'")
                .unwrap();
            assert_eq!(r, JsValue::String("promise".to_string()));
        });
    }

    #[test]
    fn cookie_store_set_and_get_all() {
        with_cookie_store(|rt| {
            rt.eval("cookieStore.set('session', 'abc123');").unwrap();
            // Verify internal store is synchronously updated
            let count = rt.eval("Object.keys(cookieStore._cookies).length").unwrap();
            assert_eq!(count, JsValue::Number(1.0));
        });
    }

    #[test]
    fn cookie_store_delete_removes_cookie() {
        with_cookie_store(|rt| {
            rt.eval("cookieStore.set('tok', 'xyz');").unwrap();
            rt.eval("cookieStore.delete('tok');").unwrap();
            // Verify internal store is synchronously updated
            let count = rt.eval("Object.keys(cookieStore._cookies).length").unwrap();
            assert_eq!(count, JsValue::Number(0.0));
        });
    }

    #[test]
    fn cookie_change_event_fires_on_set() {
        with_cookie_store(|rt| {
            rt.eval(
                "var fired = false; \
                 cookieStore.onchange = function(e) { fired = e.changed.length > 0; }; \
                 cookieStore.set('k', 'v');",
            )
            .unwrap();
            let r = rt.eval("fired ? 'yes' : 'no'").unwrap();
            assert_eq!(r, JsValue::String("yes".to_string()));
        });
    }

    #[test]
    fn cookie_store_manager_on_sw_registration() {
        with_cookie_store(|rt| {
            let r = rt
                .eval(
                    "var reg = new ServiceWorkerRegistration(); \
                     typeof reg.cookies === 'object' ? 'ok' : 'missing'",
                )
                .unwrap();
            assert_eq!(r, JsValue::String("ok".to_string()));
        });
    }

    #[test]
    fn cookie_store_get_nonexistent_returns_promise() {
        with_cookie_store(|rt| {
            // get on empty store returns a Promise object (null resolves asynchronously)
            let r = rt
                .eval(
                    "typeof cookieStore.get('nonexistent') === 'object' ? 'promise' : 'not_promise'",
                )
                .unwrap();
            assert_eq!(r, JsValue::String("promise".to_string()));
        });
    }

    #[test]
    fn cookie_store_internal_state_after_set() {
        with_cookie_store(|rt| {
            rt.eval("cookieStore.set('foo', 'bar');").unwrap();
            // Value is immediately accessible via internal store
            let v = rt
                .eval("cookieStore._cookies['foo'] ? cookieStore._cookies['foo'].value : 'missing'")
                .unwrap();
            assert_eq!(v, JsValue::String("bar".to_string()));
        });
    }
}
