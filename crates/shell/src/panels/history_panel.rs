//! Browser history panel (D-5).
//!
//! A floating overlay toggled by `Ctrl+H` that shows the user's browsing history.
//! Layout:
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │ History (42)                               × │  header
//! │ ┌─────────────────────────────────────────┐ │
//! │ │ search…                                  │ │  search box
//! │ └─────────────────────────────────────────┘ │
//! │ Today ───────────────────────────────────── │  date group header
//! │  Page Title                             ×   │  entry row (title + url)
//! │  https://example.com/               12:34   │
//! │  ──────────────────────────────────────── │
//! │ Yesterday ───────────────────────────────── │
//! │  …                                          │
//! │                              [Очистить всё] │  clear button
//! └─────────────────────────────────────────────┘
//! ```
//!
//! State lives on `Lumen`. [`hit_test`] classifies clicks; [`build_panel`] renders.
//! Data is loaded from `lumen_storage::History` on every open / delete / search.

use std::cmp::Reverse;
use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

// ── Geometry ─────────────────────────────────────────────────────────────────

/// Panel width in CSS px.
pub const PANEL_W: f32 = 480.0;

/// Total panel height in CSS px.
pub const PANEL_H: f32 = 500.0;

/// Header strip height.
const HEADER_H: f32 = 32.0;

/// Search box height including top padding.
const SEARCH_H: f32 = 36.0;

/// Height of a date-group header row.
const GROUP_H: f32 = 22.0;

/// Height of a single history entry row.
const ROW_H: f32 = 44.0;

/// Footer height (clear-all button).
const FOOTER_H: f32 = 36.0;

/// Outer padding.
const PAD: f32 = 10.0;

/// Width of the "×" delete zone per row.
const DELETE_W: f32 = 26.0;

/// Maximum title characters before ellipsis.
const TITLE_MAX_CHARS: usize = 52;

/// Maximum URL characters before ellipsis.
const URL_MAX_CHARS: usize = 60;

// ── Colours ──────────────────────────────────────────────────────────────────

const PANEL_BG: Color = Color { r: 20, g: 20, b: 27, a: 252 };
const PANEL_BORDER: Color = Color { r: 55, g: 55, b: 68, a: 255 };
const HEADER_BG: Color = Color { r: 28, g: 28, b: 36, a: 255 };
const HEADER_TEXT: Color = Color { r: 200, g: 200, b: 216, a: 255 };
const CLOSE_TEXT: Color = Color { r: 180, g: 90, b: 90, a: 255 };
const SEARCH_BG: Color = Color { r: 14, g: 14, b: 20, a: 255 };
const SEARCH_TEXT: Color = Color { r: 160, g: 160, b: 176, a: 255 };
const SEARCH_ACTIVE_BG: Color = Color { r: 18, g: 24, b: 38, a: 255 };
const SEARCH_ACTIVE_TEXT: Color = Color { r: 220, g: 220, b: 235, a: 255 };
const GROUP_BG: Color = Color { r: 26, g: 26, b: 34, a: 255 };
const GROUP_TEXT: Color = Color { r: 120, g: 120, b: 140, a: 255 };
const ROW_EVEN: Color = Color { r: 22, g: 22, b: 30, a: 255 };
const ROW_ODD: Color = Color { r: 26, g: 26, b: 34, a: 255 };
const ROW_HOVER_BG: Color = Color { r: 36, g: 36, b: 48, a: 255 };
const TITLE_TEXT: Color = Color { r: 220, g: 220, b: 232, a: 255 };
const URL_TEXT: Color = Color { r: 100, g: 140, b: 210, a: 255 };
const TIME_TEXT: Color = Color { r: 100, g: 100, b: 116, a: 255 };
const DELETE_TEXT: Color = Color { r: 170, g: 80, b: 80, a: 255 };
const SEPARATOR: Color = Color { r: 36, g: 36, b: 48, a: 255 };
const FOOTER_BG: Color = Color { r: 22, g: 22, b: 30, a: 255 };
const CLEAR_BTN_BG: Color = Color { r: 140, g: 50, b: 50, a: 200 };
const CLEAR_BTN_TEXT: Color = Color { r: 240, g: 200, b: 200, a: 255 };
const EMPTY_TEXT: Color = Color { r: 100, g: 100, b: 115, a: 255 };

