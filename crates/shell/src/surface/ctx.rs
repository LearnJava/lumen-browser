//! Per-call contexts handed to a panel during paint and event handling
//! (ADR-009).
//!
//! [`PaintCtx`] is read-only: it gives a panel its rect, the theme and a few
//! presentation hints, and nothing else — no access to other panels, no way to
//! mutate state.  [`EventCtx`] is the controlled mutation channel: a panel
//! cannot touch application state directly, it can only *queue commands*,
//! request repaint, change the cursor, or move keyboard focus.  The manager
//! drains these effects after `on_event` returns.

use std::time::Duration;

use lumen_core::geom::{Point, Rect};

use super::theme::Theme;
use super::types::{Command, CursorIcon};

/// Read-only context for [`super::Panel::paint`].
///
/// A panel paints only inside [`PaintCtx::rect`]; all positions it emits are in
/// window/screen coordinates derived from that rect.
pub struct PaintCtx<'a> {
    /// The panel's rectangle in window coordinates.  Paint only inside it.
    pub rect: Rect,
    /// Active design tokens.
    pub theme: &'a Theme,
    /// Display scale factor (1.0 = normal, 2.0 = HiDPI).
    pub scale: f32,
    /// `true` if this panel currently holds keyboard focus.
    pub focused: bool,
    /// Cursor position in panel-local coordinates, or `None` if not hovering.
    pub cursor_pos: Option<Point>,
    /// Elapsed time since the previous frame (for animations).
    pub dt: Duration,
}

impl<'a> PaintCtx<'a> {
    /// Build a paint context with default (non-focused, non-hovered) hints.
    pub fn new(rect: Rect, theme: &'a Theme) -> Self {
        Self {
            rect,
            theme,
            scale: 1.0,
            focused: false,
            cursor_pos: None,
            dt: Duration::ZERO,
        }
    }
}

/// Side effects a panel may request while handling an event.
///
/// The manager creates one `EventCtx`, passes it by `&mut` to
/// [`super::Panel::on_event`], then inspects these fields to apply the effects.
#[derive(Default)]
pub struct EventCtx {
    /// Commands queued by the panel (applied in order by the shell).
    commands: Vec<Command>,
    /// Set when the panel asked to be repainted.
    repaint: bool,
    /// Cursor the panel requested, if any.
    cursor: Option<CursorIcon>,
    /// `Some(true)` = request focus, `Some(false)` = release focus.
    focus_change: Option<bool>,
}

impl EventCtx {
    /// A fresh context with no pending effects.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a command to be applied after `on_event` returns.
    pub fn dispatch(&mut self, cmd: Command) {
        self.commands.push(cmd);
    }

    /// Mark this panel dirty so it repaints on the next frame.
    pub fn request_repaint(&mut self) {
        self.repaint = true;
    }

    /// Ask the shell to show `cursor` while over this panel.
    pub fn set_cursor(&mut self, cursor: CursorIcon) {
        self.cursor = Some(cursor);
    }

    /// Ask to capture keyboard focus.
    pub fn request_focus(&mut self) {
        self.focus_change = Some(true);
    }

    /// Ask to release keyboard focus.
    pub fn release_focus(&mut self) {
        self.focus_change = Some(false);
    }

    // ── Read-back (used by the manager / shell) ──────────────────────────

    /// Commands queued during this event, in dispatch order.
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// Take ownership of the queued commands, leaving the context empty.
    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.commands)
    }

    /// Whether the panel requested a repaint.
    pub fn wants_repaint(&self) -> bool {
        self.repaint
    }

    /// The cursor the panel requested, if any.
    pub fn requested_cursor(&self) -> Option<CursorIcon> {
        self.cursor
    }

    /// The focus change the panel requested: `Some(true)` to capture focus,
    /// `Some(false)` to release it, `None` for no change.
    pub fn requested_focus_change(&self) -> Option<bool> {
        self.focus_change
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_ctx_defaults() {
        let theme = Theme::sand_indigo();
        let ctx = PaintCtx::new(Rect::new(0.0, 0.0, 100.0, 50.0), &theme);
        assert_eq!(ctx.scale, 1.0);
        assert!(!ctx.focused);
        assert!(ctx.cursor_pos.is_none());
    }

    #[test]
    fn event_ctx_collects_commands_in_order() {
        let mut ctx = EventCtx::new();
        ctx.dispatch(Command::GoBack);
        ctx.dispatch(Command::Reload { bypass_cache: false });
        assert_eq!(ctx.commands().len(), 2);
        assert_eq!(ctx.commands()[0], Command::GoBack);
        let taken = ctx.take_commands();
        assert_eq!(taken.len(), 2);
        assert!(ctx.commands().is_empty());
    }

    #[test]
    fn event_ctx_repaint_and_cursor() {
        let mut ctx = EventCtx::new();
        assert!(!ctx.wants_repaint());
        ctx.request_repaint();
        ctx.set_cursor(CursorIcon::Pointer);
        assert!(ctx.wants_repaint());
        assert_eq!(ctx.requested_cursor(), Some(CursorIcon::Pointer));
    }

    #[test]
    fn event_ctx_focus_change() {
        let mut ctx = EventCtx::new();
        assert_eq!(ctx.requested_focus_change(), None);
        ctx.request_focus();
        assert_eq!(ctx.requested_focus_change(), Some(true));
        ctx.release_focus();
        assert_eq!(ctx.requested_focus_change(), Some(false));
    }
}
