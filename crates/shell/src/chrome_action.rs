//! Browser chrome action dispatch (CC-5, BUG-934): the chrome hit-test entry
//! `try_dispatch_chrome_click` and the `data-action` dispatcher
//! `dispatch_chrome_action` it routes to.
//!
//! Moved verbatim out of `chrome_ui.rs` (file-size gate, docs/lint-policy.md
//! §5.1). Method bodies are unchanged; only module path and visibility differ.

use crate::*;
use crate::chrome_ui::ChromeModalKind;

impl Lumen {
    /// BUG-934: the CC-5 chrome hit-test + `data-action` dispatch (originally
    /// inline in `on_mouse_input`'s `Pressed` branch), shared with automation
    /// clicks (`Lumen::handle_automation_click`) so `AutomationCommand::Click`/
    /// `::Type` reach engine-drawn chrome (toolbar, tab strip, sidebar, …)
    /// exactly like a real mouse click does, instead of always falling through
    /// to page hit-testing.
    ///
    /// Safe to share as-is: this whole branch dispatches the chrome action
    /// synchronously on press — unlike page clicks, it has no matching
    /// `ElementState::Released` bookkeeping to miss (see BUG-934's "why not a
    /// quick point-fix" for why the *page* click path can't be shared this
    /// way).
    ///
    /// Returns `true` when `(x_css, y_css)` was over chrome (handled here,
    /// however the hit-test itself resolved) — `false` means the caller
    /// should fall through to page hit-testing.
    pub(crate) fn try_dispatch_chrome_click(
        &mut self,
        x_css: f32,
        y_css: f32,
        event_loop: &crate::browser_thread::MainHandle<'_>,
    ) -> bool {
        if !self.point_over_chrome(x_css, y_css) {
            return false;
        }
        let hit = self.chrome_hit_test(x_css, y_css);
        self.chrome_active_nid = hit.as_ref().map(|r| r.node);
        self.relayout_chrome_host();
        if let Some(hit) = hit {
            // CC-7: `.omnibox`/`#omniInput` carries no `data-action` (nothing
            // to translate an `onfocus` handler from — the frozen design
            // reference has none either, see CC-7 in
            // docs/tasks/p1-css-chrome.md) — special-cased here exactly like
            // the legacy `toolbar::ToolbarHit::Omnibox` branch it mirrors: a
            // no-op while already open so an in-progress edit/dropdown
            // selection isn't reset.
            let omni_input = self
                .chrome_doc
                .as_ref()
                .and_then(|(doc, _)| doc.find_by_id(lumen_chrome::ids::OMNI_INPUT));
            if omni_input.is_some_and(|id| hit.path.contains(&id)) {
                if !self.address_bar.is_open() {
                    self.hint.close();
                    let current = self.current_display_url().to_owned();
                    self.address_bar.open(&current);
                    // CC-7: the relayout above ran before `open()` — redo it
                    // so the `:focus-within` ring/caret show on this same
                    // click, not one input later (see the matching comment
                    // in `Self::handle_address_bar_key`).
                    self.relayout_chrome_host();
                } else {
                    // UX-OMNI-EDIT: click in the already-focused field puts
                    // the caret on the glyph under the pointer. (The first
                    // click opens with the whole address selected; this
                    // dispatch runs on press, so a drag past SM_CXDRAG is
                    // not told apart from a click — no drag-select yet.)
                    self.omnibox_place_caret_at(x_css);
                    self.relayout_chrome_host();
                }
            } else if let Some((nid, action)) = self.chrome_action_at(&hit) {
                self.dispatch_chrome_action(nid, action, event_loop);
            }
        }
        self.request_redraw();
        true
    }

