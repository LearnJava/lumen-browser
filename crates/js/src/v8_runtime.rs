//! V8-based JS runtime (slices S1–S2).
//!
//! **S1** — runtime skeleton: `V8JsRuntime` handle, `V8Inner` thread-owned
//! state, `v8_thread_main` loop, `impl JsRuntime`.
//!
//! **S2** — compat layer: `native_fn_store` in `V8Inner` keeps registered
//! closures alive; `install_console_natives` proves typed closures register
//! and call back from JS.  See `crate::v8_compat` for the full compat API.
//!
//! Mirrors the `QuickJsRuntime` thread-dispatch pattern: a dedicated OS thread
//! owns the `v8::OwnedIsolate` (which is `!Send`); the handle exposes
//! `JsRuntime` methods that dispatch jobs to that thread via a bounded
//! `SyncSender`. Each job runs to completion before the caller unblocks
//! (blocking `recv`), so borrows of the caller's stack are sound via the
//! same `transmute`-lifetime trick used by `QuickJsRuntime::run`.
//!
//! Feature-gated: compiled only when `v8-backend` is enabled.

use crate::dom::{
    FullscreenRequest, HistoryUrlUpdate, NavAction, NavigateRequest, PopupRequest, PrintRequest,
};
use crate::heap_snapshot;
use crate::v8_compat::{
    OwnedNativeFn, into_v8_fn0, into_v8_fn1, into_v8_fn2, into_v8_fn3, into_v8_fn4, into_v8_fn5,
    into_v8_fn6, register_v8_native,
};
use lumen_core::ext::{AbortToken, JsSseEvent, JsWsEvent};
use lumen_core::url::Url;
use lumen_core::{JsError, JsResult, JsRuntime, JsValue, SuspendedHeap};
use lumen_dom::{
    DocumentMode, DomPosition, Namespace, NodeData, NodeId, QualName, Range as DomRange,
    Selection, ShadowRootMode, node_child_count, node_length, node_text_content, range_text,
};
use lumen_layout::{matches_selector, query_all, query_all_scoped, query_all_within};
use std::collections::{HashMap, HashSet};
use v8::{ValueDeserializerHelper, ValueSerializerHelper};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::sync::{
    Once,
    mpsc::{Sender, SyncSender, sync_channel},
};
use std::thread::JoinHandle;

// ─── DOM helpers (S3) ──────────────────────────────────────────────────────────
//
// Small private duplicates of `dom.rs`'s module-private helpers
// (`find_element_by_tag`, `set_attribute`, `HistoryState`, ...). Kept here
// instead of widening their visibility in `dom.rs`, so the QuickJS code path
// stays untouched apart from the single `WEB_API_SHIM` visibility change.
// Вынесены в подмодули батчем SPLIT-JS5; здесь остаются только объявления.

mod dom_helpers;
mod history_state;

use dom_helpers::*;
use history_state::HistoryState;

// ── JsRuntime impl ────────────────────────────────────────────────────────────
//
// Реализация трейта, отчёт об исключениях, кэш байт-кода и конвертеры значений
// вынесены в подмодули батчем SPLIT-JS5.

mod code_cache;
mod eval;
mod value;

use value::*;

// ── install_dom sections ──────────────────────────────────────────────────────
//
// 41 секция-баннер тела `install_dom` вынесена батчем SPLIT-JS6; здесь остаётся
// сама функция — её преамбула, вызовы помощников и хвост. Почему `reg!` уехал с
// контекстом в параметрах, а не «как есть», — в доккомментарии `install`.

mod install;

// ── Platform initialization ───────────────────────────────────────────────────

/// Process-global V8 platform, initialized exactly once.
static V8_INIT: Once = Once::new();

/// Initialize the V8 platform for this process.
///
/// Safe to call multiple times — subsequent calls are no-ops. All code that
/// creates a `v8::Isolate` (including the smoke test in `v8_smoke.rs`) must
/// call this first so there is exactly one `initialize_platform` call.
pub fn ensure_v8_platform() {
    V8_INIT.call_once(|| {
        let platform = v8::new_default_platform(0, false).make_shared();
        v8::V8::initialize_platform(platform);
        v8::V8::initialize();
    });
}

// ── Window named properties (HTML LS §7.3.3) ──────────────────────────────────

thread_local! {
    /// Document the Window named-property interceptor resolves names against
    /// (BUG-384). Written by every [`V8JsRuntime::install_dom`] from inside its
    /// JS-thread job, so it always points at the document of the page currently
    /// installed in this isolate. `None` before the first install — and forever
    /// in worker isolates, which never call `install_dom` — where the whole
    /// mechanism is simply inert.
    static NAMED_ACCESS_DOC: std::cell::RefCell<Option<Arc<Mutex<lumen_dom::Document>>>> =
        const { std::cell::RefCell::new(None) };
    /// Re-entrancy guard for the interceptor. Building the returned element
    /// wrapper calls back into JS (`_lumen_make_element`), and any global miss
    /// inside that call — including the lookup of `_lumen_make_element` itself
    /// before the shim has been evaluated — would re-enter the interceptor.
    static NAMED_ACCESS_BUSY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Publish `doc` as the document the Window named-property interceptor resolves
/// against (BUG-384). Must be called on the JS thread — the slot is
/// thread-local, matching the isolate's single-thread ownership.
fn set_named_access_document(doc: &Arc<Mutex<lumen_dom::Document>>) {
    NAMED_ACCESS_DOC.with(|slot| *slot.borrow_mut() = Some(Arc::clone(doc)));
}

/// How long a named-property lookup waits for the document lock before giving
/// up (BUG-794). Sized against the contention it exists for: the window `load`
/// event is dispatched through the engine thread (ADR-023), so it runs
/// *concurrently* with the UI thread's own post-load pass over the document,
/// which was measured holding the lock for 3.9 ms — a plain `try_lock` loses
/// that race outright and every name in a `load` handler becomes a
/// `ReferenceError`. The budget is generous against that and still bounded, so
/// the case the `try_lock` was there for — this very thread already holding the
/// lock — costs latency instead of deadlocking.
const NAMED_ACCESS_LOCK_BUDGET: std::time::Duration = std::time::Duration::from_millis(20);

/// Poll interval inside [`NAMED_ACCESS_LOCK_BUDGET`]. Sleeping rather than
/// spinning: the holder is another OS thread doing layout-sized work, so
/// burning this thread's quantum only makes it slower to release.
const NAMED_ACCESS_LOCK_POLL: std::time::Duration = std::time::Duration::from_micros(100);

/// Take the document lock for a named-property lookup, waiting at most
/// [`NAMED_ACCESS_LOCK_BUDGET`] and declining rather than blocking forever.
///
/// `None` means "this name is not a named property of the document" — the only
/// answer an interceptor that cannot look the name up is allowed to give, and
/// the pre-BUG-384 behaviour. A poisoned lock declines for the same reason.
fn lock_document_bounded(
    doc: &Arc<Mutex<lumen_dom::Document>>,
) -> Option<std::sync::MutexGuard<'_, lumen_dom::Document>> {
    let deadline = std::time::Instant::now() + NAMED_ACCESS_LOCK_BUDGET;
    loop {
        match doc.try_lock() {
            Ok(guard) => return Some(guard),
            Err(std::sync::TryLockError::Poisoned(_)) => return None,
            Err(std::sync::TryLockError::WouldBlock) => {
                if std::time::Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(NAMED_ACCESS_LOCK_POLL);
            }
        }
    }
}

/// Resolve `name` against the current document's supported property names
/// (HTML LS §7.3.3): any element whose `id` is `name`, plus `img`/`form`/
/// `iframe`/`embed`/`object` whose `name` attribute is `name`. Returns the
/// first match in tree order as its `NodeId` index, or `None` when the name is
/// not a named property of this document.
///
/// Three deliberate simplifications against the spec, all in the direction of
/// "resolve to something useful instead of throwing `ReferenceError`":
/// several matches yield the first one rather than an `HTMLCollection`; a
/// matching `iframe` yields the element rather than its `contentWindow`; and
/// the lookup is a tree walk per miss rather than a maintained name index.
///
/// Takes the lock through [`lock_document_bounded`] rather than `lock()`: the
/// interceptor fires on *any* global-name miss, including one made by JS that a
/// native called while holding the document lock, and a blocking lock there
/// would deadlock the JS thread against itself.
fn named_access_lookup(name: &str) -> Option<u32> {
    if name.is_empty() {
        return None;
    }
    NAMED_ACCESS_DOC.with(|slot| {
        let borrowed = slot.borrow();
        let doc = lock_document_bounded(borrowed.as_ref()?)?;
        find_first_matching(&doc, doc.root(), &|node| match &node.data {
            NodeData::Element { name: tag, .. } => {
                node.get_attr("id") == Some(name)
                    || (node.get_attr("name") == Some(name)
                        && matches!(
                            tag.local.as_str(),
                            "img" | "form" | "iframe" | "embed" | "object"
                        ))
            }
            _ => false,
        })
        .map(|n| n.index() as u32)
    })
}

/// Build the JS wrapper for node `nid` by calling the shim's own
/// `_lumen_make_element`, so a named-access hit yields the very same object
/// identity `document.getElementById` would (the shim caches wrappers per node).
///
/// Returns `None` — leaving the name unresolved — when the shim has not been
/// evaluated yet or the call throws; an exception is swallowed rather than left
/// pending, because "this name is not a named property" is the only answer an
/// interceptor that declines to intercept is allowed to give.
fn named_access_wrapper<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    nid: u32,
) -> Option<v8::Local<'s, v8::Value>> {
    let ctx = scope.get_current_context();
    let global = ctx.global(scope);
    let key = v8::String::new(scope, "_lumen_make_element")?;
    let factory = v8::Local::<v8::Function>::try_from(global.get(scope, key.into())?).ok()?;
    let arg = v8::Integer::new_from_unsigned(scope, nid).into();
    v8::tc_scope!(tc, scope);
    let wrapper = factory.call(tc, global.into(), &[arg]);
    if tc.has_caught() { None } else { wrapper }
}

/// Global-object template carrying the Window named-properties interceptor
/// (HTML LS §7.3.3, BUG-384) — the object `v8::Context::new` builds the
/// context's global from.
///
/// `NON_MASKING` is what makes the resolution order right without any bookkeeping
/// on our side: V8 consults the interceptor **only** for names that resolve
/// nowhere else, so real `Window` properties and the page's own `var`/`function`
/// declarations keep winning, and a named element is reached only where the
/// alternative was a `ReferenceError`. `ONLY_INTERCEPT_STRINGS` keeps symbol
/// lookups (`Symbol.toStringTag`, `Symbol.unscopables`, …) off the path entirely.
///
/// The interceptor is installed at context-creation time, long before any
/// document exists; [`named_access_lookup`] answers `None` until an
/// `install_dom` publishes one, so the mechanism is inert rather than absent in
/// that window.
fn window_named_properties_template<'s>(
    scope: &v8::PinScope<'s, '_, ()>,
) -> v8::Local<'s, v8::ObjectTemplate> {
    // Getter — resolves the name to an element wrapper, or declines.
    let getter = |scope: &mut v8::PinScope,
                  key: v8::Local<v8::Name>,
                  _args: v8::PropertyCallbackArguments,
                  mut rv: v8::ReturnValue<v8::Value>| {
        if NAMED_ACCESS_BUSY.with(std::cell::Cell::get) {
            return v8::Intercepted::kNo;
        }
        let Ok(key_str) = v8::Local::<v8::String>::try_from(key) else {
            return v8::Intercepted::kNo;
        };
        let Some(nid) = named_access_lookup(&key_str.to_rust_string_lossy(scope)) else {
            return v8::Intercepted::kNo;
        };
        NAMED_ACCESS_BUSY.with(|busy| busy.set(true));
        let wrapper = named_access_wrapper(scope, nid);
        NAMED_ACCESS_BUSY.with(|busy| busy.set(false));
        match wrapper {
            Some(value) => {
                rv.set(value);
                v8::Intercepted::kYes
            }
            None => v8::Intercepted::kNo,
        }
    };
    // Query — the `'x' in window` / `hasOwnProperty` half. Without it V8 would
    // fall back to calling the getter (building a wrapper object just to throw
    // it away) for every existence check.
    let query = |scope: &mut v8::PinScope,
                 key: v8::Local<v8::Name>,
                 _args: v8::PropertyCallbackArguments,
                 mut rv: v8::ReturnValue<v8::Integer>| {
        if NAMED_ACCESS_BUSY.with(std::cell::Cell::get) {
            return v8::Intercepted::kNo;
        }
        let Ok(key_str) = v8::Local::<v8::String>::try_from(key) else {
            return v8::Intercepted::kNo;
        };
        if named_access_lookup(&key_str.to_rust_string_lossy(scope)).is_none() {
            return v8::Intercepted::kNo;
        }
        // WebIDL §3.9 named properties on a global: writable, enumerable and
        // configurable (`[LegacyUnenumerableNamedProperties]` applies to
        // `Document`, not to `Window`) — `PropertyAttribute::NONE`.
        rv.set_int32(v8::PropertyAttribute::NONE.as_u32() as i32);
        v8::Intercepted::kYes
    };
    let template = v8::ObjectTemplate::new(scope);
    template.set_named_property_handler(
        v8::NamedPropertyHandlerConfiguration::new()
            .getter(getter)
            .query(query)
            .flags(
                v8::PropertyHandlerFlags::NON_MASKING
                    | v8::PropertyHandlerFlags::ONLY_INTERCEPT_STRINGS,
            ),
    );
    template
}

// ── Thread-local state ────────────────────────────────────────────────────────

/// V8 isolate + global context, owned exclusively by the JS thread.
///
/// Both `OwnedIsolate` and the `Global<Context>` are `!Send`; they are
/// created in [`v8_thread_main`] and never leave it.
///
/// Fields are dropped in declaration order (Rust spec §8.1).  `isolate` is
/// first so the isolate is disposed before the closures in `native_fn_store`
/// are freed — no dangling-pointer access by V8 during teardown.
struct V8Inner {
    /// V8 isolate — disposed first on drop.
    isolate: v8::OwnedIsolate,
    /// Persistent handle to the main JS context.
    context: v8::Global<v8::Context>,
    /// Keeps compat-layer native closures alive for the isolate's lifetime.
    ///
    /// Each entry is a `Box::into_raw(Box::new(f) as Box<Box<dyn V8NativeFn +
    /// Send>>)` thin pointer.  Freed after `isolate` drops.
    native_fn_store: Vec<OwnedNativeFn>,
    /// Keeps scoped natives (Ph3 V8 migration S9 — `crate::v8_compat::V8NativeFnScoped`)
    /// alive for the isolate's lifetime. Twin of `native_fn_store` for natives
    /// that need raw scope/argument access (the WASM host-import bridge).
    native_fn_store_scoped: Vec<crate::v8_compat::OwnedNativeFnScoped>,
    /// Own-enumerable global property names present right after context
    /// creation, before any `install_dom`/native registration or page script
    /// runs (Ph3 V8 migration S11 — `suspend`/`resume`). `suspend()` diffs the
    /// live global object against this set so only globals *added later* (by
    /// natives or page scripts) are considered for serialization — ECMAScript
    /// built-ins (`Object`, `Array`, …) are never candidates.
    baseline_globals: std::collections::HashSet<String>,
}

// ── Command channel ───────────────────────────────────────────────────────────

