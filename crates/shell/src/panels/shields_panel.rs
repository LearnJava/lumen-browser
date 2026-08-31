//! Shields toolbar widget (7C.4): floating panel anchored below the tab bar at
//! the top-right corner of the window.
//!
//! The legacy display-list renderer was removed in CC-15-4 — under the engine
//! chrome (default since CC-14) the popover is `#permPopover`. `BUG-411`
//! restored the domain readout and the shields switch there and, at the same
//! time, turned the switch from a pure indicator into the real control: the
//! per-site state below is what the shell pushes into
//! `lumen_network::set_global_adblock_enabled`, so flipping it actually stops
//! (or resumes) request filtering. `hit_test` is deliberately kept (still
//! called ungated, itself a `BUG-404` site) — see that bug file before
//! deleting anything here.
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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lumen_core::event::Event;
use lumen_core::ext::EventSink;

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
    /// Whether shields (request filtering) are on when a host has no explicit
    /// per-site choice — mirrors the persisted `BrowserSettings::shields_enabled`
    /// ("Блокировать рекламу" in `#view-settings`), pushed in by the shell via
    /// [`ShieldsPanel::set_default_enabled`].
    ///
    /// Before [BUG-411](../../../../bugs/BUG-411-FIXED.md) this was a plain
    /// `enabled: bool` nothing read: the legacy panel painted it, and its
    /// removal in CC-15-4 left the field write-only.
    default_enabled: bool,
    /// Per-host overrides of [`Self::default_enabled`], keyed by the same
    /// lowercase hostname [`Self::set_domain`] stores
    /// ([BUG-411](../../../../bugs/BUG-411-FIXED.md)).
    ///
    /// Session-scoped: not persisted, so a restart falls every host back to
    /// the setting. Persisting per-site exceptions needs a store of its own
    /// (`BrowserSettings` holds one flat `shields_enabled` bool) — out of
    /// scope here, and an unpersisted exception is the honest behaviour of a
    /// "for this site" switch that has nowhere to be written yet.
    site_overrides: HashMap<String, bool>,
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
        Self {
            visible: false,
            default_enabled: true,
            site_overrides: HashMap::new(),
            current_domain: None,
            blocked_total: 0,
            log,
        }
    }

    /// Set the fallback used by hosts with no per-site choice — call whenever
    /// the persisted "Блокировать рекламу" setting is loaded or changed
    /// ([BUG-411](../../../../bugs/BUG-411-FIXED.md)).
    ///
    /// Existing per-site overrides survive: a host the user explicitly switched
    /// keeps its choice for the session, exactly as a per-site exception should.
    pub fn set_default_enabled(&mut self, enabled: bool) {
        self.default_enabled = enabled;
    }

    /// Whether request filtering is on for [`Self::current_domain`] — the
    /// per-site override if the host has one, otherwise
    /// [`Self::default_enabled`] ([BUG-411](../../../../bugs/BUG-411-FIXED.md)).
    ///
    /// This is the single value the shell mirrors into
    /// `lumen_network::set_global_adblock_enabled`, so it decides whether the
    /// installed filter actually runs.
    #[must_use]
    pub fn enabled_for_current(&self) -> bool {
        match &self.current_domain {
            Some(domain) => self.site_overrides.get(domain).copied().unwrap_or(self.default_enabled),
            None => self.default_enabled,
        }
    }

    /// Flip shields for [`Self::current_domain`] and return the new state
    /// ([BUG-411](../../../../bugs/BUG-411-FIXED.md)).
    ///
    /// With no host loaded (`about:blank`, `file://`) there is nothing to key
    /// an exception to, so the flip lands on [`Self::default_enabled`] instead
    /// — the switch stays meaningful rather than silently doing nothing.
    pub fn toggle_current_site(&mut self) -> bool {
        let next = !self.enabled_for_current();
        match &self.current_domain {
            Some(domain) => {
                self.site_overrides.insert(domain.clone(), next);
            }
            None => self.default_enabled = next,
        }
        next
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
/// Returns `None` when the click is outside the panel. BUG-461:
/// `popover_rect` is `#permPopover`'s real measured layout rect (the shared
/// engine-drawn popover this panel's controls are now rendered into —
/// `Lumen::chrome_perm_popover_rect`), not a guessed box anchored to the
/// window's right edge/`toolbar::CHROME_H` — that guess drifted from where
/// `chrome.css` (`.popover{ position:absolute; top:34px; right:0 }`, nested
/// under `.omnibox-wrap`) actually places it. A zero-sized `popover_rect`
/// (no chrome layout yet) safely matches nothing.
pub fn hit_test(_panel: &ShieldsPanel, x: f32, y: f32, popover_rect: lumen_core::geom::Rect) -> Option<ShieldsHit> {
    let (px, py, pw, ph) = (popover_rect.x, popover_rect.y, popover_rect.width, popover_rect.height);
    if x < px || x >= px + pw || y < py || y >= py + ph {
        return None;
    }

    let rel_x = x - px;
    let rel_y = y - py;

    // Close button: top-right 20×20 area.
    if rel_x >= pw - 20.0 && rel_y < 20.0 {
        return Some(ShieldsHit::Close);
    }

    // Toggle area: bottom half of the panel.
    if rel_y >= ph * 0.55 {
        return Some(ShieldsHit::Toggle);
    }

    Some(ShieldsHit::Empty)
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
        p.set_default_enabled(enabled);
        p.current_domain = domain.map(|s| s.to_owned());
        p
    }

    // ── Per-site shields (BUG-411) ───────────────────────────────────────────

    #[test]
    fn enabled_for_current_falls_back_to_the_default_without_an_override() {
        let mut p = make_panel_visible(true, Some("example.org"));
        assert!(p.enabled_for_current());
        p.set_default_enabled(false);
        assert!(!p.enabled_for_current());
    }

    #[test]
    fn toggle_current_site_overrides_only_that_host() {
        let mut p = make_panel_visible(true, Some("example.org"));
        assert!(!p.toggle_current_site());
        assert!(!p.enabled_for_current());
        // A different host is untouched by the exception.
        p.set_domain(Some("other.test".to_owned()));
        assert!(p.enabled_for_current());
        // …and coming back restores it.
        p.set_domain(Some("example.org".to_owned()));
        assert!(!p.enabled_for_current());
    }

    #[test]
    fn per_site_override_survives_a_default_change() {
        let mut p = make_panel_visible(true, Some("example.org"));
        p.toggle_current_site();
        p.set_default_enabled(false);
        assert!(!p.enabled_for_current());
        p.set_default_enabled(true);
        assert!(!p.enabled_for_current(), "an explicit per-site choice outranks the setting");
    }

    #[test]
    fn toggle_without_a_host_flips_the_default() {
        let mut p = make_panel_visible(true, None);
        assert!(!p.toggle_current_site());
        // With no host, `enabled_for_current` *is* the default.
        assert!(!p.enabled_for_current());
    }

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

    // ── Hit-testing (BUG-461: against a measured popover rect) ────────────────

    fn test_popover_rect() -> lumen_core::geom::Rect {
        lumen_core::geom::Rect::new(700.0, TAB_H + 4.0, 220.0, 90.0)
    }

    #[test]
    fn hit_outside_panel_returns_none() {
        let p = make_panel_visible(true, Some("example.com"));
        // Click far top-left, well outside the measured popover rect.
        assert_eq!(hit_test(&p, 0.0, TAB_H + 2.0, test_popover_rect()), None);
    }

    #[test]
    fn hit_outside_zero_rect_returns_none() {
        // No chrome layout yet — a zero-sized rect must match nothing.
        let p = make_panel_visible(true, Some("example.com"));
        assert_eq!(hit_test(&p, 0.0, 0.0, lumen_core::geom::Rect::ZERO), None);
    }

    #[test]
    fn hit_close_button() {
        let p = make_panel_visible(true, Some("example.com"));
        let r = test_popover_rect();
        // Top-right corner.
        let hit = hit_test(&p, r.x + r.width - 5.0, r.y + 5.0, r);
        assert_eq!(hit, Some(ShieldsHit::Close));
    }

    #[test]
    fn hit_toggle_area() {
        let p = make_panel_visible(true, Some("example.com"));
        let r = test_popover_rect();
        // Bottom half of panel.
        let hit = hit_test(&p, r.x + r.width * 0.5, r.y + r.height * 0.8, r);
        assert_eq!(hit, Some(ShieldsHit::Toggle));
    }

    #[test]
    fn hit_empty_area() {
        let p = make_panel_visible(true, Some("example.com"));
        let r = test_popover_rect();
        // Upper middle area (not close, not toggle).
        let hit = hit_test(&p, r.x + 40.0, r.y + 15.0, r);
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
