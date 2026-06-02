//! Panel/Surface system — the shell UI foundation (ADR-009).
//!
//! Every shell UI block (tab tree, address bar, bookmark popover, command
//! palette, privacy dashboard…) is a [`Panel`]: it declares *where* it lives via
//! [`Surface`], *how big* it wants to be via [`SizeRule`], paints itself into a
//! given rect, and reacts to events by returning an [`EventResponse`].  The
//! [`SurfaceManager`] is the single coordinator: it owns the docked layout tree
//! and the float layer, composites every visible panel into one
//! [`lumen_paint::DisplayList`], and routes OS input to the correct panel.
//!
//! Design rationale and the full specification:
//! [`docs/decisions/ADR-009-shell-panel-system.md`] and
//! [`docs/shell-ui-architecture.md`].
//!
//! ## Scope of this module (Phase-1 foundation)
//!
//! This is the *infrastructure* — the trait, the value types, and a working
//! `SurfaceManager` that lays out docked slots, positions float panels,
//! composites paint output, and routes mouse events.  It is intentionally
//! self-contained and does **not** yet replace the existing ad-hoc panels in
//! [`crate::panels`]; migration happens one panel at a time in follow-up tasks.
//! `Surface::OsWindow` is modelled but not composited (no second `winit` window
//! yet); `SizeRule::Content` measurement and modal backdrops are stubbed.

mod ctx;
mod manager;
mod theme;
mod types;

pub use ctx::{EventCtx, PaintCtx};
pub use manager::{LayoutNode, SlotRect, SurfaceManager};
pub use theme::Theme;
pub use types::{
    rect_contains, Command, Corner, CursorIcon, EventResponse, FloatAnchor, HitElement, HitTarget,
    MouseButton, PanelEvent, ScrollDelta, SizeRule, Surface,
};

use lumen_core::geom::{Point, Rect};
use lumen_paint::DisplayList;

/// A self-contained shell UI block.
///
/// A panel knows only itself: it does not know where on screen it sits, who its
/// neighbours are, or how many panels exist.  It receives a rectangle and paints
/// itself into it; it receives events and returns intents.  All required methods
/// are deliberately small and single-purpose; lifecycle hooks have no-op
/// defaults.
pub trait Panel: 'static {
    /// Unique, stable identifier (used for show/hide/focus by name).
    ///
    /// Examples: `"tab-tree"`, `"address-bar"`, `"command-palette"`.
    fn id(&self) -> &'static str;

    /// Where and how the panel appears (docked slot / float / modal / window).
    fn surface(&self) -> Surface;

    /// Desired width along the cross axis of its container.
    fn width(&self) -> SizeRule;

    /// Desired height.
    fn height(&self) -> SizeRule;

    /// Paint the panel into `ctx.rect`, returning draw commands.
    ///
    /// Called only when the panel is dirty (state changed, animating, hovered) —
    /// not every frame.
    fn paint(&self, ctx: &PaintCtx) -> DisplayList;

    /// What lies under `pos` (panel-local coordinates)?  `None` = empty space.
    fn hit_test(&self, pos: Point) -> Option<HitTarget> {
        let _ = pos;
        None
    }

    /// Handle an event and return what the manager should do next.
    fn on_event(&mut self, event: &PanelEvent, ctx: &mut EventCtx) -> EventResponse;

    /// Whether the panel wants keyboard input (address bar, command palette…).
    fn accepts_focus(&self) -> bool {
        false
    }

    /// The panel was just registered; load initial data, start intro animation.
    fn on_mount(&mut self, _ctx: &mut EventCtx) {}

    /// The panel is being removed; persist state, stop timers.
    fn on_unmount(&mut self) {}

    /// The window or the panel's slot changed size.
    fn on_resize(&mut self, _new_rect: Rect) {}

    /// The panel gained keyboard focus.
    fn on_focus(&mut self) {}

    /// The panel lost keyboard focus.
    fn on_blur(&mut self) {}
}
