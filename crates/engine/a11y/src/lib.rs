//! Accessibility tree for Lumen — ARIA roles, states, and accessible name computation.
//!
//! Builds a parallel `AXTree` from a DOM `Document`. Each `AXNode` carries:
//! * a semantic role (implicit from HTML tag or explicit from `role="..."`)
//! * computed accessible name (aria-label → labelledby → alt → text content → title)
//! * ARIA state flags (checked, disabled, expanded, hidden, selected, …)
//!
//! Platform bridges (UIA / AT-SPI / NSAccessibility) are P3's job; this crate
//! only builds the language-neutral tree that bridges will consume.

mod names;
mod roles;

pub use names::{compute_description, compute_name};
pub use roles::{implicit_role, AXRole};

use lumen_dom::{Document, FlatTree, InputType, NodeData, NodeId};
use serde::{Deserialize, Serialize};

// ── State flags ──────────────────────────────────────────────────────────────

/// `aria-live` values per WAI-ARIA §6.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiveRegion {
    /// Live updates announced when user is idle.
    Polite,
    /// Live updates announced immediately, interrupting the user.
    Assertive,
}

/// `aria-current` values per WAI-ARIA §5.4.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AriaCurrent {
    /// Page the user is currently viewing.
    Page,
    /// Step in a multi-step process.
    Step,
    /// Location within a structure.
    Location,
    /// Date in a calendar.
    Date,
    /// Time in a time picker.
    Time,
}

/// ARIA state and property flags for one accessibility node.
///
/// Each field corresponds to a WAI-ARIA state/property or equivalent HTML
/// attribute. Tri-state fields use `Option<bool>`: `None` = not applicable
/// or "mixed", `Some(true/false)` = explicit value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AXState {
    /// `aria-checked` / HTML `checked`. `None` = not a checkable role.
    /// `Some(None)` = mixed (indeterminate). `Some(Some(b))` = checked/unchecked.
    pub checked: Option<Option<bool>>,
    /// `aria-disabled="true"` or HTML `disabled` attribute.
    pub disabled: bool,
    /// `aria-expanded` — disclosure widget open/closed. `None` = not applicable.
    pub expanded: Option<bool>,
    /// `aria-hidden="true"` — node and its subtree are invisible to AT.
    pub hidden: bool,
    /// `aria-selected`. `None` = not applicable.
    pub selected: Option<bool>,
    /// `aria-pressed` — toggle button state. `None` = not a toggle.
    pub pressed: Option<bool>,
    /// `aria-required="true"` / HTML `required`.
    pub required: bool,
    /// `aria-readonly="true"` / HTML `readonly`.
    pub readonly: bool,
    /// `aria-invalid="true"`.
    pub invalid: bool,
    /// `aria-busy="true"` — live region is being updated.
    pub busy: bool,
    /// Parsed `tabindex` value. Negative = focusable by script only, 0 = natural order.
    pub tab_index: Option<i32>,
    /// `aria-level` / implicit heading level for `<h1>`–`<h6>`.
    pub level: Option<u32>,
    /// `aria-setsize` — total item count in the owning set.
    pub set_size: Option<u32>,
    /// `aria-posinset` — 1-based position within the owning set.
    pub pos_in_set: Option<u32>,
    /// `aria-live` value. `None` = not a live region.
    pub live: Option<LiveRegion>,
    /// `aria-multiline="true"` (textbox).
    pub multiline: bool,
    /// `aria-multiselectable="true"` (listbox, grid, tree).
    pub multiselectable: bool,
    /// `aria-orientation`: `Some(true)` = horizontal, `Some(false)` = vertical.
    pub horizontal: Option<bool>,
    /// `aria-current` — marks the element as current in a set of options.
    pub current: Option<AriaCurrent>,
    /// `aria-modal="true"` — element is modal.
    pub modal: bool,
    /// `aria-roledescription` — custom role description text.
    pub role_description: String,
    /// `aria-valuenow` — current value for range widgets.
    pub value_now: String,
    /// `aria-valuemin` — minimum value for range widgets.
    pub value_min: String,
    /// `aria-valuemax` — maximum value for range widgets.
    pub value_max: String,
    /// `aria-valuetext` — human-readable value for range widgets.
    pub value_text: String,
}