/// A unit of work executed on the JS thread against the live [`V8Inner`].
///
/// The caller blocks until the job completes (`rx.recv()`), so even though
/// the box is `'static` (required by `SyncSender`), it may safely capture
/// borrows from the caller's stack for the duration of the call.
type V8Job = Box<dyn FnOnce(&mut V8Inner) + Send + 'static>;

/// Messages the shell sends to the dedicated V8 JS thread.
enum V8Command {
    /// Run a job against the runtime.
    Run(V8Job),
    /// Shut down the thread and drop the isolate.
    Shutdown,
}

/// Bound for the V8 command queue (same value as `QuickJsRuntime`).
const V8_CMD_QUEUE_BOUND: usize = 64;

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

// ── Unhandled promise rejection dispatch (BUG-716) ────────────────────────────
//
// `v8::Isolate::set_promise_reject_callback` fires synchronously, from inside
// V8 internals, as a bare `extern "C" fn` with no closure capture — it cannot
// reach `V8Inner` (not even its `Global<Context>`, see the module docs on
// [`V8Inner::context`]) the way a `V8Command::Run` job can. The only state it
// has is what `PromiseRejectMessage`/`v8::callback_scope!` hands it, so
// per-isolate bookkeeping lives in `thread_local!`s here instead, mirroring
// `NAMED_ACCESS_DOC` above.
//
// Firing `unhandledrejection` immediately from inside the callback would be
// wrong: a `.catch()` attached in the same synchronous turn (extremely common
// — `Promise.reject(x).catch(...)` on two consecutive lines) must cancel it
// (HTML LS §8.1.7.5 step 3). So a rejection is only ever queued here; the
// actual dispatch happens in [`drain_promise_rejections`], called from the V8
// thread loop once the job that produced the rejection has returned.
//
// BUG-918: that boundary is the *job*, not the microtask. "Notify about
// rejected promises" is a step of "perform a microtask checkpoint" (HTML LS
// §8.1.7.3 step 4), i.e. it runs once the microtask queue has drained — so a
// handler attached one `await` later, in the same task, still cancels the
// report. Deferring via `Isolate::enqueue_microtask` (what this used to do)
// puts the flush *into* that queue instead of after it: enqueued from the
// synchronous `Promise.reject`, it runs ahead of the `await` continuation
// that was queued later, and reports a rejection the page does handle. V8's
// `AddMicrotasksCompletedCallback` — the hook Node.js uses here — has no
// binding in `rusty_v8`, but the isolate's auto microtask policy already
// drains the queue before an API call that entered JS returns, so by the time
// a `V8Command::Run` job hands control back the checkpoint is over and the
// pending lists say exactly what HTML LS wants reported.
/// Identity hash + promise + rejection reason, as queued by
/// [`lumen_promise_reject_callback`] for [`PENDING_UNHANDLED`].
type PendingUnhandledEntry = (std::num::NonZeroI32, v8::Global<v8::Promise>, v8::Global<v8::Value>);
/// Identity hash + promise, as kept in [`NOTIFIED_UNHANDLED`]/[`PENDING_HANDLED`].
type NotifiedEntry = (std::num::NonZeroI32, v8::Global<v8::Promise>);

thread_local! {
    /// Promises for which `PromiseRejectWithNoHandler` fired and no matching
    /// `PromiseHandlerAddedAfterReject` has cancelled it yet: identity hash
    /// (for O(n) but allocation-free removal — the list is normally 0-1 long),
    /// the promise itself, and the rejection reason captured at reject time
    /// (`PromiseRejectMessage::get_value()` is only valid for *this* event;
    /// `Promise::result` cannot be substituted at flush time because nothing
    /// guarantees the promise is even still reachable from the reason side).
    static PENDING_UNHANDLED: std::cell::RefCell<Vec<PendingUnhandledEntry>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// Promises already flushed as `unhandledrejection`, kept so a late
    /// `PromiseHandlerAddedAfterReject` can be recognised as "handled after
    /// all" and answered with `rejectionhandled` instead of silently ignored.
    static NOTIFIED_UNHANDLED: std::cell::RefCell<Vec<NotifiedEntry>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// Promises moved out of `NOTIFIED_UNHANDLED` by a late handler, waiting
    /// for the next flush to fire `rejectionhandled`.
    static PENDING_HANDLED: std::cell::RefCell<Vec<NotifiedEntry>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Register [`lumen_promise_reject_callback`] on `isolate`. Called once per
/// isolate from the V8 thread bootstrap, alongside
/// [`crate::v8_esm::install_dynamic_import_hook`].
fn install_promise_reject_hook(isolate: &mut v8::Isolate) {
    isolate.set_promise_reject_callback(lumen_promise_reject_callback);
}

/// `v8::PromiseRejectCallback` — see the section docs above for why the
/// dispatch itself is deferred rather than done here.
extern "C" fn lumen_promise_reject_callback(msg: v8::PromiseRejectMessage) {
    // SAFETY: called by V8 with a valid `PromiseRejectMessage` for the isolate
    // currently executing; `callback_scope!` is the documented way to recover
    // a `HandleScope` from it (see the diagnostic snippet in BUG-716 itself).
    v8::callback_scope!(unsafe scope, &msg);
    v8::scope!(let scope, scope);
    let promise = msg.get_promise();
    let hash = promise.get_identity_hash();
    match msg.get_event() {
        v8::PromiseRejectEvent::PromiseRejectWithNoHandler => {
            let reason = msg.get_value().unwrap_or_else(|| v8::undefined(scope).into());
            let already_pending = PENDING_UNHANDLED.with(|p| p.borrow().iter().any(|(h, ..)| *h == hash));
            if already_pending {
                return;
            }
            PENDING_UNHANDLED.with(|p| {
                p.borrow_mut()
                    .push((hash, v8::Global::new(scope, promise), v8::Global::new(scope, reason)));
            });
        }
        v8::PromiseRejectEvent::PromiseHandlerAddedAfterReject => {
            let was_pending = PENDING_UNHANDLED.with(|p| {
                let mut p = p.borrow_mut();
                let before = p.len();
                p.retain(|(h, ..)| *h != hash);
                p.len() != before
            });
            if was_pending {
                // Handler arrived before this rejection was ever flushed —
                // per spec, no event at all.
                return;
            }
            let was_notified = NOTIFIED_UNHANDLED.with(|n| {
                let mut n = n.borrow_mut();
                if let Some(idx) = n.iter().position(|(h, _)| *h == hash) {
                    let (_, g) = n.remove(idx);
                    Some(g)
                } else {
                    None
                }
            });
            if let Some(promise_global) = was_notified {
                PENDING_HANDLED.with(|p| p.borrow_mut().push((hash, promise_global)));
            }
        }
        // V8-internal double-settle diagnostics, not part of the HTML LS
        // §8.1.7.5 unhandledrejection/rejectionhandled contract — ignored.
        v8::PromiseRejectEvent::PromiseRejectAfterResolved
        | v8::PromiseRejectEvent::PromiseResolveAfterResolved => {}
    }
}

/// True when either pending queue has something for the next flush. Checked
/// before a scope is built, so a job that rejected nothing pays one
/// thread-local read rather than a `HandleScope` + `ContextScope`.
fn have_pending_rejections() -> bool {
    PENDING_UNHANDLED.with(|p| !p.borrow().is_empty())
        || PENDING_HANDLED.with(|p| !p.borrow().is_empty())
}

/// Report everything queued by [`lumen_promise_reject_callback`] since the
/// last call, from the V8 thread loop — the end of a job, which is this
/// engine's "perform a microtask checkpoint" boundary (see the section docs
/// above for why the microtask queue itself is too fine a boundary, BUG-918).
///
/// The loop exists because [`flush_promise_rejections`] calls back into the
/// page, which may reject a promise of its own; each such round is a fresh
/// checkpoint, and the bound only keeps a page that rejects from inside its
/// own `unhandledrejection` handler from spinning the thread forever — the
/// leftovers then go out after the next job, exactly as an unbounded queue
/// would have delivered them anyway.
fn drain_promise_rejections(inner: &mut V8Inner) {
    /// Flush rounds one job may pay for. Two is already the rare case (a
    /// handler that rejects); this is a runaway backstop, not a budget.
    const MAX_ROUNDS: u32 = 8;
    for _ in 0..MAX_ROUNDS {
        if !have_pending_rejections() {
            return;
        }
        // Disjoint field borrows, as in `with_tc!` below.
        let isolate = &mut inner.isolate;
        let context_global = &inner.context;
        v8::scope!(let scope, isolate);
        let ctx = v8::Local::new(scope, context_global);
        let scope = &mut v8::ContextScope::new(scope, ctx);
        flush_promise_rejections(scope);
    }
}

/// Drains both queues and, for each entry, calls the shim's
/// `_lumen_dispatch_unhandled_rejection(type, promise, reason)`
/// (`crate::dom::WEB_API_SHIM`) directly — with the live `Local<Value>`s, not
/// through `eval`/JSON, so an `Error` reason keeps its class and `.stack` and
/// `PromiseRejectionEvent.promise` is the actual settled promise.
///
/// A lookup failure (shim not installed yet — e.g. a promise rejected by the
/// isolate's bootstrap script before any page has loaded) is not an error:
/// there is nothing to notify and nothing to clean up, the entries are simply
/// dropped.
fn flush_promise_rejections(scope: &mut v8::PinScope) {
    let unhandled = PENDING_UNHANDLED.with(|p| std::mem::take(&mut *p.borrow_mut()));
    let handled = PENDING_HANDLED.with(|p| std::mem::take(&mut *p.borrow_mut()));
    if unhandled.is_empty() && handled.is_empty() {
        return;
    }
    let ctx = scope.get_current_context();
    let global = ctx.global(scope);
    let Some(key) = v8::String::new(scope, "_lumen_dispatch_unhandled_rejection") else {
        return;
    };
    let Some(dispatch_fn) = global
        .get(scope, key.into())
        .and_then(|v| v8::Local::<v8::Function>::try_from(v).ok())
    else {
        return;
    };

    for (hash, promise_global, reason_global) in unhandled {
        let promise = v8::Local::new(scope, &promise_global);
        let reason = v8::Local::new(scope, &reason_global);
        let Some(type_str) = v8::String::new(scope, "unhandledrejection") else {
            continue;
        };
        v8::tc_scope!(tc, scope);
        let default_prevented = dispatch_fn
            .call(tc, global.into(), &[type_str.into(), promise.into(), reason])
            .map(|v| v.boolean_value(tc))
            .unwrap_or(false);
        if !default_prevented {
            // Diagnostic value proven during BUG-703 (see the bug's own
            // notes): a page whose async bootstrap swallows everything is
            // otherwise silent on stderr right up to the point it hangs.
            eprintln!(
                "[unhandled-rejection] {}",
                reason.to_rust_string_lossy(tc)
            );
        }
        NOTIFIED_UNHANDLED.with(|n| n.borrow_mut().push((hash, promise_global)));
    }

    for (_, promise_global) in handled {
        let promise = v8::Local::new(scope, &promise_global);
        if promise.state() != v8::PromiseState::Rejected {
            continue;
        }
        let reason = promise.result(scope);
        let Some(type_str) = v8::String::new(scope, "rejectionhandled") else {
            continue;
        };
        v8::tc_scope!(tc, scope);
        let _ = dispatch_fn.call(tc, global.into(), &[type_str.into(), promise.into(), reason]);
    }
}

// ── Thread entry point ────────────────────────────────────────────────────────

/// Entry point of the dedicated V8 thread.
///
/// Initialises the V8 platform (idempotent), creates the isolate and context,
/// signals the caller via `init_tx`, then services [`V8Command`]s until the
/// channel closes or [`V8Command::Shutdown`] arrives.
fn v8_thread_main(
    cmd_rx: std::sync::mpsc::Receiver<V8Command>,
    init_tx: Sender<Result<(), JsError>>,
) {
    ensure_v8_platform();

    let mut isolate = v8::Isolate::new(Default::default());
    // S12b-23: dynamic `import()` is resolved by an isolate-wide host hook
    // (static imports go through the callback passed to `instantiate_module`).
    crate::v8_esm::install_dynamic_import_hook(&mut isolate);
    // BUG-716: unhandledrejection/rejectionhandled dispatch, also isolate-wide.
    install_promise_reject_hook(&mut isolate);
    // Create the context inside a short-lived HandleScope so the scope's borrow
    // of `isolate` ends before we move `isolate` into `V8Inner`.
    let (context, baseline_globals) = {
        // scope! pins the HandleScope and gives scope: &mut PinnedRef<HandleScope<'_, ()>>
        v8::scope!(let scope, &mut isolate);
        let ctx = v8::Context::new(
            scope,
            v8::ContextOptions {
                global_template: Some(window_named_properties_template(scope)),
                ..Default::default()
            },
        );
        // Snapshot the bare context's own global keys (S11) before entering it
        // for anything else — this is the baseline `suspend()` diffs against.
        let baseline = {
            let ctx_scope = &mut v8::ContextScope::new(scope, ctx);
            let global = ctx.global(ctx_scope);
            let mut names = std::collections::HashSet::new();
            if let Some(own_props) = global.get_own_property_names(ctx_scope, Default::default())
            {
                for i in 0..own_props.length() {
                    if let Some(key) = own_props.get_index(ctx_scope, i)
                        && let Some(s) = key.to_string(ctx_scope)
                    {
                        names.insert(s.to_rust_string_lossy(ctx_scope));
                    }
                }
            }
            names
        };
        // scope deref-coerces to &Isolate via PinnedRef<HandleScope<'_,()>> → Isolate
        (v8::Global::new(scope, ctx), baseline)
    };

    let mut inner = V8Inner {
        isolate,
        context,
        native_fn_store: Vec::new(),
        native_fn_store_scoped: Vec::new(),
        baseline_globals,
    };
    let _ = init_tx.send(Ok(()));

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            V8Command::Run(job) => {
                job(&mut inner);
                // BUG-918: end of the job = end of the microtask checkpoint,
                // which is where HTML LS §8.1.7.3 step 4 notifies about
                // rejected promises. Every JS entry point on this runtime
                // funnels through `V8Command::Run`, including the ones with
                // no event loop behind them (`--dump-*`, SVG rasterization,
                // unit tests), so the report stays visible there too.
                drain_promise_rejections(&mut inner);
            }
            V8Command::Shutdown => break,
        }
    }
    // Free WASM import `v8::Global` GC roots on this thread while the isolate
    // is still alive (mirrors QuickJS's `wasm::clear_registry()` discipline at
    // `lib.rs:447` — see BUG-222). `Global::drop` no-ops safely on an already
    // disposed isolate, but releasing the persistent handle here is the
    // correct, leak-free order.
    crate::wasm::v8_bridge::clear_registry();
    // Same discipline for the ESM module map's `v8::Global<v8::Module>` roots
    // (S12b-23): release them here, while the isolate is still alive.
    crate::v8_esm::reset();
    // `inner` (OwnedIsolate + Global<Context>) drops here, on its owning thread.
}

// ── Public handle ─────────────────────────────────────────────────────────────

