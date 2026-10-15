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
//! of `PermissionKind::ALL` are now bound, in that order. Clicks on the allow/deny buttons are read from the
//! chrome hit-test (`data-action="set-permission"`).
//!
//! Toggled with `Ctrl+Shift+P`.

use std::collections::HashMap;
use std::sync::Arc;

use lumen_storage::{PermissionKind as StoreKind, PermissionState as StoreState, Permissions};

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
    /// Pop-ups opened by `window.open()` without a user gesture
    /// (UX-USER-ACTIVATION). `Allow` lets the site open them freely.
    Popups,
}

impl PermissionKind {
    /// All permission kinds in display order.
    pub const ALL: [PermissionKind; 5] = [
        PermissionKind::Camera,
        PermissionKind::Microphone,
        PermissionKind::Notifications,
        PermissionKind::Clipboard,
        PermissionKind::Popups,
    ];

    fn to_store(self) -> StoreKind {
        match self {
            Self::Camera => StoreKind::Camera,
            Self::Microphone => StoreKind::Microphone,
            Self::Notifications => StoreKind::Notifications,
            Self::Clipboard => StoreKind::Clipboard,
            Self::Popups => StoreKind::Other("popups".to_owned()),
        }
    }

    fn from_store(k: &StoreKind) -> Option<Self> {
        match k {
            StoreKind::Camera => Some(Self::Camera),
            StoreKind::Microphone => Some(Self::Microphone),
            StoreKind::Notifications => Some(Self::Notifications),
            StoreKind::Clipboard => Some(Self::Clipboard),
            StoreKind::Other(name) if name == "popups" => Some(Self::Popups),
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
            Self::Popups => "popups",
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
            Self::Popups => "Pop-ups",
        }
    }
}

/// Permission key of a page: `scheme://host` in lower case, without port and
/// path — what the panel and the store are keyed by. `None` for a URL with no
/// host (`about:blank`, `data:`).
pub fn permission_origin(url: &str) -> Option<String> {
    let scheme_end = url.find("://")?;
    let scheme = &url[..scheme_end + 3];
    let rest = &url[scheme_end + 3..];
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..host_end];
    let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
    if host.is_empty() { None } else { Some(format!("{}{}", scheme, host.to_ascii_lowercase())) }
}

/// Lookup the JS gate uses (`lumen_js::popup_policy::set_site_resolver`):
/// did the user allow `kind` (a store name: `popups`, `clipboard`) for the
/// document at `origin` (`location.origin`, with port)?
pub fn site_allows(store: &Permissions, origin: &str, kind: &str) -> bool {
    let Some(key) = permission_origin(origin) else { return false };
    matches!(store.query(&key, &StoreKind::parse(kind), now_unix()), Ok(StoreState::Granted))
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

    /// Set the state for `kind` at the current origin directly (CC-9's
    /// engine-rendered popover has two distinct allow/deny buttons and no
    /// "ask" control).
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

    #[test]
    fn permission_origin_drops_port_path_and_case() {
        assert_eq!(permission_origin("https://Example.COM:8443/a?b#c").as_deref(), Some("https://example.com"));
        assert_eq!(permission_origin("https://example.com").as_deref(), Some("https://example.com"));
        assert_eq!(permission_origin("about:blank"), None);
        assert_eq!(permission_origin("null"), None);
    }

    #[test]
    fn popups_row_is_persisted_and_visible_to_the_gate() {
        let store = Arc::new(Permissions::open_in_memory().unwrap());
        let mut p = PermissionPanel::new().with_store(store.clone());
        p.set_origin(Some("https://a.test".to_owned()));
        assert!(!site_allows(&store, "https://a.test:81", "popups"), "default is ask");
        p.set_permission(PermissionKind::Popups, PermissionState::Allow);
        assert!(site_allows(&store, "https://a.test:81", "popups"), "allowed, port ignored");
        assert!(!site_allows(&store, "https://a.test", "clipboard"), "other kinds are separate");
        assert!(!site_allows(&store, "https://b.test", "popups"), "other sites are separate");
        // A new panel over the same store reloads it.
        let mut q = PermissionPanel::new().with_store(store);
        q.set_origin(Some("https://a.test".to_owned()));
        assert_eq!(q.state_for(PermissionKind::Popups), PermissionState::Allow);
        q.revoke("https://a.test", PermissionKind::Popups);
        assert_eq!(q.state_for(PermissionKind::Popups), PermissionState::Ask);
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
    fn permissions_are_per_kind() {
        let mut p = make_panel(Some("https://example.com"));
        p.set_permission(PermissionKind::Camera, PermissionState::Allow);
        // Microphone should still be Ask.
        assert_eq!(p.state_for(PermissionKind::Microphone), PermissionState::Ask);
    }

    #[test]
    fn set_origin_does_not_clear_stored_grants() {
        let mut p = make_panel(Some("https://example.com"));
        p.set_permission(PermissionKind::Notifications, PermissionState::Allow);
        p.set_origin(Some("https://other.com".to_owned()));
        p.set_origin(Some("https://example.com".to_owned()));
        assert_eq!(
            p.state_for(PermissionKind::Notifications),
            PermissionState::Allow
        );
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
