//! Keyboard shortcuts settings panel (D-4).
//!
//! State of the `#shortcutsOverlay` modal of the engine chrome, opened by
//! `Ctrl+Shift+/`. Lists all `KeyCommand` variants with their current keybinding.
//! Clicking a row enters rebind mode — the next keypress is recorded
//! as the new binding and persisted via `lumen_storage::KeyboardShortcuts`.
//! Rendering lives in the chrome document (`lumen_chrome::bind_model`); the
//! list is windowed here because the chrome has no wheel-scroll container.

/// Height of one `.sc-row` in `assets/chrome/chrome.html`, CSS px.
const ROW_H: f32 = 32.0;
/// Chrome height of `#shortcutsOverlay`'s modal besides the list: header, footer
/// and the body's vertical padding.
const MODAL_CHROME_H: f32 = 110.0;
/// The modal's `max-height` as a fraction of the viewport (`.modal`).
const MODAL_MAX_FRAC: f32 = 0.82;

// ── Shortcut row data ─────────────────────────────────────────────────────────

/// One entry in the shortcuts list: human label + current binding.
#[derive(Debug, Clone)]
pub struct ShortcutRow {
    /// `KeyCommand` variant name (used as storage key).
    pub command: &'static str,
    /// Human-readable action label shown in the panel.
    pub label: &'static str,
    /// Current modifier string (e.g. `"ctrl"`, `"ctrl+shift"`, `""`).
    pub modifier: String,
    /// Current key name (e.g. `"R"`, `"F5"`, `"Escape"`).
    pub key: String,
}

impl ShortcutRow {
    /// Formatted binding string shown in the key badge (e.g. `"Ctrl+R"`).
    pub fn binding_label(&self) -> String {
        let m = match self.modifier.as_str() {
            "ctrl" => "Ctrl+",
            "ctrl+shift" => "Ctrl+Shift+",
            "ctrl+alt" => "Ctrl+Alt+",
            "alt" => "Alt+",
            "shift" => "Shift+",
            _ => "",
        };
        format!("{}{}", m, self.key)
    }
}

/// Compile-time default bindings for all displayed commands.
///
/// Ordered by category: navigation, tabs, scroll, UI panels, dev tools, zoom.
pub fn default_rows() -> Vec<ShortcutRow> {
    let entries: &[(&str, &str, &str, &str)] = &[
        // command           label                    modifier       key
        ("Reload",           "Перезагрузить",          "ctrl",        "R"),
        ("HistoryBack",      "Назад",                  "alt",         "ArrowLeft"),
        ("HistoryForward",   "Вперёд",                 "alt",         "ArrowRight"),
        ("OpenAddressBar",   "Открыть адресную строку","ctrl",        "L"),
        ("NewTab",           "Новая вкладка",          "ctrl",        "T"),
        ("CloseTab",         "Закрыть вкладку",        "ctrl",        "W"),
        ("NextTab",          "Следующая вкладка",      "ctrl",        "Tab"),
        ("FindOpen",         "Поиск на странице",      "ctrl",        "F"),
        ("ScrollPageDown",   "Прокрутить вниз",        "",            "PageDown"),
        ("ScrollPageUp",     "Прокрутить вверх",       "",            "PageUp"),
        ("ScrollHome",       "В начало",               "",            "Home"),
        ("ScrollEnd",        "В конец",                "",            "End"),
        ("ToggleHistory",    "История",                "ctrl",        "H"),
        ("ToggleSettings",   "Настройки",              "ctrl",        "Comma"),
        ("ToggleBookmarks",  "Закладки",               "ctrl+shift",  "O"),
        ("ToggleReadLater",  "Прочитать позже",        "ctrl+shift",  "R"),
        ("ToggleReaderView", "Режим чтения",           "",            "F9"),
        ("ViewSource",       "Исходный код",           "ctrl",        "U"),
        ("ToggleShortcuts",  "Горячие клавиши",        "ctrl+shift",  "Slash"),
        ("DownloadsPanel",   "Загрузки",               "ctrl",        "J"),
        ("ToggleShields",    "Shields",                "ctrl+shift",  "S"),
        ("ToggleSidebar",    "Боковая панель",         "ctrl+shift",  "A"),
        ("ToggleFocusMode",  "Режим фокуса",           "ctrl+shift",  "F"),
        ("ToggleCommandPalette","Палитра команд",      "ctrl",        "K"),
        ("DevConsole",       "Консоль",                "",            "F12"),
        ("DevInspector",     "Инспектор DOM",          "ctrl+shift",  "I"),
        ("DevNetwork",       "Сеть",                   "ctrl+shift",  "E"),
        ("ZoomIn",           "Увеличить",              "ctrl",        "Equal"),
        ("ZoomOut",          "Уменьшить",              "ctrl",        "Minus"),
        ("ZoomReset",        "Сбросить масштаб",       "ctrl",        "Digit0"),
    ];
    entries
        .iter()
        .map(|(cmd, lbl, modifier, key)| ShortcutRow {
            command: cmd,
            label: lbl,
            modifier: modifier.to_string(),
            key: key.to_string(),
        })
        .collect()
}

// ── Panel state ───────────────────────────────────────────────────────────────

