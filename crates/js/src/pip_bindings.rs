//! Native Picture-in-Picture window bridge (`_lumen_pip_enter` / `_lumen_pip_exit`
//! / `_lumen_pip_request_window`).
//!
//! The JS shim in [`video_pip`](crate::video_pip) implements the W3C
//! Picture-in-Picture Level 1 API and, on `video.requestPictureInPicture()` /
//! `document.exitPictureInPicture()`, calls the native hooks
//! `_lumen_pip_enter(nid)` / `_lumen_pip_exit(nid)`.
//! [`document_pip`](crate::document_pip)'s Document Picture-in-Picture shim
//! calls `_lumen_pip_request_window(width, height)` from
//! `documentPictureInPicture.requestWindow()` — there is no source element, so
//! it carries the requested size instead of a node id. This module registers
//! all three hooks: each pushes a [`PipRequest`] onto a process-global queue
//! that the shell drains every event-loop tick via [`take_pip_requests`] to
//! open or close the real OS-level floating window (CC-7, P3-pip).
//!
//! # Why a process-global queue
//!
//! Mirrors [`download_bindings`](crate::download_bindings): the binding closures
//! have no access to the shell's window state, so they record intent in a
//! `static` the shell owns the draining of — no extra `Arc` threaded through the
//! already-large `install_primitives` signature.

use std::sync::{Mutex, OnceLock};

/// A picture-in-picture request emitted by the JS PiP API, awaiting the shell.
///
/// No `Eq` — `OpenDocument`'s `f32` dimensions aren't totally ordered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PipRequest {
    /// `video.requestPictureInPicture()` — open / re-target the OS floating
    /// window for the `<video>` element with this node index.
    Enter {
        /// Node index of the `<video>` entering picture-in-picture.
        nid: u32,
    },
    /// `document.exitPictureInPicture()` — close the OS floating window.
    ///
    /// `nid` echoes the leaving element (informational; only one element can be
    /// in PiP at a time, so the shell tears down whichever window is open).
    Exit {
        /// Node index of the `<video>` leaving picture-in-picture.
        nid: u32,
    },
    /// `documentPictureInPicture.requestWindow({width, height})` — open a real
    /// OS floating window for Document Picture-in-Picture. Unlike [`Self::Enter`]
    /// there is no source `<video>`; the window shows a plain sized container
    /// (DOM-content forwarding is a follow-up, see `docs/tasks/ph3-picture-in-picture.md`).
    OpenDocument {
        /// Requested window width in CSS pixels.
        width: f32,
        /// Requested window height in CSS pixels.
        height: f32,
    },
}

/// Process-global queue of PiP requests awaiting the shell's drain.
static QUEUE: OnceLock<Mutex<Vec<PipRequest>>> = OnceLock::new();

fn queue() -> &'static Mutex<Vec<PipRequest>> {
    QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Enqueue a PiP request. Public so non-JS engine paths can reuse the channel.
pub fn enqueue(req: PipRequest) {
    queue().lock().unwrap().push(req);
}

/// Drain and return all pending PiP requests.
///
/// Called by the shell each event-loop tick; the queue is left empty.
pub fn take_pip_requests() -> Vec<PipRequest> {
    std::mem::take(&mut *queue().lock().unwrap())
}

/// Install the `_lumen_pip_enter(nid)` / `_lumen_pip_exit(nid)` /
/// `_lumen_pip_request_window(width, height)` native bindings (Ph3 V8 migration
/// S5-S7 batch 2, rquickjs path removed in S12b-9; `_lumen_pip_request_window`
/// added for Document Picture-in-Picture, P3-pip): all three natives go through
/// the compat layer; `Opt<u32>` becomes `Option<u32>` (same "missing/null →
/// None" semantics via the compat layer's `FromJsValue`).
///
/// Must be called after [`video_pip::install_video_pip_api`] / `document_pip`'s
/// installer so the JS shims that call these hooks are already present
/// (registration order is otherwise irrelevant — the shims guard each call with
/// `typeof === 'function'`).
///
/// [`video_pip::install_video_pip_api`]: crate::video_pip::install_video_pip_api
#[cfg(feature = "v8-backend")]
pub(crate) fn install_pip_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn1, into_v8_fn2};

    let enter = into_v8_fn1(move |nid: u32| {
        enqueue(PipRequest::Enter { nid });
    });
    rt.register_native("_lumen_pip_enter", enter)?;

    let exit = into_v8_fn1(move |nid: Option<u32>| {
        enqueue(PipRequest::Exit { nid: nid.unwrap_or(0) });
    });
    rt.register_native("_lumen_pip_exit", exit)?;

    // Document Picture-in-Picture (`documentPictureInPicture.requestWindow`,
    // `document_pip.rs`) — no source element, so the request carries the
    // requested window size instead of a node id.
    let request_window = into_v8_fn2(move |width: f64, height: f64| {
        enqueue(PipRequest::OpenDocument {
            width: width as f32,
            height: height as f32,
        });
    });
    rt.register_native("_lumen_pip_request_window", request_window)?;
    Ok(())
}

/// Serializes tests that touch the process-global [`PipRequest`] queue.
///
/// Shared beyond this module: `document_pip.rs`'s tests also enqueue through
/// `_lumen_pip_request_window` and would otherwise race with the tests below
/// when `cargo test` runs both files' test threads in parallel.
#[cfg(test)]
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the serialization lock and drain any leftover queue state.
#[cfg(test)]
pub(crate) fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    let g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = take_pip_requests();
    g
}

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;

    use super::test_guard as guard;

    fn with_pip_bindings(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        install_pip_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn enter_enqueues_request() {
        let _g = guard();
        enqueue(PipRequest::Enter { nid: 5 });
        let reqs = take_pip_requests();
        assert_eq!(reqs, vec![PipRequest::Enter { nid: 5 }]);
        // Queue is drained.
        assert!(take_pip_requests().is_empty());
    }

    #[test]
    fn install_registers_both_hooks() {
        let _g = guard();
        with_pip_bindings(|rt| {
            let ok = rt
                .eval(
                    "typeof _lumen_pip_enter === 'function' && \
                     typeof _lumen_pip_exit === 'function' && \
                     typeof _lumen_pip_request_window === 'function'",
                )
                .unwrap();
            assert_eq!(ok, lumen_core::JsValue::Bool(true), "all three PiP hooks must be installed");
        });
    }

    #[test]
    fn js_request_window_call_reaches_queue() {
        let _g = guard();
        with_pip_bindings(|rt| {
            rt.eval("_lumen_pip_request_window(800, 450);").unwrap();
        });
        let reqs = take_pip_requests();
        assert_eq!(reqs, vec![PipRequest::OpenDocument { width: 800.0, height: 450.0 }]);
    }

    #[test]
    fn js_enter_call_reaches_queue() {
        let _g = guard();
        with_pip_bindings(|rt| {
            rt.eval("_lumen_pip_enter(42);").unwrap();
        });
        let reqs = take_pip_requests();
        assert_eq!(reqs, vec![PipRequest::Enter { nid: 42 }]);
    }

    #[test]
    fn js_exit_call_tolerates_missing_arg() {
        let _g = guard();
        with_pip_bindings(|rt| {
            // Exit may be called with or without an explicit node id.
            rt.eval("_lumen_pip_exit();").unwrap();
            rt.eval("_lumen_pip_exit(7);").unwrap();
        });
        let reqs = take_pip_requests();
        assert_eq!(
            reqs,
            vec![PipRequest::Exit { nid: 0 }, PipRequest::Exit { nid: 7 }]
        );
    }
}
