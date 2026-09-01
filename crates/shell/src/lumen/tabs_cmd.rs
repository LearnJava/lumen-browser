//! Tab-strip commands driven by the UI: opening, closing, switching,
//! duplicating and moving tabs, the split-view toggle and the tab context
//! menu's actions.
//!
//! The tab *widgets* and their state live in `crate::tabs`; what is here is
//! the shell side of a command - swapping the live page for the target tab's
//! saved snapshot, dropping the resources of tabs that just went away and
//! asking for the redraw that shows the result.
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`. Behaviour, order
//! of operations and method bodies are unchanged; only the module path and
//! visibility (`fn` -> `pub(crate) fn`, required for callers in other
//! modules) differ.

use crate::*;

impl Lumen {
    /// Reset all per-page fields to blank-tab defaults.
    ///
    /// Called after `save_page_snapshot()` to prepare `self` for a fresh tab
    /// before loading a URL or showing an empty page.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn reset_to_blank_tab(&mut self) {
        self.set_display_list(Vec::new());
        self.title = None;
        self.pending_images = Vec::new();
        self.source = PageSource::Empty;
        self.runtime = runtime::EventLoop::new();
        self.animation_scheduler = animation_scheduler::AnimationScheduler::new();
        self.transition_scheduler = TransitionScheduler::new();
        self.starting_style_tracker = StartingStyleTracker::new();
        self.prev_styles = HashMap::new();
        self.page_prev_cascade_styles = None;
        self.page_prev_interactive = (None, None, None);
        self.anim_frame = None;
        self.layout_box = None;
        self.find = find::FindState::default();
        self.address_bar = address_bar::AddressBarState::default();
        self.hint = hints::HintState::default();
        self.scroll_y = 0.0;
        self.scroll_x = 0.0;
        // ADR-016 M3.2: the retained scroll band belongs to the old page — drop
        // it so the next frame repaints instead of blitting stale pixels.
        self.scroll_cache.invalidate();
        self.content_height = 0.0;
        self.content_width = 0.0;
        self.layout_source = None;
        self.pending_reload = Rc::new(Cell::new(false));
        self.pending_js_navigate = None;
        self.stream_builder = None;
        self.stream_last_paint = std::time::Instant::now();
        self.stream_sheet = lumen_css_parser::Stylesheet::default();
        self.stream_layout_seeded = false;
        self.preload_dispatched = std::collections::HashSet::new();
        self.stream_images_requested = std::collections::HashSet::new();
        self.stream_image_sizes = HashMap::new();
        self.stream_image_sizes_dirty = false;
        self.ime_composing = None;
        self.bfcache = BfCache::new(16);
        self.frozen_styles = HashMap::new();
        self.parked_pages = Vec::new();
        self.nav_back = Vec::new();
        self.nav_fwd = Vec::new();
        self.form_state = HashMap::new();
        self.frame_text_cursor = HashMap::new();
        self.frame_text_selection_anchor = HashMap::new();
        self.text_drag = None;
        self.validation_tooltip = None;
        self.color_picker_node = None;
        self.date_picker_node = None;
        self.date_picker_year = 0;
        self.date_picker_month = 0;
        self.select_dropdown_node = None;
        self.ls_storage = HashMap::new();
        // BUG-836: a new tab is a new browsing context, so it starts with empty
        // session storage — this reset is the *only* place it may be cleared.
        self.ss_storage = HashMap::new();
        // idb_dir is session-level — intentionally not reset here.
        self.sw_backend = Arc::new(std::sync::Mutex::new(
            lumen_storage::store::InMemoryStorage::new(),
        ));
        self.set_js_ctx(None);
        self.first_paint_delivered = false;
        self.first_contentful_paint_delivered = false;
        self.load_failed = false;
        self.load_error_message = None;
        self.nav_start = None;
        self.animated_gifs = HashMap::new();
        self.gif_last_frame = HashMap::new();
        self.video_gif_store.playback.lock().unwrap().clear();
        self.video_gif_store.pending_loads.lock().unwrap().clear();
        self.video_gif_last_frame = HashMap::new();
        self.video_gif_frames = HashMap::new();
        self.image_cache = lumen_image::ImageDecodeCache::new();
        self.zoom_factor = zoom::ZOOM_DEFAULT;
        self.display_url = None;
        self.current_history_state_json = String::from("null");
        self.reader_original_source = None;
        self.cert_info = None;
        // Cancel in-flight scroll animations.
        self.scroll_anim = None;
        self.momentum_anim = None;
        self.forward_momentum_stop();
        self.scroll_drag = None;
        // ADR-016 M2.2c-2b: очищаем хэндл + DOM в движковом потоке для чистой вкладки.
        self.sync_engine_js_state();
    }

    /// Open a new blank tab.
    pub(crate) fn open_new_tab(&mut self) {
        // In tree-style tab mode, new tabs become children of the active tab,
        // building the parent-child tree automatically.
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        let new_idx = if self.tree_tabs.visible {
            let opener_id = self.tab_strip.tabs[self.tab_strip.active].id;
            self.tab_strip.push_with_opener(opener_id, now_ms)
        } else {
            self.tab_strip.push_blank(now_ms)
        };
        let new_id = self.tab_strip.tabs[new_idx].id;
        // Save current page into bg_tabs under the old active tab's id.
        let old_active = self.tab_strip.active;
        let old_id = self.tab_strip.tabs[old_active].id;
        // Mark old tab as recently backgrounded so it gets a badge if it ages to T2.
        self.tab_strip.set_tab_state(old_active, TabState::BackgroundRecent);
        let snap = self.save_page_snapshot();
        self.bg_tabs.insert(old_id, snap);
        self.tab_strip.active = new_idx;
        self.reset_to_blank_tab();
        // Register the new tab with the lifecycle manager.
        self.lifecycle_mgr.open_tab(new_id as u64);
        // CC-6: re-sync the CSS chrome's tab list (no-op off the flag).
        self.relayout_chrome_host();
        self.request_redraw();
    }

    /// Open or toggle split view (Ctrl+\).
    ///
    /// Picks the next tab after the active one for the right pane. If no other
    /// tab exists, does nothing (split requires at least two tabs).
    pub(crate) fn toggle_split_view(&mut self) {
        let tab_count = self.tab_strip.len();
        if tab_count < 2 {
            return;
        }
        let next_idx = (self.tab_strip.active + 1) % tab_count;
        let next_id = self.tab_strip.tabs[next_idx].id;

        let (dl, scroll_y, scroll_x, content_height, content_width) =
            if let Some(snap) = self.bg_tabs.get(&next_id) {
                (
                    snap.display_list.clone(),
                    snap.scroll_y,
                    snap.scroll_x,
                    snap.content_height,
                    snap.content_width,
                )
            } else if let Some(meta) = self.hibernated_tabs.get(&next_id) {
                // Hibernated tab: show a minimal placeholder with its title/url.
                let placeholder_dl = build_split_placeholder(&meta.url);
                (placeholder_dl, 0.0, 0.0, 0.0, 0.0)
            } else {
                // Blank/new tab — show empty pane.
                (vec![], 0.0, 0.0, 0.0, 0.0)
            };

        self.split_view = Some(panels::split_view::SplitView::new(
            next_id,
            dl,
            scroll_y,
            scroll_x,
            content_height,
            content_width,
        ));
    }

    /// Close the tab at `idx`. If it was the last tab, exits the app instead.
    pub(crate) fn close_tab(&mut self, idx: usize, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.tab_strip.len() == 1 {
            // Last tab — exit.
            event_loop.exit();
            return;
        }
        let closing_id = self.tab_strip.tabs[idx].id;
        // Remove from lifecycle manager.
        self.lifecycle_mgr.close_tab(closing_id as u64);
        if idx == self.tab_strip.active {
            // Closing the active tab: save nothing (it will be dropped),
            // restore the tab that will become active after removal.
            let new_active = self.tab_strip.remove(idx);
            let new_id = self.tab_strip.tabs[new_active].id;
            // Mark the newly-activated tab as Active so its badge clears.
            self.tab_strip.set_tab_state(new_active, TabState::Active);
            // Drop the current active page.
            self.reset_to_blank_tab();
            if let Some(snap) = self.bg_tabs.remove(&new_id) {
                self.restore_page_snapshot(snap);
            } else if self.hibernated_tabs.contains_key(&new_id) {
                // Target tab is hibernated — restore from SQLite.
                self.restore_hibernated_tab(new_id);
            }
        } else {
            // Closing a background tab: drop snapshot and any hibernated/sleeping data.
            self.bg_tabs.remove(&closing_id);
            self.hibernated_tabs.remove(&closing_id);
            let _ = self.tab_snapshots.delete(closing_id as i64);
            let _ = self.t2_store.delete(closing_id as i64);
            self.tab_strip.remove(idx);
        }
        // CC-6: re-sync the CSS chrome's tab list (no-op off the flag).
        self.relayout_chrome_host();
        self.request_redraw();
    }

    /// Execute a tab context-menu action (CC-4) on `tab_context_menu.target_idx`.
    pub(crate) fn exec_tab_menu_action(
        &mut self,
        action: tabs::context_menu::MenuAction,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) {
        use tabs::context_menu::MenuAction;
        let idx = self.tab_context_menu.target_idx;
        if idx >= self.tab_strip.len() {
            return;
        }
        match action {
            MenuAction::TogglePin => {
                self.tab_strip.toggle_pin(idx);
                self.request_redraw();
            }
            MenuAction::Duplicate => self.duplicate_tab(idx),
            MenuAction::MoveToNewWindow => self.move_tab_to_new_window(idx, event_loop),
            MenuAction::AddToNewGroup => {
                // CC-6: bundle the target tab into a fresh group, cycling the
                // colour by group count so successive groups differ. Persist
                // the group metadata so a future restore can recover it.
                use tabs::groups::GroupColor;
                let color = GroupColor::from_index((self.tab_strip.groups.len() % 8) as u8);
                let gid = self.tab_strip.create_group("Группа", color);
                self.tab_strip.assign_to_group(idx, gid);
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let _ = self.tab_groups.create("Группа", color.index(), now);
                self.request_redraw();
            }
            MenuAction::ToggleGroupCollapse => {
                if let Some(gid) = self.tab_strip.group_of(idx) {
                    let now_collapsed = self.tab_strip.toggle_collapse(gid);
                    // If the active tab is hidden by collapsing, move focus to
                    // the group's chip tab so a valid page stays displayed.
                    if now_collapsed
                        && !self.tab_strip.visible_indices().contains(&self.tab_strip.active)
                        && let Some(&chip) = self.tab_strip.group_members(gid).first()
                    {
                        self.switch_tab(chip);
                    }
                    self.request_redraw();
                }
            }
            MenuAction::RemoveFromGroup => {
                if let Some(gid) = self.tab_strip.group_of(idx) {
                    self.tab_strip.ungroup(idx);
                    // Drop the group entirely once its last member leaves.
                    if self.tab_strip.group_members(gid).is_empty() {
                        self.tab_strip.remove_group(gid);
                    }
                    self.request_redraw();
                }
            }
            MenuAction::CloseOthers => {
                // Keep the target visible: switch to it first so a surviving
                // page is shown, then drop everything else (non-pinned).
                if idx != self.tab_strip.active {
                    self.switch_tab(idx);
                }
                let keep = self.tab_strip.active;
                let removed = self.tab_strip.close_others(keep);
                self.discard_tab_resources(&removed);
                self.request_redraw();
            }
            MenuAction::CloseRight => {
                // If the active tab would be removed, switch to the target
                // (which always survives) so the displayed page stays valid.
                let active = self.tab_strip.active;
                if active > idx && !self.tab_strip.is_pinned(active) {
                    self.switch_tab(idx);
                }
                let removed = self.tab_strip.close_right(idx);
                self.discard_tab_resources(&removed);
                self.request_redraw();
            }
}
        }

    /// Drop the cached page resources of background tabs removed in bulk
    /// (CC-4 "Close others" / "Close to the right"). Mirrors the background
    /// branch of [`close_tab`].
    fn discard_tab_resources(&mut self, ids: &[usize]) {
        for &id in ids {
            self.lifecycle_mgr.close_tab(id as u64);
            self.bg_tabs.remove(&id);
            self.hibernated_tabs.remove(&id);
            let _ = self.tab_snapshots.delete(id as i64);
            let _ = self.t2_store.delete(id as i64);
        }
    }

    /// Duplicate the tab at `idx` (CC-4): insert a copy right after it and
    /// load the same page into it. Phase 0 re-fetches the source URL rather
    /// than deep-cloning live page/JS state.
    fn duplicate_tab(&mut self, idx: usize) {
        // Bring the source tab to the foreground so `self.source` is its page.
        if idx != self.tab_strip.active {
            self.switch_tab(idx);
        }
        let src_idx = self.tab_strip.active;
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        let Some(new_idx) = self.tab_strip.duplicate(src_idx, now_ms) else {
            return;
        };
        let src_source = self.source.clone();
        // Park the source page in bg_tabs under its own id.
        let old_id = self.tab_strip.tabs[src_idx].id;
        self.tab_strip.set_tab_state(src_idx, TabState::BackgroundRecent);
        let snap = self.save_page_snapshot();
        self.bg_tabs.insert(old_id, snap);
        // Activate the duplicate and load a fresh copy of the page.
        let new_id = self.tab_strip.tabs[new_idx].id;
        self.lifecycle_mgr.open_tab(new_id as u64);
        self.tab_strip.active = new_idx;
        self.tab_strip.set_tab_state(new_idx, TabState::Active);
        self.tab_strip.update_last_activated(new_idx, now_ms);
        self.reset_to_blank_tab();
        self.source = src_source;
        self.reload();
        self.request_redraw();
    }

    /// Move the tab at `idx` into a new OS window (CC-4). Phase 0 launches a
    /// fresh Lumen process for the tab's URL and removes the tab from this
    /// window. The last remaining tab is duplicated rather than moved (closing
    /// it would quit the app).
    fn move_tab_to_new_window(
        &mut self,
        idx: usize,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) {
        if idx != self.tab_strip.active {
            self.switch_tab(idx);
        }
        let url = self.source.url_str().map(str::to_owned);
        if let Some(url) = url
            && let Ok(exe) = std::env::current_exe()
        {
            let _ = std::process::Command::new(exe).arg(&url).spawn();
        }
        // Remove the tab here unless it is the only one (closing it would exit).
        if self.tab_strip.len() > 1 {
            self.close_tab(self.tab_strip.active, event_loop);
        }
        self.request_redraw();
    }

    /// Assign `kind` to tab at `idx` for task 7D.2.
    ///
    /// Pre-registers a cookie/storage store id for the active page's origin
    /// if one is known, so subsequent requests can be partitioned. UI
    /// border-top strip refreshes on the next redraw via `build_tab_bar`.
    pub(crate) fn set_tab_container(&mut self, idx: usize, kind: tabs::containers::ContainerKind) {
        if idx >= self.tab_strip.len() {
            return;
        }
        self.tab_strip.set_tab_container(idx, kind);
        // Pre-warm a store id for the active tab's origin so cookie/storage
        // dispatch can partition by container id without a later allocation
        // step. Best-effort only — non-active tabs are wired up the same way
        // the next time their page loads.
        if idx == self.tab_strip.active
            && let Some(url) = self.source.url_str()
            && let Some(origin) = origin_of_url(url)
        {
            self.container_store.get_or_create(&origin, kind);
        }
        self.request_redraw();
    }

    /// Switch to tab at `idx`. No-op if already active.
    ///
    /// Handles all three cases:
    /// - T1/T2 tab: restore full `PageSnapshot` from `bg_tabs` (in-memory, fast).
    /// - T3 Hibernated tab: restore from SQLite via `Document::from_bytes()`.
    /// - Blank new tab: reset to empty state.
    pub(crate) fn switch_tab(&mut self, idx: usize) {
        if idx == self.tab_strip.active || idx >= self.tab_strip.len() {
            return;
        }
        // Save current active tab, marking it BackgroundRecent in the strip.
        let old_active = self.tab_strip.active;
        let old_id = self.tab_strip.tabs[old_active].id;
        self.tab_strip.set_tab_state(old_active, TabState::BackgroundRecent);
        // T0 → T1: fire visibilitychange(hidden=true) before parking.
        // ADR-016 M2.2d (18): снимаем прямое `self.js_ctx`-обращение park-сайта —
        // fire-and-forget void через `route_task_js` (disjoint borrow полей
        // `engine_thread`/`js_ctx`). Под флагом (`LUMEN_ENGINE_THREAD=1`) уходит
        // `task`-ом на движковый поток, где `state.js` ещё зеркалит уходящую в фон
        // вкладку (ре-зеркалирование `sync_engine_js_state` встанет в очередь позже,
        // при загрузке/восстановлении новой) — pause исполняется на верном хэндле.
        // Без флага (по умолчанию) — синхронный вызов по UI-хэндлу, байт-идентично
        // прежнему `js.pause_event_loop()`.
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
            j.pause_event_loop();
        });
        let snap = self.save_page_snapshot();
        self.bg_tabs.insert(old_id, snap);
        // GC tuning (10L): run one moderate collection on the tab that just
        // went to background so it releases unreachable objects quickly.
        if let Some(js) = self.bg_tabs.get(&old_id).and_then(|s| s.js_ctx.as_ref()) {
            js.run_gc_pass(1);
        }

        // Sync lifecycle manager: deactivate old, activate new.
        let new_id = self.tab_strip.tabs[idx].id;
        self.lifecycle_mgr.activate_tab(new_id as u64);

        // Restore new active tab, marking it Active so any badge clears.
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        self.tab_strip.active = idx;
        self.tab_strip.set_tab_state(idx, TabState::Active);
        self.tab_strip.update_last_activated(idx, now_ms);
        // BUG-411: re-point the process-global ad-block toggle at the shields
        // state of the host now in front. This used to read `TabEntry::adblock`
        // — a field the legacy in-tab checkbox wrote and CC-15 removed, leaving
        // it permanently `false`, so every tab switch silently disabled
        // filtering for the rest of the session. The restored navigation
        // handler below re-syncs on the host once the restored page loads.
        self.sync_adblock_filter();

        self.reset_to_blank_tab();

        if let Some(snap) = self.bg_tabs.remove(&new_id) {
            // T1/T2: fast in-memory restore.
            self.restore_page_snapshot(snap);
            // T1 → T0: fire visibilitychange(hidden=false) after restore.
            // ADR-016 M2.2d (18): снимаем прямое `self.js_ctx`-обращение unpark-сайта —
            // fire-and-forget void через `route_task_js`. `restore_page_snapshot` выше
            // уже вызвал `sync_engine_js_state()` (зеркалит восстановленный хэндл
            // `task`-ом), а этот `task` встаёт в очередь **после** него — под флагом
            // unpause+GC исполняются на верном (восстановленном) хэндле. Без флага —
            // синхронно по UI-хэндлу, байт-идентично прежним `js.<method>()`.
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                j.unpause_event_loop();
                // GC tuning (10L): reset threshold to active level so the heap
                // can grow freely now that this tab is in the foreground.
                j.run_gc_pass(0);
            });
        } else if self.t2_store.exists(new_id as i64).unwrap_or(false) {
            // T2 crash-recovery: bg_tabs was lost (process restart) but SQLite
            // checkpoint exists — restore scroll + form state from it.
            self.restore_t2_tab(new_id);
        } else if self.hibernated_tabs.contains_key(&new_id) {
            // T3: restore from SQLite — Document::from_bytes() + relayout.
            self.restore_hibernated_tab(new_id);
        }
        // Otherwise the tab is blank (never loaded) — leave reset state.

        // DS-17: the synthetic TabList's `selected` state is rebuilt fresh
        // from `self.tab_strip.active` every time — without this, switching
        // to an already-loaded tab (no navigation, so nothing else rebuilds
        // the AX tree) left the OS bridge reporting the *previous* tab as
        // selected until the next full page load.
        self.update_platform_ax_tree();
        // CC-6: re-sync the CSS chrome's active-tab highlight (no-op off the flag).
        self.relayout_chrome_host();
        self.request_redraw();
    }

    /// Persist the current tab-strip layout (horizontal/vertical) into
    /// `browser_settings`.
    ///
    /// CC-15-3: the legacy tab-bar layout-toggle button was the only caller of
    /// `set_tab_layout` outside the settings panel's snapshot apply — removing
    /// its paint/hit-test would have silently dropped persistence from the two
    /// remaining toggle entry points (`KeyCommand::ToggleVerticalTabs`,
    /// `PaletteAction::ToggleVerticalTabs`), which never persisted on their
    /// own. Both now route through here so the choice survives a restart the
    /// same way the removed button made it.
    pub(crate) fn persist_tab_layout(&self) {
        let layout = if self.vertical_tabs.visible {
            tabs::strip::TabLayout::Vertical
        } else {
            tabs::strip::TabLayout::Horizontal
        };
        let _ = self.settings_store.set_tab_layout(layout.as_str());
    }
}
