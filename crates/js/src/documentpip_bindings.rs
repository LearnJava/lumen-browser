//! Native Document Picture-in-Picture window bridge (`_lumen_docpip_request_window` /
//! `_lumen_docpip_close` / `_lumen_docpip_set_content_html`).
//!
//! The JS shim in [`document_pip`](crate::document_pip) implements
//! `documentPictureInPicture.requestWindow()` / `PictureInPictureWindow.close()`
//! and calls the native hooks registered here. Each hook pushes a
//! [`DocPipRequest`] onto a process-global queue that the shell drains every
//! event-loop tick (mirrors [`pip_bindings`](crate::pip_bindings) for video
//! PiP) to open/close the real OS-level floating window (slice 1) and to feed
//! it the serialized HTML of the moved DOM subtree (slice 2 — see
//! `document_pip.rs` module docs).
//!
//! # Why a process-global queue
//!
//! Mirrors [`pip_bindings`](crate::pip_bindings): the binding closures have no
//! access to the shell's window state, so they record intent in a `static` the
//! shell owns the draining of.

use std::sync::{Mutex, OnceLock};

/// A Document PiP request emitted by the JS API, awaiting the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocPipRequest {
    /// `documentPictureInPicture.requestWindow(width, height)` — open the OS
    /// floating window at this logical (CSS pixel) size.
    Open {
        /// Requested initial client width.
        width: u32,
        /// Requested initial client height.
        height: u32,
    },
    /// `PictureInPictureWindow.close()` — close the OS floating window.
    Close,
    /// `pipWindow.document.body` was mutated (`appendChild`/`removeChild`/
    /// `innerHTML` setter) — the JS shim re-serialized its hidden content
    /// container and forwards the resulting markup, which the shell parses
    /// into a fresh detached [`lumen_dom::Document`] and lays out/paints into
    /// the floating window on the next redraw.
    SetContent(String),
}

/// Process-global queue of Document PiP requests awaiting the shell's drain.
static QUEUE: OnceLock<Mutex<Vec<DocPipRequest>>> = OnceLock::new();

fn queue() -> &'static Mutex<Vec<DocPipRequest>> {
    QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Enqueue a Document PiP request. Public so non-JS engine paths can reuse the channel.
pub fn enqueue(req: DocPipRequest) {
    queue().lock().unwrap().push(req);
}

/// Drain and return all pending Document PiP requests.
///
/// Called by the shell each event-loop tick; the queue is left empty.
pub fn take_docpip_requests() -> Vec<DocPipRequest> {
    std::mem::take(&mut *queue().lock().unwrap())
}

/// Install the `_lumen_docpip_request_window(width, height)` /
/// `_lumen_docpip_close()` native bindings.
///
/// Must be called after [`document_pip::install_document_pip_api_v8`] so the JS
/// shim that calls these hooks is already present (registration order is
/// otherwise irrelevant — the shim guards each call with `typeof === 'function'`).
///
/// [`document_pip::install_document_pip_api_v8`]: crate::document_pip::install_document_pip_api_v8
#[cfg(feature = "v8-backend")]
pub(crate) fn install_docpip_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn2};

    let request_window = into_v8_fn2(move |width: f64, height: f64| {
        enqueue(DocPipRequest::Open {
            width: width.max(0.0) as u32,
            height: height.max(0.0) as u32,
        });
    });
    rt.register_native("_lumen_docpip_request_window", request_window)?;

    let close = into_v8_fn0(move || {
        enqueue(DocPipRequest::Close);
    });
    rt.register_native("_lumen_docpip_close", close)?;

    let set_content_html = into_v8_fn1(move |html: String| {
        enqueue(DocPipRequest::SetContent(html));
    });
    rt.register_native("_lumen_docpip_set_content_html", set_content_html)?;
    Ok(())
}

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;

    /// Serializes tests: the request queue is process-global, so parallel tests
    /// would otherwise observe each other's enqueues.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Acquire the serialization lock and drain any leftover queue state.
    fn guard() -> std::sync::MutexGuard<'static, ()> {
        let g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = take_docpip_requests();
        g
    }

    fn with_docpip_bindings(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        install_docpip_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn open_enqueues_request() {
        let _g = guard();
        enqueue(DocPipRequest::Open { width: 640, height: 360 });
        let reqs = take_docpip_requests();
        assert_eq!(reqs, vec![DocPipRequest::Open { width: 640, height: 360 }]);
        // Queue is drained.
        assert!(take_docpip_requests().is_empty());
    }

    #[test]
    fn install_registers_all_hooks() {
        let _g = guard();
        with_docpip_bindings(|rt| {
            let ok = rt
                .eval(
                    "typeof _lumen_docpip_request_window === 'function' && \
                     typeof _lumen_docpip_close === 'function' && \
                     typeof _lumen_docpip_set_content_html === 'function'",
                )
                .unwrap();
            assert_eq!(ok, lumen_core::JsValue::Bool(true), "all Document PiP hooks must be installed");
        });
    }

    #[test]
    fn js_request_window_call_reaches_queue() {
        let _g = guard();
        with_docpip_bindings(|rt| {
            rt.eval("_lumen_docpip_request_window(800, 450);").unwrap();
        });
        let reqs = take_docpip_requests();
        assert_eq!(reqs, vec![DocPipRequest::Open { width: 800, height: 450 }]);
    }

    #[test]
    fn js_close_call_reaches_queue() {
        let _g = guard();
        with_docpip_bindings(|rt| {
            rt.eval("_lumen_docpip_close();").unwrap();
        });
        let reqs = take_docpip_requests();
        assert_eq!(reqs, vec![DocPipRequest::Close]);
    }

    #[test]
    fn js_set_content_html_call_reaches_queue() {
        let _g = guard();
        with_docpip_bindings(|rt| {
            rt.eval("_lumen_docpip_set_content_html('<div>hi</div>');").unwrap();
        });
        let reqs = take_docpip_requests();
        assert_eq!(reqs, vec![DocPipRequest::SetContent("<div>hi</div>".to_owned())]);
    }
}
