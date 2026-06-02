//! DevTools network log panel (§7E.4).
//!
//! Captures HTTP request lifecycle events ([`Event::RequestStarted`],
//! [`Event::RequestCompleted`], [`Event::RequestBlocked`],
//! [`Event::RequestFailed`]) and renders a
//! scrollable bottom overlay showing one row per request: method, status,
//! timing and URL.  Toggle with `Ctrl+Shift+E` (mirrors Firefox's network
//! monitor shortcut; `F12` is taken by the JS console).
//!
//! # Architecture
//!
//! A shared [`NetworkLog`] (`Arc<Mutex<…>>`) is updated from the network thread
//! via [`NetworkLogSink`], an [`EventSink`] wrapper that forwards every event to
//! an inner sink and records request lifecycle events.  The shell holds the same
//! `Arc` inside [`NetworkPanel`] and pulls a fresh snapshot on each redraw via
//! [`NetworkPanel::refresh`].
//!
//! # Layout
//!
//! The panel is anchored to the bottom of the window, full width, up to
//! [`MAX_VISIBLE_LINES`] rows of [`LINE_H`] height each, plus a header bar.
//! Requests are displayed oldest-first (scroll_offset = 0 shows the tail).

use std::sync::{Arc, Mutex};
use std::time::Instant;

use lumen_core::event::Event;
use lumen_core::ext::EventSink;
use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{DisplayCommand, DisplayList};

// ── Colours ───────────────────────────────────────────────────────────────────

const BG: Color = Color { r: 24, g: 24, b: 28, a: 240 };
const HEADER_BG: Color = Color { r: 32, g: 33, b: 38, a: 255 };
const FG_URL: Color = Color { r: 210, g: 212, b: 218, a: 255 };
const FG_METHOD: Color = Color { r: 130, g: 180, b: 240, a: 255 };
const FG_DIM: Color = Color { r: 130, g: 132, b: 140, a: 255 };
const FG_TIME: Color = Color { r: 160, g: 162, b: 170, a: 255 };
/// 2xx success.
const STATUS_OK: Color = Color { r: 90, g: 200, b: 120, a: 255 };
/// 3xx redirect.
const STATUS_REDIRECT: Color = Color { r: 220, g: 190, b: 90, a: 255 };
/// 4xx / 5xx error, blocked, and failed requests.
const STATUS_ERROR: Color = Color { r: 237, g: 90, b: 90, a: 255 };
/// Pending (no status yet).
const STATUS_PENDING: Color = Color { r: 140, g: 142, b: 150, a: 255 };
const BLOCKED_BG: Color = Color { r: 45, g: 20, b: 20, a: 255 };
/// Row background for network-level failures (DNS/TCP/TLS/read errors).
const FAILED_BG: Color = Color { r: 40, g: 25, b: 10, a: 255 };

// ── Layout constants ──────────────────────────────────────────────────────────

const HEADER_H: f32 = 32.0;
const LINE_H: f32 = 20.0;
const FONT_SIZE: f32 = 12.0;
const H_PAD: f32 = 10.0;
/// X offset of the method column (CSS px from the panel's left edge).
const COL_METHOD: f32 = H_PAD;
/// X offset of the status column.
const COL_STATUS: f32 = 60.0;
/// X offset of the timing column.
const COL_TIME: f32 = 120.0;
/// X offset of the URL column.
const COL_URL: f32 = 185.0;
/// Maximum number of request rows visible without scrolling.
const MAX_VISIBLE_LINES: usize = 12;
/// Hard cap on stored entries (oldest are dropped when exceeded).
const MAX_STORED_ENTRIES: usize = 500;

// ── Network log ─────────────────────────────────────────────────────────────────

