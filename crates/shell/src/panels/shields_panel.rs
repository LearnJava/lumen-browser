//! Shields toolbar widget (7C.4): floating panel anchored below the tab bar at
//! the top-right corner of the window.
//!
//! The panel shows a shield icon, the current domain, whether shields are
//! enabled for that domain, and the number of blocked requests for the
//! current page.  Clicking the shield toggles protection on/off.
//!
//! Toggled with `Ctrl+Shift+S`.
//!
//! Blocked-count data is collected via [`ShieldCountSink`] which intercepts
//! `Event::RequestBlocked` events from the HTTP layer, and stored in a shared
//! [`BlockedLog`] (`Arc<Mutex<…>>`).  The Lumen struct polls this on each
//! redraw to refresh the panel display.
//!
//! The panel shows one honest counter — total requests blocked on the current
//! page (DS-18) — and no trackers/ads breakdown: the installed filter
//! (`lumen_network::EasyListFilter`, fed by the merged EasyList+EasyPrivacy
//! body, see `crates/shell/src/adblock.rs`) tags every match with a
//! list-format reason (`"easylist"`/`"hosts"`), not a tracker-vs-ad category,
//! so a split would be fabricated rather than measured.

use std::sync::{Arc, Mutex};

use lumen_core::event::Event;
use lumen_core::ext::EventSink;

// ── Visual constants ─────────────────────────────────────────────────────────

/// Width of the floating shields panel in CSS px.
pub const PANEL_W: f32 = 220.0;
/// Height of the floating shields panel in CSS px.
pub const PANEL_H: f32 = 90.0;
/// Top offset from the tab-bar bottom edge (CSS px).
const PANEL_TOP_OFFSET: f32 = 4.0;
/// Right margin from the window edge (CSS px).
const PANEL_RIGHT_MARGIN: f32 = 8.0;

// ── Blocked log ───────────────────────────────────────────────────────────────

/// Shared accumulator for the total blocked-request count.
///
/// Updated from the network thread via [`ShieldCountSink`]; read by the shell
/// UI thread to refresh the panel display.  The count persists for the
/// lifetime of the browser process (it is NOT reset on navigation — call
/// [`BlockedLog::clear`] explicitly on page load).
#[derive(Default)]
pub struct BlockedLog {
    /// Total requests blocked since the last [`clear`] call.
    pub total: u32,
}

impl BlockedLog {
    /// Increment the total if `url` has a valid HTTP(S) host.
    ///
    /// Non-HTTP/HTTPS URLs and malformed hostnames are silently ignored.
    pub fn record(&mut self, url: &str) {
        if extract_host(url).is_some() {
            self.total += 1;
        }
    }

    /// Clear the total (call on every top-level navigation).
    pub fn clear(&mut self) {
        self.total = 0;
    }
}

// ── EventSink wrapper ─────────────────────────────────────────────────────────

/// [`EventSink`] wrapper that forwards every event to an inner sink AND
/// records `RequestBlocked` events in the shared [`BlockedLog`].
///
/// Constructed once in `run_window_mode`; the `log` Arc is also stored in the
/// [`ShieldsPanel`] so the UI can read current counts without locking on every
/// frame (use [`ShieldsPanel::refresh`] to pull a snapshot).
pub struct ShieldCountSink {
    /// Delegate sink (e.g. `StdoutEventSink`).
    pub inner: Arc<dyn EventSink>,
    /// Shared blocked-count log updated from this sink's thread.
    pub log: Arc<Mutex<BlockedLog>>,
}

impl EventSink for ShieldCountSink {
    fn emit(&self, event: &Event) {
        // Forward to the underlying sink first (preserves stderr network log).
        self.inner.emit(event);

        if let Event::RequestBlocked { url, .. } = event
            && let Ok(mut guard) = self.log.lock()
        {
            guard.record(url.as_str());
        }
    }
}

// ── Panel state ───────────────────────────────────────────────────────────────

/// Shields floating panel state (7C.4).
pub struct ShieldsPanel {
    /// `true` while the floating panel is visible.  Toggled via Ctrl+Shift+S
    /// or by clicking the shield button in the toolbar (future task).
    pub visible: bool,
    /// Whether shields (request filtering) are enabled for `current_domain`.
    ///
    /// Starts `true` globally.  When the user toggles shields off for a
    /// domain, the shell disables the filter for that domain.
    pub enabled: bool,
    /// Hostname of the currently loaded page (e.g. `"example.com"`).
    ///
    /// `None` while no page is loaded or for local file: URLs.
    pub current_domain: Option<String>,
    /// Snapshot of the total blocked count (pulled from [`BlockedLog`] via
    /// [`ShieldsPanel::refresh`]).
    blocked_total: u32,
    /// Shared log produced by [`ShieldCountSink`].
    log: Arc<Mutex<BlockedLog>>,
}

