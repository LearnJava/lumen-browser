//! Platform accessibility bridges.
//!
//! Each OS has a native accessibility API consumed by screen readers:
//! - Windows: UI Automation (UIA) COM interface — NVDA, JAWS, Narrator
//! - macOS: NSAccessibility protocol — VoiceOver
//! - Linux: AT-SPI2 over D-Bus — Orca
//!
//! Phase 0: stubs that accept [`AXTree`] updates and maintain last-known state.
//! Phase 1 (per-platform): native bindings without adding new Cargo deps —
//! see the `// Phase 1:` comments inside each platform module.

pub mod linux;
pub mod macos;
pub mod windows;

use crate::{AXTree, NodeId};

// ── PlatformBridge trait ─────────────────────────────────────────────────────

/// Trait for platform-specific accessibility bridges.
///
/// The shell calls these methods whenever the accessibility tree changes.
/// Each platform translates the Lumen `AXTree` to the OS accessibility model.
pub trait PlatformBridge: Send + 'static {
    /// Rebuild the OS accessibility tree from the new Lumen `AXTree`.
    ///
    /// Called after every page navigation and significant DOM mutation.
    fn update(&mut self, tree: &AXTree);

    /// Notify the OS that keyboard focus moved to `node_id`.
    ///
    /// `None` means focus left the web content area (e.g. address bar active).
    fn focused_node_changed(&mut self, node_id: Option<NodeId>);

    /// Release OS resources; called when the browser window closes.
    fn shutdown(&mut self);
}

// ── NullBridge ───────────────────────────────────────────────────────────────

/// No-op bridge for headless runs, tests, and unsupported platforms.
pub struct NullBridge;

impl PlatformBridge for NullBridge {
    fn update(&mut self, _tree: &AXTree) {}
    fn focused_node_changed(&mut self, _node_id: Option<NodeId>) {}
    fn shutdown(&mut self) {}
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// Create the platform bridge appropriate for the current OS.
///
/// On Windows returns [`windows::WinUiaBridge`], on macOS [`macos::MacA11yBridge`],
/// on Linux [`linux::AtSpiBridge`]. Headless / other platforms return [`NullBridge`].
pub fn platform_bridge() -> Box<dyn PlatformBridge> {
    platform_bridge_impl()
}

#[cfg(target_os = "windows")]
fn platform_bridge_impl() -> Box<dyn PlatformBridge> {
    Box::new(windows::WinUiaBridge::new())
}

#[cfg(target_os = "macos")]
fn platform_bridge_impl() -> Box<dyn PlatformBridge> {
    Box::new(macos::MacA11yBridge::new())
}

#[cfg(target_os = "linux")]
fn platform_bridge_impl() -> Box<dyn PlatformBridge> {
    Box::new(linux::AtSpiBridge::new())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn platform_bridge_impl() -> Box<dyn PlatformBridge> {
    Box::new(NullBridge)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AXNode, AXRole, AXState};
    use lumen_dom::NodeId;

    fn dummy_tree() -> AXTree {
        AXTree {
            root: AXNode {
                node_id: NodeId::from_index(0),
                role: AXRole::Document,
                name: String::new(),
                description: String::new(),
                placeholder: String::new(),
                state: AXState::default(),
                children: Vec::new(),
                controls: None,
                owns: Vec::new(),
                flow_to: Vec::new(),
                details: None,
            },
        }
    }

    #[test]
    fn null_bridge_update_no_crash() {
        let mut b = NullBridge;
        b.update(&dummy_tree());
        b.focused_node_changed(None);
        b.focused_node_changed(Some(NodeId::from_index(1)));
        b.shutdown();
    }

    #[test]
    fn win_uia_bridge_stores_last_tree() {
        let mut b = windows::WinUiaBridge::new();
        assert!(b.last_tree().is_none());
        let tree = dummy_tree();
        b.update(&tree);
        assert!(b.last_tree().is_some());
        assert_eq!(b.last_tree().unwrap().root.role, AXRole::Document);
    }

    #[test]
    fn mac_bridge_stores_last_tree() {
        let mut b = macos::MacA11yBridge::new();
        assert!(b.last_tree().is_none());
        b.update(&dummy_tree());
        assert!(b.last_tree().is_some());
    }

    #[test]
    fn linux_bridge_stores_last_tree() {
        let mut b = linux::AtSpiBridge::new();
        assert!(b.last_tree().is_none());
        b.update(&dummy_tree());
        assert!(b.last_tree().is_some());
    }

    #[test]
    fn all_bridges_track_focused_node() {
        let nid = NodeId::from_index(42);

        let mut w = windows::WinUiaBridge::new();
        w.focused_node_changed(Some(nid));
        assert_eq!(w.focused_node(), Some(nid));
        w.focused_node_changed(None);
        assert_eq!(w.focused_node(), None);

        let mut m = macos::MacA11yBridge::new();
        m.focused_node_changed(Some(nid));
        assert_eq!(m.focused_node(), Some(nid));

        let mut l = linux::AtSpiBridge::new();
        l.focused_node_changed(Some(nid));
        assert_eq!(l.focused_node(), Some(nid));
    }

    #[test]
    fn platform_bridge_returns_bridge() {
        // Just ensures the factory doesn't panic on the current OS.
        let mut b = platform_bridge();
        b.update(&dummy_tree());
        b.shutdown();
    }
}