    pub(crate) fn dispatch_chrome_action(
        &mut self,
        nid: NodeId,
        action: lumen_chrome::ChromeAction,
        event_loop: &crate::browser_thread::MainHandle<'_>,
    ) {
        use lumen_chrome::ChromeAction;
        match action {
            ChromeAction::Reload if self.nav_start.is_some() => {
                // UX-LOADING: while a navigation is in flight the same
                // button is "Остановить".
                self.stop_loading();
            }
            ChromeAction::Reload => {
                // Mirrors `toolbar::ToolbarHit::Reload` — routed through the
                // UserInteraction task source rather than called directly
                // (HTML §8.1.4).
                let flag = Rc::clone(&self.pending_reload);
                self.runtime.handle().queue_task(
                    runtime::TaskSource::UserInteraction,
                    move || { flag.set(true); },
                );
            }
            ChromeAction::SecurityProceed => {
                self.proceed_cert_interstitial();
                self.request_redraw();
            }
            ChromeAction::SecurityBack => {
                self.cert_interstitial.close();
                self.relayout_chrome_host();
                self.request_redraw();
            }
            ChromeAction::SecurityOtherBrowser => {
                // Экран остаётся открытым: передача одноразовая, а решение
                // «перейти» или «назад» пользователь принимает отдельно.
                if let Some(reason) = self.cert_interstitial.handoff_reason() {
                    let url = self.cert_interstitial.url.clone();
                    self.open_url_in_other_browser(&url, reason, false);
                }
            }
            ChromeAction::SecurityOtherBrowserAny => {
                // Цепочку без AIA другие браузеры достраивают сами: причина
                // «сайт не работает» ведёт в Edge/Chrome, а не в ГОСТ-браузер.
                if self.cert_interstitial.handoff_reason().is_some() {
                    let url = self.cert_interstitial.url.clone();
                    self.open_url_in_other_browser(&url, crate::external_browser::HandoffReason::Other, false);
                }
            }
            ChromeAction::SecurityTimeSettings => {
                // Экран остаётся открытым: после исправления часов пользователь
                // обновляет страницу сам.
                if self.cert_interstitial.cause.offers_time_settings() {
                    crate::cert_cause::open_time_settings();
                }
            }
            ChromeAction::OpenMediaInstall => {
                if let Some(uri) = self.media_bar.install_uri() {
                    crate::media_bar::open_install_uri(uri);
                } else if let Some(command) = self.media_bar.install_command() {
                    use lumen_core::ext::ClipboardProvider;
                    platform::clipboard::PlatformClipboard.write_text(command);
                }
            }
            ChromeAction::DismissMediaBar => self.media_bar.dismiss(),
            ChromeAction::HangWait => self.script_hang.wait(),
            ChromeAction::HangStop => {
                self.script_hang.stop();
            }
            ChromeAction::HangReload => {
                // Без остановки новая загрузка встала бы за зависшим скриптом.
                self.script_hang.stop();
                let flag = Rc::clone(&self.pending_reload);
                self.runtime.handle().queue_task(
                    runtime::TaskSource::UserInteraction,
                    move || { flag.set(true); },
                );
            }
            ChromeAction::DialogOk => self.answer_page_dialog(true, event_loop),
            ChromeAction::DialogCancel => self.answer_page_dialog(false, event_loop),
            ChromeAction::NewTab => self.open_new_tab_for_typing(),
            ChromeAction::ZoomReset => self.reset_page_zoom(),
            ChromeAction::SaveLogin
            | ChromeAction::NeverSaveLogin
            | ChromeAction::NextLogin
            | ChromeAction::UseGeneratedPassword
            | ChromeAction::DismissLogin => self.dispatch_login_action(action),
            ChromeAction::OpenCertViewer => {
                let cert = self.cert_info.clone();
                self.cert_panel.toggle(cert);
                // CC-10: see the matching comment on `ToggleShieldPopover`.
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleShieldPopover => {
                self.toggle_site_popover();
                // CC-9: `#permPopover`'s `.open` class is baked into
                // `chrome_layout` at `relayout_chrome_host` time, same gap
                // CC-7 found for `#omniInput` — without this the popover
                // wouldn't show until some other trigger relayouts chrome.
                self.relayout_chrome_host();
            }
            // UX-LAYOUT-FIX: «искать исходное» — отказ от переведённого запроса.
            ChromeAction::FindUseOriginal => {
                self.find.set_keep_original();
                self.request_redraw();
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleFind => {
                if self.find.is_open() {
                    self.find.close();
                } else {
                    self.hint.close();
                    self.find.open();
                }
                self.relayout_chrome_host();
            }
            // CC-10b: `#rightSidebar` is a single tabbed panel in the design
            // (`.right-sidebar`/`.content-area` are flex siblings — opening
            // it really does push `#contentArea`, unlike the modal overlays
            // CC-9/CC-10 gated) — mutually exclusive with the AI tab so
            // `chrome_model_snapshot`'s `right_sidebar.tab` stays unambiguous.
            // `relayout_chrome()` keeps the legacy (flag-off) page-reflow
            // behavior; `relayout_chrome_host()` is the CC-7/9/10-class fix
            // this action was missing — `#rightSidebar`'s `.open` class is
            // baked into `chrome_layout` at relayout time, so without it the
            // panel wouldn't show until some other trigger relayouts chrome.
            ChromeAction::OpenWebSidebar => {
                self.ai_panel.visible = false;
                self.sidebar.toggle();
                self.relayout_chrome();
                self.relayout_chrome_host();
            }
            ChromeAction::OpenAiSidebar => {
                self.sidebar.visible = false;
                self.ai_panel.toggle();
                self.relayout_chrome();
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleDownloads => {
                self.downloads.toggle_visible();
                self.relayout_chrome_host();
            }
            ChromeAction::DismissUpdate
            | ChromeAction::DownloadUpdate
            | ChromeAction::RestartToUpdate
            | ChromeAction::CheckForUpdates
            | ChromeAction::ToggleAutoUpdate => self.dispatch_update_action(action, event_loop),
            // BUG-408: shared by `#archiveToggleBtn`, `.nt-restore`, and
            // `#archivePanel`'s own close button (all three carry this same
            // action, mirroring how `toggle-downloads` closes its own panel).
            ChromeAction::ToggleArchive => {
                self.archive.toggle();
                self.relayout_chrome_host();
            }
            // BUG-408: `.arc-restore`/`.arc-dismiss` carry their own copy of
            // `data-archive-id` (mirrors `.tab-close`'s `data-tab-id`), so
            // `nid` is the button itself, not the row.
            ChromeAction::ArchiveRestore => {
                if let Some(id) = self.chrome_data_id(nid, "data-archive-id")
                    && let Some(entry) = self.archive.take(id as usize)
                {
                    if !entry.url.is_empty() {
                        self.navigate_to(PageSource::url(entry.url));
                    }
                    self.archive.close();
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::ArchiveDismiss => {
                if let Some(id) = self.chrome_data_id(nid, "data-archive-id") {
                    self.archive.take(id as usize);
                    self.relayout_chrome_host();
                }
            }
            // UX-TRANSLATE-4: toolbar button, same toggle as the palette entry.
            ChromeAction::TranslatePage => self.toggle_page_translation(),
            ChromeAction::OpenPrintDialog => {
                self.print_panel.toggle();
                // CC-10: see the matching comment on `ToggleShieldPopover`.
                self.relayout_chrome_host();
            }
            // BUG-420: `#printOrientationSelect` has exactly two `<option>`s
            // (Книжная/Альбомная) — a click anywhere on the closed select
            // just flips between them, mirroring `#printOverlay`'s lack of a
            // real dropdown-popover mechanism in the chrome host.
            ChromeAction::CyclePrintOrientation => {
                self.print_panel.orientation = match self.print_panel.orientation {
                    panels::print_panel::Orientation::Portrait => panels::print_panel::Orientation::Landscape,
                    panels::print_panel::Orientation::Landscape => panels::print_panel::Orientation::Portrait,
                };
                self.relayout_chrome_host();
            }
            ChromeAction::TogglePrintBackgrounds => {
                self.print_panel.print_backgrounds = !self.print_panel.print_backgrounds;
                self.relayout_chrome_host();
            }
            // BUG-420: the "Печать" footer button — was `close-modal` (did
            // nothing but dismiss the overlay). Runs the real PDF export
            // with `PrintPanel`'s current settings, mirroring
            // `handle_print_request`'s JS `window.print()` path.
            ChromeAction::PrintConfirm => {
                self.handle_print_confirm();
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleDevtools => {
                self.devtools_console.toggle();
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleNetwork => {
                self.network_panel.toggle();
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleInspector => {
                self.dom_inspector.toggle();
                self.relayout_chrome_host();
            }
            ChromeAction::InspectorTabElements => {
                self.dom_inspector.switch_tab(devtools::inspector::InspectorTab::Elements);
                self.relayout_chrome_host();
            }
            ChromeAction::InspectorTabStyles => {
                self.dom_inspector.switch_tab(devtools::inspector::InspectorTab::Styles);
                self.relayout_chrome_host();
            }
            ChromeAction::InspectorTabComputed => {
                self.dom_inspector.switch_tab(devtools::inspector::InspectorTab::Computed);
                self.relayout_chrome_host();
            }
            ChromeAction::InspectorTabNetwork => {
                self.dom_inspector.switch_tab(devtools::inspector::InspectorTab::Network);
                self.relayout_chrome_host();
            }
            ChromeAction::ClearNetwork => {
                self.network_panel.clear_log();
                self.relayout_chrome_host();
            }
            ChromeAction::CloseNote => {
                self.note_viewer.close();
                self.relayout_chrome_host();
            }
            ChromeAction::TogglePrivacy => {
                self.privacy.toggle();
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleReadLater => {
                self.read_later_panel.toggle();
                if self.read_later_panel.visible {
                    self.refresh_read_later();
                }
                self.relayout_chrome_host();
            }
            ChromeAction::ReadLaterOpen => {
                if let Some(id) = self.chrome_data_id(nid, "data-read-later-id")
                    && let Ok(Some(entry)) = self.read_later_store.get(id)
                {
                    // Offline HTML snapshot; opening marks the entry as read.
                    let html = String::from_utf8_lossy(&entry.html_snapshot).into_owned();
                    let base_url = entry.url.clone();
                    let _ = self.read_later_store.set_status(id, lumen_knowledge::ReadStatus::Read);
                    self.read_later_panel.visible = false;
                    self.navigate_to(PageSource::Snapshot { html, base_url });
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::ReadLaterDelete => {
                if let Some(id) = self.chrome_data_id(nid, "data-read-later-id") {
                    let _ = self.read_later_store.delete(id);
                    self.refresh_read_later();
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::ClearConsole => {
                self.devtools_console.clear();
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleProfileMenu => {
                self.profile_menu.toggle();
                if self.profile_menu.visible {
                    self.refresh_profile_menu_entries();
                }
                self.relayout_chrome_host();
            }
            // CC-10b: `data-view` picks the target. `#view-page`/`#view-history`/
            // `#view-bookmarks`/`#view-settings` are mutually exclusive
            // (`.view.active`, one at a time — `chrome_model_snapshot`'s
            // `content_view` derives from whichever legacy panel's `visible`
            // flag is set), so opening one closes the other two. Reuses the
            // exact legacy open/refresh calls the `Ctrl+H`/`Ctrl+Shift+O`/
            // `Ctrl+,` keyboard shortcuts already make, so behavior (data
            // load, draft flush on settings close) matches exactly.
            ChromeAction::ShowView => {
                let view = self
                    .chrome_doc
                    .as_ref()
                    .and_then(|(doc, _)| doc.get(nid).get_attr("data-view"))
                    .map(str::to_owned);
                match view.as_deref() {
                    Some("settings") => {
                        if self.settings_panel.visible {
                            self.close_settings_panel();
                        } else {
                            self.history_panel.visible = false;
                            self.bookmark_panel.visible = false;
                            self.open_settings_panel();
                        }
                    }
                    Some("history") => {
                        if !self.history_panel.visible {
                            if self.settings_panel.visible {
                                self.close_settings_panel();
                            }
                            self.bookmark_panel.visible = false;
                        }
                        self.history_panel.toggle();
                        if self.history_panel.visible {
                            self.refresh_history();
                        }
                    }
                    Some("bookmarks") => {
                        if !self.bookmark_panel.visible {
                            if self.settings_panel.visible {
                                self.close_settings_panel();
                            }
                            self.history_panel.visible = false;
                        }
                        self.bookmark_panel.toggle();
                        if self.bookmark_panel.visible {
                            self.refresh_bookmarks();
                        }
                    }
                    _ => {
                        // "page" or unrecognised: back to the active tab's page.
                        if self.settings_panel.visible {
                            self.close_settings_panel();
                        }
                        self.history_panel.visible = false;
                        self.bookmark_panel.visible = false;
                    }
                }
                self.relayout_chrome_host();
            }
            // BUG-422: `#view-history`/`#view-bookmarks` entry actions. Every
            // one of them resolves the clicked node through `data-hist-url`/
            // `data-bm-url`/`data-bm-folder` (stamped by `bind_history`/
            // `bind_bookmarks`, `crates/chrome/src/model.rs`) — the same
            // attribute-carries-the-context shape as `data-tab-id` above.
            //
            // Opening an entry also drops the view back to the page: the four
            // `#contentArea` views are mutually exclusive, so navigating while
            // `#view-history` is active would load the page behind a list that
            // stays on screen. Mirrors `ShowView`'s "page" arm.
            ChromeAction::OpenHistoryEntry => {
                if let Some(url) = self.chrome_data_attr(nid, "data-hist-url").filter(|u| !u.is_empty()) {
                    self.history_panel.visible = false;
                    self.navigate_to(PageSource::url(url));
                    self.relayout_chrome_host();
                }
            }
            // The design's per-row star. `Bookmarks::add` upserts on the URL,
            // so a repeat click is idempotent rather than a duplicate row.
            // Folder `""` = the tree root, matching what `BookmarkPanel`
            // treats as the unfiltered set.
            ChromeAction::BookmarkHistoryEntry => {
                if let Some(url) = self.chrome_data_attr(nid, "data-hist-url").filter(|u| !u.is_empty()) {
                    let title = self
                        .chrome_data_attr(nid, "data-hist-title")
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| url.clone());
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let _ = self.bookmarks.add(&url, &title, "", &[], "", now);
                    if self.bookmark_panel.visible {
                        self.refresh_bookmarks();
                    }
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::CopyHistoryEntry => {
                if let Some(url) = self.chrome_data_attr(nid, "data-hist-url").filter(|u| !u.is_empty()) {
                    use lumen_core::ext::ClipboardProvider;
                    platform::clipboard::PlatformClipboard.write_text(&url);
                }
            }
            ChromeAction::DeleteHistoryEntry => {
                if let Some(url) = self.chrome_data_attr(nid, "data-hist-url").filter(|u| !u.is_empty()) {
                    let _ = self.history_store.delete(&url);
                    self.refresh_history();
                    self.relayout_chrome_host();
                }
            }
            // `.hist-head`'s "Очистить" button. Wipes the store, not just the
            // panel's cached rows — `refresh_history` then re-reads it.
            ChromeAction::ClearHistory => {
                let _ = self.history_store.clear();
                let _ = self.history_fts.clear();
                // PERF-15: the site memory is keyed by visited page URLs —
                // history in all but name.
                crate::site_memory::forget_all();
                // COMPAT-SIG-01: журнал сигналов хранит те же сайты.
                self.forget_all_signals();
                self.refresh_history();
                self.relayout_chrome_host();
            }
            ChromeAction::DeleteSavedLogin => {
                let origin = self.chrome_data_attr(nid, "data-login-origin").filter(|o| !o.is_empty());
                let user = self.chrome_data_attr(nid, "data-login-user");
                if let (Some(origin), Some(user), Some(store)) = (origin, user, password_store::global()) {
                    let _ = store.delete(&origin, &user);
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::DeleteAutofillEntry => {
                let origin = self.chrome_data_attr(nid, "data-af-origin").filter(|o| !o.is_empty());
                let field = self.chrome_data_attr(nid, "data-af-field").filter(|f| !f.is_empty());
                let value = self.chrome_data_attr(nid, "data-af-value");
                if let (Some(origin), Some(field), Some(value), Some(store)) =
                    (origin, field, value, autofill_store::global())
                {
                    let _ = store.delete(&origin, &field, &value);
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::RevokeSitePermission => {
                let origin = self.chrome_data_attr(nid, "data-perm-origin").filter(|o| !o.is_empty());
                let kind = self
                    .chrome_data_attr(nid, "data-perm-kind")
                    .and_then(|k| panels::permission_panel::PermissionKind::ALL.into_iter().find(|p| p.label() == k));
                if let (Some(origin), Some(kind)) = (origin, kind) {
                    self.permission.revoke(&origin, kind);
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::ToggleSpellcheck => {
                if let Some(dicts) = crate::assets::SPELL_DICTS.get() {
                    let enabled = dicts.prefs().enabled;
                    dicts.set_enabled(!enabled, &spellcheck::prefs_path());
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::ToggleSpellLanguage => {
                if let (Some(dicts), Some(lang)) =
                    (crate::assets::SPELL_DICTS.get(), self.chrome_data_attr(nid, "data-spell-lang"))
                {
                    let on = dicts.languages().into_iter().find(|(s, _)| *s == lang).is_some_and(|(_, on)| on);
                    dicts.set_language(&lang, !on, &spellcheck::prefs_path());
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::SetTranslateLanguage => {
                if let Some(lang) = self.chrome_data_attr(nid, "data-lang") {
                    let _ = self.settings_store.set_translate_language(&lang);
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::EditTranslateModel => {
                self.settings_panel.translate_model_draft = self.settings_store.translate_model();
                self.settings_panel.focused_input = Some(panels::settings_panel::SettingInput::TranslateModel);
                self.relayout_chrome_host();
            }
            ChromeAction::ClearSiteData => {
                let site = self
                    .chrome_data_attr(nid, "data-site")
                    .and_then(|s| lumen_storage::PartitionKey::parse(&s));
                if let Some(site) = site {
                    self.clear_site_data(&site);
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::DeleteSavedCard => {
                let last4 = self.chrome_data_attr(nid, "data-card-last4").filter(|l| !l.is_empty());
                if let (Some(last4), Some(store)) = (last4, password_store::global()) {
                    let _ = store.delete(card_store::ORIGIN, &last4);
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::ClearAutofill => {
                if let Some(store) = autofill_store::global() {
                    let _ = store.clear();
                }
                if let Some(store) = password_store::global() {
                    for card in card_store::saved_cards(store) {
                        let _ = store.delete(card_store::ORIGIN, &card.last4());
                    }
                }
                self.relayout_chrome_host();
            }
            ChromeAction::OpenBookmark => {
                if let Some(url) = self.chrome_data_attr(nid, "data-bm-url").filter(|u| !u.is_empty()) {
                    self.bookmark_panel.visible = false;
                    self.navigate_to(PageSource::url(url));
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::DeleteBookmark => {
                if let Some(url) = self.chrome_data_attr(nid, "data-bm-url").filter(|u| !u.is_empty()) {
                    let _ = self.bookmarks.delete(&url);
                    self.refresh_bookmarks();
                    self.relayout_chrome_host();
                }
            }
            // `data-bm-folder` is always present on a bound `.bm-folder` row —
            // `""` is the "Все закладки" row and means "no filter", so an
            // empty value is meaningful here (unlike the URL actions above).
            ChromeAction::SelectFolder => {
                if let Some(folder) = self.chrome_data_attr(nid, "data-bm-folder") {
                    self.bookmark_panel.selected_folder =
                        if folder.is_empty() { None } else { Some(folder) };
                    self.bookmark_panel.scroll_y = 0.0;
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::SelectTab => {
                if let Some(id) = self.chrome_data_id(nid, "data-tab-id")
                    && let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == id as usize)
                {
                    self.switch_tab(idx);
                }
            }
            // Tree mode (`Ctrl+Shift+B`): the `.tab-arrow` of a parent row
            // folds/unfolds its subtree.
            ChromeAction::ToggleTabSubtree => {
                if let Some(id) = self.chrome_data_id(nid, "data-tab-id") {
                    let id = id as usize;
                    let expanding = self.tree_tabs.collapsed.contains(&id);
                    self.tree_tabs.toggle_collapsed(id);
                    if expanding {
                        // Purge stale collapse entries for tabs closed while
                        // their parent subtree was hidden.
                        let subtree = tabs::tree::subtree_ids(&self.tab_strip.tabs, id);
                        let valid: std::collections::HashSet<usize> =
                            self.tab_strip.tabs.iter().map(|t| t.id).collect();
                        self.tree_tabs.collapsed.retain(|c| valid.contains(c) || !subtree.contains(c));
                    }
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::CloseTab => {
                if let Some(id) = self.chrome_data_id(nid, "data-tab-id")
                    && let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == id as usize)
                {
                    self.freeze_hbar_for_close(id as usize);
                    self.close_tab(idx, event_loop);
                }
            }
            ChromeAction::SelectWorkspace => {
                if let Some(id) = self.chrome_data_id(nid, "data-ws-id") {
                    self.workspace_panel.set_active(Some(id));
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::DeleteWorkspace => {
                // Never delete the last workspace — require at least one.
                if let Some(id) = self.chrome_data_id(nid, "data-ws-id")
                    && self.workspace_panel.workspaces.len() > 1
                {
                    let _ = self.workspaces.delete(id);
                    self.refresh_workspaces();
                    // If the deleted workspace was active, switch to the first.
                    if self.workspace_panel.active_id == Some(id) {
                        let first_id = self.workspace_panel.workspaces.first().map(|w| w.id);
                        self.workspace_panel.set_active(first_id);
                    }
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::AddWorkspace => {
                let idx = self.workspace_panel.workspaces.len();
                let name = format!("Workspace {}", idx + 1);
                let color = panels::workspace_panel::default_color_for_index(idx);
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                if let Ok(id) = self.workspaces.create(&name, color, "", None, now) {
                    self.refresh_workspaces();
                    self.workspace_panel.set_active(Some(id));
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::ToggleSidebar => {
                self.chrome_sidebar_collapsed = !self.chrome_sidebar_collapsed;
                self.relayout_chrome_host();
            }
            // CC-9: `#omniDropdown` rows carry `data-sugg-idx` (stamped by
            // `bind_dropdown`, mirroring `data-tab-id`) — resolve it back to
            // `AddressBarState::suggestions()[idx]` and commit exactly like
            // `AddressBarState::commit()`'s `selected_idx` branch would, just
            // without requiring keyboard navigation to have set that index
            // first.
            ChromeAction::OmniGo => {
                if let Some(idx) =
                    self.chrome_data_id(nid, "data-sugg-idx").and_then(|i| usize::try_from(i).ok())
                {
                    self.address_bar.commit_suggestion(idx);
                    if let Some(value) = self.address_bar.take_commit() {
                        self.handle_omnibox_commit(value);
                    }
                    self.relayout_chrome_host();
                }
            }
            // CC-9: resolves the clicked button's `.perm-row` ancestor back
            // to a `PermissionKind` by position (the asset's two static rows
            // are Camera then Microphone, `PermissionKind::ALL`'s first two —
            // see `Self::chrome_permission_kind_for_node`), then sets it
            // directly per `data-perm` ("allow"/"deny"). Unlike the legacy
            // panel's single cycle button, the design has two distinct
            // buttons with no "ask" control, so this sets state directly
            // rather than cycling.
            ChromeAction::SetPermission => {
                if let Some(kind) = self.chrome_permission_kind_for_node(nid)
                    && let Some(perm) = self
                        .chrome_doc
                        .as_ref()
                        .and_then(|(doc, _)| doc.get(nid).get_attr("data-perm"))
                {
                    let state = match perm {
                        "allow" => Some(panels::permission_panel::PermissionState::Allow),
                        "deny" => Some(panels::permission_panel::PermissionState::Deny),
                        _ => None,
                    };
                    if let Some(state) = state {
                        if kind == panels::permission_panel::PermissionKind::Popups
                            && state == panels::permission_panel::PermissionState::Allow
                        {
                            // The user answered the indicator: it has done its job.
                            let tab_id = self.tab_strip.tabs[self.tab_strip.active].id;
                            self.blocked_popups.remove(&tab_id);
                        }
                        let waited = self.permission.answer(kind, state);
                        if waited {
                            self.settle_page_permission(kind, state);
                        }
                        self.relayout_chrome_host();
                    }
                }
            }
            // CC-10: `#cpOverlay` itself carries this action (scrim click
            // closes the palette, mirroring the legacy modal's own
            // click-outside behavior) — `nid` doesn't need resolving further.
            ChromeAction::ClosePalette => {
                self.command_palette.close();
                self.relayout_chrome_host();
            }
            // CC-10: shared by `#certOverlay` and `#printOverlay` (root
            // scrim, `.modal-close`, and both footer buttons all carry this
            // same action) — which one to close is resolved by walking up
            // from `nid` to whichever modal ancestor it's inside.
            ChromeAction::CloseModal => match self.chrome_modal_ancestor(nid) {
                Some(ChromeModalKind::Cert) => {
                    self.cert_panel.close();
                    self.relayout_chrome_host();
                }
                Some(ChromeModalKind::Print) => {
                    self.print_panel.close();
                    self.relayout_chrome_host();
                }
                Some(ChromeModalKind::Shortcuts) => {
                    self.shortcuts_panel.close();
                    self.relayout_chrome_host();
                }
                Some(ChromeModalKind::Access) => self.close_a11y_panel(),
                None => {}
            },
            // CC-10b: `.set-nav .item`/`.set-section` both carry the same
            // slug on `data-section`/`data-set` — `bind_settings` matches
            // `ChromeSettingsModel::active_section` against either
            // attribute, so this only needs to store the clicked slug.
            // UX-CHROME-PANELS: click on a `.sc-row` starts a rebind of that row.
            ChromeAction::RebindShortcut => {
                let idx = self
                    .chrome_doc
                    .as_ref()
                    .and_then(|(doc, _)| doc.get(nid).get_attr("data-sc-index"))
                    .and_then(|v| v.parse::<usize>().ok());
                if let Some(idx) = idx.filter(|&i| i < self.shortcuts_panel.rows.len()) {
                    self.shortcuts_panel.rebinding = Some(idx);
                    self.relayout_chrome_host();
                }
            }
            // UX-CHROME-PANELS: `#accessOverlay` controls edit the draft;
            // it is persisted on close (`close_a11y_panel`).
            ChromeAction::AccessSetFont | ChromeAction::AccessSetCursor => {
                let value = self
                    .chrome_doc
                    .as_ref()
                    .and_then(|(doc, _)| doc.get(nid).get_attr("data-value").map(str::to_owned));
                match (action, value.as_deref()) {
                    (ChromeAction::AccessSetFont, Some(v)) => {
                        if let Ok(m) = v.parse::<f64>() {
                            self.a11y_panel.draft.font_size_multiplier = m;
                        }
                    }
                    (_, Some(v)) => {
                        use lumen_storage::CursorSize;
                        self.a11y_panel.draft.cursor_size = match v {
                            "large" => CursorSize::Large,
                            "extra" => CursorSize::ExtraLarge,
                            _ => CursorSize::Normal,
                        };
                    }
                    _ => {}
                }
                self.relayout_chrome_host();
            }
            ChromeAction::AccessToggleMotion => {
                self.a11y_panel.draft.reduced_motion = !self.a11y_panel.draft.reduced_motion;
                self.relayout_chrome_host();
            }
            ChromeAction::AccessToggleColors => {
                self.a11y_panel.draft.forced_colors = !self.a11y_panel.draft.forced_colors;
                self.relayout_chrome_host();
            }
            ChromeAction::SetSettingsSection => {
                if let Some(section) =
                    self.chrome_doc.as_ref().and_then(|(doc, _)| doc.get(nid).get_attr("data-section"))
                {
                    self.chrome_settings_section = section.to_owned();
                    self.relayout_chrome_host();
                }
            }
            // CC-10b: switches the active tab without closing the panel
            // (unlike `OpenAiSidebar`/`OpenWebSidebar`, which toggle
            // open/closed) — mirrors clicking a tab in an already-open
            // `#rightSidebar`.
            ChromeAction::SetSidebarTab => {
                if let Some(tab) =
                    self.chrome_doc.as_ref().and_then(|(doc, _)| doc.get(nid).get_attr("data-rs-tab"))
                {
                    match tab {
                        "ai" => {
                            self.ai_panel.visible = true;
                            self.sidebar.visible = false;
                        }
                        "web" => {
                            self.sidebar.visible = true;
                            self.ai_panel.visible = false;
                        }
                        _ => {}
                    }
                    self.relayout_chrome();
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::CloseRightSidebar => {
                self.ai_panel.visible = false;
                self.sidebar.visible = false;
                self.relayout_chrome();
                self.relayout_chrome_host();
            }
            // BUG-421: of `#view-settings`'s six `.toggle`s, only these two
            // (Privacy → "Adblock & Fingerprinting") have a clean 1:1 backing
            // field on `SettingsPanel::draft` — they got their own
            // `data-action` in the design reference (`toggleShields`/
            // `toggleFingerprintMode`) instead of the shared `toggle-switch`,
            // so `nid` here is already the specific toggle and needs no
            // structural resolver. Persisted on close via
            // `close_settings_panel` → `settings_store.apply_snapshot`, same
            // as every other `draft` field.
            ChromeAction::ToggleShields => {
                self.settings_panel.toggle_shields();
                // BUG-411: the setting is the fallback every host without a
                // per-site exception uses, so a flip here has to reach the
                // live filter too — not just the draft that `close_settings_panel`
                // will persist.
                self.shields.set_default_enabled(self.settings_panel.draft.shields_enabled);
                self.sync_adblock_filter();
                self.relayout_chrome_host();
            }
            // BUG-411: the per-site switch restored into `#permPopover`. Unlike
            // `ToggleShields` above (the global setting) this keys off the
            // current host, and unlike the legacy panel's switch — which only
            // ever painted itself — it drives the real process-global filter.
            ChromeAction::ToggleSiteShields => {
                if !self.shields.toggle_current_site() {
                    self.note_site_protection_off();
                }
                self.sync_adblock_filter();
                self.relayout_chrome_host();
            }
            // COMPAT-PACK-IND: кнопки строки «К сайту применено N исправлений».
            ChromeAction::CompatDetails => {
                self.shields.visible = false;
                self.permission.visible = false;
                self.show_compat_page();
            }
            // COMPAT-PANEL: ступени секции «Сайт работает не так?».
            ChromeAction::CompatNocache => self.fix_no_cache(),
            ChromeAction::CompatReset => self.fix_reset_ask(),
            ChromeAction::CompatResetConfirm => self.fix_reset_confirm(),
            ChromeAction::CompatResetCancel => self.fix_reset_cancel(),
            ChromeAction::CompatHandoff => self.fix_handoff(),
            ChromeAction::CompatReport => self.open_report_form(),
            ChromeAction::CompatChipOpen => self.help_chip_open(),
            ChromeAction::CompatChipDismiss => self.help_chip_dismiss(),
            ChromeAction::CompatWorkedYes => self.fix_answer(true),
            ChromeAction::CompatWorkedNo => self.fix_answer(false),
            ChromeAction::CompatDisableReload => {
                self.shields.visible = false;
                self.permission.visible = false;
                self.compat_disable_applied_and_reload();
            }
            ChromeAction::ToggleFingerprintMode => {
                self.settings_panel.toggle_fingerprint_mode();
                self.relayout_chrome_host();
            }
            // CC-18: `#demoBar`'s own profile buttons (`.demo-group
            // [data-profile]`) — a second, `chrome_doc`-reachable site for
            // the exact same switch `WindowEvent::MouseInput`'s
            // `ProfileMenuHit::SwitchTo` already performs for the legacy
            // `#profileMenu` overlay (CC-15-1 kept that one a legacy overlay
            // deliberately; this button lives inside the floating panel,
            // which CC-18 wires through `chrome_doc` instead). Slug → id
            // goes through `DEFAULT_PROFILES`' name, same lookup
            // `ChromeModel::profile_slug` does in reverse just above.
            ChromeAction::SetProfile => {
                if let Some(slug) = self
                    .chrome_doc
                    .as_ref()
                    .and_then(|(doc, _)| doc.get(nid).get_attr("data-profile"))
                    && let Some((name, ..)) =
                        panels::profile_menu::DEFAULT_PROFILES.iter().find(|(_, s, _)| *s == slug)
                    && let Some(id) =
                        self.profile_menu.entries.iter().find(|e| e.name == *name).map(|e| e.id)
                    && self.profiles.set_active(Some(id)).is_ok()
                {
                    self.profile_menu.set_active(Some(id));
                    self.profile_menu.visible = false;
                    // DS-16: Anonymous is ephemeral — mirrors the legacy
                    // overlay's own reset so switching via either site never
                    // carries a cookie over from a previous Anonymous run.
                    if self.active_profile_is_anonymous() {
                        self.reset_anonymous_cookie_jar();
                    }
                    self.sync_permission_store();
                    self.sync_profile_locale();
                    self.relayout_chrome_host();
                }
            }
            // CC-18: `#demoSwitch` — the floating panel's own 7-shape
            // switcher (`body[data-demo]` picks the CSS form,
            // `bind_control_panel` syncs `.active`).
            ChromeAction::SetDemoVariant => {
                if let Some(shape) = self
                    .chrome_doc
                    .as_ref()
                    .and_then(|(doc, _)| doc.get(nid).get_attr("data-demo-variant"))
                    .and_then(lumen_chrome::ControlPanelShape::from_attr_value)
                {
                    self.control_panel_shape = shape;
                    // Reference JS (`setDemoVariant`): switching to any shape
                    // other than "mini" also collapses the pill back down.
                    if shape != lumen_chrome::ControlPanelShape::Mini {
                        self.control_panel_mini_open = false;
                    }
                    // CC-18 срез 3: a drag offset is a delta from the *old*
                    // shape's CSS anchor — see `floating_panel_reset`.
                    self.floating_panel_reset(lumen_chrome::ids::DEMO_BAR);
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::ToggleDemoMini => {
                self.control_panel_mini_open = !self.control_panel_mini_open;
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleDemoInfo => {
                self.control_panel_info_open = !self.control_panel_info_open;
                self.relayout_chrome_host();
            }
            // CC-18: `.demo-group [data-layout]` — real backing state
            // (`self.vertical_tabs.visible`, already mirrored one-way into
            // `ChromeModel::layout_vertical`) that BUG-421 found had no
            // working *setter* anywhere in the UI (the shared `ToggleSwitch`
            // action below can't tell which of six toggles fired). Unlike
            // that shared toggle, `setLayout` carries the target value
            // directly, so no discriminator is needed.
            ChromeAction::SetLayout => {
                if let Some(layout) = self
                    .chrome_doc
                    .as_ref()
                    .and_then(|(doc, _)| doc.get(nid).get_attr("data-layout"))
                {
                    self.vertical_tabs.visible = layout == "vertical";
                    self.relayout_chrome_host();
                }
            }
            // CC-18: the "Фокус" button, the pill's exit button and
            // `KeyCommand::ToggleFocusMode` all flip `self.focus.active`;
            // the `.focus-timer` pill follows it via `bind_focus`.
            ChromeAction::ToggleFocusTimer => {
                self.focus.timer.toggle_pause();
                if self.focus.timer.running {
                    let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
                    self.focus.tick(now_ms);
                }
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleFocus => {
                self.focus.toggle(panels::focus_panel::DEFAULT_POMODORO_MIN);
                if self.focus.active {
                    let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
                    self.focus.tick(now_ms);
                }
                self.relayout_chrome_host();
            }
            // CC-18 срез 4: `#themeBtn` — explicit dark/light lock, the same
            // mechanism `close_settings_panel`'s Appearance-section flush
            // already uses (`ShellTheme::is_dark` ignores its `os_dark`
            // argument once `base` is `Light`/`Dark`, so a stale
            // `self.dark_mode` passed in here never leaks the OS value back
            // in). Persisted immediately via `settings_store.set_theme`
            // (not only on settings-panel close) since this is a one-click
            // toggle with no draft/apply step of its own — the Appearance
            // section's own theme picker (still unwired, BUG-421) and this
            // button now agree on the same backing store.
            ChromeAction::ToggleTheme => {
                self.shell_theme.base = if self.dark_mode {
                    panels::themes::ThemeBase::Light
                } else {
                    panels::themes::ThemeBase::Dark
                };
                if let Err(e) = self.settings_store.set_theme(&self.shell_theme.to_settings_str()) {
                    eprintln!("chrome: не удалось сохранить тему: {e}");
                }
                let new_dark = self.shell_theme.is_dark(self.dark_mode);
                if new_dark != self.dark_mode {
                    self.dark_mode = new_dark;
                    // ADR-016 M2.2b-4: same async-safe whole-page restyle as
                    // the OS theme flip / settings-panel lock.
                    self.relayout_chrome();
                }
                self.relayout_chrome_host();
            }
            // BUG-426 reinvestigation (2026-08-01): these were filed
            // together as "sit in one empty branch" but each is a no-op for
            // its own, unrelated reason — none is a small wiring gap like
            // BUG-419/420/421 turned out to be. `SetProfile`/`ToggleFocus`
            // used to be here too; CC-18 gave both a second,
            // `chrome_doc`-reachable call site (the floating panel) and
            // moved them out above.
            //
            // `ArchiveCard`: the two `.bm-card.readlater` demo cards
            // (`data-action="archive-card"`) live inside `#view-bookmarks`'s
            // `.bm-grid`, whose *entire* card list `bind_bookmarks`
            // (`crates/chrome/src/model.rs`) deletes and rebuilds from
            // `ChromeBookmarksModel::cards` on every relayout —
            // `remove_children_with_class(doc, grid, "bm-card")` matches by
            // class token, so it removes `.bm-card.readlater` too, and the
            // rebuilt cards never carry a `readlater`/`archive-card` variant
            // (no such concept in `ChromeBookmarkCardModel`). The action-
            // carrying markup is wiped before the first paint; this branch
            // is provably dead code, not merely low priority.
            //
            // `ToggleSwitch`: of `#view-settings`'s six `.toggle`s, the two
            // with a clean 1:1 backing field got their own `data-action` in
            // [BUG-421](../../../bugs/BUG-421-FIXED.md) (`ToggleShields`/
            // `ToggleFingerprintMode`, handled above). The remaining four
            // ("Принудительный HTTPS", the two Extensions rows, and the QA
            // "Стабильные test-id" row) have no matching real-state field at
            // all — no force-HTTPS setting, no extensions/QA-flag store —
            // so a click still can't resolve to anything.
            //
            // `ToggleQaPanel` (CC-18, `#demoBar`'s "QA-панель" button): the
            // QA/tester panel it targets is `strip_qa_panel_html`-excluded
            // from the product build entirely (same exclusion that already
            // made the identical `toggleQa()` call dead in the
            // `showView('page');toggleQa()` compound handler elsewhere) —
            // permanently nothing to switch to, not a remainder.
            //
            // `DragPanel` (CC-18 срез 3, `#demoHeader`): not a click action at
            // all — it is consumed by `floating_panel_press` at *press* time
            // (drag start / double-click reset) and never reaches this
            // dispatch table, which the press path only calls for the panel's
            // ordinary controls. The arm exists because the enum is generated
            // from the asset's `data-action` values and the match is
            // exhaustive.
            ChromeAction::DragPanel
            | ChromeAction::ArchiveCard
            | ChromeAction::ToggleSwitch
            | ChromeAction::ToggleQaPanel => {}
        }
    }
}