/// BUG-341 S7 — outcome of draining the page-side DOM-mutation tracker since
/// the last [`V8JsRuntime::take_dom_touched`] call.
///
/// Feeds [`lumen_layout::style::restyle_root_set_for_node_change`] so the
/// ADR-016 M4 page pipeline (`Lumen::try_relayout_raf_incremental`) can take
/// the incremental-cascade path (`layout_mutation_incremental_restyle`)
/// instead of a full cascade for JS DOM mutations, mirroring
/// `lumen_chrome::bind_model_tracked` (BUG-341 S6) on the chrome side.
#[derive(Debug, Default, Clone)]
pub struct DomTouched {
    /// Nodes whose selector-relevant attribute/class/style value or child
    /// list actually changed via a tracked primitive (`setAttribute`,
    /// `removeAttribute`, `className`/`classList`, inline `style`,
    /// `appendChild`/`removeChild`/`insertBefore`, `textContent`/`innerHTML`).
    pub nodes: HashSet<NodeId>,
    /// `true` when a mutation happened this cycle through a primitive whose
    /// effect on which nodes' selector-relevant state changed cannot be
    /// attributed precisely (`execCommand`, contenteditable text editing,
    /// Selection-driven range deletes, Shadow DOM attachment). When `true`,
    /// `nodes` alone is **not** a safe restyle root-set — the caller must
    /// fall back to a full cascade for this cycle.
    pub unattributed: bool,
}

/// Per-node snapshot of resolved CSS custom properties: node id → the map of
/// `--name` → computed value that node declares or inherits.
///
/// The inner map is shared behind an `Arc` because custom properties inherit —
/// one `:root` declaration is one allocation for every node under it (BUG-732).
/// Produced by `lumen_layout::collect_custom_properties`, consumed by
/// [`V8JsRuntime::update_custom_properties`].
pub type CustomPropertySnapshot = HashMap<u32, Arc<HashMap<String, String>>>;

/// V8-backed JS runtime implementing [`JsRuntime`].
///
/// The isolate lives on a dedicated thread; methods block until the dispatched
/// job completes. Cheap to clone via `Arc` if shared access is needed (but
/// callers typically hold one runtime per tab).
pub struct V8JsRuntime {
    /// Channel to the JS thread.
    cmd_tx: SyncSender<V8Command>,
    /// Join handle taken in `Drop` after sending `Shutdown`.
    js_thread: Option<JoinHandle<()>>,
    /// Navigation request written by JS via `location.href=`, `location.assign()` etc.
    /// Captured inside `install_dom`; read by [`Self::take_navigate_request`].
    nav_out: Arc<Mutex<Option<crate::dom::NavigateRequest>>>,
    /// Next timer wakeup deadline as Unix epoch ms (set by `_lumen_request_wakeup`).
    /// `take_timer_wakeup` atomically clears after reading.
    timer_wakeup: Arc<Mutex<Option<f64>>>,
    /// Set to `true` by any DOM-mutating JS binding. Cleared by `take_dom_dirty`.
    dom_dirty: Arc<AtomicBool>,
    /// BUG-341 S7: nodes touched by a tracked DOM-mutation primitive since the
    /// last [`Self::take_dom_touched`] call, plus the `unattributed` fallback
    /// flag. See [`DomTouched`].
    dom_touched: Arc<Mutex<DomTouched>>,
    /// Set to `true` when JS calls `requestAnimationFrame(fn)`.
    raf_pending: Arc<AtomicBool>,
    /// Layout bounding rects updated after each relayout by the shell.
    /// Maps `NodeId` index (u32) → `[x, y, width, height]` in viewport-relative CSS px.
    layout_rects: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    /// Current viewport size `[width, height]` in CSS px.
    viewport_size: Arc<Mutex<[f32; 2]>>,
    /// Lazy image load requests queued by `_lumen_request_lazy_image_load` from JS.
    lazy_img_requests: Arc<Mutex<Vec<(u32, String)>>>,
    /// Scroll state per scroll-container node, updated after each relayout.
    scroll_states: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    /// Pending scroll requests queued by JS via `_lumen_request_scroll`.
    pending_scrolls: Arc<Mutex<Vec<(u32, f32, f32)>>>,
    /// Pending page-level scroll requests from JS `window.scrollTo/scrollBy`.
    pending_page_scrolls: Arc<Mutex<Vec<(f32, bool)>>>,
    /// Current page scroll Y exposed to JS `window.scrollY` / `window.pageYOffset`.
    page_scroll_y: Arc<Mutex<f32>>,
    /// BUG-822: `true` when the page moved on a rendering update whose scroll
    /// sequence was still in flight, so a `scrollend` is still owed once it
    /// settles. Lives next to [`Self::page_scroll_y`] for the same reason it
    /// does — the debt belongs to the document, not to the shell.
    page_scroll_end_pending: Arc<Mutex<bool>>,
    /// Computed CSS styles per node, updated after each relayout by the shell.
    computed_styles: Arc<Mutex<HashMap<u32, HashMap<String, String>>>>,
    /// Resolved CSS custom properties per node (keys carry their `--` prefix),
    /// updated after each relayout by the shell alongside [`Self::computed_styles`].
    /// Kept in a separate map behind an `Arc` because custom properties inherit:
    /// every node under a `:root`-declared set shares one allocation instead of
    /// carrying its own copy of every variable (BUG-732).
    custom_properties: Arc<Mutex<CustomPropertySnapshot>>,
    /// Pending popup window requests queued by JS `window.open()`.
    window_open_requests: Arc<Mutex<Vec<crate::dom::PopupRequest>>>,
    /// Console messages queued by `console.log/warn/error` calls in JS.
    console_messages: Arc<Mutex<Vec<(u8, String)>>>,
    /// `history.pushState` / `history.replaceState` URL-update notifications.
    pending_history_url_updates: Arc<Mutex<Vec<crate::dom::HistoryUrlUpdate>>>,
    /// `history.go(n)` / `back` / `forward` traversal deltas.
    pending_history_traversals: Arc<Mutex<Vec<i32>>>,
    /// Shell-backed Navigation API state (serialised JSON of nav history + index).
    nav_state: Arc<Mutex<String>>,
    /// Queued by `_lumen_navigation_request`; drained by the shell.
    pending_navigation_updates: Arc<Mutex<Vec<crate::dom::NavUpdate>>>,
    /// Queued by `_lumen_navigation_report_intercept` during `NavigateEvent` dispatch.
    pending_nav_intercepted: Arc<Mutex<Vec<(bool, bool)>>>,
    /// Fullscreen requests emitted by `element.requestFullscreen()` / `document.exitFullscreen()`.
    fullscreen_requests: Arc<Mutex<Vec<crate::dom::FullscreenRequest>>>,
    /// CSS View Transitions L1 events emitted by `document.startViewTransition` (Ph3
    /// V8 migration S12b-G5). Mirrors [`crate::QuickJsRuntime`]'s field of the same
    /// name; drained by the shell in `about_to_wait` via `take_view_transition_events()`.
    view_transition_events: Arc<Mutex<Vec<crate::view_transitions::ViewTransitionEvent>>>,
    /// Print requests emitted by `window.print()`.
    print_requests: Arc<Mutex<Vec<crate::dom::PrintRequest>>>,
    /// Focus requests queued by JS via `_lumen_request_focus` / `_lumen_request_blur`.
    pending_focus_requests: Arc<Mutex<Vec<Option<u32>>>>,
    /// Node ID of the current pointer capture target (W3C Pointer Events L3 §4.1),
    /// set via `_lumen_set_capture_state`/`_lumen_release_capture_state`.
    /// Mirrors [`crate::QuickJsRuntime`]'s field of the same name.
    pointer_capture_nid: Arc<Mutex<Option<u32>>>,
    /// Deterministic render mode (8F): when `true`, `Date.now()`/`Math.random` are frozen/seeded.
    deterministic: AtomicBool,
    /// DEVX-16: `--rng-seed` override for deterministic mode's `Math.random`
    /// seed. `None` means derive the seed from the page URL hash (previous,
    /// still-default behaviour).
    deterministic_rng_seed: Mutex<Option<u64>>,
    /// BUG-480 срез 8: ключ собственного документа в ящиках моста (указатель
    /// Arc, 0 = контекст без документа — до install_dom). Дублирует
    /// `frame_docs.self_key` для чтения без захвата реестра: по нему
    /// [`Self::frame_transport_pending`] отвечает на вопрос шелла «есть ли
    /// неразобранные конверты для МЕНЯ».
    self_doc_key: AtomicUsize,
    /// DEVX-16: `--monotonic-clock` — when `true` (and `deterministic` is also
    /// `true`), `Date.now()`/`performance.now()` advance [`Self::deterministic_clock_ms`]
    /// by 1 ms per call instead of staying frozen at 0.
    deterministic_monotonic: AtomicBool,
    /// Shared counter backing `deterministic_monotonic`'s clock advance, reset
    /// to 0 by [`Self::set_deterministic_mode`].
    deterministic_clock_ms: Arc<AtomicU64>,
    /// Live SW execution threads keyed by `(origin, scope)`.
    sw_worker_store: Option<lumen_core::ext::SwWorkerStore>,
    /// `sessionStorage` partition of the browsing context this runtime serves
    /// (BUG-836). Session storage is scoped to the *tab*, not the document, so
    /// the owner of the tab hands the same `Arc` to every document's runtime
    /// via [`Self::with_session_storage`]; `None` (tests, headless) means a
    /// fresh, document-local store — which is what every document used to get.
    ss_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    /// `BroadcastChannel` instances created on this page (WHATWG HTML §9.5).
    /// Mirrors [`crate::QuickJsRuntime`]'s field of the same name.
    broadcast_channels: crate::broadcast_channel::BroadcastRegistry,
    /// Pending OS notification requests queued by `new Notification(...)` in JS.
    /// Mirrors [`crate::QuickJsRuntime`]'s field of the same name.
    pending_notifications: crate::notifications_bindings::NotificationQueue,
    /// Live dedicated-`Worker` threads spawned by this page (Ph3 V8 migration
    /// S10). Mirrors [`crate::QuickJsRuntime`]'s `workers` field.
    workers: crate::worker::WorkerRegistry,
    /// Outbound queue drained by [`Self::pump_workers`]. Mirrors
    /// [`crate::QuickJsRuntime`]'s `worker_messages` field.
    worker_messages: crate::worker::WorkerMessageQueue,
    /// Outbound uncaught-exception report queue drained by [`Self::pump_workers`]
    /// (BUG-591 worker parent-side reporting) — parallel to `worker_messages`
    /// but for `Worker`'s `error` event rather than `message`.
    worker_errors: crate::worker::WorkerErrorQueue,
    /// Next `Worker` id to assign. Mirrors [`crate::QuickJsRuntime`]'s
    /// `worker_next_id` field.
    worker_next_id: Arc<Mutex<u32>>,
    /// Blob URL → script text, mirrored from `URL.createObjectURL` for
    /// `importScripts()`. Mirrors [`crate::QuickJsRuntime`]'s
    /// `worker_blob_store` field.
    worker_blob_store: crate::worker::WorkerBlobStore,
    /// Outbound queue for this page's `SharedWorker` client ports, drained by
    /// [`Self::pump_shared_workers`]. Mirrors [`crate::QuickJsRuntime`]'s
    /// `shared_worker_outbox` field.
    shared_worker_outbox: crate::shared_worker::SharedWorkerOutbox,
    /// Outbound uncaught-exception report queue for this page's `SharedWorker`
    /// instances, drained by [`Self::pump_shared_workers`] (BUG-591
    /// SharedWorker parent-side reporting) — parallel to
    /// `shared_worker_outbox` but for the `error` event rather than `message`.
    shared_worker_errors: crate::worker::WorkerErrorQueue,
    /// Cookie-banner auto-dismiss (7C.3) enable flag (Ph3 V8 migration S12b-G6,
    /// BUG-548). Defaults to `true`. Shell sets this from the user's
    /// `cookie_banner_dismiss` preference via [`Self::set_cookie_banner_dismiss`].
    /// Mirrors [`crate::QuickJsRuntime`]'s field of the same name.
    cookie_banner_dismiss: AtomicBool,
    /// BUG-480 срез 2: реестр под-документов `<iframe>` («хост → Document»),
    /// общий с нативами [`crate::frame_bridge`]. Наполняется
    /// [`Self::register_frame_document`] после загрузки каждого фрейма.
    frame_docs: crate::frame_bridge::FrameDocRegistry,
}

/// Process-global `navigator.userAgent` override (WebDriver BiDi
/// `emulation.setUserAgentOverride`, BUG-295). `None` = the WEB_API_SHIM
/// default (`Lumen/<version>`).
///
/// A process-global rather than a `V8JsRuntime` field: the shell constructs a
/// **fresh** `V8JsRuntime` on every navigation (`run_scripts_with_dom`,
/// `bfcache_thaw`, …) — there is no single long-lived instance to carry a
/// per-session override on across navigations. Lumen also runs one JS
/// runtime at a time per process, so a process-global reads identically to a
/// "session-level" BiDi override in practice (mirrors `lumen_network`'s
/// `GLOBAL_UA_OVERRIDE`/`GLOBAL_OFFLINE` statics, same rationale).
/// Разложить плоский `[name, value, name, value, …]` из JS в пары.
///
/// Формат «плоский массив» выбран потому, что мост натива понимает
/// `Vec<String>`, но не массив массивов; непарный хвост (нечётная длина —
/// шим такого не строит) отбрасывается, а не превращается в заголовок с
/// пустым значением.
fn pairs_from_flat(flat: Vec<String>) -> Vec<(String, String)> {
    let mut it = flat.into_iter();
    let mut out = Vec::with_capacity(it.len() / 2);
    while let (Some(name), Some(value)) = (it.next(), it.next()) {
        out.push((name, value));
    }
    out
}

static GLOBAL_UA_OVERRIDE: Mutex<Option<String>> = Mutex::new(None);

/// Set (or clear with `None`) the process-global `navigator.userAgent` override.
/// Consulted by every subsequent `install_dom` call (new navigation); does
/// **not** retroactively affect an already-loaded page — see
/// [`V8JsRuntime::eval`] for re-injecting into the current page.
pub fn set_global_user_agent_override(ua: Option<String>) {
    if let Ok(mut guard) = GLOBAL_UA_OVERRIDE.lock() {
        *guard = ua;
    }
}

/// The active `navigator.userAgent` override, if any.
fn global_user_agent_override() -> Option<String> {
    GLOBAL_UA_OVERRIDE.lock().ok().and_then(|g| g.clone())
}

/// Build the JS snippet that redefines `navigator.userAgent` to `ua`
/// (BUG-295). `navigator` is a plain object literal in `WEB_API_SHIM`
/// (writable, configurable `userAgent` property), so a direct assignment is
/// enough — no `Object.defineProperty` needed. Shared between `install_dom`
/// (next-navigation application) and the shell's immediate-apply path (the
/// already-loaded page), so both go through the same escaping.
pub fn user_agent_override_script(ua: &str) -> String {
    let escaped = ua.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
    format!("navigator.userAgent = \"{escaped}\";")
}

/// Process-global `Intl`/`Date` timezone override (WebDriver BiDi
/// `browser.setTimezoneOverride`, BUG-295). `None` = host timezone.
///
/// Same process-global rationale as [`GLOBAL_UA_OVERRIDE`] (fresh
/// `V8JsRuntime` per navigation, one runtime per process).
static GLOBAL_TIMEZONE_OVERRIDE: Mutex<Option<String>> = Mutex::new(None);