// ── AXNode ───────────────────────────────────────────────────────────────────

/// One node in the accessibility tree.
///
/// Mirrors the DOM tree but carries semantic information rather than layout
/// geometry. Platform bridges map this to OS-specific accessibility APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AXNode {
    /// Corresponding DOM node identifier.
    pub node_id: NodeId,
    /// Semantic role: implicit from HTML tag or explicit via `role="..."`.
    pub role: AXRole,
    /// Computed accessible name (WAI-ARIA Accessible Name and Description §4).
    pub name: String,
    /// Computed accessible description (aria-describedby / title).
    pub description: String,
    /// Placeholder text for text inputs (`placeholder` attribute).
    pub placeholder: String,
    /// ARIA state and property flags.
    pub state: AXState,
    /// Direct children in the accessibility tree (aria-hidden subtrees excluded).
    pub children: Vec<AXNode>,
    /// `aria-controls` — NodeId of element controlled by this one. None if absent.
    pub controls: Option<NodeId>,
    /// `aria-owns` — NodeIds of elements owned by this one. Order preserved per spec.
    pub owns: Vec<NodeId>,
    /// `aria-flowto` — NodeIds indicating reading order continuation. Order preserved.
    pub flow_to: Vec<NodeId>,
    /// `aria-details` — NodeId of element with additional details. None if absent.
    pub details: Option<NodeId>,
}

// ── AXTree ───────────────────────────────────────────────────────────────────

/// Accessibility tree rooted at a document node.
///
/// Built by `build_ax_tree`. Contains one `AXNode` per semantically meaningful
/// DOM element, in document order. `aria-hidden` subtrees are omitted entirely.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AXTree {
    /// Root accessibility node — typically the `<body>` element.
    pub root: AXNode,
}

// ── Tree builder ─────────────────────────────────────────────────────────────

/// Build an `AXTree` from a `Document` starting at `root_id`.
///
/// Use the `<body>` NodeId for a normal page. `aria-hidden="true"` subtrees
/// and pure comment/text nodes are excluded from the result.
///
/// The `flat_tree` parameter handles Shadow DOM composed tree traversal:
/// instead of following DOM children, `build_node` uses `flat_tree.children_of()`
/// to respect slot assignments and shadow boundaries.
pub fn build_ax_tree(doc: &Document, root_id: NodeId, flat_tree: &FlatTree) -> AXTree {
    AXTree {
        root: build_node(doc, root_id, None, flat_tree),
    }
}

fn build_node(doc: &Document, node_id: NodeId, parent_role: Option<AXRole>, flat_tree: &FlatTree) -> AXNode {
    let node = doc.get(node_id);
    let state = compute_state(node);
    let role = resolve_role(node, parent_role);
    let name = names::compute_name(doc, node_id);
    let description = names::compute_description(doc, node_id);
    let placeholder = node.get_attr("placeholder").unwrap_or("").to_owned();

    // Use flat_tree to get composed children (respects shadow DOM boundaries and slot assignments)
    let composed_children = flat_tree.children_of(doc, node_id);
    let children = composed_children
        .iter()
        .filter(|&&child_id| {
            let child = doc.get(child_id);
            // Collapse pure text/comment nodes — their content is in the parent name.
            if matches!(child.data, NodeData::Text(_) | NodeData::Comment(_)) {
                return false;
            }
            // Exclude aria-hidden subtrees from the AX tree.
            !child
                .get_attr("aria-hidden")
                .is_some_and(|v| v.eq_ignore_ascii_case("true"))
        })
        .map(|&child_id| {
            // Transparent roles (Presentation, None, Generic, Group) don't participate in child role validation.
            // Children should skip these and see the actual semantic parent for context validation.
            let effective_parent_role = if matches!(role, AXRole::Presentation | AXRole::None | AXRole::Generic | AXRole::Group) {
                parent_role
            } else {
                Some(role)
            };
            build_node(doc, child_id, effective_parent_role, flat_tree)
        })
        .collect();

    // Resolve aria-* relationship attributes using document id lookups
    let controls = node
        .get_attr("aria-controls")
        .and_then(|id_ref| doc.find_by_id(id_ref));

    let owns = if let Some(id_list) = node.get_attr("aria-owns") {
        id_list
            .split_ascii_whitespace()
            .filter_map(|id_ref| doc.find_by_id(id_ref))
            .collect()
    } else {
        Vec::new()
    };

    let flow_to = if let Some(id_list) = node.get_attr("aria-flowto") {
        id_list
            .split_ascii_whitespace()
            .filter_map(|id_ref| doc.find_by_id(id_ref))
            .collect()
    } else {
        Vec::new()
    };

    let details = node
        .get_attr("aria-details")
        .and_then(|id_ref| doc.find_by_id(id_ref));

    AXNode { node_id, role, name, description, placeholder, state, children, controls, owns, flow_to, details }
}

