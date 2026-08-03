//! WHATWG Storage Standard §9 — StorageManager API
//!
//! `navigator.storage` singleton exposes:
//! - `estimate()` → `Promise<{usage, quota}>` — byte usage + available quota
//! - `persist()` → `Promise<boolean>` — request persistent storage
//! - `persisted()` → `Promise<boolean>` — query persistent storage status
//! - `getDirectory()` → `Promise<FileSystemDirectoryHandle>` — OPFS root
//!
//! Phase 0: no real disk measurements. `estimate()` returns 0 usage / 10 GiB
//! quota. `persist()` / `persisted()` resolve `true`. `getDirectory()` returns
//! a stub `FileSystemDirectoryHandle` with the OPFS root path. Native bindings
//! `_lumen_storage_estimate()`, `_lumen_storage_persist()`, and
//! `_lumen_storage_get_directory()` are wired for Phase 1 (real OS metrics and
//! sandboxed FS paths).

/// V8 port of the former rquickjs `install_storage_manager_bindings` (Ph3 V8
/// migration S5-S7, rquickjs side removed in S12b-B3): identical JS shim,
/// evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_storage_manager_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(STORAGE_MANAGER_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const STORAGE_MANAGER_SHIM: &str = r#"
(function() {
  'use strict';

  // ── FileSystemDirectoryHandle stub (OPFS root) ─────────────────────────────
  // Full OPFS implementation lives in filesystem_access.rs; this is a minimal
  // Phase 0 object returned by getDirectory() for type-correct API shape.
  function FileSystemDirectoryHandle(name, kind) {
    this.name = name || '';
    this.kind = kind || 'directory';
  }

  // Returns a child handle (Phase 0: stub with given name).
  FileSystemDirectoryHandle.prototype.getDirectoryHandle = function(name) {
    return Promise.resolve(new FileSystemDirectoryHandle(name, 'directory'));
  };
  FileSystemDirectoryHandle.prototype.getFileHandle = function(name) {
    return Promise.resolve({ name: name, kind: 'file' });
  };
  FileSystemDirectoryHandle.prototype.removeEntry = function(_name) {
    return Promise.resolve();
  };
  FileSystemDirectoryHandle.prototype.resolve = function(_possibleDescendant) {
    return Promise.resolve(null);
  };

  if (!window.FileSystemDirectoryHandle) {
    window.FileSystemDirectoryHandle = FileSystemDirectoryHandle;
  }

  // ── StorageManager ─────────────────────────────────────────────────────────

  function StorageManager() {}

  // WHATWG Storage §9.5 — byte usage and available quota.
  // Phase 0: 0 bytes used, 10 GiB available.
  // Phase 1: call _lumen_storage_estimate() for real OS metrics.
  StorageManager.prototype.estimate = function() {
    if (typeof _lumen_storage_estimate === 'function') {
      try { return Promise.resolve(_lumen_storage_estimate()); } catch(_) {}
    }
    return Promise.resolve({ usage: 0, quota: 10 * 1024 * 1024 * 1024 });
  };

  // WHATWG Storage §9.6 — request persistent storage permission.
  // Phase 0: always resolves true (permission granted).
  // Phase 1: call _lumen_storage_persist() and prompt user if needed.
  StorageManager.prototype.persist = function() {
    if (typeof _lumen_storage_persist === 'function') {
      try { return Promise.resolve(_lumen_storage_persist()); } catch(_) {}
    }
    return Promise.resolve(true);
  };

  // WHATWG Storage §9.7 — query whether storage is already persistent.
  // Phase 0: always resolves true.
  StorageManager.prototype.persisted = function() {
    if (typeof _lumen_storage_persisted === 'function') {
      try { return Promise.resolve(_lumen_storage_persisted()); } catch(_) {}
    }
    return Promise.resolve(true);
  };

  // WHATWG Storage §9.8 / OPFS — return the origin's private file system root.
  // Phase 0: stub FileSystemDirectoryHandle with name '' (root).
  // Phase 1: call _lumen_storage_get_directory() → real sandboxed FS path.
  StorageManager.prototype.getDirectory = function() {
    if (typeof _lumen_storage_get_directory === 'function') {
      try { return Promise.resolve(_lumen_storage_get_directory()); } catch(_) {}
    }
    return Promise.resolve(new FileSystemDirectoryHandle('', 'directory'));
  };

  // Install singleton on navigator.
  navigator.storage = new StorageManager();

  // Export class for introspection.
  window.StorageManager = StorageManager;
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_storage_manager(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("var window = globalThis; var navigator = {};").unwrap();
        install_storage_manager_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn storage_manager_exists_on_navigator() {
        with_storage_manager(|rt| {
            let ok = rt
                .eval("typeof navigator.storage === 'object' && navigator.storage !== null")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn storage_manager_class_exported() {
        with_storage_manager(|rt| {
            let ok = rt.eval("typeof window.StorageManager === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn estimate_returns_promise() {
        with_storage_manager(|rt| {
            let ok = rt.eval("navigator.storage.estimate() instanceof Promise").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn storage_manager_has_all_methods() {
        with_storage_manager(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var sm = navigator.storage;
                    typeof sm.estimate === 'function' &&
                    typeof sm.persist === 'function' &&
                    typeof sm.persisted === 'function' &&
                    typeof sm.getDirectory === 'function'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn persist_returns_promise() {
        with_storage_manager(|rt| {
            let ok = rt.eval("navigator.storage.persist() instanceof Promise").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn persisted_returns_promise() {
        with_storage_manager(|rt| {
            let ok = rt.eval("navigator.storage.persisted() instanceof Promise").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn get_directory_returns_promise() {
        with_storage_manager(|rt| {
            let ok = rt.eval("navigator.storage.getDirectory() instanceof Promise").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn file_system_directory_handle_exported() {
        with_storage_manager(|rt| {
            let ok = rt.eval("typeof window.FileSystemDirectoryHandle === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn file_system_directory_handle_constructor() {
        with_storage_manager(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var dir = new FileSystemDirectoryHandle('test', 'directory');
                    dir.name === 'test' && dir.kind === 'directory' &&
                    typeof dir.getDirectoryHandle === 'function' &&
                    typeof dir.getFileHandle === 'function' &&
                    typeof dir.removeEntry === 'function'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn file_system_directory_handle_defaults() {
        with_storage_manager(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var dir = new FileSystemDirectoryHandle();
                    dir.name === '' && dir.kind === 'directory'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
