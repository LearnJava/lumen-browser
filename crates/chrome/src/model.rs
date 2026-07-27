//! `ChromeModel` — snapshot of shell state bound into the chrome `Document`
//! (CC-6, `docs/tasks/p1-css-chrome.md`).
//!
//! The frozen design reference (`docs/design/lumen-v3_3.html`) has no
//! `<template>` markup — the brief's "clone `<template>` for lists" is
//! therefore not literally applicable (see `docs/tasks/p1-css-chrome.md`
//! CC-6 note). [`bind_model`] instead rebuilds list containers (the tab
//! strip, the workspace switcher) by constructing fresh element nodes from
//! [`ChromeModel`] data on every call — "дифф простейший: перестроить
//! изменённый список целиком" from the brief, just without a literal
//! `<template>` source. Icon glyphs (favicon symbol, close-button `×`) are
//! deliberately simplified to a single-letter fallback rather than cloning
//! the asset's inline SVG sprite — visual finish is a follow-up, not part of
//! this slice's DoD (tab/theme/profile/workspace switches reflect in chrome).

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};

use lumen_dom::{Attribute, Document, NodeData, NodeId, QualName};

/// Snapshot of shell state [`bind_model`] reflects into the chrome document.
///
/// Built fresh by the shell on every [`bind_model`] call (see
/// `Lumen::chrome_model_snapshot` in `crates/shell/src/main.rs`) — there is
/// no retained/diffed state here, matching the brief's "простейший" diff.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeModel {
    /// `true` for the dark palette — bound to `body[data-theme]`.
    pub dark_theme: bool,
    /// `true` for the vertical sidebar layout, `false` for the horizontal tab
    /// bar — bound to `body[data-layout]` (`.vertical-only`/`.horizontal-only`
    /// CSS in the asset key off this).
    pub layout_vertical: bool,
    /// Chrome `data-profile` slug (`"personal"`/`"work"`/`"anonymous"`/
    /// `"guest"`) for the active profile, or `None` for a non-seeded profile
    /// with no matching CSS branch (the attribute is then omitted).
    pub profile_slug: Option<String>,
    /// Open tabs, in strip order.
    pub tabs: Vec<ChromeTabModel>,
    /// Workspaces shown in the sidebar switcher.
    pub workspaces: Vec<ChromeWorkspaceModel>,
    /// Omnibox `#omniInput` value + spoof-warning banner (CC-7).
    pub omnibox: OmniboxModel,
    /// `true` narrows the vertical sidebar to its icon rail (`#sidebar.collapsed`,
    /// CC-8) — independent of [`Self::layout_vertical`], which picks vertical vs.
    /// horizontal layout in the first place.
    pub sidebar_collapsed: bool,
    /// `#omniDropdown` suggestion list + open state (CC-9).
    pub dropdown: ChromeDropdownModel,
    /// `#findBar` snapshot (CC-9).
    pub find: ChromeFindModel,
    /// `true` shows `#downloadsPanel` (CC-9) — mirrors `DownloadManager::visible`.
    pub downloads_open: bool,
    /// Download entries rendered into `#downloadsPanel`'s `.dl-list` (CC-9).
    pub downloads: Vec<ChromeDownloadModel>,
    /// `true` shows `#permPopover` (CC-9) — the frozen design merges the
    /// shields counters and the permission rows into one popover, so this
    /// follows either `ShieldsPanel::visible` or `PermissionPanel::visible`.
    pub popover_open: bool,
    /// Total blocked-request count written into `#statTrackers` (CC-9).
    ///
    /// `#statAds`/`#statFp` stay at the asset's own `"0"` — `ShieldsPanel`
    /// only tracks a single honest total (its own doc comment explains why a
    /// trackers/ads/fingerprint breakdown would be fabricated), so this binds
    /// only the one real number rather than inventing the other two.
    pub blocked_total: u32,
    /// Grant state for the two permission rows the frozen design covers, in
    /// asset order (`Камера`, `Микрофон`) — `PermissionKind::ALL`'s first two
    /// entries. The design has no rows for `Notifications`/`Clipboard`.
    pub permissions: [ChromePermState; 2],
    /// `#cpOverlay` snapshot (CC-10) — mirrors `CommandPalette`.
    pub palette: ChromePaletteModel,
    /// `#certOverlay` snapshot (CC-10) — mirrors `CertPanel`.
    pub cert: ChromeCertModel,
    /// `true` shows `#printOverlay` (CC-10) — mirrors `PrintPanel::visible`.
    /// The design's print form (plain `<select>`/`<input type=checkbox>`, no
    /// `data-action`/id hooks) carries no real `PrintPanel` field data —
    /// only open/close state is bound, same class of scope cut as CC-9's
    /// per-download-card buttons (frozen markup has nothing to bind to).
    pub print_open: bool,
    /// Which `#contentArea` view is shown (CC-10b).
    pub content_view: ChromeContentView,
    /// `#view-history` snapshot (CC-10b).
    pub history: ChromeHistoryModel,
    /// `#view-bookmarks` snapshot (CC-10b).
    pub bookmarks: ChromeBookmarksModel,
    /// `#view-settings` snapshot (CC-10b).
    pub settings: ChromeSettingsModel,
    /// `#rightSidebar` snapshot (CC-10b).
    pub right_sidebar: ChromeRightSidebarModel,
}

/// Which content view fills `#contentArea` (CC-10b) — bound to `.view.active`
/// on `#view-page`/`#view-history`/`#view-bookmarks`/`#view-settings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChromeContentView {
    /// The active tab's page — `#view-page`.
    #[default]
    Page,
    /// `#view-history`.
    History,
    /// `#view-bookmarks`.
    Bookmarks,
    /// `#view-settings`.
    Settings,
}

/// `#view-history` snapshot (CC-10b) — mirrors `HistoryPanel::rows`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeHistoryModel {
    /// `true` shows `#histBanner` (mirrors the legacy DS-16 "Anonymous,
    /// history not saved" banner — the design always carries the banner
    /// markup, this only controls whether it's shown).
    pub banner: bool,
    /// Day-group headers and entry rows, in display order — mirrors
    /// `HistoryPanel::rows` (`Group`/`Entry`), shaped for the asset's
    /// `.hist-day`/`.hist-item` markup.
    pub rows: Vec<ChromeHistoryRow>,
}

/// One row of [`ChromeHistoryModel::rows`].
#[derive(Debug, Clone, PartialEq)]
pub enum ChromeHistoryRow {
    /// A `.hist-day` date-group label (e.g. `"Сегодня"`).
    Group(String),
    /// A `.hist-item` entry. Per-row actions (star/copy/delete, `.hist-actions`
    /// in the design) carry no `data-action`/id hooks in the frozen markup —
    /// same class of gap as CC-9's per-download-card buttons — so this binds
    /// only the display fields and the row itself omits `.hist-actions`.
    Entry {
        /// `.hist-title`.
        title: String,
        /// `.hist-url`.
        url: String,
        /// `.hist-time`, pre-formatted by the caller (e.g. `"14:02"`).
        time_label: String,
    },
}

/// `#view-bookmarks` snapshot (CC-10b) — mirrors `BookmarkPanel`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeBookmarksModel {
    /// `.bm-tree` folder rows, in display order — `"Все закладки"` (the
    /// `None`-filter entry) followed by `BookmarkPanel::folders`. Clicking a
    /// folder is out of this slice's DoD (`.bm-folder` carries no
    /// `data-action` hook in the frozen markup) — only the active-folder
    /// highlight is bound.
    pub folders: Vec<ChromeBookmarkFolderModel>,
    /// `.bm-toolbar .title` — the active folder's display name.
    pub title: String,
    /// `.bm-grid` cards, in `BookmarkPanel::visible_entries()` order.
    pub cards: Vec<ChromeBookmarkCardModel>,
}

/// One `.bm-folder` row.
#[derive(Debug, Clone, PartialEq)]
pub struct ChromeBookmarkFolderModel {
    /// Folder label (`"Все закладки"` for the root/no-filter entry).
    pub label: String,
    /// `true` for the currently selected filter — adds `.active`.
    pub active: bool,
}

/// One `.bm-card` in `#view-bookmarks`'s `.bm-grid` (CC-10b). Per-card
/// actions are out of this slice's DoD — same gap as
/// [`ChromeHistoryRow::Entry`].
#[derive(Debug, Clone, PartialEq)]
pub struct ChromeBookmarkCardModel {
    /// `.bm-fav` single-letter fallback (first letter of `title`/`url`).
    pub fav_letter: String,
    /// `.bm-title`.
    pub title: String,
    /// `.bm-url`.
    pub url: String,
}

/// `#view-settings` snapshot (CC-10b) — mirrors a subset of `SettingsPanel`.
///
/// The design's 6 section tabs (general/privacy/appearance/sync/ext/qa) only
/// partially overlap `SettingsPanel::SettingsSection`'s 7 (no sync/ext/qa
/// section exists there; no downloads/network/adblock/language section
/// exists here) — `active_section` is therefore engine-chrome-only UI state
/// (`Lumen::chrome_settings_section`), not a projection of the legacy enum.
/// Only the two Adblock & Fingerprinting toggles with a clean 1:1 backing
/// field are bound (`ad_block_on`/`fingerprint_on`); the Shields
/// radio-cards, "Принудительный HTTPS" toggle, the General/Appearance/Sync
/// radio-cards, and the Permissions table are left as the design's static
/// demo content — none has a matching real-state field (shields is a single
/// on/off, not the 3-tier Standard/Strict/Tor-like the design shows; there
/// is no force-HTTPS setting; there is no cross-site permission list) — same
/// honesty-over-fabrication call CC-9/CC-10a made for `#statAds`/`#statFp`
/// and the cert panel's missing TLS-version row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeSettingsModel {
    /// `data-section`/`data-set` slug of the active `.set-nav` tab /
    /// `.set-section`.
    pub active_section: String,
    /// `SettingsPanel::draft.shields_enabled` — binds the "Блокировать
    /// рекламу" toggle.
    pub ad_block_on: bool,
    /// `SettingsPanel::draft.fingerprint_mode != "off"` — binds the
    /// "Блокировать фингерпринтинг" toggle.
    pub fingerprint_on: bool,
}

/// `#rightSidebar` snapshot (CC-10b) — merges the legacy `AiPanel`/
/// `SidebarPanel` (independently dockable) into the design's single tabbed
/// panel. The shell keeps them mutually exclusive under the flag
/// (`Lumen::dispatch_chrome_action`) so `tab` is unambiguous.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeRightSidebarModel {
    /// `true` shows `#rightSidebar` — mirrors `AiPanel::visible ||
    /// SidebarPanel::visible`.
    pub open: bool,
    /// Which tab is active.
    pub tab: ChromeSidebarTab,
}

/// `#rightSidebar`'s two tabs (CC-10b).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChromeSidebarTab {
    /// AI assistant chat — `#rsBodyAi`.
    #[default]
    Ai,
    /// Embedded web widget — `#rsBodyWeb`. Its content is a real secondary
    /// webview in the legacy panel (`SidebarPanel`), not representable as
    /// static markup, so the design's own placeholder text is left as-is.
    Web,
}

/// `#omniDropdown` snapshot (CC-9): whether it's open, plus its suggestion
/// rows, rebuilt the same way [`ChromeTabModel`] rebuilds `#sbTabs`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeDropdownModel {
    /// `true` shows the dropdown — mirrors the legacy gate
    /// (`AddressBarState::is_open()` with a non-empty suggestion list).
    pub open: bool,
    /// Suggestion rows, in display order.
    pub suggestions: Vec<ChromeSuggestionModel>,
}

/// One `.dd-row` in `#omniDropdown` (CC-9).
///
/// Mirrors the shell's `OmniboxSuggestion` (`address_bar.rs`) shaped down to
/// what the asset's row markup can show — label, sub-label, and an accent
/// color for the `.dd-icon` swatch. Like [`ChromeTabModel`]'s favicon
/// fallback, this deliberately skips cloning the asset's inline SVG icon
/// sprite (visual finish, not part of this slice's DoD).
#[derive(Debug, Clone, PartialEq)]
pub struct ChromeSuggestionModel {
    /// Round-trips through `data-sugg-idx` so a click on the rebuilt row can
    /// be resolved back to `AddressBarState::suggestions()[idx]`.
    pub idx: usize,
    /// Main text (`.dd-title`).
    pub label: String,
    /// Secondary text (`.dd-sub`).
    pub sub_label: String,
    /// `#RRGGBB` accent for the `.dd-icon` swatch background.
    pub color: String,
}

/// `#findBar` snapshot (CC-9) — mirrors [`OmniboxModel`]'s "engine renders,
/// legacy `FindState` still owns editing" split (CC-7).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeFindModel {
    /// `true` shows the bar — mirrors `FindState::is_open()`.
    pub open: bool,
    /// Written into `#findInput`'s `value` attribute.
    pub value: String,
    /// Written into `#findCount`'s text (e.g. `"2/5"` or `"0/0"`).
    pub count_label: String,
}

/// One `.dl-card` in `#downloadsPanel`'s `.dl-list` (CC-9).
///
/// Pre-formatted by the shell (`Lumen::chrome_model_snapshot`, reusing
/// `download::extension_label`/`human_bytes`) so this crate stays free of
/// download-domain formatting logic, matching how [`ChromeTabModel`] already
/// receives a pre-derived `container_color` rather than a `ContainerKind`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChromeDownloadModel {
    /// Round-trips through `data-dl-id` (`DownloadId`, opaque here).
    pub id: u32,
    /// Uppercase extension badge text (`.dl-icon`), e.g. `"PDF"`.
    pub ext_label: String,
    /// File name (`.dl-name`).
    pub name: String,
    /// Size/status line (`.dl-meta`), e.g. `"2.1 MB — идёт загрузка…"`.
    pub meta: String,
    /// `Some(fraction)` shows a `.dl-progress-track`/`.dl-progress-fill` bar
    /// at this fill (in-flight downloads only); `None` omits the bar
    /// (matches the asset's own "done" cards, which carry no progress track).
    pub progress_fraction: Option<f32>,
}

/// `#cpOverlay` snapshot (CC-10) — mirrors `CommandPalette`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromePaletteModel {
    /// `true` shows the overlay — mirrors `CommandPalette::visible`.
    pub open: bool,
    /// Written into `#cpInput`'s `value` attribute.
    pub query: String,
    /// Result rows, in ranked order (already capped to the same
    /// `MAX_VISIBLE_ROWS`/scroll-window slice the legacy `build_panel` shows).
    pub results: Vec<ChromePaletteResultModel>,
}

