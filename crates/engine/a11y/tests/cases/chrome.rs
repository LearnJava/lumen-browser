//! Integration tests for lumen-a11y's chrome accessibility nodes (DS-17, CC-13).

use lumen_a11y::chrome::{
    attach_chrome, chrome_nodes, chrome_root_from_document, ChromeButton, ChromeSnapshot, ChromeTab,
};
use lumen_a11y::{build_ax_tree, AXNode, AXRole};
use lumen_dom::build_flat_tree;
use lumen_html_parser::parse;

fn sample_snapshot() -> ChromeSnapshot {
    ChromeSnapshot {
        tabs: vec![
            ChromeTab { title: "Пример".to_owned(), selected: false },
            ChromeTab { title: "Активная".to_owned(), selected: true },
        ],
        buttons: vec![
            ChromeButton { name: "Назад".to_owned(), pressed: None },
            ChromeButton { name: "Загрузки".to_owned(), pressed: Some(true) },
        ],
        omnibox_value: "https://example.com".to_owned(),
    }
}

#[test]
fn chrome_nodes_produces_tablist_and_toolbar() {
    let nodes = chrome_nodes(&sample_snapshot());
    assert_eq!(nodes.len(), 2, "expected [TabList, ToolBar]");
    assert_eq!(nodes[0].role, AXRole::TabList);
    assert_eq!(nodes[1].role, AXRole::Toolbar);
}

#[test]
fn chrome_nodes_tab_titles_and_selection() {
    let nodes = chrome_nodes(&sample_snapshot());
    let tabs = &nodes[0].children;
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[0].role, AXRole::Tab);
    assert_eq!(tabs[0].name, "Пример");
    assert_eq!(tabs[0].state.selected, Some(false));
    assert_eq!(tabs[1].name, "Активная");
    assert_eq!(tabs[1].state.selected, Some(true));
}

#[test]
fn chrome_nodes_toolbar_buttons_and_omnibox() {
    let nodes = chrome_nodes(&sample_snapshot());
    let toolbar_children = &nodes[1].children;
    // 2 buttons + 1 combobox (omnibox).
    assert_eq!(toolbar_children.len(), 3);
    assert_eq!(toolbar_children[0].role, AXRole::Button);
    assert_eq!(toolbar_children[0].name, "Назад");
    assert_eq!(toolbar_children[0].state.pressed, None);
    assert_eq!(toolbar_children[1].role, AXRole::Button);
    assert_eq!(toolbar_children[1].state.pressed, Some(true));
    let omnibox = toolbar_children.last().unwrap();
    assert_eq!(omnibox.role, AXRole::ComboBox);
    assert_eq!(omnibox.state.value_text, "https://example.com");
}

#[test]
fn chrome_node_ids_are_unique() {
    let nodes = chrome_nodes(&sample_snapshot());
    let mut ids = Vec::new();
    for node in &nodes {
        collect_ids(node, &mut ids);
    }
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "expected all synthetic node ids to be unique");
}

fn collect_ids(node: &lumen_a11y::AXNode, out: &mut Vec<lumen_dom::NodeId>) {
    out.push(node.node_id);
    for child in &node.children {
        collect_ids(child, out);
    }
}

#[test]
fn attach_chrome_wraps_dom_tree_as_sibling() {
    let doc = parse("<body><p>Hello</p></body>");
    let flat_tree = build_flat_tree(&doc);
    let dom_tree = build_ax_tree(&doc, doc.root(), &flat_tree);
    let dom_root_id = dom_tree.root.node_id;
    let chrome = chrome_nodes(&sample_snapshot());
    let combined = attach_chrome(dom_tree, chrome);

    // Wrapper root has 3 children: TabList, ToolBar, and the original DOM
    // root as the last sibling — chrome nodes never displace the web tree.
    assert_eq!(combined.root.children.len(), 3);
    assert_eq!(combined.root.children[0].role, AXRole::TabList);
    assert_eq!(combined.root.children[1].role, AXRole::Toolbar);
    assert_eq!(combined.root.children[2].node_id, dom_root_id);
}