/// A single recorded HTTP request and its lifecycle state.
#[derive(Debug, Clone)]
pub struct NetworkEntry {
    /// HTTP method (currently always `"GET"` — the engine issues GET for
    /// navigations and subresources; the field is future-proofed for POST/etc.).
    pub method: String,
    /// Full request URL.
    pub url: String,
    /// Response status code once the request completes (`None` while pending).
    pub status: Option<u16>,
    /// `true` when the request was blocked by the content filter (never sent).
    pub blocked: bool,
    /// `true` when the request failed at the network level (DNS/TCP/TLS/read)
    /// before an HTTP response was received.  Mutually exclusive with
    /// [`blocked`] — exactly one of `blocked`, `failed`, or a `status` is set.
    ///
    /// [`blocked`]: NetworkEntry::blocked
    pub failed: bool,
    /// For blocked requests: the matched filter rule / source tag.
    /// For failed requests: `"<stage>: <reason>"` (e.g. `"dns: NXDOMAIN"`).
    /// `None` for pending or completed requests.
    pub reason: Option<String>,
    /// Wall-clock instant the request started (used to compute `duration_ms`).
    start: Instant,
    /// Request duration in milliseconds once completed (`None` while pending).
    pub duration_ms: Option<u64>,
}

/// Shared, append-only log of HTTP requests for the network panel.
///
/// Updated from the network thread via [`NetworkLogSink`]; read by the shell UI
/// thread through [`NetworkPanel::refresh`].  Entries persist across navigations
/// unless [`NetworkLog::clear`] is called (the shell clears on top-level
/// navigation, mirroring browser devtools default behaviour).
#[derive(Default)]
pub struct NetworkLog {
    /// Recorded requests, oldest first.  Capped at [`MAX_STORED_ENTRIES`].
    pub entries: Vec<NetworkEntry>,
}

impl NetworkLog {
    /// Record a newly started request: appends a pending entry.
    pub fn record_started(&mut self, method: &str, url: &str) {
        self.entries.push(NetworkEntry {
            method: method.to_owned(),
            url: url.to_owned(),
            status: None,
            blocked: false,
            failed: false,
            reason: None,
            start: Instant::now(),
            duration_ms: None,
        });
        self.trim();
    }

    /// Record a completed request: fills the most recent matching pending entry
    /// with `status` and elapsed time.  If no pending entry matches (e.g. the
    /// start event was missed), a synthetic completed entry is appended.
    pub fn record_completed(&mut self, url: &str, status: u16) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .rev()
            .find(|e| e.url == url && e.status.is_none() && !e.blocked && !e.failed)
        {
            entry.status = Some(status);
            entry.duration_ms = Some(entry.start.elapsed().as_millis() as u64);
        } else {
            self.entries.push(NetworkEntry {
                method: "GET".to_owned(),
                url: url.to_owned(),
                status: Some(status),
                blocked: false,
                failed: false,
                reason: None,
                start: Instant::now(),
                duration_ms: Some(0),
            });
            self.trim();
        }
    }

    /// Record a request blocked by the content filter. `reason` is the matched
    /// filter rule / block source (surfaced by the privacy panel).
    pub fn record_blocked(&mut self, url: &str, reason: &str) {
        self.entries.push(NetworkEntry {
            method: "GET".to_owned(),
            url: url.to_owned(),
            status: None,
            blocked: true,
            failed: false,
            reason: Some(reason.to_owned()),
            start: Instant::now(),
            duration_ms: None,
        });
        self.trim();
    }

    /// Record a network-level failure for a previously started request.
    ///
    /// `stage` is the failure stage string (`"dns"`, `"tcp"`, `"tls"`,
    /// `"read"`); `reason` is the underlying error message.  Finds the most
    /// recent pending entry for `url` and marks it as failed.  If no pending
    /// entry is found (start event missed), a synthetic failed entry is appended.
    pub fn record_failed(&mut self, url: &str, stage: &str, reason: &str) {
        let combined = format!("{stage}: {reason}");
        if let Some(entry) = self
            .entries
            .iter_mut()
            .rev()
            .find(|e| e.url == url && e.status.is_none() && !e.blocked && !e.failed)
        {
            entry.failed = true;
            entry.duration_ms = Some(entry.start.elapsed().as_millis() as u64);
            entry.reason = Some(combined);
        } else {
            self.entries.push(NetworkEntry {
                method: "GET".to_owned(),
                url: url.to_owned(),
                status: None,
                blocked: false,
                failed: true,
                reason: Some(combined),
                start: Instant::now(),
                duration_ms: Some(0),
            });
            self.trim();
        }
    }

    /// Clear all recorded requests (call on every top-level navigation).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Number of recorded requests.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no requests have been recorded.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop oldest entries when over the storage cap.
    fn trim(&mut self) {
        if self.entries.len() > MAX_STORED_ENTRIES {
            let drop = self.entries.len() - MAX_STORED_ENTRIES;
            self.entries.drain(..drop);
        }
    }
}

