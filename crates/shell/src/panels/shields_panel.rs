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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lumen_core::event::Event;
use lumen_core::ext::EventSink;
use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

// ── Visual constants ─────────────────────────────────────────────────────────

/// Width of the floating shields panel in CSS px.
pub const PANEL_W: f32 = 220.0;
/// Height of the floating shields panel in CSS px.
pub const PANEL_H: f32 = 90.0;
/// Top offset from the tab-bar bottom edge (CSS px).
const PANEL_TOP_OFFSET: f32 = 4.0;
/// Right margin from the window edge (CSS px).
const PANEL_RIGHT_MARGIN: f32 = 8.0;

const BG: Color = Color { r: 20, g: 20, b: 28, a: 245 };
const BORDER: Color = Color { r: 50, g: 50, b: 65, a: 255 };
const TEXT_MAIN: Color = Color { r: 220, g: 220, b: 228, a: 255 };
const TEXT_DIM: Color = Color { r: 130, g: 130, b: 145, a: 255 };
const SHIELD_ON: Color = Color { r: 60, g: 200, b: 120, a: 255 };
const SHIELD_OFF: Color = Color { r: 180, g: 80, b: 80, a: 255 };
const TOGGLE_BG_ON: Color = Color { r: 30, g: 90, b: 55, a: 255 };
const TOGGLE_BG_OFF: Color = Color { r: 90, g: 35, b: 35, a: 255 };
const CLOSE_FG: Color = Color { r: 140, g: 80, b: 80, a: 255 };

const FONT_SZ: f32 = 11.0;
const FONT_SZ_SM: f32 = 10.0;
const PANEL_RADIUS: f32 = 6.0;

// ── Blocked log ───────────────────────────────────────────────────────────────

/// Shared accumulator for blocked-request counts, indexed by hostname.
///
/// Updated from the network thread via [`ShieldCountSink`]; read by the shell
/// UI thread to refresh the panel display.  Counts persist for the lifetime of
/// the browser process (they are NOT reset on navigation — call
/// [`BlockedLog::clear`] explicitly on page load).
#[derive(Default)]
pub struct BlockedLog {
    /// Blocked-request count per hostname (`example.com → 3`).
    pub counts: HashMap<String, u32>,
    /// Total blocked across all domains since the last [`clear`] call.
    pub total: u32,
}

impl BlockedLog {
    /// Increment the count for the hostname extracted from `url`.
    ///
    /// Non-HTTP/HTTPS URLs and malformed hostnames are silently ignored.
    pub fn record(&mut self, url: &str) {
        if let Some(host) = extract_host(url) {
            *self.counts.entry(host).or_insert(0) += 1;
            self.total += 1;
        }
    }

    /// Clear all counts (call on every top-level navigation).
    pub fn clear(&mut self) {
        self.counts.clear();
        self.total = 0;
    }

