//! Per-site permission popover (7C.2): floating panel anchored below the tab
//! bar on the left side of the window (where a lock icon would sit).
//!
//! Models the allow/deny/ask state for four browser permissions — Camera,
//! Microphone, Notifications, Clipboard — for the current page origin. With a
//! store attached ([`PermissionPanel::with_store`], UX-PERMISSIONS) decisions
//! persist across sessions in `lumen_storage::Permissions`; without one the
//! state is in-memory only.
//!
//! The legacy display-list renderer was removed in CC-15-4 — under the engine
//! chrome the rows live in `#permPopover`. The frozen design gave it only two
//! (Camera, Microphone), which left Notifications/Clipboard unreachable from
//! the UI until `BUG-411` added the missing two rows to the reference; all four
//! of `PermissionKind::ALL` are now bound, in that order. `hit_test` is kept
//! (still called ungated, a `BUG-404` site).
//!
//! Toggled with `Ctrl+Shift+P`.

use std::collections::HashMap;
use std::sync::Arc;

use lumen_storage::{PermissionKind as StoreKind, PermissionState as StoreState, Permissions};

// ── Visual constants ─────────────────────────────────────────────────────────

/// Height of the header row (origin + close button).
const HEADER_H: f32 = 28.0;
/// Height of each permission row.
const ROW_H: f32 = 30.0;
/// Horizontal padding inside the panel.
const PAD_X: f32 = 10.0;

const BTN_W: f32 = 54.0;

// ── Permission types ──────────────────────────────────────────────────────────

/// A single browser permission kind tracked by the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionKind {
    /// Camera / video capture.
    Camera,
    /// Microphone / audio capture.
    Microphone,
    /// Desktop notifications (`Notification.requestPermission()`).
    Notifications,
    /// Clipboard read/write access.
    Clipboard,
}

impl PermissionKind {
    /// All four permission kinds in display order.
    pub const ALL: [PermissionKind; 4] = [
        PermissionKind::Camera,
        PermissionKind::Microphone,
        PermissionKind::Notifications,
        PermissionKind::Clipboard,
    ];

    fn to_store(self) -> StoreKind {
        match self {
            Self::Camera => StoreKind::Camera,
            Self::Microphone => StoreKind::Microphone,
            Self::Notifications => StoreKind::Notifications,
            Self::Clipboard => StoreKind::Clipboard,
        }
    }

    fn from_store(k: &StoreKind) -> Option<Self> {
        match k {
            StoreKind::Camera => Some(Self::Camera),
            StoreKind::Microphone => Some(Self::Microphone),
            StoreKind::Notifications => Some(Self::Notifications),
            StoreKind::Clipboard => Some(Self::Clipboard),
            _ => None,
        }
    }

    /// Permission name the page-side prompt queue uses for this kind
    /// (`navigator.permissions` names; clipboard is the read side).
    pub fn page_name(self) -> &'static str {
        match self {
            Self::Camera => "camera",
            Self::Microphone => "microphone",
            Self::Notifications => "notifications",
            Self::Clipboard => "clipboard-read",
        }
    }

    /// Inverse of [`Self::page_name`].
    pub fn from_page_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.page_name() == name)
    }

    /// Human-readable label for the settings table.
    pub fn label(self) -> &'static str {
        match self {
            Self::Camera => "Camera",
            Self::Microphone => "Microphone",
            Self::Notifications => "Notifications",
            Self::Clipboard => "Clipboard",
        }
    }
}

/// Grant state for a single permission on a single origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionState {
    /// The page may use this capability without a user prompt.
    Allow,
    /// The capability is blocked; no prompt is shown.
    Deny,
    /// Default: the browser will prompt the user when the capability is first
    /// requested.
    #[default]
    Ask,
}

impl PermissionState {
    /// Cycle to the next state: Ask → Allow → Deny → Ask.
    pub fn cycle(self) -> Self {
        match self {
            PermissionState::Ask => PermissionState::Allow,
            PermissionState::Allow => PermissionState::Deny,
            PermissionState::Deny => PermissionState::Ask,
        }
    }
}

// ── Panel state ───────────────────────────────────────────────────────────────

