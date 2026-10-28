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

use lumen_dom::{Attribute, Document, Namespace, NodeData, NodeId, QualName};

mod dialog;
mod security;
pub use security::ChromeSecurityModel;
mod login_bar;
mod hang_bar;
mod media_bar;
pub use dialog::ChromeDialogModel;
pub use login_bar::ChromeLoginOfferModel;
pub use hang_bar::ChromeHangBarModel;
pub use media_bar::ChromeMediaBarModel;
mod update_bar;
pub use update_bar::{ChromeUpdateAction, ChromeUpdateModel};

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
    /// `true` shows the profile dropdown (`#profileMenu`/`#profileMenuH`,
    /// UX-CHROME-PANELS) — mirrors `ProfileMenuPanel::visible`.
    pub profile_menu_open: bool,
    /// Open tabs, in strip order.
    pub tabs: Vec<ChromeTabModel>,
    /// Workspaces shown in the sidebar switcher.
    pub workspaces: Vec<ChromeWorkspaceModel>,
    /// `true` shows `#workspaceBar` (UX-CHROME-PANELS) — mirrors `WorkspacePanel::visible`.
    pub workspace_bar_open: bool,
    /// UX-TAB-CLOSE-FREEZE: `Some(w)` pins every `.hbar-tab` to `w` CSS px
    /// (the width the tab had when it was closed with the mouse), so the next
    /// tab slides under the cursor instead of the strip re-flowing; `None`
    /// leaves the tabs to the equal-width flex rule of the asset.
    pub hbar_tab_width: Option<f32>,
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
    /// `true` shows the header «Очистить» button (`.dl-clear`) — at least one
    /// download is finished (`DownloadManager::has_finished_entries`).
    pub downloads_clearable: bool,
    /// `true` shows `#archivePanel` (BUG-408) — mirrors `TabArchive::visible`.
    pub archive_open: bool,
    /// Archived-tab entries rendered into `#archivePanel`'s `.arc-list`
    /// (BUG-408) — mirrors `TabArchive::entries`.
    pub archive: Vec<ChromeArchiveEntryModel>,
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
    /// Grant state for the five permission rows, in asset order (`Камера`,
    /// `Микрофон`, `Уведомления`, `Буфер обмена`, `Всплывающие окна`) — exactly
    /// `PermissionKind::ALL`. The last two rows were added to the design
    /// reference by [BUG-411](../../../bugs/BUG-411-FIXED.md); before that the
    /// asset had only the first two, which left `Notifications`/`Clipboard`
    /// unreachable from the UI.
    pub permissions: [ChromePermState; 5],
    /// Host the popover applies to (`shieldSiteRow` + the `#permDomain` span in
    /// its `Разрешения — …` heading), or empty when no host-bearing page is
    /// loaded (`about:blank`, `file://`) — [BUG-411](../../../bugs/BUG-411-FIXED.md).
    ///
    /// Mirrors `ShieldsPanel::current_domain`; an empty string renders as
    /// [`NO_DOMAIN_LABEL`] rather than leaving the design's `figma.com` demo
    /// text in place.
    pub popover_domain: String,
    /// Whether request filtering is on for [`Self::popover_domain`] — drives
    /// `#shieldSiteToggle`'s `.on` class and `#shieldSiteState`'s Вкл/Выкл
    /// text ([BUG-411](../../../bugs/BUG-411-FIXED.md)). Mirrors
    /// `ShieldsPanel::enabled_for_current`, which is also what the shell
    /// pushes into `lumen_network::set_global_adblock_enabled`.
    pub site_shields_on: bool,
    /// Number of compatibility-list records applied to [`Self::popover_domain`]
    /// (COMPAT-PACK-IND). `0` hides `#compatRow`; otherwise the row shows
    /// [`compat_fixes_label`] with its «Подробнее» / «Выключить и перезагрузить» buttons.
    pub compat_fixes: u32,
    /// «Сайт работает не так?» section of `#permPopover` (COMPAT-PANEL).
    pub fix: ChromeFixPanelModel,
    /// `#cpOverlay` snapshot (CC-10) — mirrors `CommandPalette`.
    pub palette: ChromePaletteModel,
    /// `#certOverlay` snapshot (CC-10) — mirrors `CertPanel`.
    pub cert: ChromeCertModel,
    /// `#shortcutsOverlay` snapshot (UX-CHROME-PANELS) — mirrors `ShortcutsPanel`.
    pub shortcuts: ChromeShortcutsModel,
    /// `#devtools` snapshot (UX-CHROME-PANELS) — mirrors `ConsolePanel`.
    pub console: ChromeConsoleModel,
    /// `#networkPanel` snapshot (UX-CHROME-PANELS) — mirrors `NetworkPanel`.
    pub network: ChromeNetworkModel,
    /// `#readLaterPanel` snapshot (UX-CHROME-PANELS) — mirrors `ReadLaterPanel`.
    pub read_later: ChromeReadLaterModel,
    /// `#privacyPanel` snapshot (UX-CHROME-PANELS) — mirrors `PrivacyPanel`.
    pub privacy: ChromePrivacyModel,
    /// `.focus-timer` snapshot (UX-CHROME-PANELS) — mirrors `FocusModePanel`.
    pub focus: ChromeFocusModel,
    /// `#noteOverlay` snapshot (UX-CHROME-PANELS) — mirrors `NoteViewerPanel`.
    pub note: ChromeNoteModel,
    /// `#tabCtxMenu` snapshot (UX-CHROME-PANELS) — mirrors `TabContextMenu`.
    pub tab_ctx: ChromeTabCtxModel,
    /// `#pipCard` snapshot (UX-CHROME-PANELS) — mirrors `PipWindow`.
    pub pip: ChromePipModel,
    /// `#pageCtxMenu` snapshot (UX-CHROME-PANELS) — mirrors `PageContextMenu`.
    pub page_ctx: ChromePageCtxModel,
    /// `#inspPanel` snapshot (UX-CHROME-PANELS) — mirrors `DomInspectorPanel`.
    pub inspector: ChromeInspectorModel,
    /// `#accessOverlay` snapshot (UX-CHROME-PANELS) — mirrors `A11yPanel`.
    pub access: ChromeAccessModel,
    /// `#printOverlay` snapshot (CC-10, extended [BUG-420](../../../bugs/BUG-420-FIXED.md)) — mirrors `PrintPanel`.
    pub print: ChromePrintModel,
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
    /// `#demoBar` floating control panel snapshot (CC-18).
    pub control_panel: ChromeControlPanelModel,
    /// `#updateBar` + settings "Обновления" snapshot (UPD-9).
    pub update: ChromeUpdateModel,
    /// `#loginBar` «сохранить пароль?» (UX-PASSWORDS).
    pub login_offer: ChromeLoginOfferModel,
    /// `#mediaBar` «для видео нужен компонент ОС» (COMPAT-MEDIA-OSDEC-WIN-BAR).
    pub media_bar: ChromeMediaBarModel,
    /// `#hangBar` «страница не отвечает» (COMPAT-HANG).
    pub hang_bar: ChromeHangBarModel,
    /// `#dialogBox` — `alert`/`confirm`/`prompt` страницы (UX-DIALOGS).
    pub dialog: ChromeDialogModel,
    /// `#securityBox` — ошибка сертификата / вредоносный сайт (UX-SECURITY-UI).
    pub security: ChromeSecurityModel,
}

/// `#demoBar` floating control panel snapshot (CC-18) — the panel itself
/// stays permanently mounted (it is `position:fixed`, outside the page
/// flow), so unlike the modal overlays above there is no open/closed flag
/// here beyond [`Self::mini_open`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeControlPanelModel {
    /// Active `#demoSwitch` layout form — bound to `body[data-demo]` plus
    /// `.active` on the matching button.
    pub shape: ControlPanelShape,
    /// `true` shows the expanded card even in [`ControlPanelShape::Mini`]
    /// (`#demoBar.mini-open`) — irrelevant, but harmless, for the other six
    /// shapes (their CSS never reads `.mini-open`).
    pub mini_open: bool,
    /// `true` shows `#infoPanel` (`.open`).
    pub info_open: bool,
}

/// One of the floating control panel's 7 layout forms (`body[data-demo="…"]`
/// in `assets/chrome/chrome.html` carries the CSS for each).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControlPanelShape {
    /// Вертикальная карточка с подписями групп (bottom-left, default).
    #[default]
    Card,
    /// Узкая, без подписей.
    Compact,
    /// Горизонтальная полоса, сверху по центру.
    Bar,
    /// Высокий вертикальный рейл у правого края.
    Dock,
    /// Кнопки группами по сетке 2×N.
    Grid,
    /// Широкая, две колонки, снизу по центру.
    Wide,
    /// Свёрнута в пилюлю (bottom-right); [`ChromeControlPanelModel::mini_open`]
    /// expands it back to a full card.
    Mini,
}

impl ControlPanelShape {
    /// `data-demo`/`data-demo-variant` attribute value.
    #[must_use]
    pub const fn attr_value(self) -> &'static str {
        match self {
            Self::Card => "card",
            Self::Compact => "compact",
            Self::Bar => "bar",
            Self::Dock => "dock",
            Self::Grid => "grid",
            Self::Wide => "wide",
            Self::Mini => "mini",
        }
    }

    /// Parses a `data-demo-variant` attribute value. `None` for anything
    /// else (should not occur against the build-gated asset).
    #[must_use]
    pub fn from_attr_value(value: &str) -> Option<Self> {
        match value {
            "card" => Some(Self::Card),
            "compact" => Some(Self::Compact),
            "bar" => Some(Self::Bar),
            "dock" => Some(Self::Dock),
            "grid" => Some(Self::Grid),
            "wide" => Some(Self::Wide),
            "mini" => Some(Self::Mini),
            _ => None,
        }
    }
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
    /// A `.hist-item` entry. Since [BUG-422](../../../bugs/BUG-422-FIXED.md)
    /// the rebuilt row carries `data-action="open-history-entry"` plus the
    /// design's `.hist-actions` strip (bookmark/copy/delete), each button
    /// stamped with `data-hist-url`/`data-hist-title` — no extra model field
    /// is needed, `url`/`title` below are the only context the shell reads
    /// back (both `History::delete` and `Bookmarks::add` key off the URL).
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
    /// `None`-filter entry) followed by `BookmarkPanel::folders`. Since
    /// [BUG-422](../../../bugs/BUG-422-FIXED.md) each row also carries
    /// `data-action="select-folder"` + `data-bm-folder`, so clicking one
    /// re-filters the grid.
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
    /// The `BookmarkPanel::selected_folder` value this row sets when clicked
    /// (`None` = the "all folders" row), stamped as `data-bm-folder`
    /// (BUG-422). Kept separate from `label` because the root row's label is
    /// display text, not a folder path.
    pub filter: Option<String>,
}

