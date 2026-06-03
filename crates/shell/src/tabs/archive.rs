//! Tab auto-archive (7A.5): hides tabs inactive for > 12 h from the strip.
//!
//! Auto-archive is a UI-only concept: when a background tab has not been
//! activated for [`ARCHIVE_AFTER_MS`] milliseconds, it is removed from the
//! visible [`TabStrip`] and its title + URL are stored in [`TabArchive`].
//! The full [`PageSnapshot`] is evicted from `bg_tabs` to free memory.
//!
//! Restoration opens a fresh navigation to the stored URL — the full page
//! state is not preserved (that is the job of the T3 hibernate track 10).
//!
//! The archive toolbar button (right 36 px of the tab bar) shows a count
//! badge; clicking it toggles the archive panel.

use lumen_core::geom::Rect;
use lumen_layout::{BorderStyle, Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

use crate::tabs::containers::ContainerKind;

// ── Constants ──────────────────────────────────────────────────────────────────

/// Width of the archive button appended to the right of the tab bar, in CSS px.
pub const ARCHIVE_BTN_W: f32 = 36.0;

/// Background tabs idle for longer than this threshold are auto-archived.
/// Value: 12 hours in session-elapsed milliseconds.
pub const ARCHIVE_AFTER_MS: f64 = 12.0 * 3600.0 * 1000.0;

/// Maximum rows shown in one page of the archive panel without scrolling.
const MAX_VISIBLE_ROWS: usize = 8;

const PANEL_W: f32 = 320.0;
const ROW_H: f32 = 44.0;
const HEADER_H: f32 = 32.0;

// ── Colors ─────────────────────────────────────────────────────────────────────

const BTN_BG: Color = Color { r: 22, g: 22, b: 26, a: 255 };
const BTN_BG_ACTIVE: Color = Color { r: 30, g: 60, b: 100, a: 255 };
const BTN_ICON: Color = Color { r: 160, g: 160, b: 180, a: 255 };
const BADGE_BG: Color = Color { r: 80, g: 140, b: 220, a: 255 };
const BADGE_TEXT: Color = Color { r: 255, g: 255, b: 255, a: 255 };
const DIVIDER: Color = Color { r: 50, g: 52, b: 60, a: 255 };
const PANEL_BG: Color = Color { r: 26, g: 27, b: 32, a: 252 };
const PANEL_BORDER: Color = Color { r: 55, g: 57, b: 68, a: 255 };
const HEADER_TEXT: Color = Color { r: 190, g: 195, b: 210, a: 255 };
const ROW_BG_EVEN: Color = Color { r: 30, g: 31, b: 37, a: 255 };
const ROW_BG_ODD: Color = Color { r: 26, g: 27, b: 32, a: 255 };
const TITLE_TEXT: Color = Color { r: 218, g: 218, b: 228, a: 255 };
const URL_TEXT: Color = Color { r: 110, g: 135, b: 165, a: 255 };
const RESTORE_FG: Color = Color { r: 80, g: 165, b: 230, a: 255 };
const DISMISS_FG: Color = Color { r: 180, g: 80, b: 80, a: 255 };
const EMPTY_TEXT: Color = Color { r: 100, g: 105, b: 118, a: 255 };

// ── Types ─────────────────────────────────────────────────────────────────────

/// A tab that was auto-archived and removed from the visible tab strip.
pub struct ArchivedTab {
    /// Original tab ID (for reference only — not reused on restore).
    pub id: usize,
    /// Display title at the time of archiving.
    pub title: String,
    /// Page URL string; empty for blank/file tabs without a navigable URL.
    pub url: String,
    /// Container colour class of the archived tab.
    ///
    /// Rendered as a 3 px left-side colour strip in the archive panel row,
    /// identical to the border-top strip in the tab bar (7D.2).
    pub container: ContainerKind,
}

/// Hit result from the archive button or panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveHit {
    /// Clicked ↺ restore on the entry with this id.
    Restore(usize),
    /// Clicked × dismiss on the entry with this id.
    Dismiss(usize),
    /// Clicked inside the panel body (no specific control) — swallows event.
    Inside,
    /// Clicked outside the panel — should close it.
    Outside,
}

