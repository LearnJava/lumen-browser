//! W3C Storage Buckets API (storage buckets) — Phase 0 in-memory implementation.
//!
//! Installs the async Storage Buckets surface as a JavaScript shim:
//! - `navigator.storageBuckets` (`StorageBucketManager`) with
//!   `open(name, options?)` → Promise<StorageBucket>, `keys()` → Promise<string[]>,
//!   `delete(name)` → Promise<undefined>.
//! - `StorageBucket` with read-only `name`, `persisted()`, `persist()`, `estimate()`,
//!   `durability()`, `setExpires(ms)`, `expires()`, `getDirectory()` and the
//!   `indexedDB` / `caches` accessors (delegating to the global instances).
//!
//! Phase 0: buckets live only in memory for the lifetime of the JS context; quota /
//! persistence are advisory. `getDirectory()` delegates to `navigator.storage` (OPFS)
//! when present, otherwise rejects with an `InvalidStateError` DOMException.

/// V8 port of the former rquickjs `init_storage_buckets` (Ph3 V8 migration
/// S12b-G2, rquickjs side removed in the same batch): identical JS shim,
/// evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_storage_buckets_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(STORAGE_BUCKETS_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the W3C Storage Buckets API (Phase 0, ES5-only).
#[cfg(feature = "v8-backend")]
const STORAGE_BUCKETS_SHIM: &str = r#"(function() {
  // WebIDL: neither StorageBucketManager nor StorageBucket declares a
  // constructor — the manager is only the `navigator.storageBuckets` singleton
  // and buckets only come from `open()`, so `new X()` from a page is an illegal
  // constructor (BUG-681). The shim builds its instances with Object.create.
  function StorageBucketManager() { throw new TypeError('Illegal constructor'); }

  StorageBucketManager.prototype.open = function(name, options) {
    var self = this;
    return new Promise(function(resolve, reject) {
      if (typeof name !== 'string' || name.length === 0 || name.length > 64) {
        reject(new TypeError('Invalid bucket name'));
        return;
      }
      if (!/^[a-z0-9][a-z0-9_-]*$/.test(name)) {
        reject(new TypeError('Invalid bucket name'));
        return;
      }
      if (self._buckets[name]) {
        resolve(self._buckets[name]);
        return;
      }
      var bucket = makeBucket(name, options || {});
      self._buckets[name] = bucket;
      resolve(bucket);
    });
  };

  StorageBucketManager.prototype.keys = function() {
    var self = this;
    return new Promise(function(resolve) {
      var names = Object.keys(self._buckets);
      names.sort();
      resolve(names);
    });
  };

  StorageBucketManager.prototype.delete = function(name) {
    var self = this;
    return new Promise(function(resolve) {
      delete self._buckets[name];
      resolve(undefined);
    });
  };

  function StorageBucket() { throw new TypeError('Illegal constructor'); }

  function makeBucket(name, options) {
    var bucket = Object.create(StorageBucket.prototype);
    bucket._name = name;
    bucket._persisted = options.persisted || false;
    bucket._durability = options.durability || 'relaxed';
    bucket._quota = options.quota || 0;
    bucket._expires = options.expires || null;
    return bucket;
  }

  Object.defineProperty(StorageBucket.prototype, 'name', {
    get: function() { return this._name; }
  });

  StorageBucket.prototype.persisted = function() {
    var self = this;
    return new Promise(function(resolve) {
      resolve(self._persisted);
    });
  };

  StorageBucket.prototype.persist = function() {
    this._persisted = true;
    return Promise.resolve(true);
  };
  StorageBucket.prototype.estimate = function() {
    return Promise.resolve({ usage: 0, quota: this._quota });
  };
  StorageBucket.prototype.durability = function() {
    return Promise.resolve(this._durability);
  };
  StorageBucket.prototype.setExpires = function(ms) {
    this._expires = ms;
    return Promise.resolve(undefined);
  };
  StorageBucket.prototype.expires = function() {
    return Promise.resolve(this._expires);
  };
  StorageBucket.prototype.getDirectory = function() {
    if (typeof navigator !== 'undefined' && navigator.storage && typeof navigator.storage.getDirectory === 'function') {
      return navigator.storage.getDirectory();
    } else {
      return Promise.reject(new DOMException('getDirectory not supported', 'InvalidStateError'));
    }
  };
  Object.defineProperty(StorageBucket.prototype, 'indexedDB', {
    get: function() {
      return (typeof indexedDB !== 'undefined') ? indexedDB : null;
    }
  });
  Object.defineProperty(StorageBucket.prototype, 'caches', {
    get: function() {
      return (typeof caches !== 'undefined') ? caches : null;
    }
  });
  var _manager = Object.create(StorageBucketManager.prototype);
  _manager._buckets = {};
  globalThis.StorageBucketManager = StorageBucketManager;
  globalThis.StorageBucket = StorageBucket;
  if (typeof navigator !== 'undefined') { navigator.storageBuckets = _manager; }
  if (typeof window !== 'undefined') { window.StorageBucketManager = StorageBucketManager; window.StorageBucket = StorageBucket; }
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

    fn with_storage_buckets(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("var window = globalThis; var navigator = {};").unwrap();
        install_storage_buckets_v8(&rt).unwrap();
        f(&rt);
    }

    fn ctor_result(rt: &V8JsRuntime, call: &str) -> JsValue {
        rt.eval(&format!(
            "(function() {{ try {{ {call}; return 'constructed'; }}              catch (e) {{ return e instanceof TypeError ? 'TypeError' : String(e); }} }})()"
        ))
        .unwrap()
    }

    /// BUG-681: WebIDL defines no constructor for either interface, so page
    /// script must not be able to mint a manager or a bucket of its own.
    #[test]
    fn interfaces_are_not_constructible() {
        with_storage_buckets(|rt| {
            let type_error = JsValue::String("TypeError".into());
            assert_eq!(ctor_result(rt, "new StorageBucketManager()"), type_error);
            assert_eq!(ctor_result(rt, "new StorageBucket('x', {})"), type_error);
            assert_eq!(ctor_result(rt, "StorageBucket('x', {})"), type_error);
        });
    }

    /// The engine-made singleton and the buckets `open()` hands out are still
    /// genuine instances of the exported interfaces.
    #[test]
    fn engine_instances_keep_their_interfaces() {
        with_storage_buckets(|rt| {
            let r = rt
                .eval(
                    "var m=navigator.storageBuckets; m.open('pics');                      m instanceof StorageBucketManager &&                      m._buckets['pics'] instanceof StorageBucket &&                      StorageBucketManager.length === 0 && StorageBucket.length === 0",
                )
                .unwrap();
            assert_eq!(r, JsValue::Bool(true));
        });
    }

    #[test]
    fn manager_global_exists() {
        with_storage_buckets(|rt| {
            let r = rt
                .eval("typeof StorageBucketManager === 'function' ? 'ok' : 'no'")
                .unwrap();
            assert_eq!(r, JsValue::String("ok".to_string()));
        });
    }

    #[test]
    fn open_creates_bucket() {
        with_storage_buckets(|rt| {
            let r = rt
                .eval("var m=navigator.storageBuckets; m.open('photos'); Object.keys(m._buckets).length")
                .unwrap();
            assert_eq!(r, JsValue::Number(1.0));
        });
    }

    #[test]
    fn open_returns_promise() {
        with_storage_buckets(|rt| {
            // A native Promise reports `typeof` as "object"; assert it is thenable.
            let r = rt
                .eval(
                    "var p=navigator.storageBuckets.open('x'); \
                     typeof p === 'object' && typeof p.then === 'function' ? 'promise' : 'no'",
                )
                .unwrap();
            assert_eq!(r, JsValue::String("promise".to_string()));
        });
    }

    #[test]
    fn open_rejects_invalid_name() {
        with_storage_buckets(|rt| {
            // Leading hyphen is invalid → reject before inserting into _buckets.
            let r = rt
                .eval("var m=navigator.storageBuckets; m.open('-bad').catch(function(){}); Object.keys(m._buckets).length")
                .unwrap();
            assert_eq!(r, JsValue::Number(0.0));
        });
    }

    #[test]
    fn open_dedupes_same_name() {
        with_storage_buckets(|rt| {
            let r = rt
                .eval("var m=navigator.storageBuckets; m.open('a'); m.open('a'); Object.keys(m._buckets).length")
                .unwrap();
            assert_eq!(r, JsValue::Number(1.0));
        });
    }

    #[test]
    fn bucket_name_readonly() {
        with_storage_buckets(|rt| {
            let r = rt
                .eval("var m=navigator.storageBuckets; m.open('logs'); m._buckets['logs'].name")
                .unwrap();
            assert_eq!(r, JsValue::String("logs".to_string()));
        });
    }

    #[test]
    fn delete_removes_bucket() {
        with_storage_buckets(|rt| {
            let r = rt
                .eval("var m=navigator.storageBuckets; m.open('tmp'); m.delete('tmp'); Object.keys(m._buckets).length")
                .unwrap();
            assert_eq!(r, JsValue::Number(0.0));
        });
    }

    #[test]
    fn bucket_stores_durability() {
        with_storage_buckets(|rt| {
            let r = rt
                .eval("var m=navigator.storageBuckets; m.open('d',{durability:'strict'}); m._buckets['d']._durability")
                .unwrap();
            assert_eq!(r, JsValue::String("strict".to_string()));
        });
    }
}
