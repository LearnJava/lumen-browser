//! Whether a press landed on a CSS `resize` grip, and which axes that grip
//! may change.
//!
//! The grip's geometry belongs to paint (`lumen_paint::point_on_resize_grip`)
//! and the drag that follows is tracked by `crate::app::window_event`; what is
//! here is the one lookup between them - the layout-tree walk that answers
//! which node was grabbed, with `resize` already resolved from logical to
//! physical axes so the caller knows whether width, height or both may move.

use crate::*;

impl Lumen {
    /// Finds a layout box with a resize grip at position (x, y) in the layout tree.
    /// Returns `(node_id, allow_width, allow_height)` — the latter two are the box's
    /// `resize` value resolved to physical axes (CC-CSS-4: `Resize::allowed_axes`,
    /// writing-mode aware), so the caller knows which dimension(s) a drag from this
    /// grip is allowed to change. Returns `None` if no grip is found.
    /// This is used in B-7: CSS Resize property Phase 1 to detect mouse clicks on grips.
    pub(crate) fn find_resize_grip_node(
        &self,
        b: &lumen_layout::LayoutBox,
        x: f32,
        y: f32,
    ) -> Option<(lumen_dom::NodeId, bool, bool)> {
        // Check this box first
        if lumen_paint::point_on_resize_grip(b, x, y) {
            let (allow_w, allow_h) = b.style.resize.allowed_axes(b.style.writing_mode);
            return Some((b.node, allow_w, allow_h));
        }

        // Recursively check children
        for child in &b.children {
            if let Some(hit) = self.find_resize_grip_node(child, x, y) {
                return Some(hit);
            }
        }

        None
    }
}
