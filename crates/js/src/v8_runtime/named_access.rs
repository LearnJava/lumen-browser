//! Инициализация платформы V8 и именованный доступ окна (HTML LS §7.3.3).
//!
//! Выделено из `v8_runtime.rs` батчем SPLIT-JS7 без изменений поведения.
//! Здесь живёт единственный ограниченный по времени захват документа в
//! `crates/js` ([`lock_document_bounded`], BUG-794) — перехватчик глобальных
//! имён обязан отказываться, а не блокироваться.

use super::*;

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
pub(super) fn set_named_access_document(doc: &Arc<Mutex<lumen_dom::Document>>) {
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
pub(super) const NAMED_ACCESS_LOCK_BUDGET: std::time::Duration = std::time::Duration::from_millis(20);

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
pub(super) fn lock_document_bounded(
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
pub(super) fn window_named_properties_template<'s>(
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
