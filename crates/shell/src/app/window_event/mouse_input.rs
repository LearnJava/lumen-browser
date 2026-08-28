//! Ветка `WindowEvent::MouseInput` цикла событий (SPLIT-SH2).
//!
//! Тело ветки вынесено из `Lumen::window_event` (`main.rs`) как есть, с
//! дедентом на 8 пробелов и без единой правки логики; строки внутри
//! многострочных строковых литералов дедент не затронул.

use crate::*;

impl Lumen {
    #[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn on_mouse_input(&mut self, event_loop: &ActiveEventLoop, state: ElementState, button: MouseButton) {
        if button == MouseButton::Right {
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            let (x_css, y_css) = self
                .cursor_position
                .map(|p| ((p.x as f32) / dpr, (p.y as f32) / dpr))
                .unwrap_or((0.0, 0.0));
            // P3-spell: a fresh right-click dismisses any stale spell
            // menu before deciding whether to reopen it.
            if state == ElementState::Pressed && self.page_context_menu.is_open() {
                self.page_context_menu.close();
            }
            // CC-4: right-click on a tab opens the tab context menu
            // instead of starting a mouse gesture.
            if state == ElementState::Pressed && !self.focus.active {
                // BUG-404: resolve the tab under the cursor via the
                // chrome hit-test + `data-tab-id` (the same
                // mechanism `ChromeAction::SelectTab` uses for a
                // left-click), not `tabs::strip::hit_test`'s legacy
                // geometry вЂ” the engine-drawn tab strip's real
                // layout no longer matches `TAB_BAR_HEIGHT`/
                // `ARCHIVE_BTN_W`/`LAYOUT_BTN_W`/`SETTINGS_BTN_W`.
                let tab_id = self.chrome_hit_test(x_css, y_css).and_then(|hit| {
                    hit.path
                        .iter()
                        .find_map(|&nid| self.chrome_data_id(nid, "data-tab-id"))
                });
                if let Some(idx) = tab_id
                    .and_then(|id| self.tab_strip.tabs.iter().position(|t| t.id == id as usize))
                {
                    let pinned = self.tab_strip.is_pinned(idx);
                    let group = self.tab_strip.group_of(idx);
                    let grouped = group.is_some();
                    let collapsed = group.is_some_and(|g| self.tab_strip.is_collapsed(g));
                    self.tab_context_menu
                        .open_for(idx, pinned, grouped, collapsed, x_css, y_css);
                    self.request_redraw();
                    return;
                }
            }
            // P3-spell СЃСЂРµР· 3: right-click on a misspelled word in a
            // focused text input opens the spell suggestion menu instead
            // of starting a mouse gesture.
            if state == ElementState::Pressed && self.try_open_spell_menu(x_css, y_css) {
                self.request_redraw();
                return;
            }
            if state == ElementState::Pressed {
                self.gesture.begin(x_css, y_css);
            } else if state == ElementState::Released
                && let Some(action) = self.gesture.finish()
            {
                self.execute_gesture_action(action, event_loop);
            }
        } else if button != MouseButton::Left {
            // Middle / back / forward вЂ” ignore.
        } else if state == ElementState::Pressed {
            // CSS :active вЂ” set immediately on press so :active rules apply.
            if self.active_nid != self.hovered_nid {
                self.active_nid = self.hovered_nid;
                // ADR-016 M2.2b-5: :active restyle is async-safe вЂ” the
                // click hit-test below reads the pre-:active layout (the
                // geometry the user pressed on), which is correct.
                self.relayout_chrome();
                self.request_redraw();
            }
            let Some(cursor) = self.cursor_position else {
                // Р‘РµР· CursorMoved-snapshot-Р° РґРѕ Press вЂ” РЅРµ Р·РЅР°РµРј РіРґРµ
                // РєР»РёРє; bail out. Р РµР°Р»РёСЃС‚РёС‡РЅРѕ вЂ” Press РІСЃРµРіРґР° РїСЂРёС…РѕРґРёС‚
                // РїРѕСЃР»Рµ CursorMoved, РЅРѕ Р·Р°С‰РёС‚РёРјСЃСЏ.
                return;
            };
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            let x_css = (cursor.x as f32) / dpr;
            let y_css = (cursor.y as f32) / dpr;
            // CC-5: chrome's own `:active` вЂ” mirrors the page's
            // `active_nid` handling above but scoped to `chrome_layout`
            // (see `relayout_chrome_host`'s doc comment for why the two
            // documents can't share interactive thread-locals).
            if self.point_over_chrome(x_css, y_css) {
                self.chrome_active_nid = self.chrome_hit_test(x_css, y_css).map(|r| r.node);
                self.relayout_chrome_host();
                self.request_redraw();
            }
            // F2-6: a press on a docked panel's inner edge begins a
            // resize drag; the click never reaches the page / panels.
            if let Some(edge) = self.resize_edge_at(x_css, y_css) {
                self.panel_resize = Some(edge);
                return;
            }
            // CC-4: while the tab context menu is open it captures the
            // click вЂ” picking a row runs the action, anywhere else just
            // dismisses it. The click never reaches the page / panels.
            if self.tab_context_menu.is_open() {
                let win_w = self.viewport_width_css();
                let win_h = self.window_height_css();
                let action = tabs::context_menu::action_at(
                    &self.tab_context_menu,
                    x_css,
                    y_css,
                    win_w,
                    win_h,
                );
                self.tab_context_menu.close();
                if let Some(action) = action {
                    self.exec_tab_menu_action(action, event_loop);
                }
                self.request_redraw();
                return;
            }
            // P3-spell СЃСЂРµР· 3: while the page spell menu is open it
            // captures the click вЂ” picking a row applies the correction /
            // dictionary action, anywhere else just dismisses it.
            if self.page_context_menu.is_open() {
                let win_w = self.viewport_width_css();
                let win_h = self.window_height_css();
                let action = self.page_context_menu.action_at(x_css, y_css, win_w, win_h);
                if let Some(action) = action {
                    self.exec_spell_menu_action(action);
                }
                self.page_context_menu.close();
                self.request_redraw();
                return;
            }
            // CC-2: the download panel is a top-most bottom overlay вЂ” it
            // captures clicks on its buttons / close / body before they
            // reach the page. A click outside the panel closes it.
            if self.downloads.visible {
                let win = (
                    self.viewport_width_css() as u32,
                    self.window_height_css() as u32,
                );
                if let Some(action) = download::hit_test(&self.downloads, x_css, y_css, win) {
                    use download::DownloadAction;
                    match action {
                        DownloadAction::Open(id) => {
                            self.downloads.open_download(id);
                        }
                        DownloadAction::Reveal(id) => {
                            self.downloads.show_in_folder(id);
                        }
                        DownloadAction::Cancel(id) => {
                            self.downloads.cancel(id);
                        }
                        DownloadAction::Close | DownloadAction::Outside => {
                            self.downloads.close();
                        }
                        DownloadAction::Inside => {}
                    }
                    self.request_redraw();
                    return;
                }
            }
            // Fire mousedown + pointerdown on the hovered DOM element.
            // Per W3C UI Events В§17.6 + Pointer Events L2 В§10 вЂ” fires before
            // any default action (click). Only when cursor is over page content.
            #[cfg(feature = "v8")]
            if let Some(hov) = self.hovered_nid {
                // Ph3 pointer-events-l3: flush queued pointermove samples
                // first so they precede pointerdown in dispatch order.
                self.flush_pointer_moves();
                let nid = hov.index() as u32;
                self.js_pointer_event(nid, "pointerdown", x_css, y_css, 0, 1);
                self.js_mouse_event(nid, "mousedown", x_css, y_css, 0, 1);
            }
            // BUG-480 срез 16: курсор над содержимым фрейма — нажатие
            // принадлежит под-документу. Ветка исключающая: пока `hovered_frame`
            // непуст, `hovered_nid` пуст по построению, так что двух нажатий
            // сразу не бывает.
            #[cfg(feature = "v8")]
            if let Some((f, n)) = self.hovered_frame {
                self.flush_pointer_moves();
                let at = self
                    .pointer_target(x_css, y_css)
                    .frame
                    .map_or((0.0, 0.0), |t| (t.local.x, t.local.y));
                let nid = n.index() as u32;
                self.frame_pointer_event(f, nid, "pointerdown", at, (0, 1));
                self.frame_mouse_event(f, nid, "mousedown", at, (0, 1));
            }

            // HTML5 DnD (PH3-9 / HTML LS В§9.3.3): start candidate when the
            // pressed element is draggable.  Drag does not activate until the
            // cursor moves в‰Ґ DND_THRESHOLD px (handled in CursorMoved).
            #[cfg(feature = "v8")]
            if let Some(hov) = self.hovered_nid
                && let Some(ls) = self.layout_source.as_ref() {
                let doc = ls.document.lock().unwrap();
                if lumen_dom::is_element_draggable(&doc, hov) {
                    self.dnd_state = Some(DndState {
                        src_nid: hov,
                        press_x: x_css,
                        press_y: y_css,
                        active: false,
                        over_nid: None,
                    });
                }
            }

            // B-7: Check if click is on resize grip of any element in layout tree.
            // If so, activate resize mode. This must be checked before other UI panels.
            if let Some(ref layout_box) = self.layout_box
                && let Some((nid, allow_w, allow_h)) = self.find_resize_grip_node(layout_box, x_css, y_css) {
                    self.resize_active = Some((nid, x_css, y_css, allow_w, allow_h));
                    self.request_redraw();
                    return;
                }

            // Command palette (task #23): modal вЂ” captures every click.
            // A click on a row activates it; a click on the scrim closes.
            if self.command_palette.visible {
                let win_w = self.viewport_width_css();
                match panels::command_palette::hit_test(
                    &self.command_palette,
                    x_css,
                    y_css,
                    win_w,
                ) {
                    panels::command_palette::PaletteHit::Row(filtered_idx) => {
                        self.command_palette.selected = filtered_idx;
                        if let Some(item) = self.command_palette.selected_item().cloned() {
                            self.command_palette.close();
                            self.activate_palette(&item, event_loop);
                        }
                    }
                    panels::command_palette::PaletteHit::Dismiss => {
                        self.command_palette.close();
                    }
                    panels::command_palette::PaletteHit::Inside => {}
                }
                self.request_redraw();
                return;
            }

            // Focus mode widget (task #25): floating top-right card. A
            // click on the ring pauses/resumes; the `Г—` corner exits.
            if self.focus.active {
                let win_w = self.viewport_width_css();
                if let Some(hit) =
                    panels::focus_panel::hit_test(&self.focus, x_css, y_css, win_w)
                {
                    match hit {
                        panels::focus_panel::FocusHit::TogglePause => {
                            self.focus.timer.toggle_pause();
                            if self.focus.timer.running {
                                let now_ms =
                                    self.epoch.elapsed().as_secs_f64() * 1000.0;
                                self.focus.tick(now_ms);
                            }
                        }
                        panels::focus_panel::FocusHit::Exit => self.focus.exit(),
                    }
                    self.request_redraw();
                    return;
                }
            }

            // Picture-in-picture window (task #21): floating draggable
            // card. `Г—` closes, the centre button toggles play/pause, the
            // title bar starts a drag, the body swallows the click.
            if self.pip.active
                && let Some(hit) = panels::pip_window::hit_test(&self.pip, x_css, y_css)
            {
                match hit {
                    panels::pip_window::PipHit::Close => self.pip.close(),
                    panels::pip_window::PipHit::PlayPause => self.pip.toggle_play(),
                    panels::pip_window::PipHit::Header => {
                        self.pip.begin_drag(x_css, y_css);
                    }
                    panels::pip_window::PipHit::Body => {}
                }
                self.request_redraw();
                return;
            }

            // CC-5 (docs/tasks/p1-css-chrome.md): the engine-drawn
            // chrome (CC-4) covers the whole top-strip/sidebar area the
            // legacy hit-testers below assume вЂ” falling through to both
            // would double-count the same physical click (nothing
            // paints the legacy geometry to click on, yet their y/x-range
            // checks don't know that). Route exclusively through the
            // chrome hit-test + `data-action` dispatch instead; a click
            // outside the chrome's own opaque area (i.e. on real page
            // content, including any floating popover panel drawn above
            // it вЂ” those stay positioned within the page-content rect)
            // is left unhandled here and falls through unchanged to the
            // panel checks below.
            if self.point_over_chrome(x_css, y_css) {
                let hit = self.chrome_hit_test(x_css, y_css);
                self.chrome_active_nid = hit.as_ref().map(|r| r.node);
                self.relayout_chrome_host();
                if let Some(hit) = hit {
                    // CC-7: `.omnibox`/`#omniInput` carries no
                    // `data-action` (nothing to translate an
                    // `onfocus` handler from вЂ” the frozen design
                    // reference has none either, see CC-7 in
                    // docs/tasks/p1-css-chrome.md) вЂ” special-cased
                    // here exactly like the legacy
                    // `toolbar::ToolbarHit::Omnibox` branch it
                    // mirrors: a no-op while already open so an
                    // in-progress edit/dropdown selection isn't reset.
                    let omni_input = self
                        .chrome_doc
                        .as_ref()
                        .and_then(|(doc, _)| doc.find_by_id(lumen_chrome::ids::OMNI_INPUT));
                    if omni_input.is_some_and(|id| hit.path.contains(&id)) {
                        if !self.address_bar.is_open() {
                            self.hint.close();
                            let current = self.current_display_url().to_owned();
                            self.address_bar.open(&current);
                            // CC-7: the relayout above ran before
                            // `open()` вЂ” redo it so the
                            // `:focus-within` ring/caret show on
                            // this same click, not one input
                            // later (see the matching comment in
                            // `Self::handle_address_bar_key`).
                            self.relayout_chrome_host();
                        }
                    } else if let Some((nid, action)) = self.chrome_action_at(&hit) {
                        self.dispatch_chrome_action(nid, action, event_loop);
                    }
                }
                self.request_redraw();
                return;
            }
            // CC-15-3: the legacy tab-bar/toolbar left-click dispatch
            // (tab switch/close/adblock, archive/layout/settings
            // buttons, toolbar buttons) lived here вЂ” removed along with
            // the paint/hit-test functions it called, unreachable since
            // CC-4 routed those clicks through `chrome_hit_test` above.
            // Archive panel: close on click below tab bar when open.
            if self.archive.visible {
                let win_w = self.viewport_width_css();
                match tabs::archive::hit_test_panel(
                    &self.archive,
                    x_css,
                    y_css,
                    win_w,
                    toolbar::CHROME_H,
                ) {
                    Some(tabs::archive::ArchiveHit::Restore(id)) => {
                        if let Some(entry) = self.archive.take(id)
                            && !entry.url.is_empty()
                        {
                            self.navigate_to(PageSource::Url(entry.url));
                        }
                        self.archive.close();
                        self.request_redraw();
                        return;
                    }
                    Some(tabs::archive::ArchiveHit::Dismiss(id)) => {
                        self.archive.take(id);
                        self.request_redraw();
                        return;
                    }
                    Some(tabs::archive::ArchiveHit::Inside) => {
                        self.request_redraw();
                        return;
                    }
                    Some(tabs::archive::ArchiveHit::Outside) | None => {
                        self.archive.close();
                        self.request_redraw();
                    }
                }
            }

            // Vertical tab panel: intercept clicks within its (possibly
            // cross-docked) area. Panel hit-test is left-relative, so map
            // the window x into panel-local space.
            let vt_w = self.panel_layout.width_for(
                panel_layout::ID_VERTICAL_TABS,
                panels::vertical_tabs::PANEL_WIDTH,
            );
            let vt_origin = self
                .dock_origin_x(self.sidebar_dock_side(panel_layout::ID_VERTICAL_TABS), vt_w);
            if self.vertical_tabs.visible
                && x_css >= vt_origin
                && x_css < vt_origin + vt_w
            {
                let win_h = self.viewport_height_css() + toolbar::CHROME_H;
                match panels::vertical_tabs::hit_test(
                    &self.tab_strip,
                    x_css - vt_origin,
                    y_css,
                    toolbar::CHROME_H,
                    win_h,
                    self.vertical_tabs.scroll_y,
                    vt_w,
                ) {
                    Some(panels::vertical_tabs::VTabHit::Tab(idx)) => {
                        self.switch_tab(idx);
                    }
                    Some(panels::vertical_tabs::VTabHit::Close(idx)) => {
                        self.close_tab(idx, event_loop);
                    }
                    Some(panels::vertical_tabs::VTabHit::Empty) | None => {}
                }
                return;
            }

            // Tree-style tab panel: intercept clicks within its (possibly
            // cross-docked) area, mapping window x into panel-local space.
            let tt_w = self.panel_layout.width_for(
                panel_layout::ID_TREE_TABS,
                panels::tree_tabs::PANEL_WIDTH,
            );
            let tt_origin = self
                .dock_origin_x(self.sidebar_dock_side(panel_layout::ID_TREE_TABS), tt_w);
            if self.tree_tabs.visible
                && x_css >= tt_origin
                && x_css < tt_origin + tt_w
            {
                let win_h = self.viewport_height_css() + toolbar::CHROME_H;
                match panels::tree_tabs::hit_test(
                    &self.tab_strip,
                    &self.tree_tabs,
                    x_css - tt_origin,
                    y_css,
                    toolbar::CHROME_H,
                    win_h,
                    tt_w,
                ) {
                    Some(panels::tree_tabs::TreeTabHit::Tab(idx)) => {
                        self.switch_tab(idx);
                    }
                    Some(panels::tree_tabs::TreeTabHit::Close(idx)) => {
                        self.close_tab(idx, event_loop);
                    }
                    Some(panels::tree_tabs::TreeTabHit::Arrow(tab_id)) => {
                        let expanding = self.tree_tabs.collapsed.contains(&tab_id);
                        self.tree_tabs.toggle_collapsed(tab_id);
                        if expanding {
                            // Purge stale collapse entries for tabs that were closed
                            // while their parent subtree was hidden.
                            let subtree = tabs::tree::subtree_ids(
                                &self.tab_strip.tabs, tab_id,
                            );
                            let valid: std::collections::HashSet<usize> =
                                self.tab_strip.tabs.iter().map(|t| t.id).collect();
                            self.tree_tabs.collapsed.retain(|id| {
                                valid.contains(id) || !subtree.contains(id)
                            });
                        }
                        self.request_redraw();
                    }
                    Some(panels::tree_tabs::TreeTabHit::Empty) | None => {}
                }
                return;
            }

            // Profile switcher dropdown (DS-14): anchored below the
            // toolbar avatar button. BUG-403/BUG-404 class: use the
            // measured `page_offset()` Y, not the legacy-only
            // `toolbar::CHROME_H` constant, so the hit-test matches
            // wherever the popover is actually drawn (both chromes).
            if self.profile_menu.visible {
                let avatar_x = toolbar::avatar_x();
                let (_, page_y_offset) = self.page_offset();
                if let Some(hit) = panels::profile_menu::hit_test(
                    &self.profile_menu,
                    x_css,
                    y_css,
                    avatar_x,
                    page_y_offset,
                ) {
                    match hit {
                        panels::profile_menu::ProfileMenuHit::SwitchTo(id) => {
                            if self.profiles.set_active(Some(id)).is_ok() {
                                self.profile_menu.set_active(Some(id));
                                // DS-16: Anonymous is ephemeral вЂ” every
                                // time it becomes active, start from a
                                // fresh in-memory jar so no cookie
                                // survives a previous Anonymous session.
                                if self.active_profile_is_anonymous() {
                                    self.anonymous_cookie_jar = Arc::new(
                                        lumen_storage::CookieJar::open_in_memory()
                                            .expect("anonymous_cookie_jar reset"),
                                    );
                                }
                            }
                            self.profile_menu.visible = false;
                            // CC-6: re-sync the CSS chrome's data-profile (no-op off the flag).
                            self.relayout_chrome_host();
                        }
                        panels::profile_menu::ProfileMenuHit::Empty => {}
                    }
                    self.request_redraw();
                    return;
                }
            }

            // Shields floating panel (7C.4): top-right overlay.
            if self.shields.visible {
                let win_w = self.viewport_width_css();
                let tab_h = toolbar::CHROME_H;
                if let Some(hit) = panels::shields_panel::hit_test(
                    &self.shields,
                    x_css,
                    y_css,
                    win_w,
                    tab_h,
                ) {
                    match hit {
                        panels::shields_panel::ShieldsHit::Toggle => {
                            // BUG-411: same per-site flip the engine
                            // popover's switch performs (this legacy
                            // hit-test is still ungated вЂ” BUG-404).
                            self.shields.toggle_current_site();
                            self.sync_adblock_filter();
                            self.request_redraw();
                        }
                        panels::shields_panel::ShieldsHit::Close => {
                            self.shields.visible = false;
                            self.request_redraw();
                        }
                        panels::shields_panel::ShieldsHit::Empty => {}
                    }
                    return;
                }
            }

            // Privacy network panel (V5): right-docked overlay.
            if self.privacy.visible {
                let tab_h = toolbar::CHROME_H;
                let win_w = self.viewport_width_css();
                let win_h = self.viewport_height_css() + tab_h;
                match panels::privacy_panel::hit_test(
                    &self.privacy,
                    x_css,
                    y_css,
                    win_w,
                    win_h,
                    tab_h,
                ) {
                    panels::privacy_panel::PrivacyHit::Close => {
                        self.privacy.visible = false;
                        self.request_redraw();
                        return;
                    }
                    // Swallow clicks inside the panel so they don't reach
                    // the page underneath.
                    panels::privacy_panel::PrivacyHit::Inside => return,
                    panels::privacy_panel::PrivacyHit::Outside => {}
                }
            }

            // Permission popover (7C.2): top-left overlay below tab bar.
            if self.permission.visible {
                let tab_h = toolbar::CHROME_H;
                if let Some(hit) = panels::permission_panel::hit_test(
                    &self.permission,
                    x_css,
                    y_css,
                    tab_h,
                ) {
                    match hit {
                        panels::permission_panel::PermissionHit::Toggle(kind) => {
                            self.permission.cycle_permission(kind);
                            self.request_redraw();
                        }
                        panels::permission_panel::PermissionHit::Close => {
                            self.permission.visible = false;
                            self.request_redraw();
                        }
                        panels::permission_panel::PermissionHit::Empty => {}
                    }
                    return;
                }
            }

            // В§12.3 Read-later panel (Ctrl+Shift+R): right-docked overlay.
            if self.read_later_panel.visible {
                use panels::read_later_panel::ReadLaterHit;
                let win_w = self.viewport_width_css();
                let tab_h = toolbar::CHROME_H;
                let px = win_w - panels::read_later_panel::PANEL_W - 4.0;
                let py = tab_h + 4.0;
                let hit = panels::read_later_panel::hit_test(
                    x_css,
                    y_css,
                    px,
                    py,
                    &self.read_later_panel.entries,
                    self.read_later_panel.scroll_offset,
                );
                match hit {
                    ReadLaterHit::Close => {
                        self.read_later_panel.visible = false;
                        self.request_redraw();
                    }
                    ReadLaterHit::Open(id) => {
                        // Load from offline HTML snapshot.
                        if let Ok(Some(entry)) = self.read_later_store.get(id) {
                            let html = String::from_utf8_lossy(&entry.html_snapshot)
                                .into_owned();
                            let base_url = entry.url.clone();
                            let _ = self.read_later_store.set_status(
                                id,
                                lumen_knowledge::ReadStatus::Read,
                            );
                            self.read_later_panel.visible = false;
                            self.navigate_to(PageSource::Snapshot { html, base_url });
                        }
                    }
                    ReadLaterHit::Delete(id) => {
                        let _ = self.read_later_store.delete(id);
                        self.refresh_read_later();
                        self.request_redraw();
                    }
                    ReadLaterHit::Inside => { /* swallow */ }
                    ReadLaterHit::Outside => {
                        self.read_later_panel.visible = false;
                        self.request_redraw();
                    }
                }
                return;
            }

            // CC-10b/CC-15-6: the legacy bookmark-manager overlay's click
            // hit-test lived here, gated off the rollback flag вЂ”
            // `#view-bookmarks` in the engine chrome owns it now.

            // Accessibility settings panel (E-2): centred overlay.
            if self.a11y_panel.visible {
                let win_w = self.viewport_width_css();
                let win_h = self.viewport_height_css();
                use panels::a11y_panel::A11yHit;
                let hit = panels::a11y_panel::hit_test(
                    &self.a11y_panel,
                    x_css,
                    y_css,
                    win_w,
                    win_h,
                );
                match hit {
                    A11yHit::Close => {
                        let _ = self.a11y_store.apply_snapshot(&self.a11y_panel.draft);
                        self.a11y_panel.visible = false;
                        self.deliver_a11y_media_changes();
                        // Re-style with the (possibly toggled) forced-colors pref.
                        // Async-safe (M2.2b-6): closing the panel only shifts
                        // chrome + re-evaluates forced-colors; no page-geometry
                        // read follows (just `request_redraw` + `return`).
                        self.relayout_chrome();
                    }
                    A11yHit::FontMultiplier(v) => {
                        self.a11y_panel.draft.font_size_multiplier = v as f64;
                    }
                    A11yHit::ReducedMotion => {
                        self.a11y_panel.draft.reduced_motion =
                            !self.a11y_panel.draft.reduced_motion;
                    }
                    A11yHit::ForcedColors => {
                        self.a11y_panel.draft.forced_colors =
                            !self.a11y_panel.draft.forced_colors;
                    }
                    A11yHit::CursorSizeOption(size) => {
                        self.a11y_panel.draft.cursor_size = size;
                    }
                    A11yHit::Inside => { /* swallow */ }
                    A11yHit::Outside => {
                        let _ = self.a11y_store.apply_snapshot(&self.a11y_panel.draft);
                        self.a11y_panel.visible = false;
                        self.deliver_a11y_media_changes();
                        // Re-style with the (possibly toggled) forced-colors pref.
                        // Async-safe (M2.2b-6): closing the panel only shifts
                        // chrome + re-evaluates forced-colors; no page-geometry
                        // read follows (just `request_redraw` + `return`).
                        self.relayout_chrome();
                    }
                }
                self.request_redraw();
                return;
            }

            // CC-10b/CC-15-6: the legacy print-dialog click hit-test lived
            // here, gated off the rollback flag вЂ” the engine chrome's own
            // print panel owns it now.

            // CC-10b/CC-15-6: the legacy settings-panel click hit-test lived
            // here, gated off the rollback flag вЂ” `#view-settings` in the
            // engine chrome owns it now.

            // Keyboard shortcuts panel (В§D-4): centred overlay.
            if self.shortcuts_panel.visible {
                let win_w = self.viewport_width_css();
                let win_h = self.viewport_height_css();
                let kp_x = (win_w - panels::shortcuts_panel::PANEL_W) * 0.5;
                let kp_y = (win_h - panels::shortcuts_panel::PANEL_H) * 0.5;
                use panels::shortcuts_panel::ShortcutsHit;
                let lx = x_css - kp_x;
                let ly = y_css - kp_y;
                if (0.0..panels::shortcuts_panel::PANEL_W).contains(&lx)
                    && (0.0..panels::shortcuts_panel::PANEL_H).contains(&ly)
                {
                    match self.shortcuts_panel.hit_test(lx, ly) {
                        ShortcutsHit::Close => {
                            self.shortcuts_panel.close();
                        }
                        ShortcutsHit::StartRebind(idx) => {
                            self.shortcuts_panel.rebinding = Some(idx);
                        }
                        ShortcutsHit::Consumed => {}
                    }
                } else {
                    self.shortcuts_panel.close();
                }
                self.request_redraw();
                return;
            }

            // CC-10b/CC-15-6: the legacy certificate-panel click hit-test
            // lived here, gated off the rollback flag вЂ” the engine chrome's
            // own cert popover owns it now.

            // CC-10b/CC-15-6: the legacy history-panel click hit-test lived
            // here, gated off the rollback flag вЂ” `#view-history` in the
            // engine chrome owns it now.

            // Note viewer overlay (В§12.2, GG-2): click [Г—] to close.
            if self.note_viewer.visible {
                let win_size = self.window.as_ref().map_or((1024, 720), |w| {
                    let s = w.inner_size();
                    (s.width, s.height)
                });
                if let Some(hit) = self.note_viewer.hit_test(x_css, y_css, win_size) {
                    match hit {
                        panels::note_viewer::NoteHit::Close => {
                            self.note_viewer.close();
                            self.request_redraw();
                        }
                        panels::note_viewer::NoteHit::Body => {}
                    }
                    return;
                }
            }

            // CC-10b/CC-15-6: the legacy AI-sidebar click hit-test lived
            // here, gated off the rollback flag вЂ” `#rightSidebar` in the
            // engine chrome owns it now.

            // CC-10b/CC-15-6: the legacy web-sidebar click hit-test lived
            // here, gated off the rollback flag вЂ” `#rightSidebar` in the
            // engine chrome owns it now.

            // Workspace switcher bar (7A.3): clicks in the bottom bar area.
            if self.workspace_panel.visible {
                let win_w = self.viewport_width_css();
                let win_h = self.viewport_height_css()
                    + toolbar::CHROME_H
                    + panels::workspace_panel::SWITCHER_HEIGHT;
                if let Some(hit) = panels::workspace_panel::hit_test(
                    &self.workspace_panel,
                    x_css,
                    y_css,
                    win_w,
                    win_h,
                ) {
                    match hit {
                        panels::workspace_panel::WorkspaceHit::SwitchTo(id) => {
                            self.workspace_panel.set_active(Some(id));
                            self.request_redraw();
                        }
                        panels::workspace_panel::WorkspaceHit::DeleteWorkspace(id) => {
                            // Never delete the last workspace вЂ” require at least one.
                            if self.workspace_panel.workspaces.len() > 1 {
                                let _ = self.workspaces.delete(id);
                                self.refresh_workspaces();
                                // If the deleted workspace was active, switch to first.
                                if self.workspace_panel.active_id == Some(id) {
                                    let first_id = self
                                        .workspace_panel
                                        .workspaces
                                        .first()
                                        .map(|w| w.id);
                                    self.workspace_panel.set_active(first_id);
                                }
                                self.request_redraw();
                            }
                        }
                        panels::workspace_panel::WorkspaceHit::NewWorkspace => {
                            let idx = self.workspace_panel.workspaces.len();
                            let name = format!("Workspace {}", idx + 1);
                            let color = panels::workspace_panel::default_color_for_index(idx);
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs() as i64)
                                .unwrap_or(0);
                            if let Ok(id) =
                                self.workspaces.create(&name, color, "", None, now)
                            {
                                self.refresh_workspaces();
                                self.workspace_panel.set_active(Some(id));
                            }
                            self.request_redraw();
                        }
                        panels::workspace_panel::WorkspaceHit::Empty => {}
                    }
                    return;
                }
            }