/// One `.cp-row` in `#cpList` (CC-10).
///
/// The frozen design has no example `.cp-row` markup to pattern-match (unlike
/// the omnibox dropdown/download cards) — this reuses the `.dd-icon`/
/// `.dd-text`/`.dd-title`/`.dd-sub` shape `.cp-row .dd-icon{...}`'s own CSS
/// implies (`assets/chrome/chrome.html`), mirroring [`build_dd_row`].
/// Row-click activation is out of this slice's DoD — the design carries no
/// `data-action` for individual result rows (only `#cpOverlay` itself has
/// one, to close on scrim click); keyboard navigation (`select_next`/`prev`
/// + Enter) already works independently of rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct ChromePaletteResultModel {
    /// Main text (`.dd-title`) — command name or bookmark/history title.
    pub label: String,
    /// Secondary text (`.dd-sub`) — keyboard shortcut for commands, URL for
    /// bookmarks/history.
    pub sub_label: String,
    /// `true` for the keyboard-highlighted row — since the design has no
    /// `.cp-row.selected` CSS class, this is rendered as an inline
    /// `background:var(--surface-2)` style (the same shade `.cp-row:hover`
    /// uses), not a class.
    pub selected: bool,
}

/// `#certOverlay` snapshot (CC-10) — mirrors `CertPanel`/`PanelCertData`.
///
/// The design's 6 static `.cert-row`s + 1 `.cert-fp` cover a *subset* of
/// `PanelCertData`'s 9 fields (no TLS version row exists) — this binds only
/// what the markup has a slot for, same honesty-over-fabrication call CC-9
/// made for `#statAds`/`#statFp`. All-`None`/absent fields render as `"—"`,
/// matching `cert_panel::build_rows`'s own em-dash fallback for missing data.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeCertModel {
    /// `true` shows the overlay — mirrors `CertPanel::visible`.
    pub open: bool,
    /// `<h3>` title text, e.g. `"Сертификат — example.com"`.
    pub title: String,
    /// The 6 `.cert-row` values, in the design's fixed document order:
    /// `[subject_cn, subject_org, san, issuer, not_before, not_after]`.
    pub rows: [String; 6],
    /// `.cert-fp` text.
    pub fingerprint: String,
}

/// Grant state for one permission row (CC-9) — mirrors the shell's
/// `PermissionState` shaped down to the asset's two-button (allow/deny, no
/// explicit "ask" control) markup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChromePermState {
    /// Adds `.selected-allow` to the row's `.perm-btn.allow` button.
    Allow,
    /// Adds `.selected-deny` to the row's `.perm-btn.deny` button.
    Deny,
    /// Neither button is marked selected.
    #[default]
    Ask,
}

/// Omnibox snapshot [`bind_model`] reflects into `#omniInput`/`#omniWarn`
/// (CC-7, `docs/tasks/p1-css-chrome.md`).
///
/// Editing itself (caret, IME, selection) stays owned by the shell's legacy
/// `address_bar::AddressBarState` — this is only the text/warning it writes
/// into the chrome document each `bind_model` call, mirroring how CC-6 binds
/// tabs/workspaces without owning their state either.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OmniboxModel {
    /// Text written into `#omniInput`'s `value` attribute: the current
    /// display URL while not editing, or the live `address_bar` input while
    /// editing (both already IDN-guarded by the caller). Empty falls back to
    /// the asset's own `placeholder` text.
    pub value: String,
    /// `Some(message)` shows `#omniWarn` (adds `.show`) with this spoof-guard
    /// warning text; `None` hides it.
    pub warning: Option<String>,
}

/// One tab row for the sidebar tab list (`#sbTabs`).
#[derive(Debug, Clone, PartialEq)]
pub struct ChromeTabModel {
    /// Stable id (`TabEntry::id`) — round-tripped through `data-tab-id` so a
    /// click on the rebuilt row can be resolved back to a strip index.
    pub id: usize,
    /// Tab title, shown in `.tab-title` and used to derive the `.tab-fav`
    /// single-letter fallback favicon.
    pub title: String,
    /// `true` for the foreground tab — adds the `.active` class.
    pub active: bool,
    /// `true` for a hibernated (T3) tab — adds the `.sleeping` class and
    /// swaps the close button for a `.tab-badge` (mirrors the asset's own
    /// hibernated row, which carries no close button).
    pub sleeping: bool,
    /// `true` when the tab has an opener (`TabEntry::opener_id`, tree-style
    /// tabs 7A.2) — adds the `.child` class + a `.tree-line` connector span
    /// (CC-8). The asset's CSS only defines indentation for a single nesting
    /// level (`.tab-row.child`), so a grandchild renders at the same indent
    /// as its parent's children rather than one level deeper — a known
    /// limitation of the frozen reference, not this binding.
    pub is_child: bool,
    /// `#RRGGBB` accent for the container strip (`.container-stripe`) — the
    /// caller derives this from `TabEntry::container.border_color()`, or
    /// `None` for `ContainerKind::None`, in which case the strip is omitted
    /// entirely (matching the asset: only container-scoped rows carry it).
    pub container_color: Option<String>,
}

/// One workspace button for the sidebar switcher (`.sb-workspaces`).
#[derive(Debug, Clone, PartialEq)]
pub struct ChromeWorkspaceModel {
    /// Stable id (`WsEntry::id`) — round-tripped through `data-ws-id`.
    pub id: i64,
    /// Workspace display name, shown in `.lbl` and used to derive the
    /// `.ws-icon` single-letter fallback.
    pub name: String,
    /// `true` for the active workspace — adds the `.active` class.
    pub active: bool,
    /// `#RRGGBB` accent colour, written as the `--ws-color` custom property
    /// (CC-8) on the switcher item/pill and its icon background — the CSS
    /// asset's `.ws-item.active`/`.hbar-ws-pill.active` border falls back to
    /// this when active.
    pub color: String,
}

/// What one [`bind_model_tracked`] call changed in the document, split by what
/// each kind of consumer needs (BUG-341 S6 for `selector`, S16 for `content`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChromeMutations {
    /// Nodes whose *selector-relevant* state changed — an attribute value (most
    /// commonly `class`) or a row-list container's child count — mapped to
    /// *what* changed about them. Feeds
    /// `lumen_layout::style::restyle_root_set_for_node_change`, i.e. decides
    /// which nodes must re-run the cascade.
    ///
    /// BUG-341 S17: the attribute *names* are carried, not just the node ids.
    /// A root-set that knows only "something on `#omniInput` changed" has to
    /// assume a sibling rule could react and widen to the parent's whole
    /// subtree; one that knows the name is `value` can ask whether any selector
    /// in the sheet reaches a sibling from a compound matching that node.
    pub selector: HashMap<NodeId, SelectorTouch>,
    /// Nodes whose *content* changed — text-node data, an element's child list,
    /// or any attribute (`build_box` reads `src`/`value`/`width`/… directly, so
    /// every attribute write counts here too). Feeds
    /// `lumen_layout::counters::ContentDirty::Nodes`, i.e. decides which box
    /// subtrees may be cloned from the previous pass instead of rebuilt.
    ///
    /// This set must be **complete**: a content mutation that goes unreported
    /// yields a stale `LayoutBox` — visible corruption, not a slow frame. The
    /// completeness argument is structural, not by inspection: every write into
    /// the document from this module goes through one of the tracked primitives
    /// below, and
    /// `every_dom_mutation_in_model_rs_goes_through_a_tracked_primitive`
    /// re-checks that at test time against this file's own source.
    pub content: HashSet<NodeId>,
}

impl ChromeMutations {
    /// `true` when this bind changed nothing at all.
    pub fn is_empty(&self) -> bool {
        self.selector.is_empty() && self.content.is_empty()
    }
}

/// What changed about one selector-relevant node in a [`ChromeMutations`]
/// report (BUG-341 S17).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectorTouch {
    /// Names of the attributes written to (or removed from) this node.
    /// Lowercase as written by the `bind_*` helpers.
    pub attrs: BTreeSet<String>,
    /// The node's child list changed. No attribute name describes that, and
    /// `:nth-child`/`:empty`/sibling combinators all react to it, so a
    /// structural touch always takes the conservative widen-to-parent path.
    pub structural: bool,
}

thread_local! {
    /// BUG-341 S6/S16 — collects what [`bind_model`] actually mutated during
    /// the call currently in progress. `None` when no [`bind_model_tracked`]
    /// call is active, so plain [`bind_model`] (still used for the very first
    /// bind, before any previous cascade cache exists to diff against) pays no
    /// tracking cost.
    static MUTATION_TRACKER: RefCell<Option<ChromeMutations>> = const { RefCell::new(None) };
}

/// Records a write to `id`'s `attr` attribute in the [`bind_model_tracked`]
/// call currently in progress, if any. Also records `id` as content-dirty —
/// `build_box` reads attributes (`src`/`value`/`width`/…) directly. A no-op
/// outside [`bind_model_tracked`].
fn record_attr(id: NodeId, attr: &str) {
    MUTATION_TRACKER.with(|t| {
        if let Some(m) = t.borrow_mut().as_mut() {
            m.selector.entry(id).or_default().attrs.insert(attr.to_ascii_lowercase());
            m.content.insert(id);
        }
    });
}

/// Records `id`'s child list as changed in the [`bind_model_tracked`] call
/// currently in progress, if any — selector-relevant (`:nth-child`, `:empty`,
/// sibling combinators) *and* content-relevant. A no-op outside
/// [`bind_model_tracked`].
fn record_structural(id: NodeId) {
    MUTATION_TRACKER.with(|t| {
        if let Some(m) = t.borrow_mut().as_mut() {
            m.selector.entry(id).or_default().structural = true;
            m.content.insert(id);
        }
    });
}

/// Records `id` as content-dirty only (BUG-341 S16) — its text data or child
/// list moved, but nothing a selector can match on. A no-op outside
/// [`bind_model_tracked`].
fn record_content(id: NodeId) {
    MUTATION_TRACKER.with(|t| {
        if let Some(m) = t.borrow_mut().as_mut() {
            m.content.insert(id);
        }
    });
}

// ─── Tracked DOM primitives (BUG-341 S16) ────────────────────────────────────
//
// Every mutation of `doc` made by this module goes through one of the four
// functions below (plus `set_attr`/`remove_attr`, which own the only other
// `doc.get_mut`). Nothing else in this file may call `Document::append_child`,
// `insert_before`, `detach` or `get_mut` directly — the
// `S16-tracked-primitive` marker comments are what the source-level gate test
// keys on, so keep them on the exact lines making those calls.

/// [`Document::append_child`] + content-dirty bookkeeping for `parent`.
///
/// Recording is unconditional, including while a detached subtree is being
/// assembled (`parent` not yet in the document). That over-reports by exactly
/// the freshly-created nodes, which have no entry in `prev_styles` and so can
/// never be reused anyway — the conservative direction costs nothing here.
fn attach_child(doc: &mut Document, parent: NodeId, child: NodeId) {
    doc.append_child(parent, child); // S16-tracked-primitive
    record_content(parent);
}

/// [`Document::insert_before`] + content-dirty bookkeeping for the parent
/// `new_node` lands in (read off `reference` *before* the insertion, so it is
/// the parent as the DOM saw it).
fn insert_child_before(doc: &mut Document, new_node: NodeId, reference: NodeId) {
    let parent = doc.get(reference).parent;
    doc.insert_before(new_node, reference); // S16-tracked-primitive
    if let Some(parent) = parent {
        record_content(parent);
    }
}

/// [`Document::detach`] + content-dirty bookkeeping for the parent losing the
/// child. The parent is read before the detach, since `detach` clears the link.
fn detach_node(doc: &mut Document, id: NodeId) {
    let parent = doc.get(id).parent;
    doc.detach(id); // S16-tracked-primitive
    if let Some(parent) = parent {
        record_content(parent);
    }
}

/// Binds `model` into `doc`: `data-theme`/`data-layout`/`data-profile` on
/// `<body>`, and a full rebuild of the tab list and workspace switcher.
///
/// Called by the shell (`Lumen::relayout_chrome_host`) before every chrome
/// layout pass, so it always reflects current shell state — no separate
/// dirty-tracking. Cheap: a handful of attribute/text mutations plus two
/// small list rebuilds (tens of nodes), not a full re-parse.
pub fn bind_model(doc: &mut Document, model: &ChromeModel) {
    if let Some(body) = doc.body() {
        set_attr(doc, body, "data-theme", if model.dark_theme { "dark" } else { "light" });
        set_attr(
            doc,
            body,
            "data-layout",
            if model.layout_vertical { "vertical" } else { "horizontal" },
        );
        match &model.profile_slug {
            Some(slug) => set_attr(doc, body, "data-profile", slug),
            None => remove_attr(doc, body, "data-profile"),
        }
    }
    if let Some(sidebar) = doc.find_by_id(crate::ids::SIDEBAR) {
        set_class_token(doc, sidebar, "collapsed", model.sidebar_collapsed);
    }
    if let Some(container) = doc.find_by_id(crate::ids::SB_TABS) {
        rebuild_tab_list(doc, container, &model.tabs);
    }
    if let Some(container) = doc.find_by_id(crate::ids::HBAR_TABS) {
        rebuild_hbar_tab_list(doc, container, &model.tabs);
    }
    if let Some(container) = find_by_attr(doc, "data-testid", "workspace-switcher") {
        rebuild_workspace_list(doc, container, &model.workspaces);
    }
    if let Some(container) = find_by_class(doc, "hbar-ws") {
        rebuild_hbar_ws_list(doc, container, &model.workspaces);
    }
    bind_omnibox(doc, &model.omnibox);
    bind_dropdown(doc, &model.dropdown);
    bind_find_bar(doc, &model.find);
    bind_downloads(doc, model.downloads_open, &model.downloads);
    bind_popover(doc, model.popover_open, model.blocked_total, &model.permissions);
    bind_palette(doc, &model.palette);
    bind_cert(doc, &model.cert);
    if let Some(overlay) = doc.find_by_id(crate::ids::PRINT_OVERLAY) {
        set_class_token(doc, overlay, "open", model.print_open);
    }
    bind_content_view(doc, model.content_view);
    bind_history(doc, &model.history);
    bind_bookmarks(doc, &model.bookmarks);
    bind_settings(doc, &model.settings);
    bind_right_sidebar(doc, &model.right_sidebar);
}

