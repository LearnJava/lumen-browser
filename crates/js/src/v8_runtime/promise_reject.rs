//! Отчёт о непойманных отказах промисов (BUG-716, BUG-918).
//!
//! Выделено из `v8_runtime.rs` батчем SPLIT-JS7 без изменений поведения.

use super::*;

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
pub(super) fn install_promise_reject_hook(isolate: &mut v8::Isolate) {
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
pub(super) fn drain_promise_rejections(inner: &mut V8Inner) {
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
