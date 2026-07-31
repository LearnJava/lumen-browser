//! Tab strip: per-tab metadata.
//!
//! `TabStrip` holds the list of open tabs and the active index. Its own
//! CSS-px hit-testing (`hit_test`/`TabHit`) was removed in BUG-404 — the
//! last live caller (tab right-click context menu) now resolves the tab
//! under the cursor via the engine-drawn chrome's `data-tab-id` instead
//! (`Lumen::chrome_hit_test` + `chrome_data_id`, `crates/shell/src/main.rs`),
//! same mechanism `ChromeAction::SelectTab` uses for a left-click.

use crate::tab_lifecycle::state::TabState;
use crate::tabs::containers::ContainerKind;
use crate::tabs::groups::{GroupColor, TabGroup};

// ── Visual constants ──────────────────────────────────────────────────────────

/// Height of the tab bar in CSS px. Subtracted from `viewport_height_css()`.
pub const TAB_BAR_HEIGHT: f32 = 36.0;

/// Pixels the cursor must travel before a press becomes a drag.
pub const DRAG_THRESHOLD: f32 = 6.0;

/// Width of the vertical-tab layout toggle button in CSS px.
/// Rendered at the right edge of the tab strip, between the tabs and the
/// archive button.
pub const LAYOUT_BTN_W: f32 = 28.0;

/// Width of the settings gear button in CSS px.
/// Rendered between the tabs and the layout-toggle button (opens
/// `about:settings`, mirrors [`LAYOUT_BTN_W`]'s geometry).
pub const SETTINGS_BTN_W: f32 = 28.0;

/// Minimum tab button width in CSS px.
const TAB_MIN_W: f32 = 80.0;
/// Maximum tab button width in CSS px.
const TAB_MAX_W: f32 = 200.0;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Metadata for one browser tab.
pub struct TabEntry {
    /// Stable unique identifier, never reused within a session.
    pub id: usize,
    /// Display title shown in the tab button.
    pub title: String,
    /// Current lifecycle tier for this tab.
    ///
    /// `Active` — foreground tab, no badge rendered.
    /// `BackgroundOld` — amber "z" badge + dimmed background (fade-opacity T2).
    /// `Hibernated` — grey "Z" badge + darker background (fade-opacity T3).
    /// Other tiers — no badge rendered.
    pub tab_state: TabState,
    /// ID of the tab that opened this one, or `None` for root (top-level) tabs.
    ///
    /// Forms the parent-child tree used by tree-style tabs (7A.2).
    /// Depth is computed by walking this chain upward. Cycles are impossible
    /// because `opener_id` is set once at creation and always points to an
    /// already-existing tab.
    pub opener_id: Option<usize>,
    /// Container assigned to this tab (7D.2). Drives the left accent strip
    /// (DS-13) rendered on the tab and the cookie/storage isolation key.
    ///
    /// Default `ContainerKind::None` — no container, shared state. New
    /// tabs inherit `None`; the user changes containers via the shell's
    /// `set_tab_container` API.
    pub container: ContainerKind,
    /// Session-elapsed milliseconds when this tab was last made active.
    ///
    /// Set to `now_ms` on tab creation and on every activation via
    /// `update_last_activated`. The auto-archive tick (7A.5) compares this
    /// against `ARCHIVE_AFTER_MS` to decide whether a background tab should
    /// be moved to [`crate::tabs::archive::TabArchive`].
    pub last_activated_ms: f64,
    /// Whether the tab is pinned (CC-4). Pinned tabs survive the context-menu
    /// "Close others" / "Close to the right" bulk operations. Default `false`.
    pub pinned: bool,
    /// Id of the [`TabGroup`] this tab belongs to (CC-6), or `None` when the
    /// tab is ungrouped. Drives collapse visibility (see [`TabStrip::visible_indices`]).
    pub group_id: Option<usize>,
    /// Whether the built-in ad/tracker request filter is active for this tab.
    ///
    /// Per-tab and independent: toggled by the checkbox rendered inside the tab
    /// (at its left edge). Synced into the process-global toggle
    /// (`lumen_network::set_global_adblock_enabled`) when the tab becomes active
    /// or its checkbox is flipped, so the filter that governs the tab's page
    /// fetches reflects this flag. Default `false`.
    pub adblock: bool,
}