/// Set (or clear with `None`) the process-global IANA timezone override.
/// Consulted by every subsequent `install_dom` call (new navigation); does
/// **not** retroactively affect an already-loaded page — see
/// [`timezone_override_script`] for re-injecting into the current page.
pub fn set_global_timezone_override(timezone_id: Option<String>) {
    if let Ok(mut guard) = GLOBAL_TIMEZONE_OVERRIDE.lock() {
        *guard = timezone_id;
    }
}

/// The active timezone override, if any.
fn global_timezone_override() -> Option<String> {
    GLOBAL_TIMEZONE_OVERRIDE.lock().ok().and_then(|g| g.clone())
}

/// Build the JS snippet that sets the global timezone-override marker
/// (`globalThis.__lumen_timezone_override`) and, the first time it runs on a
/// given context, wraps `Intl.DateTimeFormat` so a construction without an
/// explicit `options.timeZone` picks up the marker (BUG-295).
///
/// Two `Intl` surfaces exist in this codebase (`crate::intl_bindings`'s
/// pure-JS ECMA-402 shim, active when the `v8` crate build lacks ICU i18n
/// data, defers to a native `Intl` otherwise) — the wrapper here covers
/// **both**: on a build with a native `Intl.DateTimeFormat` (the common
/// case; V8's bundled ICU already has full IANA tzdata, so an override like
/// `"Pacific/Kiritimati"` resolves and formats correctly, no offset table
/// needed on the Rust side) it wraps that constructor directly; the shim
/// path additionally reads the same marker itself
/// (`intl_bindings.rs::DateTimeFormat`'s `this._tz` line) so the wrap here
/// is redundant-but-harmless there. The wrap is idempotent
/// (`Intl.DateTimeFormat.__lumenPatched`) and reads the marker dynamically
/// on each call — re-running just this script (no navigation) to change the
/// override doesn't need a re-wrap, only the assignment line takes effect.
///
/// Explicit `options.timeZone` from calling JS always wins (spec behaviour
/// — a caller who names a zone should get exactly that zone, not a
/// session-wide emulation override); only the *default* (no `timeZone` key)
/// case is redirected.
pub fn timezone_override_script(timezone_id: &str) -> String {
    let escaped = timezone_id.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
    format!(
        r#"(function() {{
  globalThis.__lumen_timezone_override = "{escaped}";
  if (typeof Intl !== 'undefined' && Intl.DateTimeFormat && !Intl.DateTimeFormat.__lumenPatched) {{
    var _Orig = Intl.DateTimeFormat;
    function LumenDateTimeFormat(locales, options) {{
      var opts = options ? Object.assign({{}}, options) : {{}};
      if (!('timeZone' in opts) && globalThis.__lumen_timezone_override) {{
        opts.timeZone = globalThis.__lumen_timezone_override;
      }}
      if (!(this instanceof LumenDateTimeFormat)) return new LumenDateTimeFormat(locales, opts);
      return new _Orig(locales, opts);
    }}
    LumenDateTimeFormat.prototype = _Orig.prototype;
    LumenDateTimeFormat.__lumenPatched = true;
    if (_Orig.supportedLocalesOf) {{
      LumenDateTimeFormat.supportedLocalesOf = _Orig.supportedLocalesOf.bind(_Orig);
    }}
    Intl.DateTimeFormat = LumenDateTimeFormat;
  }}
}})();"#
    )
}

