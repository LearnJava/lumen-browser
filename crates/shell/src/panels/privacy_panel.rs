//! Privacy network panel (V5).
//!
//! A privacy-focused, real-time request monitor. Unlike the DevTools
//! [network panel](crate::devtools::network_panel) (a flat method/status/timing
//! list for debugging), this panel answers a single question: *what is this page
//! doing to my privacy right now?* It shows, newest-first, every request the
//! engine made — the **tracker domain**, whether it was **blocked or allowed**,
//! and, for blocked requests, the **matched filter rule**. A summary header
//! tallies blocked vs. allowed so the protection level is visible at a glance.
//!
//! # Data source
//!
//! The panel shares the same [`NetworkLog`] `Arc` as the DevTools network panel
//! (fed from the engine's `EventSink` via `NetworkLogSink`). No second sink is
//! added to the chain; the privacy view is purely a different *presentation* of
//! the same recorded [`NetworkEntry`] stream, pulled on each redraw via
//! [`PrivacyPanel::refresh`].
//!
//! # Layout
//!
//! Right-docked overlay, [`PANEL_WIDTH`] CSS px wide, spanning from the bottom of
//! the tab bar to the bottom of the window. It is a pure overlay — it does not
//! reflow the page (clicks inside are swallowed; clicks outside pass through).
//! Toggle with `Ctrl+Shift+Y` (priva**Y**; the other privacy shortcuts —
//! `Ctrl+Shift+S` shields, `Ctrl+Shift+K` cookie banners — are taken).

use std::sync::{Arc, Mutex};

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

use crate::devtools::network_panel::{NetworkEntry, NetworkLog};

// ── Layout constants ────────────────────────────────────────────────────────────

/// Width of the right-docked panel in CSS px.
pub const PANEL_WIDTH: f32 = 320.0;
/// Height of the title header bar.
const HEADER_H: f32 = 34.0;
/// Height of the blocked/allowed summary bar below the header.
const SUMMARY_H: f32 = 30.0;
/// Height of a single request row.
const ROW_H: f32 = 38.0;
/// Body font size.
const FONT_SIZE: f32 = 12.0;
/// Smaller font size for the secondary (filter / status) line of a row.
const SUB_FONT_SIZE: f32 = 10.5;
/// Horizontal padding inside the panel.
const H_PAD: f32 = 12.0;
/// Diameter of the blocked/allowed status dot.
const DOT_SIZE: f32 = 8.0;
/// Left x of text (after the status dot).
const TEXT_X: f32 = H_PAD + DOT_SIZE + 8.0;
/// Width of the close `×` hit box.
const CLOSE_SIZE: f32 = 22.0;

// ── Colours ─────────────────────────────────────────────────────────────────────

const BG: Color = Color { r: 22, g: 23, b: 28, a: 245 };
const HEADER_BG: Color = Color { r: 30, g: 31, b: 38, a: 255 };
const SUMMARY_BG: Color = Color { r: 26, g: 27, b: 33, a: 255 };
const BORDER: Color = Color { r: 52, g: 54, b: 62, a: 255 };
const ROW_ALT_BG: Color = Color { r: 27, g: 28, b: 34, a: 255 };
const FG_TITLE: Color = Color { r: 226, g: 228, b: 234, a: 255 };
const FG_DOMAIN: Color = Color { r: 214, g: 216, b: 222, a: 255 };
const FG_DIM: Color = Color { r: 132, g: 134, b: 142, a: 255 };
const FG_CLOSE: Color = Color { r: 170, g: 172, b: 180, a: 255 };
/// Blocked dot + matched-filter text (red).
const BLOCKED: Color = Color { r: 237, g: 90, b: 90, a: 255 };
/// Allowed dot (green).
const ALLOWED: Color = Color { r: 90, g: 200, b: 120, a: 255 };
/// Pending (in-flight) dot (grey).
const PENDING: Color = Color { r: 150, g: 152, b: 160, a: 255 };

/// Height in CSS px of the scrollable request-list area, given the full window
/// height `win_h` and the tab strip height `tab_bar_h`. Subtracts the panel
/// chrome (header + summary bar). Used by the wheel handler to clamp scrolling.
pub fn list_body_height(win_h: f32, tab_bar_h: f32) -> f32 {
    (win_h - tab_bar_h - HEADER_H - SUMMARY_H).max(0.0)
}

// ── Panel state ───────────────────────────────────────────────────────────────

