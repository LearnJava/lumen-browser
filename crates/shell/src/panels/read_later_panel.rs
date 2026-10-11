//! §12.3 Read-later panel (Ctrl+Shift+R).
//!
//! Lists saved pages (newest-first). Each row shows title + host + save date +
//! unread/read badge; clicking opens the offline HTML snapshot; pressing ×
//! deletes the entry. The panel is rendered as `#readLaterPanel` in the engine
//! chrome (`bind_read_later`, UX-CHROME-PANELS); clicks arrive as
//! `read-later-open` / `read-later-delete` actions.
//!
//! State lives on `Lumen`; this module only holds the open/scroll state and
//! builds the [`lumen_chrome::ChromeReadLaterModel`] snapshot.

use lumen_chrome::{ChromeReadLaterModel, ChromeReadLaterRowModel};
use lumen_knowledge::{ReadLaterEntry, ReadStatus};

/// Rows shown at once (the chrome list has no wheel container of its own).
const VISIBLE_ROWS: usize = 8;

/// Read-later panel state.
#[derive(Debug, Default)]
pub struct ReadLaterPanel {
    /// Whether the panel is currently visible.
    pub visible: bool,
    /// Index of the first visible row.
    pub scroll_row: usize,
    /// Cached entry list refreshed by [`ReadLaterPanel::refresh`].
    pub entries: Vec<ReadLaterEntry>,
}

impl ReadLaterPanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggle visibility; resets scroll when opening.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.scroll_row = 0;
        }
    }

    /// Replace the cached entry list (call after save/delete or on open).
    pub fn refresh(&mut self, entries: Vec<ReadLaterEntry>) {
        self.entries = entries;
        self.scroll_row = self.scroll_row.min(self.max_scroll());
    }

    pub fn scroll_up(&mut self) {
        self.scroll_row = self.scroll_row.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        self.scroll_row = (self.scroll_row + 1).min(self.max_scroll());
    }

    /// Maximum first-row index for the current entry count.
    pub fn max_scroll(&self) -> usize {
        self.entries.len().saturating_sub(VISIBLE_ROWS)
    }

    /// `#readLaterPanel` snapshot: the window of [`VISIBLE_ROWS`] rows from `scroll_row`.
    pub fn chrome_model(&self) -> ChromeReadLaterModel {
        let rows = self
            .entries
            .iter()
            .skip(self.scroll_row)
            .take(VISIBLE_ROWS)
            .map(|e| {
                let (status, read) = match e.status {
                    ReadStatus::Unread => ("Unread", false),
                    ReadStatus::Read => ("Read", true),
                    ReadStatus::Archived => ("Archived", true),
                };
                ChromeReadLaterRowModel {
                    id: e.id,
                    title: e.title.clone(),
                    meta: format!("{}  ·  {}", extract_host(&e.url), format_unix_date(e.saved_at)),
                    status: status.to_owned(),
                    read,
                }
            })
            .collect();
        ChromeReadLaterModel { open: self.visible, total: self.entries.len(), rows }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Extract host from a URL string (best-effort ASCII).
fn extract_host(url: &str) -> String {
    let after_scheme = url.find("://").map(|i| &url[i + 3..]).unwrap_or(url);
    let host = after_scheme.split('/').next().unwrap_or(after_scheme);
    let host = host.split(':').next().unwrap_or(host);
    host.strip_prefix("www.").unwrap_or(host).to_owned()
}

/// Format a Unix timestamp (seconds) as "YYYY-MM-DD".
fn format_unix_date(unix_secs: i64) -> String {
    if unix_secs < 0 {
        return "–".to_owned();
    }
    let days = (unix_secs / 86400) as u64;
    // Gregorian calendar algorithm (days since Unix epoch → year/month/day).
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

/// Extract the page title from raw HTML bytes.
///
/// Searches for `<title>…</title>` case-insensitively and returns the trimmed
/// content, or an empty string if not found.
pub fn extract_title_from_html(html: &[u8]) -> String {
    let s = match std::str::from_utf8(html) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };
    let lower = s.to_ascii_lowercase();
    let start = lower.find("<title").unwrap_or(usize::MAX);
    if start == usize::MAX {
        return String::new();
    }
    let Some(gt) = lower[start..].find('>') else { return String::new() };
    let content_start = start + gt + 1;
    let Some(end) = lower[content_start..].find("</title") else { return String::new() };
    s[content_start..content_start + end].trim().to_owned()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;



    fn entry(id: i64, status: ReadStatus) -> ReadLaterEntry {
        ReadLaterEntry {
            id,
            url: format!("https://www.example.com/{id}"),
            title: format!("Page {id}"),
            saved_at: 1_705_276_800,
            status,
            html_snapshot: Vec::new(),
            text: String::new(),
            last_accessed: None,
            tags: Vec::new(),
        }
    }

    #[test]
    fn panel_toggle_resets_scroll() {
        let mut panel = ReadLaterPanel::new();
        assert!(!panel.visible);
        panel.toggle();
        assert!(panel.visible);
        panel.scroll_row = 3;
        panel.toggle();
        panel.toggle();
        assert_eq!(panel.scroll_row, 0);
    }

    #[test]
    fn chrome_model_windows_rows_and_maps_status() {
        let mut panel = ReadLaterPanel::new();
        panel.visible = true;
        panel.refresh((0..10).map(|i| entry(i, if i == 3 { ReadStatus::Read } else { ReadStatus::Unread })).collect());
        let m = panel.chrome_model();
        assert!(m.open);
        assert_eq!(m.total, 10);
        assert_eq!(m.rows.len(), VISIBLE_ROWS);
        assert_eq!(m.rows[0].meta, "example.com  ·  2024-01-15");
        assert!(m.rows[3].read);
        assert_eq!(m.rows[3].status, "Read");
        panel.scroll_down();
        panel.scroll_down();
        panel.scroll_down();
        assert_eq!(panel.scroll_row, 2, "clamped to max_scroll");
        assert_eq!(panel.chrome_model().rows[0].id, 2);
        panel.scroll_up();
        assert_eq!(panel.scroll_row, 1);
    }

    #[test]
    fn refresh_clamps_scroll_after_delete() {
        let mut panel = ReadLaterPanel::new();
        panel.refresh((0..10).map(|i| entry(i, ReadStatus::Unread)).collect());
        panel.scroll_row = 2;
        panel.refresh((0..4).map(|i| entry(i, ReadStatus::Unread)).collect());
        assert_eq!(panel.scroll_row, 0);
    }
}