/// State of the tab strip (tab list + active index).
pub struct TabStrip {
    /// Open tabs, in left-to-right order.
    pub tabs: Vec<TabEntry>,
    /// Index of the currently-visible tab.
    pub active: usize,
    /// Counter for generating fresh `TabEntry::id` values.
    pub(crate) next_id: usize,
    /// Tab groups (CC-6), keyed by `TabGroup::id`. Order is creation order.
    pub groups: Vec<TabGroup>,
    /// Counter for generating fresh `TabGroup::id` values.
    pub(crate) next_group_id: usize,
}

impl TabStrip {
    /// Create the initial tab strip with one blank tab.
    pub fn new() -> Self {
        Self {
            tabs: vec![TabEntry {
                id: 0,
                title: "Новая вкладка".to_owned(),
                tab_state: TabState::Active,
                opener_id: None,
                container: ContainerKind::None,
                last_activated_ms: 0.0,
                pinned: false,
                group_id: None,
                adblock: false,
            }],
            active: 0,
            next_id: 1,
            groups: Vec::new(),
            next_group_id: 0,
        }
    }

    /// Number of open tabs.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Append a new blank tab and return its index.
    ///
    /// `now_ms` — current session-elapsed milliseconds, stored as
    /// `last_activated_ms` so the auto-archive timer starts from creation time.
    pub fn push_blank(&mut self, now_ms: f64) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(TabEntry {
            id,
            title: "Новая вкладка".to_owned(),
            tab_state: TabState::Active,
            opener_id: None,
            container: ContainerKind::None,
            last_activated_ms: now_ms,
            pinned: false,
            group_id: None,
            adblock: false,
        });
        self.tabs.len() - 1
    }

    /// Append a new blank child tab opened by the tab with `opener_id`.
    ///
    /// Sets `TabEntry::opener_id` so tree-style tab rendering can indent and
    /// group this tab under its parent. Returns the new tab's strip index.
    ///
    /// `now_ms` — current session-elapsed milliseconds (same semantics as
    /// [`push_blank`]).
    pub fn push_with_opener(&mut self, opener_id: usize, now_ms: f64) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(TabEntry {
            id,
            title: "Новая вкладка".to_owned(),
            tab_state: TabState::Active,
            opener_id: Some(opener_id),
            container: ContainerKind::None,
            last_activated_ms: now_ms,
            pinned: false,
            group_id: None,
            adblock: false,
        });
        self.tabs.len() - 1
    }

    /// Record `now_ms` as the activation timestamp for the tab at `idx`.
    ///
    /// Call on every tab switch so the auto-archive timer resets for the
    /// newly-active tab and advances for all background tabs.
    pub fn update_last_activated(&mut self, idx: usize, now_ms: f64) {
        if let Some(tab) = self.tabs.get_mut(idx) {
            tab.last_activated_ms = now_ms;
        }
    }

    /// Assign `container` to the tab at `idx`. Out-of-bounds index is a no-op.
    ///
    /// Cookie/storage isolation rewiring is the caller's responsibility (see
    /// `ContainerStore::get_or_create`).
    pub fn set_tab_container(&mut self, idx: usize, container: ContainerKind) {
        if let Some(tab) = self.tabs.get_mut(idx) {
            tab.container = container;
        }
    }

    /// Remove the tab at `idx`. Returns the new active index (clamped to valid
    /// range). Caller must guard against removing the only tab (check `len() > 1`).
    pub fn remove(&mut self, idx: usize) -> usize {
        self.tabs.remove(idx);
        let new_active = if self.active >= self.tabs.len() {
            self.tabs.len().saturating_sub(1)
        } else {
            self.active
        };
        self.active = new_active;
        new_active
    }

    /// Update the title of the active tab.
    pub fn set_active_title(&mut self, title: impl Into<String>) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.title = title.into();
        }
    }

    /// Update the lifecycle state of the tab at `idx`.
    ///
    /// Called by the shell on tab switch (`Active` ↔ `BackgroundRecent`) and by
    /// the lifecycle manager on idle-timeout or memory-pressure transitions.
    pub fn set_tab_state(&mut self, idx: usize, state: TabState) {
        if let Some(tab) = self.tabs.get_mut(idx) {
            tab.tab_state = state;
        }
    }

    /// Reorder: move the tab currently at `src` so that it ends up at `dst`.
    ///
    /// Out-of-bounds indices and `src == dst` are no-ops.  `active` is updated
    /// so the same logical tab remains selected after the move.
    pub fn move_tab(&mut self, src: usize, dst: usize) {
        if src == dst || src >= self.tabs.len() || dst >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(src);
        self.tabs.insert(dst, tab);
        self.active = if self.active == src {
            dst
        } else if src < dst && src < self.active && self.active <= dst {
            self.active - 1
        } else if src > dst && dst <= self.active && self.active < src {
            self.active + 1
        } else {
            self.active
        };
    }

    /// Toggle the pinned flag of the tab at `idx`. Returns the new state
    /// (`false` for an out-of-bounds index, which is a no-op).
    pub fn toggle_pin(&mut self, idx: usize) -> bool {
        if let Some(tab) = self.tabs.get_mut(idx) {
            tab.pinned = !tab.pinned;
            tab.pinned
        } else {
            false
        }
    }

    /// `true` if the tab at `idx` is pinned. Out-of-bounds → `false`.
    pub fn is_pinned(&self, idx: usize) -> bool {
        self.tabs.get(idx).is_some_and(|t| t.pinned)
    }

    /// Insert a duplicate of the tab at `src` immediately to its right.
    ///
    /// The clone gets a fresh `id`, inherits the title/container, sets
    /// `opener_id` to the source tab's id, and is never pinned. Returns the
    /// new tab's index, or `None` for an out-of-bounds `src`. `active` shifts
    /// right if the insertion happened at or before it (the same logical tab
    /// stays selected). The caller is responsible for cloning the page content.
    pub fn duplicate(&mut self, src: usize, now_ms: f64) -> Option<usize> {
        let source = self.tabs.get(src)?;
        let id = self.next_id;
        self.next_id += 1;
        let clone = TabEntry {
            id,
            title: source.title.clone(),
            tab_state: TabState::Active,
            opener_id: Some(source.id),
            container: source.container,
            last_activated_ms: now_ms,
            pinned: false,
            group_id: source.group_id,
            adblock: source.adblock,
        };
        let dst = src + 1;
        self.tabs.insert(dst, clone);
        if dst <= self.active {
            self.active += 1;
        }
        Some(dst)
    }

    /// Remove every tab except `keep_idx` and any pinned tabs.
    ///
    /// Returns the ids of the removed tabs (so the shell can drop their cached
    /// page snapshots). `active` is set to the surviving `keep` tab. Pinned
    /// tabs are preserved regardless of position.
    pub fn close_others(&mut self, keep_idx: usize) -> Vec<usize> {
        let Some(keep_id) = self.tabs.get(keep_idx).map(|t| t.id) else {
            return Vec::new();
        };
        let mut removed = Vec::new();
        self.tabs.retain(|t| {
            let keep = t.id == keep_id || t.pinned;
            if !keep {
                removed.push(t.id);
            }
            keep
        });
        self.active = self
            .tabs
            .iter()
            .position(|t| t.id == keep_id)
            .unwrap_or(0);
        removed
    }

    /// Remove all non-pinned tabs positioned to the right of `idx`.
    ///
    /// Returns the ids of the removed tabs. `active` is clamped into the new
    /// valid range if it pointed at a removed tab. Pinned tabs to the right
    /// are preserved.
    pub fn close_right(&mut self, idx: usize) -> Vec<usize> {
        if idx >= self.tabs.len() {
            return Vec::new();
        }
        let active_id = self.tabs.get(self.active).map(|t| t.id);
        let mut removed = Vec::new();
        let mut pos = 0usize;
        self.tabs.retain(|t| {
            let keep = pos <= idx || t.pinned;
            pos += 1;
            if !keep {
                removed.push(t.id);
            }
            keep
        });
        // Re-resolve active: if it survived, point at it; else clamp to `idx`.
        self.active = active_id
            .and_then(|aid| self.tabs.iter().position(|t| t.id == aid))
            .unwrap_or_else(|| idx.min(self.tabs.len().saturating_sub(1)));
        removed
    }

    // ── Tab groups (CC-6) ───────────────────────────────────────────────────

    /// Create a new expanded [`TabGroup`] with `label` and `color`.
    /// Returns the fresh group id. Does not assign any tabs to it.
    pub fn create_group(&mut self, label: impl Into<String>, color: GroupColor) -> usize {
        let id = self.next_group_id;
        self.next_group_id += 1;
        self.groups.push(TabGroup::new(id, label, color));
        id
    }

    /// Borrow the group with the given id, if it exists.
    #[must_use]
    pub fn group(&self, id: usize) -> Option<&TabGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// The group id of the tab at `idx`, or `None` when ungrouped / out of bounds.
    #[must_use]
    pub fn group_of(&self, idx: usize) -> Option<usize> {
        self.tabs.get(idx).and_then(|t| t.group_id)
    }

    /// Assign the tab at `idx` to the group `group_id`.
    ///
    /// Returns `false` (a no-op) for an out-of-bounds tab index or an unknown
    /// group id; `true` on success.
    pub fn assign_to_group(&mut self, idx: usize, group_id: usize) -> bool {
        if self.group(group_id).is_none() {
            return false;
        }
        if let Some(tab) = self.tabs.get_mut(idx) {
            tab.group_id = Some(group_id);
            true
        } else {
            false
        }
    }

    /// Remove the tab at `idx` from its group (no-op if already ungrouped or
    /// out of bounds). The group itself is kept even if it becomes empty.
    pub fn ungroup(&mut self, idx: usize) {
        if let Some(tab) = self.tabs.get_mut(idx) {
            tab.group_id = None;
        }
    }

    /// Toggle the collapsed flag of the group `id`. Returns the new collapsed
    /// state (`false` for an unknown group, which is a no-op).
    pub fn toggle_collapse(&mut self, id: usize) -> bool {
        if let Some(g) = self.groups.iter_mut().find(|g| g.id == id) {
            g.collapsed = !g.collapsed;
            g.collapsed
        } else {
            false
        }
    }

    /// `true` if the group `id` exists and is collapsed.
    #[must_use]
    pub fn is_collapsed(&self, id: usize) -> bool {
        self.group(id).is_some_and(|g| g.collapsed)
    }

    /// The colour of the group `id`, or `None` for an unknown group.
    #[must_use]
    pub fn group_color(&self, id: usize) -> Option<GroupColor> {
        self.group(id).map(|g| g.color)
    }

    /// Strip indices of every tab in the group `id`, in left-to-right order.
    #[must_use]
    pub fn group_members(&self, id: usize) -> Vec<usize> {
        self.tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.group_id == Some(id))
            .map(|(i, _)| i)
            .collect()
    }

    /// Remove the group `id` and ungroup all of its member tabs. No-op if the
    /// group is unknown.
    pub fn remove_group(&mut self, id: usize) {
        for tab in &mut self.tabs {
            if tab.group_id == Some(id) {
                tab.group_id = None;
            }
        }
        self.groups.retain(|g| g.id != id);
    }

    /// Strip indices of the tabs that should be drawn, in order.
    ///
    /// Every tab is visible except members of a *collapsed* group other than
    /// that group's leftmost member, which stays as the collapsed-group chip.
    /// For a strip with no collapsed groups this is simply `0..tabs.len()`, so
    /// the ungrouped rendering path is unchanged.
    #[must_use]
    pub fn visible_indices(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.tabs.len());
        for (i, tab) in self.tabs.iter().enumerate() {
            if let Some(gid) = tab.group_id
                && self.is_collapsed(gid)
            {
                // Keep only the leftmost member of a collapsed group.
                let earlier_member = self.tabs[..i].iter().any(|t| t.group_id == Some(gid));
                if earlier_member {
                    continue;
                }
            }
            out.push(i);
        }
        out
    }
}

