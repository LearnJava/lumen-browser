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

use rquickjs::{Ctx, Function};
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

/// Install the `_lumen_network_download(url, filename)` native binding.
///
/// `filename` is optional on the JS side; callers pass `''` (or omit it via the
/// JS shim) when they have no suggested name. An empty/whitespace URL is
/// ignored so a stray call cannot enqueue a junk entry.
pub fn install_download_bindings(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let f = Function::new(ctx.clone(), move |url: String, filename: String| {
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
    })?;
    ctx.globals().set("_lumen_network_download", f)?;
    // Convenience shim: tolerate a missing second argument.
    ctx.eval::<(), _>(
        "globalThis._lumen_download = function(url, name) { \
           _lumen_network_download(String(url), name == null ? '' : String(name)); \
         };",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuickJsRuntime;
    use lumen_core::JsRuntime;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex, MutexGuard};

    /// Serializes tests: the request queue is process-global, so parallel
    /// tests would otherwise observe each other's enqueues.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn guard() -> MutexGuard<'static, ()> {
        let g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = take_download_requests();
        g
    }

    fn runtime() -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "", None, None, None, None, None, None)
            .unwrap();
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
