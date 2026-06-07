//! T3 hibernation serialise/restore primitives (ADR-008 §10J, roadmap task #17).
//!
//! When a background tab ages into `TabState::Hibernated` the shell drops its
//! live `PersistentJs` runtime (freeing the QuickJS heap) and serialises the
//! DOM via `Document::to_bytes()` to SQLite.  On the next switch back to the
//! tab the DOM is reconstructed via `Document::from_bytes()` and a **fresh**
//! `PersistentJs` runtime is built so event handlers and `window.fetch` work
//! again — the JS heap itself cannot be serialised, so the page's inline
//! `<script>` blocks are simply re-run against the restored DOM.
//!
//! This module owns the two cross-cutting pieces that the `Lumen` methods in
//! `main.rs` plug into:
//!  - [`resource_base_from_url`] — rebuild a [`crate::ResourceBase`] from the
//!    persisted page URL (no live navigation context survives hibernation).
//!  - [`restore_js_context`] — rebuild the per-origin storage + network
//!    providers and run the inline scripts, returning the shared
//!    `Arc<Mutex<Document>>` plus the new `PersistentJs`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use lumen_core::ext::EventSink;
use lumen_dom::Document;

use crate::{PersistentJs, ResourceBase};

/// Rebuild a [`crate::ResourceBase`] from a persisted page URL string.
///
/// Hibernated tabs only retain the page URL (not the live `PageSource`), so the
/// resolution base for `window.location`, sub-resource fetches and origin-keyed
/// storage must be reconstructed from that string.
///
/// `file://` URLs map to [`ResourceBase::File`] with the decoded filesystem
/// path; everything else (http/https and the empty string) maps to
/// [`ResourceBase::Url`].  The empty string yields `ResourceBase::Url("")`,
/// for which all origin-keyed helpers return `None` (no storage, no network),
/// matching the behaviour of a script-less `about:blank`-style page.
pub(crate) fn resource_base_from_url(url: &str) -> ResourceBase {
    if let Some(rest) = url.strip_prefix("file://") {
        ResourceBase::File(PathBuf::from(rest))
    } else {
        ResourceBase::Url(url.to_owned())
    }
}

/// Rebuild the JS runtime for a tab being restored from T3 hibernation.
///
/// Reconstructs the per-origin localStorage / IndexedDB / Service Worker
/// persistence handles and the network (`fetch` / `WebSocket`) providers for
/// `url`, then runs the document's inline `<script>` blocks through a fresh
/// QuickJS runtime via [`crate::run_scripts_with_dom`].
///
/// `doc` is taken by value because `run_scripts_with_dom` wraps it in the
/// `Arc<Mutex<Document>>` shared between JS closures and the layout tree; that
/// same `Arc` is returned here so the caller can build the `LayoutSource` from
/// it (the runtime must observe the *same* document the layout sees).
///
/// Returns `(doc_arc, js_ctx)`.  `js_ctx` is `None` when the page has no inline
/// scripts, scripts are sandboxed away, or the `quickjs` feature is disabled.
/// `DOMContentLoaded` is fired on the new runtime so listeners registered
/// during re-execution still observe the standard lifecycle event.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn restore_js_context(
    url: &str,
    doc: Document,
    event_sink: Arc<dyn EventSink>,
    ls_storage: &mut HashMap<String, Arc<Mutex<lumen_core::WebStorage>>>,
    idb_dir: Option<&std::path::Path>,
    sw_backend: &Arc<Mutex<dyn lumen_core::ext::StorageBackend>>,
    cookie_banner_dismiss: bool,
    deterministic: bool,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> (Arc<Mutex<Document>>, Option<Box<dyn PersistentJs>>) {
    let base = resource_base_from_url(url);

    // Per-origin persistence + network providers, identical to a fresh load.
    let ls_store = crate::ls_store_for_base(&base, ls_storage);
    let idb = crate::idb_store_for_base(&base, idb_dir);
    let sw = crate::sw_store_for_base(&base, sw_backend);
    let (fetch_provider, ws_provider, sse_provider) = match &base {
        ResourceBase::Url(_) => {
            let client = base.http_client_for_subresource(event_sink, cookie_jar);
            let arc_client = Arc::new(client);
            let fp: Option<Arc<dyn lumen_core::ext::JsFetchProvider>> =
                Some(Arc::clone(&arc_client) as Arc<dyn lumen_core::ext::JsFetchProvider>);
            let wp: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>> =
                Some(Arc::clone(&arc_client) as Arc<dyn lumen_core::ext::JsWebSocketProvider>);
            let sp: Option<Arc<dyn lumen_core::ext::JsSseProvider>> =
                Some(arc_client as Arc<dyn lumen_core::ext::JsSseProvider>);
            (fp, wp, sp)
        }
        ResourceBase::File(_) => (None, None, None),
    };

    let (doc_arc, _nav, js_ctx) = crate::run_scripts_with_dom(
        doc,
        lumen_core::SandboxFlags::empty(),
        url,
        fetch_provider,
        ws_provider,
        sse_provider,
        ls_store,
        idb,
        sw,
        cookie_banner_dismiss,
        deterministic,
    );

    // HTML LS §8.2.3: signal DOMContentLoaded so handlers attached during
    // re-execution observe the standard lifecycle on the restored page.
    #[cfg(feature = "quickjs")]
    if let Some(js) = &js_ctx {
        js.notify_dom_content_loaded();
    }

    (doc_arc, js_ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_url_maps_to_url_base() {
        let base = resource_base_from_url("https://example.com/page");
        match base {
            ResourceBase::Url(u) => assert_eq!(u, "https://example.com/page"),
            ResourceBase::File(_) => panic!("expected Url base"),
        }
    }

    #[test]
    fn file_url_maps_to_file_base() {
        let base = resource_base_from_url("file:///C:/tmp/page.html");
        match base {
            ResourceBase::File(p) => assert_eq!(p, PathBuf::from("/C:/tmp/page.html")),
            ResourceBase::Url(_) => panic!("expected File base"),
        }
    }

    #[test]
    fn empty_url_maps_to_empty_url_base() {
        // about:blank-style hibernated tab: no origin, no providers.
        let base = resource_base_from_url("");
        match base {
            ResourceBase::Url(u) => assert!(u.is_empty()),
            ResourceBase::File(_) => panic!("expected Url base"),
        }
    }
}
