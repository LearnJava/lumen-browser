//! Navigating the tab: forward to a new page, back and forward through the
//! session history, and the multi-step traversal `history.go(n)` asks for.
//!
//! Each of these paths runs the same three parts in order: the outgoing page's
//! unload sequence and its bid for the back/forward cache ([`super::bfcache`]),
//! the shuffle of the `nav_back`/`nav_fwd` stacks, and the incoming page's
//! load — finishing at the Navigation API notifications in
//! [`super::nav_state`].

use crate::*;

impl Lumen {
    /// Сохранить текущую страницу в bfcache и стек навигации,
    /// затем загрузить `source` как новую страницу.
    /// Очищает `nav_fwd` (аналог браузера при навигации вперёд из середины истории).
    pub(crate) fn navigate_to(&mut self, source: PageSource) {
        // ADR-016 M2.2c-2d: nav dispatch (fire-and-forget) через `route_task_js` +
        // read-after-eval intercept-чтение через `route_query_js`. Под флагом
        // (`LUMEN_ENGINE_THREAD=1`) dispatch уходит off-UI-thread одним `task`, а
        // блокирующий `query` встаёт в очередь **после** него — read-after-eval
        // порядок сохранён; без флага — прежние синхронные вызовы, байт-идентично.
        {
            let url = source.url_str().unwrap_or("").to_string();
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                j.eval_js(&format!("_lumen_dispatch_navigate('push', '{url}', true, false)"));
            });
        }
        if let Some(intercept) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            |j| j.take_nav_intercept_result(),
        ) {
            if let Some(&(true, false)) = intercept.last() {
                self.pending_intercepted = Some(PendingIntercepted::Push {
                    url: source.url_str().unwrap_or("").to_string(),
                    handler_started: false,
                });
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                    j.eval_js("_lumen_run_navigate_handler()");
                });
                if let Some(PendingIntercepted::Push { handler_started, .. }) =
                    self.pending_intercepted.as_mut()
                {
                    *handler_started = true;
                }
                return;
            }
            if let Some(&(false, true)) = intercept.last() {
                self.fire_navigate_error();
                return;
            }
        }
        click_log::log_nav(&source.describe());
        // PERF-6: remember the page under navigation so a panic on any thread can
        // be attributed to it in the health journal.
        health_log::set_current_url(&source.describe());
        self.hint.close();
        // BUG-835: a page that has a JS runtime is parked *whole* — the runtime
        // goes into `parked_pages` alive, so back/forward restores a document
        // whose timers, listeners and closures still exist. The frozen-DOM path
        // below stays for pages without JS, where reinstalling a fresh runtime
        // over the restored DOM loses nothing.
        let bfcache_eligible = self.bfcache_eligible();
        let mut persisted = bfcache_eligible && self.park_current_page();
        // Phase-3 freeze: serialize live DOM arena + shell-side stylesheet.
        // JS heap suspend is gated on 10C.2, so event handlers are NOT retained.
        // The thaw path reinstalls a fresh runtime over the restored DOM.
        if !persisted
            && bfcache_eligible
            && let Some(ref ls) = self.layout_source
            && let Some(url) = self.source.url_str()
            && let Ok(guard) = ls.document.lock()
            && let Ok(dom_bytes) = guard.to_bytes()
        {
            drop(guard);
            // `frozen_styles` keeps an owned `Stylesheet` (cold freeze path), so
            // deep-clone out of the `Arc` snapshot here.
            self.frozen_styles.insert(url.to_owned(), (*ls.stylesheet).clone());
            // Lazy prune: if we have too many stylesheets, drop those whose
            // corresponding bfcache entries are no longer frozen.
            if self.frozen_styles.len() > 32 {
                let bf = &self.bfcache;
                self.frozen_styles.retain(|k, _| bf.has_frozen(k));
            }
            self.bfcache.store(BfCacheEntry {
                url: url.to_owned(),
                payload: BfCachePayload::Frozen(FrozenPage {
                    dom_bytes,
                    js_heap: Vec::new(),
                    css_source: String::new(),
                }),
                scroll_x: self.scroll_x,
                scroll_y: self.scroll_y,
                title: self.title.clone(),
            });
            persisted = true;
        }
        // Fallback: store an HTML snapshot if freeze was not possible.
        //
        // BUG-834: this does NOT make the document salvageable. Coming back
        // re-parses the page from its source, so the document object, its
        // listeners, timers and closures are all gone — HTML LS §7.4.6 calls
        // that a discarded document, which must hear `unload` and must report
        // `pagehide.persisted === false`. Only the parked and frozen paths above
        // retain anything, so `persisted` is deliberately left untouched here
        // (it used to be raised, which is why an ordinary link navigation
        // reported `persisted=true` and swallowed `unload`).
        if !persisted
            && let Some(ref ls) = self.layout_source
            && let Some(ref html) = ls.html_source
            && let Some(url) = self.source.url_str()
        {
            self.bfcache.store(BfCacheEntry {
                url: url.to_owned(),
                payload: BfCachePayload::HtmlSnapshot(html.clone()),
                scroll_x: self.scroll_x,
                scroll_y: self.scroll_y,
                title: self.title.clone(),
            });
        }
        // HTML LS §7.4.5–§7.4.6: run the full unload sequence on the outgoing
        // page — `beforeunload`, then `pagehide` → `visibilityState = 'hidden'`
        // → `unload`. `persisted = true` signals the document was retained
        // (parked/frozen above), which is also its salvageable state: such a
        // page gets `pagehide` but no `unload`, and its listeners can skip
        // teardown they would redo on `pageshow`. BUG-834.
        // ADR-016 M2.2d: fire-and-forget void via route_task_js (off-UI-thread
        // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.fire_beforeunload();
            j.unload_document(persisted);
        });
        // Push current page to back stack (full-doc entry: no same_doc_state_json).
        self.nav_back.push(NavEntry {
            source: self.source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            display_url: None,
            same_doc_state_json: None,
            nav_key: self.current_nav_key.clone(),
        });
        // New navigation invalidates forward history and resets same-doc state.
        self.nav_fwd.clear();
        self.display_url = None;
        self.current_history_state_json = String::from("null");
        // Assign a fresh key to the incoming page before it becomes current.
        self.nav_key_counter += 1;
        self.current_nav_key = format!("nav-{}", self.nav_key_counter);
        // Load new page.
        self.source = source;
        self.commit_nav_state();
        self.reload();
    }

    /// Перейти на `source`, заменяя текущую запись истории (без push в back-stack).
    /// Аналог `history.replaceState` / `location.replace()` в браузере.
    pub(crate) fn navigate_replace(&mut self, source: PageSource) {
        // ADR-016 M2.2c-2d: см. `navigate_to` — dispatch через `route_task_js`,
        // intercept-чтение через `route_query_js` (read-after-eval порядок под флагом).
        {
            let url = source.url_str().unwrap_or("").to_string();
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                j.eval_js(&format!("_lumen_dispatch_navigate('replace', '{url}', true, false)"));
            });
        }
        if let Some(intercept) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            |j| j.take_nav_intercept_result(),
        ) {
            if let Some(&(true, false)) = intercept.last() {
                self.pending_intercepted = Some(PendingIntercepted::Replace {
                    url: source.url_str().unwrap_or("").to_string(),
                    handler_started: false,
                });
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                    j.eval_js("_lumen_run_navigate_handler()");
                });
                if let Some(PendingIntercepted::Replace { handler_started, .. }) =
                    self.pending_intercepted.as_mut()
                {
                    *handler_started = true;
                }
                return;
            }
            if let Some(&(false, true)) = intercept.last() {
                self.fire_navigate_error();
                return;
            }
        }
        // New navigation invalidates forward history but does NOT push to back stack.
        self.nav_fwd.clear();
        self.display_url = None;
        self.current_history_state_json = String::from("null");
        self.source = source;
        // BUG-352: `navigate_replace` doesn't route through `commit_nav_state`
        // (that call updates JS's `window.navigation`, not needed for a plain
        // `location.replace()`-style navigation), so it needs its own chrome
        // refresh — see `commit_nav_state`'s doc comment for why this matters.
        self.relayout_chrome_host();
        self.reload();
    }

    /// Перейти на предыдущую страницу в истории (Alt+Left).
    pub(crate) fn navigate_back(&mut self) {
        // ADR-016 M2.2c-2d: см. `navigate_to` — dispatch через `route_task_js`,
        // intercept-чтение через `route_query_js` (read-after-eval порядок под флагом).
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
            j.eval_js("_lumen_dispatch_navigate('traverse', '', true, false)");
        });
        if let Some(intercept) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            |j| j.take_nav_intercept_result(),
        ) {
            if let Some(&(true, false)) = intercept.last() {
                self.pending_intercepted = Some(PendingIntercepted::Back {
                    handler_started: false,
                });
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                    j.eval_js("_lumen_run_navigate_handler()");
                });
                if let Some(PendingIntercepted::Back { handler_started }) =
                    self.pending_intercepted.as_mut()
                {
                    *handler_started = true;
                }
                return;
            }
            if let Some(&(false, true)) = intercept.last() {
                self.fire_navigate_error();
                return;
            }
        }
        let Some(prev) = self.nav_back.pop() else { return };
        let crossed_document = std::mem::take(&mut self.traversal_crossed_document);

        let mut post_reload_traversal = None;
        if let Some(state_json) = prev.same_doc_state_json {
            if !crossed_document {
                // Same-document navigation: fire popstate, update address bar, don't reload.
                // Push current same-doc state to forward stack so Alt+Right restores it.
                let cur_display = self.display_url.take();
                let cur_state = std::mem::replace(
                    &mut self.current_history_state_json,
                    state_json.clone(),
                );
                self.nav_fwd.push(NavEntry {
                    source: self.source.clone(),
                    scroll_x: self.scroll_x,
                    scroll_y: self.scroll_y,
                    display_url: cur_display,
                    same_doc_state_json: Some(cur_state),
                    nav_key: self.current_nav_key.clone(),
                });
                let url = prev.display_url.unwrap_or_default();
                self.display_url = if url.is_empty() { None } else { Some(url.clone()) };
                // ADR-016 M2.2d: fire-and-forget void via route_task_js (off-UI-thread
                // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                    j.fire_popstate(&state_json, &url);
                });
                self.fire_current_entry_change();
                self.request_redraw();
                self.current_nav_key = prev.nav_key;
                self.source = prev.source;
                self.commit_nav_state();
                return;
            }
            // Cross-document unification: the multi-step shuffle passed through
            // a full-document entry before landing here, so the loaded document
            // is not the one this same-document entry belongs to. Defer the
            // popstate/URL update until the correct document (reloaded below,
            // or thawed from bfcache) actually finishes loading.
            post_reload_traversal = Some((state_json, prev.display_url.clone()));
        }

        // Full-document navigation: restore page and reload.
        // HTML LS §7.4.5–§7.4.6: run the full unload sequence on the current
        // page — `beforeunload`, then `pagehide` → `visibilityState = 'hidden'`
        // → `unload`. BUG-834: the outgoing document is retained only on the
        // parked-page branch below, so that same condition IS its salvageable
        // state and decides both the `persisted` flag and whether `unload`
        // fires at all. Computed here (before the sequence) rather than read
        // back from `park_current_page`, because the events must reach the
        // page while it is still current.
        let outgoing_parkable = prev
            .source
            .url_str()
            .is_some_and(|u| self.has_parked_page(u))
            && self.bfcache_eligible();
        // ADR-016 M2.2d: fire-and-forget void via route_task_js (off-UI-thread
        // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.fire_beforeunload();
            j.unload_document(outgoing_parkable);
        });
        // Push current page to forward stack.
        let cur_display = self.display_url.take();
        let cur_state = std::mem::replace(
            &mut self.current_history_state_json,
            String::from("null"),
        );
        self.nav_fwd.push(NavEntry {
            source: self.source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            display_url: cur_display,

            same_doc_state_json: if cur_state != "null" { Some(cur_state) } else { None },
            nav_key: self.current_nav_key.clone(),
        });
        // BUG-835: a live parked page wins over every frozen/snapshot payload —
        // it is the only restore path that brings the document's JS state back.
        if let Some(url) = prev.source.url_str().map(str::to_owned)
            && self.has_parked_page(&url)
        {
            // Park the document being left as well, so Forward restores it alive
            // too instead of reloading it from scratch. Eligibility was already
            // resolved above as `outgoing_parkable` — re-querying it here would
            // let it disagree with the `persisted` flag just reported to the page.
            if outgoing_parkable {
                self.park_current_page();
            }
            self.source = prev.source.clone();
            self.current_nav_key = prev.nav_key.clone();
            if self.restore_parked_page(&url) {
                if let Some((state_json, display_url)) = post_reload_traversal.clone() {
                    self.apply_post_reload_traversal(state_json, display_url);
                }
                return;
            }
        }
        // Try bfcache first: a Frozen payload thaws in place (no reload); an
        // HtmlSnapshot falls back to the existing re-parse path.
        let restored_scroll = if let Some(url) = prev.source.url_str() {
            if let Some(entry) = self.bfcache.retrieve(url).cloned() {
                match entry.payload {
                    BfCachePayload::Frozen(ref frozen) => {
                        self.source = prev.source.clone();
                        self.current_nav_key = prev.nav_key.clone();
                        if self.bfcache_thaw(&entry, frozen) {
                            if let Some((state_json, display_url)) = post_reload_traversal.clone() {
                                self.apply_post_reload_traversal(state_json, display_url);
                            }
                            return;
                        }
                        // Thaw failed (stylesheet evicted / DOM decode error):
                        // fall through to a normal reload of the previous source.
                        None
                    }
                    BfCachePayload::HtmlSnapshot(ref html) => {
                        let base_url = url.to_owned();
                        self.source = PageSource::Snapshot { html: html.clone(), base_url };
                        // Restored from bfcache → the next `pageshow` is `persisted=true`.
                        self.pending_pageshow_persisted = true;
                        Some((entry.scroll_x, entry.scroll_y))
                    }
                }
            } else {
                self.source = prev.source;
                None
            }
        } else {
            self.source = prev.source;
            None
        };
        // Previous entry becomes the new current: preserve its nav key.
        self.current_nav_key = prev.nav_key;
        // Restore scroll position from bfcache (or from nav entry if no bfcache hit).
        // U-1: reload() is now asynchronous (the page resets scroll at LoadDone),
        // so stash the offset for `apply_loaded_page` to apply instead of setting
        // it here — a direct assignment would be clobbered when LoadDone arrives.
        let (sx, sy) = restored_scroll.unwrap_or((prev.scroll_x, prev.scroll_y));
        self.pending_restore_scroll = Some((sx, sy));
        if let Some(traversal) = post_reload_traversal {
            self.pending_post_reload_traversal = Some(traversal);
        }
        self.reload();
        if let Some(w) = self.window.as_ref() { w.request_redraw(); }
        self.commit_nav_state();
    }

    /// Перейти на следующую страницу в истории (Alt+Right).
    pub(crate) fn navigate_forward(&mut self) {
        // ADR-016 M2.2c-2d: см. `navigate_to` — dispatch через `route_task_js`,
        // intercept-чтение через `route_query_js` (read-after-eval порядок под флагом).
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
            j.eval_js("_lumen_dispatch_navigate('traverse', '', true, false)");
        });
        if let Some(intercept) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            |j| j.take_nav_intercept_result(),
        ) {
            if let Some(&(true, false)) = intercept.last() {
                self.pending_intercepted = Some(PendingIntercepted::Forward {
                    handler_started: false,
                });
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                    j.eval_js("_lumen_run_navigate_handler()");
                });
                if let Some(PendingIntercepted::Forward { handler_started }) =
                    self.pending_intercepted.as_mut()
                {
                    *handler_started = true;
                }
                return;
            }
            if let Some(&(false, true)) = intercept.last() {
                self.fire_navigate_error();
                return;
            }
        }
        let Some(next) = self.nav_fwd.pop() else { return };
        let crossed_document = std::mem::take(&mut self.traversal_crossed_document);

        let mut post_reload_traversal = None;
        if let Some(state_json) = next.same_doc_state_json {
            if !crossed_document {
                // Same-document forward navigation: fire popstate, update address bar.
                let cur_display = self.display_url.take();
                let cur_state = std::mem::replace(
                    &mut self.current_history_state_json,
                    state_json.clone(),
                );
                self.nav_back.push(NavEntry {
                    source: self.source.clone(),
                    scroll_x: self.scroll_x,
                    scroll_y: self.scroll_y,
                    display_url: cur_display,
                    same_doc_state_json: Some(cur_state),
                    nav_key: self.current_nav_key.clone(),
                });
                let url = next.display_url.unwrap_or_default();
                self.display_url = if url.is_empty() { None } else { Some(url.clone()) };
                // ADR-016 M2.2d: fire-and-forget void via route_task_js (off-UI-thread
                // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                    j.fire_popstate(&state_json, &url);
                });
                self.fire_current_entry_change();
                self.request_redraw();
                self.current_nav_key = next.nav_key;
                self.source = next.source;
                self.commit_nav_state();
                return;
            }
            // Cross-document unification: see `navigate_back`.
            post_reload_traversal = Some((state_json, next.display_url.clone()));
        }

        // Full-document forward navigation.
        // HTML LS §7.4.5–§7.4.6: mirror of `navigate_back` — the full unload
        // sequence, with the parked-page branch below as the salvageable state
        // (BUG-834).
        let outgoing_parkable = next
            .source
            .url_str()
            .is_some_and(|u| self.has_parked_page(u))
            && self.bfcache_eligible();
        // ADR-016 M2.2d: fire-and-forget void via route_task_js (off-UI-thread
        // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.fire_beforeunload();
            j.unload_document(outgoing_parkable);
        });
        let cur_display = self.display_url.take();
        let cur_state = std::mem::replace(
            &mut self.current_history_state_json,
            String::from("null"),
        );
        self.nav_back.push(NavEntry {
            source: self.source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            display_url: cur_display,
            same_doc_state_json: if cur_state != "null" { Some(cur_state) } else { None },
            nav_key: self.current_nav_key.clone(),
        });
        // BUG-835: mirror of `navigate_back` — a live parked page wins.
        if let Some(url) = next.source.url_str().map(str::to_owned)
            && self.has_parked_page(&url)
        {
            // See `navigate_back`: eligibility is `outgoing_parkable`, resolved
            // before the unload sequence so it cannot disagree with `persisted`.
            if outgoing_parkable {
                self.park_current_page();
            }
            self.source = next.source.clone();
            self.current_nav_key = next.nav_key.clone();
            if self.restore_parked_page(&url) {
                if let Some((state_json, display_url)) = post_reload_traversal.clone() {
                    self.apply_post_reload_traversal(state_json, display_url);
                }
                return;
            }
        }
        // Try bfcache first: a Frozen payload thaws in place (no reload); an
        // HtmlSnapshot falls back to the existing re-parse path.
        let restored_scroll = if let Some(url) = next.source.url_str() {
            if let Some(entry) = self.bfcache.retrieve(url).cloned() {
                match entry.payload {
                    BfCachePayload::Frozen(ref frozen) => {
                        self.source = next.source.clone();
                        self.current_nav_key = next.nav_key.clone();
                        if self.bfcache_thaw(&entry, frozen) {
                            if let Some((state_json, display_url)) = post_reload_traversal.clone() {
                                self.apply_post_reload_traversal(state_json, display_url);
                            }
                            return;
                        }
                        // Thaw failed (stylesheet evicted / DOM decode error):
                        // fall through to a normal reload of the next source.
                        None
                    }
                    BfCachePayload::HtmlSnapshot(ref html) => {
                        let base_url = url.to_owned();
                        self.source = PageSource::Snapshot { html: html.clone(), base_url };
                        // Restored from bfcache → the next `pageshow` is `persisted=true`.
                        self.pending_pageshow_persisted = true;
                        Some((entry.scroll_x, entry.scroll_y))
                    }
                }
            } else {
                self.source = next.source;
                None
            }
        } else {
            self.source = next.source;
            None
        };
        // Forward entry becomes the new current: preserve its nav key.
        self.current_nav_key = next.nav_key;
        // U-1: stash scroll offset for `apply_loaded_page` (async reload — see
        // navigate_back for rationale).
        let (sx, sy) = restored_scroll.unwrap_or((next.scroll_x, next.scroll_y));
        self.pending_restore_scroll = Some((sx, sy));
        if let Some(traversal) = post_reload_traversal {
            self.pending_post_reload_traversal = Some(traversal);
        }
        self.reload();
        self.commit_nav_state();
        if let Some(w) = self.window.as_ref() { w.request_redraw(); }
    }

    /// Traverse the session history by `delta` (negative = back, positive =
    /// forward) as a SINGLE logical step (HTML LS history traversal): the
    /// intermediate entries of a multi-step `history.go(n)` are skipped without
    /// rendering, and only the destination entry fires `popstate` (same-document)
    /// or reloads (full-document) — exactly one observable event, delivered by the
    /// final `navigate_back` / `navigate_forward`. An out-of-range `delta` is a
    /// no-op (per spec, a step outside the history range does nothing).
    ///
    /// This is the single authority for JS-initiated traversal: `history.go` /
    /// `back` / `forward` queue a delta that the shell drains into this method, so
    /// the real `nav_back` / `nav_fwd` stacks (not the JS read-cache mirror) decide
    /// what actually happens — eliminating the multi-step `go` drift where the JS
    /// mirror moved its cursor but the shell stacks did not.
    ///
    /// Cross-document unification: if the shuffle passes through a
    /// full-document entry en route to a same-document destination, the
    /// currently loaded document is stale relative to that destination —
    /// `self.traversal_crossed_document` flags this for `navigate_back`/
    /// `navigate_forward`, which reload the correct document first and defer
    /// the `popstate`/URL update via `pending_post_reload_traversal`.
    #[cfg_attr(not(feature = "v8"), allow(dead_code))]
    pub(crate) fn navigate_by(&mut self, delta: i32) {
        if delta == 0 {
            return;
        }
        let back = delta < 0;
        let steps = delta.unsigned_abs() as usize;

        // Out-of-range traversal is a no-op (the shell stacks are authoritative).
        if back && self.nav_back.len() < steps {
            return;
        }
        if !back && self.nav_fwd.len() < steps {
            return;
        }

        // Skip the intermediate entries without rendering: shuttle the current
        // entry and each crossed entry onto the opposite stack, leaving `self`
        // positioned at the entry just before the destination. The final
        // navigate_back/forward then performs the one real (popstate/reload) hop.
        let mut crossed_document = false;
        if steps > 1 {
            let cur = NavEntry {
                source: self.source.clone(),
                scroll_x: self.scroll_x,
                scroll_y: self.scroll_y,
                display_url: self.display_url.clone(),
                same_doc_state_json: if self.current_history_state_json != "null" {
                    Some(self.current_history_state_json.clone())
                } else {
                    None
                },
                nav_key: self.current_nav_key.clone(),
            };
            let (cur, crossed) = NavEntry::shift_multi_step(
                &mut self.nav_back,
                &mut self.nav_fwd,
                cur,
                steps,
                back,
            );
            crossed_document = crossed;
            self.source = cur.source;
            self.scroll_x = cur.scroll_x;
            self.scroll_y = cur.scroll_y;
            self.display_url = cur.display_url;
            self.current_history_state_json =
                cur.same_doc_state_json.unwrap_or_else(|| "null".to_string());
        }

        self.traversal_crossed_document = crossed_document;
        if back {
            self.navigate_back();
        } else {
            self.navigate_forward();
        }
    }

    /// Compute the delta in history steps needed to reach `key`.
    ///
    /// `nav_back` and `nav_fwd` are stacks where the *last* element is the
    /// nearest entry relative to the current one.  Returns a negative delta
    /// when the key is found in `nav_back` (steps back) and a positive delta
    /// when it is found in `nav_fwd` (steps forward).  `len - pos` counts how
    /// many entries lie between the chosen entry and the top of its stack.
    pub(crate) fn key_traversal_delta(nav_back: &[NavEntry], nav_fwd: &[NavEntry], key: &str) -> Option<i32> {
        if let Some(pos) = nav_back.iter().rposition(|e| e.nav_key == key) {
            Some(-((nav_back.len() - pos) as i32))
        } else {
            nav_fwd
                .iter()
                .rposition(|e| e.nav_key == key)
                .map(|pos| (nav_fwd.len() - pos) as i32)
        }
    }

    /// Perform a history traversal to the entry identified by `key`.
    ///
    /// Backs the JS `navigation.traverseTo(key)` call.  If `key` matches the
    /// current entry no traversal occurs.  Unknown keys are silently ignored
    /// per the Navigation API specification.
    #[cfg_attr(not(feature = "v8"), allow(dead_code))]
    pub(crate) fn navigate_to_key(&mut self, key: &str) {
        if key == self.current_nav_key {
            return;
        }
        if let Some(delta) = Self::key_traversal_delta(&self.nav_back, &self.nav_fwd, key) {
            self.navigate_by(delta);
        }
    }
}
