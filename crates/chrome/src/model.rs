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

use lumen_dom::{Attribute, Document, NodeData, NodeId, QualName};

/// Snapshot of shell state [`bind_model`] reflects into the chrome document.
///
/// Built fresh by the shell on every [`bind_model`] call (see
/// `Lumen::chrome_model_snapshot` in `crates/shell/src/main.rs`) — there is
/// no retained/diffed state here, matching the brief's "простейший" diff.
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone)]
pub struct ChromeBookmarkFolderModel {
    /// Folder label (`"Все закладки"` for the root/no-filter entry).
    pub label: String,
    /// `true` for the currently selected filter — adds `.active`.
    pub active: bool,
}

/// One `.bm-card` in `#view-bookmarks`'s `.bm-grid` (CC-10b). Per-card
/// actions are out of this slice's DoD — same gap as
/// [`ChromeHistoryRow::Entry`].
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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
        doc.append_child(wrap, node);
    }
}

fn build_hist_item(doc: &mut Document, title: &str, url: &str, time_label: &str) -> NodeId {
    let item = doc.create_element(QualName::html("div"));
    set_attr(doc, item, "class", "hist-item");

    let fav = doc.create_element(QualName::html("div"));
    set_attr(doc, fav, "class", "hist-fav");
    append_text(doc, fav, &first_letter(title));
    doc.append_child(item, fav);

    let text = doc.create_element(QualName::html("div"));
    let title_el = doc.create_element(QualName::html("div"));
    set_attr(doc, title_el, "class", "hist-title");
    append_text(doc, title_el, title);
    doc.append_child(text, title_el);
    let url_el = doc.create_element(QualName::html("div"));
    set_attr(doc, url_el, "class", "hist-url");
    append_text(doc, url_el, url);
    doc.append_child(text, url_el);
    doc.append_child(item, text);

    let time = doc.create_element(QualName::html("div"));
    set_attr(doc, time, "class", "hist-time");
    append_text(doc, time, time_label);
    doc.append_child(item, time);

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
            doc.append_child(tree, node);
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
        doc.append_child(grid, node);
    }
}