/// Privacy network panel (V5). Holds a snapshot of the shared [`NetworkLog`] and
/// renders a right-docked, newest-first list of requests with privacy-relevant
/// columns (tracker domain, blocked/allowed, matched filter).
pub struct PrivacyPanel {
    /// Snapshot of request entries, pulled from [`NetworkLog`] via [`refresh`].
    /// Stored oldest-first (as in the log); rendered newest-first.
    ///
    /// [`refresh`]: PrivacyPanel::refresh
    entries: Vec<NetworkEntry>,
    /// Number of rows scrolled past from the top (newest). 0 = show newest.
    pub scroll_offset: usize,
    /// Whether the panel is currently shown.
    pub visible: bool,
    /// Shared log produced by `NetworkLogSink` (same `Arc` the network panel holds).
    log: Arc<Mutex<NetworkLog>>,
}

impl PrivacyPanel {
    /// Create a new hidden panel backed by the given shared `log`.
    pub fn new(log: Arc<Mutex<NetworkLog>>) -> Self {
        Self {
            entries: Vec::new(),
            scroll_offset: 0,
            visible: false,
            log,
        }
    }

    /// Toggle panel visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Pull the latest entries from the shared [`NetworkLog`] into the snapshot.
    /// Call before building the display list on each redraw.
    pub fn refresh(&mut self) {
        if let Ok(guard) = self.log.lock() {
            self.entries = guard.entries.clone();
        }
    }

    /// Clear the shared log (call on every top-level navigation). The network
    /// panel clears the same log, so call only one of them per navigation.
    #[allow(dead_code)]
    pub fn clear_log(&mut self) {
        if let Ok(mut guard) = self.log.lock() {
            guard.clear();
        }
        self.entries.clear();
        self.scroll_offset = 0;
    }

    /// Number of entries in the current snapshot.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when the current snapshot has no entries.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of blocked requests in the current snapshot.
    pub fn blocked_count(&self) -> usize {
        self.entries.iter().filter(|e| e.blocked).count()
    }

    /// Number of allowed (not blocked) requests in the current snapshot —
    /// includes both completed and still-pending requests that were sent.
    pub fn allowed_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.blocked).count()
    }

    /// Maximum scroll offset given how many rows fit in `body_h` CSS px.
    fn max_scroll(&self, body_h: f32) -> usize {
        let visible = (body_h / ROW_H).floor().max(1.0) as usize;
        self.entries.len().saturating_sub(visible)
    }

    /// Scroll towards older requests by `n` rows.
    pub fn scroll_down(&mut self, n: usize, body_h: f32) {
        self.scroll_offset = (self.scroll_offset + n).min(self.max_scroll(body_h));
    }

    /// Scroll towards newer requests by `n` rows.
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }
}

// ── Hit testing ───────────────────────────────────────────────────────────────

/// Result of a click on (or near) the privacy panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyHit {
    /// The close `×` button was clicked.
    Close,
    /// A click landed inside the panel body — swallow it (do not pass through).
    Inside,
    /// The click was outside the panel — let it reach the page.
    Outside,
}