/// Per-site permission popover state (7C.2).
pub struct PermissionPanel {
    /// `true` while the floating panel is visible.  Toggled via Ctrl+Shift+P.
    pub visible: bool,
    /// Origin of the currently loaded page (e.g. `"https://example.com"`).
    ///
    /// `None` while no page is loaded or for `file:` URLs.
    pub current_origin: Option<String>,
    /// Stored permission grants keyed by `(origin, kind)`.
    ///
    /// Defaults to [`PermissionState::Ask`] when the pair is absent.
    pub permissions: HashMap<(String, PermissionKind), PermissionState>,
    /// Persistent backing store; `None` = in-memory only.
    store: Option<Arc<Permissions>>,
    /// Kinds the page asked for and the user has not answered yet
    /// (UX-PERMISSIONS-2).
    pending: Vec<PermissionKind>,
}

impl PermissionPanel {
    /// Create a new hidden panel with no stored permissions.
    pub fn new() -> Self {
        Self {
            visible: false,
            current_origin: None,
            permissions: HashMap::new(),
            store: None,
            pending: Vec::new(),
        }
    }

    /// Attach a persistent store. Decisions made from now on are written
    /// through; the current origin's saved decisions are loaded.
    pub fn with_store(mut self, store: Arc<Permissions>) -> Self {
        self.store = Some(store);
        self.load_current_origin();
        self
    }

    /// Pull saved decisions for `current_origin` from the store into the cache.
    fn load_current_origin(&mut self) {
        let (Some(store), Some(origin)) = (self.store.clone(), self.current_origin.clone()) else {
            return;
        };
        for kind in PermissionKind::ALL {
            let state = match store.query(&origin, &kind.to_store(), now_unix()) {
                Ok(StoreState::Granted) => PermissionState::Allow,
                Ok(StoreState::Denied) => PermissionState::Deny,
                _ => PermissionState::Ask,
            };
            self.permissions.insert((origin.clone(), kind), state);
        }
    }

    /// Write one decision through to the store (`Ask` = revoke the record).
    fn persist(&self, origin: &str, kind: PermissionKind, state: PermissionState) {
        let Some(store) = &self.store else { return };
        let r = match state {
            PermissionState::Allow => store.set(origin, &kind.to_store(), StoreState::Granted, None),
            PermissionState::Deny => store.set(origin, &kind.to_store(), StoreState::Denied, None),
            PermissionState::Ask => store.revoke(origin, &kind.to_store()),
        };
        if let Err(e) = r {
            eprintln!("[lumen] permissions persist: {e}");
        }
    }

