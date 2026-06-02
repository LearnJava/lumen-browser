//! `SurfaceManager` — coordinates docked layout tree, float layer, paint composite
//! and OS event routing (ADR-009).
//!
//! This is a **stub**: the data structures and public API are defined so dependents
//! can compile.  The implementations are `todo!()` and will be filled in as the
//! panel-system task progresses.

use lumen_core::geom::Rect;
use lumen_paint::DisplayList;

use super::types::Surface;
use super::Panel;

/// A node in the docked layout tree.
///
/// Each node corresponds to a named slot (`"left"`, `"right"`, `"top"`,
/// `"bottom"`, `"content"`).  Slots are arranged in a simple HSplit / VSplit
/// tree; the manager resolves concrete rects once the window size is known.
pub struct LayoutNode {
    /// Slot identifier matching [`Surface::Docked::slot`].
    pub slot: &'static str,
    /// Resolved rect in window coordinates (filled after layout).
    pub rect: Rect,
    /// Child nodes, if this is a splitter.
    pub children: Vec<LayoutNode>,
}

/// Resolved window-space rect for a slot, returned by
/// [`SurfaceManager::slot_rect`].
pub struct SlotRect {
    /// Slot identifier.
    pub slot: &'static str,
    /// Resolved rect in window coordinates.
    pub rect: Rect,
}

/// Single coordinator for all shell UI panels (ADR-009 §SurfaceManager).
///
/// Owns the docked layout tree and the float layer, composites every visible
/// panel into one [`DisplayList`], and routes OS input to the correct panel.
///
/// **Stub** — full implementation in progress.  The `register` / `composite` /
/// `slot_rect` surface are defined; all bodies are `todo!()`.
pub struct SurfaceManager {
    panels: Vec<Box<dyn Panel>>,
    window_size: (f32, f32),
}

impl SurfaceManager {
    /// Create an empty manager for a window of the given size (CSS px).
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            panels: Vec::new(),
            window_size: (width, height),
        }
    }

    /// Register a panel.  It is placed according to [`Panel::surface`].
    pub fn register(&mut self, _panel: Box<dyn Panel>) {
        todo!("SurfaceManager::register — panel-system task in progress")
    }

    /// Composite all visible panels into a single `DisplayList` for the renderer.
    pub fn composite(&self) -> DisplayList {
        todo!("SurfaceManager::composite — panel-system task in progress")
    }

    /// Return the resolved rect for a named docked slot.
    pub fn slot_rect(&self, _slot: &str) -> Option<SlotRect> {
        todo!("SurfaceManager::slot_rect — panel-system task in progress")
    }

    /// Notify the manager that the window was resized.
    pub fn on_resize(&mut self, width: f32, height: f32) {
        self.window_size = (width, height);
    }

    /// Show or hide a panel by id.
    pub fn set_visible(&mut self, _id: &str, _visible: bool) {
        todo!("SurfaceManager::set_visible — panel-system task in progress")
    }

    /// Whether a panel with the given id is registered.
    pub fn has_panel(&self, id: &str) -> bool {
        self.panels.iter().any(|p| p.id() == id)
    }

    /// How many panels are registered.
    pub fn panel_count(&self) -> usize {
        self.panels.len()
    }

    /// Current window size (CSS px).
    pub fn window_size(&self) -> (f32, f32) {
        self.window_size
    }
}

/// `SurfaceManager` is `Send` because it is driven from the winit event loop
/// on a single thread; no cross-thread access occurs.
unsafe impl Send for SurfaceManager {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_manager_is_empty() {
        let mgr = SurfaceManager::new(1024.0, 768.0);
        assert_eq!(mgr.panel_count(), 0);
        assert!(!mgr.has_panel("tab-tree"));
        assert_eq!(mgr.window_size(), (1024.0, 768.0));
    }

    #[test]
    fn on_resize_updates_size() {
        let mut mgr = SurfaceManager::new(1024.0, 768.0);
        mgr.on_resize(1920.0, 1080.0);
        assert_eq!(mgr.window_size(), (1920.0, 1080.0));
    }

    // Note: register/composite/slot_rect tests will be added as the
    // SurfaceManager implementation progresses (panel-system task).
    // Calling them now would panic on todo!().
}