/// One `.bm-card` in `#view-bookmarks`'s `.bm-grid` (CC-10b). Since
/// [BUG-422](../../../bugs/BUG-422-FIXED.md) the card carries
/// `data-action="open-bookmark"` and a `.bm-actions` delete button, both
/// keyed off `url` — same shape as [`ChromeHistoryRow::Entry`].
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
    /// Сохранённые логины для раздела «Пароли» (UX-PASSWORDS, срез 5):
    /// `(origin, username)`. Пароли сюда не попадают.
    pub passwords: Vec<(String, String)>,
    /// `false` — хранилище недоступно (приватный режим, Tor): раздел это говорит.
    pub passwords_available: bool,
    /// Сохранённые значения форм для раздела «Автозаполнение» (UX-AUTOFILL,
    /// срез 5): `(origin, ключ поля, значение)`.
    pub autofill: Vec<(String, String, String)>,
    /// Сохранённые карты: `(последние четыре цифры, подпись)`; номер сюда не попадает.
    pub cards: Vec<(String, String)>,
    /// `false` — хранилище автозаполнения недоступно (приватный режим, Tor).
    pub autofill_available: bool,
    /// Решения сайтов для раздела «Разрешения» (UX-PERMISSIONS):
    /// `(origin, вид, разрешено?)`.
    pub site_permissions: Vec<(String, String, bool)>,
    /// Top-level site текущей страницы для «Данные сайта» (UX-PARTITION);
    /// `None` — страницы без сайта (`file:`, пустая вкладка).
    pub current_site: Option<String>,
    /// Проверка орфографии включена (UX-SPELLCHECK) — `#spellToggle`.
    pub spell_enabled: bool,
    /// Языки словарей `(стем, включён)` для `#spellLangList`; пусто — словари
    /// не загружены (ещё грузятся или папки `data/spell` нет).
    pub spell_languages: Vec<(String, bool)>,
    /// Языки перевода страницы `(код, подпись, выбран)` для `#translateLangList`
    /// (UX-TRANSLATE); пусто — список остаётся статичным.
    pub translate_languages: Vec<(String, String, bool)>,
    /// Модель перевода, показанная в `#translateModel`.
    pub translate_model: String,
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
    /// Suggestion-kind label shown right-aligned in the row (`.dd-tag`,
    /// BUG-410) — e.g. "история"/"закладка"/"×3" for a repeated search
    /// query. Mirrors the legacy overlay's `OmniboxSuggestion::tag()`.
    pub tag: String,
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
    /// `true` when `count_label` is `"ERR"` (invalid regex pattern) — toggles
    /// `#findCount`'s `.error` class ([BUG-419](../../../bugs/BUG-419-FIXED.md)).
    pub error: bool,
    /// UX-LAYOUT-FIX: запрос, найденный после перевода раскладки (`привет`);
    /// пусто — перевод не применялся. Непустой показывает `#findFix` со ссылкой
    /// «искать исходное» (`ChromeAction::FindUseOriginal`).
    pub layout_fix: String,
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
    /// Buttons under the card (`.dl-actions`): Open + Reveal for finished
    /// downloads, Cancel for in-flight ones, none otherwise.
    pub actions: Vec<ChromeDownloadAction>,
}

/// One `.dl-actions` button of a [`ChromeDownloadModel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeDownloadAction {
    /// Open the file (`data-dl="open"`).
    Open,
    /// Reveal the file in the OS file manager (`data-dl="reveal"`).
    Reveal,
    /// Cancel the transfer (`data-dl="cancel"`).
    Cancel,
}

impl ChromeDownloadAction {
    /// `data-dl` attribute value the shell reads back via the chrome hit-test.
    pub fn attr(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Reveal => "reveal",
            Self::Cancel => "cancel",
        }
    }

    /// Inverse of [`Self::attr`].
    pub fn from_attr(v: &str) -> Option<Self> {
        match v {
            "open" => Some(Self::Open),
            "reveal" => Some(Self::Reveal),
            "cancel" => Some(Self::Cancel),
            _ => None,
        }
    }

    fn icon_and_label(self) -> (&'static str, &'static str) {
        match self {
            Self::Open => ("#i-ext", "Открыть"),
            Self::Reveal => ("#i-folder", "В папке"),
            Self::Cancel => ("#i-x", "Отмена"),
        }
    }
}

/// One `.arc-card` in `#archivePanel`'s `.arc-list` (BUG-408) — mirrors one
/// `tabs::archive::ArchivedTab`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChromeArchiveEntryModel {
    /// Round-trips through `data-archive-id` (`ArchivedTab::id`, opaque
    /// here) so a restore/dismiss click on the rebuilt row resolves back to
    /// a `TabArchive` entry.
    pub id: usize,
    /// `.arc-fav` single-letter fallback (first letter of `title`), mirroring
    /// [`ChromeTabModel::title`]'s own favicon fallback.
    pub fav_letter: String,
    /// `.arc-title`.
    pub title: String,
    /// `.arc-url`.
    pub url: String,
    /// `#RRGGBB` accent for the `.arc-stripe` left-edge strip — the caller
    /// derives this from `ArchivedTab::container.border_color()`, or `None`
    /// for `ContainerKind::None`, in which case the strip is omitted
    /// entirely (matches [`ChromeTabModel::container_color`]'s convention).
    pub container_color: Option<String>,
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
/// A row carries `data-cp-index` (its filtered index) rather than a
/// `data-action`: only `#cpOverlay` itself has one, to close on scrim click.
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
    /// Index into the palette's *filtered* list (scroll offset included) —
    /// stamped as `data-cp-index` so a click resolves to its row through
    /// the chrome hit test.
    pub index: usize,
}

/// `#certOverlay` snapshot (CC-10) — mirrors `CertPanel`/`PanelCertData`.
///
/// The design's 6 static `.cert-row`s + 1 `.cert-fp` cover a *subset* of
/// `PanelCertData`'s fields (no TLS version, revocation, CT-status, or
/// error-reason row exists — ph3-tls-hardening A6 added the latter two,
/// still unbound here) — this binds only what the markup has a slot for,
/// same honesty-over-fabrication call CC-9 made for `#statAds`/`#statFp`.
/// All-`None`/absent fields render as `"—"`, matching
/// `cert_panel::build_rows`'s own em-dash fallback for missing data. A
/// red/warning header for `PanelCertData::has_error()` is a follow-up CC
/// design-asset slice, not part of A6's shell-side wiring.
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

/// One `.sc-row` of `#shortcutsOverlay`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeShortcutRowModel {
    /// Index into `ShortcutsPanel::rows` — written to `data-sc-index` so a
    /// click resolves to the right row even though the list is windowed.
    pub index: usize,
    /// Action label, e.g. `"Перезагрузить"`.
    pub label: String,
    /// Binding badge, e.g. `"Ctrl+R"`, or the rebind prompt.
    pub binding: String,
    /// `true` while this row awaits a new keypress.
    pub rebinding: bool,
}

/// `#shortcutsOverlay` snapshot (UX-CHROME-PANELS) — mirrors `ShortcutsPanel`.
///
/// `rows` is the already-windowed slice the shell wants visible: the chrome
/// has no wheel-driven scroll container, so the shell owns the scroll offset.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeShortcutsModel {
    /// `true` shows the overlay — mirrors `ShortcutsPanel::visible`.
    pub open: bool,
    /// Rows rendered into `#scList`.
    pub rows: Vec<ChromeShortcutRowModel>,
}

/// One `#dt-console` line (UX-CHROME-PANELS).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeConsoleLineModel {
    /// `0` log, `1` warn, `2` error — same encoding as the JS runtime's
    /// console buffer.
    pub level: u8,
    /// Message text.
    pub text: String,
}

/// `#devtools` snapshot (UX-CHROME-PANELS) — mirrors `ConsolePanel`.
///
/// `lines` is the already-windowed tail the shell wants visible: the chrome
/// has no wheel-driven scroll container.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeConsoleModel {
    /// `true` shows the panel — mirrors `ConsolePanel::visible`.
    pub open: bool,
    /// Number of stored messages (shown in the tab title).
    pub total: usize,
    /// Lines rendered into `#dt-console`, oldest first.
    pub lines: Vec<ChromeConsoleLineModel>,
}

/// One `.net-row` of `#dt-network` (UX-CHROME-PANELS).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeNetworkRowModel {
    /// HTTP method.
    pub method: String,
    /// Status text: numeric code, `blocked`, failure stage or `…`.
    pub status: String,
    /// `2`/`3`/`4` pick the `.status-badge` colour (2xx / 3xx / 4xx-5xx,
    /// blocked, failed); `0` is a pending request without a badge colour.
    pub status_class: u8,
    /// Timing text (`123 ms`, `…`, `—`).
    pub time: String,
    /// Request URL (the chrome clips it with CSS).
    pub url: String,
    /// Request was blocked by the shields.
    pub blocked: bool,
    /// Request failed at the network level.
    pub failed: bool,
}

/// `#networkPanel` snapshot (UX-CHROME-PANELS) — mirrors `NetworkPanel`.
///
/// `rows` is the already-windowed slice (the shell owns the wheel scroll),
/// oldest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeNetworkModel {
    /// `true` shows the panel — mirrors `NetworkPanel::visible`.
    pub open: bool,
    /// Number of recorded requests (shown in the tab title).
    pub total: usize,
    /// Rows rendered into `#dt-network`.
    pub rows: Vec<ChromeNetworkRowModel>,
}

/// `.focus-timer` snapshot (UX-CHROME-PANELS) — mirrors `FocusModePanel`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeFocusModel {
    /// `true` shows the pill — focus mode is engaged.
    pub active: bool,
    /// `true` while the countdown runs (pause icon); `false` shows play.
    pub running: bool,
    /// Session complete: shows the `#focusDone` banner.
    pub finished: bool,
    /// Remaining time as `MM:SS`.
    pub label: String,
}

/// `#pipCard` snapshot (UX-CHROME-PANELS) — mirrors `PipWindow`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromePipModel {
    /// `true` shows the card.
    pub open: bool,
    /// Left edge in window CSS px.
    pub x: f32,
    /// Top edge in window CSS px.
    pub y: f32,
    /// Nominally playing — the centre button shows pause bars, else a triangle.
    pub playing: bool,
    /// Title-bar text; empty falls back to «Picture-in-Picture».
    pub title: String,
    /// Poster image URL; empty leaves the grey frame.
    pub poster: String,
}

/// `#tabCtxMenu` snapshot (UX-CHROME-PANELS) — mirrors `TabContextMenu`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeTabCtxModel {
    /// `true` shows the menu.
    pub open: bool,
    /// Left edge in window CSS px (already clamped inside the window).
    pub x: f32,
    /// Top edge in window CSS px (already clamped inside the window).
    pub y: f32,
    /// Target tab is pinned — the row reads «Открепить».
    pub pinned: bool,
    /// Target tab's group is collapsed — the row reads «Развернуть группу».
    pub collapsed: bool,
}

/// «Сайт работает не так?» section of `#permPopover` (COMPAT-PANEL): the ladder
/// of steps, the reset confirmation and the «Заработало?» question. Exactly one
/// of the three blocks is visible at a time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChromeFixPanelModel {
    /// The page is an http(s) one, so the steps apply; `false` hides the ladder.
    pub steps_enabled: bool,
    /// Show the «you will be signed out» confirmation instead of the ladder.
    pub confirm_reset: bool,
    /// The question text («Заработало?» with the step name) while one is pending.
    pub asking: Option<String>,
    /// Steps already tried on this site, in ladder order: no cache, reset,
    /// other browser — their titles are dimmed.
    pub tried: [bool; 3],
    /// Line under the section: a step's result or why there are no steps.
    pub note: String,
}

/// `#pageCtxMenu` snapshot (UX-CHROME-PANELS) — mirrors `PageContextMenu`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromePageCtxModel {
    /// `true` shows the menu.
    pub open: bool,
    /// Left edge in window CSS px (already clamped inside the window).
    pub x: f32,
    /// Top edge in window CSS px (already clamped inside the window).
    pub y: f32,
    /// Suggestion / saved-value rows, in order.
    pub suggestions: Vec<String>,
    /// Append the «Добавить в словарь» / «Пропустить» rows (spell menu only).
    pub spell_footer: bool,
    /// Rows `(data-page-ctx value, label)` appended after the suggestions
    /// behind a separator — the page menu's «Открыть в другом браузере».
    pub extra_rows: Vec<(String, String)>,
}

/// `#noteOverlay` snapshot (UX-CHROME-PANELS) — mirrors `NoteViewerPanel`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeNoteModel {
    /// `true` shows the modal.
    pub open: bool,
    /// Source URL the note was taken on.
    pub url: String,
    /// Highlighted text from the source page.
    pub selection: String,
    /// User comment; empty hides the row.
    pub comment: String,
}

