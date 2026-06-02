//! Core value types for the Panel/Surface system (ADR-009).
//!
//! These describe *where* a panel lives ([`Surface`]), *how big* it wants to be
//! ([`SizeRule`]), what *events* it receives ([`PanelEvent`]), what it *returns*
//! ([`EventResponse`] / [`Command`]) and what is *under the cursor*
//! ([`HitTarget`]).  None of these types depend on the OS, `winit`, or `wgpu`.
//!
//! This is the Phase-1 foundation: a pragmatic subset of the full specification
//! in [`docs/shell-ui-architecture.md`].  Variants are added as panels are
//! migrated onto the system; the existing ad-hoc panels remain untouched.

use lumen_core::geom::{Point, Rect};
use lumen_layout::Color;

// ── Surface ──────────────────────────────────────────────────────────────────

/// Where and how a panel appears on screen.
///
/// A panel declares exactly one `Surface`; [`super::SurfaceManager`] uses it to
/// decide whether the panel occupies a slot in the docked layout tree, floats
/// over the page, or is shown as a centred modal.
///
/// `OsWindow` (a real second `winit` window) is represented but not yet
/// composited by the foundation manager — multi-window support is deferred to
/// the event-loop refactor.  Storing the variant lets panels declare their
/// intent today without waiting for that work.
#[derive(Debug, Clone, PartialEq)]
pub enum Surface {
    /// Pinned to a named slot in the docked layout tree.  Size comes from the
    /// panel's [`SizeRule`]s and the space the slot was given.
    Docked {
        /// Slot identifier, e.g. `"left"`, `"right"`, `"bottom"`, `"content"`.
        slot: &'static str,
    },

    /// Floats over the page on the overlay layer; does not occupy layout space.
    Float {
        /// Where the float is positioned relative to the window / a rect.
        anchor: FloatAnchor,
        /// Higher draws on top.  Tooltip(1000) renders above Menu(500).
        z_order: i32,
        /// Dismiss the panel when the user clicks outside its rect.
        close_on_outside_click: bool,
    },

    /// A real separate OS window (Picture-in-Picture, detached DevTools…).
    ///
    /// Declared for completeness; the foundation manager records but does not
    /// render these (no second `winit` window yet).
    OsWindow {
        /// Window title (`""` for chromeless windows like PiP).
        title: String,
        /// Initial inner size in physical px.
        size: (u32, u32),
        /// Keep the window above all others (PiP).
        always_on_top: bool,
        /// Draw OS frame + buttons (`false` = chromeless).
        decorations: bool,
    },

    /// A centred modal dialog that dims the background and blocks input to all
    /// other panels until dismissed.
    Modal {
        /// Dismiss when the dimmed backdrop is clicked.
        closable_on_backdrop: bool,
        /// Backdrop dim colour (typically `rgba(0,0,0,0.5)`).
        backdrop_color: Color,
    },
}

impl Surface {
    /// `true` for [`Surface::Docked`].
    pub fn is_docked(&self) -> bool {
        matches!(self, Surface::Docked { .. })
    }

    /// `true` for floats and modals (anything on the overlay layer).
    pub fn is_overlay(&self) -> bool {
        matches!(self, Surface::Float { .. } | Surface::Modal { .. })
    }
}

/// Window corner, used by [`FloatAnchor::Corner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Where a [`Surface::Float`] panel is positioned.
///
/// The foundation manager resolves `Corner`, `Center`, `Below`, `Above`,
/// `Absolute`; `Cursor` and `AnchoredTo` are recorded for callers that track
/// those positions themselves.
#[derive(Debug, Clone, PartialEq)]
pub enum FloatAnchor {
    /// Next to the mouse cursor (context menus, tooltips).
    Cursor,
    /// Below the given rect; flips above automatically if it would overflow.
    Below(Rect),
    /// Above the given rect.
    Above(Rect),
    /// In a window corner (mini-player, notifications).
    Corner(Corner),
    /// Centred in the window (command palette, search).
    Center,
    /// Exact window-space coordinates.
    Absolute(Point),
}

// ── SizeRule ─────────────────────────────────────────────────────────────────

/// How a panel (or slot) describes its desired extent along one axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeRule {
    /// Exactly N px — never grows or shrinks.
    Fixed(f32),
    /// Take all remaining space after fixed siblings are allocated.
    Flex,
    /// Sized to content (panel supplies the measurement); treated as `Fixed(0)`
    /// by the foundation until a `content_size` hook is added.
    Content,
    /// Clamp to `[min, max]`, preferring `default` when space allows.
    Range { min: f32, max: f32, default: f32 },
    /// Collapsed to 0 px — the panel exists but is not visible.
    Hidden,
}

