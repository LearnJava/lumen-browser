//! Ветка `WindowEvent::MouseInput` цикла событий (SPLIT-SH2).
//!
//! Тело ветки вынесено из `Lumen::window_event` (`main.rs`) как есть, с
//! дедентом на 8 пробелов и без единой правки логики; строки внутри
//! многострочных строковых литералов дедент не затронул.

use crate::*;

impl Lumen {
    #[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn on_mouse_input(&mut self, event_loop: &MainHandle<'_>, state: ElementState, button: MouseButton) {
        // UX-CARET: a click shows the caret at once, even on the same spot.
        if state == ElementState::Pressed {
            self.caret_restart();
        }
        // GAP-LAYOUTSHIFT: mark real user input for the CLS `had_input` flag
        // (Layout Instability L1 §3) — any button press counts, not just the
        // one that ends up hit-testing something.
        if state == ElementState::Pressed {
            self.last_input_epoch_s = self.epoch.elapsed().as_secs_f32();
        }
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
                self.relayout_chrome_host();
            }
            // CC-4: right-click on a tab opens the tab context menu
            // instead of starting a mouse gesture.
            if state == ElementState::Pressed && !self.focus.active {
                // BUG-404: resolve the tab under the cursor via the
                // chrome hit-test + `data-tab-id` (the same
                // mechanism `ChromeAction::SelectTab` uses for a
                // left-click), not `tabs::strip::hit_test`'s legacy
                // geometry — the engine-drawn tab strip's real
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
                    self.relayout_chrome_host();
                    self.request_redraw();
                    return;
                }
            }
            // COMPAT-PANEL: правый клик по «Обновить» открывает окно сайта с
            // секцией «Сайт работает не так?» вместо жеста мыши.
            if state == ElementState::Pressed && !self.focus.active {
                let on_reload = self.chrome_hit_test(x_css, y_css).is_some_and(|hit| {
                    hit.path.iter().any(|&nid| self.chrome_data_attr(nid, "data-action").as_deref() == Some("reload"))
                });
                if on_reload {
                    self.open_fix_panel();
                    return;
                }
            }
            // P3-spell срез 3: right-click on a misspelled word in a
            // focused text input opens the spell suggestion menu instead
            // of starting a mouse gesture.
            if state == ElementState::Pressed && self.try_open_spell_menu(x_css, y_css) {
                self.request_redraw();
                return;
            }
            if state == ElementState::Pressed {
                self.gesture.begin(x_css, y_css);
            } else if state == ElementState::Released {
                let plain_click = self.gesture.is_plain_click();
                if let Some(action) = self.gesture.finish() {
                    self.execute_gesture_action(action, event_loop);
                } else if plain_click && self.try_open_page_menu(x_css, y_css) {
                    // COMPAT-HANDOFF: правый клик без протяжки — меню страницы.
                    self.request_redraw();
                }
            }
        } else if button == MouseButton::Middle {
            // UX-TAB-CLOSE-FREEZE: a middle click on a tab closes it.
            if state == ElementState::Pressed {
                self.close_tab_under_middle_click(event_loop);
                // UX-LINK-CLICKS: …and over the page it opens the link in a tab.
                self.open_link_under_middle_click();
            }
        } else if button != MouseButton::Left {
            // Back / forward — ignore.
        } else if state == ElementState::Pressed {
            // CSS :active — set immediately on press so :active rules apply.
            if self.active_nid != self.hovered_nid {
                self.active_nid = self.hovered_nid;
                // ADR-016 M2.2b-5: :active restyle is async-safe — the
                // click hit-test below reads the pre-:active layout (the
                // geometry the user pressed on), which is correct.
                self.relayout_chrome();
                self.request_redraw();
            }
            let Some(cursor) = self.cursor_position else {
                // Без CursorMoved-snapshot-а до Press — не знаем где
                // клик; bail out. Реалистично — Press всегда приходит
                // после CursorMoved, но защитимся.
                return;
            };
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            let x_css = (cursor.x as f32) / dpr;
            let y_css = (cursor.y as f32) / dpr;
            // CC-5: chrome's own `:active` — mirrors the page's
            // `active_nid` handling above but scoped to `chrome_layout`
            // (see `relayout_chrome_host`'s doc comment for why the two
            // documents can't share interactive thread-locals).
            if self.point_over_chrome(x_css, y_css) {
                self.chrome_active_nid = self.chrome_hit_test(x_css, y_css).map(|r| r.node);
                self.relayout_chrome_host();
                self.request_redraw();
            }
            // CC-18 срез 3: the floating control panel (`#demoBar`/
            // `#infoPanel`) is the top-most thing on screen — BUG-1059
            // paints it unclipped over everything else — and it is the
            // *only* surface `chrome_hit_test` cannot answer for, because
            // the same BUG-1059 fix detached its boxes out of
            // `chrome_layout`. Its press routing therefore runs first and
            // on its own detached trees: a press on the header starts a
            // drag (or resets the position on a double-click), a press on
            // one of its controls dispatches that control's own
            // `data-action`, and a press anywhere else on it is swallowed
            // because the panel is opaque.
            if self.floating_panel_press(x_css, y_css, event_loop) {
                return;
            }
            // UX-DIALOGS: модальный диалог страницы — остальное не кликается.
            if self.page_dialog.is_some() {
                return;
            }
            // CC-4: while the tab context menu is open it captures the
            // click — picking a row runs the action, anywhere else just
            // dismisses it. The click never reaches the page / panels.
            if self.tab_context_menu.is_open() {
                let action = self.chrome_hit_test(x_css, y_css).and_then(|hit| {
                    hit.path.iter().find_map(|&nid| {
                        self.chrome_data_attr(nid, "data-tab-ctx")
                            .and_then(|v| tabs::context_menu::MenuAction::from_attr(&v))
                    })
                });
                self.tab_context_menu.close();
                if let Some(action) = action {
                    self.exec_tab_menu_action(action, event_loop);
                }
                self.relayout_chrome_host();
                self.request_redraw();
                return;
            }
            // P3-spell срез 3: while the page spell menu is open it
            // captures the click — picking a row applies the correction /
            // dictionary action, anywhere else just dismisses it.
            if self.page_context_menu.is_open() {
                let action = self.chrome_hit_test(x_css, y_css).and_then(|hit| {
                    hit.path.iter().find_map(|&nid| {
                        self.chrome_data_attr(nid, "data-page-ctx")
                            .and_then(|v| self.page_context_menu.action_from_attr(&v))
                    })
                });
                if let Some(action) = action {
                    self.exec_spell_menu_action(action);
                }
                self.page_context_menu.close();
                self.relayout_chrome_host();
                self.request_redraw();
                return;
            }
            // CC-2: the download panel is a top-most bottom overlay — it
            // captures clicks on its buttons / close / body before they
            // reach the page. A click outside the panel closes it.
            if self.downloads.visible {
                let path = self.chrome_hit_test(x_css, y_css).map(|h| h.path).unwrap_or_default();
                let button = path.iter().find_map(|&nid| {
                    let action = self
                        .chrome_data_attr(nid, "data-dl")
                        .and_then(|v| lumen_chrome::ChromeDownloadAction::from_attr(&v));
                    let id = self.chrome_data_id(nid, "data-dl-id");
                    action.zip(id).map(|(a, id)| (a, id as u32))
                });
                let is_clear =
                    path.iter().any(|&nid| self.chrome_data_attr(nid, "data-dl").as_deref() == Some("clear"));
                let in_panel = path.iter().any(|&nid| {
                    self.chrome_doc
                        .as_ref()
                        .is_some_and(|(doc, _)| doc.get(nid).get_attr("id") == Some(lumen_chrome::ids::DOWNLOADS_PANEL))
                });
                if let Some((action, raw)) = button {
                    if let Some(id) = self.downloads.entries().iter().map(|e| e.id).find(|id| id.raw() == raw) {
                        match action {
                            lumen_chrome::ChromeDownloadAction::Open => {
                                self.downloads.open_download(id);
                            }
                            lumen_chrome::ChromeDownloadAction::Reveal => {
                                self.downloads.show_in_folder(id);
                            }
                            lumen_chrome::ChromeDownloadAction::Cancel => self.downloads.cancel(id),
                        }
                    }
                    self.relayout_chrome_host();
                    self.request_redraw();
                    return;
                }
                if is_clear {
                    self.downloads.clear_finished();
                    self.relayout_chrome_host();
                    self.request_redraw();
                    return;
                }
                // `×` (`data-action="toggle-downloads"`) falls through to the
                // chrome dispatch below; the rest of the panel swallows the
                // click, a click outside closes the popover.
                let on_action = path.iter().any(|&nid| {
                    self.chrome_doc
                        .as_ref()
                        .is_some_and(|(doc, _)| doc.get(nid).get_attr("data-action").is_some())
                });
                if in_panel && !on_action {
                    return;
                }
                if !in_panel {
                    self.downloads.close();
                    self.relayout_chrome_host();
                    self.request_redraw();
                    return;
                }
            }
            // UX-DBLCLICK: count this press within the OS double-click time
            // and zone; the count is `detail` of mousedown/mouseup/click.
            let clicks = self.click_streak.press(
                &platform::os_params::current(),
                self.epoch.elapsed().as_secs_f64() * 1000.0,
                x_css,
                y_css,
            );
            // Fire mousedown + pointerdown on the hovered DOM element.
            // Per W3C UI Events §17.6 + Pointer Events L2 §10 — fires before
            // any default action (click). Only when cursor is over page content.
            #[cfg(feature = "v8")]
            if let Some(hov) = self.hovered_nid {
                // Ph3 pointer-events-l3: flush queued pointermove samples
                // first so they precede pointerdown in dispatch order.
                self.flush_pointer_moves();
                let nid = hov.index() as u32;
                self.js_pointer_event(nid, "pointerdown", x_css, y_css, 0, 1);
                self.js_click_event(nid, "mousedown", (x_css, y_css), (0, 1), clicks);
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
                    .map_or((0.0, 0.0), |t| (t.client.x, t.client.y));
                let nid = n.index() as u32;
                self.frame_pointer_event(f, nid, "pointerdown", at, (0, 1));
                self.frame_mouse_event(f, nid, "mousedown", at, (0, 1));
            }
            // BUG-480 срез 23: `:active` под-документа — зеркало `active_nid`
            // выше. Ставится вне `#[cfg(feature = "v8")]`: нажатый вид не
            // зависит от того, есть ли у ребёнка скрипты.
            if self.active_frame != self.hovered_frame {
                self.active_frame = self.hovered_frame;
                self.refresh_frames(None);
            }

            // HTML5 DnD (PH3-9 / HTML LS §9.3.3): start candidate when the
            // pressed element is draggable.  Drag does not activate until the
            // cursor leaves the OS drag zone (handled in CursorMoved).
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

            // Command palette (task #23): modal — captures every click.
            // A click on a row activates it; a click on the scrim closes.
            if self.command_palette.visible {
                // `data-cp-index` on a `.cp-row` activates it; a hit on the
                // `#cpOverlay` scrim itself (not `.cp-box`) closes.
                let hit = self.chrome_hit_test(x_css, y_css);
                let row = hit.as_ref().and_then(|h| {
                    h.path.iter().find_map(|&nid| self.chrome_data_id(nid, "data-cp-index"))
                });
                let on_scrim = hit.as_ref().is_none_or(|h| {
                    h.path.first().and_then(|&nid| self.chrome_node_id_is(nid, lumen_chrome::ids::CP_OVERLAY))
                        .unwrap_or(true)
                });
                if let Some(idx) = row {
                    self.command_palette.selected = idx as usize;
                    if let Some(item) = self.command_palette.selected_item().cloned() {
                        self.command_palette.close();
                        self.activate_palette(&item, event_loop);
                    }
                } else if on_scrim {
                    self.command_palette.close();
                }
                self.request_redraw();
                return;
            }

            // Focus mode pill: `.focus-timer` in the engine chrome
            // (`toggle-focus-timer` pauses, `toggle-focus` exits).

            // Picture-in-picture window (task #21): floating draggable
            // card. `×` closes, the centre button toggles play/pause, the
            // title bar starts a drag, the body swallows the click.
            if self.pip.active
                && let Some(hit) = self.chrome_hit_test(x_css, y_css).and_then(|hit| {
                    hit.path.iter().find_map(|&nid| {
                        self.chrome_data_attr(nid, "data-pip")
                            .and_then(|v| panels::pip_window::PipHit::from_attr(&v))
                    })
                })
            {
                match hit {
                    panels::pip_window::PipHit::Close => self.pip.close(),
                    panels::pip_window::PipHit::PlayPause => self.pip.toggle_play(),
                    panels::pip_window::PipHit::Header => {
                        self.pip.begin_drag(x_css, y_css);
                    }
                    panels::pip_window::PipHit::Body => {}
                }
                self.relayout_chrome_host();
                self.request_redraw();
                return;
            }

            // CC-5 (docs/tasks/p1-css-chrome.md): the engine-drawn
            // chrome (CC-4) covers the whole top-strip/sidebar area the
            // legacy hit-testers below assume — falling through to both
            // would double-count the same physical click (nothing
            // paints the legacy geometry to click on, yet their y/x-range
            // checks don't know that). Route exclusively through the
            // chrome hit-test + `data-action` dispatch instead; a click
            // outside the chrome's own opaque area (i.e. on real page
            // content, including any floating popover panel drawn above
            // it — those stay positioned within the page-content rect)
            // is left unhandled here and falls through unchanged to the
            // panel checks below.
            // BUG-934: dispatch now shared with automation clicks via
            // `try_dispatch_chrome_click` — see its doc comment for why this
            // synchronous-on-press branch can be shared without a matching
            // `Released`-side counterpart.
            if self.try_dispatch_chrome_click(x_css, y_css, event_loop) {
                return;
            }
            // CC-15-3: the legacy tab-bar/toolbar left-click dispatch
            // (tab switch/close/adblock, archive/layout/settings
            // buttons, toolbar buttons) lived here — removed along with
            // the paint/hit-test functions it called, unreachable since
            // CC-4 routed those clicks through `chrome_hit_test` above.
            // Archive panel: `#archivePanel` in the engine chrome
            // (`toggle-archive` / `archive-restore` / `archive-dismiss`).

            // Privacy network panel: `#privacyPanel` in the engine chrome
            // (`bind_privacy`, `toggle-privacy`).

            // §12.3 Read-later panel: `#readLaterPanel` in the engine chrome
            // (`read-later-open` / `read-later-delete` / `toggle-read-later`).

            // CC-10b/CC-15-6: the legacy bookmark-manager overlay's click
            // hit-test lived here, gated off the rollback flag —
            // `#view-bookmarks` in the engine chrome owns it now.

            // UX-CHROME-PANELS: the accessibility panel's click hit-test lived
            // here - `#accessOverlay` in the engine chrome owns it now.
            // CC-10b/CC-15-6: the legacy print-dialog click hit-test lived
            // here, gated off the rollback flag — the engine chrome's own
            // print panel owns it now.

            // CC-10b/CC-15-6: the legacy settings-panel click hit-test lived
            // here, gated off the rollback flag — `#view-settings` in the
            // engine chrome owns it now.

            // UX-CHROME-PANELS: the shortcuts panel's click hit-test lived
            // here - `#shortcutsOverlay` in the engine chrome owns it now.

            // CC-10b/CC-15-6: the legacy certificate-panel click hit-test
            // lived here, gated off the rollback flag — the engine chrome's
            // own cert popover owns it now.

            // CC-10b/CC-15-6: the legacy history-panel click hit-test lived
            // here, gated off the rollback flag — `#view-history` in the
            // engine chrome owns it now.

            // Note viewer: `#noteOverlay` in the engine chrome (`close-note`).

            // CC-10b/CC-15-6: the legacy AI-sidebar click hit-test lived
            // here, gated off the rollback flag — `#rightSidebar` in the
            // engine chrome owns it now.

            // CC-10b/CC-15-6: the legacy web-sidebar click hit-test lived
            // here, gated off the rollback flag — `#rightSidebar` in the
            // engine chrome owns it now.

            // Workspace switcher bar: `#workspaceBar` in the engine chrome
            // (`select-workspace` / `delete-workspace` / `add-workspace`).

            // Split-view focus routing: clicking in the right pane
            // transfers focus there; clicking in the left pane transfers
            // focus back. Right-pane clicks do not navigate (frozen pane).
            if self.split_view.is_some() {
                // Pre-compute before mutable borrow of split_view.
                let split_x = (self.viewport_width_css() / 2.0).floor();
                if let Some(ref mut sv) = self.split_view {
                    sv.focus_at(x_css, split_x);
                    if sv.cursor_in_right(x_css, split_x) {
                        // Right pane clicked — focus only, no link navigation.
                        self.request_redraw();
                        return;
                    }
                }
                // Left pane clicked — fall through to normal handling below.
            }

            // FRAME-3 remainder: собственный scrollbar фрейма первым —
            // "самый глубокий скроллер под курсором должен победить" (тот же
            // принцип, что уже применён к колесу над overflow-контейнером
            // ВНУТРИ под-документа, `mouse_wheel.rs`). Страничный scrollbar
            // ниже проверяется, только если ни один фрейм под точкой не
            // ответил.
            if let Some((fidx, click, local_y)) = self.classify_frame_scrollbar_click(x_css, y_css) {
                match click {
                    scrollbar::TrackClick::Thumb => {
                        self.frame_scroll_drag = Some((
                            fidx,
                            scrollbar::ScrollDrag::new(self.frames[fidx].scroll_y, local_y),
                        ));
                    }
                    scrollbar::TrackClick::Above => {
                        // Прокрутка фрейма мгновенна (своей анимации нет,
                        // см. `apply_frame_scroll`) — page-jump без smooth.
                        let step = page_step(self.frames[fidx].viewport.height);
                        self.apply_frame_scroll(fidx, self.frames[fidx].scroll_y - step);
                    }
                    scrollbar::TrackClick::Below => {
                        let step = page_step(self.frames[fidx].viewport.height);
                        self.apply_frame_scroll(fidx, self.frames[fidx].scroll_y + step);
                    }
                    scrollbar::TrackClick::None => {}
                }
                return;
            }

            // UX-SCROLLBAR: горизонтальная полоса страницы (ниже фреймов,
            // выше вертикальной: уголок принадлежит вертикальной).
            match self.hscroll_hit(x_css, y_css) {
                scrollbar::TrackClick::Thumb => {
                    self.hscroll_drag = Some(scrollbar::ScrollDrag::new(self.scroll_x, x_css));
                    self.request_redraw();
                    return;
                }
                scrollbar::TrackClick::Above => {
                    self.scroll_x_by(-page_step(self.page_content_width_css()));
                    return;
                }
                scrollbar::TrackClick::Below => {
                    self.scroll_x_by(page_step(self.page_content_width_css()));
                    return;
                }
                scrollbar::TrackClick::None => {}
            }

            let vh = self.viewport_height_css();
            match scrollbar::classify_track_click(
                x_css,
                self.page_local_y(y_css),
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
                    self.request_redraw();
                }
                scrollbar::TrackClick::Above => {
                    // Клик по track выше thumb-а — прыжок на страницу вверх.
                    self.scroll_by_smooth(-page_step(vh));
                }
                scrollbar::TrackClick::Below => {
                    // Клик по track ниже thumb-а — прыжок на страницу вниз.
                    self.scroll_by_smooth(page_step(vh));
                }
                scrollbar::TrackClick::None => {
                    self.handle_click_at(x_css, y_css);
                    // FRAME-7 остаток: arm mouse-drag text selection if the
                    // click above landed on a typeable field — `focused_node`/
                    // `focused_frame` are this click's outcome by now.
                    self.begin_text_drag_select(x_css, y_css);
                    // CSS UI L4 §6.2: page-text drag selection with
                    // `user-select` enforced; a field drag armed above wins.
                    self.begin_doc_select(x_css, y_css);
                    // UX-DBLCLICK: the second press of a streak fires
                    // `dblclick` right after its `click`, the second and
                    // third select a word and a paragraph.
                    #[cfg(feature = "v8")]
                    if clicks == 2
                        && let Some(hov) = self.hovered_nid
                    {
                        self.js_click_event(hov.index() as u32, "dblclick", (x_css, y_css), (0, 0), 2);
                    }
                    self.select_by_click_count(x_css, y_css, clicks);
                }
            }
        } else {
            // Released — завершаем drag (если был) и сбрасываем resize.
            self.resize_active = None;
            // BUG-480 срез 23: `:active` под-документа снимается на отпускании
            // ровно как страничный ниже.
            if self.active_frame.is_some() {
                self.active_frame = None;
                self.refresh_frames(None);
            }
            // CSS :active — clear on release.
            if self.active_nid.is_some() {
                self.active_nid = None;
                // ADR-016 M2.2b-5: :active clear is async-safe — the
                // mouseup/pointerup JS events below target `hovered_nid`,
                // not this reflow's geometry.
                self.relayout_chrome();
                self.request_redraw();
            }
            // Fire mouseup + pointerup on the hovered DOM element.
            // Per W3C UI Events §17.6 + Pointer Events L2 §10.
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
                // Buffered moves must fire ahead of pointerup.
                self.flush_pointer_moves();
                // Pointer Events L3 §4.1: pointerup goes to the capture target, and
                // the capture is released implicitly afterwards.
                self.js_pointer_release(hit_nid, xu, yu);
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
                    .map_or((0.0, 0.0), |t| (t.client.x, t.client.y));
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

            // Tab drag-and-drop (§O-9): resolve the drop and reorder.
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
            // hit-test, removed with the rollback flag — the engine
            // `#view-bookmarks` has no drag source yet (BUG-422).
            // End a PiP window drag (task #21).
            if self.pip.dragging() {
                self.pip.end_drag();
            }
            // CC-18 срез 3: same for a floating-panel header drag — the
            // offset it produced stays, only the tracking stops.
            self.floating_panel_end_drag();
            self.scroll_drag = None;
            self.hscroll_drag = None;
            self.frame_scroll_drag = None;
            // FRAME-7 остаток: end an in-progress mouse-drag text selection —
            // the selection itself stays, only the drag tracking stops.
            self.text_drag = None;
            self.doc_select = None;
            // Курсор был «зафиксирован» как Pointer пока тянули
            // thumb; теперь пересчитаем по hover-точке текущего
            // положения курсора (CursorMoved-event на release сам
            // не приходит, поэтому делаем вручную).
            self.update_cursor_icon();
        }
    }
}
