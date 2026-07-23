//! Integration tests for lumen-a11y's synthetic chrome nodes (DS-17).

use lumen_a11y::chrome::{attach_chrome, chrome_nodes, ChromeButton, ChromeSnapshot, ChromeTab};
use lumen_a11y::{build_ax_tree, AXRole};
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