fn build_bm_card(doc: &mut Document, card: &ChromeBookmarkCardModel) -> NodeId {
    let node = doc.create_element(QualName::html("div"));
    set_attr(doc, node, "class", "bm-card");

    let fav = doc.create_element(QualName::html("div"));
    set_attr(doc, fav, "class", "bm-fav");
    append_text(doc, fav, &card.fav_letter);
    doc.append_child(node, fav);

    let title = doc.create_element(QualName::html("div"));
    set_attr(doc, title, "class", "bm-title");
    append_text(doc, title, &card.title);
    doc.append_child(node, title);

    let url = doc.create_element(QualName::html("div"));
    set_attr(doc, url, "class", "bm-url");
    append_text(doc, url, &card.url);
    doc.append_child(node, url);

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
    remove_children_with_class(doc, list, "cp-row");
    if palette.results.is_empty() {
        let empty = doc.create_element(QualName::html("div"));
        set_attr(doc, empty, "class", "cp-empty cp-row");
        append_text(doc, empty, "Ничего не найдено");
        doc.append_child(list, empty);
        return;
    }
    for r in &palette.results {
        let row = build_cp_row(doc, r);
        doc.append_child(list, row);
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
    doc.append_child(row, icon);

    let text = doc.create_element(QualName::html("div"));
    set_attr(doc, text, "class", "dd-text");
    let title = doc.create_element(QualName::html("div"));
    set_attr(doc, title, "class", "dd-title");
    append_text(doc, title, &r.label);
    doc.append_child(text, title);
    let sub = doc.create_element(QualName::html("div"));
    set_attr(doc, sub, "class", "dd-sub");
    append_text(doc, sub, &r.sub_label);
    doc.append_child(text, sub);
    doc.append_child(row, text);

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
        doc.append_child(container, row);
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
    doc.append_child(row, icon);

    let text = doc.create_element(QualName::html("div"));
    set_attr(doc, text, "class", "dd-text");
    let title = doc.create_element(QualName::html("div"));
    set_attr(doc, title, "class", "dd-title");
    append_text(doc, title, &s.label);
    doc.append_child(text, title);
    let sub = doc.create_element(QualName::html("div"));
    set_attr(doc, sub, "class", "dd-sub");
    append_text(doc, sub, &s.sub_label);
    doc.append_child(text, sub);
    doc.append_child(row, text);

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
        let children: Vec<NodeId> = doc.get(count).children.clone();
        for child in children {
            doc.detach(child);
        }
        append_text(doc, count, &find.count_label);
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
        doc.append_child(list, card);
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
    doc.append_child(row, icon);

    let text_wrap = doc.create_element(QualName::html("div"));
    let name = doc.create_element(QualName::html("div"));
    set_attr(doc, name, "class", "dl-name");
    append_text(doc, name, &d.name);
    doc.append_child(text_wrap, name);
    let meta = doc.create_element(QualName::html("div"));
    set_attr(doc, meta, "class", "dl-meta");
    append_text(doc, meta, &d.meta);
    doc.append_child(text_wrap, meta);
    doc.append_child(row, text_wrap);
    doc.append_child(card, row);

    if let Some(fraction) = d.progress_fraction {
        let track = doc.create_element(QualName::html("div"));
        set_attr(doc, track, "class", "dl-progress-track");
        let fill = doc.create_element(QualName::html("div"));
        set_attr(doc, fill, "class", "dl-progress-fill");
        set_attr(doc, fill, "style", &format!("width:{}%", (fraction.clamp(0.0, 1.0) * 100.0)));
        doc.append_child(track, fill);
        doc.append_child(card, track);
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
        let children: Vec<NodeId> = doc.get(stat).children.clone();
        for child in children {
            doc.detach(child);
        }
        append_text(doc, stat, &blocked_total.to_string());
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
        let children: Vec<NodeId> = doc.get(warn).children.clone();
        for child in children {
            doc.detach(child);
        }
        append_text(doc, warn, "\u{26A0} ");
        let span = doc.create_element(QualName::html("span"));
        append_text(doc, span, message);
        doc.append_child(warn, span);
    }
}

fn rebuild_tab_list(doc: &mut Document, container: NodeId, tabs: &[ChromeTabModel]) {
    remove_children_with_class(doc, container, "tab-row");
    for tab in tabs {
        let row = build_tab_row(doc, tab);
        doc.append_child(container, row);
    }
}

fn build_tab_row(doc: &mut Document, tab: &ChromeTabModel) -> NodeId {
    let row = doc.create_element(QualName::html("div"));
    let mut class = match (tab.active, tab.sleeping) {
        (true, _) => "tab-row active".to_owned(),
        (false, true) => "tab-row sleeping".to_owned(),
        (false, false) => "tab-row".to_owned(),
    };
    if tab.is_child {
        class.push_str(" child");
    }
    set_attr(doc, row, "class", &class);
    set_attr(doc, row, "data-action", "select-tab");
    set_attr(doc, row, "data-tab-id", &tab.id.to_string());

    if tab.is_child {
        let tree_line = doc.create_element(QualName::html("span"));
        set_attr(doc, tree_line, "class", "tree-line");
        doc.append_child(row, tree_line);
    }

    if let Some(color) = &tab.container_color {
        let stripe = doc.create_element(QualName::html("span"));
        set_attr(doc, stripe, "class", "container-stripe");
        set_attr(doc, stripe, "style", &format!("background:{color}"));
        doc.append_child(row, stripe);
    }

    let fav = doc.create_element(QualName::html("span"));
    set_attr(doc, fav, "class", "tab-fav");
    append_text(doc, fav, &first_letter(&tab.title));
    doc.append_child(row, fav);

    let title = doc.create_element(QualName::html("span"));
    set_attr(doc, title, "class", "tab-title");
    append_text(doc, title, &tab.title);
    doc.append_child(row, title);

    if tab.sleeping {
        let badge = doc.create_element(QualName::html("span"));
        set_attr(doc, badge, "class", "tab-badge");
        set_attr(doc, badge, "title", "Гибернирована");
        append_text(doc, badge, "\u{2726}");
        doc.append_child(row, badge);
    } else {
        let close = doc.create_element(QualName::html("button"));
        set_attr(doc, close, "class", "tab-close");
        set_attr(doc, close, "data-action", "close-tab");
        // Carries its own copy of `data-tab-id` (not just the parent row) so
        // the shell's `chrome_action_at`/`dispatch_chrome_action` — which
        // only sees the `data-action`-carrying node, not the full hit path —
        // can resolve a close click straight to a tab id without walking up
        // to the row.
        set_attr(doc, close, "data-tab-id", &tab.id.to_string());
        doc.append_child(row, close);
    }
    row
}

fn rebuild_workspace_list(doc: &mut Document, container: NodeId, workspaces: &[ChromeWorkspaceModel]) {
    remove_children_with_class(doc, container, "ws-item");
    // The "+" add-workspace button (`.ws-add`) is not a `.ws-item` and is
    // therefore untouched by the removal above — new items are inserted
    // before it so it stays last, matching the asset's own order.
    let add_btn = doc
        .get(container)
        .children
        .iter()
        .copied()
        .find(|&c| doc.get(c).get_attr("data-action") == Some("add-workspace"));
    for ws in workspaces {
        let item = build_workspace_item(doc, ws);
        match add_btn {
            Some(btn) => doc.insert_before(item, btn),
            None => doc.append_child(container, item),
        }
    }
}

fn build_workspace_item(doc: &mut Document, ws: &ChromeWorkspaceModel) -> NodeId {
    let item = doc.create_element(QualName::html("button"));
    set_attr(doc, item, "class", if ws.active { "ws-item active" } else { "ws-item" });
    set_attr(doc, item, "data-action", "select-workspace");
    set_attr(doc, item, "data-ws-id", &ws.id.to_string());
    set_attr(doc, item, "style", &format!("--ws-color:{}", ws.color));

    let icon = doc.create_element(QualName::html("span"));
    set_attr(doc, icon, "class", "ws-icon");
    set_attr(doc, icon, "style", &format!("background:{}", ws.color));
    append_text(doc, icon, &first_letter(&ws.name));
    doc.append_child(item, icon);

    let lbl = doc.create_element(QualName::html("span"));
    set_attr(doc, lbl, "class", "lbl");
    append_text(doc, lbl, &ws.name);
    doc.append_child(item, lbl);

    item
}

/// Mirrors [`rebuild_tab_list`] into `#hbarTabs` (`.hbar-tab` rows, CC-8's
/// horizontal-layout tab bar) so switching layouts doesn't leave the
/// horizontal bar showing stale asset demo data. The asset's `.hbar-tab`
/// markup has no tree/container-strip visuals (flat list only), so this
/// intentionally omits [`ChromeTabModel::is_child`]/`container_color`
/// unlike [`build_tab_row`].
fn rebuild_hbar_tab_list(doc: &mut Document, container: NodeId, tabs: &[ChromeTabModel]) {
    remove_children_with_class(doc, container, "hbar-tab");
    for tab in tabs {
        let row = build_hbar_tab(doc, tab);
        doc.append_child(container, row);
    }
}

fn build_hbar_tab(doc: &mut Document, tab: &ChromeTabModel) -> NodeId {
    let row = doc.create_element(QualName::html("div"));
    set_attr(doc, row, "class", if tab.active { "hbar-tab active" } else { "hbar-tab" });
    set_attr(doc, row, "data-action", "select-tab");
    set_attr(doc, row, "data-tab-id", &tab.id.to_string());

    let fav = doc.create_element(QualName::html("span"));
    set_attr(doc, fav, "class", "tab-fav");
    append_text(doc, fav, &first_letter(&tab.title));
    doc.append_child(row, fav);

    let title = doc.create_element(QualName::html("span"));
    set_attr(doc, title, "class", "tab-title");
    append_text(doc, title, &tab.title);
    doc.append_child(row, title);

    row
}

/// Mirrors [`rebuild_workspace_list`] into `.hbar-ws` (`.hbar-ws-pill`
/// buttons, CC-8's horizontal-layout workspace switcher).
fn rebuild_hbar_ws_list(doc: &mut Document, container: NodeId, workspaces: &[ChromeWorkspaceModel]) {
    remove_children_with_class(doc, container, "hbar-ws-pill");
    for ws in workspaces {
        let pill = build_hbar_ws_pill(doc, ws);
        doc.append_child(container, pill);
    }
}

fn build_hbar_ws_pill(doc: &mut Document, ws: &ChromeWorkspaceModel) -> NodeId {
    let pill = doc.create_element(QualName::html("button"));
    set_attr(doc, pill, "class", if ws.active { "hbar-ws-pill active" } else { "hbar-ws-pill" });
    set_attr(doc, pill, "data-action", "select-workspace");
    set_attr(doc, pill, "data-ws-id", &ws.id.to_string());
    set_attr(doc, pill, "style", &format!("--ws-color:{}", ws.color));
    append_text(doc, pill, &ws.name);
    pill
}

fn first_letter(s: &str) -> String {
    s.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "\u{2022}".to_string())
}