/// Classify a click at `(x, y)` CSS px. `tab_bar_h` is the tab strip height;
/// `(win_w, win_h)` are the window dimensions in CSS px.
pub fn hit_test(
    panel: &PrivacyPanel,
    x: f32,
    y: f32,
    win_w: f32,
    win_h: f32,
    tab_bar_h: f32,
) -> PrivacyHit {
    if !panel.visible {
        return PrivacyHit::Outside;
    }
    let px = win_w - PANEL_WIDTH;
    if x < px || y < tab_bar_h || y > win_h {
        return PrivacyHit::Outside;
    }
    // Close button: top-right of the header.
    let close_x = win_w - CLOSE_SIZE - 6.0;
    if x >= close_x && y >= tab_bar_h && y < tab_bar_h + HEADER_H {
        return PrivacyHit::Close;
    }
    PrivacyHit::Inside
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the right-docked privacy panel overlay.
///
/// Returns an empty `DisplayList` when `panel.visible` is `false`. `(win_w,
/// win_h)` are the window dimensions in CSS px; `tab_bar_h` is the tab strip
/// height (the panel starts directly below it).
pub fn build_privacy_panel(
    panel: &PrivacyPanel,
    (win_w, win_h): (u32, u32),
    tab_bar_h: f32,
) -> DisplayList {
    if !panel.visible {
        return Vec::new();
    }

    let win_w = win_w as f32;
    let win_h = win_h as f32;
    let px = win_w - PANEL_WIDTH;
    let panel_h = (win_h - tab_bar_h).max(0.0);

    // Panel background + left border.
    let mut out: DisplayList = vec![DisplayCommand::FillRect {
        rect: Rect::new(px, tab_bar_h, PANEL_WIDTH, panel_h),
        color: BG,
    }];
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px, tab_bar_h, 1.0, panel_h),
        color: BORDER,
    });

    // Header bar + title.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px, tab_bar_h, PANEL_WIDTH, HEADER_H),
        color: HEADER_BG,
    });
    out.push(make_text(
        "Privacy".to_string(),
        px + H_PAD,
        tab_bar_h + (HEADER_H - FONT_SIZE) / 2.0,
        160.0,
        FONT_SIZE,
        FontWeight::BOLD,
        FG_TITLE,
    ));
    // Close button.
    out.push(make_text(
        "×".to_string(),
        win_w - CLOSE_SIZE - 2.0,
        tab_bar_h + (HEADER_H - FONT_SIZE) / 2.0 - 1.0,
        CLOSE_SIZE,
        FONT_SIZE + 4.0,
        FontWeight::NORMAL,
        FG_CLOSE,
    ));

    // Summary bar: blocked / allowed tallies.
    let summary_y = tab_bar_h + HEADER_H;
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px, summary_y, PANEL_WIDTH, SUMMARY_H),
        color: SUMMARY_BG,
    });
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px, summary_y + SUMMARY_H - 1.0, PANEL_WIDTH, 1.0),
        color: BORDER,
    });
    let blocked = panel.blocked_count();
    let allowed = panel.allowed_count();
    let summary_text_y = summary_y + (SUMMARY_H - FONT_SIZE) / 2.0;
    out.push(make_text(
        format!("{blocked} blocked"),
        px + H_PAD,
        summary_text_y,
        130.0,
        FONT_SIZE,
        FontWeight::BOLD,
        BLOCKED,
    ));
    out.push(make_text(
        format!("{allowed} allowed"),
        px + PANEL_WIDTH / 2.0,
        summary_text_y,
        130.0,
        FONT_SIZE,
        FontWeight::BOLD,
        ALLOWED,
    ));

    // Request list body.
    let body_y = summary_y + SUMMARY_H;
    let body_h = (win_h - body_y).max(0.0);

    if panel.entries.is_empty() {
        out.push(make_text(
            "No requests yet".to_string(),
            px + H_PAD,
            body_y + 14.0,
            PANEL_WIDTH - H_PAD * 2.0,
            FONT_SIZE,
            FontWeight::NORMAL,
            FG_DIM,
        ));
        return out;
    }

    // Clip the scrollable list to the body region.
    out.push(DisplayCommand::PushClipRect {
        rect: Rect::new(px, body_y, PANEL_WIDTH, body_h),
    });

    let visible_rows = (body_h / ROW_H).ceil().max(1.0) as usize;
    // Newest-first: walk the snapshot in reverse, skipping `scroll_offset`.
    let total = panel.entries.len();
    for (i, entry) in panel
        .entries
        .iter()
        .rev()
        .skip(panel.scroll_offset)
        .take(visible_rows)
        .enumerate()
    {
        let row_y = body_y + i as f32 * ROW_H;

        // Zebra striping for readability.
        if i % 2 == 1 {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(px, row_y, PANEL_WIDTH, ROW_H),
                color: ROW_ALT_BG,
            });
        }

        // Status dot.
        let dot_y = row_y + (ROW_H - DOT_SIZE) / 2.0 - 5.0;
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(px + H_PAD, dot_y, DOT_SIZE, DOT_SIZE),
            color: dot_color(entry),
            radii: uniform_radii(DOT_SIZE / 2.0),
        });

        // Tracker domain (primary line).
        out.push(make_text(
            host_of(&entry.url),
            px + TEXT_X,
            row_y + 6.0,
            PANEL_WIDTH - TEXT_X - H_PAD,
            FONT_SIZE,
            FontWeight::NORMAL,
            FG_DOMAIN,
        ));

        // Secondary line: matched filter (blocked) or status (allowed).
        let (sub, sub_color) = sub_line(entry);
        out.push(make_text(
            sub,
            px + TEXT_X,
            row_y + 6.0 + FONT_SIZE + 4.0,
            PANEL_WIDTH - TEXT_X - H_PAD,
            SUB_FONT_SIZE,
            FontWeight::NORMAL,
            sub_color,
        ));
    }

    out.push(DisplayCommand::PopClip);

    // Scroll indicator if the list overflows.
    if total > visible_rows {
        let shown_top = total.saturating_sub(panel.scroll_offset);
        out.push(make_text(
            format!("{shown_top}/{total}"),
            win_w - 64.0,
            summary_text_y,
            56.0,
            SUB_FONT_SIZE,
            FontWeight::NORMAL,
            FG_DIM,
        ));
    }

    out
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Status dot colour: red blocked, grey pending, green allowed.
fn dot_color(entry: &NetworkEntry) -> Color {
    if entry.blocked {
        BLOCKED
    } else if entry.status.is_none() {
        PENDING
    } else {
        ALLOWED
    }
}

