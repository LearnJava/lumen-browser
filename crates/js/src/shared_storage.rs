//! Shared Storage API Phase 0 (WICG Shared Storage).
//!
//! Provides `window.sharedStorage` — a key-value store designed for
//! privacy-preserving cross-site data access (Privacy Sandbox).
//!
//! Unlike `localStorage`, Shared Storage data is *not* directly readable from
//! regular page JS; in the real spec reads are gated behind Shared Storage
//! Worklets. Phase 0 relaxes that restriction so that page code can exercise
//! the full API surface without errors.
//!
//! Phase 0 scope (in-memory, single-origin):
//! - `sharedStorage.set(key, value[, {ignoreIfPresent}])` → `Promise<undefined>`
//! - `sharedStorage.get(key)` → `Promise<string | undefined>`
//! - `sharedStorage.append(key, value)` → `Promise<undefined>`
//! - `sharedStorage.delete(key)` → `Promise<undefined>`
//! - `sharedStorage.clear()` → `Promise<undefined>`
//! - `sharedStorage.keys()` → async iterator of keys
//! - `sharedStorage.values()` → async iterator of values
//! - `sharedStorage.entries()` → async iterator of `[key, value]` pairs
//! - `sharedStorage.length` → `Promise<number>`
//! - `sharedStorage.remainingBudget()` → `Promise<number>` (Phase 0: 12 bits)
//! - `sharedStorage.worklet` → `SharedStorageWorklet` stub (`addModule → resolved`)
//! - `sharedStorage.run(name[, {data}])` → `Promise<undefined>` stub
//! - `sharedStorage.selectURL(name, urls[, {data, resolveToConfig}])` →
//!   `Promise<string>` (first URL or empty)
//!
//! Shell Phase 1: native bindings `_lumen_shared_storage_set` /
//! `_lumen_shared_storage_get` / etc. for SQLite-backed cross-origin isolation.
//! Worklet execution via a dedicated JS realm.

