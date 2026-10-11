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
//! [`MAX_VISIBLE_LINES`] rows, rendered as `#networkPanel` in the engine chrome
//! Requests are displayed oldest-first (scroll_offset = 0 shows the tail).

use std::sync::{Arc, Mutex};
use std::time::Instant;

use lumen_core::event::Event;
use lumen_core::ext::EventSink;

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

    /// Record a fully-formed request logged by page JS via
    /// `_lumen_log_network_request`. Unlike [`record_completed`], the method,
    /// status and duration are all supplied by the caller (the engine's network
    /// thread never saw this request), so a single complete entry is appended.
    ///
    /// [`record_completed`]: NetworkLog::record_completed
    pub fn record_js(
        &mut self,
        method: &str,
        url: &str,
        status: Option<u16>,
        duration_ms: Option<u64>,
    ) {
        self.entries.push(NetworkEntry {
            method: method.to_owned(),
            url: url.to_owned(),
            status,
            blocked: false,
            failed: false,
            reason: None,
            start: Instant::now(),
            duration_ms,
        });
        self.trim();
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

    /// Pull a fresh clone of the shared log's entries.
    ///
    /// Used by other consumers (e.g. the inspector's Network tab) that render
    /// from the same request stream without owning the [`NetworkPanel`]'s own
    /// snapshot. Returns an empty vec if the lock is poisoned.
    pub fn entries_clone(&self) -> Vec<NetworkEntry> {
        self.log
            .lock()
            .map(|g| g.entries.clone())
            .unwrap_or_default()
    }

    /// Append a JS-logged request to the shared log (drained from
    /// `_lumen_log_network_request`). See [`NetworkLog::record_js`].
    pub fn record_js_request(
        &self,
        method: &str,
        url: &str,
        status: Option<u16>,
        duration_ms: Option<u64>,
    ) {
        if let Ok(mut g) = self.log.lock() {
            g.record_js(method, url, status, duration_ms);
        }
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

// ── Chrome model ──────────────────────────────────────────────────────────────

impl NetworkPanel {
    /// Rows `#networkPanel` shows: the last [`MAX_VISIBLE_LINES`] entries,
    /// shifted back by `scroll_offset`, oldest first (UX-CHROME-PANELS).
    pub fn chrome_model(&self) -> lumen_chrome::ChromeNetworkModel {
        let total = self.entries.len();
        let end = total.saturating_sub(self.scroll_offset);
        let start = end.saturating_sub(MAX_VISIBLE_LINES);
        let rows = self.entries[start..end]
            .iter()
            .map(|e| lumen_chrome::ChromeNetworkRowModel {
                method: e.method.clone(),
                status: status_label(e),
                status_class: status_class(e),
                time: timing_label(e),
                url: e.url.clone(),
                blocked: e.blocked,
                failed: e.failed,
            })
            .collect();
        lumen_chrome::ChromeNetworkModel { open: self.visible, total, rows }
    }
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

/// Status badge class: `2` 2xx, `3` 3xx, `4` 4xx/5xx & blocked & failed, `0` pending.
fn status_class(entry: &NetworkEntry) -> u8 {
    if entry.blocked || entry.failed {
        return 4;
    }
    match entry.status {
        Some(c) if (200..300).contains(&c) => 2,
        Some(c) if (300..400).contains(&c) => 3,
        Some(_) => 4,
        None => 0,
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

    // ── Chrome model ───────────────────────────────────────────────────────────

    #[test]
    fn chrome_model_hidden_reports_closed() {
        let p = NetworkPanel::new(make_log());
        let m = p.chrome_model();
        assert!(!m.open);
        assert!(m.rows.is_empty());
    }

    #[test]
    fn chrome_model_shows_request_row() {
        let log = make_log();
        {
            let mut g = log.lock().unwrap();
            g.record_started("GET", "https://example.com/index.html");
            g.record_completed("https://example.com/index.html", 200);
        }
        let mut p = NetworkPanel::new(log);
        p.visible = true;
        p.refresh();
        let m = p.chrome_model();
        assert!(m.open);
        assert_eq!(m.total, 1);
        assert_eq!(m.rows[0].status, "200");
        assert_eq!(m.rows[0].status_class, 2);
        assert!(m.rows[0].url.contains("example.com"));
    }

    #[test]
    fn chrome_model_marks_blocked_row() {
        let log = make_log();
        log.lock().unwrap().record_blocked("https://ads.com/track.js", "easylist");
        let mut p = NetworkPanel::new(log);
        p.refresh();
        let m = p.chrome_model();
        assert_eq!(m.rows[0].status, "blocked");
        assert!(m.rows[0].blocked);
    }

    #[test]
    fn chrome_model_caps_at_max_visible_lines() {
        let log = make_log();
        {
            let mut g = log.lock().unwrap();
            for i in 0..MAX_VISIBLE_LINES + 5 {
                g.record_started("GET", &format!("https://a.com/{i}"));
                g.record_completed(&format!("https://a.com/{i}"), 200);
            }
        }
        let mut p = NetworkPanel::new(log);
        p.refresh();
        let m = p.chrome_model();
        assert_eq!(m.rows.len(), MAX_VISIBLE_LINES);
        assert_eq!(m.total, MAX_VISIBLE_LINES + 5);
        assert!(m.rows.last().unwrap().url.ends_with(&format!("/{}", MAX_VISIBLE_LINES + 4)));
        p.scroll_up(5);
        assert!(p.chrome_model().rows.last().unwrap().url.ends_with(&format!("/{}", MAX_VISIBLE_LINES - 1)));
    }

    // ── Helpers ────────────────────────────────────────────────────────────────

    #[test]
    fn status_class_buckets() {
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
        assert_eq!(status_class(&mk(Some(204), false, false)), 2);
        assert_eq!(status_class(&mk(Some(301), false, false)), 3);
        assert_eq!(status_class(&mk(Some(404), false, false)), 4);
        assert_eq!(status_class(&mk(None, false, false)), 0);
        assert_eq!(status_class(&mk(None, true, false)), 4);
        assert_eq!(status_class(&mk(None, false, true)), 4);
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
}
