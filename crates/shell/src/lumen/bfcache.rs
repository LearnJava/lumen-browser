//! The back/forward cache: deciding whether a page may be kept, keeping a live
//! page whole, and putting a kept page back.
//!
//! Two mechanisms live side by side. A page with a JS runtime is *parked*
//! whole (`park_current_page`/`restore_parked_page`, BUG-835) — its timers and
//! listeners simply stop being pumped and resume where they left off. A page
//! without one is *frozen* into a serialised DOM and thawed back
//! (`bfcache_thaw`), which reinstalls a fresh runtime and therefore keeps no
//! script state. `bfcache_eligible` is what HTML LS §8.6 lets either path take.

use crate::*;

impl Lumen {
    /// Whether the current page may be stored as a full bfcache freeze.
    ///
    /// `false` when the page has an open WebSocket/EventSource connection, a
    /// registered `unload`/`beforeunload` handler ([`PersistentJs::has_bfcache_freeze_blocker`]),
    /// or the response carried `Cache-Control: no-store` (HTML LS §8.6).
    /// Ineligible pages fall back to the existing HTML-snapshot bfcache path
    /// (no regression).
    pub(crate) fn bfcache_eligible(&self) -> bool {
        let no_store = self
            .layout_source
            .as_ref()
            .is_some_and(|ls| ls.cache_control_no_store);
        if no_store {
            return false;
        }
        !route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
            j.has_bfcache_freeze_blocker()
        })
        .unwrap_or(false)
    }

    /// Thaw a frozen page — restore DOM + stylesheet, reinstall a fresh JS runtime
    /// (heap resume gated on 10C.2), re-layout, restore scroll/title, fire
    /// pageshow(persisted=true). Returns false when DOM bytes fail to decode or
    /// the stylesheet was evicted (caller falls back to a normal reload).
    pub(crate) fn bfcache_thaw(&mut self, entry: &BfCacheEntry, frozen: &FrozenPage) -> bool {
        let url = entry.url.as_str();
        let Some(stylesheet) = self.frozen_styles.get(url).cloned() else {
            return false;
        };
        let Ok(doc) = Document::from_bytes(&frozen.dom_bytes) else {
            return false;
        };
        let doc_arc = Arc::new(Mutex::new(doc));
        self.layout_source = Some(LayoutSource {
            document: Arc::clone(&doc_arc),
            stylesheet: Arc::new(stylesheet),
            html_source: None,
            // The page was eligible for a full freeze (bfcache_eligible() was
            // true when it was stored), so it was not no-store at that point.
            cache_control_no_store: false,
            // BUG-743: the frozen entry keeps the parsed sheet, not the CSS
            // parts it was built from — nothing to rebuild a cascade out of.
            dynamic_css: None,
        });
        // Ph3 V8 migration S4.
        #[cfg(feature = "v8")]
        {
            match lumen_js::v8_runtime::V8JsRuntime::new() {
                Ok(mut rt) => {
                    // BUG-548 (S12b-G6): cookie-banner dismiss now wired for V8.
                    rt.set_cookie_banner_dismiss(self.cookie_banner_dismiss);
                    if self.deterministic.enabled {
                        rt.set_deterministic_mode(true, self.deterministic.rng_seed, self.deterministic.monotonic_clock);
                    }
                    let ls_store = self
                        .source
                        .origin_str()
                        .and_then(|o| self.ls_storage.get(&o).cloned());
                    // BUG-836: a page thawed out of the bfcache is a document of
                    // this tab like any other — it must see the tab's store.
                    let ss_store = self.source.origin_str().map(|o| {
                        Arc::clone(self.ss_storage.entry(o).or_insert_with(|| {
                            Arc::new(std::sync::Mutex::new(lumen_core::WebStorage::default()))
                        }))
                    });
                    if let Some(store) = ss_store {
                        rt = rt.with_session_storage(store);
                    }
                    let idb_backend = self.idb_dir.as_deref().and_then(|d| idb_store_for_url(url, Some(d)));
                    let fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>> = None;
                    let ws_provider: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>> = None;
                    let sse_provider: Option<Arc<dyn lumen_core::ext::JsSseProvider>> = None;
                    let sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>> = None;
                    let cache_backend: Option<Arc<dyn lumen_core::ext::CacheBackend>> = None;
                    if let Err(e) = rt.install_dom(
                        Arc::clone(&doc_arc),
                        url,
                        fetch_provider,
                        ws_provider,
                        sse_provider,
                        ls_store,
                        idb_backend,
                        sw_backend,
                        cache_backend,
                        None,
                        false,
                    ) {
                        eprintln!("bfcache thaw: JS DOM init failed: {e}");
                    }
                    self.set_js_ctx(Some(Arc::new(V8PersistentJs { rt }) as Arc<dyn PersistentJs>));
                }
                Err(e) => {
                    eprintln!("bfcache thaw: V8 init failed: {e}");
                    self.set_js_ctx(None);
                }
            }
        }
        #[cfg(not(feature = "v8"))]
        {
            self.set_js_ctx(None);
        }
        // ADR-016 M2.2c-2b: зеркалим восстановленный (или сброшенный) хэндл + DOM
        // в движковый поток после bfcache-thaw.
        self.sync_engine_js_state();
        self.relayout();
        self.scroll_x = entry.scroll_x;
        self.scroll_y = entry.scroll_y;
        self.title = entry.title.clone();
        if let Some(w) = self.window.as_ref() {
            w.set_title(&window_title(self.title.as_deref()));
        }
        // ADR-016 M2.2d: fire-and-forget eval via route_eval_js (off-UI-thread
        // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
        route_eval_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            "_lumen_fire_page_lifecycle('pageshow', true)".to_string(),
        );
        self.request_redraw();
        self.commit_nav_state();
        true
    }

    /// Park the current page whole — JS runtime included — so a later
    /// back/forward navigation can restore a *live* document (BUG-835).
    ///
    /// Only the handles are cloned: `js_ctx` and `layout_source` stay in place
    /// until the incoming navigation replaces them, so nothing about the page
    /// being navigated away from changes here. From the moment the shell swaps
    /// in the next page's handle, the parked runtime stops being pumped —
    /// `route_task_js`/`route_query_js` reach only the active one — which is
    /// what pauses its timers and rAF callbacks for the duration of the park.
    ///
    /// Returns `false` (and parks nothing) for a page with no JS runtime or no
    /// layout source; those go down the frozen-DOM path instead, where
    /// reinstalling a fresh runtime loses nothing.
    pub(crate) fn park_current_page(&mut self) -> bool {
        let Some(url) = self.source.url_str().map(str::to_owned) else {
            return false;
        };
        let Some(js) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            Arc::clone,
        ) else {
            return false;
        };
        let Some(ls) = self.layout_source.as_ref() else {
            return false;
        };
        let parked = ParkedPage {
            js,
            document: Arc::clone(&ls.document),
            stylesheet: Arc::clone(&ls.stylesheet),
            html_source: ls.html_source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            title: self.title.clone(),
        };
        // One entry per URL: re-parking the same page replaces the older copy.
        self.parked_pages.retain(|(u, _)| *u != url);
        self.parked_pages.push((url, parked));
        while self.parked_pages.len() > PARKED_PAGES_MAX {
            self.parked_pages.remove(0);
        }
        true
    }

    /// Whether a live page is parked for `url` — see [`Self::park_current_page`].
    pub(crate) fn has_parked_page(&self, url: &str) -> bool {
        self.parked_pages.iter().any(|(u, _)| u == url)
    }

    /// Restore a page parked by [`Self::park_current_page`]: put its DOM,
    /// stylesheet and JS runtime back into the active slot, re-lay out and fire
    /// `pageshow(persisted=true)`.
    ///
    /// Unlike [`Self::bfcache_thaw`] the runtime is the page's own, so its
    /// timers, listeners and closures resume exactly where the park left them.
    /// The caller must have set `self.source` to the page being restored first —
    /// `relayout`/`commit_nav_state` read it.
    pub(crate) fn restore_parked_page(&mut self, url: &str) -> bool {
        let Some(pos) = self.parked_pages.iter().position(|(u, _)| u == url) else {
            return false;
        };
        let (_, parked) = self.parked_pages.remove(pos);
        // Same order as `apply_loaded_page`: drop the outgoing handle before the
        // layout source it shares a `Document` with, then install the new pair.
        self.set_js_ctx(None);
        self.layout_source = Some(LayoutSource {
            document: Arc::clone(&parked.document),
            stylesheet: parked.stylesheet,
            html_source: parked.html_source,
            // Restored pages have no live response headers; treated as cacheable,
            // exactly as `bfcache_thaw` does.
            cache_control_no_store: false,
            // BUG-743: the CSS parts the sheet was built from are not kept.
            dynamic_css: None,
        });
        self.set_js_ctx(Some(parked.js));
        self.sync_engine_js_state();
        self.relayout();
        self.scroll_x = parked.scroll_x;
        self.scroll_y = parked.scroll_y;
        self.title = parked.title.clone();
        if let Some(w) = self.window.as_ref() {
            w.set_title(&window_title(self.title.as_deref()));
        }
        route_eval_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            "_lumen_fire_page_lifecycle('pageshow', true)".to_string(),
        );
        self.request_redraw();
        self.commit_nav_state();
        true
    }
}
