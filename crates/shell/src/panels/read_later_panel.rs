//! §12.3 Read-later panel (Ctrl+Shift+R).
//!
//! Floating overlay that lists saved pages (newest-first). Each row shows:
//! title + host + save date + unread/read badge; clicking opens the offline
//! HTML snapshot; pressing × deletes the entry.
//!
//! State lives on `Lumen`; this module only holds the open/scroll state and
//! provides [`hit_test`] + [`build_panel`].

use lumen_core::geom::Rect;
use lumen_knowledge::{ReadLaterEntry, ReadStatus};
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

// ── Geometry ─────────────────────────────────────────────────────────────────

/// Panel width in CSS px (exported so main.rs can compute the hit rect).
pub const PANEL_W: f32 = 420.0;

/// Total panel height in CSS px.
const PANEL_H: f32 = 456.0;

/// Header strip height.
const HEADER_H: f32 = 32.0;

/// Height of a single entry row (title line + meta line).
const ROW_H: f32 = 52.0;

/// Outer horizontal pad inside each row.
const PAD: f32 = 10.0;

/// Width of the trailing "×" delete hit zone.
const DELETE_W: f32 = 26.0;

/// Width estimate for one character in the title font (~7 px @ 12 pt).
const CHAR_W: f32 = 7.0;

// ── Colours ──────────────────────────────────────────────────────────────────

const PANEL_BG: Color = Color { r: 20, g: 20, b: 27, a: 252 };
const PANEL_BORDER: Color = Color { r: 55, g: 55, b: 68, a: 255 };
const HEADER_BG: Color = Color { r: 28, g: 28, b: 36, a: 255 };
const HEADER_TEXT: Color = Color { r: 200, g: 200, b: 216, a: 255 };
const CLOSE_TEXT: Color = Color { r: 180, g: 90, b: 90, a: 255 };
const ROW_EVEN: Color = Color { r: 24, g: 24, b: 31, a: 255 };
const ROW_ODD: Color = Color { r: 28, g: 28, b: 36, a: 255 };
const SEPARATOR: Color = Color { r: 38, g: 38, b: 48, a: 255 };
const TITLE_TEXT: Color = Color { r: 220, g: 220, b: 230, a: 255 };
const META_TEXT: Color = Color { r: 120, g: 120, b: 134, a: 255 };
const DELETE_TEXT: Color = Color { r: 170, g: 80, b: 80, a: 255 };
const BADGE_UNREAD: Color = Color { r: 55, g: 115, b: 210, a: 220 };
const BADGE_READ: Color = Color { r: 55, g: 150, b: 75, a: 200 };
const BADGE_TEXT: Color = Color { r: 240, g: 240, b: 250, a: 255 };
const EMPTY_TEXT: Color = Color { r: 100, g: 100, b: 115, a: 255 };

// ── State ────────────────────────────────────────────────────────────────────

/// Read-later panel state.
#[derive(Debug, Default)]
pub struct ReadLaterPanel {
    /// Whether the panel is currently visible.
    pub visible: bool,
    /// Vertical scroll offset in CSS px.
    pub scroll_offset: f32,
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
            self.scroll_offset = 0.0;
        }
    }

    /// Replace the cached entry list (call after save/delete or on open).
    pub fn refresh(&mut self, entries: Vec<ReadLaterEntry>) {
        self.entries = entries;
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = (self.scroll_offset - ROW_H).max(0.0);
    }

    pub fn scroll_down(&mut self, max_scroll: f32) {
        self.scroll_offset = (self.scroll_offset + ROW_H).min(max_scroll);
    }

    /// Maximum scroll offset for the current entry count.
    pub fn max_scroll(&self) -> f32 {
        let total_h = self.entries.len() as f32 * ROW_H;
        let body_h = PANEL_H - HEADER_H;
        (total_h - body_h).max(0.0)
    }
}

// ── Hit testing ──────────────────────────────────────────────────────────────

/// Result of a click inside or near the panel.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadLaterHit {
    /// Click on the "×" close button in the header.
    Close,
    /// Click on an entry row body → open offline snapshot.
    Open(i64),
    /// Click on the "×" delete button of an entry.
    Delete(i64),
    /// Click lands inside the panel (but not on a specific control).
    Inside,
    /// Click outside the panel.
    Outside,
}