// ── EventSink wrapper ─────────────────────────────────────────────────────────

/// [`EventSink`] wrapper that forwards every event to an inner sink AND records
/// HTTP request lifecycle events in the shared [`NetworkLog`].
///
/// Constructed once in `run_window_mode`; the `log` `Arc` is also stored in the
/// [`NetworkPanel`] so the UI can read current entries (use
/// [`NetworkPanel::refresh`] to pull a snapshot).
pub struct NetworkLogSink {
    /// Delegate sink (e.g. `StdoutEventSink`).
    pub inner: Arc<dyn EventSink>,
    /// Shared request log updated from this sink's thread.
    pub log: Arc<Mutex<NetworkLog>>,
}

impl EventSink for NetworkLogSink {
    fn emit(&self, event: &Event) {
        // Forward to the underlying sink first (preserves stderr network log).
        self.inner.emit(event);

        let Ok(mut guard) = self.log.lock() else {
            return;
        };
        match event {
            Event::RequestStarted { url, .. } => guard.record_started("GET", url.as_str()),
            Event::RequestCompleted { url, status, .. } => {
                guard.record_completed(url.as_str(), *status);
            }
            Event::RequestBlocked { url, reason, .. } => {
                guard.record_blocked(url.as_str(), reason.as_str());
            }
            Event::RequestFailed { url, stage, reason, .. } => {
                guard.record_failed(url.as_str(), stage.as_str(), reason.as_str());
            }
            _ => {}
        }
    }
}

// ── Panel state ───────────────────────────────────────────────────────────────

/// DevTools network log panel (§7E.4).
///
/// Holds a snapshot of [`NetworkLog`] entries and renders a scrollable bottom
/// overlay.  Toggled with `Ctrl+Shift+E`.
pub struct NetworkPanel {
    /// Snapshot of request entries, pulled from [`NetworkLog`] via [`refresh`].
    ///
    /// [`refresh`]: NetworkPanel::refresh
    entries: Vec<NetworkEntry>,
    /// How many rows to skip from the bottom (0 = show tail; scrolling up grows).
    pub scroll_offset: usize,
    /// Whether the panel is currently shown.
    pub visible: bool,
    /// Shared log produced by [`NetworkLogSink`].
    log: Arc<Mutex<NetworkLog>>,
}

impl NetworkPanel {
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

    /// Pull the latest entries from the shared [`NetworkLog`] into the panel
    /// snapshot.  Call before building the display list on each redraw.
    pub fn refresh(&mut self) {
        if let Ok(guard) = self.log.lock() {
            self.entries = guard.entries.clone();
        }
    }

    /// Clear the shared log (call on every top-level navigation).
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

    /// Scroll up by `n` rows (towards older requests).
    pub fn scroll_up(&mut self, n: usize) {
        let max = self.entries.len().saturating_sub(MAX_VISIBLE_LINES);
        self.scroll_offset = (self.scroll_offset + n).min(max);
    }