/// Like [`bind_model`], but also reports what the call actually changed, split
/// into the two questions the incremental pipeline asks separately (see
/// [`ChromeMutations`]).
///
/// BUG-341 S6: `selector` is the raw material
/// [`lumen_layout::style::restyle_root_set_for_node_change`] needs to derive
/// a correct cascade dirty-root-set for a `bind_model` call that changed real
/// DOM content (tab title, omnibox text, …), not just interactive state — the
/// gap S5 left `CC12_KEY` and any content-mutating chrome interaction stuck
/// on the full-layout path.
///
/// BUG-341 S16: `content` is the same for box reuse. Before S16 a text-only
/// change was reported nowhere — correct for the cascade (text cannot change
/// selector matching) but it forced the caller to declare the *whole document*
/// content-unstable, which disabled box reuse for all 318 chrome boxes over one
/// changed omnibox character. `content` names the mutated nodes instead, so
/// only the subtrees actually containing one lose their reuse.
pub fn bind_model_tracked(doc: &mut Document, model: &ChromeModel) -> ChromeMutations {
    MUTATION_TRACKER.with(|t| *t.borrow_mut() = Some(ChromeMutations::default()));
    bind_model(doc, model);
    MUTATION_TRACKER.with(|t| t.borrow_mut().take()).unwrap_or_default()
}

/// Toggles `.active` on exactly one of `#view-page`/`#view-history`/
/// `#view-bookmarks`/`#view-settings` per `view` (CC-10b).
fn bind_content_view(doc: &mut Document, view: ChromeContentView) {
    let views = [
        (crate::ids::VIEW_PAGE, ChromeContentView::Page),
        (crate::ids::VIEW_HISTORY, ChromeContentView::History),
        (crate::ids::VIEW_BOOKMARKS, ChromeContentView::Bookmarks),
        (crate::ids::VIEW_SETTINGS, ChromeContentView::Settings),
    ];
    for (id, kind) in views {
        if let Some(node) = doc.find_by_id(id) {
            set_class_token(doc, node, "active", kind == view);
        }
    }
}

/// Toggles `#histBanner` and rebuilds `.hist-day`/`.hist-item` rows inside
/// `.hist-wrap` from [`ChromeHistoryModel::rows`] (CC-10b). Only the two
/// row classes are removed/rebuilt — `.hist-banner`/`.hist-head` (search box
/// + toolbar buttons) are static siblings, left untouched.
fn bind_history(doc: &mut Document, history: &ChromeHistoryModel) {
    if let Some(banner) = doc.find_by_id(crate::ids::HIST_BANNER) {
        set_attr(doc, banner, "style", if history.banner { "" } else { "display:none" });
    }
    let Some(wrap) = find_by_class(doc, "hist-wrap") else { return };
    remove_children_with_class(doc, wrap, "hist-day");
    remove_children_with_class(doc, wrap, "hist-item");
    for row in &history.rows {
        let node = match row {
            ChromeHistoryRow::Group(label) => {
                let day = doc.create_element(QualName::html("div"));
                set_attr(doc, day, "class", "hist-day");
                append_text(doc, day, label);
                day
            }
            ChromeHistoryRow::Entry { title, url, time_label } => {
                build_hist_item(doc, title, url, time_label)
            }
        };
        attach_child(doc, wrap, node);
    }
}

fn build_hist_item(doc: &mut Document, title: &str, url: &str, time_label: &str) -> NodeId {
    let item = doc.create_element(QualName::html("div"));
    set_attr(doc, item, "class", "hist-item");

    let fav = doc.create_element(QualName::html("div"));
    set_attr(doc, fav, "class", "hist-fav");
    append_text(doc, fav, &first_letter(title));
    attach_child(doc, item, fav);

    let text = doc.create_element(QualName::html("div"));
    let title_el = doc.create_element(QualName::html("div"));
    set_attr(doc, title_el, "class", "hist-title");
    append_text(doc, title_el, title);
    attach_child(doc, text, title_el);
    let url_el = doc.create_element(QualName::html("div"));
    set_attr(doc, url_el, "class", "hist-url");
    append_text(doc, url_el, url);
    attach_child(doc, text, url_el);
    attach_child(doc, item, text);

    let time = doc.create_element(QualName::html("div"));
    set_attr(doc, time, "class", "hist-time");
    append_text(doc, time, time_label);
    attach_child(doc, item, time);

    item
}

/// Rebuilds `.bm-tree`'s `.bm-folder` list, `.bm-toolbar .title`, and
/// `.bm-grid`'s `.bm-card` list from `bookmarks` (CC-10b).
fn bind_bookmarks(doc: &mut Document, bookmarks: &ChromeBookmarksModel) {
    if let Some(tree) = find_by_class(doc, "bm-tree") {
        remove_children_with_class(doc, tree, "bm-folder");
        for (i, folder) in bookmarks.folders.iter().enumerate() {
            let node = doc.create_element(QualName::html("div"));
            let mut class = "bm-folder".to_owned();
            if folder.active {
                class.push_str(" active");
            }
            if i > 0 {
                class.push_str(" indent");
            }
            set_attr(doc, node, "class", &class);
            append_text(doc, node, &folder.label);
            attach_child(doc, tree, node);
        }
    }
    if let Some(toolbar) = find_by_class(doc, "bm-toolbar")
        && let Some(title) = find_descendant_by_class(doc, toolbar, "title")
    {
        set_text(doc, title, &bookmarks.title);
    }
    let Some(grid) = find_by_class(doc, "bm-grid") else { return };
    remove_children_with_class(doc, grid, "bm-card");
    for card in &bookmarks.cards {
        let node = build_bm_card(doc, card);
        attach_child(doc, grid, node);
    }
}

fn build_bm_card(doc: &mut Document, card: &ChromeBookmarkCardModel) -> NodeId {
    let node = doc.create_element(QualName::html("div"));
    set_attr(doc, node, "class", "bm-card");

    let fav = doc.create_element(QualName::html("div"));
    set_attr(doc, fav, "class", "bm-fav");
    append_text(doc, fav, &card.fav_letter);
    attach_child(doc, node, fav);

    let title = doc.create_element(QualName::html("div"));
    set_attr(doc, title, "class", "bm-title");
    append_text(doc, title, &card.title);
    attach_child(doc, node, title);

    let url = doc.create_element(QualName::html("div"));
    set_attr(doc, url, "class", "bm-url");
    append_text(doc, url, &card.url);
    attach_child(doc, node, url);

    node
}

/// Toggles `.set-nav .item`/`.set-section` `.active` per
/// [`ChromeSettingsModel::active_section`], and the two mapped
/// `.toggle`s' `.on` class inside the Privacy section (CC-10b).
fn bind_settings(doc: &mut Document, settings: &ChromeSettingsModel) {
    if let Some(nav) = find_by_class(doc, "set-nav") {
        for item in doc.get(nav).children.clone() {
            let is_active = doc.get(item).get_attr("data-section") == Some(settings.active_section.as_str());
            set_class_token(doc, item, "active", is_active);
        }
    }
    let Some(main) = find_by_class(doc, "set-main") else { return };
    for section in doc.get(main).children.clone() {
        let is_active = doc.get(section).get_attr("data-set") == Some(settings.active_section.as_str());
        set_class_token(doc, section, "active", is_active);
    }
    let privacy =
        doc.get(main).children.iter().copied().find(|&c| doc.get(c).get_attr("data-set") == Some("privacy"));
    if let Some(privacy) = privacy {
        let toggles = find_descendants_by_class(doc, privacy, "toggle");
        if let Some(&ad) = toggles.first() {
            set_class_token(doc, ad, "on", settings.ad_block_on);
        }
        if let Some(&fp) = toggles.get(1) {
            set_class_token(doc, fp, "on", settings.fingerprint_on);
        }
    }
}

/// Toggles `#rightSidebar`'s `.open` class, the active `.rs-tab`, `#rsTitle`,
/// and swaps `#rsBodyAi`/`#rsBodyWeb` visibility per `sidebar.tab` (CC-10b).
fn bind_right_sidebar(doc: &mut Document, sidebar: &ChromeRightSidebarModel) {
    let Some(panel) = doc.find_by_id(crate::ids::RIGHT_SIDEBAR) else { return };
    set_class_token(doc, panel, "open", sidebar.open);
    let is_ai = sidebar.tab == ChromeSidebarTab::Ai;
    if let Some(tabs) = find_descendant_by_class(doc, panel, "rs-tabs") {
        for tab_btn in doc.get(tabs).children.clone() {
            let this_is_ai = doc.get(tab_btn).get_attr("data-rs-tab") == Some("ai");
            set_class_token(doc, tab_btn, "active", this_is_ai == is_ai);
        }
    }
    if let Some(title) = doc.find_by_id(crate::ids::RS_TITLE) {
        set_text(doc, title, if is_ai { "AI" } else { "Web" });
    }
    if let Some(ai_body) = doc.find_by_id(crate::ids::RS_BODY_AI) {
        set_attr(doc, ai_body, "style", if is_ai { "" } else { "display:none" });
    }
    if let Some(web_body) = doc.find_by_id(crate::ids::RS_BODY_WEB) {
        set_attr(doc, web_body, "style", if is_ai { "display:none" } else { "" });
    }
}

/// Toggles `#cpOverlay`'s `.open` class, writes `#cpInput`'s value, and
/// rebuilds `#cpList`'s `.cp-row` list from [`ChromePaletteModel::results`]
/// (CC-10).
fn bind_palette(doc: &mut Document, palette: &ChromePaletteModel) {
    let Some(overlay) = doc.find_by_id(crate::ids::CP_OVERLAY) else { return };
    set_class_token(doc, overlay, "open", palette.open);
    if let Some(input) = doc.find_by_id(crate::ids::CP_INPUT) {
        set_attr(doc, input, "value", &palette.query);
    }
    let Some(list) = doc.find_by_id(crate::ids::CP_LIST) else { return };
    if palette.results.is_empty() {
        // BUG-341 S6: skip the remove+recreate entirely when the empty
        // placeholder is already showing — otherwise `#cpList` gets reported
        // touched (`bind_model_tracked`) on every single `bind_model` call
        // even while the palette stays closed, the overwhelmingly common
        // case, needlessly widening the incremental cascade's dirty-root-set
        // every cycle for a state that never actually changes.
        let already_empty = doc.get(list).children.len() == 1
            && has_class(doc, doc.get(list).children[0], "cp-empty");
        if already_empty {
            return;
        }
        remove_children_with_class(doc, list, "cp-row");
        let empty = doc.create_element(QualName::html("div"));
        set_attr(doc, empty, "class", "cp-empty cp-row");
        append_text(doc, empty, "Ничего не найдено");
        attach_child(doc, list, empty);
        return;
    }
    remove_children_with_class(doc, list, "cp-row");
    for r in &palette.results {
        let row = build_cp_row(doc, r);
        attach_child(doc, list, row);
    }
}

fn build_cp_row(doc: &mut Document, r: &ChromePaletteResultModel) -> NodeId {
    let row = doc.create_element(QualName::html("div"));
    set_attr(doc, row, "class", "cp-row");
    if r.selected {
        set_attr(doc, row, "style", "background:var(--surface-2)");
    }

    let icon = doc.create_element(QualName::html("span"));
    set_attr(doc, icon, "class", "dd-icon");
    attach_child(doc, row, icon);

    let text = doc.create_element(QualName::html("div"));
    set_attr(doc, text, "class", "dd-text");
    let title = doc.create_element(QualName::html("div"));
    set_attr(doc, title, "class", "dd-title");
    append_text(doc, title, &r.label);
    attach_child(doc, text, title);
    let sub = doc.create_element(QualName::html("div"));
    set_attr(doc, sub, "class", "dd-sub");
    append_text(doc, sub, &r.sub_label);
    attach_child(doc, text, sub);
    attach_child(doc, row, text);

    row
}

/// Toggles `#certOverlay`'s `.open` class, writes the `<h3>` title, and
/// writes [`ChromeCertModel::rows`]/`fingerprint` into the 6 static
/// `.cert-row .v` cells (in document order) + `.cert-fp` (CC-10).
fn bind_cert(doc: &mut Document, cert: &ChromeCertModel) {
    let Some(overlay) = doc.find_by_id(crate::ids::CERT_OVERLAY) else { return };
    set_class_token(doc, overlay, "open", cert.open);
    if let Some(h3) = find_descendant_by_tag(doc, overlay, "h3") {
        set_text(doc, h3, &cert.title);
    }
    let rows = find_descendants_by_class(doc, overlay, "cert-row");
    for (row, value) in rows.iter().zip(cert.rows.iter()) {
        if let Some(v) = doc.get(*row).children.iter().copied().find(|&c| has_class(doc, c, "v")) {
            set_text(doc, v, value);
        }
    }
    if let Some(fp) = find_descendant_by_class(doc, overlay, "cert-fp") {
        set_text(doc, fp, &cert.fingerprint);
    }
}

/// Rebuilds `#omniDropdown`'s `.dd-row` list from
/// [`ChromeDropdownModel::suggestions`] and toggles its `.open` class (CC-9).
fn bind_dropdown(doc: &mut Document, dropdown: &ChromeDropdownModel) {
    let Some(container) = doc.find_by_id(crate::ids::OMNI_DROPDOWN) else { return };
    set_class_token(doc, container, "open", dropdown.open);
    remove_children_with_class(doc, container, "dd-row");
    for s in &dropdown.suggestions {
        let row = build_dd_row(doc, s);
        attach_child(doc, container, row);
    }
}

fn build_dd_row(doc: &mut Document, s: &ChromeSuggestionModel) -> NodeId {
    let row = doc.create_element(QualName::html("div"));
    set_attr(doc, row, "class", "dd-row");
    set_attr(doc, row, "data-action", "omni-go");
    set_attr(doc, row, "data-sugg-idx", &s.idx.to_string());

    let icon = doc.create_element(QualName::html("span"));
    set_attr(doc, icon, "class", "dd-icon");
    set_attr(doc, icon, "style", &format!("background:{}", s.color));
    attach_child(doc, row, icon);

    let text = doc.create_element(QualName::html("div"));
    set_attr(doc, text, "class", "dd-text");
    let title = doc.create_element(QualName::html("div"));
    set_attr(doc, title, "class", "dd-title");
    append_text(doc, title, &s.label);
    attach_child(doc, text, title);
    let sub = doc.create_element(QualName::html("div"));
    set_attr(doc, sub, "class", "dd-sub");
    append_text(doc, sub, &s.sub_label);
    attach_child(doc, text, sub);
    attach_child(doc, row, text);

    row
}