/// Secondary row line: the matched filter for blocked requests, otherwise the
/// HTTP status (or `"pending…"`), paired with its colour.
fn sub_line(entry: &NetworkEntry) -> (String, Color) {
    if entry.blocked {
        let rule = entry.reason.as_deref().unwrap_or("blocked");
        (format!("blocked · {rule}"), BLOCKED)
    } else if let Some(code) = entry.status {
        (format!("allowed · {code}"), FG_DIM)
    } else {
        ("pending…".to_string(), FG_DIM)
    }
}

/// Extract the host (tracker domain) from a URL string, dropping scheme,
/// userinfo, port and path. Falls back to the raw string when it has no
/// recognisable authority. Purely lexical — no allocation of intermediate URLs.
fn host_of(url: &str) -> String {
    // Strip scheme.
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    // Authority ends at the first '/', '?' or '#'.
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    // Drop userinfo (before '@') and port (after ':').
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host.split_once(':').map_or(host, |(h, _)| h);
    if host.is_empty() {
        url.to_string()
    } else {
        host.to_string()
    }
}

/// Uniform corner radii of `r` CSS px for all four corners.
fn uniform_radii(r: f32) -> CornerRadii {
    CornerRadii {
        tl: r, tl_y: r,
        tr: r, tr_y: r,
        br: r, br_y: r,
        bl: r, bl_y: r,
    }
}