// ── Drag state ────────────────────────────────────────────────────────────────

/// State for an in-progress tab drag-and-drop.
///
/// Created when the user presses on a tab; transitions to `active` after the
/// cursor crosses [`DRAG_THRESHOLD`] CSS px.
pub struct TabDragState {
    /// Index of the tab being dragged.
    pub src_idx: usize,
    /// X position where the mouse was first pressed (CSS px).
    pub press_x: f32,
    /// Current cursor X (CSS px) — drives the drop-indicator position.
    pub ghost_x: f32,
    /// Whether the drag crossed the threshold and should be rendered visually.
    pub active: bool,
}

impl TabDragState {
    /// Compute the tab index where the dragged tab would be dropped if the
    /// mouse were released at the current [`ghost_x`].
    pub fn drop_target(&self, n_tabs: usize, window_w: f32) -> usize {
        if n_tabs == 0 { return 0; }
        let tab_w = (window_w / n_tabs as f32).clamp(TAB_MIN_W, TAB_MAX_W);
        let raw = (self.ghost_x / tab_w).round() as usize;
        raw.min(n_tabs.saturating_sub(1))
    }
}

/// Tab layout mode: horizontal strip or vertical sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabLayout {
    /// Horizontal strip at the top of the window (default).
    #[default]
    Horizontal,
    /// Vertical 200 px sidebar on the left (GG-4).
    Vertical,
}