/// Classify a click at `(mx, my)` (window-space CSS px).
///
/// `(px, py)` is the top-left corner of the panel in window-space coordinates.
pub fn hit_test(
    mx: f32,
    my: f32,
    px: f32,
    py: f32,
    entries: &[ReadLaterEntry],
    scroll_offset: f32,
) -> ReadLaterHit {
    if mx < px || mx > px + PANEL_W || my < py || my > py + PANEL_H {
        return ReadLaterHit::Outside;
    }
    // Header.
    if my < py + HEADER_H {
        let close_x = px + PANEL_W - 28.0;
        if mx >= close_x {
            return ReadLaterHit::Close;
        }
        return ReadLaterHit::Inside;
    }
    // Body row.
    let body_y = my - (py + HEADER_H) + scroll_offset;
    let row_idx = (body_y / ROW_H) as usize;
    if row_idx < entries.len() {
        let entry_id = entries[row_idx].id;
        if mx >= px + PANEL_W - DELETE_W - PAD {
            return ReadLaterHit::Delete(entry_id);
        }
        return ReadLaterHit::Open(entry_id);
    }
    ReadLaterHit::Inside
}

// ── Rendering ────────────────────────────────────────────────────────────────

/// Build the panel display list.
///
/// `(win_w, tab_bar_h)` — window width and tab bar height in CSS px.
pub fn build_panel(
    panel: &ReadLaterPanel,
    win_w: f32,
    tab_bar_h: f32,
) -> DisplayList {
    let mut dl: DisplayList = Vec::new();
    if !panel.visible {
        return dl;
    }

    let px = win_w - PANEL_W - 4.0;
    let py = tab_bar_h + 4.0;

    // ── Background + border ──────────────────────────────────────────────────
    let border_radii = uniform_radii(6.0);
    dl.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, PANEL_H),
        radii: border_radii,
        color: PANEL_BORDER,
    });
    dl.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px + 1.0, py + 1.0, PANEL_W - 2.0, PANEL_H - 2.0),
        radii: uniform_radii(5.0),
        color: PANEL_BG,
    });

    // ── Header ───────────────────────────────────────────────────────────────
    dl.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, HEADER_H),
        radii: CornerRadii { tl: 5.0, tl_y: 5.0, tr: 5.0, tr_y: 5.0, bl: 0.0, bl_y: 0.0, br: 0.0, br_y: 0.0 },
        color: HEADER_BG,
    });
    let count = panel.entries.len();
    let header_label = if count == 0 {
        "Read Later".to_owned()
    } else {
        format!("Read Later ({count})")
    };
    dl.push(make_text(header_label, px + PAD, py + 9.0, 200.0, 13.0, FontWeight::BOLD, HEADER_TEXT));
    // Close button.
    dl.push(make_text("×".to_owned(), px + PANEL_W - 22.0, py + 8.0, 20.0, 15.0, FontWeight::BOLD, CLOSE_TEXT));
    // Separator.
    dl.push(DisplayCommand::FillRect {
        rect: Rect::new(px, py + HEADER_H - 1.0, PANEL_W, 1.0),
        color: SEPARATOR,
    });

    // ── Body (clipped, scrolled) ─────────────────────────────────────────────
    let body_y = py + HEADER_H;
    let body_h = PANEL_H - HEADER_H;
    dl.push(DisplayCommand::PushClipRect {
        rect: Rect::new(px, body_y, PANEL_W, body_h),
    });

    if panel.entries.is_empty() {
        dl.push(make_text(
            "No saved pages yet. Use @read-later <url> to save.".to_owned(),
            px + PAD,
            body_y + 24.0,
            PANEL_W - 2.0 * PAD,
            12.0,
            FontWeight::NORMAL,
            EMPTY_TEXT,
        ));
    } else {
        let scroll = panel.scroll_offset;
        // Available title area width (minus badge + delete button zone).
        let title_area_w = PANEL_W - 2.0 * PAD - DELETE_W - 56.0;
        let max_title_chars = (title_area_w / CHAR_W) as usize;

        for (i, entry) in panel.entries.iter().enumerate() {
            let row_y = body_y + i as f32 * ROW_H - scroll;
            if row_y + ROW_H < body_y || row_y > body_y + body_h {
                continue; // culled by clip
            }

            // Row background (alternating).
            let row_bg = if i % 2 == 0 { ROW_EVEN } else { ROW_ODD };
            dl.push(DisplayCommand::FillRect {
                rect: Rect::new(px, row_y, PANEL_W, ROW_H),
                color: row_bg,
            });

            // Title.
            let title = truncate_str(&entry.title, max_title_chars);
            dl.push(make_text(title, px + PAD, row_y + 9.0, title_area_w, 12.0, FontWeight::NORMAL, TITLE_TEXT));

            // Meta: host + date.
            let host = extract_host(&entry.url);
            let date = format_unix_date(entry.saved_at);
            let meta = format!("{host}  ·  {date}");
            dl.push(make_text(meta, px + PAD, row_y + 28.0, title_area_w, 11.0, FontWeight::NORMAL, META_TEXT));

            // Status badge.
            let (badge_color, badge_text) = match entry.status {
                ReadStatus::Unread => (BADGE_UNREAD, "Unread"),
                ReadStatus::Read => (BADGE_READ, "Read"),
                ReadStatus::Archived => (BADGE_READ, "Archived"),
            };
            let badge_x = px + PANEL_W - DELETE_W - PAD - 54.0;
            dl.push(DisplayCommand::FillRoundedRect {
                rect: Rect::new(badge_x, row_y + 10.0, 46.0, 14.0),
                radii: uniform_radii(3.0),
                color: badge_color,
            });
            dl.push(make_text(badge_text.to_owned(), badge_x + 4.0, row_y + 12.0, 40.0, 9.0, FontWeight::BOLD, BADGE_TEXT));

            // Delete "×" button.
            dl.push(make_text("×".to_owned(), px + PANEL_W - DELETE_W + 2.0, row_y + 18.0, 20.0, 14.0, FontWeight::BOLD, DELETE_TEXT));

            // Row separator.
            dl.push(DisplayCommand::FillRect {
                rect: Rect::new(px, row_y + ROW_H - 1.0, PANEL_W, 1.0),
                color: SEPARATOR,
            });
        }
    }

    dl.push(DisplayCommand::PopClip);
    dl
}

