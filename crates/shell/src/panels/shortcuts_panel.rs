//! Keyboard shortcuts settings panel (D-4).
//!
//! A centred overlay (360 × 500 px) opened by `Ctrl+Shift+/`.
//! Displays all `KeyCommand` variants with their current keybinding.
//! Clicking a row enters rebind mode — the next keypress is recorded
//! as the new binding and persisted via `lumen_storage::KeyboardShortcuts`.

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand};

type DisplayList = Vec<DisplayCommand>;

// ── Geometry ─────────────────────────────────────────────────────────────────

/// Panel width in CSS px.
pub const PANEL_W: f32 = 360.0;
/// Panel height in CSS px.
pub const PANEL_H: f32 = 500.0;
/// Header bar height.
const HEADER_H: f32 = 36.0;
/// Height of one shortcut row.
const ROW_H: f32 = 36.0;
/// Left / right padding inside each row.
const PAD_H: f32 = 14.0;
/// Width of the × close button hit zone.
const CLOSE_W: f32 = 30.0;
/// Visible content area height (panel minus header).
const CONTENT_H: f32 = PANEL_H - HEADER_H;

// ── Colours ──────────────────────────────────────────────────────────────────

const PANEL_BG: Color = Color { r: 18, g: 18, b: 26, a: 254 };
const PANEL_BORDER: Color = Color { r: 52, g: 52, b: 66, a: 255 };
const HEADER_BG: Color = Color { r: 26, g: 26, b: 36, a: 255 };
const HEADER_TEXT: Color = Color { r: 210, g: 210, b: 225, a: 255 };
const CLOSE_COL: Color = Color { r: 180, g: 80, b: 80, a: 255 };
const ROW_EVEN: Color = Color { r: 22, g: 22, b: 32, a: 255 };
const ROW_ODD: Color = Color { r: 26, g: 26, b: 36, a: 255 };
const ROW_REBIND: Color = Color { r: 30, g: 50, b: 80, a: 255 };
const LABEL_COL: Color = Color { r: 200, g: 200, b: 218, a: 255 };
const KEY_COL: Color = Color { r: 100, g: 160, b: 240, a: 255 };
const KEY_BADGE_BG: Color = Color { r: 34, g: 44, b: 64, a: 255 };
const REBIND_TEXT: Color = Color { r: 200, g: 170, b: 80, a: 255 };
const SEPARATOR: Color = Color { r: 36, g: 36, b: 50, a: 255 };

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
        ("DownloadsPanel",   "Загрузки",               "ctrl+shift",  "J"),
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

/// Hit result from `hit_test`.
#[derive(Debug, Clone, PartialEq)]
pub enum ShortcutsHit {
    /// User clicked the × close button.
    Close,
    /// User clicked a shortcut row to start rebinding (row index).
    StartRebind(usize),
    /// Click inside panel but not on an actionable element.
    Consumed,
}

/// Keyboard shortcuts panel UI state.
#[derive(Debug)]
pub struct ShortcutsPanel {
    /// Whether the panel is currently visible.
    pub visible: bool,
    /// Vertical scroll offset in the content area (px).
    pub scroll_y: f32,
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
        Self { visible: false, scroll_y: 0.0, rebinding: None, rows }
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

    /// Scroll the content area by `delta` px (clamped to valid range).
    pub fn scroll_by(&mut self, delta: f32) {
        let max_scroll = (self.rows.len() as f32 * ROW_H - CONTENT_H).max(0.0);
        self.scroll_y = (self.scroll_y + delta).clamp(0.0, max_scroll);
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

    /// Hit-test a click at `(cx, cy)` in panel-local coordinates.
    pub fn hit_test(&self, cx: f32, cy: f32) -> ShortcutsHit {
        // Close button (top-right corner of header).
        if cy < HEADER_H && cx > PANEL_W - CLOSE_W {
            return ShortcutsHit::Close;
        }
        // Content area: check which row was clicked.
        if cy >= HEADER_H {
            let content_y = cy - HEADER_H + self.scroll_y;
            let row_idx = (content_y / ROW_H) as usize;
            if row_idx < self.rows.len() {
                return ShortcutsHit::StartRebind(row_idx);
            }
        }
        ShortcutsHit::Consumed
    }

    /// Render the panel into `dl`, anchored at `(ox, oy)` in screen space.
    pub fn build_panel(&self, dl: &mut DisplayList, ox: f32, oy: f32) {
        // Outer border.
        dl.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(ox - 1.0, oy - 1.0, PANEL_W + 2.0, PANEL_H + 2.0),
            radii: CornerRadii { tl: 6.0, tl_y: 6.0, tr: 6.0, tr_y: 6.0,
                                 bl: 6.0, bl_y: 6.0, br: 6.0, br_y: 6.0 },
            color: PANEL_BORDER,
        });
        // Panel background.
        dl.push(DisplayCommand::FillRect {
            rect: Rect::new(ox, oy, PANEL_W, PANEL_H),
            color: PANEL_BG,
        });

