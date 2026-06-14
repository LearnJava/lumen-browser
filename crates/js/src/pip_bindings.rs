//! Native Picture-in-Picture window bridge (`_lumen_pip_enter` / `_lumen_pip_exit`).
//!
//! The JS shim in [`video_pip`](crate::video_pip) implements the W3C
//! Picture-in-Picture Level 1 API and, on `video.requestPictureInPicture()` /
//! `document.exitPictureInPicture()`, calls the native hooks
//! `_lumen_pip_enter(nid)` / `_lumen_pip_exit(nid)`. This module registers those
//! hooks: each pushes a [`PipRequest`] onto a process-global queue that the shell
//! drains every event-loop tick via [`take_pip_requests`] to open or close the
//! real OS-level floating window (CC-7).
//!
//! # Why a process-global queue
//!
//! Mirrors [`download_bindings`](crate::download_bindings): the binding closures
//! have no access to the shell's window state, so they record intent in a
//! `static` the shell owns the draining of — no extra `Arc` threaded through the
//! already-large `install_primitives` signature.

use rquickjs::function::Opt;
use rquickjs::{Ctx, Function};
use std::sync::{Mutex, OnceLock};

/// A picture-in-picture request emitted by the JS PiP API, awaiting the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Install the `_lumen_pip_enter(nid)` / `_lumen_pip_exit(nid)` native bindings.
///
/// Must be called after [`video_pip::install_video_pip_api`] so the JS shim that
/// calls these hooks is already present (registration order is otherwise
/// irrelevant — the shim guards each call with `typeof === 'function'`).
///
/// [`video_pip::install_video_pip_api`]: crate::video_pip::install_video_pip_api
pub fn install_pip_bindings(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let enter = Function::new(ctx.clone(), move |nid: u32| {
        enqueue(PipRequest::Enter { nid });
    })?;
    ctx.globals().set("_lumen_pip_enter", enter)?;

    let exit = Function::new(ctx.clone(), move |nid: Opt<u32>| {
        enqueue(PipRequest::Exit { nid: nid.0.unwrap_or(0) });
    })?;
    ctx.globals().set("_lumen_pip_exit", exit)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};
    use std::sync::MutexGuard;

    /// Serializes tests: the request queue is process-global, so parallel tests
    /// would otherwise observe each other's enqueues.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Acquire the serialization lock and drain any leftover queue state.
    fn guard() -> MutexGuard<'static, ()> {
        let g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = take_pip_requests();
        g
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
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_pip_bindings(&ctx).unwrap();
            let ok: bool = ctx
                .eval(
                    "typeof _lumen_pip_enter === 'function' && \
                     typeof _lumen_pip_exit === 'function'",
                )
                .unwrap();
            assert!(ok, "both PiP hooks must be installed");
        });
    }

    #[test]
    fn js_enter_call_reaches_queue() {
        let _g = guard();
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_pip_bindings(&ctx).unwrap();
            ctx.eval::<(), _>("_lumen_pip_enter(42);").unwrap();
        });
        let reqs = take_pip_requests();
        assert_eq!(reqs, vec![PipRequest::Enter { nid: 42 }]);
    }

    #[test]
    fn js_exit_call_tolerates_missing_arg() {
        let _g = guard();
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            install_pip_bindings(&ctx).unwrap();
            // Exit may be called with or without an explicit node id.
            ctx.eval::<(), _>("_lumen_pip_exit();").unwrap();
            ctx.eval::<(), _>("_lumen_pip_exit(7);").unwrap();
        });
        let reqs = take_pip_requests();
        assert_eq!(
            reqs,
            vec![PipRequest::Exit { nid: 0 }, PipRequest::Exit { nid: 7 }]
        );
    }
}
