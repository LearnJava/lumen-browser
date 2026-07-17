//! Programmatic download trigger (`_lumen_network_download`).
//!
//! Lets page scripts and the engine's own `<a download>` / blob-save paths ask
//! the shell to start a background download. The native function
//! `_lumen_network_download(url, filename)` pushes a [`DownloadRequest`] onto a
//! process-global queue; the shell drains it each event-loop tick via
//! [`take_download_requests`] and hands each entry to its
//! `DownloadManager::start_url_download`.
//!
//! # Why a process-global queue
//!
//! Mirrors `clipboard::PROVIDER` / `broadcast_channel::HUB`: the binding has no
//! access to the shell's `DownloadManager`, so it records intent in a `static`
//! that the shell owns the draining of. This avoids threading another `Arc`
//! through `install_primitives`' already-large signature.
//!
//! Downloads are *requests*, not promises — JS gets no completion callback in
//! Phase 1 (matches the `<a download>` fire-and-forget model). Progress and
//! completion are surfaced only in the shell's downloads panel.

use std::sync::{Mutex, OnceLock};

/// A single pending download asked for by JS, awaiting the shell to start it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRequest {
    /// Absolute or page-relative URL to fetch. The shell resolves it against
    /// the active document base URL if it is not absolute.
    pub url: String,
    /// Suggested file name (from the `download` attribute or an explicit JS
    /// argument). `None` when the caller passed an empty string — the shell
    /// then derives a name from the URL path.
    pub filename: Option<String>,
}

/// Process-global queue of download requests awaiting the shell's drain.
static QUEUE: OnceLock<Mutex<Vec<DownloadRequest>>> = OnceLock::new();

fn queue() -> &'static Mutex<Vec<DownloadRequest>> {
    QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Enqueue a download request. Public so non-JS engine paths (e.g. a future
/// native `<a download>` click handler) can reuse the same channel.
pub fn enqueue(url: String, filename: Option<String>) {
    queue().lock().unwrap().push(DownloadRequest { url, filename });
}

/// Drain and return all pending download requests.
///
/// Called by the shell each event-loop tick; the queue is left empty.
pub fn take_download_requests() -> Vec<DownloadRequest> {
    std::mem::take(&mut *queue().lock().unwrap())
}

/// Install the `_lumen_network_download(url, filename)` native binding (V8).
///
/// `filename` is optional on the JS side; callers pass `''` (or omit it via the
/// JS shim) when they have no suggested name. An empty/whitespace URL is
/// ignored so a stray call cannot enqueue a junk entry. The native goes through
/// the compat layer (`into_v8_fn2` + `register_native`); the convenience shim
/// (`_lumen_download`, tolerating a missing second argument) evaluates unchanged.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_download_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::into_v8_fn2;
    use lumen_core::ext::JsRuntime as _;

    let native = into_v8_fn2(move |url: String, filename: String| {
        let url = url.trim();
        if url.is_empty() {
            return;
        }
        let filename = filename.trim();
        let filename = if filename.is_empty() {
            None
        } else {
            Some(filename.to_string())
        };
        enqueue(url.to_string(), filename);
    });
    rt.register_native("_lumen_network_download", native)?;
    rt.eval(
        "globalThis._lumen_download = function(url, name) { \
           _lumen_network_download(String(url), name == null ? '' : String(name)); \
         };",
    )?;
    Ok(())
}

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use std::sync::{Mutex, MutexGuard};

    /// Serializes tests: the request queue is process-global, so parallel
    /// tests would otherwise observe each other's enqueues.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn guard() -> MutexGuard<'static, ()> {
        let g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = take_download_requests();
        g
    }

    fn runtime() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        install_download_bindings_v8(&rt).unwrap();
        rt
    }

    #[test]
    fn enqueue_and_take_roundtrips() {
        let _g = guard();
        enqueue("https://example.com/a.bin".into(), Some("a.bin".into()));
        let reqs = take_download_requests();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].url, "https://example.com/a.bin");
        assert_eq!(reqs[0].filename.as_deref(), Some("a.bin"));
    }

    #[test]
    fn take_clears_queue() {
        let _g = guard();
        enqueue("https://example.com/x".into(), None);
        assert_eq!(take_download_requests().len(), 1);
        assert_eq!(take_download_requests().len(), 0);
    }

    #[test]
    fn js_call_enqueues() {
        let _g = guard();
        let rt = runtime();
        rt.eval("_lumen_network_download('https://h/file.zip', 'file.zip')")
            .unwrap();
        let reqs = take_download_requests();
        assert!(
            reqs.iter()
                .any(|r| r.url == "https://h/file.zip" && r.filename.as_deref() == Some("file.zip"))
        );
    }

    #[test]
    fn js_empty_filename_becomes_none() {
        let _g = guard();
        let rt = runtime();
        rt.eval("_lumen_network_download('https://h/noname', '')")
            .unwrap();
        let reqs = take_download_requests();
        let r = reqs.iter().find(|r| r.url == "https://h/noname").unwrap();
        assert_eq!(r.filename, None);
    }

    #[test]
    fn js_blank_url_ignored() {
        let _g = guard();
        let rt = runtime();
        rt.eval("_lumen_network_download('   ', 'x')").unwrap();
        assert!(take_download_requests().is_empty());
    }

    #[test]
    fn shim_tolerates_missing_name() {
        let _g = guard();
        let rt = runtime();
        rt.eval("_lumen_download('https://h/shimmed')").unwrap();
        let reqs = take_download_requests();
        let r = reqs.iter().find(|r| r.url == "https://h/shimmed").unwrap();
        assert_eq!(r.filename, None);
    }
}