    /// Blocked count for a specific hostname (0 if unseen).
    pub fn count_for(&self, host: &str) -> u32 {
        self.counts.get(host).copied().unwrap_or(0)
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
    /// Snapshot of blocked counts (pulled from [`BlockedLog`] via
    /// [`ShieldsPanel::refresh`]).
    blocked_total: u32,
    /// Snapshot: blocked count for the current domain only.
    blocked_domain: u32,
    /// Shared log produced by [`ShieldCountSink`].
    log: Arc<Mutex<BlockedLog>>,
}

impl ShieldsPanel {
    /// Create a new hidden panel backed by the given shared `log`.
    pub fn new(log: Arc<Mutex<BlockedLog>>) -> Self {
        Self {
            visible: false,
            enabled: true,
            current_domain: None,
            blocked_total: 0,
            blocked_domain: 0,
            log,
        }
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

    /// Pull the latest counts from the shared [`BlockedLog`] into the panel
    /// snapshot fields.  Call after every network event or on each redraw.
    pub fn refresh(&mut self) {
        if let Ok(guard) = self.log.lock() {
            self.blocked_total = guard.total;
            if let Some(ref d) = self.current_domain {
                self.blocked_domain = guard.count_for(d);
            } else {
                self.blocked_domain = 0;
            }
        }
    }

    /// Clear the shared blocked log (call on top-level navigation).
    pub fn clear_log(&mut self) {
        if let Ok(mut guard) = self.log.lock() {
            guard.clear();
        }
        self.blocked_total = 0;
        self.blocked_domain = 0;
    }

    /// Blocked-request count for the current domain (from last `refresh`).
    pub fn blocked_domain_count(&self) -> u32 {
        self.blocked_domain
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

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the display list for the shields floating panel.
///
/// The panel is anchored at the top-right of the window, offset by
/// `tab_bar_h` from the top.
pub fn build_panel(panel: &ShieldsPanel, window_w: f32, tab_bar_h: f32) -> DisplayList {
    let (px, py) = panel_origin(window_w, tab_bar_h);
    let mut out = DisplayList::with_capacity(20);
    let radii = uniform_radii(PANEL_RADIUS);

    // Background + border.
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, PANEL_H),
        radii,
        color: BORDER,
    });
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px + 1.0, py + 1.0, PANEL_W - 2.0, PANEL_H - 2.0),
        radii: uniform_radii(PANEL_RADIUS - 1.0),
        color: BG,
    });

    // Close "×" button (top-right).
    let close_x = px + PANEL_W - 18.0;
    let close_y = py + 5.0;
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(close_x, close_y, 14.0, FONT_SZ * 1.2),
        text: "×".to_owned(),
        font_size: FONT_SZ,
        color: CLOSE_FG,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    // Shield icon + status.
    let shield_color = if panel.enabled { SHIELD_ON } else { SHIELD_OFF };
    let status_label = if panel.enabled { "SHIELDS ON" } else { "SHIELDS OFF" };
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(px + 10.0, py + 6.0, 16.0, 16.0),
        text: "🛡".to_owned(),
        font_size: 14.0,
        color: shield_color,
        font_family: Vec::new(),
        font_weight: FontWeight::BOLD,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(px + 30.0, py + 8.0, 100.0, FONT_SZ * 1.3),
        text: status_label.to_owned(),
        font_size: FONT_SZ,
        color: shield_color,
        font_family: Vec::new(),
        font_weight: FontWeight::BOLD,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    // Domain row.
    let domain_label = panel
        .current_domain
        .as_deref()
        .unwrap_or("(no domain)");
    let domain_text = truncate_label(domain_label, 26);
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(px + 10.0, py + 26.0, PANEL_W - 20.0, FONT_SZ_SM * 1.3),
        text: domain_text,
        font_size: FONT_SZ_SM,
        color: TEXT_DIM,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    // Blocked-count row.
    let count_text = format!(
        "{} blocked (this page: {})",
        panel.blocked_domain_count(),
        panel.blocked_total_count(),
    );
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(px + 10.0, py + 40.0, PANEL_W - 20.0, FONT_SZ_SM * 1.3),
        text: count_text,
        font_size: FONT_SZ_SM,
        color: TEXT_MAIN,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    // Toggle button (bottom strip).
    let toggle_label = if panel.enabled {
        "Disable for this site"
    } else {
        "Enable for this site"
    };
    let toggle_bg = if panel.enabled { TOGGLE_BG_OFF } else { TOGGLE_BG_ON };
    let toggle_fg = if panel.enabled { SHIELD_OFF } else { SHIELD_ON };
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px + 1.0, py + PANEL_H - 25.0, PANEL_W - 2.0, 24.0),
        radii: uniform_radii(PANEL_RADIUS - 1.0),
        color: toggle_bg,
    });
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(
            px + 10.0,
            py + PANEL_H - 20.0,
            PANEL_W - 20.0,
            FONT_SZ * 1.2,
        ),
        text: toggle_label.to_owned(),
        font_size: FONT_SZ,
        color: toggle_fg,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    out
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Top-left corner of the shields panel in CSS px.
fn panel_origin(window_w: f32, tab_bar_h: f32) -> (f32, f32) {
    let px = (window_w - PANEL_W - PANEL_RIGHT_MARGIN).max(0.0);
    let py = tab_bar_h + PANEL_TOP_OFFSET;
    (px, py)
}

fn uniform_radii(r: f32) -> CornerRadii {
    CornerRadii {
        tl: r, tl_y: r,
        tr: r, tr_y: r,
        br: r, br_y: r,
        bl: r, bl_y: r,
    }
}

/// Truncate a label to at most `max_chars` characters, appending "…" if needed.
fn truncate_label(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_owned();
    }
    let truncated: String = s.chars().take(max_chars - 1).collect();
    format!("{truncated}…")
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
        assert_eq!(log.count_for("tracker.example.com"), 2);
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
        assert!(log.counts.is_empty());
    }

    #[test]
    fn blocked_log_strips_port() {
        let mut log = BlockedLog::default();
        log.record("https://ads.example.com:8080/track");
        assert_eq!(log.count_for("ads.example.com"), 1);
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
        assert_eq!(p.blocked_domain_count(), 2);
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

    // ── Rendering ────────────────────────────────────────────────────────────

    #[test]
    fn build_panel_emits_commands() {
        let p = make_panel_visible(true, Some("example.com"));
        let dl = build_panel(&p, WIN_W, TAB_H);
        assert!(!dl.is_empty());
    }

    #[test]
    fn build_panel_shields_on_label() {
        let p = make_panel_visible(true, Some("example.com"));
        let dl = build_panel(&p, WIN_W, TAB_H);
        let has_on = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("ON"))
        });
        assert!(has_on, "panel must show SHIELDS ON when enabled");
    }

    #[test]
    fn build_panel_shields_off_label() {
        let p = make_panel_visible(false, Some("example.com"));
        let dl = build_panel(&p, WIN_W, TAB_H);
        let has_off = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("OFF"))
        });
        assert!(has_off, "panel must show SHIELDS OFF when disabled");
    }

    #[test]
    fn build_panel_shows_domain() {
        let p = make_panel_visible(true, Some("example.com"));
        let dl = build_panel(&p, WIN_W, TAB_H);
        let has_domain = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("example.com"))
        });
        assert!(has_domain);
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
        assert_eq!(guard.count_for("tracker.example.com"), 1);
    }

    #[test]
    fn truncate_label_short() {
        assert_eq!(truncate_label("example.com", 26), "example.com");
    }

    #[test]
    fn truncate_label_long() {
        let long = "a".repeat(30);
        let t = truncate_label(&long, 26);
        assert!(t.chars().count() <= 26);
        assert!(t.ends_with('…'));
    }
}
