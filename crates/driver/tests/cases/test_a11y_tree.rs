//! Tests for 8G: A11y tree first-class via BrowserSession (lumen-a11y::build_ax_tree).

use lumen_driver::{AxQuery, BrowserSession, InProcessSession};

fn make_session(html: &str) -> InProcessSession {
    let mut s = InProcessSession::new();
    s.navigate_html(html).expect("navigate_html failed");
    s
}

// ── a11y_tree ──────────────────────────────────────────────────────────────────

#[test]
fn a11y_tree_link_role() {
    let s = make_session(r#"<body><a href="/about">About</a></body>"#);
    let tree = s.a11y_tree().expect("a11y_tree failed");
    let found = find_role(&tree, "link");
    assert!(found.is_some(), "expected a 'link' node in a11y tree");
    let node = found.unwrap();
    assert_eq!(node.name, "About", "link name should be its text content");
}

#[test]
fn a11y_tree_button_role() {
    let s = make_session(r#"<body><button>Click me</button></body>"#);
    let tree = s.a11y_tree().expect("a11y_tree failed");
    let node = find_role(&tree, "button").expect("no button node");
    assert_eq!(node.name, "Click me");
}

#[test]
fn a11y_tree_heading_level() {
    let s = make_session(r#"<body><h2>Section</h2></body>"#);
    let tree = s.a11y_tree().expect("a11y_tree failed");
    let node = find_role(&tree, "heading").expect("no heading node");
    assert_eq!(node.name, "Section");
    assert_eq!(node.state.level, Some(2), "h2 should have level=2");
}

#[test]
fn a11y_tree_image_alt() {
    let s = make_session(r#"<body><img src="x.png" alt="Logo"></body>"#);
    let tree = s.a11y_tree().expect("a11y_tree failed");
    let node = find_role(&tree, "img").expect("no img node");
    assert_eq!(node.name, "Logo");
}

#[test]
fn a11y_tree_node_id_nonzero() {
    // DOM NodeId(0) is the document root, semantic elements start at higher indices.
    let s = make_session(r#"<body><button>OK</button></body>"#);
    let tree = s.a11y_tree().expect("a11y_tree failed");
    let node = find_role(&tree, "button").expect("no button node");
    // node_id should be > 0 (document root is 0, button is some later node)
    assert!(node.node_id > 0, "button node_id should be > 0, got {}", node.node_id);
}

#[test]
fn a11y_tree_disabled_state() {
    let s = make_session(r#"<body><button disabled>Nope</button></body>"#);
    let tree = s.a11y_tree().expect("a11y_tree failed");
    let node = find_role(&tree, "button").expect("no button node");
    assert!(node.state.disabled, "disabled button should have state.disabled=true");
}

#[test]
fn a11y_tree_checkbox_checked() {
    let s = make_session(r#"<body><input type="checkbox" checked></body>"#);
    let tree = s.a11y_tree().expect("a11y_tree failed");
    let node = find_role(&tree, "checkbox").expect("no checkbox node");
    assert_eq!(node.state.checked, Some(Some(true)), "checked checkbox should have checked=Some(Some(true))");
}

#[test]
fn a11y_tree_aria_label() {
    let s = make_session(r#"<body><button aria-label="Close dialog">X</button></body>"#);
    let tree = s.a11y_tree().expect("a11y_tree failed");
    let node = find_role(&tree, "button").expect("no button node");
    assert_eq!(node.name, "Close dialog", "aria-label should override text content");
}

#[test]
fn a11y_tree_input_placeholder() {
    let s = make_session(r#"<body><input type="text" placeholder="Search..."></body>"#);
    let tree = s.a11y_tree().expect("a11y_tree failed");
    let node = find_role(&tree, "textbox").expect("no textbox node");
    assert_eq!(node.placeholder, "Search...");
}

// ── query_a11y ─────────────────────────────────────────────────────────────────

#[test]
fn query_a11y_by_role_found() {
    let s = make_session(r#"<body><nav><a href="/">Home</a></nav></body>"#);
    let result = s
        .query_a11y(&AxQuery::Role { role: "navigation".into(), name: None })
        .expect("query_a11y failed");
    assert!(result.is_some(), "should find navigation role");
}

#[test]
fn query_a11y_by_role_not_found() {
    let s = make_session(r#"<body><p>Plain text</p></body>"#);
    let result = s
        .query_a11y(&AxQuery::Role { role: "button".into(), name: None })
        .expect("query_a11y failed");
    assert!(result.is_none(), "should not find button in plain paragraph");
}

#[test]
fn query_a11y_by_role_and_name() {
    let s = make_session(r#"<body><button>Save</button><button>Cancel</button></body>"#);
    let result = s
        .query_a11y(&AxQuery::Role { role: "button".into(), name: Some("Save".into()) })
        .expect("query_a11y failed");
    assert!(result.is_some(), "should find Save button");
    let node = result.unwrap();
    assert_eq!(node.name, "Save");
}

#[test]
fn query_a11y_all_by_role() {
    let s = make_session(r#"<body><button>A</button><button>B</button><button>C</button></body>"#);
    let results = s
        .query_a11y_all(&AxQuery::Role { role: "button".into(), name: None })
        .expect("query_a11y_all failed");
    assert_eq!(results.len(), 3, "should find 3 buttons");
}

#[test]
fn query_a11y_name_contains() {
    let s = make_session(r#"<body><a href="/">Homepage link</a></body>"#);
    let result = s
        .query_a11y(&AxQuery::NameContains("Homepage".into()))
        .expect("query_a11y failed");
    assert!(result.is_some(), "should find link by partial name");
}

// ── helpers ────────────────────────────────────────────────────────────────────

use lumen_driver::A11yNode;

fn find_role<'a>(node: &'a A11yNode, role: &str) -> Option<&'a A11yNode> {
    if node.role == role {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_role(child, role) {
            return Some(found);
        }
    }
    None
}