/// State of the tab archive system.
pub struct TabArchive {
    /// Archived tab entries, newest-first.
    pub entries: Vec<ArchivedTab>,
    /// Whether the archive panel is currently visible.
    pub visible: bool,
    /// Index of the first visible row when the list overflows.
    pub scroll_row: usize,
}

impl Default for TabArchive {
    fn default() -> Self {
        Self::new()
    }
}

impl TabArchive {
    /// Create an empty archive with the panel closed.
    pub fn new() -> Self {
        Self { entries: Vec::new(), visible: false, scroll_row: 0 }
    }

    /// Push a newly-archived tab (prepend — newest entry shown first).
    pub fn push(&mut self, tab: ArchivedTab) {
        self.entries.insert(0, tab);
    }

    /// Remove and return the archived entry with the given original tab `id`.
    pub fn take(&mut self, id: usize) -> Option<ArchivedTab> {
        let pos = self.entries.iter().position(|e| e.id == id)?;
        Some(self.entries.remove(pos))
    }

    /// Number of archived entries.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Toggle panel open/closed; resets scroll on open.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.scroll_row = 0;
        }
    }

    /// Close panel without clearing entries.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Scroll up by one row (clamped at zero).
    #[allow(dead_code)]
    pub fn scroll_up(&mut self) {
        self.scroll_row = self.scroll_row.saturating_sub(1);
    }

    /// Scroll down by one row (clamped at last page).
    #[allow(dead_code)]
    pub fn scroll_down(&mut self) {
        let max_row = self.entries.len().saturating_sub(MAX_VISIBLE_ROWS);
        if self.scroll_row < max_row {
            self.scroll_row += 1;
        }
    }
}

// ── Geometry helpers ───────────────────────────────────────────────────────────

/// Pixel x-coordinate where the archive button begins (right of all tabs).
///
/// `tab_area_w` is the effective width available for tabs (window_w - ARCHIVE_BTN_W).
pub fn archive_btn_x(window_w: f32) -> f32 {
    window_w - ARCHIVE_BTN_W
}

/// Right edge of the archive panel when anchored to the window right.
fn panel_left(window_w: f32) -> f32 {
    window_w - PANEL_W
}