fn resolve_role(node: &lumen_dom::Node, parent_role: Option<AXRole>) -> AXRole {
    // Step 1: Check explicit role attribute and validate against parent context.
    if let Some(role_attr) = node.get_attr("role") {
        // The `role` attribute is a space-separated list; take the first valid value.
        for token in role_attr.split_ascii_whitespace() {
            if let Some(r) = AXRole::parse(token) {
                // Validate role in parent context. If invalid, fall back to implicit.
                if is_role_valid_in_context(r, parent_role) {
                    return r;
                }
            }
        }
    }
    // Step 2: Fall back to implicit role from HTML tag.
    implicit_role(node)
}

fn compute_state(node: &lumen_dom::Node) -> AXState {
    AXState {
        checked: checked_state(node),
        disabled: node
            .get_attr("aria-disabled")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
            || node.get_attr("disabled").is_some(),
        expanded: parse_bool_attr(node, "aria-expanded"),
        hidden: node
            .get_attr("aria-hidden")
            .is_some_and(|v| v.eq_ignore_ascii_case("true")),
        selected: parse_bool_attr(node, "aria-selected"),
        pressed: parse_bool_attr(node, "aria-pressed"),
        required: node
            .get_attr("aria-required")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
            || node.get_attr("required").is_some(),
        readonly: node
            .get_attr("aria-readonly")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
            || node.get_attr("readonly").is_some(),
        invalid: node
            .get_attr("aria-invalid")
            .is_some_and(|v| v.eq_ignore_ascii_case("true")),
        busy: node
            .get_attr("aria-busy")
            .is_some_and(|v| v.eq_ignore_ascii_case("true")),
        tab_index: node
            .get_attr("tabindex")
            .and_then(|v| v.trim().parse::<i32>().ok()),
        level: node
            .get_attr("aria-level")
            .and_then(|v| v.trim().parse::<u32>().ok())
            .or_else(|| {
                node.element_name().and_then(|n| match n.local.as_str() {
                    "h1" => Some(1),
                    "h2" => Some(2),
                    "h3" => Some(3),
                    "h4" => Some(4),
                    "h5" => Some(5),
                    "h6" => Some(6),
                    _ => None,
                })
            }),
        set_size: node
            .get_attr("aria-setsize")
            .and_then(|v| v.trim().parse::<u32>().ok()),
        pos_in_set: node
            .get_attr("aria-posinset")
            .and_then(|v| v.trim().parse::<u32>().ok()),
        live: match node.get_attr("aria-live") {
            Some(v) if v.eq_ignore_ascii_case("polite") => Some(LiveRegion::Polite),
            Some(v) if v.eq_ignore_ascii_case("assertive") => Some(LiveRegion::Assertive),
            _ => None,
        },
        multiline: node
            .get_attr("aria-multiline")
            .is_some_and(|v| v.eq_ignore_ascii_case("true")),
        multiselectable: node
            .get_attr("aria-multiselectable")
            .is_some_and(|v| v.eq_ignore_ascii_case("true")),
        horizontal: match node.get_attr("aria-orientation") {
            Some(v) if v.eq_ignore_ascii_case("horizontal") => Some(true),
            Some(v) if v.eq_ignore_ascii_case("vertical") => Some(false),
            _ => None,
        },
        current: match node.get_attr("aria-current") {
            Some(v) if v.eq_ignore_ascii_case("page") => Some(AriaCurrent::Page),
            Some(v) if v.eq_ignore_ascii_case("step") => Some(AriaCurrent::Step),
            Some(v) if v.eq_ignore_ascii_case("location") => Some(AriaCurrent::Location),
            Some(v) if v.eq_ignore_ascii_case("date") => Some(AriaCurrent::Date),
            Some(v) if v.eq_ignore_ascii_case("time") => Some(AriaCurrent::Time),
            Some(v) if v.eq_ignore_ascii_case("true") => Some(AriaCurrent::Page),
            _ => None,
        },
        modal: node
            .get_attr("aria-modal")
            .is_some_and(|v| v.eq_ignore_ascii_case("true")),
        role_description: node.get_attr("aria-roledescription").unwrap_or("").to_owned(),
        value_now: node.get_attr("aria-valuenow").unwrap_or("").to_owned(),
        value_min: node.get_attr("aria-valuemin").unwrap_or("").to_owned(),
        value_max: node.get_attr("aria-valuemax").unwrap_or("").to_owned(),
        value_text: node.get_attr("aria-valuetext").unwrap_or("").to_owned(),
    }
}

