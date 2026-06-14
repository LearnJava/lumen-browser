//! Tab strip: per-tab metadata and rendering.
//!
//! `TabStrip` holds the list of open tabs and the active index.
//! `build_tab_bar` produces a viewport-locked `DisplayList` for the strip area.
//! `hit_test` maps CSS-px (x, y) → `TabHit` for mouse dispatch.
//!
//! Visual constants follow a dark-chrome aesthetic consistent with
//! `address_bar.rs` and `find.rs`.

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

use crate::tab_lifecycle::state::TabState;
use crate::tabs::containers::ContainerKind;
use crate::tabs::groups::{GroupColor, TabGroup};

// ── Visual constants ──────────────────────────────────────────────────────────

/// Height of the tab bar in CSS px. Subtracted from `viewport_height_css()`.
pub const TAB_BAR_HEIGHT: f32 = 36.0;

/// Pixels the cursor must travel before a press becomes a drag.
pub const DRAG_THRESHOLD: f32 = 6.0;

/// Width of the drop-indicator bar rendered between tabs during a drag.
const DROP_INDICATOR_W: f32 = 3.0;

/// Colour of the vertical drop-indicator bar.
const DROP_INDICATOR_COLOR: Color = Color { r: 255, g: 255, b: 255, a: 180 };

const BAR_BG: Color = Color { r: 22, g: 22, b: 26, a: 255 };
const TAB_INACTIVE_BG: Color = Color { r: 32, g: 33, b: 36, a: 255 };
const TAB_ACTIVE_BG: Color = Color { r: 18, g: 18, b: 22, a: 255 };
const TAB_TEXT: Color = Color { r: 218, g: 218, b: 228, a: 255 };
const TAB_TEXT_DIM: Color = Color { r: 140, g: 140, b: 148, a: 255 };
const CLOSE_FG: Color = Color { r: 180, g: 80, b: 80, a: 255 };
const DIVIDER: Color = Color { r: 45, g: 46, b: 52, a: 255 };

/// Badge colour for BackgroundOld tier — amber "z" sleep icon.
/// Indicator colour for a pinned tab (CC-4) — cyan dot at the tab's left edge.
const PIN_COLOR: Color = Color { r: 90, g: 200, b: 220, a: 255 };
const BADGE_OLD_COLOR: Color = Color { r: 255, g: 168, b: 0, a: 210 };
/// Badge colour for Hibernated tier — grey "Z" sleep icon.
const BADGE_HIBERNATE_COLOR: Color = Color { r: 110, g: 110, b: 120, a: 210 };
/// Dimmed background for BackgroundOld (T2) tabs — signals reduced activity.
const TAB_T2_BG: Color = Color { r: 26, g: 27, b: 30, a: 255 };
/// Dimmed background for Hibernated (T3) tabs — signals deep sleep.
const TAB_T3_BG: Color = Color { r: 21, g: 21, b: 24, a: 255 };

const FONT_SZ: f32 = 12.0;
/// Minimum tab button width in CSS px.
const TAB_MIN_W: f32 = 80.0;
/// Maximum tab button width in CSS px.
const TAB_MAX_W: f32 = 200.0;
/// Horizontal padding inside a tab (text from left edge).
const TAB_PAD: f32 = 10.0;
/// Close-button glyph size.
const CLOSE_SZ: f32 = 14.0;
/// Gap between text area right edge and close-button left edge.
const CLOSE_MARGIN: f32 = 4.0;
/// Font size for the "Z"/"z" sleep-icon badge on T2/T3 tabs.
const BADGE_Z_SZ: f32 = 9.0;
/// Height of the container border-top strip in CSS px (7D.2). Drawn at the
/// very top of each tab button when its `container` is not `ContainerKind::None`.
const CONTAINER_STRIP_HEIGHT: f32 = 3.0;
/// Height of the tab-group accent bar in CSS px (CC-6). Drawn at the bottom
/// edge of a grouped tab in its group's [`GroupColor`].
const GROUP_BAR_HEIGHT: f32 = 3.0;
/// Width of the collapsed-group chip marker (a square swatch) in CSS px.
const COLLAPSE_CHIP_W: f32 = 10.0;

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
    /// Container assigned to this tab (7D.2). Drives the border-top strip
    /// rendered above the tab and the cookie/storage isolation key.
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
    /// tab is ungrouped. Drives the coloured group accent bar and collapse
    /// visibility in [`build_tab_bar`].
    pub group_id: Option<usize>,
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
    /// Triggers a visual change on the next `build_tab_bar` call — the
    /// border-top strip swaps colour or appears/disappears. Cookie/storage
    /// isolation rewiring is the caller's responsibility (see
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

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Result of clicking inside the tab bar area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabHit {
    /// Clicked the tab body (not close button) — `idx` = tab index.
    Tab(usize),
    /// Clicked the close ×  button — `idx` = tab index.
    Close(usize),
    /// Clicked empty area (right of all tabs).
    Empty,
}