/// Toggles `#findBar`'s `.open` class and writes [`ChromeFindModel::value`]/
/// `count_label` into `#findInput`/`#findCount` (CC-9). Editing (caret,
/// append/backspace) stays owned by the legacy `FindState`, mirroring
/// [`bind_omnibox`].
fn bind_find_bar(doc: &mut Document, find: &ChromeFindModel) {
    let Some(bar) = doc.find_by_id(crate::ids::FIND_BAR) else { return };
    set_class_token(doc, bar, "open", find.open);
    if let Some(input) = doc.find_by_id(crate::ids::FIND_INPUT) {
        set_attr(doc, input, "value", &find.value);
    }
    if let Some(count) = doc.find_by_id(crate::ids::FIND_COUNT) {
        set_text(doc, count, &find.count_label);
    }
}

/// Toggles `#downloadsPanel`'s `.open` class and rebuilds `.dl-card` rows
/// from `downloads` (CC-9).
fn bind_downloads(doc: &mut Document, open: bool, downloads: &[ChromeDownloadModel]) {
    let Some(panel) = doc.find_by_id(crate::ids::DOWNLOADS_PANEL) else { return };
    set_class_token(doc, panel, "open", open);
    let Some(list) = find_by_class(doc, "dl-list") else { return };
    remove_children_with_class(doc, list, "dl-card");
    for d in downloads {
        let card = build_dl_card(doc, d);
        attach_child(doc, list, card);
    }
}

fn build_dl_card(doc: &mut Document, d: &ChromeDownloadModel) -> NodeId {
    let card = doc.create_element(QualName::html("div"));
    set_attr(doc, card, "class", "dl-card");
    set_attr(doc, card, "data-dl-id", &d.id.to_string());

    let row = doc.create_element(QualName::html("div"));
    set_attr(doc, row, "class", "dl-row");
    let icon = doc.create_element(QualName::html("div"));
    set_attr(doc, icon, "class", "dl-icon");
    append_text(doc, icon, &d.ext_label);
    attach_child(doc, row, icon);

    let text_wrap = doc.create_element(QualName::html("div"));
    let name = doc.create_element(QualName::html("div"));
    set_attr(doc, name, "class", "dl-name");
    append_text(doc, name, &d.name);
    attach_child(doc, text_wrap, name);
    let meta = doc.create_element(QualName::html("div"));
    set_attr(doc, meta, "class", "dl-meta");
    append_text(doc, meta, &d.meta);
    attach_child(doc, text_wrap, meta);
    attach_child(doc, row, text_wrap);
    attach_child(doc, card, row);

    if let Some(fraction) = d.progress_fraction {
        let track = doc.create_element(QualName::html("div"));
        set_attr(doc, track, "class", "dl-progress-track");
        let fill = doc.create_element(QualName::html("div"));
        set_attr(doc, fill, "class", "dl-progress-fill");
        set_attr(doc, fill, "style", &format!("width:{}%", (fraction.clamp(0.0, 1.0) * 100.0)));
        attach_child(doc, track, fill);
        attach_child(doc, card, track);
    }

    card
}

/// Toggles `#permPopover`'s `.open` class, writes `blocked_total` into
/// `#statTrackers`, and marks the two permission rows' allow/deny buttons
/// per `permissions` (CC-9).
fn bind_popover(doc: &mut Document, open: bool, blocked_total: u32, permissions: &[ChromePermState; 2]) {
    let Some(popover) = doc.find_by_id(crate::ids::PERM_POPOVER) else { return };
    set_class_token(doc, popover, "open", open);
    if let Some(stat) = doc.find_by_id(crate::ids::STAT_TRACKERS) {
        set_text(doc, stat, &blocked_total.to_string());
    }
    let rows: Vec<NodeId> =
        doc.get(popover).children.iter().copied().filter(|&c| has_class(doc, c, "perm-row")).collect();
    for (row, state) in rows.into_iter().zip(permissions.iter()) {
        bind_permission_row(doc, row, *state);
    }
}

fn bind_permission_row(doc: &mut Document, row: NodeId, state: ChromePermState) {
    let Some(actions) = doc.get(row).children.iter().copied().find(|&c| has_class(doc, c, "perm-actions"))
    else {
        return;
    };
    let buttons: Vec<NodeId> = doc.get(actions).children.clone();
    for btn in buttons {
        if has_class(doc, btn, "allow") {
            set_class_token(doc, btn, "selected-allow", state == ChromePermState::Allow);
        } else if has_class(doc, btn, "deny") {
            set_class_token(doc, btn, "selected-deny", state == ChromePermState::Deny);
        }
    }
}

/// Writes [`OmniboxModel`] into `#omniInput`'s `value` attribute and toggles
/// `#omniWarn`'s `.show` class (CC-7). The warning's text content is only
/// rebuilt while a warning is present — hidden (`.show` absent) stale text
/// left over from a previous warning is harmless, since CSS keeps it
/// `display:none`.
fn bind_omnibox(doc: &mut Document, omnibox: &OmniboxModel) {
    if let Some(input) = doc.find_by_id(crate::ids::OMNI_INPUT) {
        set_attr(doc, input, "value", &omnibox.value);
    }
    let Some(warn) = doc.find_by_id(crate::ids::OMNI_WARN) else { return };
    set_class_token(doc, warn, "show", omnibox.warning.is_some());
    if let Some(message) = &omnibox.warning {
        // BUG-341 S16: update the existing `⚠ <span>` shape in place when it is
        // already there. Rebuilding it unconditionally made a *displayed*
        // warning report two content-dirty nodes on every bind, which would
        // cost the whole document's box reuse for as long as the warning stays
        // up — the same defect `set_text` carried before this slice.
        let children: Vec<NodeId> = doc.get(warn).children.clone();
        if let [_lead, span] = children.as_slice()
            && matches!(doc.get(*span).data, NodeData::Element { .. })
        {
            set_text(doc, *span, message);
            return;
        }
        for child in children {
            detach_node(doc, child);
        }
        append_text(doc, warn, "\u{26A0} ");
        let span = doc.create_element(QualName::html("span"));
        append_text(doc, span, message);
        attach_child(doc, warn, span);
    }
}

/// Reconciles `container`'s `class`-carrying children against `items` by
/// position, preserving each surviving row's `NodeId` via `update` instead
/// of detaching and rebuilding every row on every [`bind_model`] call.
///
/// The previous "clear all, build fresh" approach gave every row (and every
/// one of its descendants) a brand-new `NodeId` on every relayout even when
/// `items` hadn't changed at all — this broke
/// [`lumen_layout::layout_mutation_incremental`]'s `graft_geometry` subtree
/// matching (it compares by node id) and was a hard blocker for CC-14 (see
/// BUG-341: `bind_model` is called on every chrome relayout, so a
/// mouse-hover or keystroke that touches neither tabs nor workspaces still
/// tore down and rebuilt both lists). Rows beyond `items.len()` are
/// `build`-created and inserted before `anchor` (or appended if `anchor` is
/// `None` — the tab-list containers have no trailing sibling); existing
/// rows beyond `items.len()` are detached.
fn reconcile_row_list<T>(
    doc: &mut Document,
    container: NodeId,
    class: &str,
    items: &[T],
    build: impl Fn(&mut Document, &T) -> NodeId,
    update: impl Fn(&mut Document, NodeId, &T),
    anchor: Option<NodeId>,
) {
    let existing: Vec<NodeId> = doc
        .get(container)
        .children
        .iter()
        .copied()
        .filter(|&c| has_class(doc, c, class))
        .collect();

    if items.len() != existing.len() {
        // BUG-341 S6: row count changed — a structural change, relevant to
        // any `:nth-child`/`:last-child`/sibling-combinator rule over this
        // list. Per-row content changes (a row updated in place) are caught
        // by `update`'s own `set_attr` calls instead.
        record_structural(container);
    }

    for (i, item) in items.iter().enumerate() {
        match existing.get(i) {
            Some(&row) => update(doc, row, item),
            None => {
                let row = build(doc, item);
                match anchor {
                    Some(a) => insert_child_before(doc, row, a),
                    None => attach_child(doc, container, row),
                }
            }
        }
    }
    for &row in existing.iter().skip(items.len()) {
        detach_node(doc, row);
    }
}

fn rebuild_tab_list(doc: &mut Document, container: NodeId, tabs: &[ChromeTabModel]) {
    reconcile_row_list(doc, container, "tab-row", tabs, build_tab_row, update_tab_row, None);
}

fn build_tab_row(doc: &mut Document, tab: &ChromeTabModel) -> NodeId {
    let row = doc.create_element(QualName::html("div"));
    set_attr(doc, row, "data-action", "select-tab");
    // CC-13: mirrors the `role="tab"`/`aria-selected` `scripts/gen_chrome_assets.py`
    // bakes into the static asset — this row replaces that static markup
    // wholesale, so the generator's injection never reaches it and has to
    // be set here instead.
    set_attr(doc, row, "role", "tab");
    apply_tab_row_attrs(doc, row, tab);
    populate_tab_row_children(doc, row, tab);
    row
}

/// Sets [`build_tab_row`]'s attributes that never change the row's child
/// shape — shared with [`update_tab_row`], which calls this unconditionally
/// (cheap, idempotent `set_attr`s) even on its fast path.
fn apply_tab_row_attrs(doc: &mut Document, row: NodeId, tab: &ChromeTabModel) {
    let mut class = match (tab.active, tab.sleeping) {
        (true, _) => "tab-row active".to_owned(),
        (false, true) => "tab-row sleeping".to_owned(),
        (false, false) => "tab-row".to_owned(),
    };
    if tab.is_child {
        class.push_str(" child");
    }
    set_attr(doc, row, "class", &class);
    set_attr(doc, row, "data-tab-id", &tab.id.to_string());
    set_attr(doc, row, "aria-selected", if tab.active { "true" } else { "false" });
}

/// Builds `row`'s children fresh (tree-line? / stripe? / fav / title /
/// badge-or-close) — assumes `row` currently has none. Shared by
/// [`build_tab_row`] and [`update_tab_row`]'s shape-mismatch fallback.
fn populate_tab_row_children(doc: &mut Document, row: NodeId, tab: &ChromeTabModel) {
    if tab.is_child {
        let tree_line = doc.create_element(QualName::html("span"));
        set_attr(doc, tree_line, "class", "tree-line");
        attach_child(doc, row, tree_line);
    }

    if let Some(color) = &tab.container_color {
        let stripe = doc.create_element(QualName::html("span"));
        set_attr(doc, stripe, "class", "container-stripe");
        set_attr(doc, stripe, "style", &format!("background:{color}"));
        attach_child(doc, row, stripe);
    }

    let fav = doc.create_element(QualName::html("span"));
    set_attr(doc, fav, "class", "tab-fav");
    append_text(doc, fav, &first_letter(&tab.title));
    attach_child(doc, row, fav);

    let title = doc.create_element(QualName::html("span"));
    set_attr(doc, title, "class", "tab-title");
    append_text(doc, title, &tab.title);
    attach_child(doc, row, title);

    if tab.sleeping {
        let badge = doc.create_element(QualName::html("span"));
        set_attr(doc, badge, "class", "tab-badge");
        set_attr(doc, badge, "title", "Гибернирована");
        append_text(doc, badge, "\u{2726}");
        attach_child(doc, row, badge);
    } else {
        let close = doc.create_element(QualName::html("button"));
        set_attr(doc, close, "class", "tab-close");
        set_attr(doc, close, "data-action", "close-tab");
        set_attr(doc, close, "aria-label", "Закрыть вкладку");
        // Carries its own copy of `data-tab-id` (not just the parent row) so
        // the shell's `chrome_action_at`/`dispatch_chrome_action` — which
        // only sees the `data-action`-carrying node, not the full hit path —
        // can resolve a close click straight to a tab id without walking up
        // to the row.
        set_attr(doc, close, "data-tab-id", &tab.id.to_string());
        attach_child(doc, row, close);
    }
}

/// Updates an existing `.tab-row` (built by [`build_tab_row`] on an earlier
/// [`bind_model`] call) in place instead of discarding it, so an unchanged
/// row keeps its `NodeId` and every descendant's (BUG-341/CC-14 — see
/// [`reconcile_row_list`]). The child slots are matched against the row's
/// *current* shape and updated in place — text via [`set_text`],
/// stripe colour via `set_attr` — as long as the shape still matches `tab`.
/// A shape change (`is_child`/`container_color`-presence/`sleeping`
/// flipped — rare: only real tab-state changes cause this, never a bare
/// hover/keystroke relayout) falls back to clearing and rebuilding the
/// row's children fresh; the row itself still keeps its `NodeId`.
fn update_tab_row(doc: &mut Document, row: NodeId, tab: &ChromeTabModel) {
    apply_tab_row_attrs(doc, row, tab);

    let children: Vec<NodeId> = doc.get(row).children.clone();
    let mut idx = 0;
    let has_tree_line = children.first().is_some_and(|&c| has_class(doc, c, "tree-line"));
    if has_tree_line != tab.is_child {
        rebuild_tab_row_children(doc, row, tab);
        return;
    }
    if has_tree_line {
        idx += 1;
    }
    let has_stripe = children.get(idx).is_some_and(|&c| has_class(doc, c, "container-stripe"));
    if has_stripe != tab.container_color.is_some() {
        rebuild_tab_row_children(doc, row, tab);
        return;
    }
    if let Some(color) = &tab.container_color {
        set_attr(doc, children[idx], "style", &format!("background:{color}"));
        idx += 1;
    }
    let (Some(&fav), Some(&title)) = (children.get(idx), children.get(idx + 1)) else {
        rebuild_tab_row_children(doc, row, tab);
        return;
    };
    set_text(doc, fav, &first_letter(&tab.title));
    set_text(doc, title, &tab.title);
    idx += 2;

    match children.get(idx) {
        Some(&trailing) if tab.sleeping && has_class(doc, trailing, "tab-badge") => {}
        Some(&trailing) if !tab.sleeping && has_class(doc, trailing, "tab-close") => {
            set_attr(doc, trailing, "data-tab-id", &tab.id.to_string());
        }
        _ => rebuild_tab_row_children(doc, row, tab),
    }
}

fn rebuild_tab_row_children(doc: &mut Document, row: NodeId, tab: &ChromeTabModel) {
    let children: Vec<NodeId> = doc.get(row).children.clone();
    for child in children {
        detach_node(doc, child);
    }
    populate_tab_row_children(doc, row, tab);
}