/// One `.pv-row` of `#pvList` (UX-CHROME-PANELS).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromePrivacyRowModel {
    /// Tracker domain (primary line).
    pub host: String,
    /// Secondary line: matched filter or HTTP status.
    pub sub: String,
    /// `blocked` / `allowed` / `pending` — picks the dot and text colour.
    pub state: String,
}

/// `#privacyPanel` snapshot (UX-CHROME-PANELS) — mirrors `PrivacyPanel`.
///
/// `rows` is the already-windowed slice, newest first (the shell owns the
/// wheel scroll).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromePrivacyModel {
    /// `true` shows the panel — mirrors `PrivacyPanel::visible`.
    pub open: bool,
    /// Blocked requests in the snapshot.
    pub blocked: usize,
    /// Allowed (sent) requests in the snapshot.
    pub allowed: usize,
    /// `shown/total` scroll indicator; empty when the list fits.
    pub position: String,
    /// Rows rendered into `#pvList`.
    pub rows: Vec<ChromePrivacyRowModel>,
}

/// One `.rl-row` of `#rlList` (UX-CHROME-PANELS).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeReadLaterRowModel {
    /// Store id; round-trips through `data-read-later-id` on both buttons.
    pub id: i64,
    /// Page title.
    pub title: String,
    /// `host  ·  date` line.
    pub meta: String,
    /// Badge text (`Unread` / `Read` / `Archived`).
    pub status: String,
    /// `true` paints the green read badge instead of the blue unread one.
    pub read: bool,
}

/// `#readLaterPanel` snapshot (UX-CHROME-PANELS) — mirrors `ReadLaterPanel`.
///
/// `rows` is the already-windowed slice (the shell owns the wheel scroll).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeReadLaterModel {
    /// `true` shows the panel — mirrors `ReadLaterPanel::visible`.
    pub open: bool,
    /// Number of saved pages (shown in the header).
    pub total: usize,
    /// Rows rendered into `#rlList`.
    pub rows: Vec<ChromeReadLaterRowModel>,
}

/// One line of `#inspRows` (UX-CHROME-PANELS).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeInspectorRowModel {
    /// `0` property row (`key: value`), `1` rule selector header, `2` separator
    /// between rule blocks.
    pub kind: u8,
    /// Property name or selector text.
    pub key: String,
    /// Property value (empty for selector and separator lines).
    pub value: String,
}

/// `#inspPanel` snapshot (UX-CHROME-PANELS) — mirrors `DomInspectorPanel`.
///
/// `rows` and `net_rows` are the already-windowed slices (the shell owns the
/// wheel scroll). `rows` serves the Elements/Styles/Computed tabs, `net_rows`
/// the Network tab; the shell leaves the one that does not apply empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeInspectorModel {
    /// `true` shows the panel — mirrors `DomInspectorPanel::visible`.
    pub open: bool,
    /// Active tab: `0` Elements, `1` Styles, `2` Computed, `3` Network.
    pub tab: u8,
    /// Label of the pinned element (`div#main.card`); empty without selection.
    pub label: String,
    /// `NodeId(n)` of the pinned element; empty without selection.
    pub node: String,
    /// Placeholder shown instead of rows (`Hover a box…`, `(no CSS rules matched)`).
    pub hint: String,
    /// Property / selector lines of the active element tab.
    pub rows: Vec<ChromeInspectorRowModel>,
    /// Request rows of the Network tab, oldest first.
    pub net_rows: Vec<ChromeNetworkRowModel>,
    /// `shown/total` scroll indicator; empty when everything fits.
    pub position: String,
}

/// `#accessOverlay` snapshot (UX-CHROME-PANELS) — mirrors `A11yPanel`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeAccessModel {
    /// `true` shows the overlay — mirrors `A11yPanel::visible`.
    pub open: bool,
    /// `data-value` of the active `#acFontPills` pill (`"1"`, `"1.25"`, …).
    pub font_value: String,
    /// `#acMotionToggle` state (prefers-reduced-motion).
    pub reduced_motion: bool,
    /// `#acColorsToggle` state (forced colors).
    pub forced_colors: bool,
    /// `data-value` of the active `#acCursorPills` pill (`normal`/`large`/`extra`).
    pub cursor_value: String,
}

/// `#printOverlay` snapshot ([BUG-420](../../../bugs/BUG-420-FIXED.md)) —
/// mirrors `PrintPanel`.
///
/// The design's print form has controls for printer target, page range,
/// orientation, scale, and two checkboxes — of these only `orientation`
/// (`#printOrientationSelect`, a clean 1:1 `Книжная`/`Альбомная` ↔
/// `Portrait`/`Landscape` mapping) and `backgrounds` (`#printBgCheck`) map
/// onto a `PrintPanel` field *and* have a real engine effect
/// (`do_print_to_pdf_with_opts`'s `landscape`/`print_backgrounds` params).
/// Printer target, page range, scale, and the header/footer checkbox stay
/// static demo content — same honesty-over-fabrication call CC-9/CC-10 made
/// for `#statAds`/`#statFp`: "Принтер" has no non-PDF backend to select
/// between, page-range/scale have no click-driven text-edit or dropdown
/// mechanism in the chrome host yet, and there is no `PrintPanel` field for
/// headers/footers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromePrintModel {
    /// `true` shows the overlay — mirrors `PrintPanel::visible`.
    pub open: bool,
    /// `true` selects "Альбомная" (landscape) — mirrors
    /// `PrintPanel::orientation == Orientation::Landscape`.
    pub landscape: bool,
    /// `true` checks "Фон и графика" — mirrors `PrintPanel::print_backgrounds`.
    pub backgrounds: bool,
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
    /// `Some(percent)` shows the `#zoomBtn` page-zoom indicator with this
    /// label (UX-ZOOM); `None` — zoom is 100 % and the button stays hidden.
    pub zoom_percent: Option<u32>,
    /// `n > 0` shows the `#popupBtn` blocked-popup indicator with this count
    /// (UX-USER-ACTIVATION): the page called `window.open()` without a user
    /// gesture `n` times since it loaded.
    pub popups_blocked: u32,
    /// `Some(text)` shows the `#helpChip` next to the address bar with this question
    /// (COMPAT-FRUSTRATION); `None` hides it. A click on the text opens the «Сайт
    /// работает не так?» panel, «Не сейчас» silences the site.
    pub help_chip: Option<String>,
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
    /// `true` while this tab's navigation is in flight (UX-LOADING) — the
    /// `.tab-fav` slot becomes a `.spinner`, and for the active tab the
    /// reload button turns into a stop button.
    pub loading: bool,
    /// `true` when the tab has an opener (`TabEntry::opener_id`, tree-style
    /// tabs 7A.2) — adds the `.child` class + a `.tree-line` connector span
    /// (CC-8). The asset's CSS only defines indentation for a single nesting
    /// level (`.tab-row.child`), so a grandchild renders at the same indent
    /// as its parent's children rather than one level deeper — a known
    /// limitation of the frozen reference, not this binding.
    pub is_child: bool,
    /// Tree-mode placement (UX-CHROME-PANELS 14, `Ctrl+Shift+B`): `Some` when
    /// the sidebar lists tabs as a tree — indentation by depth and, for a
    /// tab with children, a collapse arrow. `None` in the flat list.
    pub tree: Option<ChromeTabTree>,
    /// `#RRGGBB` accent for the container strip (`.container-stripe`) — the
    /// caller derives this from `TabEntry::container.border_color()`, or
    /// `None` for `ContainerKind::None`, in which case the strip is omitted
    /// entirely (matching the asset: only container-scoped rows carry it).
    pub container_color: Option<String>,
    /// Tab-group (CC-6) membership, or `None` for an ungrouped tab.
    ///
    /// BUG-409: previously carried nowhere — `ChromeTabModel` had no group
    /// field at all, so a grouped tab rendered identically to an ungrouped
    /// one under the engine chrome.
    pub group: Option<ChromeTabGroup>,
}

/// Where a tab sits in the tree-style sidebar list (`TreeTabsPanel`, 7A.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeTabTree {
    /// Nesting depth: 0 for a root tab, one more per opener above it.
    pub depth: usize,
    /// `Some(expanded)` for a tab with children — renders the `.tab-arrow`
    /// button (▾ expanded / ▸ collapsed); `None` for a leaf.
    pub expanded: Option<bool>,
}

/// A tab's group membership, as rendered by `.group-stripe` (BUG-409).
///
/// Orthogonal to [`ChromeTabModel::container_color`]: a container isolates
/// cookies/storage, a group is a pure UI-organisation construct — a tab can
/// carry both a container stripe and a group stripe at once.
#[derive(Debug, Clone, PartialEq)]
pub struct ChromeTabGroup {
    /// `#RRGGBB` accent for `.group-stripe`, derived from `GroupColor::color()`
    /// the same way [`ChromeTabModel::container_color`] derives from
    /// `ContainerKind::border_color()`.
    pub color: String,
    /// Group label (`TabGroup::label`), written as the stripe's `title`
    /// tooltip — the sidebar row has no room for a full-text caption, so the
    /// name surfaces on hover rather than as always-visible text (mirrors
    /// `.tab-badge`'s `title="Гибернирована"` convention for the same reason).
    pub name: String,
    /// `true` when the group is collapsed (`TabGroup::collapsed`). A
    /// collapsed group's non-leftmost members are omitted from
    /// [`ChromeModel::tabs`] entirely by the caller (mirrors the legacy
    /// strip's `visible_indices()` behaviour) — this flag describes the one
    /// surviving row, so its stripe tooltip can hint that other tabs are
    /// folded behind it.
    pub collapsed: bool,
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
        set_attr(doc, body, "data-demo", model.control_panel.shape.attr_value());
    }
    bind_control_panel(doc, model);
    bind_profile_menu(doc, model);
    if let Some(sidebar) = doc.find_by_id(crate::ids::SIDEBAR) {
        set_class_token(doc, sidebar, "collapsed", model.sidebar_collapsed);
    }
    if let Some(container) = doc.find_by_id(crate::ids::SB_TABS) {
        rebuild_tab_list(doc, container, &model.tabs);
    }
    if let Some(container) = doc.find_by_id(crate::ids::HBAR_TABS) {
        rebuild_hbar_tab_list(doc, container, &model.tabs);
        apply_hbar_tab_freeze(doc, container, model.hbar_tab_width);
    }
    if let Some(container) = find_by_attr(doc, "data-testid", "workspace-switcher") {
        rebuild_workspace_list(doc, container, &model.workspaces);
    }
    if let Some(container) = find_by_class(doc, "hbar-ws") {
        rebuild_hbar_ws_list(doc, container, &model.workspaces);
    }
    if let Some(btn) = find_by_attr(doc, "data-action", "reload") {
        let loading = model.tabs.iter().any(|t| t.active && t.loading);
        set_class_token(doc, btn, "loading", loading);
        set_attr(doc, btn, "aria-label", if loading { "Остановить" } else { "Обновить" });
    }
    bind_omnibox(doc, &model.omnibox);
    bind_dropdown(doc, &model.dropdown);
    bind_find_bar(doc, &model.find);
    bind_downloads(doc, model.downloads_open, &model.downloads, model.downloads_clearable);
    bind_archive(doc, model.archive_open, &model.archive);
    bind_popover(doc, model);
    bind_palette(doc, &model.palette);
    bind_cert(doc, &model.cert);
    bind_shortcuts(doc, &model.shortcuts);
    bind_console(doc, &model.console);
    bind_network(doc, &model.network);
    bind_inspector(doc, &model.inspector);
    bind_read_later(doc, &model.read_later);
    bind_privacy(doc, &model.privacy);
    bind_focus(doc, &model.focus);
    bind_note(doc, &model.note);
    bind_tab_ctx(doc, &model.tab_ctx);
    bind_pip(doc, &model.pip);
    bind_page_ctx(doc, &model.page_ctx);
    bind_workspace_bar(doc, model.workspace_bar_open, &model.workspaces);
    bind_access(doc, &model.access);
    bind_print(doc, &model.print);
    bind_content_view(doc, model.content_view);
    bind_history(doc, &model.history);
    bind_bookmarks(doc, &model.bookmarks);
    bind_settings(doc, &model.settings);
    bind_right_sidebar(doc, &model.right_sidebar);
    update_bar::bind_update(doc, &model.update);
    login_bar::bind_login_offer(doc, &model.login_offer);
    media_bar::bind_media_bar(doc, &model.media_bar);
    hang_bar::bind_hang_bar(doc, &model.hang_bar);
    dialog::bind_dialog(doc, &model.dialog);
    security::bind_security(doc, &model.security);
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

