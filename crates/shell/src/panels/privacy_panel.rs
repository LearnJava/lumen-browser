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
//! Rendered as `#privacyPanel` in the engine chrome (`bind_privacy`,
//! UX-CHROME-PANELS): a right-docked overlay spanning from the bottom of the
//! toolbar to the bottom of the window. Toggle with `Ctrl+Shift+Y` (priva**Y**; the other privacy shortcuts —
//! `Ctrl+Shift+S` shields, `Ctrl+Shift+K` cookie banners — are taken).

use std::sync::{Arc, Mutex};

use lumen_chrome::{ChromePrivacyModel, ChromePrivacyRowModel};
use crate::devtools::network_panel::{NetworkEntry, NetworkLog};

// ── Layout constants ────────────────────────────────────────────────────────────

/// Height of the title header bar.
const HEADER_H: f32 = 34.0;
/// Height of the blocked/allowed summary bar below the header.
const SUMMARY_H: f32 = 30.0;
/// Height of a single request row (`.pv-row` in the chrome asset).
const ROW_H: f32 = 38.0;

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

impl PrivacyPanel {
    /// `#privacyPanel` snapshot (UX-CHROME-PANELS): newest-first rows that fit
    /// in `body_h` CSS px of list area, shifted by `scroll_offset`.
    pub fn chrome_model(&self, body_h: f32) -> ChromePrivacyModel {
        let visible_rows = (body_h / ROW_H).ceil().max(1.0) as usize;
        let total = self.entries.len();
        let rows = self
            .entries
            .iter()
            .rev()
            .skip(self.scroll_offset)
            .take(visible_rows)
            .map(|e| {
                let (sub, class) = sub_line(e);
                ChromePrivacyRowModel { host: host_of(&e.url), sub, state: class.to_owned() }
            })
            .collect();
        let position = if total > visible_rows {
            format!("{}/{total}", total.saturating_sub(self.scroll_offset))
        } else {
            String::new()
        };
        ChromePrivacyModel {
            open: self.visible,
            blocked: self.blocked_count(),
            allowed: self.allowed_count(),
            position,
            rows,
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Secondary row line and its state class: the matched filter for blocked
/// requests, otherwise the HTTP status (or `"pending…"`). The class
/// (`blocked` / `allowed` / `pending`) picks the dot and text colour.
fn sub_line(entry: &NetworkEntry) -> (String, &'static str) {
    if entry.blocked {
        let rule = entry.reason.as_deref().unwrap_or("blocked");
        (format!("blocked · {rule}"), "blocked")
    } else if let Some(code) = entry.status {
        (format!("allowed · {code}"), "allowed")
    } else {
        ("pending…".to_string(), "pending")
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

    // ── sub_line ─────────────────────────────────────────────────────────────

    #[test]
    fn sub_line_blocked_shows_filter() {
        let log = make_log();
        log.lock().unwrap().record_blocked("https://a.com/x", "||evil^");
        let mut p = PrivacyPanel::new(log);
        p.refresh();
        let (text, class) = sub_line(&p.entries[0]);
        assert!(text.contains("||evil^"));
        assert_eq!(class, "blocked");
    }

    #[test]
    fn sub_line_state_buckets() {
        let log = make_log();
        seed(&log);
        let mut p = PrivacyPanel::new(log);
        p.refresh();
        // 0: completed → allowed, 1: blocked, 2: pending.
        assert_eq!(sub_line(&p.entries[0]).1, "allowed");
        assert_eq!(sub_line(&p.entries[1]).1, "blocked");
        assert_eq!(sub_line(&p.entries[2]).1, "pending");
    }

    // ── chrome_model ─────────────────────────────────────────────────────────

    #[test]
    fn chrome_model_hidden_reports_closed() {
        let p = PrivacyPanel::new(make_log());
        let m = p.chrome_model(400.0);
        assert!(!m.open);
        assert!(m.rows.is_empty());
        assert!(m.position.is_empty());
    }

    #[test]
    fn chrome_model_lists_newest_first_with_filter() {
        let log = make_log();
        seed(&log);
        let mut p = PrivacyPanel::new(log);
        p.visible = true;
        p.refresh();
        let m = p.chrome_model(400.0);
        assert!(m.open);
        assert_eq!((m.blocked, m.allowed), (1, 2));
        assert_eq!(m.rows[0].host, "cdn.example.com");
        assert_eq!(m.rows[0].state, "pending");
        assert_eq!(m.rows[1].host, "ads.tracker.com");
        assert!(m.rows[1].sub.contains("||tracker.com^"));
        assert!(m.position.is_empty());
    }

    #[test]
    fn chrome_model_windows_rows_and_reports_position() {
        let log = make_log();
        {
            let mut g = log.lock().unwrap();
            for i in 0..30 {
                g.record_started("GET", &format!("https://a.com/{i}"));
            }
        }
        let mut p = PrivacyPanel::new(log);
        p.refresh();
        let m = p.chrome_model(ROW_H * 4.0);
        assert_eq!(m.rows.len(), 4);
        assert_eq!(m.position, "30/30");
        p.scroll_offset = 5;
        let m = p.chrome_model(ROW_H * 4.0);
        assert_eq!(m.rows[0].host, "a.com");
        assert_eq!(m.position, "25/30");
    }
}