    /// Scroll down by `n` rows (towards newer requests).
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the viewport-locked network panel overlay.
///
/// Returns an empty `DisplayList` when `panel.visible` is `false`.
/// `(win_w, win_h)` are the window dimensions in CSS pixels (same units used by
/// all other shell overlay builders).
pub fn build_network_panel(panel: &NetworkPanel, (win_w, win_h): (u32, u32)) -> DisplayList {
    if !panel.visible {
        return Vec::new();
    }

    let visible_count = panel.entries.len().min(MAX_VISIBLE_LINES);
    let panel_h = HEADER_H + visible_count.max(1) as f32 * LINE_H;
    let panel_y = win_h as f32 - panel_h;
    let panel_w = win_w as f32;

    let mut out: DisplayList = Vec::with_capacity(4 + visible_count * 4);

    // Background.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, panel_y, panel_w, panel_h),
        color: BG,
    });

    // Header bar.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, panel_y, panel_w, HEADER_H),
        color: HEADER_BG,
    });

    // Header label.
    out.push(make_text(
        format!("Network ({} requests)", panel.entries.len()),
        H_PAD,
        panel_y + (HEADER_H - FONT_SIZE) / 2.0,
        panel_w * 0.5,
        FONT_SIZE,
        FG_DIM,
    ));

    // Close hint.
    out.push(make_text(
        "Ctrl+Shift+E to close".to_string(),
        panel_w - 150.0,
        panel_y + (HEADER_H - FONT_SIZE) / 2.0,
        140.0,
        FONT_SIZE,
        FG_DIM,
    ));

    // Empty-state hint.
    if panel.entries.is_empty() {
        out.push(make_text(
            "(no requests yet)".to_string(),
            H_PAD,
            panel_y + HEADER_H + (LINE_H - FONT_SIZE) / 2.0,
            panel_w - H_PAD * 2.0,
            FONT_SIZE,
            FG_DIM,
        ));
        return out;
    }

    // Request rows — show the last MAX_VISIBLE_LINES, respecting scroll_offset.
    let total = panel.entries.len();
    let end = total.saturating_sub(panel.scroll_offset);
    let start = end.saturating_sub(MAX_VISIBLE_LINES);

    for (i, entry) in panel.entries[start..end].iter().enumerate() {
        let row_y = panel_y + HEADER_H + i as f32 * LINE_H;
        let text_y = row_y + (LINE_H - FONT_SIZE) / 2.0;

        // Highlight blocked / failed rows.
        if entry.blocked {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(0.0, row_y, panel_w, LINE_H),
                color: BLOCKED_BG,
            });
        } else if entry.failed {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(0.0, row_y, panel_w, LINE_H),
                color: FAILED_BG,
            });
        }

        // Method column.
        out.push(make_text(
            entry.method.clone(),
            COL_METHOD,
            text_y,
            COL_STATUS - COL_METHOD,
            FONT_SIZE,
            FG_METHOD,
        ));

        // Status column.
        out.push(make_text(
            status_label(entry),
            COL_STATUS,
            text_y,
            COL_TIME - COL_STATUS,
            FONT_SIZE,
            status_color(entry),
        ));

        // Timing column.
        out.push(make_text(
            timing_label(entry),
            COL_TIME,
            text_y,
            COL_URL - COL_TIME,
            FONT_SIZE,
            FG_TIME,
        ));

        // URL column (truncated to fit).
        out.push(make_text(
            truncate_url(&entry.url, panel_w - COL_URL - H_PAD),
            COL_URL,
            text_y,
            panel_w - COL_URL - H_PAD,
            FONT_SIZE,
            FG_URL,
        ));
    }

    // Scroll indicator if requests overflow the visible area.
    if total > MAX_VISIBLE_LINES {
        let indicator = if panel.scroll_offset > 0 {
            format!("↑↓  {end}/{total}")
        } else {
            format!("{total}/{total}")
        };
        out.push(make_text(
            indicator,
            panel_w - 250.0,
            panel_y + (HEADER_H - FONT_SIZE) / 2.0,
            90.0,
            FONT_SIZE,
            FG_DIM,
        ));
    }

    out
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Status text for an entry: `"blocked"`, failure stage, numeric code, or `"…"`.
fn status_label(entry: &NetworkEntry) -> String {
    if entry.blocked {
        "blocked".to_string()
    } else if entry.failed {
        // Show the stage prefix (e.g. "dns", "tcp") — fits the narrow status column.
        entry
            .reason
            .as_deref()
            .and_then(|r| r.split(':').next())
            .unwrap_or("err")
            .to_string()
    } else if let Some(code) = entry.status {
        code.to_string()
    } else {
        "…".to_string()
    }
}

