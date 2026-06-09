//! macOS NSAccessibility bridge — Phase 0 stub.
//!
//! Phase 0: holds the last-known AXTree in memory. No OS calls.
//!
//! Phase 1 (requires `objc2` or `cocoa` crate — add only when implementing):
//!   - Register an `NSAccessibilityElement` for the WKWebView content area.
//!   - Map each `AXNode` to an `NSAccessibilityElement` with:
//!     - `accessibilityRole` → `NSAccessibilityRole` constant (e.g. `NSAccessibilityButtonRole`)
//!     - `accessibilityLabel` / `accessibilityHelp` → node name / description
//!     - `accessibilityValue` → `aria-valuenow` / `aria-valuetext`
//!     - `isAccessibilityFocused` → `focused_node == node_id`
//!   - Call `NSAccessibilityPostNotification(element, NSAccessibilityFocusedUIElementChangedNotification)`
//!     on focus change.
//!   - Call `NSAccessibilityPostNotification(element, NSAccessibilityLayoutChangedNotification)`
//!     on tree mutation.

use crate::{AXTree, NodeId};
use super::PlatformBridge;

// ── MacA11yBridge ─────────────────────────────────────────────────────────────

/// macOS NSAccessibility bridge.
///
/// Phase 0: no-op — accepts updates and stores the last tree for inspection.
/// Phase 1: post `NSAccessibilityNotification` events via `objc2`.
pub struct MacA11yBridge {
    /// Most recent accessibility tree received from the shell. `None` before the first page load.
    tree: Option<AXTree>,
    /// Currently focused DOM node. `None` if focus is outside web content.
    focused: Option<NodeId>,
}

impl MacA11yBridge {
    /// Create a new, uninitialized NSAccessibility bridge.
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

impl Default for MacA11yBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformBridge for MacA11yBridge {
    fn update(&mut self, tree: &AXTree) {
        // Phase 0: clone tree into local storage.
        // Phase 1: diff tree and post NSAccessibilityLayoutChangedNotification.
        self.tree = Some(tree.clone());
    }

    fn focused_node_changed(&mut self, node_id: Option<NodeId>) {
        // Phase 0: store focused node.
        // Phase 1: post NSAccessibilityFocusedUIElementChangedNotification.
        self.focused = node_id;
    }

    fn shutdown(&mut self) {
        // Phase 0: drop tree.
        // Phase 1: unregister NSAccessibilityElement from the accessibility hierarchy.
        self.tree = None;
        self.focused = None;
    }
}