#[test]
fn attach_chrome_wrapper_id_does_not_collide_with_chrome_ids() {
    let doc = parse("<body></body>");
    let flat_tree = build_flat_tree(&doc);
    let dom_tree = build_ax_tree(&doc, doc.root(), &flat_tree);
    let chrome = chrome_nodes(&sample_snapshot());
    let mut chrome_ids = Vec::new();
    for node in &chrome {
        collect_ids(node, &mut chrome_ids);
    }
    let combined = attach_chrome(dom_tree, chrome);
    assert!(
        !chrome_ids.contains(&combined.root.node_id),
        "wrapper root id must not collide with a chrome node id"
    );
}

// ── chrome_root_from_document (CC-13) ───────────────────────────────────────

fn find_role_dfs(node: &AXNode, role: AXRole) -> Option<&AXNode> {
    if node.role == role {
        return Some(node);
    }
    node.children.iter().find_map(|child| find_role_dfs(child, role))
}

/// Mirrors the shape `scripts/gen_chrome_assets.py` injects into
/// `assets/chrome/chrome.html`: a `role="tablist"` container with two
/// `role="tab"` rows (one `aria-selected`), a `role="toolbar"` with an
/// icon-only labelled button and a `role="combobox"` address bar.
const CHROME_LIKE_HTML: &str = r#"<body>
    <div id="sbTabs" role="tablist">
        <div role="tab" aria-selected="true">Активная</div>
        <div role="tab" aria-selected="false">Пример</div>
    </div>
    <div data-testid="toolbar" role="toolbar">
        <button aria-label="Обновить"></button>
        <input id="omniInput" role="combobox" aria-autocomplete="list" value="https://example.com">
    </div>
</body>"#;

#[test]
fn chrome_root_from_document_derives_roles_from_markup() {
    let doc = parse(CHROME_LIKE_HTML);
    let flat_tree = build_flat_tree(&doc);
    let root = chrome_root_from_document(&doc, doc.root(), &flat_tree);

    let tablist = find_role_dfs(&root, AXRole::TabList).expect("expected TabList from role=\"tablist\"");
    assert_eq!(tablist.children.len(), 2, "expected two role=\"tab\" rows");
    assert_eq!(tablist.children[0].state.selected, Some(true));
    assert_eq!(tablist.children[1].state.selected, Some(false));

    let toolbar = find_role_dfs(&root, AXRole::Toolbar).expect("expected Toolbar from role=\"toolbar\"");
    let button = toolbar
        .children
        .iter()
        .find(|c| c.role == AXRole::Button)
        .expect("expected Button in toolbar");
    assert_eq!(button.name, "Обновить", "aria-label should become the accessible name");
    assert!(
        toolbar.children.iter().any(|c| c.role == AXRole::ComboBox),
        "expected ComboBox from role=\"combobox\" address bar"
    );
}

#[test]
fn chrome_root_from_document_ids_are_synthetic_and_unique() {
    let doc = parse(CHROME_LIKE_HTML);
    let flat_tree = build_flat_tree(&doc);
    let root = chrome_root_from_document(&doc, doc.root(), &flat_tree);

    let mut ids = Vec::new();
    collect_ids(&root, &mut ids);
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "expected all remapped node ids to be unique");
    // Real DOM node indices grow from 0 (see `synthetic_id`'s doc comment) —
    // every remapped id must land well above that range so it can never
    // collide with the page document's own (also 0-based) NodeIds.
    assert!(
        ids.iter().all(|id| id.index() > 1_000_000),
        "expected every chrome_root_from_document id to be remapped into the synthetic range"
    );
}

#[test]
fn chrome_root_from_document_attaches_via_attach_chrome() {
    let chrome_doc = parse(CHROME_LIKE_HTML);
    let chrome_flat_tree = build_flat_tree(&chrome_doc);
    let chrome_root = chrome_root_from_document(&chrome_doc, chrome_doc.root(), &chrome_flat_tree);

    let page_doc = parse("<body><p>Hello</p></body>");
    let page_flat_tree = build_flat_tree(&page_doc);
    let page_tree = build_ax_tree(&page_doc, page_doc.root(), &page_flat_tree);
    let page_root_id = page_tree.root.node_id;

    let combined = attach_chrome(page_tree, vec![chrome_root]);

    assert_eq!(combined.root.children.len(), 2, "expected [chrome subtree, page tree]");
    assert_eq!(combined.root.children[1].node_id, page_root_id, "page tree stays the last sibling");
    assert!(
        find_role_dfs(&combined.root, AXRole::TabList).is_some(),
        "engine-derived chrome roles should survive attach_chrome"
    );
}
