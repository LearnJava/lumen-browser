//! HTML5 drag-and-drop gesture state (PH3-9), tracked by the shell's own mouse
//! handling in [`crate::app::window_event`].
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

// ── HTML5 Drag and Drop state (PH3-9) ────────────────────────────────────────
// ── HTML5 Drag and Drop state (PH3-9) ────────────────────────────────────────

/// Minimum cursor displacement in CSS pixels before a press becomes a drag.
pub(crate) const DND_THRESHOLD: f32 = 4.0;

/// State for an in-progress HTML5 drag-and-drop gesture (HTML LS §9.3.3, §9.10).
///
/// Created on `mousedown` when the pressed element is draggable.  Becomes
/// `active` once the cursor moves ≥ `DND_THRESHOLD` px (`dragstart` fires).
/// Cleared on `mouseup` after firing `drop` + `dragend`.
pub(crate) struct DndState {
    /// DOM node that is being dragged (source of `dragstart`/`drag`/`dragend`).
    pub(crate) src_nid: lumen_dom::NodeId,
    /// CSS-pixel coordinates where the mouse button was pressed.
    pub(crate) press_x: f32,
    /// CSS-pixel coordinates where the mouse button was pressed.
    pub(crate) press_y: f32,
    /// Whether the drag has been activated (threshold crossed, `dragstart` fired).
    pub(crate) active: bool,
    /// Drop target currently under the cursor — used to synthesise
    /// `dragenter` / `dragleave` when the target changes.
    pub(crate) over_nid: Option<lumen_dom::NodeId>,
}
