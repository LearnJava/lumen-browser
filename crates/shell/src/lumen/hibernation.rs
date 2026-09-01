//! The shell's half of the tab lifecycle tiers (T1 live / T2 in-memory
//! snapshot / T3 hibernated to SQLite).
//!
//! `crate::tab_lifecycle` decides *when* a tab changes tier - it hands back a
//! list of transitions once per second from `tick_idle` + `lru_evict`. The
//! methods here execute those decisions against real page state: serialising a
//! background tab's DOM out to SQLite and evicting its `PageSnapshot`, reading
//! one back, and keeping the tab-strip badge in step.

use crate::*;

impl Lumen {
    /// Promote a background tab from T2в†’T3 (Hibernated) by serialising its DOM
    /// to SQLite and evicting the in-memory `PageSnapshot`.
    ///
    /// On failure (serialise error, SQLite error) the snapshot is put back into
    /// `bg_tabs` and the tab stays at T2.
    ///
    /// This is also the T2в†’T3 bfcache degradation point (`docs/tasks/ph3-bfcache.md`
    /// step 8): `snap` owns the tab's `bfcache: BfCache`, which may hold `Frozen`
    /// entries (each carrying a full DOM byte blob). `bg_tabs.remove` moves `snap`
    /// into this function; on the success path it is never re-inserted anywhere,
    /// so it вЂ” and every `FrozenPage` inside its `bfcache` вЂ” is freed when this
    /// function returns. No separate `degrade_bfcache_entries` pass is needed: the
    /// whole per-tab state (bfcache included) is already released at T3.
    fn hibernate_bg_tab(&mut self, tab_id: usize) {
        let Some(snap) = self.bg_tabs.remove(&tab_id) else { return };

        // Serialise DOM via Document::to_bytes() (bincode).
        let (dom_blob, css_source) = if let Some(ls) = snap.layout_source.as_ref() {
            match ls.document.lock() {
                Ok(doc) => {
                    let blob = doc.to_bytes().unwrap_or_default();
                    let css = extract_style_blocks(&doc);
                    (blob, css)
                }
                Err(_) => (vec![], String::new()),
            }
        } else {
            (vec![], String::new())
        };

        let url = match &snap.source {
            PageSource::Url(u) => u.clone(),
            PageSource::File(p) => format!("file://{}", p.display()),
            PageSource::Snapshot { base_url, .. } => base_url.clone(),
            PageSource::Empty | PageSource::AboutBlank | PageSource::Static { .. } => String::new(),
        };
        let title = snap.title.clone().unwrap_or_default();
        let scroll_x = snap.scroll_x;
        let scroll_y = snap.scroll_y;

        let data = lumen_storage::HibernatedTabData {
            dom_blob,
            css_source,
            url: url.clone(),
            title: title.clone(),
            scroll_x,
            scroll_y,
        };

        if let Err(e) = self.tab_snapshots.store(tab_id as i64, &data) {
            eprintln!("РћС€РёР±РєР° hibernate tab {tab_id}: {e}");
            // Rollback вЂ” keep the snapshot in RAM.
            self.bg_tabs.insert(tab_id, snap);
            return;
        }

        // Keep only lightweight metadata in RAM (scroll state stays in SQLite).
        self.hibernated_tabs.insert(
            tab_id,
            tab_lifecycle::TabMetadata { url, title },
        );

        // Update badge in the strip (T3 = grey dot).
        if let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == tab_id) {
            self.tab_strip.set_tab_state(idx, tab_lifecycle::TabState::Hibernated);
        }
    }

    /// Restore a T2 (BackgroundOld) tab from SQLite crash-recovery checkpoint.
    ///
    /// Used only when `bg_tabs` is empty for this tab (process-restart path).
    /// Reads scroll + form state from `t2_store` and applies them to the current
    /// (blank-reset) active slot.  The page URL is not stored in `t2_store`, so
    /// the tab will appear blank; a future enhancement may store the URL to
    /// trigger a background reload (10I Phase 2).
    ///
    /// Shows `sleep_hint` overlay if restore takes >100 ms.
    pub(crate) fn restore_t2_tab(&mut self, tab_id: usize) {
        self.t2_restore_start_ms = Some(self.epoch.elapsed().as_secs_f64() * 1000.0);
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }

        if let Ok(Some(data)) = self.t2_store.fetch(tab_id as i64) {
            self.scroll_x = data.scroll_x;
            self.scroll_y = data.scroll_y;
            self.form_state = tab_lifecycle::deserialize_form_state(&data.form_state_json);
            let _ = self.t2_store.delete(tab_id as i64);
        }

        self.t2_restore_start_ms = None;
    }

    /// Restore a T3-hibernated tab into the active slot.
    ///
    /// Fetches the DOM blob from SQLite, reconstructs the `Document` via
    /// `Document::from_bytes()`, re-parses inline CSS, and re-runs
    /// layout+paint.  Returns `true` on success so `switch_tab` knows
    /// whether to fall back to a blank tab.
    pub(crate) fn restore_hibernated_tab(&mut self, tab_id: usize) -> bool {
        // Start spinner timer for long restore operations (>200ms).
        self.restore_spinner_start_ms = Some(self.epoch.elapsed().as_secs_f64() * 1000.0);
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }

        let Some(meta) = self.hibernated_tabs.remove(&tab_id) else {
            self.restore_spinner_start_ms = None;
            return false;
        };

        // Pre-fill title from lightweight metadata for immediate window title update.
        self.title = Some(meta.title.clone());

        let data = match self.tab_snapshots.fetch(tab_id as i64) {
            Ok(Some(d)) => d,
            Ok(None) => {
                eprintln!("tab {tab_id}: snapshot missing (url={})", meta.url);
                // Put metadata back so the strip still shows Hibernated.
                self.hibernated_tabs.insert(tab_id, meta);
                self.restore_spinner_start_ms = None;
                return false;
            }
            Err(e) => {
                eprintln!("tab {tab_id}: snapshot read error (url={}): {e}", meta.url);
                self.hibernated_tabs.insert(tab_id, meta);
                self.restore_spinner_start_ms = None;
                return false;
            }
        };

        // Reconstruct Document from bincode blob.
        let doc = match Document::from_bytes(&data.dom_blob) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("РћС€РёР±РєР° РґРµСЃРµСЂРёР°Р»РёР·Р°С†РёРё DOM РІРєР»Р°РґРєРё {tab_id}: {e}");
                self.hibernated_tabs.insert(tab_id, meta);
                self.restore_spinner_start_ms = None;
                return false;
            }
        };

        // Re-parse CSS from inline <style> blocks preserved in the DOM.
        let css = if data.css_source.is_empty() {
            extract_style_blocks(&doc)
        } else {
            data.css_source.clone()
        };
        let stylesheet = lumen_css_parser::parse(&css);

        // Rebuild a fresh PersistentJs runtime. The JS heap cannot be
        // serialised, so the page's inline <script> blocks are re-run against
        // the restored DOM. The runtime shares the returned Arc<Mutex<Document>>
        // with the layout tree so both observe the same document.
        self.set_js_ctx(None);
        let event_sink = self.event_sink.clone();
        let cookie_banner_dismiss = self.cookie_banner_dismiss;
        let deterministic = self.deterministic;
        // Computed up front: `&mut self.ls_storage` below would otherwise
        // conflict with this `&self` method call as a later call argument.
        let cookie_jar = self.active_cookie_jar();
        let (document_arc, js_ctx) = tab_lifecycle::hibernate::restore_js_context(
            &data.url,
            doc,
            event_sink,
            &mut self.ls_storage,
            &mut self.ss_storage,
            self.idb_dir.as_deref(),
            &self.sw_backend,
            cookie_banner_dismiss,
            deterministic,
            Some(cookie_jar),
        );

        let layout_source = LayoutSource {
            document: Arc::clone(&document_arc),
            stylesheet: Arc::new(stylesheet),
            html_source: None,
            // Tab hibernation (T3в†’T0) restore вЂ” original Cache-Control is not
            // preserved across the hibernate/restore round-trip; treat as
            // cacheable (matches the rest of this struct's restore paths).
            cache_control_no_store: false,
            // BUG-743: only the inline `<style>` text survives hibernation
            // (`extract_style_blocks`), the external-sheet bodies do not вЂ” a
            // rebuild would silently drop them, so the cascade stays frozen.
            dynamic_css: None,
        };

        // Re-run layout+paint with the current viewport (including zoom).
        let phys = self.renderer.as_ref().map_or_else(
            || (1024.0_f32, 720.0_f32),
            |r| {
                let s = r.viewport_size();
                (s.width, s.height)
            },
        );
        let meta_scale = meta_initial_scale(&layout_source);
        let (css_w, css_h) = zoom::effective_viewport(phys.0, phys.1, meta_scale, self.zoom_factor);
        let viewport = lumen_core::geom::Size::new(css_w, css_h);
        // content-visibility: auto (BB-4): relevance РїСЂРѕС‚РёРІ РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРЅРѕРіРѕ
        // scroll-РїРѕР»РѕР¶РµРЅРёСЏ; ratchet РЅРѕРІРѕР№ СЃС‚СЂР°РЅРёС†С‹ СЃС‚Р°СЂС‚СѓРµС‚ СЃ РЅСѓР»СЏ.
        lumen_layout::set_cv_scroll(data.scroll_x, data.scroll_y);
        lumen_layout::set_cv_relevant(std::collections::HashSet::new());
        let (display_list, lb) = relayout_page(&layout_source, viewport, &*self.hyp_provider, self.dark_mode, &self.web_fonts);
        lumen_layout::set_cv_scroll(0.0, 0.0);

        // Install into the active slot.
        self.set_display_list(display_list);
        self.title = Some(data.title);
        self.layout_source = Some(layout_source);
        // BUG-341 S7: hibernate restore bypasses the restyle-aware path.
        self.page_prev_cascade_styles = None;
        self.layout_box = Some(lb);
        self.cv_relevant.clear();
        self.cv_events.clear();
        self.cv_skipped.clear();
        self.cv_auto_state.clear();
        self.refresh_cv_state();
        self.set_js_ctx(js_ctx);
        // ADR-016 M2.2c-2b: Р·РµСЂРєР°Р»РёРј РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРЅС‹Р№ С…СЌРЅРґР» + DOM РІ РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє.
        self.sync_engine_js_state();
        self.scroll_x = data.scroll_x;
        self.scroll_y = data.scroll_y;
        self.content_height = content_height_of(&self.display_list);
        self.content_width = content_width_of(&self.display_list);

        // Seed the restored runtime with layout geometry + viewport so JS can
        // query bounding rects immediately (mirrors the fresh-load path).
        // ADR-016 M2.2c-2d: routed off-thread through `route_task_js`, same as the
        // fresh-load seed above (`self.js_present` gate в†’ byte-identical off).
        #[cfg(feature = "v8")]
        if self.js_present
            && let Some(lb_ref) = self.layout_box.as_ref()
        {
            let rects = collect_layout_rects(lb_ref);
            let hit_test_tree = Arc::new(lb_ref.clone());
            let styles = collect_computed_styles(lb_ref);
            let customs = collect_custom_properties(lb_ref);
            let (vw, vh) = (viewport.width, viewport.height);
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                js.update_layout_rects(rects);
                js.update_hit_test_tree(hit_test_tree);
                js.update_computed_styles(styles);
                js.update_custom_properties(customs);
                js.update_viewport_size(vw, vh);
            });
        }

        // Remove the SQLite entry вЂ” it is no longer needed.
        let _ = self.tab_snapshots.delete(tab_id as i64);

        // Restore complete вЂ” hide the spinner overlay.
        self.restore_spinner_start_ms = None;

        true
    }

    /// Poll the lifecycle manager approximately once per second.
    ///
    /// Processes tier transitions returned by `tick_idle` + `lru_evict`:
    /// - `Hibernated` transitions evict the corresponding `bg_tabs` entry to SQLite.
    /// - Other transitions update the tab strip badge.
    pub(crate) fn tick_lifecycle(&mut self) {
        if self.lifecycle_last_tick.elapsed().as_secs() < 1 {
            return;
        }
        self.lifecycle_last_tick = std::time::Instant::now();

        let transitions = self.lifecycle_mgr.tick_idle(tab_lifecycle::MemoryPressure::Low);
        let evicted = self.lifecycle_mgr.lru_evict();

        for tr in transitions.into_iter().chain(evicted) {
            let tab_id = tr.tab_id as usize;

            if tr.to == tab_lifecycle::TabState::Hibernated {
                if self.bg_tabs.contains_key(&tab_id) {
                    self.hibernate_bg_tab(tab_id);
                }
                continue;
            }

            // T1 в†’ T2: checkpoint scroll + form state to SQLite for crash recovery.
            if tr.to == tab_lifecycle::TabState::BackgroundOld
                && let Some(snap) = self.bg_tabs.get(&tab_id)
            {
                let data = lumen_storage::T2SleepData {
                    js_heap_blob: vec![],
                    dom_blob: vec![],
                    scroll_x: snap.scroll_x,
                    scroll_y: snap.scroll_y,
                    form_state_json: tab_lifecycle::serialize_form_state(&snap.form_state),
                    ts: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs() as i64),
                };
                let _ = self.t2_store.store(tab_id as i64, &data);
            }

            // GC tuning per tier (10L): run progressively aggressive GC as a
            // background tab ages, reclaiming heap without full hibernation cost.
            let gc_level_opt: Option<u8> = match tr.to {
                tab_lifecycle::TabState::BackgroundRecent => Some(1), // moderate
                tab_lifecycle::TabState::BackgroundOld => Some(2),    // aggressive
                _ => None,
            };
            if let (Some(gc_level), Some(js)) = (
                gc_level_opt,
                self.bg_tabs.get(&tab_id).and_then(|s| s.js_ctx.as_ref()),
            ) {
                js.run_gc_pass(gc_level);
            }

            // Update strip badge for BackgroundOld (amber) or other tier changes.
            if let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == tab_id) {
                self.tab_strip.set_tab_state(idx, tr.to);
            }
        }

        // Auto-archive (7A.5): move background tabs idle for > 12 h out of the
        // strip.  Only runs when there are в‰Ґ 2 tabs (the active tab is never
        // archived) and the tab is not already hibernated (RAM already saved).
        if self.tab_strip.len() >= 2 {
            let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
            let threshold = tabs::archive::ARCHIVE_AFTER_MS;
            // Collect IDs to archive (avoiding borrow conflict on tab_strip).
            let to_archive: Vec<usize> = self
                .tab_strip
                .tabs
                .iter()
                .enumerate()
                .filter(|(i, t)| {
                    *i != self.tab_strip.active
                        && t.tab_state != tab_lifecycle::TabState::Hibernated
                        && (now_ms - t.last_activated_ms) > threshold
                })
                .map(|(_, t)| t.id)
                .collect();

            for tab_id in to_archive {
                // Guard: never archive down to 0 tabs.
                if self.tab_strip.len() <= 1 {
                    break;
                }
                let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == tab_id) else {
                    continue;
                };
                let title = self.tab_strip.tabs[idx].title.clone();
                let container = self.tab_strip.tabs[idx].container;
                let url = self
                    .bg_tabs
                    .get(&tab_id)
                    .and_then(|s| s.source.url_str().map(|u| u.to_owned()))
                    .unwrap_or_default();
                self.archive.push(tabs::archive::ArchivedTab {
                    id: tab_id,
                    title,
                    url,
                    container,
                });
                // Evict in-memory snapshot and remove from strip + lifecycle.
                self.bg_tabs.remove(&tab_id);
                self.lifecycle_mgr.close_tab(tab_id as u64);
                self.tab_strip.remove(idx);
            }
        }
    }

    // в”Ђв”Ђ Tab management в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
}