// ── Helpers ──────────────────────────────────────────────────────────────────

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
    }
}

fn uniform_radii(r: f32) -> CornerRadii {
    CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, bl: r, bl_y: r, br: r, br_y: r }
}

/// Truncate a string to `max_chars` Unicode scalar values, adding "…" if cut.
fn truncate_str(s: &str, max_chars: usize) -> String {
    let mut result = String::with_capacity(max_chars + 3);
    for (i, c) in s.chars().enumerate() {
        if i == max_chars {
            result.push('…');
            return result;
        }
        result.push(c);
    }
    result
}

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

    #[test]
    fn extract_host_basic() {
        assert_eq!(extract_host("https://www.example.com/path"), "example.com");
        assert_eq!(extract_host("http://sub.domain.org/"), "sub.domain.org");
        assert_eq!(extract_host("https://example.com:8080/foo"), "example.com");
        assert_eq!(extract_host("https://example.com"), "example.com");
    }

    #[test]
    fn format_date_epoch() {
        assert_eq!(format_unix_date(0), "1970-01-01");
    }

    #[test]
    fn format_date_known() {
        // 2024-01-15: 1_705_276_800 seconds after epoch
        assert_eq!(format_unix_date(1_705_276_800), "2024-01-15");
    }

    #[test]
    fn format_date_2026() {
        // 2026-06-03: 1_780_444_800 seconds after Unix epoch
        assert_eq!(format_unix_date(1_780_444_800), "2026-06-03");
    }

    #[test]
    fn format_date_negative_returns_dash() {
        assert_eq!(format_unix_date(-1), "–");
    }

    #[test]
    fn truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn truncate_str_long() {
        assert_eq!(truncate_str("hello world", 5), "hello…");
    }

    #[test]
    fn extract_title_basic() {
        assert_eq!(
            extract_title_from_html(b"<html><head><title>My Page</title></head></html>"),
            "My Page"
        );
    }

    #[test]
    fn extract_title_case_insensitive() {
        assert_eq!(
            extract_title_from_html(b"<TITLE> Whitespace  </TITLE>"),
            "Whitespace"
        );
    }

    #[test]
    fn extract_title_missing() {
        assert_eq!(extract_title_from_html(b"<html>no title</html>"), "");
    }

    #[test]
    fn hit_test_outside() {
        let entries: Vec<ReadLaterEntry> = Vec::new();
        assert_eq!(hit_test(0.0, 0.0, 100.0, 100.0, &entries, 0.0), ReadLaterHit::Outside);
    }

    #[test]
    fn hit_test_close_button() {
        let entries: Vec<ReadLaterEntry> = Vec::new();
        let px = 50.0;
        let py = 50.0;
        let hit = hit_test(px + PANEL_W - 5.0, py + 10.0, px, py, &entries, 0.0);
        assert_eq!(hit, ReadLaterHit::Close);
    }

    #[test]
    fn hit_test_inside_empty_body() {
        let entries: Vec<ReadLaterEntry> = Vec::new();
        let px = 50.0;
        let py = 50.0;
        let hit = hit_test(px + 10.0, py + HEADER_H + 5.0, px, py, &entries, 0.0);
        assert_eq!(hit, ReadLaterHit::Inside);
    }

    #[test]
    fn panel_toggle() {
        let mut panel = ReadLaterPanel::new();
        assert!(!panel.visible);
        panel.toggle();
        assert!(panel.visible);
        panel.toggle();
        assert!(!panel.visible);
    }

    #[test]
    fn panel_max_scroll_empty() {
        let panel = ReadLaterPanel::new();
        assert_eq!(panel.max_scroll(), 0.0);
    }
}