/// Syncs `#demoBar` (CC-18): `mini-open`/`#infoPanel`'s `open`, plus `.active`
/// on the matching `#demoSwitch` shape button and on whichever profile/layout
/// button inside its `.demo-group`s matches the model's current values.
/// `body[data-demo]`/`[data-profile]`/`[data-layout]` themselves are set by
/// the caller (`bind_model`) — this only handles the panel's own buttons.
fn bind_control_panel(doc: &mut Document, model: &ChromeModel) {
    if let Some(bar) = doc.find_by_id(crate::ids::DEMO_BAR) {
        set_class_token(doc, bar, "mini-open", model.control_panel.mini_open);
        for node in collect_by_attr(doc, bar, "data-demo-variant") {
            let active = doc.get(node).get_attr("data-demo-variant")
                == Some(model.control_panel.shape.attr_value());
            set_class_token(doc, node, "active", active);
        }
        for node in collect_by_attr(doc, bar, "data-profile") {
            let active = doc.get(node).get_attr("data-profile") == model.profile_slug.as_deref();
            set_class_token(doc, node, "active", active);
        }
        let layout = if model.layout_vertical { "vertical" } else { "horizontal" };
        for node in collect_by_attr(doc, bar, "data-layout") {
            let active = doc.get(node).get_attr("data-layout") == Some(layout);
            set_class_token(doc, node, "active", active);
        }
    }
    if let Some(info) = doc.find_by_id(crate::ids::INFO_PANEL) {
        set_class_token(doc, info, "open", model.control_panel.info_open);
    }
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
    // BUG-422: the row itself opens the entry; the `.hist-actions` buttons
    // appended below carry their own `data-action` and shadow it, because
    // `Lumen::chrome_action_at` walks the hit path closest-node-first.
    set_attr(doc, item, "data-action", "open-history-entry");
    set_attr(doc, item, "data-hist-url", url);

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

    // BUG-422: the design's hover-revealed action strip. Rebuilt here rather
    // than left to the static asset markup because `bind_history` deletes
    // and recreates every `.hist-item`; `aria-label` mirrors what
    // `gen_chrome_assets.py`'s `ARIA_LABEL_RULES` injects into the asset's
    // own copies of these buttons, so the a11y tree matches either way.
    let actions = doc.create_element(QualName::html("div"));
    set_attr(doc, actions, "class", "hist-actions");
    for (action, icon, label) in [
        ("bookmark-history-entry", "#i-star", "Добавить в закладки"),
        ("copy-history-entry", "#i-copy", "Копировать ссылку"),
        ("delete-history-entry", "#i-trash", "Удалить из истории"),
    ] {
        let btn = doc.create_element(QualName::html("button"));
        set_attr(doc, btn, "class", "tb-btn");
        set_attr(doc, btn, "data-action", action);
        set_attr(doc, btn, "data-hist-url", url);
        set_attr(doc, btn, "data-hist-title", title);
        set_attr(doc, btn, "aria-label", label);
        append_icon(doc, btn, icon);
        attach_child(doc, actions, btn);
    }
    attach_child(doc, item, actions);

    item
}