impl V8JsRuntime {
    /// Create a new V8 runtime on a dedicated thread.
    pub fn new() -> Result<Self, JsError> {
        let (cmd_tx, cmd_rx) = sync_channel::<V8Command>(V8_CMD_QUEUE_BOUND);
        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), JsError>>();
        let js_thread = std::thread::Builder::new()
            .name("lumen-v8".to_string())
            .spawn(move || v8_thread_main(cmd_rx, init_tx))
            .map_err(|e| JsError::Runtime(format!("spawn V8 thread: {e}")))?;
        match init_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(JsError::Runtime("V8 thread died during init".into())),
        }
        Ok(Self {
            cmd_tx,
            js_thread: Some(js_thread),
            nav_out: Arc::new(Mutex::new(None)),
            timer_wakeup: Arc::new(Mutex::new(None)),
            dom_dirty: Arc::new(AtomicBool::new(false)),
            dom_touched: Arc::new(Mutex::new(DomTouched::default())),
            raf_pending: Arc::new(AtomicBool::new(false)),
            layout_rects: Arc::new(Mutex::new(HashMap::new())),
            viewport_size: Arc::new(Mutex::new([0.0, 0.0])),
            lazy_img_requests: Arc::new(Mutex::new(Vec::new())),
            scroll_states: Arc::new(Mutex::new(HashMap::new())),
            pending_scrolls: Arc::new(Mutex::new(Vec::new())),
            pending_page_scrolls: Arc::new(Mutex::new(Vec::new())),
            page_scroll_y: Arc::new(Mutex::new(0.0)),
            page_scroll_end_pending: Arc::new(Mutex::new(false)),
            computed_styles: Arc::new(Mutex::new(HashMap::new())),
            custom_properties: Arc::new(Mutex::new(HashMap::new())),
            window_open_requests: Arc::new(Mutex::new(Vec::new())),
            console_messages: Arc::new(Mutex::new(Vec::new())),
            pending_history_url_updates: Arc::new(Mutex::new(Vec::new())),
            pending_history_traversals: Arc::new(Mutex::new(Vec::new())),
            nav_state: Arc::new(Mutex::new(String::from(r#"{"entries":[],"index":0}"#))),
            pending_navigation_updates: Arc::new(Mutex::new(Vec::new())),
            pending_nav_intercepted: Arc::new(Mutex::new(Vec::new())),
            fullscreen_requests: Arc::new(Mutex::new(Vec::new())),
            view_transition_events: Arc::new(Mutex::new(Vec::new())),
            print_requests: Arc::new(Mutex::new(Vec::new())),
            pending_focus_requests: Arc::new(Mutex::new(Vec::new())),
            pointer_capture_nid: Arc::new(Mutex::new(None)),
            deterministic: AtomicBool::new(false),
            deterministic_rng_seed: Mutex::new(None),
            self_doc_key: AtomicUsize::new(0),
            deterministic_monotonic: AtomicBool::new(false),
            deterministic_clock_ms: Arc::new(AtomicU64::new(0)),
            sw_worker_store: None,
            ss_store: None,
            broadcast_channels: Arc::new(Mutex::new(Vec::new())),
            pending_notifications: Arc::new(Mutex::new(Vec::new())),
            workers: Arc::new(Mutex::new(HashMap::new())),
            worker_messages: Arc::new(Mutex::new(Vec::new())),
            worker_errors: Arc::new(Mutex::new(Vec::new())),
            worker_next_id: Arc::new(Mutex::new(0)),
            worker_blob_store: Arc::new(Mutex::new(HashMap::new())),
            shared_worker_outbox: Arc::new(Mutex::new(Vec::new())),
            shared_worker_errors: Arc::new(Mutex::new(Vec::new())),
            cookie_banner_dismiss: AtomicBool::new(true),
            frame_docs: Arc::new(Mutex::new(crate::frame_bridge::FrameDocSlots::default())),
        })
    }

    /// Set whether the cookie-banner auto-dismiss shim (7C.3) is injected on
    /// the next `install_dom`. Mirrors [`crate::QuickJsRuntime::set_cookie_banner_dismiss`].
    pub fn set_cookie_banner_dismiss(&self, enabled: bool) {
        self.cookie_banner_dismiss.store(enabled, Ordering::Relaxed);
    }

    /// Shared handle to this runtime's `BroadcastChannel` registry, for the
    /// natives registered by [`crate::broadcast_channel::install_broadcast_channel_bindings_v8`].
    pub(crate) fn broadcast_registry(&self) -> crate::broadcast_channel::BroadcastRegistry {
        Arc::clone(&self.broadcast_channels)
    }

    /// Deliver messages posted to this page's `BroadcastChannel` instances.
    /// Mirrors [`crate::QuickJsRuntime::pump_broadcast_channels`].
    pub fn pump_broadcast_channels(&self) {
        let messages = crate::broadcast_channel::drain(&self.broadcast_channels);
        if messages.is_empty() {
            return;
        }
        let json = crate::build_worker_messages_json(&messages);
        let script = format!(
            "if(typeof _lumen_deliver_broadcast_messages==='function')\
             _lumen_deliver_broadcast_messages({json})"
        );
        let _ = self.eval(&script);
    }

    /// Deliver messages posted by worker threads to their `Worker` JS
    /// instances (Ph3 V8 migration S10). Mirrors
    /// [`crate::QuickJsRuntime::pump_workers`].
    pub fn pump_workers(&self) {
        let messages = crate::worker::drain_messages(&self.worker_messages);
        if !messages.is_empty() {
            let json = crate::build_worker_messages_json(&messages);
            let script = format!(
                "if(typeof _lumen_deliver_worker_messages==='function')\
                 _lumen_deliver_worker_messages({json})"
            );
            let _ = self.eval(&script);
        }

        // BUG-591 worker parent-side reporting: deliver uncaught-exception
        // reports (top-level script failure, or a message/timer callback
        // throw) as `Worker`'s `error` event.
        let errors = crate::worker::drain_errors(&self.worker_errors);
        if !errors.is_empty() {
            let json = crate::build_worker_messages_json(&errors);
            let script = format!(
                "if(typeof _lumen_deliver_worker_errors==='function')\
                 _lumen_deliver_worker_errors({json})"
            );
            let _ = self.eval(&script);
        }
    }

    /// Deliver messages posted by `SharedWorker` threads to this page's
    /// ports (Ph3 V8 migration S10). Mirrors
    /// [`crate::QuickJsRuntime::pump_shared_workers`].
    pub fn pump_shared_workers(&self) {
        let messages = crate::shared_worker::drain_messages(&self.shared_worker_outbox);
        if !messages.is_empty() {
            let json = crate::build_worker_messages_json(&messages);
            let script = format!(
                "if(typeof _lumen_deliver_shared_worker_messages==='function')\
                 _lumen_deliver_shared_worker_messages({json})"
            );
            let _ = self.eval(&script);
        }

        // BUG-591 SharedWorker parent-side reporting: deliver uncaught-exception
        // reports broadcast from the shared-worker thread as `SharedWorker`'s
        // `error` event.
        let errors = crate::worker::drain_errors(&self.shared_worker_errors);
        if !errors.is_empty() {
            let json = crate::build_worker_messages_json(&errors);
            let script = format!(
                "if(typeof _lumen_deliver_shared_worker_errors==='function')\
                 _lumen_deliver_shared_worker_errors({json})"
            );
            let _ = self.eval(&script);
        }
    }

    /// Shared handle to this runtime's pending-notifications queue, for the
    /// natives registered by [`crate::notifications_bindings::install_notifications_bindings_v8`].
    pub(crate) fn notification_queue(&self) -> crate::notifications_bindings::NotificationQueue {
        Arc::clone(&self.pending_notifications)
    }

    /// Drain all OS notification requests queued by `new Notification(...)` in JS.
    /// Mirrors [`crate::QuickJsRuntime::take_notification_requests`].
    pub fn take_notification_requests(
        &self,
    ) -> Vec<crate::notifications_bindings::NotificationRequest> {
        crate::notifications_bindings::drain_notifications(&self.pending_notifications)
    }

    /// Consume any navigation request that JS placed via `location.href =` etc.
    /// Returns `None` if no navigation was requested during script execution.
    pub fn take_navigate_request(&self) -> Option<crate::dom::NavigateRequest> {
        self.nav_out
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    /// Take the next timer wakeup as Unix epoch ms, clearing the stored value.
    /// Returns `None` when no timers are pending.
    pub fn take_timer_wakeup(&self) -> Option<f64> {
        self.timer_wakeup
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    /// Drain all print requests queued by `window.print()`.
    pub fn take_print_requests(&self) -> Vec<crate::dom::PrintRequest> {
        std::mem::take(
            &mut *self
                .print_requests
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        )
    }

    /// BUG-480 срез 8: есть ли в ящиках моста неразобранные конверты,
    /// адресованные ЭТОМУ контексту. Шелл опрашивает живые рантаймы на каждом
    /// `about_to_wait` и, пока хоть кто-то отвечает «да», держит короткий poll-
    /// дедлайн — иначе конверт после затихания страницы лежит до случайного
    /// пробуждения цикла. До install_dom (ключ 0) всегда `false`.
    pub fn frame_transport_pending(&self) -> bool {
        let key = self.self_doc_key.load(Ordering::Relaxed);
        crate::frame_bridge::frame_transport_has_for((key != 0).then_some(key))
    }

    /// Enable or disable deterministic render mode (8F) before calling `install_dom`.
    ///
    /// `rng_seed` (DEVX-16, `--rng-seed`): overrides the URL-hash-derived
    /// `Math.random` seed when `Some`; ignored when `on` is `false`.
    /// `monotonic_clock` (DEVX-16, `--monotonic-clock`): when `true`,
    /// `Date.now()`/`performance.now()` advance by 1 ms per call instead of
    /// staying frozen at 0; also ignored when `on` is `false`.
    pub fn set_deterministic_mode(&self, on: bool, rng_seed: Option<u64>, monotonic_clock: bool) {
        self.deterministic.store(on, Ordering::Relaxed);
        *self
            .deterministic_rng_seed
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = rng_seed;
        self.deterministic_monotonic
            .store(monotonic_clock, Ordering::Relaxed);
        self.deterministic_clock_ms.store(0, Ordering::Relaxed);
    }

    /// Attach a `SwWorkerStore` so that `_lumen_sw_activate_script` can spawn and
    /// register SW execution threads when pages activate a Service Worker.
    ///
    /// Must be called before `install_dom` to take effect (mirrors
    /// [`crate::QuickJsRuntime::with_sw_worker_store`]).
    pub fn with_sw_worker_store(mut self, store: lumen_core::ext::SwWorkerStore) -> Self {
        self.sw_worker_store = Some(store);
        self
    }

    /// Attach the browsing context's `sessionStorage` partition (BUG-836).
    ///
    /// HTML LS §12.2 binds session storage to the browsing context, so the tab —
    /// not the document — owns the store and hands the same `Arc` to every
    /// document it loads. Without this the runtime builds its own empty store in
    /// `install_dom`, and everything written by the previous document is lost on
    /// navigation. Must be called before `install_dom` to take effect (mirrors
    /// [`Self::with_sw_worker_store`]).
    pub fn with_session_storage(mut self, store: Arc<Mutex<lumen_core::WebStorage>>) -> Self {
        self.ss_store = Some(store);
        self
    }

    /// Returns `true` if JS mutated the DOM since the last call, clearing the flag.
    /// Mirrors [`crate::QuickJsRuntime::take_dom_dirty`].
    pub fn take_dom_dirty(&self) -> bool {
        self.dom_dirty.swap(false, Ordering::Relaxed)
    }

    /// BUG-341 S7: drain the set of nodes touched by tracked DOM-mutation
    /// primitives since the last call, clearing it (and the `unattributed`
    /// flag) for the next cycle. See [`DomTouched`].
    pub fn take_dom_touched(&self) -> DomTouched {
        std::mem::take(&mut *self.dom_touched.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Returns `true` if `requestAnimationFrame` was called since the last call,
    /// clearing the flag. Mirrors [`crate::QuickJsRuntime::take_raf_pending`].
    pub fn take_raf_pending(&self) -> bool {
        self.raf_pending.swap(false, Ordering::Relaxed)
    }

    /// Non-consuming peek: `true` if rAF callbacks are queued.
    /// Mirrors [`crate::QuickJsRuntime::has_raf_pending`].
    pub fn has_raf_pending(&self) -> bool {
        self.raf_pending.load(Ordering::Relaxed)
    }

    /// ADR-016 M2.3: shared, lock-free handle to the rAF-pending flag.
    /// Mirrors [`crate::QuickJsRuntime::raf_pending_flag`].
    pub fn raf_pending_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.raf_pending)
    }

    /// ADR-016 M2.3: shared, lock-free handle to the DOM-dirty flag.
    /// Mirrors [`crate::QuickJsRuntime::dom_dirty_flag`].
    pub fn dom_dirty_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.dom_dirty)
    }

    /// Replace the layout bounding-rect table with a fresh snapshot.
    /// Mirrors [`crate::QuickJsRuntime::update_layout_rects`].
    pub fn update_layout_rects(&self, rects: HashMap<u32, [f32; 4]>) {
        *self.layout_rects.lock().unwrap_or_else(|e| e.into_inner()) = rects;
    }

    /// Update the current viewport dimensions.
    /// Mirrors [`crate::QuickJsRuntime::update_viewport_size`].
    pub fn update_viewport_size(&self, width: f32, height: f32) {
        *self.viewport_size.lock().unwrap_or_else(|e| e.into_inner()) = [width, height];
    }

    /// Drain lazy image load requests queued by JS.
    /// Mirrors [`crate::QuickJsRuntime::take_lazy_image_requests`].
    pub fn take_lazy_image_requests(&self) -> Vec<(u32, String)> {
        std::mem::take(&mut *self.lazy_img_requests.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Replace the scroll-state table with a fresh snapshot from the layout tree.
    /// Mirrors [`crate::QuickJsRuntime::update_scroll_states`].
    pub fn update_scroll_states(&self, states: HashMap<u32, [f32; 4]>) {
        *self.scroll_states.lock().unwrap_or_else(|e| e.into_inner()) = states;
    }

    /// Drain JS-initiated scroll requests queued by `_lumen_request_scroll`.
    /// Mirrors [`crate::QuickJsRuntime::take_scroll_requests`].
    pub fn take_scroll_requests(&self) -> Vec<(u32, f32, f32)> {
        std::mem::take(&mut *self.pending_scrolls.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain JS page-level scroll requests from `window.scrollTo/scrollBy`.
    /// Mirrors [`crate::QuickJsRuntime::take_page_scroll_requests`].
    pub fn take_page_scroll_requests(&self) -> Vec<(f32, bool)> {
        std::mem::take(&mut *self.pending_page_scrolls.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Update the page scroll Y exposed to JS `window.scrollY`/`pageYOffset`.
    ///
    /// Returns `true` when the position actually moved since the last call, so
    /// the caller can run CSSOM-View §14 «run the scroll steps» for the
    /// viewport (BUG-821). The previous value is kept here, per runtime, i.e.
    /// per document: a navigation builds a fresh runtime whose stored position
    /// is 0, so resetting the shell's `scroll_y` to the top cannot report a
    /// change against the *outgoing* document's position.
    pub fn set_page_scroll_y(&self, y: f32) -> bool {
        let mut cur = self.page_scroll_y.lock().unwrap_or_else(|e| e.into_inner());
        let moved = (*cur - y).abs() > f32::EPSILON;
        *cur = y;
        moved
    }

    /// Fire a non-bubbling `scroll` Event on the DOM element identified by `nid`.
    /// Mirrors [`crate::QuickJsRuntime::fire_element_scroll`].
    pub fn fire_element_scroll(&self, nid: u32) {
        let script = format!(
            "if(typeof _lumen_fire_scroll_on_element==='function')\
             _lumen_fire_scroll_on_element({nid});"
        );
        self.eval(&script).ok();
    }

    /// Fire a non-bubbling `scroll` Event on the `window` object (page scroll).
    /// Mirrors [`crate::QuickJsRuntime::fire_window_scroll`].
    pub fn fire_window_scroll(&self) {
        self.eval(
            "if(typeof _lumen_fire_window_scroll_event==='function')\
             _lumen_fire_window_scroll_event();"
        ).ok();
    }

    /// BUG-822: decide whether this rendering update owes a `scrollend` on the
    /// viewport, and clear the debt when it does.
    ///
    /// `moved` is [`Self::set_page_scroll_y`]'s answer for this update;
    /// `settled` says nothing is still driving the scroll (no smooth animation,
    /// no touch momentum, no scrollbar drag in progress). An instant scroll is
    /// therefore `moved && settled` and gets `scroll` + `scrollend` in the same
    /// frame, which CSSOM-View §14 allows; an animated one accumulates the debt
    /// while it runs and pays it on the update that stops moving the page —
    /// including the case where the last frame of touch momentum happens to
    /// clamp at the edge and moves nothing at all.
    pub fn page_scrollend_due(&self, moved: bool, settled: bool) -> bool {
        let mut pending = self.page_scroll_end_pending.lock().unwrap_or_else(|e| e.into_inner());
        if !settled {
            *pending |= moved;
            return false;
        }
        let due = moved || *pending;
        *pending = false;
        due
    }

    /// Fire a non-bubbling `scrollend` Event on the DOM element identified by `nid`.
    pub fn fire_element_scrollend(&self, nid: u32) {
        let script = format!(
            "if(typeof _lumen_fire_scrollend_on_element==='function')\
             _lumen_fire_scrollend_on_element({nid});"
        );
        self.eval(&script).ok();
    }

    /// Fire a non-bubbling `scrollend` Event on the `window` object (page scroll).
    pub fn fire_window_scrollend(&self) {
        self.eval(
            "if(typeof _lumen_fire_window_scrollend_event==='function')\
             _lumen_fire_window_scrollend_event();"
        ).ok();
    }

    /// Fire a CSS Scroll Snap L2 `snapchanging` event on a scroll container.
    /// Mirrors [`crate::QuickJsRuntime::fire_snap_changing`].
    pub fn fire_snap_changing(&self, nid: u32, block: Option<u32>, inline: Option<u32>) {
        self.fire_snap_event("_lumen_fire_snap_changing", nid, block, inline);
    }

    /// Fire a CSS Scroll Snap L2 `snapchanged` event on a scroll container.
    /// Mirrors [`crate::QuickJsRuntime::fire_snap_changed`].
    pub fn fire_snap_changed(&self, nid: u32, block: Option<u32>, inline: Option<u32>) {
        self.fire_snap_event("_lumen_fire_snap_changed", nid, block, inline);
    }

    /// Shared dispatch path for the snap events.
    /// Mirrors [`crate::QuickJsRuntime::fire_snap_event`].
    fn fire_snap_event(&self, func: &str, nid: u32, block: Option<u32>, inline: Option<u32>) {
        let blk = match block {
            Some(b) => format!("_lumen_make_element({b})"),
            None => "null".to_string(),
        };
        let inl = match inline {
            Some(i) => format!("_lumen_make_element({i})"),
            None => "null".to_string(),
        };
        let script = format!(
            "if(typeof {func}==='function'&&typeof _lumen_make_element==='function')\
             {func}({nid},{blk},{inl});"
        );
        self.eval(&script).ok();
    }

    /// Push a fresh snapshot of computed CSS styles into the JS runtime.
    /// Mirrors [`crate::QuickJsRuntime::update_computed_styles`].
    pub fn update_computed_styles(&self, styles: HashMap<u32, HashMap<String, String>>) {
        *self.computed_styles.lock().unwrap_or_else(|e| e.into_inner()) = styles;
    }

    /// Push a fresh snapshot of resolved CSS custom properties into the JS
    /// runtime, feeding `getComputedStyle(el).getPropertyValue('--x')`
    /// (BUG-732). Published from the same places as
    /// [`Self::update_computed_styles`] — a page whose custom properties are
    /// never pushed answers `""` for every variable.
    pub fn update_custom_properties(&self, props: CustomPropertySnapshot) {
        *self
            .custom_properties
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = props;
    }

    /// Update `document.hidden` / `document.visibilityState` and fire
    /// `visibilitychange` on both `document` and `window`.
    /// Mirrors [`crate::QuickJsRuntime::set_document_visibility`].
    pub fn set_document_visibility(&self, hidden: bool) {
        let script = if hidden {
            "_lumen_apply_visibility(true)"
        } else {
            "_lumen_apply_visibility(false)"
        };
        self.eval(script).ok();
    }

    /// Drain all popup window requests queued by JS `window.open(...)`.
    /// Mirrors [`crate::QuickJsRuntime::take_window_open_requests`].
    pub fn take_window_open_requests(&self) -> Vec<PopupRequest> {
        std::mem::take(&mut *self.window_open_requests.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain all `console.log/warn/error` messages queued since the last call.
    /// Mirrors [`crate::QuickJsRuntime::take_console_messages`].
    pub fn take_console_messages(&self) -> Vec<(u8, String)> {
        std::mem::take(&mut *self.console_messages.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain `history.pushState`/`history.replaceState` URL-update notifications.
    /// Mirrors [`crate::QuickJsRuntime::take_history_url_updates`].
    pub fn take_history_url_updates(&self) -> Vec<HistoryUrlUpdate> {
        std::mem::take(&mut *self.pending_history_url_updates.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain all `history.go(n)`/back/forward traversal deltas queued by JS.
    /// Mirrors [`crate::QuickJsRuntime::take_history_traversals`].
    pub fn take_history_traversals(&self) -> Vec<i32> {
        std::mem::take(&mut *self.pending_history_traversals.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain all Navigation API update requests queued by `_lumen_navigation_request`.
    /// Mirrors [`crate::QuickJsRuntime::take_nav_updates`].
    pub fn take_nav_updates(&self) -> Vec<crate::dom::NavUpdate> {
        std::mem::take(&mut *self.pending_navigation_updates.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain `NavigateEvent` intercept results queued during event dispatch.
    /// Mirrors [`crate::QuickJsRuntime::take_nav_intercept_result`].
    pub fn take_nav_intercept_result(&self) -> Vec<(bool, bool)> {
        std::mem::take(&mut *self.pending_nav_intercepted.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain all fullscreen requests queued by `element.requestFullscreen()`/`exitFullscreen()`.
    /// Mirrors [`crate::QuickJsRuntime::take_fullscreen_requests`].
    pub fn take_fullscreen_requests(&self) -> Vec<FullscreenRequest> {
        std::mem::take(&mut *self.fullscreen_requests.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain CSS View Transition events queued by `document.startViewTransition()` natives.
    /// Mirrors [`crate::QuickJsRuntime::take_view_transition_events`].
    pub fn take_view_transition_events(&self) -> Vec<crate::view_transitions::ViewTransitionEvent> {
        std::mem::take(
            &mut *self.view_transition_events.lock().unwrap_or_else(|e| e.into_inner()),
        )
    }

    /// Drain JS dialog focus requests queued by `_lumen_request_focus`/`_lumen_request_blur`.
    /// Mirrors [`crate::QuickJsRuntime::take_focus_requests`].
    pub fn take_focus_requests(&self) -> Vec<Option<u32>> {
        std::mem::take(&mut *self.pending_focus_requests.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Returns the DOM node nid that currently holds pointer capture (pointer_id=1).
    ///
    /// Shell calls this before dispatching pointer events to redirect them to the
    /// capture target instead of the hit-tested element (W3C Pointer Events L3 §4.1).
    /// Returns `None` when no capture is active. Mirrors [`crate::QuickJsRuntime::pointer_capture_nid`].
    pub fn pointer_capture_nid(&self) -> Option<u32> {
        *self.pointer_capture_nid.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Release the active pointer capture, returning the former capture target nid.
    ///
    /// Called by the shell implicitly on `pointerup`/`pointercancel` per spec §4.1.
    /// Returns `None` if no capture was active. Mirrors [`crate::QuickJsRuntime::take_pointer_capture`].
    pub fn take_pointer_capture(&self) -> Option<u32> {
        self.pointer_capture_nid.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    /// Test-only: run `f` on the JS thread and return its result. Gives module
    /// test suites (e.g. `canvas2d::tests_v8`) a way to inspect `thread_local!`
    /// state that has no JS-visible getter native — the JS thread owns its own
    /// thread-local instance, distinct from the test's calling thread. Mirrors
    /// [`Self::flush_canvas_updates`], generalized.
    #[cfg(test)]
    pub(crate) fn run_for_test<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        self.run(move |_inner| f())
    }

    /// Drain dirty `<canvas>` 2D buffers for GPU re-upload. Mirrors
    /// [`crate::QuickJsRuntime::flush_canvas_updates`]. Must run on the JS
    /// thread since `canvas2d`'s `CANVASES`/`DIRTY` registries are `thread_local!`.
    pub fn flush_canvas_updates(&self) -> Vec<(u32, u32, u32, Vec<u8>)> {
        self.run(|_inner| crate::canvas2d::flush_dirty())
    }

    /// Register decoded `<img>` bitmaps for canvas `drawImage`, keyed by node id.
    ///
    /// Mirrors [`crate::QuickJsRuntime::register_img_bitmaps`]: the shell calls it
    /// after `fetch_and_decode_images` so `drawImage(imgElement, …)` can read the
    /// decoded pixels out of [`crate::img_bitmap_store`]. The store is
    /// `thread_local!`, so the writes must happen on the JS thread — hence `run`.
    /// The `Arc` is shared with the shell's decode cache (no pixel copy, BUG-272
    /// срез 20); previous contents are cleared first (navigation-scoped).
    pub fn register_img_bitmaps(&self, bitmaps: Vec<(u32, Arc<lumen_image::Image>)>) {
        self.run(move |_inner| {
            crate::img_bitmap_store::clear_img_bitmaps();
            for (nid, image) in bitmaps {
                crate::img_bitmap_store::set_img_bitmap(nid, image);
            }
        });
    }

    /// BUG-480 срез 3: зарегистрировать загруженный под-документ `<iframe>` для доступа из
    /// JS родителя через `contentWindow`/`contentDocument`
    /// ([`crate::frame_bridge`]).
    ///
    /// Вызывается shell-ом после загрузки ребёнка и **до** диспатча trusted
    /// `load` на хосте — обработчики родителя вправе прочитать фасады прямо из
    /// обработчика. `accessible=false` (cross-origin / opaque sandbox)
    /// регистрирует биндинг без доступа к содержимому: `contentWindow`
    /// существует, `contentDocument` и все нативы чтения пусты.
    pub fn register_frame_document(
        &self,
        host_nid: u32,
        doc: Arc<Mutex<lumen_dom::Document>>,
        url: String,
        name: Option<String>,
        accessible: bool,
    ) {
        let registry = Arc::clone(&self.frame_docs);
        let idx = self.run(move |_inner| {
            let mut reg = registry.lock().unwrap_or_else(|e| e.into_inner());
            reg.frames.push(crate::frame_bridge::FrameDocBinding {
                host_nid,
                doc,
                url,
                name,
                accessible,
            });
            reg.frames.len() - 1
        });
        // Срез 3: индексный (`window[idx]`) + именованный (`window[имя]`)
        // доступники окна фрейма. Порядок регистрации = порядок документа,
        // поэтому idx совпадает со спечным tree order. Ошибки не фатальны:
        // контекст без window (минимальный тестовый изолят) просто пропустит
        // установку внутри шима.
        if let Err(e) =
            self.eval(&format!("typeof _lumen_frame_install_index === 'function' && _lumen_frame_install_index({idx})"))
        {
            eprintln!("v8: _lumen_frame_install_index({idx}) failed: {e}");
        }
    }

    /// BUG-480 срез 3: зарегистрировать документ **родителя** в JS-контексте
    /// фрейма — после этого `window.parent`/`window.top`/`window.frameElement`
    /// внутри фрейма видят фасады предков ([`crate::frame_bridge`]).
    ///
    /// `host_nid` — nid хоста в дереве родителя (для `frameElement`);
    /// `accessible=false` (cross-origin / opaque sandbox) оставляет окно
    /// предка доступным, но скрывает `.document` и содержимое.
    pub fn register_parent_document(
        &self,
        host_nid: u32,
        doc: Arc<Mutex<lumen_dom::Document>>,
        url: String,
        accessible: bool,
    ) {
        let registry = Arc::clone(&self.frame_docs);
        self.run(move |_inner| {
            registry.lock().unwrap_or_else(|e| e.into_inner()).parent =
                Some(crate::frame_bridge::FrameDocBinding {
                    host_nid,
                    doc,
                    url,
                    name: None,
                    accessible,
                });
        });
        // Срез 3: включить геттеры window.parent/top/frameElement/name. Ошибки
        // не фатальны (контекст без window просто пропустит установку).
        if let Err(e) = self.eval(
            "typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()",
        ) {
            eprintln!("v8: _lumen_frame_install_hierarchy failed: {e}");
        }
    }

    /// BUG-480 срез 3: зарегистрировать документ **верха** в JS-контексте
    /// фрейма глубины ≥ 2 — `window.top` должен вести в корень, а не в
    /// непосредственного родителя. У фрейма первого уровня не вызывается:
    /// там top разрешается через слот родителя.
    pub fn register_top_document(
        &self,
        doc: Arc<Mutex<lumen_dom::Document>>,
        url: String,
        accessible: bool,
    ) {
        let registry = Arc::clone(&self.frame_docs);
        self.run(move |_inner| {
            registry.lock().unwrap_or_else(|e| e.into_inner()).top =
                Some(crate::frame_bridge::FrameDocBinding {
                    host_nid: 0,
                    doc,
                    url,
                    name: None,
                    accessible,
                });
        });
        // Слот top ставится только у фреймов глубины ≥ 2, у которых parent
        // уже включил иерархию; повторный вызов идемпотентен.
        if let Err(e) = self.eval(
            "typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()",
        ) {
            eprintln!("v8: _lumen_frame_install_hierarchy failed: {e}");
        }
    }

    /// V8 isolate heap statistics: `(total_heap_size, used_heap_size)` in bytes.
    ///
    /// BUG-306 diagnostics: `V8JsRuntime` replaced `QuickJsRuntime` as the only
    /// JS engine (S12b), but `debug_js_heap` (`PersistentJs` trait, consumed by
    /// `LUMEN_MEM_REPORT`) was never wired to the V8 isolate — it silently fell
    /// back to the trait default `(-1, -1)`. `get_heap_statistics` requires
    /// running on the JS thread (mutable isolate access), hence `self.run`.
    pub fn debug_heap_stats(&self) -> (i64, i64) {
        self.run(|inner| {
            let stats = inner.isolate.get_heap_statistics();
            (stats.total_heap_size() as i64, stats.used_heap_size() as i64)
        })
    }

    /// Install the import map (HTML LS §8.1.6.2) used to resolve bare module
    /// specifiers such as `"react"`.
    ///
    /// Call before evaluating module scripts. Mirrors
    /// [`crate::QuickJsRuntime::set_import_map`]; the map is stored on the JS
    /// thread because V8's resolve callback can only reach thread-local state.
    pub fn set_import_map(&self, map: crate::esm::ImportMap) {
        self.run(move |_inner| crate::v8_esm::set_import_map(map));
    }

    /// Point this runtime's ESM loader at `base_url` and give it `provider` as
    /// its network bridge.
    ///
    /// `install_dom` does the same two writes for a page runtime; a **worker**
    /// runtime never goes through `install_dom` at all, so a module worker
    /// (BUG-777) has to set them itself or its `import` would resolve against
    /// an empty base and find no fetcher. Both live in `v8_esm`'s thread-local
    /// state — V8's resolve callback is capture-less and can reach nothing
    /// else — hence `self.run`, which lands on the runtime's own JS thread.
    pub(crate) fn set_module_context(
        &self,
        base_url: &str,
        provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    ) {
        let base_url = base_url.to_owned();
        self.run(move |_inner| {
            crate::v8_esm::set_page_url(&base_url);
            crate::v8_esm::set_fetch_provider(provider);
        });
    }

    /// Dispatch `f` to the JS thread, blocking until it completes.
    ///
    /// # Safety
    /// `f` may borrow from the caller's stack; we block on `rx.recv()` until the
    /// JS thread executes the job, so every borrow stays live. Erasing `'_` to
    /// `'static` is sound for the same reason as in `QuickJsRuntime::run`.
    #[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
    fn run<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&mut V8Inner) -> R + Send,
        R: Send,
    {
        let (tx, rx) = std::sync::mpsc::channel::<R>();
        let job: Box<dyn FnOnce(&mut V8Inner) + Send + '_> = Box::new(move |inner| {
            let _ = tx.send(f(inner));
        });
        // SAFETY: we block on rx.recv() below until the JS thread has completed
        // the job. Any borrows captured by `f` (e.g. `&str` args) outlive the
        // execution. The two Box types have identical fat-pointer layout; the
        // transmute only adjusts the lifetime annotation.
        let job: Box<dyn FnOnce(&mut V8Inner) + Send + 'static> = unsafe {
            std::mem::transmute::<
                Box<dyn FnOnce(&mut V8Inner) + Send + '_>,
                Box<dyn FnOnce(&mut V8Inner) + Send + 'static>,
            >(job)
        };
        // Исключение из `clippy::panic` (docs/lint-policy.md §10): `run` возвращает
        // `R`, а не `Result<R>`, и все его вызывающие — тоже. Смерть JS-потока
        // означает, что ответа не будет никогда; следующая строка всё равно
        // паникует на `rx.recv()`. Убрать панику можно только сменой сигнатуры
        // `run` на `Result` — это работа владельца крейта, не правка линта.
        #[allow(clippy::panic)]
        if self.cmd_tx.send(V8Command::Run(job)).is_err() {
            panic!("lumen-v8 thread terminated unexpectedly");
        }
        rx.recv().expect("lumen-v8 thread dropped without replying")
    }

    /// Register one already-wrapped native (built via [`crate::v8_compat::into_v8_fn0`]..`fn7`)
    /// as a global JS function `name`.
    ///
    /// Standalone module ports (S5-S7 batch 2+, one file per Web API) that need
    /// `Function::new`-style natives call this instead of duplicating the
    /// scope/store setup that `install_dom`'s mega-closure does inline for the
    /// natives it owns directly.
    pub(crate) fn register_native(
        &self,
        name: &'static str,
        native: Box<dyn crate::v8_compat::V8NativeFn + Send>,
    ) -> JsResult<()> {
        self.run(move |inner| {
            let isolate = &mut inner.isolate;
            let context_global = &inner.context;
            let store = &mut inner.native_fn_store;
            v8::scope!(let scope, isolate);
            let ctx = v8::Local::new(scope, context_global);
            let scope = &mut v8::ContextScope::new(scope, ctx);
            register_v8_native(scope, ctx, store, name, native)
        })
    }

    /// Register one already-wrapped scoped native (built via a
    /// [`crate::v8_compat::V8NativeFnScoped`] closure) as a global JS function
    /// `name`. Twin of [`Self::register_native`] for natives that need raw
    /// scope/argument access — currently only the WASM host-import bridge
    /// (Ph3 V8 migration S9).
    pub(crate) fn register_native_scoped(
        &self,
        name: &'static str,
        native: Box<dyn crate::v8_compat::V8NativeFnScoped + Send>,
    ) -> JsResult<()> {
        self.run(move |inner| {
            let isolate = &mut inner.isolate;
            let context_global = &inner.context;
            let store = &mut inner.native_fn_store_scoped;
            v8::scope!(let scope, isolate);
            let ctx = v8::Local::new(scope, context_global);
            let scope = &mut v8::ContextScope::new(scope, ctx);
            crate::v8_compat::register_v8_native_scoped(scope, ctx, store, name, native)
        })
    }
}

impl Drop for V8JsRuntime {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(V8Command::Shutdown);
        if let Some(handle) = self.js_thread.take() {
            let _ = handle.join();
        }
    }
}

// ── S2: console native registration ──────────────────────────────────────────

impl V8JsRuntime {
    /// Register the three console natives (`_lumen_console_log`,
    /// `_lumen_console_warn`, `_lumen_console_error`) as global JS functions.
    ///
    /// This is the S2 proof-of-concept that typed Rust closures can be
    /// registered via the compat layer and called from JS with auto-converted
    /// arguments.  S3 will extend this to all 184 `install_primitives` natives.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn install_console_natives(
        &self,
        console_messages: Arc<std::sync::Mutex<Vec<(u8, String)>>>,
    ) -> JsResult<()> {
        self.run(move |inner| {
            // Disjoint field borrows: scope borrows isolate, native_fn_store is separate.
            let isolate = &mut inner.isolate;
            let context_global = &inner.context;
            let store = &mut inner.native_fn_store;

            v8::scope!(let scope, isolate);
            let ctx = v8::Local::new(scope, context_global);
            let scope = &mut v8::ContextScope::new(scope, ctx);

            // Local `reg!` macro that mirrors the rquickjs original in dom.rs.
            // Arity 0 and 1 shown as proof; higher arities use into_v8_fn2..7.
            macro_rules! reg {
                ($name:expr, move || $body:expr) => {{
                    let native = into_v8_fn0(move || $body);
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                ($name:expr, move |$a:ident: $A:ty| $body:expr) => {{
                    let native = into_v8_fn1(move |$a: $A| $body);
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
            }

            // ── console ──────────────────────────────────────────────────────
            {
                let buf_log = Arc::clone(&console_messages);
                reg!("_lumen_console_log", move |msg: String| {
                    eprintln!("[JS] {msg}");
                    buf_log.lock().unwrap().push((0, msg));
                });
                let buf_warn = Arc::clone(&console_messages);
                reg!("_lumen_console_warn", move |msg: String| {
                    eprintln!("[JS warn] {msg}");
                    buf_warn.lock().unwrap().push((1, msg));
                });
                let buf_err = Arc::clone(&console_messages);
                reg!("_lumen_console_error", move |msg: String| {
                    eprintln!("[JS error] {msg}");
                    buf_err.lock().unwrap().push((2, msg));
                });
            }

            Ok(())
        })
    }
}

// ── S3: DOM-core native registration ─────────────────────────────────────────
//
// Ports `dom::install_primitives`'s 184 `_lumen_*` natives (rquickjs) to the V8
// compat layer. Scoped to DOM-core only, mirroring `dom::install_dom_api` minus
// the WebGL/Canvas2D/OffscreenCanvas/AudioContext installs that
// `QuickJsRuntime::install_dom` also performs — those, plus Highlight/Battery/
// Navigator-normalization/CSS-Houdini/SubtleCrypto/TrustedTypes, are separate
// future slices that each need their own ctx-taking install fn ported.

#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
impl V8JsRuntime {
    /// Install DOM-core native bindings (`_lumen_*`, 184 functions) and the
    /// `WEB_API_SHIM` JavaScript that builds `document`, `window`, `console`,
    /// `location`, `navigator`, `fetch`, `WebSocket`, `localStorage`, and
    /// `sessionStorage` on top of them.
    ///
    /// Mirrors [`crate::QuickJsRuntime::install_dom`] but scoped to the DOM-core
    /// piece only (`dom::install_dom_api`'s `install_primitives` + shim eval).
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn install_dom(
        &self,
        doc: Arc<Mutex<lumen_dom::Document>>,
        page_url: &str,
        fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
        ws_provider: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>>,
        sse_provider: Option<Arc<dyn lumen_core::ext::JsSseProvider>>,
        ls_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
        idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
        sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
        cache_backend: Option<Arc<dyn lumen_core::ext::CacheBackend>>,
        sw_worker_store: Option<lumen_core::ext::SwWorkerStore>,
        cross_origin_isolated: bool,
    ) -> JsResult<()> {
        let ls_store =
            ls_store.unwrap_or_else(|| Arc::new(Mutex::new(lumen_core::WebStorage::default())));
        // BUG-836: the tab's store when the owner attached one via
        // `with_session_storage`, so `sessionStorage` survives navigation the way
        // HTML LS §12.2 requires; a fresh (document-local) store otherwise.
        let ss_store: Arc<Mutex<lumen_core::WebStorage>> = self
            .ss_store
            .clone()
            .unwrap_or_else(|| Arc::new(Mutex::new(lumen_core::WebStorage::default())));
        // PH3-20: an explicit `sw_worker_store` argument takes precedence over a
        // store set earlier via a builder (mirrors `QuickJsRuntime::install_dom`).
        let sw_worker_store = sw_worker_store.or_else(|| self.sw_worker_store.clone());
        // Cookie access is not part of the S3 DOM-core signature; document.cookie
        // reads/writes as empty until a future slice threads a CookieProvider through.
        let cookie_jar: Option<Arc<dyn lumen_core::ext::CookieProvider>> = None;
        let deterministic_seed = if self
            .deterministic
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            // DEVX-16: an explicit `--rng-seed` override takes precedence over
            // the URL-hash derivation.
            let override_seed = *self
                .deterministic_rng_seed
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            Some(override_seed.unwrap_or_else(|| crate::deterministic_seed_from_url(page_url)))
        } else {
            None
        };
        let monotonic_clock = self.deterministic_monotonic.load(Ordering::Relaxed);
        let deterministic_clock_ms = Arc::clone(&self.deterministic_clock_ms);
        // BUG-371: the file-API grants are bound to this document's origin.
        // Derived here, before `page_url` is moved into the `self.run` closure
        // below, and never taken from a JS argument.
        let page_origin = crate::file_input::origin_for_url(page_url);
        let page_url = page_url.to_owned();
        // BUG-480 срез 4: ключ этого контекста в исходящем ящике кросс-
        // фреймовых postMessage — указатель Arc собственного документа. Тот
        // же инстанс Arc уходит и в реестр родителя (register_iframe_document),
        // поэтому адресата можно найти с обеих сторон. Ставится до run() —
        // doc уезжает в замыкание по значению.
        //
        // Срез 8: тот же ключ дублируется в рантайме — по нему шелл спрашивает
        // «есть ли неразобранные конверты для МЕНЯ» и будит спящий цикл.
        //
        // Срез 10: клон Arc остаётся в реестре — натив обратной доставки
        // ресурсных событий (`_lumen_f_queue_parent_resource`) проверяет по
        // нему nid и берёт `source_doc` конверта.
        {
            let mut reg = self
                .frame_docs
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            reg.self_key = Some(Arc::as_ptr(&doc) as usize);
            reg.self_doc = Some(Arc::clone(&doc));
            reg.self_origin = page_origin.clone();
            self.self_doc_key
                .store(Arc::as_ptr(&doc) as usize, Ordering::Relaxed);
        }
        // BUG-295: session-level `navigator.userAgent` override, if any.
        let ua_override = global_user_agent_override();
        // BUG-295: session-level `Intl`/`Date` timezone override, if any.
        let timezone_override = global_timezone_override();
        // BUG-364: dedicated/shared Worker script fetch reuses the same fetch
        // provider as the Fetch API below. Cloned *before* the `move` closure
        // below takes ownership of `fetch_provider` — Worker/SharedWorker
        // installation happens after that closure returns.
        let fp_worker = fetch_provider.clone();
        let fp_shared_worker = fetch_provider.clone();
        // Тот же провайдер отдаётся загрузчику модулей: `import('./chunk.js')`
        // обязан сходить в сеть, а не искать чанк в заранее зарегистрированных
        // исходниках (иначе code-split приложение не собирается вовсе).
        let fp_esm = fetch_provider.clone();
        // Сеть области сервис-воркера (`importScripts`, `fetch`): клонируется
        // здесь по той же причине, что и предыдущие — ниже провайдер уезжает
        // в замыкание по значению.
        let fp_sw_net = fetch_provider.clone();
        // База IndexedDB воркера — та же, что у страницы: воркер, ведущий свою
        // очередь в `indexedDB`, обязан видеть те же данные.
        let idb_sw = idb_backend.clone();

        self.run(move |inner| {
            // ESM (S12b-23): fallback base URL the module resolver uses for
            // relative imports issued from inline `<script type=module>` bodies,
            // which have no URL of their own. Mirrors the `module_page_url`
            // write in `QuickJsRuntime::install_dom`.
            crate::v8_esm::set_page_url(&page_url);
            crate::v8_esm::set_fetch_provider(fp_esm);
            // BUG-384: point the Window named-properties interceptor (installed
            // on the global object template back at context creation) at this
            // navigation's document. Done here, on the JS thread, because the
            // slot it writes is thread-local to the isolate's owning thread.
            set_named_access_document(&doc);
            // Disjoint field borrows: scope borrows isolate, native_fn_store is separate.
            let isolate = &mut inner.isolate;
            let context_global = &inner.context;
            let store = &mut inner.native_fn_store;

            v8::scope!(let scope, isolate);
            let ctx = v8::Local::new(scope, context_global);
            let scope = &mut v8::ContextScope::new(scope, ctx);

            let nav_out = Arc::clone(&self.nav_out);
            let timer_wakeup = Arc::clone(&self.timer_wakeup);
            let dom_dirty = Arc::clone(&self.dom_dirty);
            let dom_touched = Arc::clone(&self.dom_touched);
            let raf_pending = Arc::clone(&self.raf_pending);
            let layout_rects = Arc::clone(&self.layout_rects);
            let viewport_size = Arc::clone(&self.viewport_size);
            let lazy_img_requests = Arc::clone(&self.lazy_img_requests);
            let scroll_states = Arc::clone(&self.scroll_states);
            let pending_scrolls = Arc::clone(&self.pending_scrolls);
            let pending_page_scrolls = Arc::clone(&self.pending_page_scrolls);
            let page_scroll_y = Arc::clone(&self.page_scroll_y);
            let computed_styles = Arc::clone(&self.computed_styles);
            let custom_properties = Arc::clone(&self.custom_properties);
            let window_open_requests = Arc::clone(&self.window_open_requests);
            let console_messages = Arc::clone(&self.console_messages);
            let pending_history_url_updates = Arc::clone(&self.pending_history_url_updates);
            let pending_history_traversals = Arc::clone(&self.pending_history_traversals);
            let nav_state = Arc::clone(&self.nav_state);
            let pending_navigation_updates = Arc::clone(&self.pending_navigation_updates);
            let pending_nav_intercepted = Arc::clone(&self.pending_nav_intercepted);
            let fullscreen_requests = Arc::clone(&self.fullscreen_requests);
            let print_requests = Arc::clone(&self.print_requests);
            let pending_focus_requests = Arc::clone(&self.pending_focus_requests);

            install::install_console(scope, ctx, store, Arc::clone(&console_messages))?;

            install::install_print(scope, ctx, store, Arc::clone(&print_requests))?;

            install::install_dialog_focus(scope, ctx, store, Arc::clone(&pending_focus_requests))?;

            install::install_document_meta(scope, ctx, store, Arc::clone(&doc))?;

            install::install_document_fonts(scope, ctx, store, Arc::clone(&doc))?;

            install::install_node_lookup(scope, ctx, store, Arc::clone(&doc))?;

            install::install_node_properties(
                scope,
                ctx,
                store,
                Arc::clone(&doc),
                Arc::clone(&dom_dirty),
                Arc::clone(&dom_touched),
            )?;

            install::install_tree_navigation(scope, ctx, store, Arc::clone(&doc))?;

            install::install_node_count(scope, ctx, store, Arc::clone(&doc))?;

            install::install_tree_mutation(
                scope,
                ctx,
                store,
                Arc::clone(&doc),
                Arc::clone(&dom_dirty),
                Arc::clone(&dom_touched),
            )?;

            install::install_service_worker(
                scope,
                ctx,
                store,
                sw_backend.clone(),
                cache_backend.clone(),
                sw_worker_store.clone(),
                fp_sw_net.clone(),
                idb_sw.clone(),
            )?;

            install::install_history(
                scope,
                ctx,
                store,
                Arc::clone(&pending_history_url_updates),
                Arc::clone(&pending_history_traversals),
            )?;

            install::install_navigation_api(
                scope,
                ctx,
                store,
                Arc::clone(&nav_state),
                Arc::clone(&pending_navigation_updates),
                Arc::clone(&pending_nav_intercepted),
            )?;

            install::install_navigation(scope, ctx, store, Arc::clone(&nav_out))?;

            install::install_fetch(scope, ctx, store, fetch_provider.clone())?;

            install::install_clipboard(scope, ctx, store)?;

            install::install_webauthn(scope, ctx, store)?;

            install::install_websocket(scope, ctx, store, ws_provider.clone())?;

            install::install_text_decoder(scope, ctx, store)?;

            install::install_sse(scope, ctx, store, sse_provider.clone())?;

            install::install_local_storage(scope, ctx, store, Arc::clone(&ls_store))?;

            install::install_session_storage(scope, ctx, store, Arc::clone(&ss_store))?;

            install::install_indexed_db(scope, ctx, store, idb_backend.clone())?;

            install::install_performance_now(
                scope,
                ctx,
                store,
                deterministic_seed,
                monotonic_clock,
                Arc::clone(&deterministic_clock_ms),
            )?;

            install::install_timer_wakeup(
                scope,
                ctx,
                store,
                Arc::clone(&timer_wakeup),
                Arc::clone(&raf_pending),
            )?;

            install::install_element_geometry(
                scope,
                ctx,
                store,
                Arc::clone(&layout_rects),
                Arc::clone(&viewport_size),
            )?;

            install::install_match_media(scope, ctx, store)?;

            install::install_css_supports_and_lazy_images(
                scope,
                ctx,
                store,
                Arc::clone(&lazy_img_requests),
            )?;

            install::install_scroll_state(
                scope,
                ctx,
                store,
                Arc::clone(&scroll_states),
                Arc::clone(&pending_scrolls),
                Arc::clone(&pending_page_scrolls),
                Arc::clone(&page_scroll_y),
            )?;

            install::install_window_open(scope, ctx, store, Arc::clone(&window_open_requests))?;

            install::install_fullscreen(scope, ctx, store, Arc::clone(&fullscreen_requests))?;

            install::install_pointer_lock(scope, ctx, store)?;

            install::install_computed_styles(
                scope,
                ctx,
                store,
                Arc::clone(&computed_styles),
                Arc::clone(&custom_properties),
            )?;

            install::install_shadow_dom(
                scope,
                ctx,
                store,
                Arc::clone(&doc),
                Arc::clone(&dom_dirty),
                Arc::clone(&dom_touched),
            )?;

            install::install_selection(
                scope,
                ctx,
                store,
                Arc::clone(&doc),
                Arc::clone(&dom_dirty),
                Arc::clone(&dom_touched),
            )?;
            install::install_contenteditable(scope, ctx, store, Arc::clone(&doc))?;
            install::install_design_mode(
                scope,
                ctx,
                store,
                Arc::clone(&doc),
                Arc::clone(&dom_dirty),
                Arc::clone(&dom_touched),
            )?;

            install::install_cookie(scope, ctx, store, page_url.clone(), cookie_jar.clone())?;

            install::install_esm_registry(scope, ctx, store)?;

            install::install_microtask_drain(scope, ctx, store)?;

            install::install_crypto_and_typed_om(
                scope,
                ctx,
                store,
                Arc::clone(&doc),
                Arc::clone(&dom_dirty),
                Arc::clone(&dom_touched),
                Arc::clone(&computed_styles),
                Arc::clone(&custom_properties),
            )?;

            // Inject the page URL + cross-origin-isolation state as JS globals so
            // WEB_API_SHIM can initialise `location` and `window.crossOriginIsolated`.
            {
                let key = v8::String::new(scope, "_LUMEN_PAGE_URL")
                    .ok_or_else(|| JsError::Runtime("OOM: key '_LUMEN_PAGE_URL'".into()))?;
                let val = v8::String::new(scope, &page_url)
                    .ok_or_else(|| JsError::Runtime("OOM: page_url value".into()))?;
                ctx.global(scope).set(scope, key.into(), val.into());
            }
            {
                let key = v8::String::new(scope, "_LUMEN_CROSS_ORIGIN_ISOLATED").ok_or_else(
                    || JsError::Runtime("OOM: key '_LUMEN_CROSS_ORIGIN_ISOLATED'".into()),
                )?;
                let val = v8::Boolean::new(scope, cross_origin_isolated);
                ctx.global(scope).set(scope, key.into(), val.into());
            }

            // Polyfill `DOMException`: quickjs-ng provides it as a built-in (part of
            // `Context::full()`'s bundled extras), V8 has no web-platform globals at
            // all. `WEB_API_SHIM` and dozens of `install_*` module shims (Ph3 V8
            // migration S5-S7) construct `new DOMException(...)` — without this,
            // `class X extends DOMException` throws `ReferenceError` the moment any
            // such module is evaluated. Shape probed against quickjs-ng's built-in
            // (legacy numeric `code`, full WHATWG constant table, `instanceof Error`).
            {
                v8::tc_scope!(tc, scope);
                let src = v8::String::new(tc, DOM_EXCEPTION_POLYFILL)
                    .ok_or_else(|| JsError::Runtime("OOM: DOM_EXCEPTION_POLYFILL source".into()))?;
                let compiled = v8::Script::compile(tc, src, None);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled.ok_or_else(|| {
                    JsError::Runtime("DOM_EXCEPTION_POLYFILL compile returned None".into())
                })?;
                let result = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let _ = result;
            }

            // Evaluate WEB_API_SHIM inline. Cannot call `self.eval(...)` here: the JS
            // thread is already busy running this job (dispatched via `self.run`), and
            // `run` cannot be re-entered from inside its own job closure (it would
            // deadlock waiting on a channel the thread isn't servicing).
            //
            // BUG-378: run it through *indirect* eval rather than as a Script.
            // Both forms create the shim's top-level `var`/`function` bindings as
            // properties of the global object, but with different attributes: a
            // Script's GlobalDeclarationInstantiation passes `D = false`
            // (ECMA-262 §16.1.7), so `_lumen_u2n`, `_lumen_timers`, … come out
            // **non-configurable** — and `enumerable: true → false` is exactly
            // the transition `Object.defineProperty` forbids on a
            // non-configurable property, which left 247 internal names
            // permanently visible to `for (k in window)` no matter what the
            // sealing pass did. Indirect eval's EvalDeclarationInstantiation
            // passes `D = true` (§19.2.1.3), so the same bindings become
            // configurable and `internal_globals::seal_internal_globals_v8` can
            // hide and freeze them at the end of this function.
            //
            // Safe only because the shim has no top-level `let`/`const`/`class`:
            // those are lexical, and eval puts them in a declarative environment
            // that dies with the eval call instead of on the global object — a
            // future top-level `const` would silently vanish. Guarded by
            // `internal_globals`'s `shim_has_no_top_level_lexical_declarations`.
            {
                v8::tc_scope!(tc, scope);
                let shim = crate::dom::web_api_shim();
                let src = v8::String::new(tc, &shim)
                    .ok_or_else(|| JsError::Runtime("OOM: WEB_API_SHIM source".into()))?;
                let wrapper_src = v8::String::new(tc, "(function(s) { (0, eval)(s); })")
                    .ok_or_else(|| JsError::Runtime("OOM: WEB_API_SHIM eval wrapper".into()))?;
                let compiled = v8::Script::compile(tc, wrapper_src, None);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled
                    .ok_or_else(|| JsError::Runtime("WEB_API_SHIM compile returned None".into()))?;
                let wrapper = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let wrapper: v8::Local<v8::Function> = wrapper
                    .ok_or_else(|| JsError::Runtime("WEB_API_SHIM eval wrapper is None".into()))?
                    .try_into()
                    .map_err(|_| {
                        JsError::Runtime("WEB_API_SHIM eval wrapper is not a function".into())
                    })?;
                let recv = v8::undefined(tc).into();
                let result = wrapper.call(tc, recv, &[src.into()]);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let _ = result;
            }

            // Trusted Types API (W3C TT L2, Phase 0): plain JS, no rquickjs-specific API,
            // so the shared shim string is evaluated the same way as WEB_API_SHIM above.
            {
                v8::tc_scope!(tc, scope);
                let src = v8::String::new(tc, crate::trusted_types::TRUSTED_TYPES_SHIM)
                    .ok_or_else(|| JsError::Runtime("OOM: TRUSTED_TYPES_SHIM source".into()))?;
                let compiled = v8::Script::compile(tc, src, None);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled.ok_or_else(|| {
                    JsError::Runtime("TRUSTED_TYPES_SHIM compile returned None".into())
                })?;
                let result = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let _ = result;
            }

            // Deterministic render mode (8F): override Math.random with a seeded
            // xorshift32 PRNG and freeze Date.now() at 0. Must run after WEB_API_SHIM
            // so Date and Math are fully set up. Mirrors the script QuickJS's
            // `dom::install_dom_api` builds, except for the DEVX-16 monotonic-clock
            // branch below, which is V8-only (QuickJS is a frozen rollback path,
            // see CLAUDE.md — no new functionality is added there).
            if let Some(seed) = deterministic_seed {
                let seed32 = u32::try_from(seed & 0xffff_ffff).unwrap_or(1);
                let seed32 = if seed32 == 0 { 1 } else { seed32 };
                // DEVX-16: `--monotonic-clock` routes Date.now() through the same
                // `_lumen_now_ms()` native binding performance.now() uses, so both
                // advance in lockstep off one shared counter instead of Date.now()
                // staying frozen at 0.
                let date_now_body = if monotonic_clock {
                    "return _lumen_now_ms();"
                } else {
                    "return 0;"
                };
                let js = format!(
                    "(function(){{var s={seed32};\
                     Math.random=function(){{s^=s<<13;s^=s>>>17;s^=s<<5;return (s>>>0)/4294967296;}};\
                     Date.now=function(){{{date_now_body}}};\
                     }})()"
                );
                v8::tc_scope!(tc, scope);
                let src = v8::String::new(tc, &js)
                    .ok_or_else(|| JsError::Runtime("OOM: deterministic seed script".into()))?;
                let compiled = v8::Script::compile(tc, src, None);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled.ok_or_else(|| {
                    JsError::Runtime("deterministic seed script compile returned None".into())
                })?;
                let result = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let _ = result;
            }

            // BUG-295 (`emulation.setUserAgentOverride`): must run after
            // WEB_API_SHIM (same ordering constraint as the deterministic-seed
            // block above) so `navigator` already exists, and before any page
            // `<script>` executes (those run after `install_dom` returns) so
            // even a synchronous top-level read of `navigator.userAgent` sees
            // the override from the very first script.
            if let Some(ua) = ua_override {
                v8::tc_scope!(tc, scope);
                let js = user_agent_override_script(&ua);
                let src = v8::String::new(tc, &js)
                    .ok_or_else(|| JsError::Runtime("OOM: UA override script".into()))?;
                let compiled = v8::Script::compile(tc, src, None);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled
                    .ok_or_else(|| JsError::Runtime("UA override script compile returned None".into()))?;
                let result = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let _ = result;
            }

            // BUG-295 (`browser.setTimezoneOverride`): a plain global
            // assignment, order-independent w.r.t. the `Intl` shim (installed
            // later, outside this closure — see `intl_bindings::install_intl_bindings_v8`
            // call site) since the shim reads the marker lazily at
            // `DateTimeFormat` construction time, not at shim-install time.
            if let Some(tz) = timezone_override {
                v8::tc_scope!(tc, scope);
                let js = timezone_override_script(&tz);
                let src = v8::String::new(tc, &js)
                    .ok_or_else(|| JsError::Runtime("OOM: timezone override script".into()))?;
                let compiled = v8::Script::compile(tc, src, None);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled.ok_or_else(|| {
                    JsError::Runtime("timezone override script compile returned None".into())
                })?;
                let result = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let _ = result;
            }

            Ok(())
        })?;

        // Ph3 V8 migration S5-S7: simple-module batch (~68 modules, plain JS-shim
        // installs with no native `Function::new` registrations, or ≤2 trivial
        // shims) — see docs/tasks/ph3-v8-migration.md for the ported/pending
        // checklist. Called outside the `self.run` dispatch above because each
        // `install_*_v8` helper dispatches its own job via `self.eval` internally.
        // Best-effort like the rquickjs orchestration in `lib.rs::install_dom`
        // (`if let Err(e) = X::install_Y(&ctx) { eprintln!(...) }`): one broken/
        // partial module (Phase 0 stub) must not abort DOM bootstrap for the
        // other 67.
        macro_rules! install_v8 {
            ($module:ident :: $func:ident) => {
                if let Err(e) = crate::$module::$func(self) {
                    eprintln!("v8: {}::{} failed: {e}", stringify!($module), stringify!($func));
                }
            };
        }
        // Ph3 V8 migration S8: canvas2d + webgl_canvas (hand-port, not part of the
        // simple-module S5-S7 batch, but same best-effort orchestration). Mirrors
        // lib.rs::install_dom's ordering (webgl before canvas2d).
        let fingerprint = lumen_paint::GpuFingerprint {
            vendor: "WebKit".to_string(),
            renderer: "Generic GPU".to_string(),
        };
        if let Err(e) = crate::webgl_canvas::install_webgl_canvas_v8(self, &fingerprint) {
            eprintln!("v8: webgl_canvas::install_webgl_canvas_v8 failed: {e}");
        }
        install_v8!(canvas2d::install_canvas2d_bindings_v8);
        // P1-imagebitmap: OffscreenCanvas was deferred past S8 (see the note at
        // canvas2d.rs's transferControlToOffscreen V8 port); ported now so
        // createImageBitmap/ImageBitmapRenderingContext work under the default engine.
        install_v8!(offscreen_canvas::install_offscreen_canvas_bindings_v8);

        install_v8!(async_context::install_async_context_v8);
        install_v8!(attribution_reporting::install_attribution_reporting_api_v8);
        install_v8!(audio_element::install_audio_element_bindings_v8);
        install_v8!(background_fetch::install_background_fetch_v8);
        install_v8!(background_sync::install_background_sync_v8);
        install_v8!(badging::install_badging_bindings_v8);
        install_v8!(battery_bindings::install_battery_bindings_v8);
        install_v8!(bluetooth::install_bluetooth_bindings_v8);
        install_v8!(broadcast_channel::install_broadcast_channel_bindings_v8);
        install_v8!(close_watcher::install_close_watcher_v8);
        install_v8!(compute_pressure::install_compute_pressure_bindings_v8);
        install_v8!(contacts::install_contacts_manager_v8);
        install_v8!(content_index::install_content_index_api_v8);
        // Default: enabled (mirrors lib.rs::install_dom's `self.cookie_banner_dismiss`
        // AtomicBool read; extra-arg call since the flag lives on `self`, not `&ctx`).
        let cb_enabled = self.cookie_banner_dismiss.load(Ordering::Relaxed);
        if let Err(e) = crate::cookie_banner::install_cookie_banner_bindings_v8(self, cb_enabled) {
            eprintln!("v8: cookie_banner::install_cookie_banner_bindings_v8 failed: {e}");
        }
        install_v8!(cookie_store::install_cookie_store_v8);
        install_v8!(credentials::install_credentials_bindings_v8);
        install_v8!(csp::install_csp_bindings_v8);
        install_v8!(css_properties_values_api::install_css_properties_values_api_v8);
        install_v8!(decorators::install_decorator_shim_v8);
        install_v8!(device_sensors::install_device_sensors_bindings_v8);
        install_v8!(digital_credentials::install_digital_credentials_api_v8);
        install_v8!(document_pip::install_document_pip_api_v8);
        install_v8!(documentpip_bindings::install_docpip_bindings_v8);
        install_v8!(dom_parser::install_dom_parser_v8);
        install_v8!(download_bindings::install_download_bindings_v8);
        install_v8!(element_internals::install_element_internals_bindings_v8);
        install_v8!(es2026_proposals::install_es2026_proposals_v8);
        install_v8!(eye_dropper::install_eye_dropper_bindings_v8);
        // BUG-371: both file APIs take the document origin their grants are
        // bound to, so they get explicit-arg calls rather than `install_v8!`.
        if let Err(e) = crate::file_input::install_file_input_bindings_v8(self, &page_origin) {
            eprintln!("v8: file_input::install_file_input_bindings_v8 failed: {e}");
        }
        if let Err(e) = crate::filesystem_access::install_filesystem_access_v8(self, &page_origin) {
            eprintln!("v8: filesystem_access::install_filesystem_access_v8 failed: {e}");
        }
        // BUG-371 point 1: both shims have now captured the natives they need,
        // so take the whole file-API surface off the global object. Runs even if
        // one of the two installs above failed — a half-installed shim must not
        // leave `__lumen_file_read_text` sitting on `window`.
        if let Err(e) = crate::file_input::seal_file_natives_v8(self) {
            eprintln!("v8: file_input::seal_file_natives_v8 failed: {e}");
        }
        install_v8!(form_validation::install_form_validation_bindings_v8);
        install_v8!(gamepad::install_gamepad_bindings_v8);
        install_v8!(generic_sensor::install_generic_sensor_bindings_v8);
        // Default: PERMISSION_DENIED (mirrors lib.rs::install_dom's hardcoded `None`).
        if let Err(e) = crate::geolocation::install_geolocation_bindings_v8(self, None) {
            eprintln!("v8: geolocation::install_geolocation_bindings_v8 failed: {e}");
        }
        install_v8!(highlight_api::install_highlight_api_bindings_v8);
        install_v8!(idle_detection::install_idle_detection_bindings_v8);
        // BUG-480 срез 2: бридж contentWindow/contentDocument получает реестр
        // рантайма явным аргументом (как geolocation/pointer_capture ниже),
        // а не через install_v8! — нативы `_lumen_f_*` читают тот же Arc,
        // куда пишет register_frame_document. Ставится до iframe_element:
        // его геттеры вызывают функции шима бриджа.
        if let Err(e) =
            crate::frame_bridge::install_frame_bridge_v8(self, Arc::clone(&self.frame_docs))
        {
            eprintln!("v8: frame_bridge::install_frame_bridge_v8 failed: {e}");
        }
        install_v8!(iframe_element::install_iframe_element_bindings_v8);
        install_v8!(inert::install_inert_api_v8);
        install_v8!(intl_bindings::install_intl_bindings_v8);
        install_v8!(launch_handler::install_launch_handler_api_v8);
        install_v8!(local_font_access::install_local_font_access_api_v8);
        install_v8!(long_animation_frames::install_long_animation_frames_bindings_v8);
        install_v8!(media_capabilities::install_media_capabilities_bindings_v8);
        install_v8!(media_capture::install_media_capture_bindings_v8);
        install_v8!(media_devices::install_media_devices_bindings_v8);
        install_v8!(media_session::install_media_session_bindings_v8);
        install_v8!(media_stream_recording::install_media_stream_recording_v8);
        install_v8!(navigation_api::install_navigation_api_v8);
        install_v8!(navigator_bindings::install_navigator_bindings_v8);
        install_v8!(network_log_bindings::install_network_log_bindings_v8);
        // Default permission: "denied" (mirrors lib.rs::install_dom's hardcoded `false`).
        if let Err(e) = crate::notifications_bindings::install_notifications_bindings_v8(self, false)
        {
            eprintln!("v8: notifications_bindings::install_notifications_bindings_v8 failed: {e}");
        }
        install_v8!(paint_worklet::install_paint_worklet_api_v8);
        install_v8!(payment_request::install_payment_request_v8);
        install_v8!(periodic_sync::install_periodic_sync_v8);
        install_v8!(permissions::install_permissions_api_v8);
        install_v8!(permissions_policy::install_permissions_policy_bindings_v8);
        install_v8!(pip_bindings::install_pip_bindings_v8);
        // W3C Pointer Events Level 3 §4.1 — takes `pointer_capture_nid` by ref since the
        // native closures need the runtime-instance Arc, not just `&self` (mirrors the
        // `geolocation`/`shared_worker` extra-arg calls above, not the plain `install_v8!` macro).
        if let Err(e) = crate::pointer_capture::install_pointer_capture_bindings_v8(
            self,
            Arc::clone(&self.pointer_capture_nid),
        ) {
            eprintln!("v8: pointer_capture::install_pointer_capture_bindings_v8 failed: {e}");
        }
        install_v8!(presentation_api::install_presentation_api_v8);
        install_v8!(push_api::install_push_api_v8);
        install_v8!(reporting_api::install_reporting_api_bindings_v8);
        install_v8!(sanitizer::install_sanitizer_bindings_v8);
        install_v8!(scheduler::install_scheduler_api_v8);
        install_v8!(screen_capture::install_screen_capture_bindings_v8);
        install_v8!(screen_orientation::install_screen_orientation_bindings_v8);
        install_v8!(scroll_snap_events::install_scroll_snap_events_bindings_v8);
        install_v8!(scroll_timeline::install_scroll_timeline_bindings_v8);
        install_v8!(serial::install_serial_bindings_v8);
        install_v8!(shape_detection::install_shape_detection_bindings_v8);
        install_v8!(shared_storage::install_shared_storage_v8);
        install_v8!(soft_navigation::install_soft_navigation_api_v8);
        install_v8!(speculation_rules::install_speculation_rules_api_v8);
        install_v8!(speech::install_speech_bindings_v8);
        install_v8!(storage_buckets::install_storage_buckets_v8);
        // BUG-372: `navigator.storage.getDirectory()` opens the OPFS subtree of
        // the installing document's origin, so this one takes the origin too.
        if let Err(e) =
            crate::storage_manager::install_storage_manager_bindings_v8(self, &page_origin)
        {
            eprintln!("v8: storage_manager::install_storage_manager_bindings_v8 failed: {e}");
        }
        install_v8!(surface_api::install_surface_api_protection_v8);
        install_v8!(svg::install_svg_bindings_v8);
        install_v8!(tc39_proposals::install_tc39_proposals_v8);
        install_v8!(temporal_api::install_temporal_api_v8);
        install_v8!(topics_api::install_topics_api_v8);
        install_v8!(typed_om_api::install_typed_om_api_v8);
        install_v8!(ua_client_hints::install_ua_client_hints_bindings_v8);
        install_v8!(url_pattern::install_url_pattern_api_v8);
        install_v8!(video_bindings::install_video_bindings_v8);
        install_v8!(video_pip::install_video_pip_api_v8);
        // CSS View Transitions L1 (BUG-545): takes `view_transition_events` by ref since
        // the native closures need the runtime-instance Arc, not just `&self` (mirrors the
        // `geolocation`/`pointer_capture` extra-arg calls above, not the plain `install_v8!` macro).
        if let Err(e) = crate::view_transitions::install_view_transition_bindings_v8(
            self,
            Arc::clone(&self.view_transition_events),
        ) {
            eprintln!("v8: view_transitions::install_view_transition_bindings_v8 failed: {e}");
        }
        install_v8!(virtual_keyboard::install_virtual_keyboard_bindings_v8);
        install_v8!(wake_lock::install_wake_lock_bindings_v8);
        install_v8!(web_audio::install_web_audio_api_v8);
        install_v8!(webhid::install_webhid_bindings_v8);
        install_v8!(web_locks::install_web_locks_bindings_v8);
        install_v8!(web_midi::install_web_midi_api_v8);
        install_v8!(webrtc_stub::install_webrtc_bindings_v8);
        install_v8!(webusb::install_webusb_bindings_v8);
        install_v8!(webxr::install_webxr_bindings_v8);
        install_v8!(window_management::install_window_management_api_v8);
        install_v8!(xhr::install_xhr_bindings_v8);
        install_v8!(web_codecs::install_webcodecs_bindings_v8);
        // Ph3 V8 migration S9: wasm + webgpu (hand-port, same best-effort
        // orchestration as S8's canvas2d/webgl_canvas above).
        install_v8!(webassembly::install_webassembly_bindings_v8);
        install_v8!(webgpu::install_webgpu_bindings_v8);
        // Ph3 V8 migration S10: worker + shared_worker (hand-port — extra
        // per-runtime state beyond `&ctx`, same shape as S5-S7 batch 3's
        // geolocation/broadcast_channel/notifications_bindings, so called
        // directly rather than through the `install_v8!` macro). Service
        // Worker activation (`_lumen_sw_activate_script`) is already wired
        // above in the S3 core-native block, via `spawn_sw_worker_v8`.
        if let Err(e) = crate::worker::install_worker_bindings_v8(
            self,
            &self.workers,
            &self.worker_messages,
            &self.worker_errors,
            &self.worker_next_id,
            &self.worker_blob_store,
            fp_worker,
        ) {
            eprintln!("v8: worker::install_worker_bindings_v8 failed: {e}");
        }
        if let Err(e) = crate::shared_worker::install_shared_worker_bindings_v8(
            self,
            &self.shared_worker_outbox,
            &self.shared_worker_errors,
            fp_shared_worker,
        ) {
            eprintln!("v8: shared_worker::install_shared_worker_bindings_v8 failed: {e}");
        }
        // BUG-378: must be the LAST install step — it hides every internal
        // `_lumen_*` global from enumeration and freezes the function-valued
        // ones, so anything registering a native or patching one afterwards
        // would either stay visible (register) or fail silently (patch). See
        // `internal_globals`'s module docs.
        if let Err(e) = crate::internal_globals::seal_internal_globals_v8(self) {
            eprintln!("v8: internal_globals::seal_internal_globals_v8 failed: {e}");
        }
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