            // Split-view focus routing: clicking in the right pane
            // transfers focus there; clicking in the left pane transfers
            // focus back. Right-pane clicks do not navigate (frozen pane).
            if self.split_view.is_some() {
                // Pre-compute before mutable borrow of split_view.
                let split_x = (self.viewport_width_css() / 2.0).floor();
                if let Some(ref mut sv) = self.split_view {
                    sv.focus_at(x_css, split_x);
                    if sv.cursor_in_right(x_css, split_x) {
                        // Right pane clicked вЂ” focus only, no link navigation.
                        self.request_redraw();
                        return;
                    }
                }
                // Left pane clicked вЂ” fall through to normal handling below.
            }

            let vh = self.viewport_height_css();
            match scrollbar::classify_track_click(
                x_css,
                y_css,
                self.scroll_y,
                self.content_height,
                self.viewport_width_css(),
                vh,
            ) {
                scrollbar::TrackClick::Thumb => {
                    self.scroll_drag = Some(scrollbar::ScrollDrag::new(
                        self.scroll_y,
                        y_css,
                    ));
                }
                scrollbar::TrackClick::Above => {
                    // РљР»РёРє РїРѕ track РІС‹С€Рµ thumb-Р° вЂ” РїСЂС‹Р¶РѕРє РЅР° СЃС‚СЂР°РЅРёС†Сѓ РІРІРµСЂС….
                    self.scroll_by_smooth(-page_step(vh));
                }
                scrollbar::TrackClick::Below => {
                    // РљР»РёРє РїРѕ track РЅРёР¶Рµ thumb-Р° вЂ” РїСЂС‹Р¶РѕРє РЅР° СЃС‚СЂР°РЅРёС†Сѓ РІРЅРёР·.
                    self.scroll_by_smooth(page_step(vh));
                }
                scrollbar::TrackClick::None => {
                    self.handle_click_at(x_css, y_css);
                }
            }
        } else {
            // Released вЂ” Р·Р°РІРµСЂС€Р°РµРј drag (РµСЃР»Рё Р±С‹Р») Рё СЃР±СЂР°СЃС‹РІР°РµРј resize.
            self.resize_active = None;
            // F2-6: end a docked-panel resize drag and persist the layout.
            if let Some((_, id)) = self.panel_resize.take() {
                self.panel_layout.save();
                // Reflow the web sidebar page to its new width: its content
                // is a frozen display list, so unlike the AI panel (drawn
                // procedurally) it does not reflow during the drag itself.
                if id == panel_layout::ID_SIDEBAR {
                    self.relayout_sidebar();
                }
                self.update_cursor_icon();
                return;
            }
            // CSS :active вЂ” clear on release.
            if self.active_nid.is_some() {
                self.active_nid = None;
                // ADR-016 M2.2b-5: :active clear is async-safe вЂ” the
                // mouseup/pointerup JS events below target `hovered_nid`,
                // not this reflow's geometry.
                self.relayout_chrome();
                self.request_redraw();
            }
            // Fire mouseup + pointerup on the hovered DOM element.
            // Per W3C UI Events В§17.6 + Pointer Events L2 В§10.
            #[cfg(feature = "v8")]
            if let (Some(hov), Some(pos)) = (self.hovered_nid, self.cursor_position) {
                // Ph3 pointer-events-l3: flush queued pointermove samples
                // first so they precede pointerup in dispatch order.
                self.flush_pointer_moves();
                let dpr = self.renderer.as_ref()
                    .map_or(1.0_f32, |r| r.scale_factor() as f32).max(1e-6);
                let xu = (pos.x as f32) / dpr;
                let yu = (pos.y as f32) / dpr;
                let hit_nid = hov.index() as u32;
                // Pointer Events L3 В§4.1: route pointerup to capture target if active.
                // ADR-016 M2.2c-2d: pre-dispatch capture-read С‡РµСЂРµР· `route_query_js`
                // (РїРѕРґ С„Р»Р°РіРѕРј вЂ” Р±Р»РѕРєРёСЂСѓСЋС‰РёР№ `query`; РІРЅРµС€РЅРёР№ `None` = РІРµС‚РєР° В«Р±РµР· JSВ»
                // в†’ `hit_nid`, РєР°Рє РїСЂРµР¶РЅРёР№ `and_then(...).unwrap_or(hit_nid)`).
                let ptr_nid = route_query_js(
                    self.engine_thread.as_ref(),
                    self.js_ctx.as_ref(),
                    |c| c.pointer_capture_nid(),
                )
                .flatten()
                .unwrap_or(hit_nid);
                // Buffered moves must fire ahead of pointerup.
                self.flush_pointer_moves();
                self.js_pointer_event(ptr_nid, "pointerup", xu, yu, 0, 0);
                self.js_mouse_event(hit_nid, "mouseup", xu, yu, 0, 0);
                // Pointer Events L3 В§4.1: implicit release on pointerup.
                // Р§РёС‚Р°РµС‚СЃСЏ **РїРѕСЃР»Рµ** СѓР¶Рµ РјР°СЂС€СЂСѓС‚РёР·РёСЂРѕРІР°РЅРЅС‹С… pointerup/mouseup
                // eval-`task` вЂ” read-after-eval РїРѕСЂСЏРґРѕРє СЃРѕС…СЂР°РЅС‘РЅ.
                if let Some(cap_nid) = route_query_js(
                    self.engine_thread.as_ref(),
                    self.js_ctx.as_ref(),
                    |c| c.take_pointer_capture(),
                )
                .flatten()
                {
                    self.js_capture_event(cap_nid, "lostpointercapture");
                }
            }
            // BUG-480 срез 16: отпускание над содержимым фрейма — парная
            // ветка к `pointerdown`/`mousedown` выше. Захват указателя
            // (`pointer_capture_nid`) читается только у страницы: у фрейма
            // свой контекст, и его захват — отдельный срез очереди.
            #[cfg(feature = "v8")]
            if let (Some((f, n)), Some(pos)) = (self.hovered_frame, self.cursor_position) {
                self.flush_pointer_moves();
                let dpr = self.renderer.as_ref()
                    .map_or(1.0_f32, |r| r.scale_factor() as f32).max(1e-6);
                let xu = (pos.x as f32) / dpr;
                let yu = (pos.y as f32) / dpr;
                let at = self
                    .pointer_target(xu, yu)
                    .frame
                    .map_or((0.0, 0.0), |t| (t.local.x, t.local.y));
                let nid = n.index() as u32;
                self.frame_pointer_event(f, nid, "pointerup", at, (0, 0));
                self.frame_mouse_event(f, nid, "mouseup", at, (0, 0));
            }
            // HTML5 DnD (PH3-9): fire drop + dragend on release.
            #[cfg(feature = "v8")]
            if let Some(dnd) = self.dnd_state.take() && dnd.active {
                let dpr = self
                    .renderer
                    .as_ref()
                    .map_or(1.0_f32, |r| r.scale_factor() as f32)
                    .max(1e-6);
                let (xu, yu) = self.cursor_position
                    .map(|p| ((p.x as f32) / dpr, (p.y as f32) / dpr))
                    .unwrap_or((dnd.press_x, dnd.press_y));
                // drop fires on the element under the cursor (if any).
                if let Some(ov) = dnd.over_nid {
                    self.js_drag_event(ov.index() as u32, "drop", xu, yu);
                }
                // dragend always fires on the source element.
                self.js_drag_event(dnd.src_nid.index() as u32, "dragend", xu, yu);
            }

            // Tab drag-and-drop (В§O-9): resolve the drop and reorder.
            if let Some(drag) = self.tab_drag.take()
                && drag.active {
                    let dpr = self
                        .renderer
                        .as_ref()
                        .map_or(1.0_f32, |r| r.scale_factor() as f32)
                        .max(1e-6);
                    let win_w = self.viewport_width_css();
                    let tab_area_w = win_w
                        - tabs::archive::ARCHIVE_BTN_W
                        - tabs::strip::LAYOUT_BTN_W
                        - tabs::strip::SETTINGS_BTN_W;
                    let release_x = self.cursor_position
                        .map(|p| (p.x as f32) / dpr)
                        .unwrap_or(drag.ghost_x);
                    let updated = tabs::strip::TabDragState {
                        ghost_x: release_x,
                        ..drag
                    };
                    let dst = updated.drop_target(self.tab_strip.len(), tab_area_w);
                    if dst != updated.src_idx {
                        self.tab_strip.move_tab(updated.src_idx, dst);
                    }
                    self.request_redraw();
                }
            // CC-15-6: the bookmark drag-drop release handler lived here.
            // Its only drag source was the legacy overlay's press
            // hit-test, removed with the rollback flag вЂ” the engine
            // `#view-bookmarks` has no drag source yet (BUG-422).
            // End a PiP window drag (task #21).
            if self.pip.dragging() {
                self.pip.end_drag();
            }
            self.scroll_drag = None;
            // РљСѓСЂСЃРѕСЂ Р±С‹Р» В«Р·Р°С„РёРєСЃРёСЂРѕРІР°РЅВ» РєР°Рє Pointer РїРѕРєР° С‚СЏРЅСѓР»Рё
            // thumb; С‚РµРїРµСЂСЊ РїРµСЂРµСЃС‡РёС‚Р°РµРј РїРѕ hover-С‚РѕС‡РєРµ С‚РµРєСѓС‰РµРіРѕ
            // РїРѕР»РѕР¶РµРЅРёСЏ РєСѓСЂСЃРѕСЂР° (CursorMoved-event РЅР° release СЃР°Рј
            // РЅРµ РїСЂРёС…РѕРґРёС‚, РїРѕСЌС‚РѕРјСѓ РґРµР»Р°РµРј РІСЂСѓС‡РЅСѓСЋ).
            self.update_cursor_icon();
        }
    }
}