impl SizeRule {
    /// Resolve a concrete length against the `available` space along the axis.
    ///
    /// `Flex` returns `available`; the manager redistributes flex space itself,
    /// so this is only the per-rule clamp used for fixed/range sizing.
    pub fn resolve(self, available: f32) -> f32 {
        match self {
            SizeRule::Fixed(n) => n.max(0.0),
            SizeRule::Flex => available.max(0.0),
            SizeRule::Content | SizeRule::Hidden => 0.0,
            SizeRule::Range { min, max, default } => default.clamp(min, max).min(available),
        }
    }

    /// `true` if this rule expands to fill leftover space.
    pub fn is_flex(self) -> bool {
        matches!(self, SizeRule::Flex)
    }
}

// ── Input events ─────────────────────────────────────────────────────────────

/// Mouse button identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Scroll wheel / trackpad delta in CSS px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollDelta {
    /// Horizontal delta (positive = content moves right).
    pub x: f32,
    /// Vertical delta (positive = content moves down).
    pub y: f32,
}

/// An event delivered to a panel via [`super::Panel::on_event`].
///
/// All positions are in *panel-local* coordinates (origin at the panel's
/// top-left), so a panel never needs to know where it sits on screen.
#[derive(Debug, Clone, PartialEq)]
pub enum PanelEvent {
    /// Cursor entered the panel's rect.
    MouseEnter,
    /// Cursor left the panel's rect.
    MouseLeave,
    /// Cursor moved within the panel.
    MouseMove { pos: Point },
    /// A mouse button was pressed.
    MouseDown { pos: Point, button: MouseButton },
    /// A mouse button was released.
    MouseUp { pos: Point, button: MouseButton },
    /// A full click (press + release in the same panel).
    Click { pos: Point, button: MouseButton },
    /// Wheel / trackpad scroll.
    Scroll { delta: ScrollDelta },
    /// Printable text input (already IME / layout resolved).
    TextInput { text: String },
    /// Panel gained keyboard focus.
    FocusGained,
    /// Panel lost keyboard focus.
    FocusLost,
    /// The window or the panel's slot was resized.
    Resized { new_rect: Rect },
    /// The active [`super::Theme`] changed.
    ThemeChanged,
}

// ── Event responses & commands ───────────────────────────────────────────────

/// What a panel returns from [`super::Panel::on_event`].
#[derive(Debug, Clone, PartialEq)]
pub enum EventResponse {
    /// Handled here — do not pass the event to anything else.
    Consumed,
    /// Not handled — keep routing to lower panels / the page.
    Ignored,
    /// Run this command against application state.
    Command(Command),
    /// Run several commands in order.
    Commands(Vec<Command>),
    /// Close this panel (shorthand for `Command::CloseSurface(self.id())`).
    Close,
}