// ── Data types ────────────────────────────────────────────────────────────────

/// Lightweight history entry for panel rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryItem {
    /// Database id (matches `lumen_storage::HistoryEntry::id`).
    pub id: i64,
    /// Full page URL.
    pub url: String,
    /// Page title (may be empty — URL shown as fallback).
    pub title: String,
    /// Unix timestamp (seconds) of the last visit.
    pub visit_date: i64,
    /// Number of times this URL has been visited.
    pub visit_count: i64,
}

/// One display row in the scrollable body — either a date-group header or an entry.
#[derive(Debug, Clone)]
pub enum HistoryRow {
    /// A date separator label (e.g. "Today", "Yesterday", "2026-05-30").
    Group(String),
    /// A history entry row.
    Entry(HistoryItem),
}

/// History panel state.
#[derive(Debug)]
pub struct HistoryPanel {
    /// Whether the panel is currently visible.
    pub visible: bool,
    /// Vertical scroll offset in CSS px into the body area.
    pub scroll_y: f32,
    /// Whether the search box is focused.
    pub search_active: bool,
    /// Current search query string.
    pub query: String,
    /// Ordered display rows (groups + entries) for the current view.
    pub rows: Vec<HistoryRow>,
    /// Hovered row index into `rows` (for hover highlight), or `None`.
    pub hover_row: Option<usize>,
}

impl Default for HistoryPanel {
    fn default() -> Self {
        Self {
            visible: false,
            scroll_y: 0.0,
            search_active: false,
            query: String::new(),
            rows: Vec::new(),
            hover_row: None,
        }
    }
}

impl HistoryPanel {
    /// Create a new, hidden panel.
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggle visibility and reset scroll/search when opening.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.scroll_y = 0.0;
            self.search_active = false;
        }
    }

    /// Replace the displayed rows (call after data refresh or search).
    pub fn set_items(&mut self, items: Vec<HistoryItem>) {
        self.rows = build_rows(items);
    }

    /// Append a character to the search query.
    pub fn append_search(&mut self, ch: char) {
        self.query.push(ch);
    }

    /// Delete the last character from the search query.
    pub fn backspace_search(&mut self) {
        self.query.pop();
    }

    /// Scroll by `dy` CSS px (positive = down).
    pub fn scroll_by(&mut self, dy: f32) {
        let max = self.max_scroll();
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, max);
    }

    /// Maximum scroll offset for the current row set.
    pub fn max_scroll(&self) -> f32 {
        let total_h: f32 = self.rows.iter().map(row_height).sum();
        let body_h = PANEL_H - HEADER_H - SEARCH_H - FOOTER_H;
        (total_h - body_h).max(0.0)
    }
}

// ── Row builder ───────────────────────────────────────────────────────────────

/// Build the display row list: insert date-group headers between entries.
fn build_rows(mut items: Vec<HistoryItem>) -> Vec<HistoryRow> {
    // Items come in newest-first order from the DB.
    items.sort_by_key(|a| Reverse(a.visit_date));

    let now_secs = now_unix_secs();
    let mut rows: Vec<HistoryRow> = Vec::with_capacity(items.len() * 2);
    let mut last_day: Option<i64> = None;

    for item in items {
        let day = item.visit_date / 86400;
        if last_day != Some(day) {
            let label = format_day_label(item.visit_date, now_secs);
            rows.push(HistoryRow::Group(label));
            last_day = Some(day);
        }
        rows.push(HistoryRow::Entry(item));
    }
    rows
}

fn row_height(row: &HistoryRow) -> f32 {
    match row {
        HistoryRow::Group(_) => GROUP_H,
        HistoryRow::Entry(_) => ROW_H,
    }
}

// ── Hit testing ───────────────────────────────────────────────────────────────

/// Result of a click inside the history panel.
#[derive(Debug, Clone, PartialEq)]
pub enum HistoryHit {
    /// Click on the header "×" close button.
    Close,
    /// Click on the search box area.
    FocusSearch,
    /// Click on the "Очистить всё" button.
    ClearAll,
    /// Click on the "×" delete button of an entry (entry id).
    Delete(i64),
    /// Click on an entry row body → navigate to URL.
    Navigate(String),
    /// Click lands inside the panel but no specific action.
    Inside,
    /// Click lands outside the panel.
    Outside,
}