fn remove_children_with_class(doc: &mut Document, container: NodeId, class: &str) {
    let children: Vec<NodeId> = doc.get(container).children.clone();
    for child in children {
        if has_class(doc, child, class) {
            doc.detach(child);
        }
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
    doc.append_child(parent, node);
}

fn set_attr(doc: &mut Document, id: NodeId, name: &str, value: &str) {
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
        if let Some(attr) = attrs.iter_mut().find(|a| a.name.local.eq_ignore_ascii_case(name)) {
            attr.value = value.to_string();
        } else {
            attrs.push(Attribute { name: QualName::html(name.to_ascii_lowercase()), value: value.to_string() });
        }
    }
}

fn remove_attr(doc: &mut Document, id: NodeId, name: &str) {
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
        attrs.retain(|a| !a.name.local.eq_ignore_ascii_case(name));
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

/// Replaces `id`'s text-node children with a single text node containing
/// `text` — the same "detach old children, append one text node" pattern
/// [`bind_omnibox`]/[`bind_find_bar`] already use inline, factored out for
/// [`bind_cert`]'s several title/value cells (CC-10).
fn set_text(doc: &mut Document, id: NodeId, text: &str) {
    let children: Vec<NodeId> = doc.get(id).children.clone();
    for child in children {
        doc.detach(child);
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