fn panel_height(n_entries: usize) -> f32 {
    let visible = n_entries.min(MAX_VISIBLE_ROWS);
    HEADER_H + visible as f32 * ROW_H
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Hit-test the archive toolbar button area.
///
/// Returns `true` when `(x, y)` falls within the button (rightmost
/// `ARCHIVE_BTN_W` px of the tab bar).
pub fn hit_test_button(x: f32, y: f32, window_w: f32, tab_bar_h: f32) -> bool {
    y >= 0.0 && y < tab_bar_h && x >= archive_btn_x(window_w)
}

/// Hit-test the archive panel when it is open.
///
/// Returns `None` if the panel is hidden.  Otherwise returns an [`ArchiveHit`]
/// variant that describes what was clicked.
pub fn hit_test_panel(
    archive: &TabArchive,
    x: f32,
    y: f32,
    window_w: f32,
    tab_bar_h: f32,
) -> Option<ArchiveHit> {
    if !archive.visible {
        return None;
    }
    let pl = panel_left(window_w);
    let pt = tab_bar_h;
    let ph = panel_height(archive.entries.len());
    let pb = pt + ph;

    // Click outside panel bounds → dismiss.
    if x < pl || x >= window_w || y < pt || y >= pb {
        return Some(ArchiveHit::Outside);
    }

    let rel_y = y - pt;
    if rel_y < HEADER_H {
        // Header row — swallow click.
        return Some(ArchiveHit::Inside);
    }

    let row_y = rel_y - HEADER_H;
    let row_local = (row_y / ROW_H) as usize;
    let entry_idx = archive.scroll_row + row_local;

    let Some(entry) = archive.entries.get(entry_idx) else {
        return Some(ArchiveHit::Inside);
    };
    let entry_id = entry.id;

    // Restore button occupies the left 28 px of the row.
    // Dismiss button occupies the right 28 px of the row.
    if x < pl + 28.0 {
        return Some(ArchiveHit::Restore(entry_id));
    }
    if x >= window_w - 28.0 {
        return Some(ArchiveHit::Dismiss(entry_id));
    }

    Some(ArchiveHit::Inside)
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the archive toolbar button appended to the right of the tab bar.
///
/// `window_w` is the full window width; the button occupies
/// `[window_w - ARCHIVE_BTN_W, window_w)` at y `0..tab_bar_h`.
pub fn build_button(archive: &TabArchive, window_w: f32, tab_bar_h: f32) -> DisplayList {
    let mut out = DisplayList::with_capacity(6);
    let bx = archive_btn_x(window_w);

    // Button background.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(bx, 0.0, ARCHIVE_BTN_W, tab_bar_h),
        color: if archive.visible { BTN_BG_ACTIVE } else { BTN_BG },
    });

    // Left divider separating button from last tab.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(bx, 4.0, 1.0, tab_bar_h - 8.0),
        color: DIVIDER,
    });

    // Archive clock glyph centred in the button.
    let icon_w = 14.0_f32;
    let icon_h = 14.0_f32;
    let icon_x = bx + (ARCHIVE_BTN_W - icon_w) * 0.5;
    let icon_y = (tab_bar_h - icon_h) * 0.5;
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(icon_x, icon_y, icon_w, icon_h),
        text: "\u{25F7}".to_owned(), // ◷ clock face (U+25F7)
        font_size: 12.0,
        color: BTN_ICON,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
    });

    // Count badge (top-right corner of button) when there are archived tabs.
    if archive.count() > 0 {
        let badge_s = 14.0_f32;
        let bx2 = window_w - badge_s - 1.0;
        let by2 = 2.0;
        let r = badge_s * 0.5;
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(bx2, by2, badge_s, badge_s),
            radii: CornerRadii {
                tl: r, tl_y: r, tr: r, tr_y: r,
                br: r, br_y: r, bl: r, bl_y: r,
            },
            color: BADGE_BG,
        });
        let count_str = if archive.count() > 99 {
            "99+".to_owned()
        } else {
            archive.count().to_string()
        };
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(bx2, by2 + 1.0, badge_s, badge_s),
            text: count_str,
            font_size: 8.0,
            color: BADGE_TEXT,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
        });
    }

    out
}

