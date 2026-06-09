//! Linux AT-SPI2 bridge — Phase 0 stub.
//!
//! Phase 0: holds the last-known AXTree in memory. No OS calls.
//!
//! Phase 1 (requires `atspi` + `zbus` crates — add only when implementing):
//!   - Connect to the AT-SPI2 D-Bus session bus (accessibility-enabled session).
//!   - Register as an AT-SPI2 Application object on the D-Bus.
//!   - Expose each `AXNode` as an `Accessible` object implementing:
//!     - `org.a11y.atspi.Accessible` (name, description, role, state-set, children)
//!     - `org.a11y.atspi.Component` (bounds, layer)
//!     - `org.a11y.atspi.Text` for text roles (character count, text, caret offset)
//!   - Emit `object:state-changed:focused` signal on focus change.
//!   - Emit `object:children-changed` on tree mutation.
//!
//! AT-SPI2 role mapping (incomplete — expand in Phase 1):
//!   AXRole::Button          → atspi::Role::PushButton
//!   AXRole::Link            → atspi::Role::Link
//!   AXRole::Heading         → atspi::Role::Heading
//!   AXRole::TextInput       → atspi::Role::Entry
//!   AXRole::ListItem        → atspi::Role::ListItem
//!   AXRole::Document        → atspi::Role::DocumentWeb

use crate::{AXTree, NodeId};
use super::PlatformBridge;

// ── AtSpiBridge ──────────────────────────────────────────────────────────────

/// Linux AT-SPI2 accessibility bridge.
///
/// Phase 0: no-op — accepts updates and stores the last tree for inspection.
/// Phase 1: expose an AT-SPI2 D-Bus service for Orca and other Linux screen readers.
pub struct AtSpiBridge {
    /// Most recent accessibility tree received from the shell. `None` before the first page load.
    tree: Option<AXTree>,
    /// Currently focused DOM node. `None` if focus is outside web content.
    focused: Option<NodeId>,
}

impl AtSpiBridge {
    /// Create a new, uninitialized AT-SPI2 bridge.
    pub fn new() -> Self {
        Self { tree: None, focused: None }
    }

    /// Return the last-received accessibility tree, if any.
    pub fn last_tree(&self) -> Option<&AXTree> {
        self.tree.as_ref()
    }

    /// Return the currently focused node, if any.
    pub fn focused_node(&self) -> Option<NodeId> {
        self.focused
    }
}

impl Default for AtSpiBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformBridge for AtSpiBridge {
    fn update(&mut self, tree: &AXTree) {
        // Phase 0: clone tree into local storage.
        // Phase 1: diff tree and emit object:children-changed D-Bus signals.
        self.tree = Some(tree.clone());
    }

    fn focused_node_changed(&mut self, node_id: Option<NodeId>) {
        // Phase 0: store focused node.
        // Phase 1: emit object:state-changed:focused D-Bus signal via zbus.
        self.focused = node_id;
    }

    fn shutdown(&mut self) {
        // Phase 0: drop tree.
        // Phase 1: unregister from D-Bus session bus.
        self.tree = None;
        self.focused = None;
    }
}