/// Returns the `[left, right)` x-range of tab `idx` given `n_tabs` tabs and
/// a `window_w`-wide window.
fn tab_x_range(idx: usize, n_tabs: usize, window_w: f32) -> (f32, f32) {
    let tab_w = (window_w / n_tabs as f32).clamp(TAB_MIN_W, TAB_MAX_W);
    let left = idx as f32 * tab_w;
    (left, left + tab_w)
}

/// Hit-test a click at CSS-px `(x, y)` against the tab bar.
///
/// Returns `TabHit::Empty` if `y >= TAB_BAR_HEIGHT` (below the strip).
pub fn hit_test(strip: &TabStrip, x: f32, y: f32, window_w: f32) -> TabHit {
    if !(0.0..TAB_BAR_HEIGHT).contains(&y) {
        return TabHit::Empty;
    }
    // Lay out over the *visible* tabs so collapsed-group members map to the
    // chip tab. For a strip with no collapsed groups this is `0..tabs.len()`.
    let visible = strip.visible_indices();
    let n = visible.len();
    for (slot, &i) in visible.iter().enumerate() {
        let (left, right) = tab_x_range(slot, n, window_w);
        if x >= left && x < right {
            // Close-button occupies the rightmost CLOSE_SZ + CLOSE_MARGIN px.
            let close_right = right - TAB_PAD;
            let close_left = close_right - CLOSE_SZ;
            if x >= close_left && x < close_right {
                return TabHit::Close(i);
            }
            return TabHit::Tab(i);
        }
    }
    TabHit::Empty
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build a viewport-locked display list for the tab bar.
///
/// `accent` overrides the active-tab indicator colour (bottom 2 px bar).
/// Pass `drag` during a drag-and-drop operation to render the drop indicator.
///
/// Appended to the overlay buffer each frame; rendered on top of page content
/// at y = 0..`TAB_BAR_HEIGHT`.
///
/// Lifecycle badge rendering:
/// - `TabState::BackgroundOld` → amber dot at top-right corner of the tab button.
/// - `TabState::Hibernated`    → grey dot at top-right corner of the tab button.
/// - All other states          → no badge rendered.
pub fn build_tab_bar(
    strip: &TabStrip,
    window_w: f32,
    accent: Color,
    drag: Option<&TabDragState>,
) -> DisplayList {
    // Lay out over the *visible* tabs: members of a collapsed group (except the
    // chip) are skipped. With no collapsed groups this is `0..tabs.len()`.
    let visible = strip.visible_indices();
    let n = visible.len();
    let mut out = DisplayList::with_capacity(4 + n * 7);

    // Background strip.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, window_w, TAB_BAR_HEIGHT),
        color: BAR_BG,
    });

    for (slot, &i) in visible.iter().enumerate() {
        let tab = &strip.tabs[i];
        let (left, right) = tab_x_range(slot, n, window_w);
        let is_active = i == strip.active;

        // Tab background: T2/T3 use darker backgrounds as fade-opacity signal.
        let bg = if is_active {
            TAB_ACTIVE_BG
        } else {
            match tab.tab_state {
                TabState::BackgroundOld => TAB_T2_BG,
                TabState::Hibernated => TAB_T3_BG,
                _ => TAB_INACTIVE_BG,
            }
        };
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(left, 0.0, right - left, TAB_BAR_HEIGHT),
            color: bg,
        });

        // Tab-group accent bar (CC-6): a coloured strip along the bottom of a
        // grouped tab in its group's colour. Drawn before the active accent so
        // the active 2 px bar still reads on top for a grouped active tab.
        if let Some(gid) = tab.group_id
            && let Some(gc) = strip.group_color(gid)
        {
            let col = gc.color();
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(left, TAB_BAR_HEIGHT - GROUP_BAR_HEIGHT, right - left, GROUP_BAR_HEIGHT),
                color: col,
            });
            // Collapsed-group chip: a small square swatch near the left edge of
            // the leftmost (chip) tab, signalling the group is folded.
            if strip.is_collapsed(gid) {
                out.push(DisplayCommand::FillRect {
                    rect: Rect::new(left + 4.0, (TAB_BAR_HEIGHT - COLLAPSE_CHIP_W) * 0.5, COLLAPSE_CHIP_W, COLLAPSE_CHIP_W),
                    color: col,
                });
            }
        }

        // Active tab accent bar at the bottom.
        if is_active {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(left, TAB_BAR_HEIGHT - 2.0, right - left, 2.0),
                color: accent,
            });
        }

        // Container border-top strip (7D.2). 3 px tall coloured bar at the
        // very top edge of the tab. Skipped for ContainerKind::None.
        if let Some(color) = tab.container.border_color() {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(left, 0.0, right - left, CONTAINER_STRIP_HEIGHT),
                color,
            });
        }

        // Pinned indicator (CC-4): a small cyan dot near the tab's left edge.
        if tab.pinned {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(left + 4.0, TAB_BAR_HEIGHT * 0.5 - 2.5, 5.0, 5.0),
                color: PIN_COLOR,
            });
        }

        // Tab right divider (skip last tab).
        if i + 1 < n {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(right - 1.0, 4.0, 1.0, TAB_BAR_HEIGHT - 8.0),
                color: DIVIDER,
            });
        }

        // Lifecycle badge — "Z" glyph at top-right corner (sleep icon).
        // BackgroundOld → amber lowercase "z"; Hibernated → grey uppercase "Z".
        let badge_info: Option<(&str, Color)> = match tab.tab_state {
            TabState::BackgroundOld => Some(("z", BADGE_OLD_COLOR)),
            TabState::Hibernated => Some(("Z", BADGE_HIBERNATE_COLOR)),
            _ => None,
        };
        if let Some((glyph, color)) = badge_info {
            // Position: top-right of the tab, inset 3px from right edge, 3px from top.
            let bx = right - BADGE_Z_SZ - 3.0;
            let by = 3.0;
            out.push(DisplayCommand::DrawText {
                rect: Rect::new(bx, by, BADGE_Z_SZ, BADGE_Z_SZ * 1.2),
                text: glyph.to_owned(),
                font_size: BADGE_Z_SZ,
                color,
                font_family: Vec::new(),
                font_weight: FontWeight::BOLD,
                font_style: FontStyle::Italic,
                font_variation_axes: Vec::new(),
                tab_size: 0.0,
                highlight_name: None,
            });
        }

        // Close button — ×
        let close_right = right - TAB_PAD;
        let close_left = close_right - CLOSE_SZ;
        let close_cy = (TAB_BAR_HEIGHT - CLOSE_SZ * 1.2) * 0.5;
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(close_left, close_cy, CLOSE_SZ, CLOSE_SZ * 1.2),
            text: "×".to_owned(),
            font_size: CLOSE_SZ,
            color: CLOSE_FG,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
        });

        // Tab title — truncated to fit between left edge and close button.
        let text_x = left + TAB_PAD;
        let text_w = (close_left - CLOSE_MARGIN - text_x).max(0.0);
        let text_y = (TAB_BAR_HEIGHT - FONT_SZ * 1.3) * 0.5;
        let text_color = if is_active { TAB_TEXT } else { TAB_TEXT_DIM };
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(text_x, text_y, text_w, FONT_SZ * 1.3),
            text: tab.title.clone(),
            font_size: FONT_SZ,
            color: text_color,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
        });
    }

    // Drop indicator: vertical bar at the target insertion gap, shown only
    // while a drag is active (cursor moved past threshold).
    if let Some(d) = drag
        && d.active {
            let tab_w = (window_w / n as f32).clamp(TAB_MIN_W, TAB_MAX_W);
            let target = d.drop_target(n, window_w);
            let ix = target as f32 * tab_w - DROP_INDICATOR_W * 0.5;
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(ix, 2.0, DROP_INDICATOR_W, TAB_BAR_HEIGHT - 4.0),
                color: DROP_INDICATOR_COLOR,
            });
        }

    out
}