/// Appends the asset's `<svg class="icon"><use href="#…"/></svg>` sprite
/// reference to `parent`.
///
/// BUG-422: the history/bookmark row actions are the first list rebuilt here
/// that needs a real icon rather than the single-letter fallback CC-6 chose
/// for favicons — these buttons have no text at all, so a letter would be
/// meaningless. The sprite `<symbol>`s live in the asset itself, so this only
/// has to construct the two-element reference; both elements go in the SVG
/// namespace, matching what the parser produces for the asset's own copies.
fn append_icon(doc: &mut Document, parent: NodeId, symbol_href: &str) {
    let svg = doc.create_element(QualName { namespace: Namespace::Svg, local: "svg".to_owned() });
    set_attr(doc, svg, "class", "icon");
    let use_el = doc.create_element(QualName { namespace: Namespace::Svg, local: "use".to_owned() });
    set_attr(doc, use_el, "href", symbol_href);
    attach_child(doc, svg, use_el);
    attach_child(doc, parent, svg);
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
            // BUG-422: `""` is the "all folders" row — an absent attribute
            // would be indistinguishable from a stale asset row.
            set_attr(doc, node, "data-action", "select-folder");
            set_attr(doc, node, "data-bm-folder", folder.filter.as_deref().unwrap_or(""));
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
    // BUG-422: same row-opens / button-shadows-it shape as `build_hist_item`.
    set_attr(doc, node, "data-action", "open-bookmark");
    set_attr(doc, node, "data-bm-url", &card.url);

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

    let actions = doc.create_element(QualName::html("div"));
    set_attr(doc, actions, "class", "bm-actions");
    let del = doc.create_element(QualName::html("button"));
    set_attr(doc, del, "class", "tb-btn");
    set_attr(doc, del, "data-action", "delete-bookmark");
    set_attr(doc, del, "data-bm-url", &card.url);
    set_attr(doc, del, "aria-label", "Удалить закладку");
    append_icon(doc, del, "#i-trash");
    attach_child(doc, actions, del);
    attach_child(doc, node, actions);

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
    bind_passwords(doc, settings);
    bind_autofill(doc, settings);
    bind_site_permissions(doc, settings);
    bind_site_data(doc, settings);
    bind_spelling(doc, settings);
    bind_translate(doc, settings);
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

/// Перестраивает `#pwList` из [`ChromeSettingsModel::passwords`] и меняет
/// `#pwDesc`, если хранилище недоступно (UX-PASSWORDS, срез 5).
fn bind_passwords(doc: &mut Document, settings: &ChromeSettingsModel) {
    if let Some(desc) = doc.find_by_id(crate::ids::PW_DESC) {
        let text = if !settings.passwords_available {
            "Менеджер паролей недоступен в приватном или анонимном профиле."
        } else if settings.passwords.is_empty() {
            "Сохранённых паролей нет. Они появятся после входа на сайт и согласия на сохранение."
        } else {
            "Пароли хранятся зашифрованными ключом профиля. Здесь виден только список аккаунтов."
        };
        set_text(doc, desc, text);
    }
    let Some(list) = doc.find_by_id(crate::ids::PW_LIST) else { return };
    remove_children_with_class(doc, list, "pw-row");
    for (origin, username) in &settings.passwords {
        let row = doc.create_element(QualName::html("div"));
        set_attr(doc, row, "class", "set-row pw-row");
        let text = doc.create_element(QualName::html("div"));
        set_attr(doc, text, "class", "st-text");
        let title = doc.create_element(QualName::html("div"));
        set_attr(doc, title, "class", "st-title");
        append_text(doc, title, origin);
        attach_child(doc, text, title);
        let desc = doc.create_element(QualName::html("div"));
        set_attr(doc, desc, "class", "st-desc");
        append_text(doc, desc, if username.is_empty() { "(без логина)" } else { username });
        attach_child(doc, text, desc);
        attach_child(doc, row, text);
        let del = doc.create_element(QualName::html("button"));
        set_attr(doc, del, "class", "upd-check");
        set_attr(doc, del, "data-action", "delete-saved-login");
        set_attr(doc, del, "data-login-origin", origin);
        set_attr(doc, del, "data-login-user", username);
        set_attr(doc, del, "aria-label", "Удалить пароль");
        append_text(doc, del, "Удалить");
        attach_child(doc, row, del);
        attach_child(doc, list, row);
    }
}

/// Перестраивает `#permList` из [`ChromeSettingsModel::site_permissions`]
/// (UX-PERMISSIONS): строка на решение, кнопка «Отозвать».
fn bind_site_permissions(doc: &mut Document, settings: &ChromeSettingsModel) {
    if let Some(desc) = doc.find_by_id(crate::ids::PERM_DESC) {
        let text = if settings.site_permissions.is_empty() {
            "Сохранённых решений нет. Они появятся после того, как вы разрешите или запретите доступ сайту."
        } else {
            "Решения сохраняются между сессиями. Отзыв возвращает сайту запрос при следующем обращении."
        };
        set_text(doc, desc, text);
    }
    let Some(list) = doc.find_by_id(crate::ids::PERM_LIST) else { return };
    remove_children_with_class(doc, list, "perm-row");
    for (origin, kind, allowed) in &settings.site_permissions {
        let row = doc.create_element(QualName::html("div"));
        set_attr(doc, row, "class", "set-row perm-row");
        let text = doc.create_element(QualName::html("div"));
        set_attr(doc, text, "class", "st-text");
        let title = doc.create_element(QualName::html("div"));
        set_attr(doc, title, "class", "st-title");
        append_text(doc, title, origin);
        attach_child(doc, text, title);
        let desc = doc.create_element(QualName::html("div"));
        set_attr(doc, desc, "class", "st-desc");
        append_text(doc, desc, &format!("{kind} · {}", if *allowed { "разрешено" } else { "запрещено" }));
        attach_child(doc, text, desc);
        attach_child(doc, row, text);
        let del = doc.create_element(QualName::html("button"));
        set_attr(doc, del, "class", "upd-check");
        set_attr(doc, del, "data-action", "revoke-site-permission");
        set_attr(doc, del, "data-perm-origin", origin);
        set_attr(doc, del, "data-perm-kind", kind);
        set_attr(doc, del, "aria-label", "Отозвать");
        append_text(doc, del, "Отозвать");
        attach_child(doc, row, del);
        attach_child(doc, list, row);
    }
}

/// Раздел «Орфография»: выключатель и строка на язык словаря (UX-SPELLCHECK).
fn bind_spelling(doc: &mut Document, settings: &ChromeSettingsModel) {
    if let Some(toggle) = doc.find_by_id(crate::ids::SPELL_TOGGLE) {
        set_class_token(doc, toggle, "on", settings.spell_enabled);
    }
    if let Some(desc) = doc.find_by_id(crate::ids::SPELL_DESC) {
        let text = if settings.spell_languages.is_empty() {
            "Словари не загружены: положите пары <язык>.aff и <язык>.dic в папку data/spell и перезапустите браузер."
        } else {
            "Подчёркивает ошибки в полях ввода; слова проверяются локально, текст никуда не отправляется."
        };
        set_text(doc, desc, text);
    }
    let Some(list) = doc.find_by_id(crate::ids::SPELL_LANG_LIST) else { return };
    remove_children_with_class(doc, list, "spell-lang-row");
    for (stem, on) in &settings.spell_languages {
        let row = doc.create_element(QualName::html("div"));
        set_attr(doc, row, "class", "set-row spell-lang-row");
        let text = doc.create_element(QualName::html("div"));
        set_attr(doc, text, "class", "st-text");
        let title = doc.create_element(QualName::html("div"));
        set_attr(doc, title, "class", "st-title");
        append_text(doc, title, stem);
        attach_child(doc, text, title);
        attach_child(doc, row, text);
        let toggle = doc.create_element(QualName::html("div"));
        set_attr(doc, toggle, "class", if *on { "toggle on" } else { "toggle" });
        set_attr(doc, toggle, "data-action", "toggle-spell-language");
        set_attr(doc, toggle, "data-spell-lang", stem);
        let thumb = doc.create_element(QualName::html("div"));
        set_attr(doc, thumb, "class", "thumb");
        attach_child(doc, toggle, thumb);
        attach_child(doc, row, toggle);
        attach_child(doc, list, row);
    }
}

/// Раздел «Перевод»: строка на язык перевода (UX-TRANSLATE) и название модели.
fn bind_translate(doc: &mut Document, settings: &ChromeSettingsModel) {
    if settings.translate_languages.is_empty() {
        return;
    }
    if let Some(model) = doc.find_by_id(crate::ids::TRANSLATE_MODEL) {
        set_text(doc, model, &settings.translate_model);
    }
    let Some(list) = doc.find_by_id(crate::ids::TRANSLATE_LANG_LIST) else { return };
    remove_children_with_class(doc, list, "translate-lang-row");
    for (code, label, on) in &settings.translate_languages {
        let row = doc.create_element(QualName::html("div"));
        set_attr(doc, row, "class", "set-row translate-lang-row");
        let text = doc.create_element(QualName::html("div"));
        set_attr(doc, text, "class", "st-text");
        let title = doc.create_element(QualName::html("div"));
        set_attr(doc, title, "class", "st-title");
        append_text(doc, title, label);
        attach_child(doc, text, title);
        attach_child(doc, row, text);
        let toggle = doc.create_element(QualName::html("div"));
        set_attr(doc, toggle, "class", if *on { "toggle on" } else { "toggle" });
        set_attr(doc, toggle, "data-action", "set-translate-language");
        set_attr(doc, toggle, "data-lang", code);
        let thumb = doc.create_element(QualName::html("div"));
        set_attr(doc, thumb, "class", "thumb");
        attach_child(doc, toggle, thumb);
        attach_child(doc, row, toggle);
        attach_child(doc, list, row);
    }
}

/// Блок «Данные сайта»: имя сайта и `data-site` кнопки очистки (UX-PARTITION).
fn bind_site_data(doc: &mut Document, settings: &ChromeSettingsModel) {
    let site = settings.current_site.as_deref().unwrap_or("");
    if let Some(name) = doc.find_by_id(crate::ids::SITE_DATA_SITE) {
        set_text(doc, name, if site.is_empty() { "Нет открытого сайта" } else { site });
    }
    if let Some(btn) = doc.find_by_id(crate::ids::SITE_DATA_BTN) {
        set_attr(doc, btn, "data-site", site);
    }
}

/// Перестраивает `#afList` из [`ChromeSettingsModel::autofill`] и
/// [`ChromeSettingsModel::cards`] (UX-AUTOFILL, срез 5).
fn bind_autofill(doc: &mut Document, settings: &ChromeSettingsModel) {
    if let Some(desc) = doc.find_by_id(crate::ids::AF_DESC) {
        let text = if !settings.autofill_available {
            "Автозаполнение недоступно в приватном или анонимном профиле."
        } else if settings.autofill.is_empty() && settings.cards.is_empty() {
            "Сохранённых данных форм нет. Они появятся после отправки формы и согласия на сохранение."
        } else {
            "Значения форм хранятся открытым текстом, карты — зашифрованными; код проверки карты не сохраняется."
        };
        set_text(doc, desc, text);
    }
    let Some(list) = doc.find_by_id(crate::ids::AF_LIST) else { return };
    remove_children_with_class(doc, list, "af-row");
    type Row<'a> = (String, String, Vec<(&'a str, &'a str)>);
    let mut rows: Vec<Row<'_>> = Vec::new();
    for (last4, label) in &settings.cards {
        rows.push((label.clone(), "Платёжная карта".to_owned(), vec![("data-action", "delete-saved-card"), ("data-card-last4", last4)]));
    }
    for (origin, field, value) in &settings.autofill {
        rows.push((
            value.clone(),
            format!("{origin} · {field}"),
            vec![
                ("data-action", "delete-autofill-entry"),
                ("data-af-origin", origin),
                ("data-af-field", field),
                ("data-af-value", value),
            ],
        ));
    }
    for (title_text, desc_text, attrs) in rows {
        let row = doc.create_element(QualName::html("div"));
        set_attr(doc, row, "class", "set-row af-row");
        let text = doc.create_element(QualName::html("div"));
        set_attr(doc, text, "class", "st-text");
        let title = doc.create_element(QualName::html("div"));
        set_attr(doc, title, "class", "st-title");
        append_text(doc, title, &title_text);
        attach_child(doc, text, title);
        let desc = doc.create_element(QualName::html("div"));
        set_attr(doc, desc, "class", "st-desc");
        append_text(doc, desc, &desc_text);
        attach_child(doc, text, desc);
        attach_child(doc, row, text);
        let del = doc.create_element(QualName::html("button"));
        set_attr(doc, del, "class", "upd-check");
        for (k, v) in attrs {
            set_attr(doc, del, k, v);
        }
        set_attr(doc, del, "aria-label", "Удалить");
        append_text(doc, del, "Удалить");
        attach_child(doc, row, del);
        attach_child(doc, list, row);
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
    set_attr(doc, row, "data-cp-index", &r.index.to_string());
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

/// Toggles `#devtools`'s `.open` class, writes the message count into the
/// tab title and rebuilds the `.console-line`s of `#dt-console` from
/// [`ChromeConsoleModel::lines`] (UX-CHROME-PANELS).
fn bind_console(doc: &mut Document, console: &ChromeConsoleModel) {
    let Some(panel) = doc.find_by_id(crate::ids::DEVTOOLS) else { return };
    set_class_token(doc, panel, "open", console.open);
    if let Some(title) = doc.find_by_id(crate::ids::DT_CONSOLE_TITLE) {
        set_text(doc, title, &format!("Console ({})", console.total));
    }
    let Some(list) = doc.find_by_id(crate::ids::DT_CONSOLE) else { return };
    remove_children_with_class(doc, list, "console-line");
    for line in &console.lines {
        let node = doc.create_element(QualName::html("div"));
        let class = match line.level {
            1 => "console-line warn",
            2 => "console-line error",
            _ => "console-line",
        };
        set_attr(doc, node, "class", class);
        let text = doc.create_element(QualName::html("span"));
        append_text(doc, text, &line.text);
        attach_child(doc, node, text);
        attach_child(doc, list, node);
    }
}

/// Toggles `#networkPanel`'s `.open` class, writes the request count into the
/// tab title and rebuilds the `.net-row`s of `#dt-network` from
/// [`ChromeNetworkModel::rows`] (UX-CHROME-PANELS).
fn bind_network(doc: &mut Document, network: &ChromeNetworkModel) {
    let Some(panel) = doc.find_by_id(crate::ids::NETWORK_PANEL) else { return };
    set_class_token(doc, panel, "open", network.open);
    if let Some(title) = doc.find_by_id(crate::ids::DT_NETWORK_TITLE) {
        set_text(doc, title, &format!("Network ({})", network.total));
    }
    let Some(list) = doc.find_by_id(crate::ids::DT_NETWORK) else { return };
    bind_net_rows(doc, list, &network.rows);
}

/// Rebuilds the `.net-row`s of `list` from `rows` (shared by `#dt-network` and
/// the inspector's Network tab).
fn bind_net_rows(doc: &mut Document, list: NodeId, rows: &[ChromeNetworkRowModel]) {
    remove_children_with_class(doc, list, "net-row");
    for row in rows {
        let node = doc.create_element(QualName::html("div"));
        let class = if row.blocked {
            "net-row blocked"
        } else if row.failed {
            "net-row failed"
        } else {
            "net-row"
        };
        set_attr(doc, node, "class", class);
        let status_class = match row.status_class {
            2 => "net-status status-badge s2",
            3 => "net-status status-badge s3",
            4 => "net-status status-badge s4",
            _ => "net-status status-badge",
        };
        for (class, text) in [
            ("net-method", &row.method),
            (status_class, &row.status),
            ("net-time", &row.time),
            ("net-url", &row.url),
        ] {
            let cell = doc.create_element(QualName::html("span"));
            set_attr(doc, cell, "class", class);
            append_text(doc, cell, text);
            attach_child(doc, node, cell);
        }
        attach_child(doc, list, node);
    }
}

/// Toggles `#inspPanel`'s `.open` class, marks the active tab button, writes
/// the pinned element's label / `NodeId` / placeholder / scroll indicator and
/// rebuilds the `.insp-row`s of `#inspRows` and the `.net-row`s of `#inspNet`
/// (UX-CHROME-PANELS).
fn bind_inspector(doc: &mut Document, insp: &ChromeInspectorModel) {
    let Some(panel) = doc.find_by_id(crate::ids::INSP_PANEL) else { return };
    set_class_token(doc, panel, "open", insp.open);
    for (i, id) in [
        crate::ids::INSP_TAB_ELEMENTS,
        crate::ids::INSP_TAB_STYLES,
        crate::ids::INSP_TAB_COMPUTED,
        crate::ids::INSP_TAB_NETWORK,
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(tab) = doc.find_by_id(id) {
            set_class_token(doc, tab, "active", usize::from(insp.tab) == i);
        }
    }
    for (id, value) in [
        (crate::ids::INSP_LABEL, &insp.label),
        (crate::ids::INSP_NODE, &insp.node),
        (crate::ids::INSP_HINT, &insp.hint),
        (crate::ids::INSP_POS, &insp.position),
    ] {
        if let Some(n) = doc.find_by_id(id) {
            set_text(doc, n, value);
        }
    }
    if let Some(list) = doc.find_by_id(crate::ids::INSP_NET) {
        bind_net_rows(doc, list, &insp.net_rows);
    }
    let Some(list) = doc.find_by_id(crate::ids::INSP_ROWS) else { return };
    remove_children_with_class(doc, list, "insp-row");
    for row in &insp.rows {
        let node = doc.create_element(QualName::html("div"));
        let class = match row.kind {
            1 => "insp-row insp-sel",
            2 => "insp-row insp-sep",
            _ => "insp-row",
        };
        set_attr(doc, node, "class", class);
        if row.kind != 2 {
            let key = doc.create_element(QualName::html("span"));
            set_attr(doc, key, "class", "insp-key");
            let key_text = if row.kind == 0 { format!("{}:", row.key) } else { row.key.clone() };
            append_text(doc, key, &key_text);
            attach_child(doc, node, key);
            if row.kind == 0 {
                let val = doc.create_element(QualName::html("span"));
                set_attr(doc, val, "class", "insp-val");
                append_text(doc, val, &row.value);
                attach_child(doc, node, val);
            }
        }
        attach_child(doc, list, node);
    }
}


/// Toggles `#focusTimer`'s `.open` class, writes the countdown into
/// `#focusTime`, swaps the play/pause icon and shows `#focusDone` once the
/// session is complete (UX-CHROME-PANELS). Only the `.open`/`.show` classes
/// are touched: the design's `body.focus-mode` also hides the sidebar and
/// toolbar, which would desync the shell's page viewport.
fn bind_focus(doc: &mut Document, focus: &ChromeFocusModel) {
    let Some(pill) = doc.find_by_id(crate::ids::FOCUS_TIMER) else { return };
    set_class_token(doc, pill, "open", focus.active);
    if let Some(n) = doc.find_by_id(crate::ids::FOCUS_TIME) {
        let label = if focus.active { focus.label.as_str() } else { "25:00" };
        set_text(doc, n, label);
    }
    if let Some(icon) = doc.find_by_id(crate::ids::FOCUS_TOGGLE_ICON) {
        set_attr(doc, icon, "href", if focus.running { "#i-pause" } else { "#i-play" });
    }
    if let Some(done) = doc.find_by_id(crate::ids::FOCUS_DONE) {
        set_class_token(doc, done, "show", focus.active && focus.finished);
    }
}

/// Toggles `#pipCard`'s `.open`/`.playing` classes, places it at the card's
/// position and writes the title and poster (UX-CHROME-PANELS).
fn bind_pip(doc: &mut Document, pip: &ChromePipModel) {
    let Some(root) = doc.find_by_id(crate::ids::PIP_CARD) else { return };
    set_class_token(doc, root, "open", pip.open);
    if !pip.open {
        return;
    }
    set_class_token(doc, root, "playing", pip.playing);
    set_attr(doc, root, "style", &format!("left:{}px;top:{}px", pip.x, pip.y));
    if let Some(n) = doc.find_by_id(crate::ids::PIP_TITLE) {
        let title = if pip.title.is_empty() { "Picture-in-Picture" } else { pip.title.as_str() };
        set_text(doc, n, title);
    }
    if let Some(n) = doc.find_by_id(crate::ids::PIP_POSTER) {
        set_class_token(doc, n, "empty", pip.poster.is_empty());
        if !pip.poster.is_empty() {
            set_attr(doc, n, "src", &pip.poster);
        }
    }
}

/// Toggles `#tabCtxMenu`'s `.open` class, places it at the clamped anchor and
/// flips the Pin/Collapse row wording (UX-CHROME-PANELS).
fn bind_tab_ctx(doc: &mut Document, menu: &ChromeTabCtxModel) {
    let Some(root) = doc.find_by_id(crate::ids::TAB_CTX_MENU) else { return };
    set_class_token(doc, root, "open", menu.open);
    if !menu.open {
        return;
    }
    set_attr(doc, root, "style", &format!("left:{}px;top:{}px", menu.x, menu.y));
    if let Some(n) = doc.find_by_id(crate::ids::TAB_CTX_PIN) {
        set_text(doc, n, if menu.pinned { "Открепить" } else { "Закрепить" });
    }
    if let Some(n) = doc.find_by_id(crate::ids::TAB_CTX_TOGGLE_GROUP) {
        set_text(doc, n, if menu.collapsed { "Развернуть группу" } else { "Свернуть группу" });
    }
}

/// Binds the «Сайт работает не так?» section of `#permPopover` (COMPAT-PANEL).
fn bind_fix_panel(doc: &mut Document, fix: &ChromeFixPanelModel) {
    let asking = fix.asking.is_some();
    if let Some(steps) = doc.find_by_id(crate::ids::FIX_STEPS) {
        set_class_token(doc, steps, "hide", !fix.steps_enabled || fix.confirm_reset || asking);
    }
    for (id, tried) in [
        (crate::ids::FIX_NO_CACHE, fix.tried[0]),
        (crate::ids::FIX_RESET, fix.tried[1]),
        (crate::ids::FIX_HANDOFF, fix.tried[2]),
    ] {
        if let Some(btn) = doc.find_by_id(id) {
            set_class_token(doc, btn, "tried", tried);
        }
    }
    if let Some(confirm) = doc.find_by_id(crate::ids::FIX_CONFIRM) {
        set_class_token(doc, confirm, "show", fix.confirm_reset && !asking);
    }
    if let Some(ask) = doc.find_by_id(crate::ids::FIX_ASK) {
        set_class_token(doc, ask, "show", asking);
    }
    if let (Some(text), Some(question)) = (doc.find_by_id(crate::ids::FIX_ASK_TEXT), &fix.asking) {
        set_text(doc, text, question);
    }
    if let Some(note) = doc.find_by_id(crate::ids::FIX_NOTE) {
        set_text(doc, note, &fix.note);
    }
}

/// Toggles `#pageCtxMenu`'s `.open` class, places it and rebuilds its rows:
/// one `use:<i>` button per suggestion, then optionally «Добавить в словарь» /
/// «Пропустить» behind a separator and the model's `extra_rows` behind another
/// (UX-CHROME-PANELS).
fn bind_page_ctx(doc: &mut Document, menu: &ChromePageCtxModel) {
    let Some(root) = doc.find_by_id(crate::ids::PAGE_CTX_MENU) else { return };
    set_class_token(doc, root, "open", menu.open);
    remove_children_with_class(doc, root, "page-ctx-row");
    if !menu.open {
        return;
    }
    set_attr(doc, root, "style", &format!("left:{}px;top:{}px", menu.x, menu.y));
    let mut rows: Vec<(String, String, bool)> = menu
        .suggestions
        .iter()
        .enumerate()
        .map(|(i, s)| (format!("use:{i}"), s.clone(), false))
        .collect();
    if menu.spell_footer {
        let sep = !rows.is_empty();
        rows.push(("add".to_owned(), "Добавить в словарь".to_owned(), sep));
        rows.push(("ignore".to_owned(), "Пропустить".to_owned(), false));
    }
    for (i, (action, label)) in menu.extra_rows.iter().enumerate() {
        rows.push((action.clone(), label.clone(), i == 0 && !rows.is_empty()));
    }
    for (action, label, sep) in rows {
        let btn = doc.create_element(QualName::html("button"));
        set_attr(doc, btn, "class", if sep { "page-ctx-row sep" } else { "page-ctx-row" });
        set_attr(doc, btn, "data-page-ctx", &action);
        append_text(doc, btn, &label);
        attach_child(doc, root, btn);
    }
}

/// Toggles `#noteOverlay`'s `.open` class and writes the note's URL, quoted
/// selection and comment into the modal body (UX-CHROME-PANELS).
fn bind_note(doc: &mut Document, note: &ChromeNoteModel) {
    let Some(overlay) = doc.find_by_id(crate::ids::NOTE_OVERLAY) else { return };
    set_class_token(doc, overlay, "open", note.open);
    for (id, value) in [
        (crate::ids::NOTE_URL, &note.url),
        (crate::ids::NOTE_SEL, &note.selection),
        (crate::ids::NOTE_COMMENT, &note.comment),
    ] {
        if let Some(n) = doc.find_by_id(id) {
            set_text(doc, n, value);
        }
    }
}

/// Toggles `#workspaceBar`'s `.open` class and rebuilds the `.wsb-chip`s of
/// `#wsbChips` from `workspaces` (UX-CHROME-PANELS).
fn bind_workspace_bar(doc: &mut Document, open: bool, workspaces: &[ChromeWorkspaceModel]) {
    let Some(bar) = doc.find_by_id(crate::ids::WORKSPACE_BAR) else { return };
    set_class_token(doc, bar, "open", open);
    let Some(list) = doc.find_by_id(crate::ids::WSB_CHIPS) else { return };
    // Reconciled in place (not cleared and rebuilt): `bind_model` runs on every
    // chrome relayout, and fresh node ids would dirty a hidden subtree on every
    // keystroke (BUG-341).
    reconcile_row_list(doc, list, "wsb-chip", workspaces, build_wsb_chip, update_wsb_chip, None);
}

fn apply_wsb_chip_attrs(doc: &mut Document, chip: NodeId, ws: &ChromeWorkspaceModel) {
    set_attr(doc, chip, "class", if ws.active { "wsb-chip active" } else { "wsb-chip" });
    set_attr(doc, chip, "style", &format!("--ws-color:{}", ws.color));
}

/// Both buttons carry their own `data-ws-id`: `chrome_action_at` only sees the
/// `data-action`-carrying node.
fn build_wsb_chip(doc: &mut Document, ws: &ChromeWorkspaceModel) -> NodeId {
    let chip = doc.create_element(QualName::html("div"));
    apply_wsb_chip_attrs(doc, chip, ws);
    let name = doc.create_element(QualName::html("button"));
    set_attr(doc, name, "class", "wsb-name");
    set_attr(doc, name, "data-action", "select-workspace");
    set_attr(doc, name, "data-ws-id", &ws.id.to_string());
    append_text(doc, name, &ws.name);
    attach_child(doc, chip, name);
    let del = doc.create_element(QualName::html("button"));
    set_attr(doc, del, "class", "wsb-del");
    set_attr(doc, del, "data-action", "delete-workspace");
    set_attr(doc, del, "data-ws-id", &ws.id.to_string());
    set_attr(doc, del, "aria-label", "Удалить пространство");
    append_text(doc, del, "×");
    attach_child(doc, chip, del);
    chip
}

fn update_wsb_chip(doc: &mut Document, chip: NodeId, ws: &ChromeWorkspaceModel) {
    let children: Vec<NodeId> = doc.get(chip).children.clone();
    let (Some(&name), Some(&del)) = (children.first(), children.get(1)) else {
        for child in children {
            detach_node(doc, child);
        }
        let fresh = build_wsb_chip(doc, ws);
        let kids: Vec<NodeId> = doc.get(fresh).children.clone();
        for k in kids {
            attach_child(doc, chip, k);
        }
        apply_wsb_chip_attrs(doc, chip, ws);
        return;
    };
    apply_wsb_chip_attrs(doc, chip, ws);
    let id = ws.id.to_string();
    set_attr(doc, name, "data-ws-id", &id);
    set_text(doc, name, &ws.name);
    set_attr(doc, del, "data-ws-id", &id);
}

/// Toggles `#privacyPanel`'s `.open` class, writes the blocked/allowed tallies
/// and the scroll indicator, and rebuilds the `.pv-row`s of `#pvList` from
/// [`ChromePrivacyModel::rows`] (UX-CHROME-PANELS).
fn bind_privacy(doc: &mut Document, pv: &ChromePrivacyModel) {
    let Some(panel) = doc.find_by_id(crate::ids::PRIVACY_PANEL) else { return };
    set_class_token(doc, panel, "open", pv.open);
    if let Some(n) = doc.find_by_id(crate::ids::PV_BLOCKED) {
        set_text(doc, n, &format!("{} blocked", pv.blocked));
    }
    if let Some(n) = doc.find_by_id(crate::ids::PV_ALLOWED) {
        set_text(doc, n, &format!("{} allowed", pv.allowed));
    }
    if let Some(n) = doc.find_by_id(crate::ids::PV_POSITION) {
        set_text(doc, n, &pv.position);
    }
    let Some(list) = doc.find_by_id(crate::ids::PV_LIST) else { return };
    remove_children_with_class(doc, list, "pv-row");
    for row in &pv.rows {
        let node = doc.create_element(QualName::html("div"));
        set_attr(doc, node, "class", &format!("pv-row {}", row.state));
        let dot = doc.create_element(QualName::html("span"));
        set_attr(doc, dot, "class", "pv-dot");
        attach_child(doc, node, dot);
        let text = doc.create_element(QualName::html("div"));
        set_attr(doc, text, "class", "pv-text");
        for (class, value) in [("pv-host", &row.host), ("pv-sub", &row.sub)] {
            let line = doc.create_element(QualName::html("div"));
            set_attr(doc, line, "class", class);
            append_text(doc, line, value);
            attach_child(doc, text, line);
        }
        attach_child(doc, node, text);
        attach_child(doc, list, node);
    }
}

/// Toggles `#readLaterPanel`'s `.open` class, writes the saved-page count into
/// the header and rebuilds the `.rl-row`s of `#rlList` from
/// [`ChromeReadLaterModel::rows`] (UX-CHROME-PANELS).
fn bind_read_later(doc: &mut Document, rl: &ChromeReadLaterModel) {
    let Some(panel) = doc.find_by_id(crate::ids::READ_LATER_PANEL) else { return };
    set_class_token(doc, panel, "open", rl.open);
    if let Some(title) = doc.find_by_id(crate::ids::RL_TITLE) {
        let text = if rl.total == 0 { "Read Later".to_owned() } else { format!("Read Later ({})", rl.total) };
        set_text(doc, title, &text);
    }
    let Some(list) = doc.find_by_id(crate::ids::RL_LIST) else { return };
    remove_children_with_class(doc, list, "rl-row");
    for row in &rl.rows {
        let node = doc.create_element(QualName::html("div"));
        set_attr(doc, node, "class", "rl-row");
        let text = doc.create_element(QualName::html("div"));
        set_attr(doc, text, "class", "rl-text");
        // The text block opens the page; it carries the id itself because
        // `chrome_action_at` only sees the `data-action`-carrying node.
        set_attr(doc, text, "data-action", "read-later-open");
        set_attr(doc, text, "data-read-later-id", &row.id.to_string());
        for (class, value) in [("rl-title", &row.title), ("rl-meta", &row.meta)] {
            let line = doc.create_element(QualName::html("div"));
            set_attr(doc, line, "class", class);
            append_text(doc, line, value);
            attach_child(doc, text, line);
        }
        attach_child(doc, node, text);
        let badge = doc.create_element(QualName::html("span"));
        set_attr(doc, badge, "class", if row.read { "rl-badge read" } else { "rl-badge" });
        append_text(doc, badge, &row.status);
        attach_child(doc, node, badge);
        let del = doc.create_element(QualName::html("button"));
        set_attr(doc, del, "class", "rl-delete");
        set_attr(doc, del, "data-action", "read-later-delete");
        set_attr(doc, del, "aria-label", "Удалить");
        set_attr(doc, del, "data-read-later-id", &row.id.to_string());
        append_text(doc, del, "×");
        attach_child(doc, node, del);
        attach_child(doc, list, node);
    }
}

/// Toggles `#shortcutsOverlay`'s `.open` class and rebuilds the `.sc-row`s of
/// `#scList` from [`ChromeShortcutsModel::rows`] (UX-CHROME-PANELS).
fn bind_shortcuts(doc: &mut Document, shortcuts: &ChromeShortcutsModel) {
    let Some(overlay) = doc.find_by_id(crate::ids::SHORTCUTS_OVERLAY) else { return };
    set_class_token(doc, overlay, "open", shortcuts.open);
    let Some(list) = doc.find_by_id(crate::ids::SC_LIST) else { return };
    remove_children_with_class(doc, list, "sc-row");
    for row in &shortcuts.rows {
        let node = doc.create_element(QualName::html("div"));
        set_attr(doc, node, "class", if row.rebinding { "sc-row rebinding" } else { "sc-row" });
        set_attr(doc, node, "data-action", "rebind-shortcut");
        set_attr(doc, node, "data-sc-index", &row.index.to_string());
        let label = doc.create_element(QualName::html("span"));
        set_attr(doc, label, "class", "sc-label");
        append_text(doc, label, &row.label);
        attach_child(doc, node, label);
        let key = doc.create_element(QualName::html("span"));
        set_attr(doc, key, "class", "sc-key");
        append_text(doc, key, &row.binding);
        attach_child(doc, node, key);
        attach_child(doc, list, node);
    }
}

/// Toggles `.open` on both profile dropdowns (`#profileMenu` in the vertical
/// sidebar, `#profileMenuH` in the horizontal bar) and marks the item of the
/// active profile `.active` (UX-CHROME-PANELS).
fn bind_profile_menu(doc: &mut Document, model: &ChromeModel) {
    for id in [crate::ids::PROFILE_MENU, crate::ids::PROFILE_MENU_H] {
        let Some(menu) = doc.find_by_id(id) else { continue };
        set_class_token(doc, menu, "open", model.profile_menu_open);
        for item in find_descendants_by_class(doc, menu, "pm-item") {
            let on = doc.get(item).get_attr("data-profile") == model.profile_slug.as_deref();
            set_class_token(doc, item, "active", on);
        }
    }
}

/// Toggles `#accessOverlay`'s `.open` class, moves `.active` between the pills
/// of `#acFontPills`/`#acCursorPills` and flips the two `.toggle`s' `.on`
/// (UX-CHROME-PANELS).
fn bind_access(doc: &mut Document, access: &ChromeAccessModel) {
    let Some(overlay) = doc.find_by_id(crate::ids::ACCESS_OVERLAY) else { return };
    set_class_token(doc, overlay, "open", access.open);
    for (id, value) in [
        (crate::ids::AC_FONT_PILLS, access.font_value.as_str()),
        (crate::ids::AC_CURSOR_PILLS, access.cursor_value.as_str()),
    ] {
        let Some(group) = doc.find_by_id(id) else { continue };
        for pill in find_descendants_by_class(doc, group, "ac-pill") {
            let on = doc.get(pill).get_attr("data-value") == Some(value);
            set_class_token(doc, pill, "active", on);
        }
    }
    for (id, on) in [
        (crate::ids::AC_MOTION_TOGGLE, access.reduced_motion),
        (crate::ids::AC_COLORS_TOGGLE, access.forced_colors),
    ] {
        if let Some(t) = doc.find_by_id(id) {
            set_class_token(doc, t, "on", on);
        }
    }
}

/// Toggles `#printOverlay`'s `.open` class, moves the `selected` attribute
/// between `#printOrientationSelect`'s two `<option>`s, and toggles
/// `#printBgCheck`'s `checked` attribute ([BUG-420](../../../bugs/BUG-420-FIXED.md)).
///
/// Both the native `<select>`/`<option selected>` and `<input checked>`
/// attributes are what `lumen_layout::box_tree` reads to paint a collapsed
/// select's shown option / a checkbox's tick — the same mechanism
/// `crates/shell/src/forms.rs` drives for real web-content forms, reused
/// here directly on the chrome `Document`.
fn bind_print(doc: &mut Document, print: &ChromePrintModel) {
    let Some(overlay) = doc.find_by_id(crate::ids::PRINT_OVERLAY) else { return };
    set_class_token(doc, overlay, "open", print.open);
    if let Some(select) = doc.find_by_id(crate::ids::PRINT_ORIENTATION_SELECT) {
        let options = doc.get(select).children.clone();
        let selected_idx = usize::from(print.landscape);
        for (i, &opt) in options.iter().enumerate() {
            if i == selected_idx {
                set_attr(doc, opt, "selected", "");
            } else {
                remove_attr(doc, opt, "selected");
            }
        }
    }
    if let Some(check) = doc.find_by_id(crate::ids::PRINT_BG_CHECK) {
        if print.backgrounds {
            set_attr(doc, check, "checked", "");
        } else {
            remove_attr(doc, check, "checked");
        }
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

    let tag = doc.create_element(QualName::html("span"));
    set_attr(doc, tag, "class", "dd-tag");
    append_text(doc, tag, &s.tag);
    attach_child(doc, row, tag);

    row
}

/// Toggles `#findBar`'s `.open` class, writes [`ChromeFindModel::value`]/
/// `count_label` into `#findInput`/`#findCount` (CC-9), and toggles
/// `#findCount`'s `.error` class per [`ChromeFindModel::error`] (BUG-419).
/// Editing (caret, append/backspace) stays owned by the legacy `FindState`,
/// mirroring [`bind_omnibox`].
fn bind_find_bar(doc: &mut Document, find: &ChromeFindModel) {
    let Some(bar) = doc.find_by_id(crate::ids::FIND_BAR) else { return };
    set_class_token(doc, bar, "open", find.open);
    if let Some(input) = doc.find_by_id(crate::ids::FIND_INPUT) {
        set_attr(doc, input, "value", &find.value);
    }
    if let Some(count) = doc.find_by_id(crate::ids::FIND_COUNT) {
        set_text(doc, count, &find.count_label);
        set_class_token(doc, count, "error", find.error);
    }
    if let Some(fix) = doc.find_by_id(crate::ids::FIND_FIX) {
        set_class_token(doc, fix, "open", find.open && !find.layout_fix.is_empty());
    }
    if let Some(text) = doc.find_by_id(crate::ids::FIND_FIX_TEXT) {
        let label = if find.layout_fix.is_empty() { String::new() } else { format!("Показано: «{}»", find.layout_fix) };
        // An empty `<span>` has no text node yet: `set_text` would append an
        // empty one on every bind, which the incremental graft counts as a
        // structural change (BUG-341 gates).
        if !label.is_empty() || !doc.get(text).children.is_empty() {
            set_text(doc, text, &label);
        }
    }
}

/// Toggles `#downloadsPanel`'s `.open` class and rebuilds `.dl-card` rows
/// from `downloads` (CC-9).
fn bind_downloads(doc: &mut Document, open: bool, downloads: &[ChromeDownloadModel], clearable: bool) {
    let Some(panel) = doc.find_by_id(crate::ids::DOWNLOADS_PANEL) else { return };
    set_class_token(doc, panel, "open", open);
    if let Some(header) = find_by_class(doc, "dl-header") {
        set_class_token(doc, header, "has-finished", clearable);
    }
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

    if !d.actions.is_empty() {
        let actions = doc.create_element(QualName::html("div"));
        set_attr(doc, actions, "class", "dl-actions");
        for action in &d.actions {
            let (icon, label) = action.icon_and_label();
            let btn = doc.create_element(QualName::html("button"));
            set_attr(doc, btn, "data-dl", action.attr());
            // Own copy of the id: the hit path is read button-first.
            set_attr(doc, btn, "data-dl-id", &d.id.to_string());
            append_icon(doc, btn, icon);
            append_text(doc, btn, label);
            attach_child(doc, actions, btn);
        }
        attach_child(doc, card, actions);
    }

    card
}

/// Toggles `#archivePanel`'s `.open` class and rebuilds `.arc-card` rows
/// from `entries` (BUG-408) — mirrors [`bind_downloads`].
fn bind_archive(doc: &mut Document, open: bool, entries: &[ChromeArchiveEntryModel]) {
    let Some(panel) = doc.find_by_id(crate::ids::ARCHIVE_PANEL) else { return };
    set_class_token(doc, panel, "open", open);
    let Some(list) = find_by_class(doc, "arc-list") else { return };
    remove_children_with_class(doc, list, "arc-card");
    for e in entries {
        let card = build_arc_card(doc, e);
        attach_child(doc, list, card);
    }
}

fn build_arc_card(doc: &mut Document, e: &ChromeArchiveEntryModel) -> NodeId {
    let card = doc.create_element(QualName::html("div"));
    set_attr(doc, card, "class", "arc-card");

    let row = doc.create_element(QualName::html("div"));
    set_attr(doc, row, "class", "arc-row");

    if let Some(color) = &e.container_color {
        let stripe = doc.create_element(QualName::html("span"));
        set_attr(doc, stripe, "class", "arc-stripe");
        set_attr(doc, stripe, "style", &format!("background:{color}"));
        attach_child(doc, row, stripe);
    }

    let fav = doc.create_element(QualName::html("div"));
    set_attr(doc, fav, "class", "arc-fav");
    append_text(doc, fav, &e.fav_letter);
    attach_child(doc, row, fav);

    let text = doc.create_element(QualName::html("div"));
    set_attr(doc, text, "class", "arc-text");
    let title = doc.create_element(QualName::html("div"));
    set_attr(doc, title, "class", "arc-title");
    append_text(doc, title, &e.title);
    attach_child(doc, text, title);
    let url = doc.create_element(QualName::html("div"));
    set_attr(doc, url, "class", "arc-url");
    append_text(doc, url, &e.url);
    attach_child(doc, text, url);
    attach_child(doc, row, text);

    let actions = doc.create_element(QualName::html("div"));
    set_attr(doc, actions, "class", "arc-actions");

    let restore = doc.create_element(QualName::html("button"));
    set_attr(doc, restore, "class", "arc-restore");
    set_attr(doc, restore, "data-action", "archive-restore");
    set_attr(doc, restore, "aria-label", "Восстановить вкладку");
    // Carries its own copy of `data-archive-id` (not just the row), same
    // reason `build_tab_row`'s close button does — `chrome_action_at` only
    // sees the `data-action`-carrying node.
    set_attr(doc, restore, "data-archive-id", &e.id.to_string());
    attach_child(doc, actions, restore);

    let dismiss = doc.create_element(QualName::html("button"));
    set_attr(doc, dismiss, "class", "arc-dismiss");
    set_attr(doc, dismiss, "data-action", "archive-dismiss");
    set_attr(doc, dismiss, "aria-label", "Удалить из архива");
    set_attr(doc, dismiss, "data-archive-id", &e.id.to_string());
    attach_child(doc, actions, dismiss);

    attach_child(doc, row, actions);
    attach_child(doc, card, row);

    card
}

/// Stand-in for [`ChromeModel::popover_domain`] when the loaded page has no
/// host to name (`about:blank`, `file://`) — [BUG-411](../../../bugs/BUG-411-FIXED.md).
/// Without it the design reference's `figma.com` demo text would stay on
/// screen and read as a real domain.
pub const NO_DOMAIN_LABEL: &str = "этой странице";

/// Text of `#compatText` for `n` applied compatibility fixes (COMPAT-PACK-IND):
/// «К сайту применено N исправлений совместимости» with the Russian plural form
/// of «исправление» (1 → исправление, 2–4 → исправления, otherwise исправлений).
#[must_use]
pub fn compat_fixes_label(n: u32) -> String {
    let word = match (n % 100, n % 10) {
        (11..=14, _) => "исправлений",
        (_, 1) => "исправление",
        (_, 2..=4) => "исправления",
        _ => "исправлений",
    };
    format!("К сайту применено {n} {word} совместимости")
}

/// Toggles `#permPopover`'s `.open` class, writes `blocked_total` into
/// `#statTrackers`, binds the per-site shields row (`#shieldSiteToggle`'s
/// `.on` class + `#shieldSiteState`'s Вкл/Выкл text) and the popover's
/// `#permDomain` heading span, and marks each permission row's allow/deny
/// buttons per `permissions` (CC-9; domain + shields row + rows 3/4 added by
/// [BUG-411](../../../bugs/BUG-411-FIXED.md)).
fn bind_popover(doc: &mut Document, model: &ChromeModel) {
    let Some(popover) = doc.find_by_id(crate::ids::PERM_POPOVER) else { return };
    set_class_token(doc, popover, "open", model.popover_open);
    if let Some(stat) = doc.find_by_id(crate::ids::STAT_TRACKERS) {
        set_text(doc, stat, &model.blocked_total.to_string());
    }
    let domain =
        if model.popover_domain.is_empty() { NO_DOMAIN_LABEL } else { model.popover_domain.as_str() };
    if let Some(span) = doc.find_by_id(crate::ids::PERM_DOMAIN) {
        set_text(doc, span, domain);
    }
    if let Some(toggle) = doc.find_by_id(crate::ids::SHIELD_SITE_TOGGLE) {
        set_class_token(doc, toggle, "on", model.site_shields_on);
    }
    if let Some(state) = doc.find_by_id(crate::ids::SHIELD_SITE_STATE) {
        set_text(doc, state, if model.site_shields_on { "Вкл" } else { "Выкл" });
    }
    if let Some(row) = doc.find_by_id(crate::ids::COMPAT_ROW) {
        set_class_token(doc, row, "show", model.compat_fixes > 0);
    }
    if model.compat_fixes > 0
        && let Some(text) = doc.find_by_id(crate::ids::COMPAT_TEXT)
    {
        set_text(doc, text, &compat_fixes_label(model.compat_fixes));
    }
    bind_fix_panel(doc, &model.fix);
    let rows: Vec<NodeId> =
        doc.get(popover).children.iter().copied().filter(|&c| has_class(doc, c, "perm-row")).collect();
    for (row, state) in rows.into_iter().zip(model.permissions.iter()) {
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
    if let Some(btn) = doc.find_by_id(crate::ids::ZOOM_BTN) {
        set_class_token(doc, btn, "show", omnibox.zoom_percent.is_some());
    }
    if let (Some(label), Some(percent)) =
        (doc.find_by_id(crate::ids::ZOOM_LABEL), omnibox.zoom_percent)
    {
        set_text(doc, label, &format!("{percent}%"));
    }
    if let Some(btn) = doc.find_by_id(crate::ids::POPUP_BTN) {
        set_class_token(doc, btn, "show", omnibox.popups_blocked > 0);
    }
    if let Some(label) = doc.find_by_id(crate::ids::POPUP_LABEL) {
        set_text(doc, label, &omnibox.popups_blocked.max(1).to_string());
    }
    if let Some(chip) = doc.find_by_id(crate::ids::HELP_CHIP) {
        set_class_token(doc, chip, "show", omnibox.help_chip.is_some());
    }
    if let (Some(text), Some(question)) = (doc.find_by_id(crate::ids::HELP_CHIP_TEXT), &omnibox.help_chip) {
        set_text(doc, text, question);
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
    // The asset indents one nesting level through `.child`; tree mode carries
    // its own margin, `TREE_INDENT_PX` per level.
    match tab.tree {
        Some(t) if t.depth > 0 => {
            set_attr(doc, row, "style", &format!("margin-left:{}px", t.depth * TREE_INDENT_PX));
        }
        _ => remove_attr(doc, row, "style"),
    }
}

/// Indent per tree level in the sidebar list, CSS px (as `.tab-row.child`).
const TREE_INDENT_PX: usize = 12;

/// Builds `row`'s children fresh (tree-line? / stripe? / fav / title /
/// badge-or-close) — assumes `row` currently has none. Shared by
/// [`build_tab_row`] and [`update_tab_row`]'s shape-mismatch fallback.
fn populate_tab_row_children(doc: &mut Document, row: NodeId, tab: &ChromeTabModel) {
    if tab.is_child {
        let tree_line = doc.create_element(QualName::html("span"));
        set_attr(doc, tree_line, "class", "tree-line");
        attach_child(doc, row, tree_line);
    }

    if let Some(expanded) = tab.tree.and_then(|t| t.expanded) {
        let arrow = doc.create_element(QualName::html("button"));
        set_attr(doc, arrow, "class", "tab-arrow");
        set_attr(doc, arrow, "data-action", "toggle-tab-subtree");
        set_attr(doc, arrow, "data-tab-id", &tab.id.to_string());
        set_attr(doc, arrow, "aria-expanded", if expanded { "true" } else { "false" });
        append_text(doc, arrow, arrow_glyph(expanded));
        attach_child(doc, row, arrow);
    }

    if let Some(group) = &tab.group {
        let stripe = doc.create_element(QualName::html("span"));
        set_attr(doc, stripe, "class", "group-stripe");
        set_attr(doc, stripe, "style", &format!("background:{}", group.color));
        set_attr(doc, stripe, "title", &group_stripe_title(group));
        attach_child(doc, row, stripe);
    }

    if let Some(color) = &tab.container_color {
        let stripe = doc.create_element(QualName::html("span"));
        set_attr(doc, stripe, "class", "container-stripe");
        set_attr(doc, stripe, "style", &format!("background:{color}"));
        attach_child(doc, row, stripe);
    }

    let fav = doc.create_element(QualName::html("span"));
    set_attr(doc, fav, "class", fav_class(tab));
    append_text(doc, fav, &fav_text(tab));
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
/// A shape change (`is_child`/`group`-presence/`container_color`-presence/
/// `sleeping` flipped — rare: only real tab-state changes cause this, never a
/// bare hover/keystroke relayout) falls back to clearing and rebuilding the
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
    let arrow_state = tab.tree.and_then(|t| t.expanded);
    let has_arrow = children.get(idx).is_some_and(|&c| has_class(doc, c, "tab-arrow"));
    if has_arrow != arrow_state.is_some() {
        rebuild_tab_row_children(doc, row, tab);
        return;
    }
    if let Some(expanded) = arrow_state {
        set_attr(doc, children[idx], "data-tab-id", &tab.id.to_string());
        set_attr(doc, children[idx], "aria-expanded", if expanded { "true" } else { "false" });
        set_text(doc, children[idx], arrow_glyph(expanded));
        idx += 1;
    }
    let has_group_stripe = children.get(idx).is_some_and(|&c| has_class(doc, c, "group-stripe"));
    if has_group_stripe != tab.group.is_some() {
        rebuild_tab_row_children(doc, row, tab);
        return;
    }
    if let Some(group) = &tab.group {
        set_attr(doc, children[idx], "style", &format!("background:{}", group.color));
        set_attr(doc, children[idx], "title", &group_stripe_title(group));
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
    set_attr(doc, fav, "class", fav_class(tab));
    set_text(doc, fav, &fav_text(tab));
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

fn arrow_glyph(expanded: bool) -> &'static str {
    if expanded { "\u{25BE}" } else { "\u{25B8}" }
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

/// UX-TAB-CLOSE-FREEZE: pins every `.hbar-tab` of `container` to `width` CSS px
/// via an inline style, or drops the style again when `width` is `None`.
fn apply_hbar_tab_freeze(doc: &mut Document, container: NodeId, width: Option<f32>) {
    let rows: Vec<NodeId> =
        doc.get(container).children.iter().copied().filter(|&r| has_class(doc, r, "hbar-tab")).collect();
    for row in rows {
        match width {
            Some(w) => set_attr(doc, row, "style", &format!("width:{w}px;min-width:{w}px;max-width:{w}px;flex:none")),
            None => remove_attr(doc, row, "style"),
        }
    }
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
    set_attr(doc, fav, "class", fav_class(tab));
    append_text(doc, fav, &fav_text(tab));
    attach_child(doc, row, fav);

    let title = doc.create_element(QualName::html("span"));
    set_attr(doc, title, "class", "tab-title");
    append_text(doc, title, &tab.title);
    attach_child(doc, row, title);

    // UX-TAB-CLOSE-FREEZE: the × is always present (CSS shows it on the active
    // tab and on hover), carrying its own `data-tab-id` like the sidebar's.
    let close = doc.create_element(QualName::html("button"));
    set_attr(doc, close, "class", "tab-close");
    set_attr(doc, close, "data-action", "close-tab");
    set_attr(doc, close, "aria-label", "Закрыть вкладку");
    set_attr(doc, close, "data-tab-id", &tab.id.to_string());
    append_text(doc, close, "\u{00D7}");
    attach_child(doc, row, close);
}

/// Updates an existing `.hbar-tab` in place (BUG-341/CC-14 — see
/// [`update_tab_row`]). Fixed fav+title shape, no fallback branches needed
/// beyond the defensive missing-children case.
fn update_hbar_tab(doc: &mut Document, row: NodeId, tab: &ChromeTabModel) {
    apply_hbar_tab_attrs(doc, row, tab);
    let children: Vec<NodeId> = doc.get(row).children.clone();
    let (Some(&fav), Some(&title), Some(&close)) = (children.first(), children.get(1), children.get(2)) else {
        for child in children {
            detach_node(doc, child);
        }
        populate_hbar_tab_children(doc, row, tab);
        return;
    };
    set_attr(doc, fav, "class", fav_class(tab));
    set_text(doc, fav, &fav_text(tab));
    set_text(doc, title, &tab.title);
    set_attr(doc, close, "data-tab-id", &tab.id.to_string());
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

/// Class of the `.tab-fav` slot: the asset's `.spinner` while loading.
fn fav_class(tab: &ChromeTabModel) -> &'static str {
    if tab.loading { "spinner" } else { "tab-fav" }
}

fn fav_text(tab: &ChromeTabModel) -> String {
    if tab.loading { String::new() } else { first_letter(&tab.title) }
}

fn first_letter(s: &str) -> String {
    s.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "\u{2022}".to_string())
}

/// Tooltip text for a `.group-stripe` — the group name, plus a "folded"
/// hint when this row is the sole surviving member of a collapsed group
/// (see [`ChromeTabGroup::collapsed`]).
fn group_stripe_title(group: &ChromeTabGroup) -> String {
    if group.collapsed {
        format!("{} (свёрнута)", group.name)
    } else {
        group.name.clone()
    }
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

/// Every element within `root`'s subtree carrying attribute `name` (with any
/// value) — collects instead of stopping at the first match, used to sync
/// `.active` across a whole button group (CC-18's `#demoSwitch` shape
/// buttons and the profile/layout buttons inside `#demoBar`'s `.demo-group`s)
/// without also touching `<body>`'s own same-named attribute outside `root`.
fn collect_by_attr(doc: &Document, root: NodeId, name: &str) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack: Vec<NodeId> = vec![root];
    while let Some(id) = stack.pop() {
        let node = doc.get(id);
        if matches!(node.data, NodeData::Element { .. }) && node.get_attr(name).is_some() {
            out.push(id);
        }
        for &child in node.children.iter().rev() {
            stack.push(child);
        }
    }
    out
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
mod tests;