/// Status colour: green 2xx, amber 3xx, red 4xx/5xx & blocked & failed, grey pending.
fn status_color(entry: &NetworkEntry) -> Color {
    if entry.blocked || entry.failed {
        return STATUS_ERROR;
    }
    match entry.status {
        Some(c) if (200..300).contains(&c) => STATUS_OK,
        Some(c) if (300..400).contains(&c) => STATUS_REDIRECT,
        Some(_) => STATUS_ERROR,
        None => STATUS_PENDING,
    }
}

/// Timing text: `"123 ms"` once completed, `"…"` while pending, `"—"` if blocked or failed.
fn timing_label(entry: &NetworkEntry) -> String {
    if entry.blocked || entry.failed {
        "—".to_string()
    } else if let Some(ms) = entry.duration_ms {
        format!("{ms} ms")
    } else {
        "…".to_string()
    }
}

/// Truncate a URL to roughly fit `width` CSS px (assuming ~6.5 px/char at the
/// panel font size), prepending an ellipsis when the tail is kept.
fn truncate_url(url: &str, width: f32) -> String {
    let max_chars = (width / 6.5).floor().max(8.0) as usize;
    let count = url.chars().count();
    if count <= max_chars {
        return url.to_owned();
    }
    // Keep the tail (path/file is usually more informative than the scheme).
    let skip = count - (max_chars - 1);
    let tail: String = url.chars().skip(skip).collect();
    format!("…{tail}")
}

