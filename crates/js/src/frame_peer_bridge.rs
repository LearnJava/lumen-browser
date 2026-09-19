//! BUG-979 — synchronous cross-isolate access to a frame's REAL global object.
//!
//! `frame_bridge.rs`'s `winFacade` used to be a fixed ~11-name IDL whitelist:
//! any global the framed document's own script declared itself (a function, a
//! plain variable) was unreachable through `contentWindow.foo`/named-window
//! access, even same-origin. Every OTHER cross-isolate operation in this
//! bridge (`postMessage`, `click`/`focus`/`dispatchEvent`, inserted-`<script>`
//! execution, resource-event mirroring — see `frame_bridge.rs` module doc,
//! срезы 4/6-10) is deliberately ASYNCHRONOUS: an envelope in a mailbox,
//! delivered on the recipient's own next pump tick. That is a documented,
//! repeated design invariant — and it structurally cannot give
//! `contentWindow.foo`/`someIframeName.foo` spec same-origin semantics
//! (HTML LS: an ordinary, synchronous property read/call), which is exactly
//! what the WPT case behind this bug needs (`testWindow.prepareForTest(...)`
//! used synchronously, its result read the same turn).
//!
//! This is a deliberate, user-approved exception to that invariant (decided
//! 2026-09-19): a blocking cross-thread call into the peer frame's own
//! `V8JsRuntime`. Each runtime already lives on its own thread and already
//! accepts blocking cross-thread jobs — `V8JsRuntime::run`/`eval`/
//! `get_global`/`call_function` (`v8_runtime/eval.rs`) all block the calling
//! thread until the target thread's job completes. This trait is a thin,
//! object-safe wrapper around that existing channel so `frame_bridge.rs`
//! (which knows nothing about the concrete `V8JsRuntime` type) can hold a
//! peer's handle in [`crate::frame_bridge::FrameDocBinding`].
//!
//! Reentrancy is the one real risk a fully async design avoided: if frame A
//! synchronously calls a global function of frame B, and that function body
//! synchronously calls a global of frame A right back, both threads block on
//! each other's `run()` forever. [`enter_call`] guards against exactly that
//! cycle — a call from A into B is refused (not blocked) while the reverse
//! edge B→A is already open, using the same `Arc<Mutex<Document>>` pointer
//! identity `frame_bridge.rs` already keys `FRAME_OUTBOX`/`self_key` by.

use lumen_core::JsValue;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

/// Handle for a synchronous cross-isolate call into a peer frame's real
/// `globalThis`. Implemented once, by `V8JsRuntime`
/// (`v8_runtime/eval.rs`) — kept as a trait so `frame_bridge.rs` doesn't have
/// to depend on the concrete `v8_runtime` module.
///
/// Both methods return an envelope `JsValue::Object` rather than a
/// `JsResult`: the native-function glue in `frame_bridge.rs` has no path to
/// turn a Rust `Err` into a thrown JS exception (`v8_compat.rs`'s
/// `into_v8_fnN` always wraps the closure's return in `Ok`), so failure is
/// encoded as data instead and the JS shim throws it itself. Shape:
/// `{"kind":"value","value":V}` | `{"kind":"function"}` |
/// `{"kind":"absent"}` | `{"kind":"error","message":M}`.
pub trait FramePeerBridge: Send + Sync {
    /// `globalThis[name]` in the peer context.
    fn peer_global_get(&self, name: &str) -> JsValue;
    /// `globalThis[name](...args)` in the peer context.
    fn peer_global_call(&self, name: &str, args: &[JsValue]) -> JsValue;
}

/// `{"kind": tag}` with no other fields.
pub(crate) fn envelope_tag(tag: &str) -> JsValue {
    JsValue::object([("kind".to_owned(), JsValue::String(tag.to_owned()))])
}

/// `{"kind": "value", "value": v}`.
pub(crate) fn envelope_value(v: JsValue) -> JsValue {
    JsValue::object([
        ("kind".to_owned(), JsValue::String("value".to_owned())),
        ("value".to_owned(), v),
    ])
}

/// `{"kind": "error", "message": msg}`.
pub(crate) fn envelope_error(msg: impl Into<String>) -> JsValue {
    JsValue::object([
        ("kind".to_owned(), JsValue::String("error".to_owned())),
        ("message".to_owned(), JsValue::String(msg.into())),
    ])
}

fn in_flight_edges() -> &'static Mutex<HashSet<(usize, usize)>> {
    static EDGES: OnceLock<Mutex<HashSet<(usize, usize)>>> = OnceLock::new();
    EDGES.get_or_init(|| Mutex::new(HashSet::new()))
}

/// RAII guard for one synchronous cross-frame call. `from`/`to` are the
/// caller's/target's own-document pointer identity (`Arc::as_ptr(&doc) as
/// usize` — the same identity `frame_bridge.rs` already uses for
/// `self_key`/`FRAME_OUTBOX`). Held for the duration of the blocking call;
/// dropped (edge removed) once it returns, however it returns.
pub(crate) struct CallGuard {
    from: usize,
    to: usize,
}

impl Drop for CallGuard {
    fn drop(&mut self) {
        in_flight_edges()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&(self.from, self.to));
    }
}

/// Open the `from -> to` edge, refusing (instead of blocking forever) if the
/// reverse edge `to -> from` is already open — the direct A-calls-B-calls-A
/// reentrancy cycle. `from == to` (a frame calling into itself through its
/// own facade) is allowed: it is a single thread re-entering its own
/// `run()`, which is an existing hazard of that channel on its own (not
/// something this guard can fix), not a new one this guard needs to police.
pub(crate) fn enter_call(from: usize, to: usize) -> Result<CallGuard, JsValue> {
    let mut edges = in_flight_edges().lock().unwrap_or_else(|e| e.into_inner());
    if edges.contains(&(to, from)) {
        return Err(envelope_error(
            "cross-frame call refused: would deadlock (reentrant call cycle)",
        ));
    }
    edges.insert((from, to));
    Ok(CallGuard { from, to })
}
