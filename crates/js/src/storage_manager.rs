//! WHATWG Storage Standard §9 — StorageManager API
//!
//! `navigator.storage` singleton exposes:
//! - `estimate()` → `Promise<{usage, quota}>` — byte usage + available quota
//! - `persist()` → `Promise<boolean>` — request persistent storage
//! - `persisted()` → `Promise<boolean>` — query persistent storage status
//! - `getDirectory()` → `Promise<FileSystemDirectoryHandle>` — OPFS root
//!
//! Phase 0 for the quota half: no real disk measurements. `estimate()` returns
//! 0 usage / 10 GiB quota, `persist()` / `persisted()` resolve `true`. The shim
//! calls `_lumen_storage_estimate()` / `_lumen_storage_persist()` /
//! `_lumen_storage_persisted()` when they exist, but nothing registers them yet
//! — they are extension points for Phase 1 (real OS metrics), not live bindings.
//!
//! `getDirectory()` is real: it hands back a
//! [`crate::filesystem_access`]`::FileSystemDirectoryHandle` over the origin's
//! own directory under `<exe_dir>/data/opfs/`, obtained through the one native
//! this module does register, `_lumen_storage_get_directory()`.
//!
//! # BUG-372 — why this module defines no handle class
//!
//! It used to define its own `FileSystemDirectoryHandle` inside the shim's IIFE
//! and return that from `getDirectory()`. The engine therefore had two classes
//! of that name with *disjoint* method sets — `entries`/`values`/`keys`/
//! `isSameEntry` on the real one, `resolve` on the stub — and which one a page
//! got depended on how it reached the handle. Worse, the stub's methods
//! answered without a file system behind them: `getFileHandle()` resolved an
//! object literal with no `getFile()`, and `removeEntry()` resolved successfully
//! while removing nothing.
//!
//! There is now exactly one such class, owned by `filesystem_access.rs`, and
//! this module never names it: BUG-374 removed its public constructor, so the
//! root handle is built through the private `__lumen_fsa_internal` factory that
//! shim publishes and this one captures (and unpublishes) at eval time. That
//! makes `install_filesystem_access_v8` an ordering requirement of this module —
//! `install_dom` runs it first — and if it is missing, `getDirectory()` rejects
//! with `NotSupportedError` rather than substituting something that looks like a
//! handle.