/// Build a small tooltip overlay for a tab with a non-Active tier badge.
///
/// Returns `None` if the hovered tab has no tier badge (Active / BackgroundRecent).
/// Tooltip displays above the tab bar with context about the tab state.
pub fn build_tab_tooltip(
    tab: &TabEntry,
    tab_center_x: f32,
    tab_bar_bottom: f32,
) -> Option<DisplayList> {
    let msg = match tab.tab_state {
        TabState::BackgroundOld => "Вкладка фоновая — потребляет меньше памяти",
        TabState::Hibernated => "Вкладка спит — клик восстановит (~1 сек)",
        _ => return None,
    };

    const TT_W: f32 = 240.0;
    const TT_H: f32 = 28.0;
    const PAD: f32 = 8.0;
    const RADIUS: f32 = 4.0;
    const FONT_SZ: f32 = 11.0;

    let x = (tab_center_x - TT_W / 2.0).max(4.0);
    let y = tab_bar_bottom + 4.0;

    let bg = Color { r: 38, g: 38, b: 42, a: 235 };
    let text_color = Color { r: 255, g: 255, b: 255, a: 255 };

    Some(vec![
        DisplayCommand::FillRoundedRect {
            rect: Rect::new(x, y, TT_W, TT_H),
            radii: CornerRadii { tl: RADIUS, tl_y: RADIUS, tr: RADIUS, tr_y: RADIUS, br: RADIUS, br_y: RADIUS, bl: RADIUS, bl_y: RADIUS },
            color: bg,
        },
        DisplayCommand::DrawText {
            rect: Rect::new(x + PAD, y + TT_H / 2.0 - FONT_SZ * 0.4, TT_W - 2.0 * PAD, FONT_SZ * 1.2),
            text: msg.to_string(),
            font_size: FONT_SZ,
            color: text_color,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
        },
    ])
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

    #[test]
    fn hit_test_tab_body() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        // Two tabs, each 512px wide in a 1024px window.
        // Click in the middle of the first tab, away from close button.
        let hit = hit_test(&s, 100.0, 18.0, 1024.0);
        assert_eq!(hit, TabHit::Tab(0));
    }

    #[test]
    fn hit_test_close_button() {
        let s = TabStrip::new();
        // Single tab: tab_w = clamp(1024/1, 80, 200) = 200, so tab occupies [0, 200).
        // Close button: close_right = 200 - 10 = 190, close_left = 190 - 14 = 176.
        // → button at [176, 190); click at 182 should hit it.
        let hit = hit_test(&s, 182.0, 18.0, 1024.0);
        assert_eq!(hit, TabHit::Close(0));
    }

    #[test]
    fn hit_test_below_bar_returns_empty() {
        let s = TabStrip::new();
        let hit = hit_test(&s, 100.0, TAB_BAR_HEIGHT + 1.0, 1024.0);
        assert_eq!(hit, TabHit::Empty);
    }

    #[test]
    fn build_tab_bar_emits_commands() {
        let s = TabStrip::new();
        let dl = build_tab_bar(&s, 1024.0, Color { r: 100, g: 128, b: 255, a: 255 }, None);
        assert!(!dl.is_empty());
        let has_title = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("вкладка"))
        });
        assert!(has_title);
    }

    #[test]
    fn build_tab_bar_no_badge_for_active() {
        let s = TabStrip::new(); // single Active tab
        let dl = build_tab_bar(&s, 1024.0, Color { r: 100, g: 128, b: 255, a: 255 }, None);
        // Active tab must not emit a sleep-icon badge (no "Z"/"z" glyph).
        let has_sleep_badge = dl.iter().any(|c| match c {
            DisplayCommand::DrawText { text, .. } => text == "Z" || text == "z",
            _ => false,
        });
        assert!(!has_sleep_badge, "Active tab must not render a sleep badge");
    }

    #[test]
    fn build_tab_bar_badge_for_background_old() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        s.set_tab_state(0, TabState::BackgroundOld);
        let dl = build_tab_bar(&s, 1024.0, Color { r: 100, g: 128, b: 255, a: 255 }, None);
        // Amber "z" glyph badge for BackgroundOld tier.
        let has_z = dl.iter().any(|c| match c {
            DisplayCommand::DrawText { text, color, .. } => {
                text == "z" && color.r == BADGE_OLD_COLOR.r && color.g == BADGE_OLD_COLOR.g
            }
            _ => false,
        });
        assert!(has_z, "BackgroundOld tab must render amber 'z' badge");
    }

    #[test]
    fn build_tab_bar_badge_for_hibernated() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        s.set_tab_state(0, TabState::Hibernated);
        let dl = build_tab_bar(&s, 1024.0, Color { r: 100, g: 128, b: 255, a: 255 }, None);
        // Grey "Z" glyph badge for Hibernated tier.
        let has_z = dl.iter().any(|c| match c {
            DisplayCommand::DrawText { text, color, .. } => {
                text == "Z" && color.r == BADGE_HIBERNATE_COLOR.r && color.g == BADGE_HIBERNATE_COLOR.g
            }
            _ => false,
        });
        assert!(has_z, "Hibernated tab must render grey 'Z' badge");
    }

    #[test]
    fn build_tab_bar_fade_bg_for_background_old() {
        let mut s = TabStrip::new();
        s.push_blank(0.0); // index 0 — active
        s.push_blank(0.0); // index 1 — inactive BackgroundOld
        s.set_tab_state(1, TabState::BackgroundOld);
        let dl = build_tab_bar(&s, 1024.0, Color { r: 100, g: 128, b: 255, a: 255 }, None);
        // T2 background must be TAB_T2_BG, not TAB_INACTIVE_BG.
        let has_t2_bg = dl.iter().any(|c| match c {
            DisplayCommand::FillRect { color, .. } => *color == TAB_T2_BG,
            _ => false,
        });
        assert!(has_t2_bg, "BackgroundOld inactive tab must use dimmed T2 background");
    }

    #[test]
    fn build_tab_bar_fade_bg_for_hibernated() {
        let mut s = TabStrip::new();
        s.push_blank(0.0); // index 0 — active
        s.push_blank(0.0); // index 1 — inactive Hibernated
        s.set_tab_state(1, TabState::Hibernated);
        let dl = build_tab_bar(&s, 1024.0, Color { r: 100, g: 128, b: 255, a: 255 }, None);
        // T3 background must be TAB_T3_BG.
        let has_t3_bg = dl.iter().any(|c| match c {
            DisplayCommand::FillRect { color, .. } => *color == TAB_T3_BG,
            _ => false,
        });
        assert!(has_t3_bg, "Hibernated inactive tab must use dimmed T3 background");
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

    /// Helper: count `FillRect` commands whose rect matches the container
    /// border-top strip — height equals `CONTAINER_STRIP_HEIGHT` and origin
    /// `y == 0.0`. Excludes the full-bar background rect (its height ==
    /// `TAB_BAR_HEIGHT`).
    fn count_container_strips(dl: &DisplayList, expected_color: Color) -> usize {
        dl.iter()
            .filter(|c| match c {
                DisplayCommand::FillRect { rect, color } => {
                    (rect.height - CONTAINER_STRIP_HEIGHT).abs() < f32::EPSILON
                        && rect.y.abs() < f32::EPSILON
                        && *color == expected_color
                }
                _ => false,
            })
            .count()
    }

    #[test]
    fn build_tab_bar_renders_strip_for_work() {
        let mut s = TabStrip::new();
        s.set_tab_container(0, ContainerKind::Work);
        let dl = build_tab_bar(&s, 1024.0, Color { r: 100, g: 128, b: 255, a: 255 }, None);
        let expected = ContainerKind::Work.border_color().expect("Work has colour");
        assert_eq!(count_container_strips(&dl, expected), 1);
    }

    #[test]
    fn build_tab_bar_renders_strip_for_personal() {
        let mut s = TabStrip::new();
        s.set_tab_container(0, ContainerKind::Personal);
        let dl = build_tab_bar(&s, 1024.0, Color { r: 100, g: 128, b: 255, a: 255 }, None);
        let expected = ContainerKind::Personal.border_color().expect("Personal has colour");
        assert_eq!(count_container_strips(&dl, expected), 1);
    }

    #[test]
    fn build_tab_bar_renders_strip_for_finance() {
        let mut s = TabStrip::new();
        s.set_tab_container(0, ContainerKind::Finance);
        let dl = build_tab_bar(&s, 1024.0, Color { r: 100, g: 128, b: 255, a: 255 }, None);
        let expected = ContainerKind::Finance.border_color().expect("Finance has colour");
        assert_eq!(count_container_strips(&dl, expected), 1);
    }

    #[test]
    fn build_tab_bar_renders_strip_for_shopping() {
        let mut s = TabStrip::new();
        s.set_tab_container(0, ContainerKind::Shopping);
        let dl = build_tab_bar(&s, 1024.0, Color { r: 100, g: 128, b: 255, a: 255 }, None);
        let expected = ContainerKind::Shopping.border_color().expect("Shopping has colour");
        assert_eq!(count_container_strips(&dl, expected), 1);
    }

    #[test]
    fn build_tab_bar_renders_strip_for_custom_rgb() {
        let mut s = TabStrip::new();
        s.set_tab_container(0, ContainerKind::Custom(200, 50, 100));
        let dl = build_tab_bar(&s, 1024.0, Color { r: 100, g: 128, b: 255, a: 255 }, None);
        let expected = Color { r: 200, g: 50, b: 100, a: 255 };
        assert_eq!(count_container_strips(&dl, expected), 1);
    }

    #[test]
    fn build_tab_bar_no_strip_for_none_container() {
        let s = TabStrip::new(); // single tab, ContainerKind::None
        let dl = build_tab_bar(&s, 1024.0, Color { r: 100, g: 128, b: 255, a: 255 }, None);
        // No FillRect of CONTAINER_STRIP_HEIGHT may exist when container is None.
        let strips = dl
            .iter()
            .filter(|c| match c {
                DisplayCommand::FillRect { rect, .. } => {
                    (rect.height - CONTAINER_STRIP_HEIGHT).abs() < f32::EPSILON
                        && rect.y.abs() < f32::EPSILON
                }
                _ => false,
            })
            .count();
        assert_eq!(strips, 0, "ContainerKind::None must not render a strip");
    }

    #[test]
    fn build_tab_bar_strip_only_for_tabs_with_container() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        s.push_blank(0.0);
        s.set_tab_container(1, ContainerKind::Work);
        let dl = build_tab_bar(&s, 1024.0, Color { r: 100, g: 128, b: 255, a: 255 }, None);
        let work_color = ContainerKind::Work.border_color().expect("Work has colour");
        // Exactly one Work-coloured strip (tab 1); tabs 0 and 2 have None.
        assert_eq!(count_container_strips(&dl, work_color), 1);
    }

    #[test]
    fn tooltip_none_for_active_tab() {
        let tab = TabEntry {
            id: 0,
            title: "Test".to_owned(),
            tab_state: TabState::Active,
            opener_id: None,
            container: ContainerKind::None,
            last_activated_ms: 0.0,
            pinned: false,
            group_id: None,
        };
        assert!(build_tab_tooltip(&tab, 100.0, 36.0).is_none());
    }

    #[test]
    fn tooltip_some_for_hibernated_tab() {
        let tab = TabEntry {
            id: 0,
            title: "Test".to_owned(),
            tab_state: TabState::Hibernated,
            opener_id: None,
            container: ContainerKind::None,
            last_activated_ms: 0.0,
            pinned: false,
            group_id: None,
        };
        let cmds = build_tab_tooltip(&tab, 100.0, 36.0);
        assert!(cmds.is_some());
        // Tooltip must have at least background + text.
        assert!(cmds.unwrap().len() >= 2);
    }

    #[test]
    fn tooltip_some_for_background_old() {
        let tab = TabEntry {
            id: 0,
            title: "Test".to_owned(),
            tab_state: TabState::BackgroundOld,
            opener_id: None,
            container: ContainerKind::None,
            last_activated_ms: 0.0,
            pinned: false,
            group_id: None,
        };
        assert!(build_tab_tooltip(&tab, 100.0, 36.0).is_some());
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

    #[test]
    fn build_tab_bar_drop_indicator_when_active_drag() {
        let mut s = TabStrip::new();
        s.push_blank(0.0);
        let drag = TabDragState { src_idx: 0, press_x: 0.0, ghost_x: 100.0, active: true };
        let accent = Color { r: 100, g: 128, b: 255, a: 255 };
        let dl = build_tab_bar(&s, 1024.0, accent, Some(&drag));
        // Drop indicator must produce a FillRect
        let has_indicator = dl.iter().any(|c| match c {
            DisplayCommand::FillRect { color, .. } => *color == DROP_INDICATOR_COLOR,
            _ => false,
        });
        assert!(has_indicator, "active drag must render a drop indicator");
    }

    #[test]
    fn build_tab_bar_no_indicator_when_drag_not_active() {
        let s = TabStrip::new();
        let drag = TabDragState { src_idx: 0, press_x: 0.0, ghost_x: 100.0, active: false };
        let accent = Color { r: 100, g: 128, b: 255, a: 255 };
        let dl = build_tab_bar(&s, 1024.0, accent, Some(&drag));
        let has_indicator = dl.iter().any(|c| match c {
            DisplayCommand::FillRect { color, .. } => *color == DROP_INDICATOR_COLOR,
            _ => false,
        });
        assert!(!has_indicator, "inactive drag must not render a drop indicator");
    }

    #[test]
    fn build_tab_bar_accent_color_used_for_active_tab() {
        let s = TabStrip::new(); // one active tab
        let custom_accent = Color { r: 230, g: 59, b: 111, a: 255 }; // rose
        let dl = build_tab_bar(&s, 1024.0, custom_accent, None);
        let has_accent = dl.iter().any(|c| match c {
            DisplayCommand::FillRect { color, .. } => *color == custom_accent,
            _ => false,
        });
        assert!(has_accent, "active tab must use the provided accent color");
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

    #[test]
    fn build_tab_bar_draws_group_accent_bar() {
        let mut s = strip_with_n(2);
        let g = s.create_group("G", GroupColor::Green);
        s.assign_to_group(0, g);
        let dl = build_tab_bar(&s, 1024.0, Color { r: 1, g: 2, b: 3, a: 255 }, None);
        let green = GroupColor::Green.color();
        let has_group_bar = dl.iter().any(|c| match c {
            DisplayCommand::FillRect { rect, color } => {
                (rect.height - GROUP_BAR_HEIGHT).abs() < f32::EPSILON
                    && (rect.y - (TAB_BAR_HEIGHT - GROUP_BAR_HEIGHT)).abs() < f32::EPSILON
                    && *color == green
            }
            _ => false,
        });
        assert!(has_group_bar, "grouped tab must render its group accent bar");
    }

    #[test]
    fn build_tab_bar_no_group_bar_for_ungrouped() {
        let s = strip_with_n(2);
        let dl = build_tab_bar(&s, 1024.0, Color { r: 1, g: 2, b: 3, a: 255 }, None);
        let bars = dl
            .iter()
            .filter(|c| match c {
                DisplayCommand::FillRect { rect, .. } => {
                    (rect.height - GROUP_BAR_HEIGHT).abs() < f32::EPSILON
                        && (rect.y - (TAB_BAR_HEIGHT - GROUP_BAR_HEIGHT)).abs() < f32::EPSILON
                }
                _ => false,
            })
            .count();
        assert_eq!(bars, 0, "ungrouped tabs must not render a group bar");
    }

    #[test]
    fn hit_test_collapsed_group_maps_to_chip() {
        let mut s = strip_with_n(4); // 4 tabs
        let g = s.create_group("G", GroupColor::Blue);
        s.assign_to_group(1, g);
        s.assign_to_group(2, g);
        s.toggle_collapse(g);
        // Visible: [0, 1(chip), 3]. With 3 visible tabs the width is clamped to
        // TAB_MAX_W=200, so slot 1 spans [200, 400). Click inside it, away from
        // the close button — it must map to the chip's real index (1).
        let visible = s.visible_indices();
        assert_eq!(visible, vec![0, 1, 3]);
        let hit = hit_test(&s, 250.0, 18.0, 1024.0);
        assert_eq!(hit, TabHit::Tab(1));
    }
}
