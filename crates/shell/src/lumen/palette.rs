//! The command palette: assembling the item list from what the browser can do
//! right now, filtering it as the user types, and executing the chosen entry.
//!
//! The widget and its layout are `crate::panels::command_palette`; the reason
//! the list is built here is that almost every item is a `Lumen` method call,
//! so both halves - what to offer and what to run - need the same state.

use crate::*;

impl Lumen {
    /// Rebuild the command-palette item list: curated commands, every bookmark,
    /// and вЂ” when the query is non-empty вЂ” matching history pages (FTS).
    ///
    /// History depends on the query (the FTS index has no "list all"), so this
    /// is called both on open and on every query edit. Commands and bookmarks
    /// are query-independent; the palette's own fuzzy filter ranks the union.
    pub(crate) fn refresh_palette_items(&mut self) {
        use panels::command_palette::{PaletteAction, PaletteItem};

        let mut items: Vec<PaletteItem> =
            PaletteAction::all().iter().copied().map(PaletteItem::command).collect();

        // Bookmarks (query-independent вЂ” fuzzy-filtered in the palette).
        for b in self.bookmarks.list_all().unwrap_or_default() {
            items.push(PaletteItem::bookmark(b.title, b.url));
        }

        // History: FTS needs a query, so only add hits once the user types.
        let query = self.command_palette.query.trim().to_owned();
        if !query.is_empty()
            && let Ok(hits) = self.history_fts.search(&query, 12)
        {
            for hit in hits {
                items.push(PaletteItem::history(hit.title, hit.url));
            }
        }

        self.command_palette.set_items(items);
    }

    /// Handle a key while the command palette modal is open.
    ///
    /// Always returns `true` (the modal swallows every key). `Esc` closes,
    /// `Enter` activates the selected item, `в†‘/в†“` move the selection,
    /// `Backspace` edits the query, and printable characters extend it. Editing
    /// the query refreshes history results.
    pub(crate) fn handle_palette_key(
        &mut self,
        code: KeyCode,
        key_event: &KeyEvent,
        event_loop: &ActiveEventLoop,
    ) -> bool {
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.command_palette.close();
                self.request_redraw();
            }
            KeyCode::ArrowDown if !key_event.repeat => {
                self.command_palette.select_next();
                self.request_redraw();
            }
            KeyCode::ArrowUp if !key_event.repeat => {
                self.command_palette.select_prev();
                self.request_redraw();
            }
            KeyCode::Enter if !key_event.repeat => {
                if let Some(item) = self.command_palette.selected_item().cloned() {
                    self.command_palette.close();
                    self.activate_palette(&item, event_loop);
                }
                self.request_redraw();
            }
            KeyCode::Backspace => {
                self.command_palette.backspace();
                self.refresh_palette_items();
                self.request_redraw();
            }
            _ => {
                // Ignore modified keys other than the toggle (handled globally).
                if self.modifiers.control_key() || self.modifiers.super_key() {
                    return false;
                }
                if let Some(text) = key_event.text.as_ref()
                    && !text.is_empty()
                    && !text.chars().any(char::is_control)
                {
                    self.command_palette.append(text);
                    self.refresh_palette_items();
                    self.request_redraw();
                }
            }
        }
        true
    }

    /// Execute the action behind a selected palette item: run the command, or
    /// navigate to the bookmark / history URL.
    pub(crate) fn activate_palette(
        &mut self,
        item: &panels::command_palette::PaletteItem,
        event_loop: &ActiveEventLoop,
    ) {
        use panels::command_palette::{PaletteAction, PaletteKind};
        match &item.kind {
            PaletteKind::Bookmark | PaletteKind::History => {
                if !item.url.is_empty() {
                    self.navigate_to(PageSource::from_arg(Some(&item.url)));
                }
            }
            PaletteKind::Command(action) => match action {
                PaletteAction::NewTab => self.open_new_tab(),
                PaletteAction::CloseTab => {
                    let idx = self.tab_strip.active;
                    self.close_tab(idx, event_loop);
                }
                PaletteAction::Reload => self.reload(),
                PaletteAction::NavigateBack => self.navigate_back(),
                PaletteAction::NavigateForward => self.navigate_forward(),
                PaletteAction::FindOnPage => {
                    self.hint.close();
                    self.find.open();
                }
                PaletteAction::OpenAddressBar => {
                    self.hint.close();
                    let current = self.current_display_url().to_owned();
                    self.address_bar.open(&current);
                    // CC-7: see the comment on the matching call in
                    // `Self::handle_address_bar_key`.
                    self.relayout_chrome_host();
                }
                PaletteAction::ToggleBookmarks => {
                    self.bookmark_panel.toggle();
                    if self.bookmark_panel.visible {
                        self.refresh_bookmarks();
                    }
                }
                PaletteAction::BookmarkCurrentPage => self.bookmark_current_page(),
                PaletteAction::ToggleVerticalTabs => {
                    self.vertical_tabs.toggle();
                    self.persist_tab_layout();
                    // ADR-016 M2.2b: async-safe chrome-inset relayout.
                    self.relayout_chrome();
                }
                PaletteAction::ToggleDevConsole => self.devtools_console.toggle(),
                PaletteAction::ToggleShields => self.shields.toggle(),
                PaletteAction::ToggleVimMode => {
                    if self.vim_mode.is_some() {
                        self.vim_mode = None;
                    } else {
                        self.vim_mode = Some(input::vim::VimMode::new());
                    }
                }
            },
        }
        self.request_redraw();
    }
}