/// Build the drop-down archive panel anchored below the archive button.
///
/// Returns an empty list when the panel is hidden.
pub fn build_panel(archive: &TabArchive, window_w: f32, tab_bar_h: f32) -> DisplayList {
    if !archive.visible {
        return DisplayList::new();
    }

    let pl = panel_left(window_w);
    let pt = tab_bar_h;
    let ph = panel_height(archive.entries.len());
    let visible_count = archive.entries.len().min(MAX_VISIBLE_ROWS);

    let mut out = DisplayList::with_capacity(6 + visible_count * 7);

    // Panel background.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(pl, pt, PANEL_W, ph),
        color: PANEL_BG,
    });

    // Panel border.
    out.push(DisplayCommand::DrawBorder {
        rect: Rect::new(pl, pt, PANEL_W, ph),
        widths: [1.0, 1.0, 1.0, 1.0],
        colors: [PANEL_BORDER; 4],
        styles: [BorderStyle::Solid; 4],
        radii: CornerRadii { tl: 0.0, tl_y: 0.0, tr: 0.0, tr_y: 0.0, br: 3.0, br_y: 3.0, bl: 3.0, bl_y: 3.0 },
    });

    // Header.
    let header_text = if archive.entries.is_empty() {
        "Архив вкладок — пусто".to_owned()
    } else {
        format!("Архив вкладок ({})", archive.count())
    };
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(pl + 10.0, pt + 8.0, PANEL_W - 20.0, 16.0),
        text: header_text,
        font_size: 11.0,
        color: HEADER_TEXT,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
    });

    // Empty state placeholder.
    if archive.entries.is_empty() {
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(pl + 10.0, pt + HEADER_H + 8.0, PANEL_W - 20.0, 14.0),
            text: "Вкладки старше 12 ч будут здесь".to_owned(),
            font_size: 10.0,
            color: EMPTY_TEXT,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
        });
        return out;
    }

    // Rows.
    for (row_local, entry) in archive
        .entries
        .iter()
        .skip(archive.scroll_row)
        .take(visible_count)
        .enumerate()
    {
        let row_top = pt + HEADER_H + row_local as f32 * ROW_H;
        let row_bg = if row_local % 2 == 0 { ROW_BG_EVEN } else { ROW_BG_ODD };

        // Row background.
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(pl, row_top, PANEL_W, ROW_H),
            color: row_bg,
        });

        // Container colour strip — 3 px left border for tabs with a container.
        if let Some(c) = entry.container.border_color() {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(pl, row_top, 3.0, ROW_H),
                color: c,
            });
        }

        // Restore button (↺) — left side.
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(pl + 4.0, row_top + (ROW_H - 16.0) * 0.5, 20.0, 16.0),
            text: "\u{21BA}".to_owned(), // ↺ counterclockwise open circle arrow
            font_size: 14.0,
            color: RESTORE_FG,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
        });

        // Dismiss button (×) — right side.
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(window_w - 22.0, row_top + (ROW_H - 16.0) * 0.5, 16.0, 16.0),
            text: "\u{00D7}".to_owned(), // ×
            font_size: 13.0,
            color: DISMISS_FG,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
        });

        // Title text — truncated.
        let title = truncate_str(&entry.title, 36);
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(pl + 30.0, row_top + 6.0, PANEL_W - 60.0, 14.0),
            text: title,
            font_size: 11.0,
            color: TITLE_TEXT,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
        });

        // URL text — smaller, truncated.
        if !entry.url.is_empty() {
            let url_display = truncate_str(&entry.url, 44);
            out.push(DisplayCommand::DrawText {
                rect: Rect::new(pl + 30.0, row_top + 24.0, PANEL_W - 60.0, 12.0),
                text: url_display,
                font_size: 9.0,
                color: URL_TEXT,
                font_family: Vec::new(),
                font_weight: FontWeight::NORMAL,
                font_style: FontStyle::Normal,
                font_variation_axes: Vec::new(),
                tab_size: 0.0,
            });
        }
    }

    out
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Truncate `s` to at most `max_chars` Unicode scalar values, appending "…"
/// if truncated.  Allocation-free for strings that fit within the limit.
fn truncate_str(s: &str, max_chars: usize) -> String {
    let mut result = String::with_capacity(s.len().min(max_chars * 4 + 3));
    for (i, ch) in s.chars().enumerate() {
        if i >= max_chars {
            result.push('\u{2026}'); // …
            return result;
        }
        result.push(ch);
    }
    result
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tab(id: usize, url: &str) -> ArchivedTab {
        ArchivedTab {
            id,
            title: format!("Tab {id}"),
            url: url.to_owned(),
            container: ContainerKind::None,
        }
    }

    #[test]
    fn new_archive_is_empty() {
        let a = TabArchive::new();
        assert_eq!(a.count(), 0);
        assert!(!a.visible);
    }

    #[test]
    fn push_prepends_newest_first() {
        let mut a = TabArchive::new();
        a.push(make_tab(1, "https://a.com"));
        a.push(make_tab(2, "https://b.com"));
        assert_eq!(a.entries[0].id, 2); // newest first
        assert_eq!(a.entries[1].id, 1);
    }

    #[test]
    fn take_removes_by_id() {
        let mut a = TabArchive::new();
        a.push(make_tab(10, "https://x.com"));
        a.push(make_tab(20, "https://y.com"));
        let removed = a.take(10).unwrap();
        assert_eq!(removed.id, 10);
        assert_eq!(a.count(), 1);
        assert_eq!(a.entries[0].id, 20);
    }

    #[test]
    fn take_missing_id_returns_none() {
        let mut a = TabArchive::new();
        assert!(a.take(99).is_none());
    }

    #[test]
    fn toggle_opens_and_closes_panel() {
        let mut a = TabArchive::new();
        assert!(!a.visible);
        a.toggle();
        assert!(a.visible);
        a.toggle();
        assert!(!a.visible);
    }

    #[test]
    fn toggle_resets_scroll_on_open() {
        let mut a = TabArchive::new();
        a.scroll_row = 5;
        a.toggle(); // open → reset scroll
        assert_eq!(a.scroll_row, 0);
    }

    #[test]
    fn scroll_down_clamps_at_last_page() {
        let mut a = TabArchive::new();
        for i in 0..10 {
            a.push(make_tab(i, ""));
        }
        // MAX_VISIBLE_ROWS = 8, so max scroll_row = 10 - 8 = 2.
        for _ in 0..20 {
            a.scroll_down();
        }
        assert_eq!(a.scroll_row, 2);
    }

    #[test]
    fn scroll_up_clamps_at_zero() {
        let mut a = TabArchive::new();
        a.scroll_row = 0;
        a.scroll_up();
        assert_eq!(a.scroll_row, 0);
    }

    #[test]
    fn hit_test_button_detects_right_strip() {
        // Window 1024px wide, tab bar 36px high.
        let window_w = 1024.0_f32;
        let tab_bar_h = 36.0_f32;
        // Right of archive_btn_x should be detected.
        assert!(hit_test_button(1000.0, 18.0, window_w, tab_bar_h));
        // Inside tab area — not the archive button.
        assert!(!hit_test_button(500.0, 18.0, window_w, tab_bar_h));
        // Below tab bar — not detected.
        assert!(!hit_test_button(1000.0, 40.0, window_w, tab_bar_h));
    }

    #[test]
    fn hit_test_panel_returns_none_when_closed() {
        let a = TabArchive::new();
        assert!(hit_test_panel(&a, 900.0, 50.0, 1024.0, 36.0).is_none());
    }

    #[test]
    fn hit_test_panel_outside_returns_outside() {
        let mut a = TabArchive::new();
        a.push(make_tab(1, "https://a.com"));
        a.visible = true;
        // x far left of panel — outside.
        let hit = hit_test_panel(&a, 100.0, 50.0, 1024.0, 36.0);
        assert_eq!(hit, Some(ArchiveHit::Outside));
    }

    #[test]
    fn hit_test_panel_restore_button_detected() {
        let mut a = TabArchive::new();
        a.push(make_tab(5, "https://example.com"));
        a.visible = true;
        // Row 0 starts at y = tab_bar_h + HEADER_H = 36 + 32 = 68.
        // Restore button: x < panel_left + 28 = (1024-320) + 28 = 732.
        let hit = hit_test_panel(&a, 710.0, 70.0, 1024.0, 36.0);
        assert_eq!(hit, Some(ArchiveHit::Restore(5)));
    }

    #[test]
    fn hit_test_panel_dismiss_button_detected() {
        let mut a = TabArchive::new();
        a.push(make_tab(7, "https://example.com"));
        a.visible = true;
        // Dismiss button: x >= window_w - 28 = 1024 - 28 = 996.
        let hit = hit_test_panel(&a, 1010.0, 70.0, 1024.0, 36.0);
        assert_eq!(hit, Some(ArchiveHit::Dismiss(7)));
    }

    #[test]
    fn build_button_empty_archive_no_badge() {
        let a = TabArchive::new();
        let dl = build_button(&a, 1024.0, 36.0);
        // Should have 3 commands: bg + divider + icon (no badge).
        assert_eq!(dl.len(), 3);
    }

    #[test]
    fn build_button_with_archive_has_badge() {
        let mut a = TabArchive::new();
        a.push(make_tab(1, ""));
        let dl = build_button(&a, 1024.0, 36.0);
        // bg + divider + icon + badge_bg + badge_text = 5 commands.
        assert_eq!(dl.len(), 5);
    }

    #[test]
    fn build_panel_hidden_returns_empty() {
        let a = TabArchive::new();
        let dl = build_panel(&a, 1024.0, 36.0);
        assert!(dl.is_empty());
    }

    #[test]
    fn truncate_str_short_passthrough() {
        let s = truncate_str("hello", 20);
        assert_eq!(s, "hello");
    }

    #[test]
    fn truncate_str_long_appends_ellipsis() {
        let s = truncate_str("abcdefghij", 5);
        assert_eq!(s, "abcde\u{2026}");
    }

    #[test]
    fn archive_after_ms_is_twelve_hours() {
        assert_eq!(ARCHIVE_AFTER_MS, 43_200_000.0);
    }
}