/// Keyboard shortcuts panel UI state.
#[derive(Debug)]
pub struct ShortcutsPanel {
    /// Whether the panel is currently visible.
    pub visible: bool,
    /// Index of the first row shown in the (windowed) list.
    pub first_row: usize,
    /// Index of the row currently awaiting a new keypress, if any.
    pub rebinding: Option<usize>,
    /// All rows with their current (possibly overridden) bindings.
    pub rows: Vec<ShortcutRow>,
}

impl ShortcutsPanel {
    /// Create a new, hidden panel using compile-time default bindings.
    ///
    /// `overrides` — entries loaded from `lumen_storage::KeyboardShortcuts`
    /// at startup; each matching row has its binding replaced.
    pub fn new(overrides: &[lumen_storage::KeyboardShortcutEntry]) -> Self {
        let mut rows = default_rows();
        for ov in overrides {
            if let Some(row) = rows.iter_mut().find(|r| r.command == ov.command) {
                row.modifier = ov.modifier.clone();
                row.key = ov.key.clone();
            }
        }
        Self { visible: false, first_row: 0, rebinding: None, rows }
    }

    /// Show the panel.
    pub fn open(&mut self) {
        self.visible = true;
        self.rebinding = None;
    }

    /// Toggle visibility.
    pub fn toggle(&mut self) {
        if self.visible { self.close(); } else { self.open(); }
    }

    /// Hide the panel and cancel any pending rebind.
    pub fn close(&mut self) {
        self.visible = false;
        self.rebinding = None;
    }

    /// How many rows fit into the modal at viewport height `viewport_h` (CSS px).
    pub fn visible_count(viewport_h: f32) -> usize {
        (((viewport_h * MODAL_MAX_FRAC - MODAL_CHROME_H) / ROW_H) as usize).max(3)
    }

    /// Scroll the window by `delta` rows (clamped to the valid range).
    pub fn scroll_rows(&mut self, delta: i32, viewport_h: f32) {
        let max_first = self.rows.len().saturating_sub(Self::visible_count(viewport_h));
        self.first_row = self.first_row.saturating_add_signed(delta as isize).min(max_first);
    }

    /// Called when a rebind keypress arrives.
    ///
    /// Returns `Some((command, modifier, key))` to be persisted via storage,
    /// or `None` if no rebind was in progress.
    pub fn accept_rebind(
        &mut self,
        modifier: &str,
        key: &str,
    ) -> Option<(String, String, String)> {
        let idx = self.rebinding.take()?;
        if let Some(row) = self.rows.get_mut(idx) {
            row.modifier = modifier.to_owned();
            row.key = key.to_owned();
            Some((row.command.to_owned(), modifier.to_owned(), key.to_owned()))
        } else {
            None
        }
    }

    /// Cancel the current rebind without changing the binding.
    pub fn cancel_rebind(&mut self) {
        self.rebinding = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_panel_is_hidden() {
        let p = ShortcutsPanel::new(&[]);
        assert!(!p.visible);
    }

    #[test]
    fn open_makes_visible() {
        let mut p = ShortcutsPanel::new(&[]);
        p.open();
        assert!(p.visible);
    }

    #[test]
    fn toggle_opens_and_closes() {
        let mut p = ShortcutsPanel::new(&[]);
        p.toggle();
        assert!(p.visible);
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn close_clears_rebinding() {
        let mut p = ShortcutsPanel::new(&[]);
        p.open();
        p.rebinding = Some(0);
        p.close();
        assert!(p.rebinding.is_none());
        assert!(!p.visible);
    }

    #[test]
    fn default_rows_non_empty() {
        assert!(!default_rows().is_empty());
    }

    #[test]
    fn override_replaces_default_binding() {
        let ov = vec![lumen_storage::KeyboardShortcutEntry {
            command: "Reload".to_string(),
            modifier: "".to_string(),
            key: "F5".to_string(),
        }];
        let p = ShortcutsPanel::new(&ov);
        let row = p.rows.iter().find(|r| r.command == "Reload").unwrap();
        assert_eq!(row.modifier, "");
        assert_eq!(row.key, "F5");
    }

    #[test]
    fn accept_rebind_updates_row_and_returns_triple() {
        let mut p = ShortcutsPanel::new(&[]);
        p.open();
        p.rebinding = Some(0);
        let result = p.accept_rebind("ctrl+shift", "Z");
        let (cmd, modifier, key) = result.unwrap();
        assert_eq!(modifier, "ctrl+shift");
        assert_eq!(key, "Z");
        assert!(!cmd.is_empty());
        assert!(p.rebinding.is_none());
    }

    #[test]
    fn accept_rebind_no_rebinding_returns_none() {
        let mut p = ShortcutsPanel::new(&[]);
        assert!(p.accept_rebind("ctrl", "X").is_none());
    }

    #[test]
    fn binding_label_formats_correctly() {
        let row = ShortcutRow {
            command: "Reload",
            label: "Reload",
            modifier: "ctrl".to_string(),
            key: "R".to_string(),
        };
        assert_eq!(row.binding_label(), "Ctrl+R");
    }

    #[test]
    fn scroll_rows_clamps_to_range() {
        let mut p = ShortcutsPanel::new(&[]);
        p.scroll_rows(-5, 700.0);
        assert_eq!(p.first_row, 0);
        p.scroll_rows(10_000, 700.0);
        assert_eq!(p.first_row, p.rows.len() - ShortcutsPanel::visible_count(700.0));
    }
}