fn rebuild_workspace_list(doc: &mut Document, container: NodeId, workspaces: &[ChromeWorkspaceModel]) {
    // The "+" add-workspace button (`.ws-add`) is not a `.ws-item` and is
    // therefore untouched by reconciliation — new items are inserted before
    // it so it stays last, matching the asset's own order.
    let add_btn = doc
        .get(container)
        .children
        .iter()
        .copied()
        .find(|&c| doc.get(c).get_attr("data-action") == Some("add-workspace"));
    reconcile_row_list(doc, container, "ws-item", workspaces, build_workspace_item, update_workspace_item, add_btn);
}

fn build_workspace_item(doc: &mut Document, ws: &ChromeWorkspaceModel) -> NodeId {
    let item = doc.create_element(QualName::html("button"));
    set_attr(doc, item, "data-action", "select-workspace");
    apply_workspace_item_attrs(doc, item, ws);
    populate_workspace_item_children(doc, item, ws);
    item
}

/// Shared by [`build_workspace_item`] and [`update_workspace_item`] — see
/// [`apply_tab_row_attrs`] for why this is split from child construction.
fn apply_workspace_item_attrs(doc: &mut Document, item: NodeId, ws: &ChromeWorkspaceModel) {
    set_attr(doc, item, "class", if ws.active { "ws-item active" } else { "ws-item" });
    set_attr(doc, item, "data-ws-id", &ws.id.to_string());
    set_attr(doc, item, "style", &format!("--ws-color:{}", ws.color));
}

fn populate_workspace_item_children(doc: &mut Document, item: NodeId, ws: &ChromeWorkspaceModel) {
    let icon = doc.create_element(QualName::html("span"));
    set_attr(doc, icon, "class", "ws-icon");
    set_attr(doc, icon, "style", &format!("background:{}", ws.color));
    append_text(doc, icon, &first_letter(&ws.name));
    attach_child(doc, item, icon);

    let lbl = doc.create_element(QualName::html("span"));
    set_attr(doc, lbl, "class", "lbl");
    append_text(doc, lbl, &ws.name);
    attach_child(doc, item, lbl);
}

/// Updates an existing `.ws-item` in place (BUG-341/CC-14 — see
/// [`update_tab_row`]'s doc comment, same rationale). The asset's item
/// shape (icon + label, always both present) never varies by `ws`, so
/// unlike [`update_tab_row`] there is no shape-mismatch fallback needed —
/// only the defensive "children missing entirely" case falls back to
/// [`populate_workspace_item_children`].
fn update_workspace_item(doc: &mut Document, item: NodeId, ws: &ChromeWorkspaceModel) {
    apply_workspace_item_attrs(doc, item, ws);
    let children: Vec<NodeId> = doc.get(item).children.clone();
    let (Some(&icon), Some(&lbl)) = (children.first(), children.get(1)) else {
        for child in children {
            detach_node(doc, child);
        }
        populate_workspace_item_children(doc, item, ws);
        return;
    };
    set_attr(doc, icon, "style", &format!("background:{}", ws.color));
    set_text(doc, icon, &first_letter(&ws.name));
    set_text(doc, lbl, &ws.name);
}

/// Mirrors [`rebuild_tab_list`] into `#hbarTabs` (`.hbar-tab` rows, CC-8's
/// horizontal-layout tab bar) so switching layouts doesn't leave the
/// horizontal bar showing stale asset demo data. The asset's `.hbar-tab`
/// markup has no tree/container-strip visuals (flat list only), so this
/// intentionally omits [`ChromeTabModel::is_child`]/`container_color`
/// unlike [`build_tab_row`].
fn rebuild_hbar_tab_list(doc: &mut Document, container: NodeId, tabs: &[ChromeTabModel]) {
    reconcile_row_list(doc, container, "hbar-tab", tabs, build_hbar_tab, update_hbar_tab, None);
}

fn build_hbar_tab(doc: &mut Document, tab: &ChromeTabModel) -> NodeId {
    let row = doc.create_element(QualName::html("div"));
    set_attr(doc, row, "data-action", "select-tab");
    // CC-13: see the matching comment in `build_tab_row` — this row also
    // replaces static markup wholesale, so ARIA has to be set here too.
    set_attr(doc, row, "role", "tab");
    apply_hbar_tab_attrs(doc, row, tab);
    populate_hbar_tab_children(doc, row, tab);
    row
}

fn apply_hbar_tab_attrs(doc: &mut Document, row: NodeId, tab: &ChromeTabModel) {
    set_attr(doc, row, "class", if tab.active { "hbar-tab active" } else { "hbar-tab" });
    set_attr(doc, row, "data-tab-id", &tab.id.to_string());
    set_attr(doc, row, "aria-selected", if tab.active { "true" } else { "false" });
}

fn populate_hbar_tab_children(doc: &mut Document, row: NodeId, tab: &ChromeTabModel) {
    let fav = doc.create_element(QualName::html("span"));
    set_attr(doc, fav, "class", "tab-fav");
    append_text(doc, fav, &first_letter(&tab.title));
    attach_child(doc, row, fav);

    let title = doc.create_element(QualName::html("span"));
    set_attr(doc, title, "class", "tab-title");
    append_text(doc, title, &tab.title);
    attach_child(doc, row, title);
}

/// Updates an existing `.hbar-tab` in place (BUG-341/CC-14 — see
/// [`update_tab_row`]). Fixed fav+title shape, no fallback branches needed
/// beyond the defensive missing-children case.
fn update_hbar_tab(doc: &mut Document, row: NodeId, tab: &ChromeTabModel) {
    apply_hbar_tab_attrs(doc, row, tab);
    let children: Vec<NodeId> = doc.get(row).children.clone();
    let (Some(&fav), Some(&title)) = (children.first(), children.get(1)) else {
        for child in children {
            detach_node(doc, child);
        }
        populate_hbar_tab_children(doc, row, tab);
        return;
    };
    set_text(doc, fav, &first_letter(&tab.title));
    set_text(doc, title, &tab.title);
}

/// Mirrors [`rebuild_workspace_list`] into `.hbar-ws` (`.hbar-ws-pill`
/// buttons, CC-8's horizontal-layout workspace switcher).
fn rebuild_hbar_ws_list(doc: &mut Document, container: NodeId, workspaces: &[ChromeWorkspaceModel]) {
    reconcile_row_list(doc, container, "hbar-ws-pill", workspaces, build_hbar_ws_pill, update_hbar_ws_pill, None);
}

fn apply_hbar_ws_pill_attrs(doc: &mut Document, pill: NodeId, ws: &ChromeWorkspaceModel) {
    set_attr(doc, pill, "class", if ws.active { "hbar-ws-pill active" } else { "hbar-ws-pill" });
    set_attr(doc, pill, "data-action", "select-workspace");
    set_attr(doc, pill, "data-ws-id", &ws.id.to_string());
    set_attr(doc, pill, "style", &format!("--ws-color:{}", ws.color));
}

fn build_hbar_ws_pill(doc: &mut Document, ws: &ChromeWorkspaceModel) -> NodeId {
    let pill = doc.create_element(QualName::html("button"));
    apply_hbar_ws_pill_attrs(doc, pill, ws);
    append_text(doc, pill, &ws.name);
    pill
}

/// Updates an existing `.hbar-ws-pill` in place (BUG-341/CC-14 — see
/// [`update_tab_row`]); the pill's name is its own single text-node child,
/// updated via [`set_text`].
fn update_hbar_ws_pill(doc: &mut Document, pill: NodeId, ws: &ChromeWorkspaceModel) {
    apply_hbar_ws_pill_attrs(doc, pill, ws);
    set_text(doc, pill, &ws.name);
}

fn first_letter(s: &str) -> String {
    s.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "\u{2022}".to_string())
}

fn remove_children_with_class(doc: &mut Document, container: NodeId, class: &str) {
    let children: Vec<NodeId> = doc.get(container).children.clone();
    let mut removed_any = false;
    for child in children {
        if has_class(doc, child, class) {
            detach_node(doc, child);
            removed_any = true;
        }
    }
    if removed_any {
        // BUG-341 S6: a structural change (row(s) gone) — the container's
        // remaining/future children can depend on it via `:nth-child` etc.
        record_structural(container);
    }
}

fn has_class(doc: &Document, id: NodeId, class: &str) -> bool {
    doc.get(id).get_attr("class").is_some_and(|c| c.split_whitespace().any(|t| t == class))
}

/// Adds (`present: true`) or removes (`present: false`) a single class token
/// on `id`'s `class` attribute, preserving the rest.
fn set_class_token(doc: &mut Document, id: NodeId, token: &str, present: bool) {
    let current = doc.get(id).get_attr("class").unwrap_or("").to_owned();
    let mut tokens: Vec<&str> = current.split_whitespace().filter(|&t| t != token).collect();
    if present {
        tokens.push(token);
    }
    let joined = tokens.join(" ");
    set_attr(doc, id, "class", &joined);
}

fn append_text(doc: &mut Document, parent: NodeId, text: &str) {
    let node = doc.create_text(text.to_string());
    attach_child(doc, parent, node);
}

fn set_attr(doc: &mut Document, id: NodeId, name: &str, value: &str) {
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data { // S16-tracked-primitive
        if let Some(attr) = attrs.iter_mut().find(|a| a.name.local.eq_ignore_ascii_case(name)) {
            if attr.value != value {
                attr.value = value.to_string();
                // BUG-341 S6: value actually changed — a selector keyed on
                // this attribute (most commonly `class`) could now match
                // differently. S17 carries the name, so the root-set can ask
                // which selectors key on *this* attribute.
                record_attr(id, name);
            }
        } else {
            attrs.push(Attribute { name: QualName::html(name.to_ascii_lowercase()), value: value.to_string() });
            record_attr(id, name);
        }
    }
}

fn remove_attr(doc: &mut Document, id: NodeId, name: &str) {
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data { // S16-tracked-primitive
        let before = attrs.len();
        attrs.retain(|a| !a.name.local.eq_ignore_ascii_case(name));
        if attrs.len() != before {
            record_attr(id, name);
        }
    }
}

/// Finds the first element whose `name` attribute equals `value` — used for
/// containers the asset marks only with `data-testid` (no `id`), e.g. the
/// workspace switcher.
fn find_by_attr(doc: &Document, name: &str, value: &str) -> Option<NodeId> {
    let mut stack: Vec<NodeId> = vec![doc.root()];
    while let Some(id) = stack.pop() {
        let node = doc.get(id);
        if matches!(node.data, NodeData::Element { .. }) && node.get_attr(name) == Some(value) {
            return Some(id);
        }
        for &child in node.children.iter().rev() {
            stack.push(child);
        }
    }
    None
}

/// Finds the first element carrying `class` as one of its class tokens —
/// used for containers the asset marks only by class, e.g. `.hbar-ws`
/// (no `id`/`data-testid` of its own).
fn find_by_class(doc: &Document, class: &str) -> Option<NodeId> {
    let mut stack: Vec<NodeId> = vec![doc.root()];
    while let Some(id) = stack.pop() {
        let node = doc.get(id);
        if matches!(node.data, NodeData::Element { .. }) && has_class(doc, id, class) {
            return Some(id);
        }
        for &child in node.children.iter().rev() {
            stack.push(child);
        }
    }
    None
}

/// Makes `id`'s content exactly the single text node `text`.
///
/// Updates the existing text node's payload in place when `id` already has
/// exactly one text-node child, which is the overwhelmingly common case for
/// chrome-built cells — that preserves the text node's `NodeId` so
/// [`lumen_layout::layout_mutation_incremental`]'s `graft_geometry` (which
/// matches subtrees by node id) can reuse its box, and makes an unchanged
/// rebind a genuine no-op. Otherwise falls back to detach-all + append one
/// fresh text node.
///
/// BUG-341 S16: the in-place path used to live in a separate
/// `set_text_in_place`, and the seven [`bind_cert`]/[`bind_bookmarks`]/… cells
/// that called plain `set_text` detached and recreated their text node on
/// *every* bind, changed or not. That was invisible while nothing tracked
/// content; with S16 it reported twelve content-dirty nodes on a rebind of a
/// bit-identical model, which would have cancelled S15's whole-document box
/// reuse on every hover frame. One function, one behaviour.
fn set_text(doc: &mut Document, id: NodeId, text: &str) {
    let children: Vec<NodeId> = doc.get(id).children.clone();
    if let [only] = children.as_slice()
        && let NodeData::Text(s) = &mut doc.get_mut(*only).data // S16-tracked-primitive
    {
        if *s != text {
            *s = text.to_string();
            // BUG-341 S16: the text node keeps its `NodeId` and carries no
            // style, so neither the cascade nor `graft_geometry`'s id matching
            // can notice this. Reporting the text node itself is what drops
            // `id` (whose box embeds this text) out of `clean_subtrees`.
            record_content(*only);
        }
        return;
    }
    for child in children {
        detach_node(doc, child);
    }
    append_text(doc, id, text);
}

/// Depth-first search scoped to `root`'s subtree (inclusive of `root`
/// itself is never matched — mirrors [`crate::main`]'s `take_content_area`
/// convention of never matching the search root) for the first element
/// carrying `class`. Used by [`bind_cert`] to find `.cert-row`/`.cert-fp`
/// only within `#certOverlay`, not anywhere else in the document (CC-10).
fn find_descendant_by_class(doc: &Document, root: NodeId, class: &str) -> Option<NodeId> {
    find_descendants_by_class(doc, root, class).into_iter().next()
}

/// Like [`find_descendant_by_class`] but collects every match, in document
/// order — used by [`bind_cert`] to enumerate all 6 `.cert-row`s (CC-10).
fn find_descendants_by_class(doc: &Document, root: NodeId, class: &str) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack: Vec<NodeId> = doc.get(root).children.iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        let node = doc.get(id);
        if matches!(node.data, NodeData::Element { .. }) && has_class(doc, id, class) {
            out.push(id);
        }
        for &child in node.children.iter().rev() {
            stack.push(child);
        }
    }
    out
}