/// State-changing intents a panel can emit.
///
/// Panels never mutate application state directly — they return a `Command`,
/// the manager queues it, and the shell drains the queue and applies it.  This
/// is a pragmatic subset of the full command set; new variants are added as
/// panels migrate onto the system.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Navigate the active tab (or a new tab) to `url`.
    Navigate { url: String, new_tab: bool },
    /// Go back in history.
    GoBack,
    /// Go forward in history.
    GoForward,
    /// Reload the active tab.
    Reload { bypass_cache: bool },
    /// Open a new tab, optionally at `url`.
    NewTab { url: Option<String> },
    /// Close the tab with the given index.
    CloseTab(usize),
    /// Select (focus) the tab with the given index.
    SelectTab(usize),
    /// Show or hide a panel by id.
    SetSurfaceVisible { id: &'static str, visible: bool },
    /// Close (hide) a panel by id.
    CloseSurface(&'static str),
    /// Give keyboard focus to a panel by id.
    FocusSurface(&'static str),
    /// Copy text to the system clipboard.
    CopyToClipboard(String),
    /// A shell-specific command not yet modelled; carries a free-form tag so a
    /// panel can request behaviour during incremental migration.
    Custom(String),
}

// ── Hit testing ──────────────────────────────────────────────────────────────

/// Mouse cursor shape requested for a hit target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorIcon {
    Default,
    Pointer,
    Text,
    Grab,
    Grabbing,
    ResizeHorizontal,
    ResizeVertical,
}

/// Semantic identity of the element under the cursor.
#[derive(Debug, Clone, PartialEq)]
pub enum HitElement {
    /// A named button (for hover styling in `paint`).
    Button(&'static str),
    /// A hyperlink.
    Link { url: String },
    /// A browser tab by index.
    Tab(usize),
    /// A panel resize handle.
    ResizeHandle {
        panel: &'static str,
        horizontal: bool,
    },
    /// A drag handle (e.g. a floating window title bar).
    DragHandle,
    /// Selectable text.
    Text,
    /// Empty / non-interactive space.
    Empty,
    /// A panel-specific element identified by a free-form key.
    Custom(String),
}

/// Result of [`super::Panel::hit_test`]: what is under a point and how the shell
/// should present it (cursor, tooltip, status URL).
#[derive(Debug, Clone, PartialEq)]
pub struct HitTarget {
    /// The semantic element hit.
    pub element: HitElement,
    /// Cursor to display while hovering.
    pub cursor: CursorIcon,
    /// Tooltip text (shown after a hover delay), if any.
    pub tooltip: Option<String>,
    /// URL to show in the status line on hover, if any.
    pub status_url: Option<String>,
}

impl HitTarget {
    /// A minimal hit target for `element` with a default cursor and no tooltip.
    pub fn new(element: HitElement) -> Self {
        let cursor = match &element {
            HitElement::Button(_) | HitElement::Link { .. } | HitElement::Tab(_) => {
                CursorIcon::Pointer
            }
            HitElement::Text => CursorIcon::Text,
            HitElement::DragHandle => CursorIcon::Grab,
            HitElement::ResizeHandle { horizontal, .. } => {
                if *horizontal {
                    CursorIcon::ResizeHorizontal
                } else {
                    CursorIcon::ResizeVertical
                }
            }
            HitElement::Empty | HitElement::Custom(_) => CursorIcon::Default,
        };
        Self {
            element,
            cursor,
            tooltip: None,
            status_url: None,
        }
    }
}

/// `true` if `rect` contains `p` (left/top inclusive, right/bottom exclusive).
pub fn rect_contains(rect: Rect, p: Point) -> bool {
    p.x >= rect.x && p.x < rect.right() && p.y >= rect.y && p.y < rect.bottom()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_rule_fixed_resolves() {
        assert_eq!(SizeRule::Fixed(280.0).resolve(1000.0), 280.0);
        assert_eq!(SizeRule::Fixed(-5.0).resolve(1000.0), 0.0);
    }

    #[test]
    fn size_rule_flex_takes_available() {
        assert_eq!(SizeRule::Flex.resolve(640.0), 640.0);
        assert!(SizeRule::Flex.is_flex());
    }

    #[test]
    fn size_rule_range_clamps_and_caps_at_available() {
        let r = SizeRule::Range { min: 100.0, max: 400.0, default: 300.0 };
        assert_eq!(r.resolve(1000.0), 300.0);
        assert_eq!(r.resolve(250.0), 250.0); // capped at available
        let r2 = SizeRule::Range { min: 100.0, max: 200.0, default: 300.0 };
        assert_eq!(r2.resolve(1000.0), 200.0); // clamped to max
    }

    #[test]
    fn size_rule_hidden_and_content_are_zero() {
        assert_eq!(SizeRule::Hidden.resolve(500.0), 0.0);
        assert_eq!(SizeRule::Content.resolve(500.0), 0.0);
    }

    #[test]
    fn surface_classification() {
        let d = Surface::Docked { slot: "left" };
        assert!(d.is_docked());
        assert!(!d.is_overlay());
        let f = Surface::Float {
            anchor: FloatAnchor::Center,
            z_order: 10,
            close_on_outside_click: true,
        };
        assert!(f.is_overlay());
        assert!(!f.is_docked());
    }

    #[test]
    fn rect_contains_edges() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert!(rect_contains(r, Point::new(10.0, 20.0))); // top-left inclusive
        assert!(rect_contains(r, Point::new(109.9, 69.9)));
        assert!(!rect_contains(r, Point::new(110.0, 40.0))); // right exclusive
        assert!(!rect_contains(r, Point::new(50.0, 70.0))); // bottom exclusive
        assert!(!rect_contains(r, Point::new(0.0, 0.0)));
    }

    #[test]
    fn hit_target_default_cursor_by_element() {
        assert_eq!(
            HitTarget::new(HitElement::Button("x")).cursor,
            CursorIcon::Pointer
        );
        assert_eq!(HitTarget::new(HitElement::Text).cursor, CursorIcon::Text);
        assert_eq!(
            HitTarget::new(HitElement::ResizeHandle { panel: "p", horizontal: true }).cursor,
            CursorIcon::ResizeHorizontal
        );
        assert_eq!(
            HitTarget::new(HitElement::Empty).cursor,
            CursorIcon::Default
        );
    }
}
