//! Keyboard handling for the panels that take focus while they are open:
//! bookmarks, history, the AI panel, settings and the shortcuts sheet.
//!
//! Each returns `bool` - "the panel consumed this key" - because `handle_key`
//! offers a key to whichever panel is open before the page ever sees it. The
//! panel widgets are `crate::panels`; the reason the handlers are `Lumen`
//! methods is that acting on a key almost always means doing something to the
//! browser (navigate to a history row, apply a setting, close the panel).

use crate::*;

impl Lumen {
    /// Handle a key while the bookmark panel search box is focused.
    ///
    /// Returns `true` when the key was consumed. Modified keys (Ctrl/Cmd) are
    /// *not* consumed so global shortcuts continue to work.
    pub(crate) fn handle_bookmark_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if self.modifiers.control_key() || self.modifiers.super_key() {
            return false;
        }
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.bookmark_panel.search_active = false;
                self.request_redraw();
                true
            }
            KeyCode::Backspace => {
                self.bookmark_panel.backspace_search();
                self.request_redraw();
                true
            }
            _ => {
                if let Some(text) = key_event.text.as_ref()
                    && !text.is_empty()
                    && !text.chars().any(char::is_control)
                {
                    self.bookmark_panel.append_search(text);
                    self.request_redraw();
                    return true;
                }
                false
            }
        }
    }

    /// Handle keyboard input when the history panel is visible.
    ///
    /// When `search_active`: printable chars → search query, Backspace → delete
    /// char, Escape → blur search (panel stays open). Arrow keys scroll the list.
    /// Returns `true` if the key was consumed.
    pub(crate) fn handle_history_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if self.modifiers.control_key() || self.modifiers.super_key() {
            return false;
        }
        match code {
            KeyCode::Escape if !key_event.repeat => {
                if self.history_panel.search_active {
                    self.history_panel.search_active = false;
                } else {
                    self.history_panel.visible = false;
                }
                self.request_redraw();
                true
            }
            KeyCode::Backspace if self.history_panel.search_active => {
                self.history_panel.backspace_search();
                self.refresh_history();
                self.request_redraw();
                true
            }
            KeyCode::ArrowDown => {
                self.history_panel.scroll_by(LINE_STEP_CSS_PX);
                self.request_redraw();
                true
            }
            KeyCode::ArrowUp => {
                self.history_panel.scroll_by(-LINE_STEP_CSS_PX);
                self.request_redraw();
                true
            }
            _ => {
                if self.history_panel.search_active
                    && let Some(text) = key_event.text.as_ref()
                        && !text.is_empty()
                        && !text.chars().any(char::is_control)
                    {
                        for ch in text.chars() {
                            self.history_panel.append_search(ch);
                        }
                        self.refresh_history();
                        self.request_redraw();
                        return true;
                    }
                false
            }
        }
    }

    /// Handle keyboard input when the print dialog is visible (E-1).
    ///
    /// Printable chars go to the focused text field. Escape closes the dialog.
    /// Returns `true` if the key was consumed.
    /// Handle keyboard input while the AI panel is visible.
    ///
    /// Returns `true` if the event was consumed (swallowed from the global
    /// keybinding table).  Modified keys (Ctrl, Meta) fall through so that
    /// `Ctrl+Shift+A` (toggle AI panel) still works.
    pub(crate) fn handle_ai_panel_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if self.modifiers.control_key() || self.modifiers.super_key() {
            return false;
        }
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.ai_panel.close();
                // ADR-016 M2.2b-3: closing the AI panel is an async-safe chrome
                // toggle (content viewport widens, no synchronous geometry read),
                // so route off-thread when the engine thread is enabled.
                self.relayout_chrome();
                self.request_redraw();
                true
            }
            KeyCode::Backspace => {
                self.ai_panel.backspace();
                self.request_redraw();
                true
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                // Split borrows: inline the submit logic to let Rust prove
                // ai_panel and ai_backend are disjoint fields.
                let prompt = self.ai_panel.input.clone();
                if !prompt.trim().is_empty() {
                    let response = self.ai_backend.query(&prompt);
                    self.ai_panel.response = response;
                    self.ai_panel.input.clear();
                    self.ai_panel.scroll_y = 0.0;
                }
                self.request_redraw();
                true
            }
            _ => {
                if let Some(text) = key_event.text.as_ref()
                    && !text.is_empty()
                    && !text.chars().any(char::is_control)
                {
                    for ch in text.chars() {
                        self.ai_panel.push_char(ch);
                    }
                    self.request_redraw();
                    return true;
                }
                false
            }
        }
    }

    /// Open the settings panel, populating every section — including the ones
    /// backed by stores other than `settings_store` (HTTP/3 from the process-
    /// global fingerprint profile, Tor status from the same, ad-block
    /// subscriptions from `AdblockStore`, spellcheck locale from `SPELL_DICTS`).
    pub(crate) fn open_settings_panel(&mut self) {
        let snap = self.settings_store.snapshot();
        self.settings_panel.open(snap);
        self.settings_panel.set_http3(config::global().http3);
        self.settings_panel.set_tor_active(
            config::global().http_profile == lumen_network::HttpProfile::TorBrowser,
        );
        self.settings_panel
            .set_adblock_subs(self.adblock_store.list_subscriptions().unwrap_or_default());
        self.settings_panel
            .set_spell_locale(SPELL_DICTS.get().map(|d| d.locale().to_owned()));
    }

    /// Close the settings panel, flushing the draft to every backing store.
    ///
    /// Centralised so all four close paths (× button, click outside, `Ctrl+,`
    /// toggle, `Escape`) apply theme/dark-mode sync and the HTTP/3 rewrite
    /// identically — previously only the × button synced `dark_mode`.
    pub(crate) fn close_settings_panel(&mut self) {
        let draft = self.settings_panel.apply_draft();
        // Apply theme & accent from draft when panel closes.
        self.shell_theme = panels::themes::ShellTheme::parse(&draft.theme);
        // Mirror explicit dark/light lock to dark_mode so that
        // @media prefers-color-scheme reflects the user choice. For System
        // theme, is_dark(self.dark_mode) = self.dark_mode (no change); for
        // Dark/Light it overrides.
        let new_dark = self.shell_theme.is_dark(self.dark_mode);
        if new_dark != self.dark_mode {
            self.dark_mode = new_dark;
            // ADR-016 M2.2b-4: an explicit dark/light lock is async-safe like
            // the OS theme flip — a whole-page restyle with no synchronous
            // geometry read here (only chrome state follows), so route it
            // off-thread.
            self.relayout_chrome();
        }
        // Live-sync the tab-strip layout so the Appearance section's toggle
        // takes effect immediately rather than only after the next restart.
        self.vertical_tabs.visible =
            tabs::strip::TabLayout::from_str(&draft.tab_layout) == tabs::strip::TabLayout::Vertical;
        let _ = self.settings_store.apply_snapshot(&draft);
        // BUG-411: `Escape`/click-outside close paths never went through
        // `ChromeAction::ToggleShields`, so re-apply the draft's shields flag
        // as the fallback here too before pushing it at the live filter.
        self.shields.set_default_enabled(draft.shields_enabled);
        self.sync_adblock_filter();
        // HTTP/3 lives in fingerprint.toml, loaded once into a process-global
        // at startup — only rewrite the file (and note the restart) if the
        // draft actually changed it.
        if self.settings_panel.http3_draft != config::global().http3 {
            match config::set_http3(self.settings_panel.http3_draft) {
                Ok(()) => eprintln!(
                    "settings: HTTP/3 изменён на {} — вступит в силу после перезапуска браузера",
                    self.settings_panel.http3_draft
                ),
                Err(e) => eprintln!("settings: не удалось записать fingerprint.toml: {e}"),
            }
        }
        self.settings_panel.visible = false;
        // CC-6: re-sync the CSS chrome's data-theme/data-layout (no-op off the flag).
        self.relayout_chrome_host();
    }

    /// Handle keyboard input when the settings panel is visible.
    ///
    /// Printable chars go to the focused text input. Escape closes panel (flushing
    /// draft). Returns `true` if the key was consumed.
    pub(crate) fn handle_settings_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if self.modifiers.control_key() || self.modifiers.super_key() {
            return false;
        }
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.close_settings_panel();
                self.request_redraw();
                true
            }
            KeyCode::Backspace if self.settings_panel.focused_input.is_some() => {
                self.settings_panel.backspace();
                self.request_redraw();
                true
            }
            _ => {
                if self.settings_panel.focused_input.is_some()
                    && let Some(text) = key_event.text.as_ref()
                        && !text.is_empty()
                        && !text.chars().any(char::is_control)
                    {
                        for ch in text.chars() {
                            self.settings_panel.append_char(ch);
                        }
                        self.request_redraw();
                        return true;
                    }
                false
            }
        }
    }

    /// Обрабатывает клавишный ввод для панели горячих клавиш (§D-4).
    ///
    /// Когда активен rebind mode (`rebinding.is_some()`): захватывает
    /// следующую клавишу и передаёт в `accept_rebind`. Esc отменяет rebind.
    /// Возвращает `true`, если событие поглощено.
    pub(crate) fn handle_shortcuts_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if key_event.repeat {
            return false;
        }
        if self.shortcuts_panel.rebinding.is_some() {
            if code == KeyCode::Escape {
                self.shortcuts_panel.cancel_rebind();
                self.request_redraw();
                return true;
            }
            let modifier = {
                let m = self.modifiers;
                let ctrl = m.control_key();
                let shift = m.shift_key();
                let alt = m.alt_key();
                match (ctrl, shift, alt) {
                    (true, true, false) => "ctrl+shift",
                    (true, false, true) => "ctrl+alt",
                    (true, false, false) => "ctrl",
                    (false, true, false) => "shift",
                    (false, false, true) => "alt",
                    _ => "",
                }
            };
            let key = format!("{:?}", code);
            let key = key.trim_start_matches("Key").trim_start_matches("Digit").to_string();
            self.shortcuts_panel.accept_rebind(modifier, &key);
            self.request_redraw();
            return true;
        }
        if code == KeyCode::Escape {
            self.shortcuts_panel.close();
            self.request_redraw();
            return true;
        }
        false
    }
}
