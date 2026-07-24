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
}