    /// All saved non-default decisions as `(origin, kind, state)`, for the
    /// `about:settings` table. Empty without a store.
    pub fn saved(&self) -> Vec<(String, PermissionKind, PermissionState)> {
        let Some(store) = &self.store else { return Vec::new() };
        let now = now_unix();
        store
            .list_all()
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.expires_at.is_none_or(|x| x > now))
            .filter_map(|e| {
                let kind = PermissionKind::from_store(&e.kind)?;
                let state = match e.state {
                    StoreState::Granted => PermissionState::Allow,
                    StoreState::Denied => PermissionState::Deny,
                    StoreState::Prompt => return None,
                };
                Some((e.origin, kind, state))
            })
            .collect()
    }

    /// Revoke a saved decision for any origin (settings table "revoke").
    pub fn revoke(&mut self, origin: &str, kind: PermissionKind) {
        self.permissions.remove(&(origin.to_string(), kind));
        self.persist(origin, kind, PermissionState::Ask);
    }

    /// Drop the in-memory decisions of every origin on `site`
    /// (UX-PARTITION «clear site data»; the store rows go via `clear_site`).
    pub fn forget_site(&mut self, site: &lumen_storage::PartitionKey) {
        self.permissions.retain(|(origin, _), _| !site.matches(origin));
    }

    /// The page asked for `kind`. A saved answer is returned at once;
    /// otherwise the request waits, the popover opens and `None` is returned —
    /// the answer comes from [`Self::answer`].
    pub fn request(&mut self, kind: PermissionKind) -> Option<PermissionState> {
        match self.state_for(kind) {
            PermissionState::Ask => {}
            decided => return Some(decided),
        }
        if self.current_origin.is_none() {
            return Some(PermissionState::Deny);
        }
        if !self.pending.contains(&kind) {
            self.pending.push(kind);
        }
        self.visible = true;
        None
    }

    /// Record the user's answer for `kind` and report whether a page request
    /// was waiting for it (the caller then settles the page's promise).
    pub fn answer(&mut self, kind: PermissionKind, state: PermissionState) -> bool {
        self.set_permission(kind, state);
        let before = self.pending.len();
        self.pending.retain(|k| *k != kind);
        self.pending.len() != before
    }

    /// Flip panel visibility. Hiding the popover dismisses pending requests
    /// (see [`Self::close`]); the returned kinds are the same.
    pub fn toggle(&mut self) -> Vec<PermissionKind> {
        if self.visible {
            return self.close();
        }
        self.visible = true;
        Vec::new()
    }

    /// Hide the popover. A page request still waiting for an answer is
    /// dropped; returns the kinds that were waiting, so the caller settles the
    /// page's promises as `default` (dismissed).
    pub fn close(&mut self) -> Vec<PermissionKind> {
        self.visible = false;
        std::mem::take(&mut self.pending)
    }

    /// Swap the backing store (the Anonymous profile gets an in-memory one so
    /// nothing it decides reaches the disk). The cache is rebuilt from it.
    pub fn set_store(&mut self, store: Arc<Permissions>) {
        self.store = Some(store);
        self.permissions.clear();
        self.pending.clear();
        self.load_current_origin();
    }

    /// Update the current origin on navigation (does not clear stored grants).
    pub fn set_origin(&mut self, origin: Option<String>) {
        self.current_origin = origin;
        // A request waiting for an answer belonged to the page we left.
        self.pending.clear();
        self.load_current_origin();
    }

    /// Return the stored state for `kind` at the current origin.
    ///
    /// Returns [`PermissionState::Ask`] when no grant has been recorded.
    pub fn state_for(&self, kind: PermissionKind) -> PermissionState {
        let Some(ref origin) = self.current_origin else {
            return PermissionState::Ask;
        };
        self.permissions
            .get(&(origin.clone(), kind))
            .copied()
            .unwrap_or_default()
    }

    /// Cycle the state for `kind` at the current origin to the next value.
    ///
    /// Does nothing if `current_origin` is `None`.
    pub fn cycle_permission(&mut self, kind: PermissionKind) {
        let Some(ref origin) = self.current_origin.clone() else {
            return;
        };
        let current = self
            .permissions
            .get(&(origin.clone(), kind))
            .copied()
            .unwrap_or_default();
        let next = current.cycle();
        self.permissions.insert((origin.clone(), kind), next);
        self.persist(origin, kind, next);
    }

    /// Set the state for `kind` at the current origin directly (CC-9's
    /// engine-rendered popover has two distinct allow/deny buttons — unlike
    /// the legacy panel's single [`Self::cycle_permission`] toggle button,
    /// there's no "ask" control to cycle back to, so this sets the state a
    /// click actually asked for instead of advancing a cycle).
    ///
    /// Does nothing if `current_origin` is `None`.
    pub fn set_permission(&mut self, kind: PermissionKind, state: PermissionState) {
        let Some(ref origin) = self.current_origin.clone() else {
            return;
        };
        self.permissions.insert((origin.clone(), kind), state);
        self.persist(origin, kind, state);
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Default for PermissionPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Result of a click inside the permission panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionHit {
    /// User clicked the toggle button for the given permission kind.
    Toggle(PermissionKind),
    /// User closed the panel (clicked the "×").
    Close,
    /// Clicked inside the panel but on a non-interactive area.
    Empty,
}

/// Hit-test a click at CSS-px `(x, y)` against the permission panel.
///
/// Returns `None` when the click is outside the panel. BUG-461:
/// `popover_rect` is `#permPopover`'s real measured layout rect
/// (`Lumen::chrome_perm_popover_rect`) — the shared engine-drawn popover
/// this panel's controls now render into — not a guess anchored to the
/// window's left edge/`toolbar::CHROME_H`, which drifted from where
/// `chrome.css` actually places it (nested under `.omnibox-wrap`, anchored
/// to the omnibox/shield button, not the window). The internal
/// header/row/button proportions below stay a heuristic (the real markup's
/// shield-stats block and allow/deny button pair don't match this panel's
/// single-toggle-per-row model — a residual noted in the bug file, not
/// fixed here), but the outer bound now matches the real box instead of a
/// stale corner of the window. A zero-sized `popover_rect` (no chrome
/// layout yet) safely matches nothing.
pub fn hit_test(_panel: &PermissionPanel, x: f32, y: f32, popover_rect: lumen_core::geom::Rect) -> Option<PermissionHit> {
    let (px, py, pw, ph) = (popover_rect.x, popover_rect.y, popover_rect.width, popover_rect.height);
    if x < px || x >= px + pw || y < py || y >= py + ph {
        return None;
    }

    let rel_x = x - px;
    let rel_y = y - py;

    // Close button: top-right 20×20 area of the header.
    if rel_x >= pw - 20.0 && rel_y < HEADER_H {
        return Some(PermissionHit::Close);
    }

    // Permission rows — each is ROW_H tall starting at HEADER_H.
    for (i, kind) in PermissionKind::ALL.iter().enumerate() {
        let row_top = HEADER_H + i as f32 * ROW_H;
        let row_bot = row_top + ROW_H;
        if rel_y >= row_top && rel_y < row_bot {
            // Toggle button: right side of the row.
            let btn_x = pw - PAD_X - BTN_W;
            if rel_x >= btn_x && rel_x < btn_x + BTN_W {
                return Some(PermissionHit::Toggle(*kind));
            }
            return Some(PermissionHit::Empty);
        }
    }

    Some(PermissionHit::Empty)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_panel(origin: Option<&str>) -> PermissionPanel {
        let mut p = PermissionPanel::new();
        p.visible = true;
        p.current_origin = origin.map(|s| s.to_owned());
        p
    }

    const TAB_H: f32 = 36.0;

    // ── PermissionState ──────────────────────────────────────────────────────

    #[test]
    fn state_cycle_ask_to_allow() {
        assert_eq!(PermissionState::Ask.cycle(), PermissionState::Allow);
    }

    #[test]
    fn state_cycle_allow_to_deny() {
        assert_eq!(PermissionState::Allow.cycle(), PermissionState::Deny);
    }

    #[test]
    fn state_cycle_deny_to_ask() {
        assert_eq!(PermissionState::Deny.cycle(), PermissionState::Ask);
    }

    // ── PermissionPanel ──────────────────────────────────────────────────────

    #[test]
    fn new_panel_hidden() {
        let p = PermissionPanel::new();
        assert!(!p.visible);
    }

    #[test]
    fn toggle_shows_panel() {
        let mut p = PermissionPanel::new();
        p.toggle();
        assert!(p.visible);
    }

    #[test]
    fn double_toggle_hides() {
        let mut p = PermissionPanel::new();
        p.toggle();
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn default_state_is_ask() {
        let p = make_panel(Some("https://example.com"));
        assert_eq!(p.state_for(PermissionKind::Camera), PermissionState::Ask);
    }

    #[test]
    fn cycle_permission_advances_state() {
        let mut p = make_panel(Some("https://example.com"));
        p.cycle_permission(PermissionKind::Camera);
        assert_eq!(p.state_for(PermissionKind::Camera), PermissionState::Allow);
        p.cycle_permission(PermissionKind::Camera);
        assert_eq!(p.state_for(PermissionKind::Camera), PermissionState::Deny);
        p.cycle_permission(PermissionKind::Camera);
        assert_eq!(p.state_for(PermissionKind::Camera), PermissionState::Ask);
    }

    #[test]
    fn cycle_without_origin_is_noop() {
        let mut p = make_panel(None);
        p.cycle_permission(PermissionKind::Microphone);
        // Still Ask (no origin → no entry stored).
        assert_eq!(p.state_for(PermissionKind::Microphone), PermissionState::Ask);
    }

    #[test]
    fn permissions_are_per_kind() {
        let mut p = make_panel(Some("https://example.com"));
        p.cycle_permission(PermissionKind::Camera);
        // Microphone should still be Ask.
        assert_eq!(p.state_for(PermissionKind::Microphone), PermissionState::Ask);
    }

    #[test]
    fn set_origin_does_not_clear_stored_grants() {
        let mut p = make_panel(Some("https://example.com"));
        p.cycle_permission(PermissionKind::Notifications);
        p.set_origin(Some("https://other.com".to_owned()));
        p.set_origin(Some("https://example.com".to_owned()));
        assert_eq!(
            p.state_for(PermissionKind::Notifications),
            PermissionState::Allow
        );
    }

    // ── Hit-testing (BUG-461: against a measured popover rect) ────────────────

    fn test_popover_rect() -> lumen_core::geom::Rect {
        lumen_core::geom::Rect::new(700.0, TAB_H + 4.0, 240.0, 200.0)
    }

    #[test]
    fn hit_outside_panel_returns_none() {
        let p = make_panel(Some("https://example.com"));
        // Far top-left, well outside the measured popover rect.
        assert_eq!(hit_test(&p, 0.0, TAB_H + 2.0, test_popover_rect()), None);
    }

    #[test]
    fn hit_outside_zero_rect_returns_none() {
        // No chrome layout yet — a zero-sized rect must match nothing.
        let p = make_panel(Some("https://example.com"));
        assert_eq!(hit_test(&p, 0.0, 0.0, lumen_core::geom::Rect::ZERO), None);
    }

    #[test]
    fn hit_close_button() {
        let p = make_panel(Some("https://example.com"));
        let r = test_popover_rect();
        let hit = hit_test(&p, r.x + r.width - 5.0, r.y + 5.0, r);
        assert_eq!(hit, Some(PermissionHit::Close));
    }

    #[test]
    fn hit_first_toggle_button() {
        let p = make_panel(Some("https://example.com"));
        let r = test_popover_rect();
        let btn_x = r.x + r.width - PAD_X - BTN_W + BTN_W / 2.0;
        let btn_y = r.y + HEADER_H + ROW_H / 2.0;
        let hit = hit_test(&p, btn_x, btn_y, r);
        assert_eq!(hit, Some(PermissionHit::Toggle(PermissionKind::Camera)));
    }

    #[test]
    fn hit_second_toggle_button() {
        let p = make_panel(Some("https://example.com"));
        let r = test_popover_rect();
        let btn_x = r.x + r.width - PAD_X - BTN_W + BTN_W / 2.0;
        let btn_y = r.y + HEADER_H + ROW_H + ROW_H / 2.0;
        let hit = hit_test(&p, btn_x, btn_y, r);
        assert_eq!(hit, Some(PermissionHit::Toggle(PermissionKind::Microphone)));
    }

    #[test]
    fn hit_row_label_returns_empty() {
        let p = make_panel(Some("https://example.com"));
        let r = test_popover_rect();
        // Click the label area (left side of row), not the button.
        let hit = hit_test(&p, r.x + 30.0, r.y + HEADER_H + 15.0, r);
        assert_eq!(hit, Some(PermissionHit::Empty));
    }

    #[test]
    fn request_without_decision_waits_and_opens_popover() {
        let mut p = make_panel(Some("https://a.test"));
        p.visible = false;
        assert_eq!(p.request(PermissionKind::Notifications), None);
        assert!(p.visible);
        assert_eq!(p.pending, &[PermissionKind::Notifications]);
        assert!(p.answer(PermissionKind::Notifications, PermissionState::Allow));
        assert!(p.pending.is_empty());
        assert_eq!(p.request(PermissionKind::Notifications), Some(PermissionState::Allow));
    }

    #[test]
    fn page_names_round_trip_and_close_reports_kinds() {
        for k in PermissionKind::ALL {
            assert_eq!(PermissionKind::from_page_name(k.page_name()), Some(k));
        }
        let mut p = make_panel(Some("https://a.test"));
        assert_eq!(p.request(PermissionKind::Microphone), None);
        assert_eq!(p.close(), vec![PermissionKind::Microphone]);
        assert!(p.close().is_empty());
    }

    #[test]
    fn request_without_origin_is_denied() {
        let mut p = make_panel(None);
        assert_eq!(p.request(PermissionKind::Notifications), Some(PermissionState::Deny));
    }

    #[test]
    fn answer_without_request_reports_false() {
        let mut p = make_panel(Some("https://a.test"));
        assert!(!p.answer(PermissionKind::Camera, PermissionState::Deny));
    }

    #[test]
    fn decisions_persist_across_panels_and_revoke() {
        let store = Arc::new(Permissions::open_in_memory().unwrap());
        let mut a = PermissionPanel::new().with_store(store.clone());
        a.set_origin(Some("https://a.test".into()));
        a.set_permission(PermissionKind::Camera, PermissionState::Allow);
        a.set_permission(PermissionKind::Clipboard, PermissionState::Deny);

        let mut b = PermissionPanel::new().with_store(store);
        b.set_origin(Some("https://a.test".into()));
        assert_eq!(b.state_for(PermissionKind::Camera), PermissionState::Allow);
        assert_eq!(b.state_for(PermissionKind::Clipboard), PermissionState::Deny);
        assert_eq!(b.saved().len(), 2);

        b.revoke("https://a.test", PermissionKind::Camera);
        assert_eq!(b.state_for(PermissionKind::Camera), PermissionState::Ask);
        assert_eq!(b.saved().len(), 1);
    }
}