impl TabLayout {
    /// Parse from a stored settings string (`"horizontal"` or `"vertical"`).
    pub fn from_str(s: &str) -> Self {
        if s == "vertical" { Self::Vertical } else { Self::Horizontal }
    }

    /// Serialize to a settings string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_strip_has_one_tab() {
        let s = TabStrip::new();
        assert_eq!(s.len(), 1);
        assert_eq!(s.active, 0);
    }

    #[test]
    fn new_tab_starts_active() {
        let s = TabStrip::new();
        assert_eq!(s.tabs[0].tab_state, TabState::Active);
    }

    #[test]
    fn push_blank_increments_len() {
        let mut s = TabStrip::new();
        let idx = s.push_blank(0.0);
        assert_eq!(idx, 1);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn push_blank_starts_active_state() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        assert_eq!(s.tabs[1].tab_state, TabState::Active);
    }

    #[test]
    fn remove_tab_clamps_active() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        s.push_blank(0.0);
        s.active = 2;
        let new_active = s.remove(2);
        assert_eq!(s.len(), 2);
        assert_eq!(new_active, 1);
    }

    #[test]
    fn set_active_title_updates() {
        let mut s = TabStrip::new();
        s.set_active_title("Rust Lang");
        assert_eq!(s.tabs[0].title, "Rust Lang");
    }

    #[test]
    fn set_tab_state_updates_entry() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        s.set_tab_state(0, TabState::BackgroundOld);
        assert_eq!(s.tabs[0].tab_state, TabState::BackgroundOld);
        assert_eq!(s.tabs[1].tab_state, TabState::Active);
    }

    #[test]
    fn set_tab_state_out_of_bounds_no_panic() {
        let mut s = TabStrip::new();
        s.set_tab_state(99, TabState::Hibernated); // must not panic
    }

    // ── Container strip tests (7D.2) ─────────────────────────────────────────

    #[test]
    fn new_tab_has_no_container() {
        let s = TabStrip::new();
        assert_eq!(s.tabs[0].container, ContainerKind::None);
    }

    #[test]
    fn push_blank_starts_without_container() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        assert_eq!(s.tabs[1].container, ContainerKind::None);
    }

    #[test]
    fn push_with_opener_starts_without_container() {
        let mut s = TabStrip::new();
        let opener_id = s.tabs[0].id;
        s.push_with_opener(opener_id, 0.0);
        assert_eq!(s.tabs[1].container, ContainerKind::None);
    }

    #[test]
    fn set_tab_container_updates_entry() {
        let mut s = TabStrip::new();
        s.set_tab_container(0, ContainerKind::Work);
        assert_eq!(s.tabs[0].container, ContainerKind::Work);
    }

    #[test]
    fn set_tab_container_out_of_bounds_no_panic() {
        let mut s = TabStrip::new();
        s.set_tab_container(99, ContainerKind::Personal); // must not panic
        assert_eq!(s.tabs[0].container, ContainerKind::None);
    }

    // ── move_tab tests ───────────────────────────────────────────────────────

    /// Helper: extract tab ids from the strip in order.
    fn ids(s: &TabStrip) -> Vec<usize> {
        s.tabs.iter().map(|t| t.id).collect()
    }

    fn strip_with_n(n: usize) -> TabStrip {
        let mut s = TabStrip::new(); // id=0
        for _ in 1..n { s.push_blank(0.0); }
        s
    }

    #[test]
    fn move_tab_forward() {
        let mut s = strip_with_n(5);
        // ids: [0,1,2,3,4], move id=1 (idx=1) to idx=3
        s.move_tab(1, 3);
        assert_eq!(ids(&s), vec![0, 2, 3, 1, 4]);
    }

    #[test]
    fn move_tab_backward() {
        let mut s = strip_with_n(5);
        // ids: [0,1,2,3,4], move id=3 (idx=3) to idx=1
        s.move_tab(3, 1);
        assert_eq!(ids(&s), vec![0, 3, 1, 2, 4]);
    }

    #[test]
    fn move_tab_same_index_noop() {
        let mut s = strip_with_n(3);
        s.move_tab(1, 1);
        assert_eq!(ids(&s), vec![0, 1, 2]);
    }

    #[test]
    fn move_tab_out_of_bounds_noop() {
        let mut s = strip_with_n(3);
        s.move_tab(0, 99);
        assert_eq!(ids(&s), vec![0, 1, 2]);
    }

    #[test]
    fn move_tab_active_tracks_src() {
        let mut s = strip_with_n(5);
        s.active = 1; // id=1
        s.move_tab(1, 3);
        assert_eq!(s.active, 3, "active tab moved from 1 to 3");
    }

    #[test]
    fn move_tab_active_shifts_left_when_src_before() {
        let mut s = strip_with_n(5);
        s.active = 2; // id=2
        s.move_tab(1, 3);
        // id=1 moved forward past id=2, so active shifts left
        assert_eq!(s.active, 1);
    }

    #[test]
    fn move_tab_active_shifts_right_when_src_after() {
        let mut s = strip_with_n(5);
        s.active = 2; // id=2
        s.move_tab(3, 1);
        // id=3 moved backward past id=2, so active shifts right
        assert_eq!(s.active, 3);
    }

    #[test]
    fn move_tab_active_unaffected_outside_range() {
        let mut s = strip_with_n(5);
        s.active = 4;
        s.move_tab(1, 3);
        assert_eq!(s.active, 4);
    }

    // ── pin / duplicate / close-others / close-right tests (CC-4) ─────────────

    #[test]
    fn toggle_pin_flips_state() {
        let mut s = TabStrip::new();
        assert!(!s.is_pinned(0));
        assert!(s.toggle_pin(0));
        assert!(s.is_pinned(0));
        assert!(!s.toggle_pin(0));
        assert!(!s.is_pinned(0));
    }

    #[test]
    fn toggle_pin_out_of_bounds_is_false() {
        let mut s = TabStrip::new();
        assert!(!s.toggle_pin(99));
    }

    #[test]
    fn duplicate_inserts_clone_after_source() {
        let mut s = strip_with_n(3); // ids [0,1,2]
        s.tabs[1].title = "Page B".to_owned();
        let new_idx = s.duplicate(1, 0.0).expect("in-bounds");
        assert_eq!(new_idx, 2);
        assert_eq!(s.len(), 4);
        assert_eq!(s.tabs[2].title, "Page B");
        // Clone opener points at the source tab id.
        assert_eq!(s.tabs[2].opener_id, Some(1));
        // Original ordering preserved around the clone.
        assert_eq!(ids(&s), vec![0, 1, 3, 2]);
    }

    #[test]
    fn duplicate_clone_is_not_pinned() {
        let mut s = TabStrip::new();
        s.tabs[0].pinned = true;
        let new_idx = s.duplicate(0, 0.0).unwrap();
        assert!(!s.tabs[new_idx].pinned);
    }

    #[test]
    fn duplicate_shifts_active_when_inserted_before() {
        let mut s = strip_with_n(3);
        s.active = 2;
        s.duplicate(0, 0.0); // inserts at index 1, before active
        assert_eq!(s.active, 3);
    }

    #[test]
    fn duplicate_out_of_bounds_returns_none() {
        let mut s = TabStrip::new();
        assert_eq!(s.duplicate(5, 0.0), None);
    }

    #[test]
    fn close_others_keeps_only_target() {
        let mut s = strip_with_n(4); // ids [0,1,2,3]
        let removed = s.close_others(2);
        assert_eq!(s.len(), 1);
        assert_eq!(s.tabs[0].id, 2);
        assert_eq!(s.active, 0);
        let mut sorted = removed.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 1, 3]);
    }

    #[test]
    fn close_others_preserves_pinned() {
        let mut s = strip_with_n(4); // ids [0,1,2,3]
        s.tabs[0].pinned = true;
        let removed = s.close_others(2);
        // Tab 0 (pinned) and tab 2 (target) survive.
        assert_eq!(ids(&s), vec![0, 2]);
        let mut sorted = removed.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![1, 3]);
        // Active points at the kept target (id=2 → new index 1).
        assert_eq!(s.active, 1);
    }

    #[test]
    fn close_right_removes_tabs_after_idx() {
        let mut s = strip_with_n(5); // ids [0,1,2,3,4]
        s.active = 1;
        let removed = s.close_right(1);
        assert_eq!(ids(&s), vec![0, 1]);
        assert_eq!(s.active, 1);
        let mut sorted = removed.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![2, 3, 4]);
    }

    #[test]
    fn close_right_preserves_pinned_to_right() {
        let mut s = strip_with_n(5); // ids [0,1,2,3,4]
        s.tabs[3].pinned = true;
        let removed = s.close_right(1);
        // Pinned tab 3 survives; 2 and 4 removed.
        assert_eq!(ids(&s), vec![0, 1, 3]);
        let mut sorted = removed.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![2, 4]);
    }

    #[test]
    fn close_right_clamps_active_when_active_removed() {
        let mut s = strip_with_n(5);
        s.active = 4; // will be removed
        s.close_right(1);
        // active clamped to the kept range (idx 1).
        assert_eq!(s.active, 1);
        assert!(s.active < s.len());
    }

    // ── TabDragState::drop_target tests ──────────────────────────────────────

    #[test]
    fn drop_target_first_tab() {
        let drag = TabDragState { src_idx: 0, press_x: 0.0, ghost_x: 10.0, active: true };
        // 5 tabs, each 200px wide in 1000px window → ghost at 10 → target 0
        assert_eq!(drag.drop_target(5, 1000.0), 0);
    }

    #[test]
    fn drop_target_last_tab() {
        let drag = TabDragState { src_idx: 0, press_x: 0.0, ghost_x: 950.0, active: true };
        assert_eq!(drag.drop_target(5, 1000.0), 4);
    }

    #[test]
    fn drop_target_middle() {
        let drag = TabDragState { src_idx: 0, press_x: 0.0, ghost_x: 400.0, active: true };
        // ghost at 400 / 200 = 2 → target 2
        assert_eq!(drag.drop_target(5, 1000.0), 2);
    }

    // ── Tab group tests (CC-6) ───────────────────────────────────────────────

    #[test]
    fn create_group_returns_fresh_ids() {
        let mut s = TabStrip::new();
        let a = s.create_group("Work", GroupColor::Blue);
        let b = s.create_group("Play", GroupColor::Green);
        assert_ne!(a, b);
        assert_eq!(s.groups.len(), 2);
        assert_eq!(s.group(a).unwrap().label, "Work");
        assert_eq!(s.group_color(b), Some(GroupColor::Green));
    }

    #[test]
    fn assign_to_group_sets_membership() {
        let mut s = strip_with_n(3);
        let g = s.create_group("G", GroupColor::Red);
        assert!(s.assign_to_group(1, g));
        assert_eq!(s.group_of(1), Some(g));
        assert_eq!(s.group_of(0), None);
    }

    #[test]
    fn assign_to_unknown_group_is_noop() {
        let mut s = strip_with_n(2);
        assert!(!s.assign_to_group(0, 999));
        assert_eq!(s.group_of(0), None);
    }

    #[test]
    fn assign_out_of_bounds_is_noop() {
        let mut s = strip_with_n(2);
        let g = s.create_group("G", GroupColor::Red);
        assert!(!s.assign_to_group(99, g));
    }

    #[test]
    fn group_members_lists_in_order() {
        let mut s = strip_with_n(4); // ids [0,1,2,3]
        let g = s.create_group("G", GroupColor::Cyan);
        s.assign_to_group(2, g);
        s.assign_to_group(0, g);
        assert_eq!(s.group_members(g), vec![0, 2]);
    }

    #[test]
    fn toggle_collapse_flips_state() {
        let mut s = TabStrip::new();
        let g = s.create_group("G", GroupColor::Purple);
        assert!(!s.is_collapsed(g));
        assert!(s.toggle_collapse(g));
        assert!(s.is_collapsed(g));
        assert!(!s.toggle_collapse(g));
        assert!(!s.is_collapsed(g));
    }

    #[test]
    fn toggle_collapse_unknown_group_is_false() {
        let mut s = TabStrip::new();
        assert!(!s.toggle_collapse(42));
    }

    #[test]
    fn ungroup_clears_membership() {
        let mut s = strip_with_n(2);
        let g = s.create_group("G", GroupColor::Grey);
        s.assign_to_group(1, g);
        s.ungroup(1);
        assert_eq!(s.group_of(1), None);
    }

    #[test]
    fn remove_group_ungroups_members() {
        let mut s = strip_with_n(3);
        let g = s.create_group("G", GroupColor::Yellow);
        s.assign_to_group(0, g);
        s.assign_to_group(2, g);
        s.remove_group(g);
        assert!(s.group(g).is_none());
        assert_eq!(s.group_of(0), None);
        assert_eq!(s.group_of(2), None);
    }

    #[test]
    fn visible_indices_all_when_no_collapse() {
        let s = strip_with_n(4);
        assert_eq!(s.visible_indices(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn visible_indices_hides_collapsed_members_except_chip() {
        let mut s = strip_with_n(4); // ids [0,1,2,3]
        let g = s.create_group("G", GroupColor::Blue);
        s.assign_to_group(1, g);
        s.assign_to_group(2, g);
        s.toggle_collapse(g);
        // Tab 1 is the chip (leftmost member); tab 2 is hidden.
        assert_eq!(s.visible_indices(), vec![0, 1, 3]);
    }

    #[test]
    fn visible_indices_expanded_group_shows_all() {
        let mut s = strip_with_n(4);
        let g = s.create_group("G", GroupColor::Blue);
        s.assign_to_group(1, g);
        s.assign_to_group(2, g);
        // Not collapsed → every tab visible.
        assert_eq!(s.visible_indices(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn duplicate_inherits_group() {
        let mut s = strip_with_n(2);
        let g = s.create_group("G", GroupColor::Pink);
        s.assign_to_group(0, g);
        let new_idx = s.duplicate(0, 0.0).unwrap();
        assert_eq!(s.group_of(new_idx), Some(g));
    }

}