/// V8 port of the former rquickjs `install_shared_storage` (Ph3 V8 migration S5-S7):
/// identical JS shim, evaluated via [`lumen_core::ext::JsRuntime::eval`].
///
/// Must run after the DOM shim so that `window` is available.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_shared_storage_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SHARED_STORAGE_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const SHARED_STORAGE_SHIM: &str = r#"(function(global) {
  'use strict';

  // ── Async Iterator helper ──────────────────────────────────────────────────

  function asyncIter(arr) {
    var i = 0;
    return {
      next: function() {
        if (i < arr.length) { return Promise.resolve({ value: arr[i++], done: false }); }
        return Promise.resolve({ value: undefined, done: true });
      },
      [Symbol.asyncIterator || Symbol.iterator]: function() { return this; }
    };
  }

  // ── SharedStorageWorklet stub ──────────────────────────────────────────────

  /// Stub worklet; Phase 1 will run operations inside an isolated JS realm.
  function SharedStorageWorklet() {}

  SharedStorageWorklet.prototype.addModule = function(_moduleUrl) {
    // Phase 0: no module loading; resolve immediately.
    // Phase 1: _lumen_shared_storage_worklet_add_module(moduleUrl) native binding.
    return Promise.resolve(undefined);
  };

  // ── SharedStorage ──────────────────────────────────────────────────────────

  function SharedStorage() {
    // In-memory store.  Phase 1: backed by SQLite per-origin partition.
    Object.defineProperty(this, '_store', { value: Object.create(null), writable: true, configurable: true });
    Object.defineProperty(this, 'worklet', { value: new SharedStorageWorklet(), enumerable: true, configurable: true });
  }

  // set(key, value[, {ignoreIfPresent}]) → Promise<undefined>
  SharedStorage.prototype.set = function(key, value, opts) {
    key = String(key); value = String(value);
    if (opts && opts.ignoreIfPresent && Object.prototype.hasOwnProperty.call(this._store, key)) {
      return Promise.resolve(undefined);
    }
    // Phase 1 native: _lumen_shared_storage_set(key, value, ignoreIfPresent)
    this._store[key] = value;
    return Promise.resolve(undefined);
  };

  // get(key) → Promise<string | undefined>
  // NOTE: in the real spec, get() is only callable from within a worklet.
  // Phase 0 relaxes this so page code can test the API.
  SharedStorage.prototype.get = function(key) {
    key = String(key);
    // Phase 1 native: _lumen_shared_storage_get(key)
    var val = Object.prototype.hasOwnProperty.call(this._store, key) ? this._store[key] : undefined;
    return Promise.resolve(val);
  };

  // append(key, value) → Promise<undefined>
  SharedStorage.prototype.append = function(key, value) {
    key = String(key); value = String(value);
    // Phase 1 native: _lumen_shared_storage_append(key, value)
    this._store[key] = Object.prototype.hasOwnProperty.call(this._store, key)
      ? this._store[key] + value
      : value;
    return Promise.resolve(undefined);
  };

  // delete(key) → Promise<undefined>
  SharedStorage.prototype.delete = function(key) {
    key = String(key);
    // Phase 1 native: _lumen_shared_storage_delete(key)
    delete this._store[key];
    return Promise.resolve(undefined);
  };

  // clear() → Promise<undefined>
  SharedStorage.prototype.clear = function() {
    // Phase 1 native: _lumen_shared_storage_clear()
    this._store = Object.create(null);
    return Promise.resolve(undefined);
  };

  // keys() → async iterator of string keys
  SharedStorage.prototype.keys = function() {
    return asyncIter(Object.keys(this._store));
  };

  // values() → async iterator of string values
  SharedStorage.prototype.values = function() {
    var store = this._store;
    return asyncIter(Object.keys(store).map(function(k) { return store[k]; }));
  };

  // entries() → async iterator of [key, value] arrays
  SharedStorage.prototype.entries = function() {
    var store = this._store;
    return asyncIter(Object.keys(store).map(function(k) { return [k, store[k]]; }));
  };

  // length → Promise<number>
  Object.defineProperty(SharedStorage.prototype, 'length', {
    get: function() { return Promise.resolve(Object.keys(this._store).length); },
    enumerable: true, configurable: true
  });

  // remainingBudget() → Promise<number>
  // Phase 0: always returns maximum budget (12 bits per WICG spec default).
  // Phase 1: track actual bit budget per origin.
  SharedStorage.prototype.remainingBudget = function() {
    return Promise.resolve(12);
  };

  // run(name[, {data, keepAlive}]) → Promise<undefined>
  // Phase 0: no-op stub; Phase 1: invoke registered worklet operation by name.
  SharedStorage.prototype.run = function(_name, _opts) {
    // Phase 1 native: _lumen_shared_storage_run(name, dataJson)
    return Promise.resolve(undefined);
  };

  // selectURL(name, urls[, {data, resolveToConfig, keepAlive}]) → Promise<string | FencedFrameConfig>
  // Phase 0: returns the first URL (no fenced-frame selection logic).
  // Phase 1: _lumen_shared_storage_select_url(name, urlsJson, dataJson, resolveToConfig)
  SharedStorage.prototype.selectURL = function(_name, urls, _opts) {
    var first = (Array.isArray(urls) && urls.length > 0) ? (urls[0].url || String(urls[0])) : '';
    return Promise.resolve(first);
  };

  // ── Install ────────────────────────────────────────────────────────────────

  var _ss = new SharedStorage();
  global.sharedStorage = _ss;
  if (typeof window !== 'undefined' && window !== global) window.sharedStorage = _ss;

  // Also expose constructor for instanceof checks.
  global.SharedStorage = SharedStorage;
  global.SharedStorageWorklet = SharedStorageWorklet;

})(typeof globalThis !== 'undefined' ? globalThis : this);
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_shared_storage(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        super::install_shared_storage_v8(&rt).unwrap();
        f(&rt);
    }

    // V8 auto-drains microtasks between separate `eval` calls (S12b-B8 finding,
    // matches ua_client_hints.rs), so a `.then()` scheduled in one `eval` has
    // already run by the time the next `eval` reads the global it wrote to.
    fn promise_result(rt: &V8JsRuntime, setup_js: &str, global: &str) -> JsValue {
        rt.eval(setup_js).unwrap();
        rt.eval(global).unwrap()
    }

    #[test]
    fn shared_storage_exists() {
        with_shared_storage(|rt| {
            let ok = rt.eval("typeof sharedStorage !== 'undefined'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn shared_storage_set_returns_promise() {
        with_shared_storage(|rt| {
            let ok = rt.eval("sharedStorage.set('k', 'v') instanceof Promise").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn shared_storage_get_after_set() {
        with_shared_storage(|rt| {
            let val = promise_result(rt,
                "sharedStorage.set('key1','hello'); globalThis.__t='none'; sharedStorage.get('key1').then(function(x){globalThis.__t=x!==undefined?x:'undef';});",
                "globalThis.__t");
            assert_eq!(val, JsValue::String("hello".to_string()));
        });
    }

    #[test]
    fn shared_storage_append() {
        with_shared_storage(|rt| {
            let val = promise_result(rt,
                "sharedStorage.set('k','foo'); sharedStorage.append('k','bar'); globalThis.__t=''; sharedStorage.get('k').then(function(x){globalThis.__t=x||'';});",
                "globalThis.__t");
            assert_eq!(val, JsValue::String("foobar".to_string()));
        });
    }

    #[test]
    fn shared_storage_delete() {
        with_shared_storage(|rt| {
            let val = promise_result(rt,
                "sharedStorage.set('k','v'); sharedStorage.delete('k'); globalThis.__t='found'; sharedStorage.get('k').then(function(x){globalThis.__t=x===undefined?'not-found':'found';});",
                "globalThis.__t");
            assert_eq!(val, JsValue::String("not-found".to_string()));
        });
    }

    #[test]
    fn shared_storage_clear() {
        with_shared_storage(|rt| {
            let len = promise_result(rt,
                "sharedStorage.set('a','1'); sharedStorage.set('b','2'); sharedStorage.clear(); globalThis.__n='-1'; sharedStorage.length.then(function(x){globalThis.__n=String(x);});",
                "globalThis.__n");
            assert_eq!(len, JsValue::String("0".to_string()));
        });
    }

    #[test]
    fn shared_storage_length() {
        with_shared_storage(|rt| {
            let len = promise_result(rt,
                "sharedStorage.clear(); sharedStorage.set('x','1'); sharedStorage.set('y','2'); globalThis.__n='-1'; sharedStorage.length.then(function(x){globalThis.__n=String(x);});",
                "globalThis.__n");
            assert_eq!(len, JsValue::String("2".to_string()));
        });
    }

    #[test]
    fn shared_storage_ignore_if_present() {
        with_shared_storage(|rt| {
            let val = promise_result(rt,
                "sharedStorage.set('k','first'); sharedStorage.set('k','second',{ignoreIfPresent:true}); globalThis.__t=''; sharedStorage.get('k').then(function(x){globalThis.__t=x||'';});",
                "globalThis.__t");
            assert_eq!(val, JsValue::String("first".to_string()));
        });
    }

    #[test]
    fn shared_storage_remaining_budget() {
        with_shared_storage(|rt| {
            let ok = rt.eval("sharedStorage.remainingBudget() instanceof Promise").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
            let budget = promise_result(rt,
                "globalThis.__b='-1'; sharedStorage.remainingBudget().then(function(x){globalThis.__b=String(x);});",
                "globalThis.__b");
            assert_eq!(budget, JsValue::String("12".to_string()));
        });
    }

    #[test]
    fn shared_storage_worklet_exists() {
        with_shared_storage(|rt| {
            let ok = rt.eval("sharedStorage.worklet !== undefined").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
            let ok2 = rt.eval("sharedStorage.worklet.addModule('ops.js') instanceof Promise").unwrap();
            assert_eq!(ok2, JsValue::Bool(true));
        });
    }

    #[test]
    fn shared_storage_run_returns_promise() {
        with_shared_storage(|rt| {
            let ok = rt.eval("sharedStorage.run('myOp') instanceof Promise").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn shared_storage_select_url_returns_first() {
        with_shared_storage(|rt| {
            let url = promise_result(rt,
                "globalThis.__u=''; sharedStorage.selectURL('ad',[{url:'https://a.test/'},{url:'https://b.test/'}]).then(function(x){globalThis.__u=x;});",
                "globalThis.__u");
            assert_eq!(url, JsValue::String("https://a.test/".to_string()));
        });
    }

    #[test]
    fn shared_storage_keys_async_iter() {
        with_shared_storage(|rt| {
            // Verify keys() returns an async iterator (has a .next function),
            // and that two sequential next() calls yield two keys.
            rt.eval(
                "sharedStorage.clear(); sharedStorage.set('aa','1'); sharedStorage.set('bb','2');"
            ).unwrap();
            let has_next = rt.eval("typeof sharedStorage.keys().next === 'function'").unwrap();
            assert_eq!(has_next, JsValue::Bool(true), "keys() should return an async iterator");
            // Two sequential next() calls: collect values via globals, one eval per
            // call so each `.then()` gets a chance to run before the next is read.
            rt.eval(
                "globalThis.__k1=''; globalThis.__k2=''; \
                 var __ki = sharedStorage.keys(); \
                 __ki.next().then(function(r){globalThis.__k1=r.done?'done':r.value;});"
            ).unwrap();
            let k1 = rt.eval("globalThis.__k1").unwrap();
            rt.eval(
                "__ki.next().then(function(r){globalThis.__k2=r.done?'done':r.value;});"
            ).unwrap();
            let k2 = rt.eval("globalThis.__k2").unwrap();
            let k1 = match k1 { JsValue::String(s) => s, other => panic!("expected string, got {other:?}") };
            let k2 = match k2 { JsValue::String(s) => s, other => panic!("expected string, got {other:?}") };
            assert!(!k1.is_empty() && k1 != "done", "first key should be non-empty, got: {k1}");
            assert!(!k2.is_empty() && k2 != "done", "second key should be non-empty, got: {k2}");
        });
    }
}