/// V8 port of the former rquickjs `install_storage_manager_bindings` (Ph3 V8
/// migration S5-S7, rquickjs side removed in S12b-B3): identical JS shim,
/// evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// `origin` is the installing document's origin
/// ([`crate::file_input::origin_for_url`]), captured here in Rust: it selects
/// the OPFS subtree `getDirectory()` opens, so a page must not be able to name
/// it (BUG-371's rule, applied to the sandbox root in BUG-372).
#[cfg(feature = "v8-backend")]
pub(crate) fn install_storage_manager_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    origin: &str,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::into_v8_fn0;
    use lumen_core::ext::JsRuntime as _;

    let root_origin = origin.to_string();
    let get_directory = into_v8_fn0(move || -> Option<String> {
        crate::filesystem_access::opfs_root_entry_json(&root_origin)
    });
    rt.register_native("_lumen_storage_get_directory", get_directory)?;

    rt.eval(STORAGE_MANAGER_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const STORAGE_MANAGER_SHIM: &str = r#"
(function() {
  'use strict';

  // BUG-372: capture the OPFS-root native, then take it off the global object.
  // The file-API sealing pass (file_input::seal_file_natives_v8) has already run
  // by the time this module installs, so this native has to seal itself.
  var NAT_GET_DIRECTORY = (typeof globalThis._lumen_storage_get_directory === 'function')
    ? globalThis._lumen_storage_get_directory
    : null;
  try { delete globalThis._lumen_storage_get_directory; } catch (e) {}

  // BUG-374: `FileSystemDirectoryHandle` has no constructor any more (the spec
  // declares none, and the public one took the internal grant id as an
  // argument), so the OPFS root is built through the same private factory
  // `filesystem_access.rs` uses itself. Captured and unpublished here for the
  // same reason as the native above.
  var FSA_INTERNAL = (typeof globalThis.__lumen_fsa_internal === 'object')
    ? globalThis.__lumen_fsa_internal
    : null;
  try { delete globalThis.__lumen_fsa_internal; } catch (e) {}

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

  // WHATWG Storage §9.8 / OPFS — return the origin's private file system root,
  // as an instance of the one FileSystemDirectoryHandle the engine has.
  StorageManager.prototype.getDirectory = function() {
    return Promise.resolve().then(function() {
      // If filesystem_access.rs did not install, there is no handle class to
      // hand back — reject loudly instead of substituting something that looks
      // like a handle and answers every call with a plausible lie (BUG-372).
      if (!NAT_GET_DIRECTORY || !FSA_INTERNAL) {
        throw new DOMException(
          'The origin private file system is not available', 'NotSupportedError');
      }
      var info = NAT_GET_DIRECTORY();
      if (info == null) {
        throw new DOMException(
          'Could not open the origin private file system', 'NotAllowedError');
      }
      var root = JSON.parse(info);
      return FSA_INTERNAL.makeDirectoryHandle(root.name, root.path_id);
    });
  };

  // Install singleton on navigator.
  navigator.storage = new StorageManager();

  // Export class for introspection.
  window.StorageManager = StorageManager;
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Origin every test here installs under.
    const TEST_ORIGIN: &str = "https://storage.test";

    fn with_storage_manager(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            "var window = globalThis; var navigator = {};\
             function DOMException(message, name) { this.message = message; this.name = name; }",
        )
        .unwrap();
        install_storage_manager_bindings_v8(&rt, TEST_ORIGIN).unwrap();
        f(&rt);
    }

    /// The real two-shim composition, over a mocked OPFS-root native.
    ///
    /// `filesystem_access` installs first (as `install_dom` runs them), so this
    /// module's shim finds the `__lumen_fsa_internal` bridge and the handle it
    /// hands back is the engine's one real `FileSystemDirectoryHandle` — the
    /// whole point of BUG-372, and not something a mocked handle class can show.
    /// Only `_lumen_storage_get_directory` is mocked: the real one would create
    /// the installation's OPFS tree on disk. It goes in before the shim is
    /// evaluated because the shim captures (and deletes) it at eval time.
    fn with_real_fsa(native: &str, f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            "var window = globalThis; var navigator = {}; \
             function Blob(parts, options) {} \
             function _lumen_set_attr(nid, name, val) {} \
             function _lumen_get_attr(nid, name) { return undefined; } \
             function _lumen_dispatch_bubble(nid, type) {} \
             function _lumen_make_element(nid) { return {__nid__: nid}; } \
             window._lumen_make_element = _lumen_make_element;",
        )
        .unwrap();
        rt.eval(crate::v8_runtime::DOM_EXCEPTION_POLYFILL).unwrap();
        crate::file_input::install_file_input_bindings_v8(&rt, TEST_ORIGIN).unwrap();
        crate::filesystem_access::install_filesystem_access_v8(&rt, TEST_ORIGIN).unwrap();
        rt.eval(&format!("globalThis._lumen_storage_get_directory = {native};"))
            .unwrap();
        rt.eval(STORAGE_MANAGER_SHIM).unwrap();
        f(&rt);
    }

    /// Settle `expr` and read the outcome back: V8 drains microtasks at the end
    /// of each `eval`, so the next call sees the result of this one.
    fn settle(rt: &V8JsRuntime, expr: &str) {
        rt.eval(&format!(
            "var __v = null, __e = null; \
             ({expr}).then(function(v) {{ __v = v; }}, function(e) {{ __e = e; }});"
        ))
        .unwrap();
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

    /// BUG-372: the module must not define a second handle class of its own —
    /// the one on `window` is `filesystem_access.rs`'s, or there is none.
    #[test]
    fn defines_no_handle_class_of_its_own() {
        with_storage_manager(|rt| {
            let ok = rt
                .eval("typeof window.FileSystemDirectoryHandle === 'undefined'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    /// BUG-372: `getDirectory()` resolves an instance of the global class, not
    /// of a look-alike private to this shim. BUG-374: it is built through the
    /// private factory, since the class no longer has a public constructor.
    #[test]
    fn get_directory_resolves_the_global_handle_class() {
        with_real_fsa(
            r#"function() { return '{"name":"","path_id":"root-id"}'; }"#,
            |rt| {
                settle(rt, "navigator.storage.getDirectory()");
                let ok = rt
                    .eval(
                        "__e === null && __v instanceof window.FileSystemDirectoryHandle && \
                         __v instanceof window.FileSystemHandle && \
                         __v.name === '' && __v.kind === 'directory' && \
                         Object.getOwnPropertyNames(__v).length === 0",
                    )
                    .unwrap();
                assert_eq!(ok, JsValue::Bool(true));
            },
        );
    }

    /// BUG-374: the handle factory is a private bridge between the two shims —
    /// page script must not be left holding it any more than the natives.
    #[test]
    fn handle_factory_bridge_is_unpublished_after_install() {
        with_real_fsa(
            r#"function() { return '{"name":"","path_id":"root-id"}'; }"#,
            |rt| {
                let ok = rt
                    .eval("typeof globalThis.__lumen_fsa_internal === 'undefined'")
                    .unwrap();
                assert_eq!(ok, JsValue::Bool(true));
            },
        );
    }

    /// BUG-372: no handle class installed → reject. The old shim answered with
    /// its stub instead, which is how a page ended up holding an OPFS root that
    /// silently did nothing.
    #[test]
    fn get_directory_rejects_without_a_handle_class() {
        with_storage_manager(|rt| {
            settle(rt, "navigator.storage.getDirectory()");
            let ok = rt
                .eval("__v === null && __e !== null && __e.name === 'NotSupportedError'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    /// BUG-372: the sandbox root could not be opened → reject, rather than
    /// resolve a handle to nothing.
    #[test]
    fn get_directory_rejects_when_the_root_is_unavailable() {
        with_real_fsa("function() { return null; }", |rt| {
            settle(rt, "navigator.storage.getDirectory()");
            let ok = rt
                .eval("__v === null && __e !== null && __e.name === 'NotAllowedError'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    /// BUG-371's rule for the OPFS root: page script must not be left holding
    /// the native that mints it.
    #[test]
    fn opfs_root_native_is_sealed_after_install() {
        with_storage_manager(|rt| {
            let ok = rt
                .eval("typeof globalThis._lumen_storage_get_directory === 'undefined'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