/// Classify a click at `(mx, my)` in window-space CSS px.
///
/// `(px, py)` is the panel's top-left corner.
pub fn hit_test(panel: &HistoryPanel, mx: f32, my: f32, px: f32, py: f32) -> HistoryHit {
    if mx < px || mx > px + PANEL_W || my < py || my > py + PANEL_H {
        return HistoryHit::Outside;
    }
    // Header.
    if my < py + HEADER_H {
        if mx >= px + PANEL_W - 28.0 {
            return HistoryHit::Close;
        }
        return HistoryHit::Inside;
    }
    // Search box.
    if my < py + HEADER_H + SEARCH_H {
        return HistoryHit::FocusSearch;
    }
    // Footer.
    let footer_y = py + PANEL_H - FOOTER_H;
    if my >= footer_y {
        let btn_x = px + PANEL_W - PAD - 90.0;
        if mx >= btn_x {
            return HistoryHit::ClearAll;
        }
        return HistoryHit::Inside;
    }
    // Body rows.
    let body_top = py + HEADER_H + SEARCH_H;
    let local_y = my - body_top + panel.scroll_y;
    let mut cursor = 0.0_f32;
    for row in &panel.rows {
        let h = row_height(row);
        if local_y < cursor + h {
            return match row {
                HistoryRow::Group(_) => HistoryHit::Inside,
                HistoryRow::Entry(item) => {
                    if mx >= px + PANEL_W - DELETE_W - PAD {
                        HistoryHit::Delete(item.id)
                    } else {
                        HistoryHit::Navigate(item.url.clone())
                    }
                }
            };
        }
        cursor += h;
    }
    HistoryHit::Inside
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the panel display list.
///
/// `(win_w, toolbar_h)` — full window width and toolbar height in CSS px.
pub fn build_panel(panel: &HistoryPanel, win_w: f32, toolbar_h: f32) -> DisplayList {
    let mut dl: DisplayList = Vec::new();
    if !panel.visible {
        return dl;
    }

    // Position: centred horizontally, anchored below toolbar.
    let px = (win_w - PANEL_W) * 0.5;
    let py = toolbar_h + 4.0;

    // ── Outer border + background ────────────────────────────────────────────
    dl.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, PANEL_H),
        radii: uniform_radii(7.0),
        color: PANEL_BORDER,
    });
    dl.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px + 1.0, py + 1.0, PANEL_W - 2.0, PANEL_H - 2.0),
        radii: uniform_radii(6.0),
        color: PANEL_BG,
    });

    // ── Header ───────────────────────────────────────────────────────────────
    dl.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, HEADER_H),
        radii: CornerRadii {
            tl: 6.0,
            tl_y: 6.0,
            tr: 6.0,
            tr_y: 6.0,
            bl: 0.0,
            bl_y: 0.0,
            br: 0.0,
            br_y: 0.0,
        },
        color: HEADER_BG,
    });
    let count = panel.rows.iter().filter(|r| matches!(r, HistoryRow::Entry(_))).count();
    let header_label = if count == 0 {
        "History".to_owned()
    } else {
        format!("History ({count})")
    };
    dl.push(make_text(
        header_label,
        px + PAD,
        py + 9.0,
        200.0,
        13.0,
        FontWeight::BOLD,
        HEADER_TEXT,
    ));
    dl.push(make_text(
        "×".to_owned(),
        px + PANEL_W - 22.0,
        py + 8.0,
        20.0,
        15.0,
        FontWeight::BOLD,
        CLOSE_TEXT,
    ));
    dl.push(DisplayCommand::FillRect {
        rect: Rect::new(px, py + HEADER_H - 1.0, PANEL_W, 1.0),
        color: SEPARATOR,
    });

    // ── Search box ───────────────────────────────────────────────────────────
    let sx = px + PAD;
    let sy = py + HEADER_H + 5.0;
    let sw = PANEL_W - 2.0 * PAD;
    let sh = SEARCH_H - 10.0;
    let (sbg, stxt) = if panel.search_active {
        (SEARCH_ACTIVE_BG, SEARCH_ACTIVE_TEXT)
    } else {
        (SEARCH_BG, SEARCH_TEXT)
    };
    dl.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(sx, sy, sw, sh),
        radii: uniform_radii(4.0),
        color: sbg,
    });
    let search_display = if panel.query.is_empty() {
        if panel.search_active { String::new() } else { "Search history…".to_owned() }
    } else {
        panel.query.clone()
    };
    dl.push(make_text(search_display, sx + 6.0, sy + 4.0, sw - 12.0, 12.0, FontWeight::NORMAL, stxt));

    // ── Body ─────────────────────────────────────────────────────────────────
    let body_top = py + HEADER_H + SEARCH_H;
    let body_h = PANEL_H - HEADER_H - SEARCH_H - FOOTER_H;
    dl.push(DisplayCommand::PushClipRect { rect: Rect::new(px, body_top, PANEL_W, body_h) });

    if panel.rows.is_empty() {
        dl.push(make_text(
            "No browsing history yet.".to_owned(),
            px + PAD,
            body_top + 20.0,
            PANEL_W - 2.0 * PAD,
            12.0,
            FontWeight::NORMAL,
            EMPTY_TEXT,
        ));
    } else {
        let scroll = panel.scroll_y;
        let mut cursor = 0.0_f32;
        let mut entry_idx = 0_usize;

        for row in &panel.rows {
            let h = row_height(row);
            let ry = body_top + cursor - scroll;
            if ry + h >= body_top && ry <= body_top + body_h {
                match row {
                    HistoryRow::Group(label) => {
                        dl.push(DisplayCommand::FillRect {
                            rect: Rect::new(px, ry, PANEL_W, GROUP_H),
                            color: GROUP_BG,
                        });
                        dl.push(make_text(
                            label.clone(),
                            px + PAD,
                            ry + 4.0,
                            PANEL_W - 2.0 * PAD,
                            10.5,
                            FontWeight::BOLD,
                            GROUP_TEXT,
                        ));
                    }
                    HistoryRow::Entry(item) => {
                        let row_bg = if entry_idx.is_multiple_of(2) { ROW_EVEN } else { ROW_ODD };
                        let row_bg = if panel.hover_row == Some(entry_idx) {
                            ROW_HOVER_BG
                        } else {
                            row_bg
                        };
                        dl.push(DisplayCommand::FillRect {
                            rect: Rect::new(px, ry, PANEL_W, ROW_H),
                            color: row_bg,
                        });

                        // Title (or URL fallback).
                        let title = if item.title.is_empty() {
                            truncate_str(&item.url, TITLE_MAX_CHARS)
                        } else {
                            truncate_str(&item.title, TITLE_MAX_CHARS)
                        };
                        let title_w = PANEL_W - 2.0 * PAD - DELETE_W - 50.0;
                        dl.push(make_text(
                            title,
                            px + PAD,
                            ry + 7.0,
                            title_w,
                            12.0,
                            FontWeight::NORMAL,
                            TITLE_TEXT,
                        ));

                        // URL.
                        let url_w = PANEL_W - 2.0 * PAD - DELETE_W - 50.0;
                        let url_short = truncate_str(&item.url, URL_MAX_CHARS);
                        dl.push(make_text(
                            url_short,
                            px + PAD,
                            ry + 24.0,
                            url_w,
                            10.5,
                            FontWeight::NORMAL,
                            URL_TEXT,
                        ));

                        // Time (HH:MM).
                        let time_str = format_time_hhmm(item.visit_date);
                        dl.push(make_text(
                            time_str,
                            px + PANEL_W - DELETE_W - PAD - 38.0,
                            ry + 14.0,
                            36.0,
                            10.0,
                            FontWeight::NORMAL,
                            TIME_TEXT,
                        ));

                        // Delete button.
                        dl.push(make_text(
                            "×".to_owned(),
                            px + PANEL_W - DELETE_W + 2.0,
                            ry + 14.0,
                            20.0,
                            14.0,
                            FontWeight::BOLD,
                            DELETE_TEXT,
                        ));

                        // Row separator.
                        dl.push(DisplayCommand::FillRect {
                            rect: Rect::new(px + PAD, ry + ROW_H - 1.0, PANEL_W - 2.0 * PAD, 1.0),
                            color: SEPARATOR,
                        });
                        entry_idx += 1;
                    }
                }
            }
            cursor += h;
        }
    }

    dl.push(DisplayCommand::PopClip);

    // ── Footer ────────────────────────────────────────────────────────────────
    let fy = py + PANEL_H - FOOTER_H;
    dl.push(DisplayCommand::FillRect { rect: Rect::new(px, fy, PANEL_W, FOOTER_H), color: FOOTER_BG });
    dl.push(DisplayCommand::FillRect {
        rect: Rect::new(px, fy, PANEL_W, 1.0),
        color: SEPARATOR,
    });

    // "Очистить всё" button.
    let btn_x = px + PANEL_W - PAD - 90.0;
    let btn_y = fy + 7.0;
    dl.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(btn_x, btn_y, 88.0, 22.0),
        radii: uniform_radii(4.0),
        color: CLEAR_BTN_BG,
    });
    dl.push(make_text(
        "Очистить всё".to_owned(),
        btn_x + 5.0,
        btn_y + 4.0,
        78.0,
        11.0,
        FontWeight::NORMAL,
        CLEAR_BTN_TEXT,
    ));

    dl
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_text(
    text: String,
    x: f32,
    y: f32,
    w: f32,
    font_size: f32,
    weight: FontWeight,
    color: Color,
) -> DisplayCommand {
    DisplayCommand::DrawText {
        rect: Rect::new(x, y, w, font_size * 1.4),
        text,
        font_size,
        color,
        font_family: Vec::new(),
        font_weight: weight,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    }
}