        // Header bar.
        dl.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(ox, oy, PANEL_W, HEADER_H),
            radii: CornerRadii { tl: 5.0, tl_y: 5.0, tr: 5.0, tr_y: 5.0,
                                 bl: 0.0, bl_y: 0.0, br: 0.0, br_y: 0.0 },
            color: HEADER_BG,
        });
        dl.push(txt("Горячие клавиши", ox + PAD_H, oy + 10.0,
                    PANEL_W - PAD_H * 2.0 - CLOSE_W, 13.0, FontWeight::BOLD, HEADER_TEXT));
        dl.push(txt("×", ox + PANEL_W - CLOSE_W + 6.0, oy + 9.0,
                    20.0, 15.0, FontWeight::BOLD, CLOSE_COL));

        // Clip content area.
        dl.push(DisplayCommand::PushClipRect {
            rect: Rect::new(ox, oy + HEADER_H, PANEL_W, CONTENT_H),
        });

        let visible_start = (self.scroll_y / ROW_H) as usize;
        let visible_end = ((self.scroll_y + CONTENT_H) / ROW_H).ceil() as usize + 1;

        for (i, row) in self.rows.iter().enumerate() {
            if i < visible_start || i > visible_end {
                continue;
            }
            let row_top = oy + HEADER_H + i as f32 * ROW_H - self.scroll_y;
            let bg = if self.rebinding == Some(i) {
                ROW_REBIND
            } else if i % 2 == 0 {
                ROW_EVEN
            } else {
                ROW_ODD
            };
            dl.push(DisplayCommand::FillRect {
                rect: Rect::new(ox, row_top, PANEL_W, ROW_H),
                color: bg,
            });
            // Separator at bottom of row.
            dl.push(DisplayCommand::FillRect {
                rect: Rect::new(ox, row_top + ROW_H - 1.0, PANEL_W, 1.0),
                color: SEPARATOR,
            });
            // Action label (left).
            dl.push(txt(row.label, ox + PAD_H, row_top + 10.0,
                        PANEL_W * 0.58, 12.0, FontWeight::NORMAL, LABEL_COL));
            // Key badge (right) or rebind hint.
            let (badge_text, badge_col) = if self.rebinding == Some(i) {
                ("Нажмите клавишу\u{2026}".to_owned(), REBIND_TEXT)
            } else {
                (row.binding_label(), KEY_COL)
            };
            let badge_x = ox + PANEL_W - PAD_H - 120.0;
            dl.push(DisplayCommand::FillRoundedRect {
                rect: Rect::new(badge_x - 4.0, row_top + 7.0, 128.0, 22.0),
                radii: CornerRadii { tl: 3.0, tl_y: 3.0, tr: 3.0, tr_y: 3.0,
                                     bl: 3.0, bl_y: 3.0, br: 3.0, br_y: 3.0 },
                color: KEY_BADGE_BG,
            });
            dl.push(txt(badge_text, badge_x, row_top + 10.0,
                        120.0, 11.0, FontWeight::NORMAL, badge_col));
        }

        dl.push(DisplayCommand::PopClip);
    }
}

fn txt(text: impl Into<String>, x: f32, y: f32, w: f32, font_size: f32,
       weight: FontWeight, color: Color) -> DisplayCommand {
    DisplayCommand::DrawText {
        rect: Rect::new(x, y, w, font_size * 1.4),
        text: text.into(),
        font_size,
        color,
        font_family: Vec::new(),
        font_weight: weight,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
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
    fn hit_test_close_button() {
        let p = ShortcutsPanel::new(&[]);
        assert_eq!(p.hit_test(PANEL_W - 5.0, 10.0), ShortcutsHit::Close);
    }

    #[test]
    fn hit_test_row_starts_rebind() {
        let p = ShortcutsPanel::new(&[]);
        let hit = p.hit_test(100.0, HEADER_H + 5.0);
        assert_eq!(hit, ShortcutsHit::StartRebind(0));
    }

    #[test]
    fn scroll_clamps_to_range() {
        let mut p = ShortcutsPanel::new(&[]);
        p.open();
        p.scroll_by(-100.0);
        assert_eq!(p.scroll_y, 0.0);
        p.scroll_by(999_999.0);
        let max = (p.rows.len() as f32 * ROW_H - CONTENT_H).max(0.0);
        assert_eq!(p.scroll_y, max);
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
}