fn checked_state(node: &lumen_dom::Node) -> Option<Option<bool>> {
    match node.get_attr("aria-checked") {
        Some(v) if v.eq_ignore_ascii_case("true") => Some(Some(true)),
        Some(v) if v.eq_ignore_ascii_case("false") => Some(Some(false)),
        Some(v) if v.eq_ignore_ascii_case("mixed") => Some(None),
        Some(_) | None => {
            let is_checkable = node
                .input_type()
                .is_some_and(|t| matches!(t, InputType::Checkbox | InputType::Radio));
            if is_checkable {
                Some(Some(node.get_attr("checked").is_some()))
            } else {
                None
            }
        }
    }
}

/// Parse a boolean ARIA attribute (`"true"` / `"false"`). Returns `None` if absent.
fn parse_bool_attr(node: &lumen_dom::Node, attr: &str) -> Option<bool> {
    match node.get_attr(attr) {
        Some(v) if v.eq_ignore_ascii_case("true") => Some(true),
        Some(v) if v.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

/// Check if a role is valid in the given parent role context per HTML-AAM.
///
/// Some roles have strict parent requirements (e.g., `row` requires a `table`-like parent).
/// If parent_role is `None`, the role is at the document root and most roles are allowed.
/// Returns `true` if the role is permitted in this context.
fn is_role_valid_in_context(role: AXRole, parent_role: Option<AXRole>) -> bool {
    use crate::roles::AXRole;

    let parent = match parent_role {
        Some(p) => p,
        None => return true, // Most roles allowed at document root
    };

    match role {
        // Table-related roles require table parent
        AXRole::Cell | AXRole::ColumnHeader | AXRole::RowHeader => {
            parent == AXRole::Row
        }
        AXRole::Row => {
            parent == AXRole::Table
        }

        // List item requires list parent
        AXRole::ListItem => {
            parent == AXRole::List
        }

        // Menu item requires menu parent
        AXRole::MenuItem => {
            parent == AXRole::Menu
        }

        // Tab requires tablist parent
        AXRole::Tab => {
            parent == AXRole::TabList
        }

        // Option requires listbox parent
        AXRole::Option => {
            parent == AXRole::ListBox
        }

        // TreeItem requires tree parent
        AXRole::TreeItem => {
            parent == AXRole::Tree
        }

        // By default, most roles are allowed anywhere (permissive fallback)
        _ => true,
    }
}