impl ShieldsPanel {
    /// Create a new hidden panel backed by the given shared `log`.
    pub fn new(log: Arc<Mutex<BlockedLog>>) -> Self {
        Self { visible: false, enabled: true, current_domain: None, blocked_total: 0, log }
    }

    /// Flip panel visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Update `current_domain` and refresh blocked counts.
    pub fn set_domain(&mut self, domain: Option<String>) {
        self.current_domain = domain;
        self.refresh();
    }

    /// Pull the latest total from the shared [`BlockedLog`] into the panel
    /// snapshot field.  Call after every network event or on each redraw.
    pub fn refresh(&mut self) {
        if let Ok(guard) = self.log.lock() {
            self.blocked_total = guard.total;
        }
    }

    /// Clear the shared blocked log (call on top-level navigation).
    pub fn clear_log(&mut self) {
        if let Ok(mut guard) = self.log.lock() {
            guard.clear();
        }
        self.blocked_total = 0;
    }

    /// Total blocked-request count for the current page (from last `refresh`).
    pub fn blocked_total_count(&self) -> u32 {
        self.blocked_total
    }
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Result of a click inside the shields panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShieldsHit {
    /// User toggled shields on/off (clicked the shield / toggle area).
    Toggle,
    /// User closed the panel (clicked the "×").
    Close,
    /// Clicked inside the panel but on a non-interactive area.
    Empty,
}

/// Hit-test a click at CSS-px `(x, y)` against the shields panel.
///
/// Returns `None` when the click is outside the panel.
/// `tab_bar_h` is the height of the tab bar (panel is anchored below it).
pub fn hit_test(
    _panel: &ShieldsPanel,
    x: f32,
    y: f32,
    window_w: f32,
    tab_bar_h: f32,
) -> Option<ShieldsHit> {
    let (px, py) = panel_origin(window_w, tab_bar_h);
    if x < px || x >= px + PANEL_W || y < py || y >= py + PANEL_H {
        return None;
    }

    let rel_x = x - px;
    let rel_y = y - py;

    // Close button: top-right 20×20 area.
    if rel_x >= PANEL_W - 20.0 && rel_y < 20.0 {
        return Some(ShieldsHit::Close);
    }

    // Toggle area: bottom half of the panel.
    if rel_y >= PANEL_H * 0.55 {
        return Some(ShieldsHit::Toggle);
    }

    Some(ShieldsHit::Empty)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Top-left corner of the shields panel in CSS px.
fn panel_origin(window_w: f32, tab_bar_h: f32) -> (f32, f32) {
    let px = (window_w - PANEL_W - PANEL_RIGHT_MARGIN).max(0.0);
    let py = tab_bar_h + PANEL_TOP_OFFSET;
    (px, py)
}

/// Extract the hostname from an HTTP/HTTPS URL string.
///
/// Returns `None` for non-HTTP/HTTPS schemes and malformed URLs.
fn extract_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    // Path starts at the first '/', query at '?', fragment at '#'.
    let host_end = rest
        .find(['/', '?', '#'])
        .unwrap_or(rest.len());
    let host = &rest[..host_end];
    // Strip port if present.
    let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_log() -> Arc<Mutex<BlockedLog>> {
        Arc::new(Mutex::new(BlockedLog::default()))
    }

    fn make_panel_visible(enabled: bool, domain: Option<&str>) -> ShieldsPanel {
        let log = make_log();
        let mut p = ShieldsPanel::new(log);
        p.visible = true;
        p.enabled = enabled;
        p.current_domain = domain.map(|s| s.to_owned());
        p
    }

    const WIN_W: f32 = 1024.0;
    const TAB_H: f32 = 36.0;

    // ── BlockedLog ───────────────────────────────────────────────────────────

    #[test]
    fn blocked_log_record_increments_count() {
        let mut log = BlockedLog::default();
        log.record("https://tracker.example.com/pixel.gif");
        log.record("https://tracker.example.com/other.js");
        assert_eq!(log.total, 2);
    }

    #[test]
    fn blocked_log_ignores_non_http() {
        let mut log = BlockedLog::default();
        log.record("data:text/plain,hello");
        log.record("chrome://settings");
        assert_eq!(log.total, 0);
    }

    #[test]
    fn blocked_log_clear_resets() {
        let mut log = BlockedLog::default();
        log.record("https://ads.example.com/ad.js");
        log.clear();
        assert_eq!(log.total, 0);
    }

    #[test]
    fn blocked_log_counts_url_with_port() {
        let mut log = BlockedLog::default();
        log.record("https://ads.example.com:8080/track");
        assert_eq!(log.total, 1);
    }

    // ── extract_host ─────────────────────────────────────────────────────────

    #[test]
    fn extract_host_https() {
        assert_eq!(
            extract_host("https://www.example.com/path?q=1"),
            Some("www.example.com".to_owned())
        );
    }

    #[test]
    fn extract_host_http() {
        assert_eq!(
            extract_host("http://ads.com/pixel"),
            Some("ads.com".to_owned())
        );
    }

    #[test]
    fn extract_host_with_port() {
        assert_eq!(
            extract_host("https://api.example.com:443/v1"),
            Some("api.example.com".to_owned())
        );
    }

    #[test]
    fn extract_host_data_url_returns_none() {
        assert_eq!(extract_host("data:text/plain,abc"), None);
    }

    // ── ShieldsPanel ─────────────────────────────────────────────────────────

    #[test]
    fn new_panel_hidden() {
        let p = ShieldsPanel::new(make_log());
        assert!(!p.visible);
    }

    #[test]
    fn toggle_shows_panel() {
        let mut p = ShieldsPanel::new(make_log());
        p.toggle();
        assert!(p.visible);
    }

    #[test]
    fn double_toggle_hides() {
        let mut p = ShieldsPanel::new(make_log());
        p.toggle();
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn refresh_picks_up_counts() {
        let log = make_log();
        {
            let mut guard = log.lock().unwrap();
            guard.record("https://tracker.com/pixel");
            guard.record("https://tracker.com/pixel2");
            guard.record("https://other.com/js");
        }
        let mut p = ShieldsPanel::new(log);
        p.current_domain = Some("tracker.com".to_owned());
        p.refresh();
        assert_eq!(p.blocked_total_count(), 3);
    }

    #[test]
    fn clear_log_resets_counts() {
        let log = make_log();
        {
            let mut guard = log.lock().unwrap();
            guard.record("https://tracker.com/pixel");
        }
        let mut p = ShieldsPanel::new(log);
        p.current_domain = Some("tracker.com".to_owned());
        p.refresh();
        assert_eq!(p.blocked_total_count(), 1);
        p.clear_log();
        assert_eq!(p.blocked_total_count(), 0);
    }

    // ── Hit-testing ──────────────────────────────────────────────────────────

    #[test]
    fn hit_outside_panel_returns_none() {
        let p = make_panel_visible(true, Some("example.com"));
        // Click far top-left.
        assert_eq!(hit_test(&p, 0.0, TAB_H + 2.0, WIN_W, TAB_H), None);
    }

    #[test]
    fn hit_close_button() {
        let p = make_panel_visible(true, Some("example.com"));
        let (px, py) = panel_origin(WIN_W, TAB_H);
        // Top-right corner.
        let hit = hit_test(&p, px + PANEL_W - 5.0, py + 5.0, WIN_W, TAB_H);
        assert_eq!(hit, Some(ShieldsHit::Close));
    }

    #[test]
    fn hit_toggle_area() {
        let p = make_panel_visible(true, Some("example.com"));
        let (px, py) = panel_origin(WIN_W, TAB_H);
        // Bottom half of panel.
        let hit = hit_test(&p, px + PANEL_W * 0.5, py + PANEL_H * 0.8, WIN_W, TAB_H);
        assert_eq!(hit, Some(ShieldsHit::Toggle));
    }

    #[test]
    fn hit_empty_area() {
        let p = make_panel_visible(true, Some("example.com"));
        let (px, py) = panel_origin(WIN_W, TAB_H);
        // Upper middle area (not close, not toggle).
        let hit = hit_test(&p, px + 40.0, py + 15.0, WIN_W, TAB_H);
        assert_eq!(hit, Some(ShieldsHit::Empty));
    }

    // ── ShieldCountSink ──────────────────────────────────────────────────────

    #[test]
    fn shield_count_sink_records_blocked() {
        use lumen_core::event::{TabId, Event};

        struct NullSink;
        impl EventSink for NullSink {
            fn emit(&self, _: &Event) {}
        }

        let log: Arc<Mutex<BlockedLog>> = Arc::new(Mutex::new(BlockedLog::default()));
        let sink = ShieldCountSink {
            inner: Arc::new(NullSink),
            log: Arc::clone(&log),
        };
        let url = lumen_core::Url::parse("https://tracker.example.com/pixel.gif")
            .expect("valid URL");
        sink.emit(&Event::RequestBlocked {
            tab_id: TabId(0),
            url,
            reason: "easylist".to_owned(),
        });
        let guard = log.lock().unwrap();
        assert_eq!(guard.total, 1);
    }

}