fn make_text(text: String, x: f32, y: f32, w: f32, font_size: f32, color: Color) -> DisplayCommand {
    DisplayCommand::DrawText {
        rect: Rect::new(x, y, w, font_size * 1.4),
        text,
        font_size,
        color,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::event::TabId;

    fn make_log() -> Arc<Mutex<NetworkLog>> {
        Arc::new(Mutex::new(NetworkLog::default()))
    }

    fn url(s: &str) -> lumen_core::Url {
        lumen_core::Url::parse(s).expect("valid URL")
    }

    // ── NetworkLog ─────────────────────────────────────────────────────────────

    #[test]
    fn record_started_appends_pending() {
        let mut log = NetworkLog::default();
        log.record_started("GET", "https://example.com/");
        assert_eq!(log.len(), 1);
        assert!(log.entries[0].status.is_none());
        assert!(!log.entries[0].blocked);
        assert_eq!(log.entries[0].method, "GET");
    }

    #[test]
    fn record_completed_fills_pending() {
        let mut log = NetworkLog::default();
        log.record_started("GET", "https://example.com/a");
        log.record_completed("https://example.com/a", 200);
        assert_eq!(log.len(), 1);
        assert_eq!(log.entries[0].status, Some(200));
        assert!(log.entries[0].duration_ms.is_some());
    }

    #[test]
    fn record_completed_without_start_synthesizes() {
        let mut log = NetworkLog::default();
        log.record_completed("https://example.com/orphan", 404);
        assert_eq!(log.len(), 1);
        assert_eq!(log.entries[0].status, Some(404));
        assert_eq!(log.entries[0].duration_ms, Some(0));
    }

    #[test]
    fn record_completed_matches_most_recent_pending() {
        let mut log = NetworkLog::default();
        log.record_started("GET", "https://x.com/dup");
        log.record_started("GET", "https://x.com/dup");
        log.record_completed("https://x.com/dup", 200);
        // Exactly one of the two pending entries gets filled.
        let pending = log.entries.iter().filter(|e| e.status.is_none()).count();
        let done = log.entries.iter().filter(|e| e.status == Some(200)).count();
        assert_eq!(pending, 1);
        assert_eq!(done, 1);
    }

    #[test]
    fn record_blocked_marks_entry() {
        let mut log = NetworkLog::default();
        log.record_blocked("https://ads.com/track.js", "easylist");
        assert!(log.entries[0].blocked);
        assert!(log.entries[0].status.is_none());
    }

    #[test]
    fn clear_empties_log() {
        let mut log = NetworkLog::default();
        log.record_started("GET", "https://a.com/");
        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn trim_respects_cap() {
        let mut log = NetworkLog::default();
        for i in 0..MAX_STORED_ENTRIES + 10 {
            log.record_started("GET", &format!("https://a.com/{i}"));
        }
        assert_eq!(log.len(), MAX_STORED_ENTRIES);
        // Oldest dropped — first kept URL should be /10.
        assert!(log.entries[0].url.ends_with("/10"));
    }

    // ── NetworkLogSink ─────────────────────────────────────────────────────────

    #[test]
    fn sink_records_lifecycle() {
        struct NullSink;
        impl EventSink for NullSink {
            fn emit(&self, _: &Event) {}
        }

        let log = make_log();
        let sink = NetworkLogSink {
            inner: Arc::new(NullSink),
            log: Arc::clone(&log),
        };
        sink.emit(&Event::RequestStarted {
            tab_id: TabId(0),
            url: url("https://example.com/page"),
        });
        sink.emit(&Event::RequestCompleted {
            tab_id: TabId(0),
            url: url("https://example.com/page"),
            status: 200,
        });
        sink.emit(&Event::RequestBlocked {
            tab_id: TabId(0),
            url: url("https://tracker.com/pixel"),
            reason: "easylist".to_owned(),
        });
        let guard = log.lock().unwrap();
        assert_eq!(guard.len(), 2);
        assert_eq!(guard.entries[0].status, Some(200));
        assert!(guard.entries[1].blocked);
    }

    // ── NetworkPanel ───────────────────────────────────────────────────────────

    #[test]
    fn new_panel_hidden_empty() {
        let p = NetworkPanel::new(make_log());
        assert!(!p.visible);
        assert!(p.is_empty());
    }

    #[test]
    fn toggle_visibility() {
        let mut p = NetworkPanel::new(make_log());
        p.toggle();
        assert!(p.visible);
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn refresh_pulls_snapshot() {
        let log = make_log();
        {
            let mut g = log.lock().unwrap();
            g.record_started("GET", "https://a.com/");
            g.record_completed("https://a.com/", 200);
        }
        let mut p = NetworkPanel::new(log);
        p.refresh();
        assert_eq!(p.len(), 1);
        assert_eq!(p.entries[0].status, Some(200));
    }

    #[test]
    fn clear_log_resets() {
        let log = make_log();
        {
            log.lock().unwrap().record_started("GET", "https://a.com/");
        }
        let mut p = NetworkPanel::new(log);
        p.refresh();
        p.scroll_offset = 1;
        p.clear_log();
        assert!(p.is_empty());
        assert_eq!(p.scroll_offset, 0);
    }

    #[test]
    fn scroll_up_down_clamps() {
        let log = make_log();
        {
            let mut g = log.lock().unwrap();
            for i in 0..20 {
                g.record_started("GET", &format!("https://a.com/{i}"));
            }
        }
        let mut p = NetworkPanel::new(log);
        p.refresh();
        p.scroll_up(5);
        assert_eq!(p.scroll_offset, 5);
        p.scroll_down(10);
        assert_eq!(p.scroll_offset, 0);
        p.scroll_up(9999);
        assert_eq!(p.scroll_offset, 20 - MAX_VISIBLE_LINES);
    }

    // ── Rendering ──────────────────────────────────────────────────────────────

    #[test]
    fn build_hidden_returns_empty() {
        let p = NetworkPanel::new(make_log());
        assert!(build_network_panel(&p, (1280, 800)).is_empty());
    }

    #[test]
    fn build_visible_empty_has_header() {
        let mut p = NetworkPanel::new(make_log());
        p.visible = true;
        let dl = build_network_panel(&p, (1280, 800));
        let has_header = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("Network"))
        });
        assert!(has_header);
    }

    #[test]
    fn build_shows_request_row() {
        let log = make_log();
        {
            let mut g = log.lock().unwrap();
            g.record_started("GET", "https://example.com/index.html");
            g.record_completed("https://example.com/index.html", 200);
        }
        let mut p = NetworkPanel::new(log);
        p.visible = true;
        p.refresh();
        let dl = build_network_panel(&p, (1280, 800));
        let has_status = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "200")
        });
        let has_url = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("example.com"))
        });
        assert!(has_status, "must show status 200");
        assert!(has_url, "must show URL");
    }

    #[test]
    fn build_shows_blocked_row() {
        let log = make_log();
        log.lock().unwrap().record_blocked("https://ads.com/track.js", "easylist");
        let mut p = NetworkPanel::new(log);
        p.visible = true;
        p.refresh();
        let dl = build_network_panel(&p, (1280, 800));
        let has_blocked = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "blocked")
        });
        assert!(has_blocked);
    }

    #[test]
    fn build_caps_at_max_visible_lines() {
        let log = make_log();
        {
            let mut g = log.lock().unwrap();
            for i in 0..MAX_VISIBLE_LINES + 5 {
                g.record_started("GET", &format!("https://a.com/{i}"));
                g.record_completed(&format!("https://a.com/{i}"), 200);
            }
        }
        let mut p = NetworkPanel::new(log);
        p.visible = true;
        p.refresh();
        let dl = build_network_panel(&p, (1280, 800));
        // Count status-column "200" cells (one per visible row).
        let rows = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "200")
        }).count();
        assert_eq!(rows, MAX_VISIBLE_LINES);
    }

    // ── Helpers ────────────────────────────────────────────────────────────────

    #[test]
    fn status_color_buckets() {
        let mk = |status: Option<u16>, blocked: bool, failed: bool| NetworkEntry {
            method: "GET".into(),
            url: "https://a.com/".into(),
            status,
            blocked,
            failed,
            reason: if blocked {
                Some("easylist".to_owned())
            } else if failed {
                Some("dns: NXDOMAIN".to_owned())
            } else {
                None
            },
            start: Instant::now(),
            duration_ms: None,
        };
        assert_eq!(status_color(&mk(Some(204), false, false)), STATUS_OK);
        assert_eq!(status_color(&mk(Some(301), false, false)), STATUS_REDIRECT);
        assert_eq!(status_color(&mk(Some(404), false, false)), STATUS_ERROR);
        assert_eq!(status_color(&mk(None, false, false)), STATUS_PENDING);
        assert_eq!(status_color(&mk(None, true, false)), STATUS_ERROR);
        assert_eq!(status_color(&mk(None, false, true)), STATUS_ERROR);
    }

    // ── RequestFailed ──────────────────────────────────────────────────────────

    #[test]
    fn record_failed_marks_pending_entry() {
        let mut log = NetworkLog::default();
        log.record_started("GET", "https://bad.example/");
        log.record_failed("https://bad.example/", "dns", "NXDOMAIN");
        assert_eq!(log.len(), 1);
        let e = &log.entries[0];
        assert!(e.failed);
        assert!(!e.blocked);
        assert!(e.status.is_none());
        assert_eq!(e.reason.as_deref(), Some("dns: NXDOMAIN"));
        assert!(e.duration_ms.is_some());
    }

    #[test]
    fn record_failed_without_start_synthesizes() {
        let mut log = NetworkLog::default();
        log.record_failed("https://orphan.example/", "tcp", "connection refused");
        assert_eq!(log.len(), 1);
        let e = &log.entries[0];
        assert!(e.failed);
        assert_eq!(e.reason.as_deref(), Some("tcp: connection refused"));
    }

    #[test]
    fn record_failed_does_not_clobber_completed() {
        let mut log = NetworkLog::default();
        log.record_started("GET", "https://ok.example/");
        log.record_completed("https://ok.example/", 200);
        // A stale failed event for the same URL must not touch the completed entry.
        log.record_failed("https://ok.example/", "read", "timeout");
        assert_eq!(log.len(), 2);
        assert_eq!(log.entries[0].status, Some(200));
        assert!(!log.entries[0].failed);
        assert!(log.entries[1].failed);
    }

    #[test]
    fn sink_records_failed_event() {
        struct NullSink;
        impl EventSink for NullSink {
            fn emit(&self, _: &Event) {}
        }

        let log = make_log();
        let sink = NetworkLogSink {
            inner: Arc::new(NullSink),
            log: Arc::clone(&log),
        };
        sink.emit(&Event::RequestStarted {
            tab_id: TabId(0),
            url: url("https://timeout.example/"),
        });
        sink.emit(&Event::RequestFailed {
            tab_id: TabId(0),
            url: url("https://timeout.example/"),
            stage: lumen_core::event::RequestStage::Tcp,
            reason: "connection refused".to_owned(),
        });
        let guard = log.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert!(guard.entries[0].failed);
        assert_eq!(guard.entries[0].reason.as_deref(), Some("tcp: connection refused"));
    }

    #[test]
    fn build_shows_failed_row() {
        let log = make_log();
        {
            let mut g = log.lock().unwrap();
            g.record_started("GET", "https://fail.example/");
            g.record_failed("https://fail.example/", "dns", "NXDOMAIN");
        }
        let mut p = NetworkPanel::new(log);
        p.visible = true;
        p.refresh();
        let dl = build_network_panel(&p, (1280, 800));
        let has_stage = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "dns")
        });
        assert!(has_stage, "must show failure stage in status column");
    }

    #[test]
    fn status_label_failed_shows_stage() {
        let entry = NetworkEntry {
            method: "GET".into(),
            url: "https://a.com/".into(),
            status: None,
            blocked: false,
            failed: true,
            reason: Some("tls: bad cert".to_owned()),
            start: Instant::now(),
            duration_ms: Some(5),
        };
        assert_eq!(status_label(&entry), "tls");
    }

    #[test]
    fn timing_label_failed_is_dash() {
        let entry = NetworkEntry {
            method: "GET".into(),
            url: "https://a.com/".into(),
            status: None,
            blocked: false,
            failed: true,
            reason: Some("dns: NXDOMAIN".to_owned()),
            start: Instant::now(),
            duration_ms: Some(10),
        };
        assert_eq!(timing_label(&entry), "—");
    }

    #[test]
    fn truncate_url_keeps_tail() {
        let long = "https://example.com/very/long/path/to/resource.js";
        // Width 100 px → ~15 chars, enough to keep the file name in the tail.
        let t = truncate_url(long, 100.0);
        assert!(t.starts_with('…'));
        assert!(t.ends_with("resource.js"));
    }

    #[test]
    fn truncate_url_short_untouched() {
        assert_eq!(truncate_url("https://a.com/", 400.0), "https://a.com/");
    }
}