fn uniform_radii(r: f32) -> CornerRadii {
    CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, bl: r, bl_y: r, br: r, br_y: r }
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(max_chars + 1);
    for (i, c) in s.chars().enumerate() {
        if i == max_chars {
            out.push('…');
            return out;
        }
        out.push(c);
    }
    out
}

/// Format a Unix timestamp (seconds) as "HH:MM".
fn format_time_hhmm(unix_secs: i64) -> String {
    if unix_secs < 0 {
        return "--:--".to_owned();
    }
    let secs_in_day = unix_secs % 86400;
    let h = secs_in_day / 3600;
    let m = (secs_in_day % 3600) / 60;
    format!("{h:02}:{m:02}")
}

/// Format a day label: "Today", "Yesterday", or "YYYY-MM-DD".
fn format_day_label(unix_secs: i64, now_secs: i64) -> String {
    let today_start = (now_secs / 86400) * 86400;
    let yesterday_start = today_start - 86400;
    if unix_secs >= today_start {
        return "Today".to_owned();
    }
    if unix_secs >= yesterday_start {
        return "Yesterday".to_owned();
    }
    format_unix_date(unix_secs)
}

/// Format a Unix timestamp (seconds) as "YYYY-MM-DD".
fn format_unix_date(unix_secs: i64) -> String {
    if unix_secs < 0 {
        return "–".to_owned();
    }
    let days = (unix_secs / 86400) as u64;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Return the current time as Unix seconds (best-effort; falls back to 0).
fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(id: i64, url: &str, title: &str, visit_date: i64) -> HistoryItem {
        HistoryItem { id, url: url.to_owned(), title: title.to_owned(), visit_date, visit_count: 1 }
    }

    #[test]
    fn panel_starts_hidden() {
        let panel = HistoryPanel::new();
        assert!(!panel.visible);
    }

    #[test]
    fn toggle_opens_and_resets_scroll() {
        let mut panel = HistoryPanel::new();
        panel.scroll_y = 120.0;
        panel.toggle();
        assert!(panel.visible);
        assert_eq!(panel.scroll_y, 0.0);
    }

    #[test]
    fn toggle_closes() {
        let mut panel = HistoryPanel::new();
        panel.toggle();
        panel.toggle();
        assert!(!panel.visible);
    }

    #[test]
    fn set_items_groups_by_day() {
        let mut panel = HistoryPanel::new();
        let items = vec![
            make_item(1, "https://a.com", "A", 86400 * 100 + 3600),
            make_item(2, "https://b.com", "B", 86400 * 100 + 7200),
            make_item(3, "https://c.com", "C", 86400 * 101 + 1000),
        ];
        panel.set_items(items);
        // Sorted newest-first: day 101 then day 100.
        // Group(101), Entry(3), Group(100), Entry(2), Entry(1) = 5 rows.
        assert_eq!(panel.rows.len(), 5);
        assert!(matches!(panel.rows[0], HistoryRow::Group(_)));
        assert!(matches!(panel.rows[1], HistoryRow::Entry(_)));
        assert!(matches!(panel.rows[2], HistoryRow::Group(_)));
        assert!(matches!(panel.rows[3], HistoryRow::Entry(_)));
        assert!(matches!(panel.rows[4], HistoryRow::Entry(_)));
    }

    #[test]
    fn set_items_single_day_one_group() {
        let mut panel = HistoryPanel::new();
        let items = vec![
            make_item(1, "https://x.com", "X", 86400 * 200 + 100),
            make_item(2, "https://y.com", "Y", 86400 * 200 + 200),
        ];
        panel.set_items(items);
        assert_eq!(panel.rows.len(), 3); // 1 group + 2 entries
    }

    #[test]
    fn search_append_backspace() {
        let mut panel = HistoryPanel::new();
        panel.append_search('r');
        panel.append_search('u');
        panel.append_search('s');
        panel.append_search('t');
        assert_eq!(panel.query, "rust");
        panel.backspace_search();
        assert_eq!(panel.query, "rus");
    }

    #[test]
    fn scroll_clamped_to_zero() {
        let mut panel = HistoryPanel::new();
        panel.scroll_by(-100.0);
        assert_eq!(panel.scroll_y, 0.0);
    }

    #[test]
    fn hit_test_outside() {
        let panel = HistoryPanel::new();
        let hit = hit_test(&panel, 0.0, 0.0, 200.0, 100.0);
        assert_eq!(hit, HistoryHit::Outside);
    }

    #[test]
    fn hit_test_close_button() {
        let panel = HistoryPanel::new();
        let px = 100.0_f32;
        let py = 50.0_f32;
        // Close button is at px + PANEL_W - 28.0 to px + PANEL_W, within header height.
        let hit = hit_test(&panel, px + PANEL_W - 10.0, py + 5.0, px, py);
        assert_eq!(hit, HistoryHit::Close);
    }

    #[test]
    fn hit_test_search_box() {
        let panel = HistoryPanel::new();
        let px = 100.0_f32;
        let py = 50.0_f32;
        let hit = hit_test(&panel, px + 50.0, py + HEADER_H + 5.0, px, py);
        assert_eq!(hit, HistoryHit::FocusSearch);
    }

    #[test]
    fn hit_test_clear_all() {
        let panel = HistoryPanel::new();
        let px = 100.0_f32;
        let py = 50.0_f32;
        // Clear button is in footer, right side.
        let hit = hit_test(&panel, px + PANEL_W - PAD - 5.0, py + PANEL_H - 15.0, px, py);
        assert_eq!(hit, HistoryHit::ClearAll);
    }

    #[test]
    fn build_panel_empty_no_crash() {
        let panel = HistoryPanel::new();
        let dl = build_panel(&panel, 1280.0, 40.0);
        assert!(dl.is_empty()); // panel is not visible
    }

    #[test]
    fn build_panel_visible_has_commands() {
        let mut panel = HistoryPanel::new();
        panel.toggle();
        let dl = build_panel(&panel, 1280.0, 40.0);
        assert!(!dl.is_empty());
    }

    #[test]
    fn format_day_label_today() {
        let now = now_unix_secs();
        let label = format_day_label(now - 60, now); // 1 minute ago
        assert_eq!(label, "Today");
    }

    #[test]
    fn format_day_label_yesterday() {
        let now = 86400 * 1000 + 43200; // midday day 1000
        let yesterday = 86400 * 999 + 3600;
        let label = format_day_label(yesterday, now);
        assert_eq!(label, "Yesterday");
    }
}
