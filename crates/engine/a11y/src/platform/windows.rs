//! Windows UI Automation bridge — Phase 0 stub.
//!
//! Phase 0: holds the last-known AXTree in memory. No OS calls.
//!
//! Phase 1 (no new Cargo deps required — `windows` crate already a transitive dep
//! via winit in lumen-shell):
//!   - Register an `IRawElementProviderSimple` COM server on the HWND via
//!     `UiaReturnRawElementProvider` (Windows::UI::Accessibility).
//!   - Map each `AXNode` to an `IAccessible2` / UIA element with:
//!     - `get_accRole` → `AXRole` → ROLE_SYSTEM_* constant
//!     - `get_accName` / `get_accDescription` → node name / description
//!     - `get_accState` → `AXState` flags → STATE_SYSTEM_* bitmask
//!   - Fire `UiaRaiseAutomationEvent(UIA_FocusChangedEventId)` on focus change.
//!   - Fire `UiaRaiseStructureChangedEvent` on tree mutation.

use crate::{AXTree, NodeId};
use super::PlatformBridge;

// ── WinUiaBridge ─────────────────────────────────────────────────────────────

/// Windows UI Automation bridge.
///
/// Phase 0: no-op — accepts updates and stores the last tree for inspection.
/// Phase 1: expose an `IRawElementProviderSimple` COM provider on the shell HWND.
pub struct WinUiaBridge {
    /// Most recent accessibility tree received from the shell. `None` before the first page load.
    tree: Option<AXTree>,
    /// Currently focused DOM node. `None` if focus is outside web content.
    focused: Option<NodeId>,
}

impl WinUiaBridge {
    /// Create a new, uninitialized UIA bridge.
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

impl Default for WinUiaBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformBridge for WinUiaBridge {
    fn update(&mut self, tree: &AXTree) {
        // Phase 0: clone tree into local storage.
        // Phase 1: diff against previous tree and fire UiaRaiseStructureChangedEvent
        // for each added/removed subtree; update COM element map.
        self.tree = Some(tree.clone());
    }

    fn focused_node_changed(&mut self, node_id: Option<NodeId>) {
        // Phase 0: store focused node.
        // Phase 1: call UiaRaiseAutomationEvent(provider, UIA_FocusChangedEventId).
        self.focused = node_id;
    }

    fn shutdown(&mut self) {
        // Phase 0: drop tree.
        // Phase 1: revoke COM registration via UiaDisconnectProvider.
        self.tree = None;
        self.focused = None;
    }
}
