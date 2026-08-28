//! Состояние, принадлежащее потоку V8, и канал команд к нему.
//!
//! Выделено из `v8_runtime.rs` батчем SPLIT-JS7 без изменений поведения.

use super::*;

// ── Thread-local state ────────────────────────────────────────────────────────

/// V8 isolate + global context, owned exclusively by the JS thread.
///
/// Both `OwnedIsolate` and the `Global<Context>` are `!Send`; they are
/// created in [`v8_thread_main`] and never leave it.
///
/// Fields are dropped in declaration order (Rust spec §8.1).  `isolate` is
/// first so the isolate is disposed before the closures in `native_fn_store`
/// are freed — no dangling-pointer access by V8 during teardown.
pub(super) struct V8Inner {
    /// V8 isolate — disposed first on drop.
    pub(super) isolate: v8::OwnedIsolate,
    /// Persistent handle to the main JS context.
    pub(super) context: v8::Global<v8::Context>,
    /// Keeps compat-layer native closures alive for the isolate's lifetime.
    ///
    /// Each entry is a `Box::into_raw(Box::new(f) as Box<Box<dyn V8NativeFn +
    /// Send>>)` thin pointer.  Freed after `isolate` drops.
    pub(super) native_fn_store: Vec<OwnedNativeFn>,
    /// Keeps scoped natives (Ph3 V8 migration S9 — `crate::v8_compat::V8NativeFnScoped`)
    /// alive for the isolate's lifetime. Twin of `native_fn_store` for natives
    /// that need raw scope/argument access (the WASM host-import bridge).
    pub(super) native_fn_store_scoped: Vec<crate::v8_compat::OwnedNativeFnScoped>,
    /// Own-enumerable global property names present right after context
    /// creation, before any `install_dom`/native registration or page script
    /// runs (Ph3 V8 migration S11 — `suspend`/`resume`). `suspend()` diffs the
    /// live global object against this set so only globals *added later* (by
    /// natives or page scripts) are considered for serialization — ECMAScript
    /// built-ins (`Object`, `Array`, …) are never candidates.
    pub(super) baseline_globals: std::collections::HashSet<String>,
}

// ── Command channel ───────────────────────────────────────────────────────────

/// A unit of work executed on the JS thread against the live [`V8Inner`].
///
/// The caller blocks until the job completes (`rx.recv()`), so even though
/// the box is `'static` (required by `SyncSender`), it may safely capture
/// borrows from the caller's stack for the duration of the call.
pub(super) type V8Job = Box<dyn FnOnce(&mut V8Inner) + Send + 'static>;

/// Messages the shell sends to the dedicated V8 JS thread.
pub(super) enum V8Command {
    /// Run a job against the runtime.
    Run(V8Job),
    /// Shut down the thread and drop the isolate.
    Shutdown,
}

/// Bound for the V8 command queue (same value as `QuickJsRuntime`).
pub(super) const V8_CMD_QUEUE_BOUND: usize = 64;

/// `DOMException` polyfill (Ph3 V8 migration S5-S7). quickjs-ng bundles this as a
/// built-in (`Context::full()`); V8 has no web-platform globals. Mirrors the
/// probed quickjs-ng shape: legacy numeric `code` derived from the WHATWG DOM
/// §4.3 name table, full constant table on the constructor, `instanceof Error`.
///
/// Visible to the crate so a module shim's own tests can stand up the engine's
/// real constructor instead of a hand-written twin: a test that asserts which
/// argument becomes `name` (BUG-373) proves nothing against a stub it wrote
/// itself.
pub(crate) const DOM_EXCEPTION_POLYFILL: &str = r#"(function() {
  if (typeof globalThis.DOMException !== 'undefined') return;
  var LEGACY_CODES = {
    IndexSizeError: 1, DOMStringSizeError: 2, HierarchyRequestError: 3,
    WrongDocumentError: 4, InvalidCharacterError: 5, NoDataAllowedError: 6,
    NoModificationAllowedError: 7, NotFoundError: 8, NotSupportedError: 9,
    InUseAttributeError: 10, InvalidStateError: 11, SyntaxError: 12,
    InvalidModificationError: 13, NamespaceError: 14, InvalidAccessError: 15,
    ValidationError: 16, TypeMismatchError: 17, SecurityError: 18,
    NetworkError: 19, AbortError: 20, URLMismatchError: 21,
    QuotaExceededError: 22, TimeoutError: 23, InvalidNodeTypeError: 24,
    DataCloneError: 25,
  };
  function DOMException(message, name) {
    var err = Error.call(this, message === undefined ? '' : String(message));
    this.message = err.message;
    this.name = name === undefined ? 'Error' : String(name);
    this.code = LEGACY_CODES[this.name] || 0;
    if (Error.captureStackTrace) Error.captureStackTrace(this, DOMException);
  }
  DOMException.prototype = Object.create(Error.prototype);
  DOMException.prototype.constructor = DOMException;
  DOMException.prototype.name = 'Error';
  Object.defineProperty(DOMException, 'name', { value: 'DOMException' });
  // WHATWG DOM §4.3 legacy constant table (numeric codes on the constructor
  // and prototype, e.g. `DOMException.ABORT_ERR === 20`).
  var CONSTANTS = {
    INDEX_SIZE_ERR: 1, DOMSTRING_SIZE_ERR: 2, HIERARCHY_REQUEST_ERR: 3,
    WRONG_DOCUMENT_ERR: 4, INVALID_CHARACTER_ERR: 5, NO_DATA_ALLOWED_ERR: 6,
    NO_MODIFICATION_ALLOWED_ERR: 7, NOT_FOUND_ERR: 8, NOT_SUPPORTED_ERR: 9,
    INUSE_ATTRIBUTE_ERR: 10, INVALID_STATE_ERR: 11, SYNTAX_ERR: 12,
    INVALID_MODIFICATION_ERR: 13, NAMESPACE_ERR: 14, INVALID_ACCESS_ERR: 15,
    VALIDATION_ERR: 16, TYPE_MISMATCH_ERR: 17, SECURITY_ERR: 18,
    NETWORK_ERR: 19, ABORT_ERR: 20, URL_MISMATCH_ERR: 21,
    QUOTA_EXCEEDED_ERR: 22, TIMEOUT_ERR: 23, INVALID_NODE_TYPE_ERR: 24,
    DATA_CLONE_ERR: 25,
  };
  for (var c in CONSTANTS) {
    Object.defineProperty(DOMException, c, { value: CONSTANTS[c], enumerable: true });
    Object.defineProperty(DOMException.prototype, c, { value: CONSTANTS[c], enumerable: true });
  }
  globalThis.DOMException = DOMException;
})();
"#;
