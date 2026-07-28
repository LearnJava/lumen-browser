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
//! State lives on `Lumen`; [`hit_test`] classifies clicks. The legacy
//! display-list renderer was removed in CC-15-4 - under the engine chrome the
//! panel is `#view-history`.
//! Data is loaded from `lumen_storage::History` on every open / delete / search.

use std::cmp::Reverse;

// ── Geometry ─────────────────────────────────────────────────────────────────

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
}

impl Default for HistoryPanel {
    fn default() -> Self {
        Self {
            visible: false,
            scroll_y: 0.0,
            search_active: false,
            query: String::new(),
            rows: Vec::new(),
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

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Format a Unix timestamp (seconds) as "HH:MM".
///
/// `pub(crate)` (not just module-private) since `Lumen::chrome_model_snapshot`
/// (CC-10b, `main.rs`) formats `#view-history`'s `.hist-time` the same way
/// the deleted legacy overlay renderer did.
pub(crate) fn format_time_hhmm(unix_secs: i64) -> String {
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

    // ── DS-16: Anonymous banner ──────────────────────────────────────────────

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