/// Depth-first search scoped to `root`'s subtree for the first element with
/// tag name `tag` (case-insensitive) — used by [`bind_cert`] to find the
/// modal's `<h3>` title (CC-10).
fn find_descendant_by_tag(doc: &Document, root: NodeId, tag: &str) -> Option<NodeId> {
    let mut stack: Vec<NodeId> = doc.get(root).children.iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        let node = doc.get(id);
        if let NodeData::Element { name, .. } = &node.data
            && name.local.eq_ignore_ascii_case(tag)
        {
            return Some(id);
        }
        for &child in node.children.iter().rev() {
            stack.push(child);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_asset() -> Document {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
        let html = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        crate::parse_document(&html).0
    }

    fn model_with_tabs(tabs: Vec<ChromeTabModel>) -> ChromeModel {
        ChromeModel { tabs, ..ChromeModel::default() }
    }

    /// BUG-341 S16 completeness gate — the structural half of the slice.
    ///
    /// `ChromeMutations::content` is only sound if **every** write this module
    /// makes into the document is bookkept. That cannot be established by
    /// reading the file once: a future `bind_*` helper that calls
    /// `doc.append_child` directly would silently produce stale boxes, and no
    /// behavioural test covers a binding nobody has written yet. So the
    /// invariant is enforced against this file's own source: the four raw
    /// `Document` mutators may appear only on lines carrying the
    /// `S16-tracked-primitive` marker, i.e. inside the wrappers that do the
    /// bookkeeping.
    ///
    /// If this fails on code you just added: don't add the marker — route the
    /// call through `attach_child` / `insert_child_before` / `detach_node`, or
    /// (for a new kind of in-place write) add a wrapper next to them that calls
    /// `record_content`.
    #[test]
    fn every_dom_mutation_in_model_rs_goes_through_a_tracked_primitive() {
        const SRC: &str = include_str!("model.rs");
        // Assembled at runtime rather than written out: a literal
        // `"doc.append_child("` in this file would match itself.
        let raw: Vec<String> = ["append_child", "insert_before", "detach", "get_mut"]
            .iter()
            .map(|m| format!("doc.{m}("))
            .collect();

        let mut offenders = Vec::new();
        for (i, line) in SRC.lines().enumerate() {
            if line.trim_start().starts_with("//") || line.contains("S16-tracked-primitive") {
                continue;
            }
            if raw.iter().any(|needle| line.contains(needle.as_str())) {
                offenders.push(format!("model.rs:{}: {}", i + 1, line.trim()));
            }
        }
        assert!(
            offenders.is_empty(),
            "these lines mutate the document without content bookkeeping, so\n\
             `ChromeMutations::content` would under-report and\n\
             `ContentDirty::Nodes` would hand `build_box` a stale subtree:\n{}",
            offenders.join("\n"),
        );
    }

    /// BUG-341 S16: a text-only model change (a renamed tab) must be reported
    /// as content, and only as content.
    ///
    /// Both halves matter. Reporting it as content is what stops the reused box
    /// from keeping the old string — the corruption this slice risks. *Not*
    /// reporting it as selector-relevant is what keeps S6's cascade root-set
    /// empty, so a rename costs a box rebuild on one chain instead of a
    /// document re-cascade.
    ///
    /// Contrast with the omnibox, whose value lives in a `value` **attribute**:
    /// that is a selector-relevant write (`[value]`, `:placeholder-shown`, …)
    /// and correctly lands in both sets.
    #[test]
    fn bind_model_tracked_reports_a_renamed_tab_as_content_only() {
        let mut doc = parse_asset();
        let tab = |title: &str| {
            model_with_tabs(vec![ChromeTabModel {
                id: 1, title: title.to_owned(), active: true, sleeping: false,
                is_child: false, container_color: None,
            }])
        };
        bind_model_tracked(&mut doc, &tab("Alpha"));
        let touched = bind_model_tracked(&mut doc, &tab("Alphabet"));
        assert!(
            touched.selector.is_empty(),
            "a text-only change matches no selector differently — reporting it as \
             selector-relevant would re-cascade the subtree for nothing: {:?}",
            touched.selector,
        );
        assert!(
            !touched.content.is_empty(),
            "a text-only change MUST be reported as content — otherwise `ContentDirty::Nodes` \
             lets `build_box` reuse the tab row's box with the previous title in it",
        );
    }

    /// BUG-341 S16: rebinding a bit-identical model must report **nothing**,
    /// content included.
    ///
    /// This is the load-bearing precondition for S15's whole-document box reuse
    /// on a pure hover frame: `relayout_chrome_host` calls `bind_model_tracked`
    /// before *every* chrome layout, so any binding that rewrites the DOM
    /// unconditionally shows up here as a content-dirty node and cancels the
    /// reuse of its whole ancestor chain. Twelve cells did exactly that before
    /// S16 (`set_text` detached and recreated its text node every call), which
    /// only became visible once content was tracked at all — the sibling
    /// `..._reports_nothing_touched_...` test above passes either way, because
    /// none of those rewrites is selector-relevant.
    #[test]
    fn bind_model_tracked_reports_no_content_for_an_unchanged_model() {
        let mut doc = parse_asset();
        let model = ChromeModel::default();
        bind_model_tracked(&mut doc, &model);
        let second = bind_model_tracked(&mut doc, &model);
        assert!(
            second.content.is_empty(),
            "rebinding an identical model rewrote {} node(s) anyway: {:?}. Every binding must \
             compare before it writes — an unconditional rewrite here costs box reuse on every \
             single chrome frame, hover included.",
            second.content.len(),
            second.content,
        );
    }

    /// BUG-341 S6: two `bind_model_tracked` calls with a bit-identical model
    /// must report nothing touched — this is the precondition
    /// `relayout_chrome_host`'s incremental path relies on for a pure
    /// interactive-state (hover/focus/active) cycle to still take the
    /// `ContentDirty::Nothing` fast path exactly like before S6.
    #[test]
    fn bind_model_tracked_reports_nothing_touched_for_an_unchanged_model() {
        let mut doc = parse_asset();
        let model = model_with_tabs(vec![ChromeTabModel {
            id: 1, title: "Alpha".to_owned(), active: true, sleeping: false,
            is_child: false, container_color: None,
        }]);
        let first = bind_model_tracked(&mut doc, &model);
        assert!(!first.is_empty(), "the very first bind populates the tab list — must report touched nodes");
        let second = bind_model_tracked(&mut doc, &model);
        assert!(
            second.is_empty(),
            "rebinding the identical model must touch nothing: {second:?}",
        );
    }

    /// A theme flip changes `body[data-theme]` — `bind_model_tracked` must
    /// report exactly `<body>`, not the whole document.
    #[test]
    fn bind_model_tracked_reports_the_body_on_a_theme_change() {
        let mut doc = parse_asset();
        bind_model_tracked(&mut doc, &ChromeModel { dark_theme: false, ..ChromeModel::default() });
        let touched = bind_model_tracked(&mut doc, &ChromeModel { dark_theme: true, ..ChromeModel::default() });
        let body = doc.body().expect("asset has <body>");
        assert_eq!(
            touched.selector.keys().copied().collect::<HashSet<_>>(),
            HashSet::from([body]),
            "only <body> should be reported touched",
        );
        // BUG-341 S17: the report names the attribute, not just the node.
        assert_eq!(
            touched.selector[&body].attrs.iter().map(String::as_str).collect::<Vec<_>>(),
            ["data-theme"],
            "a theme flip writes exactly `data-theme`",
        );
        assert!(!touched.selector[&body].structural, "a theme flip moves no children");
    }

    /// Adding a tab is a structural change on `#sbTabs`/`#hbarTabs` (the
    /// `reconcile_row_list`-driven containers) — `bind_model_tracked` must
    /// report the containers, not silently miss the row-count change.
    #[test]
    fn bind_model_tracked_reports_the_container_when_a_tab_is_added() {
        let mut doc = parse_asset();
        let one_tab = model_with_tabs(vec![ChromeTabModel {
            id: 1, title: "Alpha".to_owned(), active: true, sleeping: false,
            is_child: false, container_color: None,
        }]);
        bind_model_tracked(&mut doc, &one_tab);
        let two_tabs = model_with_tabs(vec![
            ChromeTabModel {
                id: 1, title: "Alpha".to_owned(), active: true, sleeping: false,
                is_child: false, container_color: None,
            },
            ChromeTabModel {
                id: 2, title: "Beta".to_owned(), active: false, sleeping: false,
                is_child: false, container_color: None,
            },
        ]);
        let touched = bind_model_tracked(&mut doc, &two_tabs);
        let sb_tabs = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
        let hbar_tabs = doc.find_by_id(crate::ids::HBAR_TABS).expect("asset has #hbarTabs");
        assert!(
            touched.selector.get(&sb_tabs).is_some_and(|t| t.structural),
            "touched must report #sbTabs as structural: {touched:?}",
        );
        assert!(
            touched.selector.get(&hbar_tabs).is_some_and(|t| t.structural),
            "touched must report #hbarTabs as structural: {touched:?}",
        );
    }

    #[test]
    fn binds_theme_layout_and_profile_onto_body() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            dark_theme: true,
            layout_vertical: false,
            profile_slug: Some("work".to_owned()),
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let body = doc.body().expect("asset has <body>");
        assert_eq!(doc.get(body).get_attr("data-theme"), Some("dark"));
        assert_eq!(doc.get(body).get_attr("data-layout"), Some("horizontal"));
        assert_eq!(doc.get(body).get_attr("data-profile"), Some("work"));
    }

    #[test]
    fn no_profile_slug_removes_the_attribute() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &ChromeModel { profile_slug: None, ..ChromeModel::default() });
        let body = doc.body().expect("asset has <body>");
        assert_eq!(doc.get(body).get_attr("data-profile"), None);
    }

    #[test]
    fn tab_list_is_rebuilt_from_the_model() {
        let mut doc = parse_asset();
        let model = model_with_tabs(vec![
            ChromeTabModel {
                id: 7, title: "Alpha".to_owned(), active: true, sleeping: false,
                is_child: false, container_color: None,
            },
            ChromeTabModel {
                id: 9, title: "Beta".to_owned(), active: false, sleeping: true,
                is_child: false, container_color: None,
            },
        ]);
        bind_model(&mut doc, &model);
        let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
        let rows: Vec<NodeId> = doc
            .get(container)
            .children
            .iter()
            .copied()
            .filter(|&c| has_class(&doc, c, "tab-row"))
            .collect();
        assert_eq!(rows.len(), 2, "old demo rows must be gone, only the 2 model tabs remain");
        assert!(has_class(&doc, rows[0], "active"));
        assert_eq!(doc.get(rows[0]).get_attr("data-tab-id"), Some("7"));
        assert!(has_class(&doc, rows[1], "sleeping"));
        assert_eq!(doc.get(rows[1]).get_attr("data-tab-id"), Some("9"));
    }

    #[test]
    fn empty_tab_list_clears_all_rows() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &model_with_tabs(Vec::new()));
        let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
        let rows = doc.get(container).children.iter().filter(|&&c| has_class(&doc, c, "tab-row")).count();
        assert_eq!(rows, 0);
    }

    /// BUG-341/CC-14: rebinding an unchanged tab list must not tear down and
    /// recreate the rows — `layout_mutation_incremental`'s `graft_geometry`
    /// matches subtrees by `NodeId`, so a fresh id on every relayout (even
    /// when nothing changed) defeated it. Checks identity is preserved not
    /// just for the row itself but every descendant graft matches into
    /// (fav/title/close), since `graft_geometry` recurses.
    #[test]
    fn rebinding_unchanged_tabs_preserves_row_and_descendant_node_ids() {
        let mut doc = parse_asset();
        let model = model_with_tabs(vec![
            ChromeTabModel { id: 7, title: "Alpha".to_owned(), active: true, sleeping: false, is_child: false, container_color: None },
            ChromeTabModel { id: 9, title: "Beta".to_owned(), active: false, sleeping: true, is_child: false, container_color: None },
        ]);
        bind_model(&mut doc, &model);
        let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
        let rows_before: Vec<NodeId> =
            doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "tab-row")).collect();
        let descendants_before: Vec<Vec<NodeId>> =
            rows_before.iter().map(|&r| doc.get(r).children.clone()).collect();

        bind_model(&mut doc, &model);
        let rows_after: Vec<NodeId> =
            doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "tab-row")).collect();
        let descendants_after: Vec<Vec<NodeId>> =
            rows_after.iter().map(|&r| doc.get(r).children.clone()).collect();

        assert_eq!(rows_before, rows_after, "unchanged rows must keep their NodeId across bind_model calls");
        assert_eq!(
            descendants_before, descendants_after,
            "unchanged rows' children (fav/title/close) must keep their NodeId too"
        );
    }

    /// A title change is the common case (only content differs, shape
    /// doesn't) — must hit `update_tab_row`'s fast path: same row id, text
    /// updated in place.
    #[test]
    fn rebinding_a_changed_title_keeps_the_row_id_and_updates_text_in_place() {
        let mut doc = parse_asset();
        let mut model = model_with_tabs(vec![
            ChromeTabModel { id: 7, title: "Alpha".to_owned(), active: true, sleeping: false, is_child: false, container_color: None },
        ]);
        bind_model(&mut doc, &model);
        let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
        let row_before = doc.get(container).children.iter().copied().find(|&c| has_class(&doc, c, "tab-row")).unwrap();

        model.tabs[0].title = "Alpha Renamed".to_owned();
        bind_model(&mut doc, &model);
        let row_after = doc.get(container).children.iter().copied().find(|&c| has_class(&doc, c, "tab-row")).unwrap();
        assert_eq!(row_before, row_after, "a title-only change must not recreate the row");

        let title_node = doc
            .get(row_after)
            .children
            .iter()
            .copied()
            .find(|&c| has_class(&doc, c, "tab-title"))
            .expect("row has a .tab-title span");
        let text: Vec<NodeId> = doc.get(title_node).children.clone();
        let NodeData::Text(s) = &doc.get(text[0]).data else { panic!(".tab-title has a text child") };
        assert_eq!(s, "Alpha Renamed");
    }

    /// Removing a tab must detach only the trailing surplus row and keep the
    /// surviving row's id (position-based reconciliation, matching
    /// `graft_geometry`'s own by-index matching).
    #[test]
    fn shrinking_the_tab_list_detaches_the_trailing_row_and_keeps_the_survivor_id() {
        let mut doc = parse_asset();
        let model = model_with_tabs(vec![
            ChromeTabModel { id: 1, title: "One".to_owned(), active: true, sleeping: false, is_child: false, container_color: None },
            ChromeTabModel { id: 2, title: "Two".to_owned(), active: false, sleeping: false, is_child: false, container_color: None },
        ]);
        bind_model(&mut doc, &model);
        let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
        let rows_before: Vec<NodeId> =
            doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "tab-row")).collect();

        bind_model(&mut doc, &model_with_tabs(vec![model.tabs[0].clone()]));
        let rows_after: Vec<NodeId> =
            doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "tab-row")).collect();
        assert_eq!(rows_after, vec![rows_before[0]], "surviving row must keep its id; the extra row is detached");
    }

    /// A `sleeping` flip changes the row's trailing slot shape (badge vs
    /// close button) — must fall back to rebuilding the row's children, but
    /// the row itself keeps its id and the final DOM state is correct.
    #[test]
    fn toggling_sleeping_keeps_the_row_id_and_swaps_badge_for_close_button() {
        let mut doc = parse_asset();
        let mut model = model_with_tabs(vec![
            ChromeTabModel { id: 1, title: "One".to_owned(), active: false, sleeping: false, is_child: false, container_color: None },
        ]);
        bind_model(&mut doc, &model);
        let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
        let row_before = doc.get(container).children.iter().copied().find(|&c| has_class(&doc, c, "tab-row")).unwrap();

        model.tabs[0].sleeping = true;
        bind_model(&mut doc, &model);
        let row_after = doc.get(container).children.iter().copied().find(|&c| has_class(&doc, c, "tab-row")).unwrap();
        assert_eq!(row_before, row_after, "a shape change must still keep the row's own id");
        assert!(has_class(&doc, row_after, "sleeping"));
        let children = doc.get(row_after).children.clone();
        assert!(children.iter().any(|&c| has_class(&doc, c, "tab-badge")));
        assert!(!children.iter().any(|&c| has_class(&doc, c, "tab-close")));
    }

    /// Same identity-preservation guarantee for the workspace switcher.
    #[test]
    fn rebinding_unchanged_workspaces_preserves_item_and_descendant_node_ids() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            workspaces: vec![
                ChromeWorkspaceModel { id: 1, name: "Личное".to_owned(), active: true, color: "#0066FF".to_owned() },
                ChromeWorkspaceModel { id: 2, name: "Проект Х".to_owned(), active: false, color: "#8B5CF6".to_owned() },
            ],
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let container = find_by_attr(&doc, "data-testid", "workspace-switcher").expect("asset has the switcher");
        let items_before: Vec<NodeId> =
            doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "ws-item")).collect();

        bind_model(&mut doc, &model);
        let items_after: Vec<NodeId> =
            doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "ws-item")).collect();
        assert_eq!(items_before, items_after, "unchanged workspace items must keep their NodeId");
    }

    #[test]
    fn workspace_switcher_is_rebuilt_and_add_button_stays_last() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            workspaces: vec![
                ChromeWorkspaceModel { id: 1, name: "Личное".to_owned(), active: true, color: "#0066FF".to_owned() },
                ChromeWorkspaceModel { id: 2, name: "Проект Х".to_owned(), active: false, color: "#8B5CF6".to_owned() },
            ],
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let container = find_by_attr(&doc, "data-testid", "workspace-switcher").expect("asset has the switcher");
        let children = doc.get(container).children.clone();
        let items: Vec<NodeId> = children.iter().copied().filter(|&c| has_class(&doc, c, "ws-item")).collect();
        assert_eq!(items.len(), 2);
        assert_eq!(doc.get(items[0]).get_attr("data-ws-id"), Some("1"));
        assert!(has_class(&doc, items[0], "active"));
        assert_eq!(doc.get(items[0]).get_attr("style"), Some("--ws-color:#0066FF"));
        assert_eq!(doc.get(items[1]).get_attr("data-ws-id"), Some("2"));
        assert!(!has_class(&doc, items[1], "active"));
        let add_btn_pos = children
            .iter()
            .position(|&c| doc.get(c).get_attr("data-action") == Some("add-workspace"))
            .expect("the '+' button must still be present");
        let last_item_pos = children.iter().position(|&c| c == items[1]).unwrap();
        assert!(add_btn_pos > last_item_pos, "the '+' button must stay after every rebuilt workspace item");
    }

    #[test]
    fn hbar_tab_list_mirrors_the_sidebar_tab_list() {
        let mut doc = parse_asset();
        let model = model_with_tabs(vec![
            ChromeTabModel {
                id: 3, title: "Gamma".to_owned(), active: true, sleeping: false,
                is_child: false, container_color: None,
            },
        ]);
        bind_model(&mut doc, &model);
        let container = doc.find_by_id(crate::ids::HBAR_TABS).expect("asset has #hbarTabs");
        let rows: Vec<NodeId> = doc
            .get(container)
            .children
            .iter()
            .copied()
            .filter(|&c| has_class(&doc, c, "hbar-tab"))
            .collect();
        assert_eq!(rows.len(), 1, "old demo hbar rows must be gone, only the 1 model tab remains");
        assert!(has_class(&doc, rows[0], "active"));
        assert_eq!(doc.get(rows[0]).get_attr("data-tab-id"), Some("3"));
    }

    #[test]
    fn hbar_workspace_pills_mirror_the_sidebar_switcher() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            workspaces: vec![
                ChromeWorkspaceModel { id: 5, name: "Чтение".to_owned(), active: true, color: "#1F9D55".to_owned() },
            ],
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let container = find_by_class(&doc, "hbar-ws").expect("asset has .hbar-ws");
        let pills: Vec<NodeId> = doc
            .get(container)
            .children
            .iter()
            .copied()
            .filter(|&c| has_class(&doc, c, "hbar-ws-pill"))
            .collect();
        assert_eq!(pills.len(), 1, "old demo pills must be gone, only the 1 model workspace remains");
        assert!(has_class(&doc, pills[0], "active"));
        assert_eq!(doc.get(pills[0]).get_attr("data-ws-id"), Some("5"));
        assert_eq!(doc.get(pills[0]).get_attr("style"), Some("--ws-color:#1F9D55"));
    }

    #[test]
    fn sidebar_collapsed_flag_toggles_the_collapsed_class() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &ChromeModel { sidebar_collapsed: true, ..ChromeModel::default() });
        let sidebar = doc.find_by_id(crate::ids::SIDEBAR).expect("asset has #sidebar");
        assert!(has_class(&doc, sidebar, "collapsed"));

        bind_model(&mut doc, &ChromeModel { sidebar_collapsed: false, ..ChromeModel::default() });
        assert!(!has_class(&doc, sidebar, "collapsed"), "a later bind_model with the flag off must remove the class");
    }

    #[test]
    fn child_tab_gets_the_child_class_tree_line_and_container_stripe() {
        let mut doc = parse_asset();
        let model = model_with_tabs(vec![ChromeTabModel {
            id: 11, title: "Reply".to_owned(), active: false, sleeping: false,
            is_child: true, container_color: Some("#1F9D55".to_owned()),
        }]);
        bind_model(&mut doc, &model);
        let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
        let row = doc
            .get(container)
            .children
            .iter()
            .copied()
            .find(|&c| has_class(&doc, c, "tab-row"))
            .expect("one tab row bound");
        assert!(has_class(&doc, row, "child"));
        let has_tree_line = doc.get(row).children.iter().any(|&c| has_class(&doc, c, "tree-line"));
        assert!(has_tree_line, "child row must carry a .tree-line connector span");
        let stripe = doc
            .get(row)
            .children
            .iter()
            .copied()
            .find(|&c| has_class(&doc, c, "container-stripe"))
            .expect("container_color must render a .container-stripe span");
        assert_eq!(doc.get(stripe).get_attr("style"), Some("background:#1F9D55"));
    }

    /// CC-13: `rebuild_tab_list`/`rebuild_hbar_tab_list` replace the asset's
    /// static `.tab-row`/`.hbar-tab` markup wholesale, so the `role="tab"`/
    /// `aria-selected` `scripts/gen_chrome_assets.py` bakes into the static
    /// asset never reaches a real bound row — this must be set in Rust
    /// instead, mirroring `data-action`.
    #[test]
    fn bound_tab_rows_carry_role_tab_and_aria_selected_in_both_layouts() {
        let mut doc = parse_asset();
        let model = model_with_tabs(vec![
            ChromeTabModel { id: 1, title: "Активная".to_owned(), active: true, sleeping: false, is_child: false, container_color: None },
            ChromeTabModel { id: 2, title: "Пример".to_owned(), active: false, sleeping: false, is_child: false, container_color: None },
        ]);
        bind_model(&mut doc, &model);

        for container_id in [crate::ids::SB_TABS, crate::ids::HBAR_TABS] {
            let container = doc.find_by_id(container_id).expect("asset has the tab-list container");
            let rows: Vec<NodeId> = doc
                .get(container)
                .children
                .iter()
                .copied()
                .filter(|&c| has_class(&doc, c, "tab-row") || has_class(&doc, c, "hbar-tab"))
                .collect();
            assert_eq!(rows.len(), 2, "expected both bound tabs as rows under {container_id:?}");
            assert_eq!(doc.get(rows[0]).get_attr("role"), Some("tab"));
            assert_eq!(doc.get(rows[0]).get_attr("aria-selected"), Some("true"));
            assert_eq!(doc.get(rows[1]).get_attr("role"), Some("tab"));
            assert_eq!(doc.get(rows[1]).get_attr("aria-selected"), Some("false"));
        }

        let sb_container = doc.find_by_id(crate::ids::SB_TABS).unwrap();
        let first_row = doc
            .get(sb_container)
            .children
            .iter()
            .copied()
            .find(|&c| has_class(&doc, c, "tab-row"))
            .unwrap();
        let close = doc
            .get(first_row)
            .children
            .iter()
            .copied()
            .find(|&c| has_class(&doc, c, "tab-close"))
            .expect("active tab row has a close button");
        assert_eq!(doc.get(close).get_attr("aria-label"), Some("Закрыть вкладку"));
    }

    #[test]
    fn omnibox_value_is_written_to_the_input_element() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            omnibox: OmniboxModel { value: "https://example.com".to_owned(), warning: None },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let input = doc.find_by_id(crate::ids::OMNI_INPUT).expect("asset has #omniInput");
        assert_eq!(doc.get(input).get_attr("value"), Some("https://example.com"));
    }

    #[test]
    fn omnibox_warning_shows_the_warn_banner_with_its_message() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            omnibox: OmniboxModel { value: String::new(), warning: Some("spoof risk".to_owned()) },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let warn = doc.find_by_id(crate::ids::OMNI_WARN).expect("asset has #omniWarn");
        assert!(has_class(&doc, warn, "show"));
        let span = doc
            .get(warn)
            .children
            .iter()
            .copied()
            .find(|&c| matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local.eq_ignore_ascii_case("span")))
            .expect("#omniWarn rebuilds a <span> with the warning message");
        let text: String = doc
            .get(span)
            .children
            .iter()
            .filter_map(|&c| match &doc.get(c).data {
                NodeData::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "spoof risk");
    }

    #[test]
    fn no_warning_hides_the_warn_banner() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &ChromeModel::default());
        let warn = doc.find_by_id(crate::ids::OMNI_WARN).expect("asset has #omniWarn");
        assert!(!has_class(&doc, warn, "show"));
    }

    #[test]
    fn dropdown_is_rebuilt_from_suggestions_and_toggles_open() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            dropdown: ChromeDropdownModel {
                open: true,
                suggestions: vec![ChromeSuggestionModel {
                    idx: 0,
                    label: "figma.com".to_owned(),
                    sub_label: "Посещено вчера".to_owned(),
                    color: "#0B6FE0".to_owned(),
                }],
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let container = doc.find_by_id(crate::ids::OMNI_DROPDOWN).expect("asset has #omniDropdown");
        assert!(has_class(&doc, container, "open"));
        let rows: Vec<NodeId> =
            doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "dd-row")).collect();
        assert_eq!(rows.len(), 1, "old demo rows must be gone, only the 1 model suggestion remains");
        assert_eq!(doc.get(rows[0]).get_attr("data-sugg-idx"), Some("0"));
    }

    #[test]
    fn dropdown_closed_hides_the_open_class() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &ChromeModel::default());
        let container = doc.find_by_id(crate::ids::OMNI_DROPDOWN).expect("asset has #omniDropdown");
        assert!(!has_class(&doc, container, "open"));
    }

    #[test]
    fn find_bar_binds_value_count_and_open_state() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            find: ChromeFindModel { open: true, value: "needle".to_owned(), count_label: "2/5".to_owned() },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let bar = doc.find_by_id(crate::ids::FIND_BAR).expect("asset has #findBar");
        assert!(has_class(&doc, bar, "open"));
        let input = doc.find_by_id(crate::ids::FIND_INPUT).expect("asset has #findInput");
        assert_eq!(doc.get(input).get_attr("value"), Some("needle"));
        let count = doc.find_by_id(crate::ids::FIND_COUNT).expect("asset has #findCount");
        let text: String = doc
            .get(count)
            .children
            .iter()
            .filter_map(|&c| match &doc.get(c).data {
                NodeData::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "2/5");
    }

    #[test]
    fn downloads_panel_is_rebuilt_from_entries_and_toggles_open() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            downloads_open: true,
            downloads: vec![ChromeDownloadModel {
                id: 1,
                ext_label: "PDF".to_owned(),
                name: "report.pdf".to_owned(),
                meta: "1.0 MB — идёт загрузка…".to_owned(),
                progress_fraction: Some(0.5),
            }],
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let panel = doc.find_by_id(crate::ids::DOWNLOADS_PANEL).expect("asset has #downloadsPanel");
        assert!(has_class(&doc, panel, "open"));
        let list = find_by_class(&doc, "dl-list").expect("asset has .dl-list");
        let cards: Vec<NodeId> =
            doc.get(list).children.iter().copied().filter(|&c| has_class(&doc, c, "dl-card")).collect();
        assert_eq!(cards.len(), 1, "old demo cards must be gone, only the 1 model entry remains");
        assert_eq!(doc.get(cards[0]).get_attr("data-dl-id"), Some("1"));
        let fill = doc
            .get(cards[0])
            .children
            .iter()
            .find(|&&c| has_class(&doc, c, "dl-progress-track"))
            .and_then(|&track| doc.get(track).children.iter().copied().find(|&c| has_class(&doc, c, "dl-progress-fill")))
            .expect("in-flight entry must render a progress fill");
        assert_eq!(doc.get(fill).get_attr("style"), Some("width:50%"));
    }

    #[test]
    fn empty_downloads_clears_all_cards() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &ChromeModel::default());
        let list = find_by_class(&doc, "dl-list").expect("asset has .dl-list");
        let cards = doc.get(list).children.iter().filter(|&&c| has_class(&doc, c, "dl-card")).count();
        assert_eq!(cards, 0);
    }

    #[test]
    fn popover_binds_blocked_total_and_permission_rows() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            popover_open: true,
            blocked_total: 42,
            permissions: [ChromePermState::Allow, ChromePermState::Deny],
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let popover = doc.find_by_id(crate::ids::PERM_POPOVER).expect("asset has #permPopover");
        assert!(has_class(&doc, popover, "open"));
        let stat = doc.find_by_id(crate::ids::STAT_TRACKERS).expect("asset has #statTrackers");
        let text: String = doc
            .get(stat)
            .children
            .iter()
            .filter_map(|&c| match &doc.get(c).data {
                NodeData::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "42");

        let rows: Vec<NodeId> =
            doc.get(popover).children.iter().copied().filter(|&c| has_class(&doc, c, "perm-row")).collect();
        assert_eq!(rows.len(), 2);
        let camera_actions = doc.get(rows[0]).children.iter().copied().find(|&c| has_class(&doc, c, "perm-actions")).unwrap();
        let camera_allow = doc.get(camera_actions).children.iter().copied().find(|&c| has_class(&doc, c, "allow")).unwrap();
        assert!(has_class(&doc, camera_allow, "selected-allow"));

        let mic_actions = doc.get(rows[1]).children.iter().copied().find(|&c| has_class(&doc, c, "perm-actions")).unwrap();
        let mic_deny = doc.get(mic_actions).children.iter().copied().find(|&c| has_class(&doc, c, "deny")).unwrap();
        assert!(has_class(&doc, mic_deny, "selected-deny"));
    }

    #[test]
    fn ask_state_selects_neither_button() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &ChromeModel::default());
        let popover = doc.find_by_id(crate::ids::PERM_POPOVER).expect("asset has #permPopover");
        let rows: Vec<NodeId> =
            doc.get(popover).children.iter().copied().filter(|&c| has_class(&doc, c, "perm-row")).collect();
        let actions = doc.get(rows[0]).children.iter().copied().find(|&c| has_class(&doc, c, "perm-actions")).unwrap();
        for &btn in &doc.get(actions).children {
            assert!(!has_class(&doc, btn, "selected-allow"));
            assert!(!has_class(&doc, btn, "selected-deny"));
        }
    }

    #[test]
    fn palette_binds_query_and_rebuilds_results_and_toggles_open() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            palette: ChromePaletteModel {
                open: true,
                query: "new t".to_owned(),
                results: vec![
                    ChromePaletteResultModel {
                        label: "New Tab".to_owned(),
                        sub_label: "Ctrl+T".to_owned(),
                        selected: true,
                    },
                    ChromePaletteResultModel {
                        label: "New Window".to_owned(),
                        sub_label: "".to_owned(),
                        selected: false,
                    },
                ],
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let overlay = doc.find_by_id(crate::ids::CP_OVERLAY).expect("asset has #cpOverlay");
        assert!(has_class(&doc, overlay, "open"));
        let input = doc.find_by_id(crate::ids::CP_INPUT).expect("asset has #cpInput");
        assert_eq!(doc.get(input).get_attr("value"), Some("new t"));
        let list = doc.find_by_id(crate::ids::CP_LIST).expect("asset has #cpList");
        let rows: Vec<NodeId> =
            doc.get(list).children.iter().copied().filter(|&c| has_class(&doc, c, "cp-row")).collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(doc.get(rows[0]).get_attr("style"), Some("background:var(--surface-2)"));
        assert_eq!(doc.get(rows[1]).get_attr("style"), None);
    }

    #[test]
    fn palette_closed_hides_open_class_and_empty_results_show_empty_state() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &ChromeModel::default());
        let overlay = doc.find_by_id(crate::ids::CP_OVERLAY).expect("asset has #cpOverlay");
        assert!(!has_class(&doc, overlay, "open"));
        let list = doc.find_by_id(crate::ids::CP_LIST).expect("asset has #cpList");
        let empty = doc.get(list).children.iter().copied().find(|&c| has_class(&doc, c, "cp-empty"));
        assert!(empty.is_some(), "no results must render the .cp-empty state");
    }

    #[test]
    fn cert_binds_title_rows_and_fingerprint_and_toggles_open() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            cert: ChromeCertModel {
                open: true,
                title: "Сертификат — example.com".to_owned(),
                rows: [
                    "example.com".to_owned(),
                    "Example Inc.".to_owned(),
                    "example.com, *.example.com".to_owned(),
                    "Example CA".to_owned(),
                    "2026-01-01".to_owned(),
                    "2027-01-01".to_owned(),
                ],
                fingerprint: "AA:BB:CC".to_owned(),
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let overlay = doc.find_by_id(crate::ids::CERT_OVERLAY).expect("asset has #certOverlay");
        assert!(has_class(&doc, overlay, "open"));
        let h3 = find_descendant_by_tag(&doc, overlay, "h3").expect("#certOverlay has an <h3> title");
        let title_text: String = doc
            .get(h3)
            .children
            .iter()
            .filter_map(|&c| match &doc.get(c).data {
                NodeData::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(title_text, "Сертификат — example.com");

        let rows = find_descendants_by_class(&doc, overlay, "cert-row");
        assert_eq!(rows.len(), 6, "asset must have exactly 6 static .cert-row elements");
        let first_v = doc.get(rows[0]).children.iter().copied().find(|&c| has_class(&doc, c, "v")).unwrap();
        let first_v_text: String = doc
            .get(first_v)
            .children
            .iter()
            .filter_map(|&c| match &doc.get(c).data {
                NodeData::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(first_v_text, "example.com");

        let fp = find_descendant_by_class(&doc, overlay, "cert-fp").expect("#certOverlay has .cert-fp");
        let fp_text: String = doc
            .get(fp)
            .children
            .iter()
            .filter_map(|&c| match &doc.get(c).data {
                NodeData::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(fp_text, "AA:BB:CC");
    }

    #[test]
    fn cert_closed_hides_open_class() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &ChromeModel::default());
        let overlay = doc.find_by_id(crate::ids::CERT_OVERLAY).expect("asset has #certOverlay");
        assert!(!has_class(&doc, overlay, "open"));
    }

    #[test]
    fn print_open_toggles_the_open_class() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &ChromeModel { print_open: true, ..ChromeModel::default() });
        let overlay = doc.find_by_id(crate::ids::PRINT_OVERLAY).expect("asset has #printOverlay");
        assert!(has_class(&doc, overlay, "open"));

        bind_model(&mut doc, &ChromeModel { print_open: false, ..ChromeModel::default() });
        assert!(!has_class(&doc, overlay, "open"));
    }

    fn text_of(doc: &Document, id: NodeId) -> String {
        doc.get(id)
            .children
            .iter()
            .filter_map(|&c| match &doc.get(c).data {
                NodeData::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn default_content_view_is_page() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &ChromeModel::default());
        let page = doc.find_by_id(crate::ids::VIEW_PAGE).expect("asset has #view-page");
        assert!(has_class(&doc, page, "active"));
        let history = doc.find_by_id(crate::ids::VIEW_HISTORY).expect("asset has #view-history");
        assert!(!has_class(&doc, history, "active"));
    }

    #[test]
    fn content_view_switches_active_view_exclusively() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &ChromeModel { content_view: ChromeContentView::Settings, ..ChromeModel::default() });
        let page = doc.find_by_id(crate::ids::VIEW_PAGE).expect("asset has #view-page");
        let history = doc.find_by_id(crate::ids::VIEW_HISTORY).expect("asset has #view-history");
        let bookmarks = doc.find_by_id(crate::ids::VIEW_BOOKMARKS).expect("asset has #view-bookmarks");
        let settings = doc.find_by_id(crate::ids::VIEW_SETTINGS).expect("asset has #view-settings");
        assert!(!has_class(&doc, page, "active"));
        assert!(!has_class(&doc, history, "active"));
        assert!(!has_class(&doc, bookmarks, "active"));
        assert!(has_class(&doc, settings, "active"));
    }

    #[test]
    fn history_banner_and_rows_are_rebuilt_from_the_model() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            history: ChromeHistoryModel {
                banner: true,
                rows: vec![
                    ChromeHistoryRow::Group("Сегодня".to_owned()),
                    ChromeHistoryRow::Entry {
                        title: "Example".to_owned(),
                        url: "example.com".to_owned(),
                        time_label: "14:02".to_owned(),
                    },
                ],
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let banner = doc.find_by_id(crate::ids::HIST_BANNER).expect("asset has #histBanner");
        assert_eq!(doc.get(banner).get_attr("style"), Some(""));
        let wrap = find_by_class(&doc, "hist-wrap").expect("asset has .hist-wrap");
        let days: Vec<NodeId> = doc.get(wrap).children.iter().copied().filter(|&c| has_class(&doc, c, "hist-day")).collect();
        assert_eq!(days.len(), 1, "old demo groups must be gone, only the 1 model group remains");
        assert_eq!(text_of(&doc, days[0]), "Сегодня");
        let items: Vec<NodeId> = doc.get(wrap).children.iter().copied().filter(|&c| has_class(&doc, c, "hist-item")).collect();
        assert_eq!(items.len(), 1, "old demo entries must be gone, only the 1 model entry remains");
        let title = doc.get(items[0]).children.iter().copied().nth(1).expect("entry has a text wrapper");
        let title_el = doc.get(title).children.iter().copied().find(|&c| has_class(&doc, c, "hist-title")).unwrap();
        assert_eq!(text_of(&doc, title_el), "Example");
    }

    #[test]
    fn history_banner_hidden_when_not_anonymous() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &ChromeModel::default());
        let banner = doc.find_by_id(crate::ids::HIST_BANNER).expect("asset has #histBanner");
        assert_eq!(doc.get(banner).get_attr("style"), Some("display:none"));
    }

    #[test]
    fn bookmarks_folders_and_cards_are_rebuilt_from_the_model() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            bookmarks: ChromeBookmarksModel {
                folders: vec![
                    ChromeBookmarkFolderModel { label: "Все закладки".to_owned(), active: false },
                    ChromeBookmarkFolderModel { label: "Работа".to_owned(), active: true },
                ],
                title: "Работа".to_owned(),
                cards: vec![ChromeBookmarkCardModel {
                    fav_letter: "R".to_owned(),
                    title: "Rust".to_owned(),
                    url: "rust-lang.org".to_owned(),
                }],
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let tree = find_by_class(&doc, "bm-tree").expect("asset has .bm-tree");
        let folders: Vec<NodeId> =
            doc.get(tree).children.iter().copied().filter(|&c| has_class(&doc, c, "bm-folder")).collect();
        assert_eq!(folders.len(), 2, "old demo folders must be gone, only the 2 model folders remain");
        assert!(!has_class(&doc, folders[0], "active"));
        assert!(has_class(&doc, folders[1], "active"));
        assert!(has_class(&doc, folders[1], "indent"), "every folder but the first gets .indent");
        let grid = find_by_class(&doc, "bm-grid").expect("asset has .bm-grid");
        let cards: Vec<NodeId> =
            doc.get(grid).children.iter().copied().filter(|&c| has_class(&doc, c, "bm-card")).collect();
        assert_eq!(cards.len(), 1, "old demo cards must be gone, only the 1 model card remains");
        let title_el = doc.get(cards[0]).children.iter().copied().find(|&c| has_class(&doc, c, "bm-title")).unwrap();
        assert_eq!(text_of(&doc, title_el), "Rust");
    }

    #[test]
    fn settings_section_toggles_nav_and_section_active_class() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            settings: ChromeSettingsModel { active_section: "appearance".to_owned(), ..ChromeSettingsModel::default() },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let nav = find_by_class(&doc, "set-nav").expect("asset has .set-nav");
        let active_items: Vec<NodeId> =
            doc.get(nav).children.iter().copied().filter(|&c| has_class(&doc, c, "active")).collect();
        assert_eq!(active_items.len(), 1);
        assert_eq!(doc.get(active_items[0]).get_attr("data-section"), Some("appearance"));
        let main = find_by_class(&doc, "set-main").expect("asset has .set-main");
        let active_sections: Vec<NodeId> =
            doc.get(main).children.iter().copied().filter(|&c| has_class(&doc, c, "active")).collect();
        assert_eq!(active_sections.len(), 1);
        assert_eq!(doc.get(active_sections[0]).get_attr("data-set"), Some("appearance"));
    }

    #[test]
    fn settings_privacy_toggles_reflect_ad_block_and_fingerprint_state() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            settings: ChromeSettingsModel {
                active_section: "privacy".to_owned(),
                ad_block_on: true,
                fingerprint_on: false,
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let main = find_by_class(&doc, "set-main").expect("asset has .set-main");
        let privacy = doc
            .get(main)
            .children
            .iter()
            .copied()
            .find(|&c| doc.get(c).get_attr("data-set") == Some("privacy"))
            .expect("asset has the privacy section");
        let toggles = find_descendants_by_class(&doc, privacy, "toggle");
        assert!(toggles.len() >= 2, "privacy section must have at least the 2 mapped toggles");
        assert!(has_class(&doc, toggles[0], "on"), "ad_block_on must set the first toggle's .on class");
        assert!(!has_class(&doc, toggles[1], "on"), "fingerprint_on=false must clear the second toggle's .on class");
    }

    #[test]
    fn right_sidebar_open_and_tab_are_bound() {
        let mut doc = parse_asset();
        let model = ChromeModel {
            right_sidebar: ChromeRightSidebarModel { open: true, tab: ChromeSidebarTab::Web },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        let panel = doc.find_by_id(crate::ids::RIGHT_SIDEBAR).expect("asset has #rightSidebar");
        assert!(has_class(&doc, panel, "open"));
        let title = doc.find_by_id(crate::ids::RS_TITLE).expect("asset has #rsTitle");
        assert_eq!(text_of(&doc, title), "Web");
        let ai_body = doc.find_by_id(crate::ids::RS_BODY_AI).expect("asset has #rsBodyAi");
        assert_eq!(doc.get(ai_body).get_attr("style"), Some("display:none"));
        let web_body = doc.find_by_id(crate::ids::RS_BODY_WEB).expect("asset has #rsBodyWeb");
        assert_eq!(doc.get(web_body).get_attr("style"), Some(""));
    }

    #[test]
    fn right_sidebar_closed_hides_open_class() {
        let mut doc = parse_asset();
        bind_model(&mut doc, &ChromeModel::default());
        let panel = doc.find_by_id(crate::ids::RIGHT_SIDEBAR).expect("asset has #rightSidebar");
        assert!(!has_class(&doc, panel, "open"));
    }
}