#[allow(clippy::too_many_arguments)]
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_log() -> Arc<Mutex<NetworkLog>> {
        Arc::new(Mutex::new(NetworkLog::default()))
    }

    fn seed(log: &Arc<Mutex<NetworkLog>>) {
        let mut g = log.lock().unwrap();
        g.record_started("GET", "https://example.com/index.html");
        g.record_completed("https://example.com/index.html", 200);
        g.record_blocked("https://ads.tracker.com/pixel.gif", "||tracker.com^");
        g.record_started("GET", "https://cdn.example.com/app.js");
    }

    // ── State ────────────────────────────────────────────────────────────────

    #[test]
    fn new_panel_hidden_empty() {
        let p = PrivacyPanel::new(make_log());
        assert!(!p.visible);
        assert!(p.is_empty());
    }

    #[test]
    fn toggle_visibility() {
        let mut p = PrivacyPanel::new(make_log());
        p.toggle();
        assert!(p.visible);
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn refresh_pulls_snapshot() {
        let log = make_log();
        seed(&log);
        let mut p = PrivacyPanel::new(log);
        p.refresh();
        // started+completed collapse into one entry; blocked + second started
        // add two more → 3 total.
        assert_eq!(p.len(), 3);
    }

    #[test]
    fn blocked_and_allowed_counts() {
        let log = make_log();
        seed(&log);
        let mut p = PrivacyPanel::new(log);
        p.refresh();
        assert_eq!(p.blocked_count(), 1);
        // completed (1) + pending started (1) = 2 allowed/sent.
        assert_eq!(p.allowed_count(), 2);
    }

    #[test]
    fn clear_log_resets() {
        let log = make_log();
        seed(&log);
        let mut p = PrivacyPanel::new(log);
        p.refresh();
        p.scroll_offset = 1;
        p.clear_log();
        assert!(p.is_empty());
        assert_eq!(p.scroll_offset, 0);
    }

    #[test]
    fn scroll_clamps_to_max() {
        let log = make_log();
        {
            let mut g = log.lock().unwrap();
            for i in 0..30 {
                g.record_started("GET", &format!("https://a.com/{i}"));
            }
        }
        let mut p = PrivacyPanel::new(log);
        p.refresh();
        // body_h = 4 rows worth → max_scroll = 30 - 4 = 26.
        let body_h = ROW_H * 4.0;
        p.scroll_down(1000, body_h);
        assert_eq!(p.scroll_offset, 26);
        p.scroll_up(5);
        assert_eq!(p.scroll_offset, 21);
        p.scroll_up(1000);
        assert_eq!(p.scroll_offset, 0);
    }

    // ── host_of ──────────────────────────────────────────────────────────────

    #[test]
    fn host_of_strips_scheme_and_path() {
        assert_eq!(host_of("https://ads.tracker.com/pixel.gif?id=5"), "ads.tracker.com");
        assert_eq!(host_of("http://example.com"), "example.com");
        assert_eq!(host_of("https://example.com:8443/x"), "example.com");
        assert_eq!(host_of("https://user:pw@cdn.example.com/a"), "cdn.example.com");
        assert_eq!(host_of("https://host.com#frag"), "host.com");
    }

    #[test]
    fn host_of_falls_back_to_raw() {
        assert_eq!(host_of("not a url"), "not a url");
    }

    // ── sub_line / dot_color ─────────────────────────────────────────────────

    #[test]
    fn sub_line_blocked_shows_filter() {
        let log = make_log();
        log.lock().unwrap().record_blocked("https://a.com/x", "||evil^");
        let mut p = PrivacyPanel::new(log);
        p.refresh();
        let (text, color) = sub_line(&p.entries[0]);
        assert!(text.contains("||evil^"));
        assert_eq!(color, BLOCKED);
    }

    #[test]
    fn dot_color_buckets() {
        let log = make_log();
        seed(&log);
        let mut p = PrivacyPanel::new(log);
        p.refresh();
        // 0: completed → allowed, 1: blocked, 2: pending.
        assert_eq!(dot_color(&p.entries[0]), ALLOWED);
        assert_eq!(dot_color(&p.entries[1]), BLOCKED);
        assert_eq!(dot_color(&p.entries[2]), PENDING);
    }

    // ── hit_test ─────────────────────────────────────────────────────────────

    #[test]
    fn hit_test_hidden_is_outside() {
        let p = PrivacyPanel::new(make_log());
        assert_eq!(hit_test(&p, 1000.0, 100.0, 1280.0, 800.0, 36.0), PrivacyHit::Outside);
    }

    #[test]
    fn hit_test_inside_and_close() {
        let mut p = PrivacyPanel::new(make_log());
        p.visible = true;
        let win_w = 1280.0;
        // A point well inside the body.
        assert_eq!(
            hit_test(&p, win_w - 100.0, 300.0, win_w, 800.0, 36.0),
            PrivacyHit::Inside
        );
        // The close button area (top-right of header).
        assert_eq!(
            hit_test(&p, win_w - 10.0, 36.0 + 10.0, win_w, 800.0, 36.0),
            PrivacyHit::Close
        );
        // Left of the panel → passes through.
        assert_eq!(
            hit_test(&p, win_w - PANEL_WIDTH - 50.0, 300.0, win_w, 800.0, 36.0),
            PrivacyHit::Outside
        );
        // Above the tab bar → outside.
        assert_eq!(
            hit_test(&p, win_w - 100.0, 10.0, win_w, 800.0, 36.0),
            PrivacyHit::Outside
        );
    }

    // ── Rendering ────────────────────────────────────────────────────────────

    #[test]
    fn build_hidden_returns_empty() {
        let p = PrivacyPanel::new(make_log());
        assert!(build_privacy_panel(&p, (1280, 800), 36.0).is_empty());
    }

    #[test]
    fn build_visible_has_title_and_summary() {
        let log = make_log();
        seed(&log);
        let mut p = PrivacyPanel::new(log);
        p.visible = true;
        p.refresh();
        let dl = build_privacy_panel(&p, (1280, 800), 36.0);
        let has_title = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "Privacy")
        });
        let has_blocked = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "1 blocked")
        });
        assert!(has_title, "must show title");
        assert!(has_blocked, "must show blocked tally");
    }

    #[test]
    fn build_shows_tracker_domain_and_filter() {
        let log = make_log();
        seed(&log);
        let mut p = PrivacyPanel::new(log);
        p.visible = true;
        p.refresh();
        let dl = build_privacy_panel(&p, (1280, 800), 36.0);
        let has_domain = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "ads.tracker.com")
        });
        let has_filter = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("||tracker.com^"))
        });
        assert!(has_domain, "must show tracker domain");
        assert!(has_filter, "must show matched filter");
    }

    #[test]
    fn build_empty_shows_placeholder() {
        let mut p = PrivacyPanel::new(make_log());
        p.visible = true;
        let dl = build_privacy_panel(&p, (1280, 800), 36.0);
        let has_placeholder = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("No requests"))
        });
        assert!(has_placeholder);
    }
}
