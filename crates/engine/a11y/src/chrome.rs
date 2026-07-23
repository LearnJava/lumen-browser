//! Synthetic accessibility nodes for Lumen's own chrome — DS-17.
//!
//! The tab strip, toolbar, and omnibox are not DOM elements, so
//! [`build_ax_tree`](crate::build_ax_tree) never sees them. This module lets
//! the shell describe their current state as a [`ChromeSnapshot`] and turn it
//! into synthetic [`AXNode`]s (`chrome_nodes`), then sew those in as siblings
//! of the DOM-derived tree (`attach_chrome`) so a screen reader or MCP
//! `resource://a11y_tree` client sees `TabList`/`ToolBar` alongside the page.

use crate::{AXNode, AXRole, AXState, AXTree};
use lumen_dom::NodeId;

/// One open tab, as shown in the synthetic `TabList`.
#[derive(Debug, Clone)]
pub struct ChromeTab {
    /// Tab title, mirrors the shell's `TabEntry::title`.
    pub title: String,
    /// Whether this is the active (foreground) tab.
    pub selected: bool,
}

/// One button in the synthetic `ToolBar` (profile avatar, nav cluster, right
/// action cluster). Plain actions (back/forward/reload) report `pressed:
/// None`; toggle buttons (find/sidebars/downloads/DevTools/settings) report
/// their panel's `visible` flag.
#[derive(Debug, Clone)]
pub struct ChromeButton {
    /// Accessible name, e.g. `"Назад"`.
    pub name: String,
    /// `aria-pressed`-equivalent state for toggle buttons; `None` for plain actions.
    pub pressed: Option<bool>,
}

/// Snapshot of chrome UI state needed to build synthetic AX nodes.
///
/// Built fresh from the shell's own fields every time the AX tree is
/// rebuilt — nothing here is cached across calls.
#[derive(Debug, Clone, Default)]
pub struct ChromeSnapshot {
    /// Open tabs, in strip order.
    pub tabs: Vec<ChromeTab>,
    /// Toolbar buttons, in visual left-to-right order.
    pub buttons: Vec<ChromeButton>,
    /// Current omnibox field value (page URL when not focused, live edit
    /// buffer when focused — matches what's actually painted).
    pub omnibox_value: String,
}

/// First `u32::MAX`-descending offset reserved for the synthetic wrapper root
/// built by [`attach_chrome`], kept well clear of the small per-call offsets
/// `chrome_nodes` hands out so the two never collide.
const WRAPPER_NODE_OFFSET: u32 = 1_000_000;

/// Build a synthetic [`NodeId`] that cannot collide with a real DOM node.
///
/// Real DOM node indices grow from 0; counting down from `u32::MAX` instead
/// leaves an effectively unreachable gap (no document has billions of nodes).
fn synthetic_id(offset: u32) -> NodeId {
    NodeId::from_index((u32::MAX - offset) as usize)
}

fn next_id(counter: &mut u32) -> NodeId {
    let id = synthetic_id(*counter);
    *counter += 1;
    id
}

/// Construct a leaf [`AXNode`] with no relationships and empty children.
fn leaf(node_id: NodeId, role: AXRole, name: String, state: AXState) -> AXNode {
    AXNode {
        node_id,
        role,
        name,
        description: String::new(),
        placeholder: String::new(),
        state,
        children: Vec::new(),
        controls: None,
        owns: Vec::new(),
        flow_to: Vec::new(),
        details: None,
    }
}

/// Build the synthetic `TabList` and `ToolBar` nodes for `snapshot` — DS-17.
///
/// Returns `[TabList[Tab×N], ToolBar[Button×K, ComboBox]]`, meant to be
/// attached as siblings of the DOM-derived tree via [`attach_chrome`]. Node
/// IDs are synthetic (see [`synthetic_id`]) and only meaningful within one
/// call — they are not persisted across snapshots.
pub fn chrome_nodes(snapshot: &ChromeSnapshot) -> Vec<AXNode> {
    let mut counter = 0u32;

    let tabs = snapshot
        .tabs
        .iter()
        .map(|tab| {
            leaf(
                next_id(&mut counter),
                AXRole::Tab,
                tab.title.clone(),
                AXState { selected: Some(tab.selected), ..AXState::default() },
            )
        })
        .collect();
    let mut tab_list = leaf(next_id(&mut counter), AXRole::TabList, String::new(), AXState::default());
    tab_list.children = tabs;

    let mut toolbar_children: Vec<AXNode> = snapshot
        .buttons
        .iter()
        .map(|btn| {
            leaf(
                next_id(&mut counter),
                AXRole::Button,
                btn.name.clone(),
                AXState { pressed: btn.pressed, ..AXState::default() },
            )
        })
        .collect();
    toolbar_children.push(leaf(
        next_id(&mut counter),
        AXRole::ComboBox,
        "Адресная строка".to_owned(),
        AXState { value_text: snapshot.omnibox_value.clone(), ..AXState::default() },
    ));
    let mut toolbar = leaf(next_id(&mut counter), AXRole::Toolbar, String::new(), AXState::default());
    toolbar.children = toolbar_children;

    vec![tab_list, toolbar]
}

/// Attach synthetic `chrome` nodes (from [`chrome_nodes`]) as siblings of the
/// existing DOM-derived tree, under one wrapping root — DS-17.
///
/// The wrapper uses [`AXRole::Generic`]: WAI-ARIA has no "browser window"
/// role, and the wrapper carries no semantics of its own — it only gives the
/// chrome nodes and the web content a common parent.
pub fn attach_chrome(tree: AXTree, mut chrome: Vec<AXNode>) -> AXTree {
    chrome.push(tree.root);
    let mut wrapper = leaf(synthetic_id(WRAPPER_NODE_OFFSET), AXRole::Generic, String::new(), AXState::default());
    wrapper.children = chrome;
    AXTree { root: wrapper }
}
