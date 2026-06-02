//! `SurfaceManager` — layout tree, composite, and event routing (ADR-009).
//!
//! Panels register once; the manager resolves their rects whenever the window
//! resizes, composites all visible DisplayLists in one pass, and routes OS
//! input to the correct panel from top to bottom.
//!
//! **Phase-1 scope:** docked cross-layout (top / left / right / bottom /
//! content), float / modal positioning, and mouse event routing.  OS windows
//! and keyboard dispatch are recorded but not yet wired.

use std::collections::HashMap;

use lumen_core::geom::{Point, Rect};
use lumen_paint::DisplayList;

use super::{
    ctx::{EventCtx, PaintCtx},
    theme::Theme,
    types::{Corner, EventResponse, FloatAnchor, MouseButton, PanelEvent, ScrollDelta, Surface},
    Panel,
};

// ── Internal entry ────────────────────────────────────────────────────────────

struct PanelEntry {
    panel: Box<dyn Panel>,
    /// Panel rect in window coordinates; zero when hidden.
    rect: Rect,
    visible: bool,
}

impl PanelEntry {
    fn new(panel: Box<dyn Panel>) -> Self {
        Self { panel, rect: Rect::ZERO, visible: true }
    }
}

// ── Public types ──────────────────────────────────────────────────────────────

/// Resolved window-space rect for a named docked slot.
pub struct SlotRect {
    /// Slot identifier (e.g. `"content"`).
    pub slot: &'static str,
    /// Window-space rect.
    pub rect: Rect,
}

/// Informational snapshot of one slot in the docked layout tree.
pub struct LayoutNode {
    /// Slot identifier.
    pub slot: &'static str,
    /// Resolved rect.
    pub rect: Rect,
    /// Sub-nodes (empty in the flat Phase-1 layout).
    pub children: Vec<LayoutNode>,
}

// ── Known static slot names ───────────────────────────────────────────────────

static SLOT_NAMES: &[&str] = &["top", "left", "right", "bottom", "content"];

fn as_static_slot(s: &str) -> Option<&'static str> {
    SLOT_NAMES.iter().copied().find(|n| *n == s)
}

// ── SurfaceManager ────────────────────────────────────────────────────────────

/// Single coordinator for all shell UI panels (ADR-009 §SurfaceManager).
///
/// Owns the docked cross-layout tree and the float layer.  Composites every
/// visible panel into one `DisplayList` and routes OS mouse events to the
/// topmost panel under the cursor.
///
/// All methods run on the winit event-loop thread; no synchronisation required.
pub struct SurfaceManager {
    panels: Vec<PanelEntry>,
    window_size: (f32, f32),
    theme: Theme,
    /// Index of the currently focused panel (keyboard target).
    focused: Option<usize>,
    /// Resolved rects for the five docked slots.
    docked_rects: HashMap<&'static str, Rect>,
}

impl SurfaceManager {
    /// Create an empty manager sized to `(width, height)` CSS px.
    pub fn new(width: f32, height: f32) -> Self {
        let mut mgr = Self {
            panels: Vec::new(),
            window_size: (width, height),
            theme: Theme::default(),
            focused: None,
            docked_rects: HashMap::new(),
        };
        mgr.compute_slot_rects();
        mgr
    }

    /// Register a panel.  Its rect is computed immediately; `on_mount` is called.
    pub fn register(&mut self, mut panel: Box<dyn Panel>) {
        let mut ctx = EventCtx::new();
        panel.on_mount(&mut ctx);
        self.panels.push(PanelEntry::new(panel));
        self.compute_slot_rects();   // recompute slots with the new panel included
        self.assign_panel_rects();
        // commands emitted during on_mount are applied by the shell on drain
    }

    /// Composite all visible panels into one `DisplayList` for the renderer.
    ///
    /// Order: docked panels (registration order), then float/modal panels
    /// sorted by `z_order` ascending (lowest first → highest drawn last).
    pub fn composite(&self) -> DisplayList {
        let theme = &self.theme;
        let mut dl: DisplayList = Vec::new();

        // ── Docked panels ──────────────────────────────────────────────────
        for (idx, entry) in self.panels.iter().enumerate() {
            if !entry.visible { continue; }
            if !matches!(entry.panel.surface(), Surface::Docked { .. }) { continue; }
            if entry.rect.width <= 0.0 || entry.rect.height <= 0.0 { continue; }
            let ctx = self.make_paint_ctx(idx, theme);
            dl.extend(entry.panel.paint(&ctx));
        }

        // ── Float and modal panels (z-sorted) ────────────────────────────
        let mut overlay: Vec<(usize, i32)> = self.panels.iter()
            .enumerate()
            .filter(|(_, e)| {
                e.visible
                    && matches!(e.panel.surface(), Surface::Float { .. } | Surface::Modal { .. })
                    && (e.rect.width > 0.0 && e.rect.height > 0.0)
            })
            .map(|(i, e)| (i, z_order_of(e.panel.as_ref())))
            .collect();
        overlay.sort_by_key(|(_, z)| *z);

        for (idx, _) in overlay {
            let entry = &self.panels[idx];
            let ctx = self.make_paint_ctx(idx, theme);
            dl.extend(entry.panel.paint(&ctx));
        }

        dl
    }

    /// Resolved rect for a named docked slot, or `None` if not present.
    pub fn slot_rect(&self, slot: &str) -> Option<SlotRect> {
        let name = as_static_slot(slot)?;
        let rect = self.docked_rects.get(name).copied()?;
        Some(SlotRect { slot: name, rect })
    }

    /// Snapshot of the docked layout tree (diagnostic / test helper).
    pub fn layout_snapshot(&self) -> Vec<LayoutNode> {
        SLOT_NAMES
            .iter()
            .filter_map(|&name| {
                self.docked_rects.get(name).map(|&rect| LayoutNode {
                    slot: name,
                    rect,
                    children: Vec::new(),
                })
            })
            .collect()
    }

    /// Notify that the window was resized.  All panel rects are recomputed and
    /// `on_resize` is called on each visible panel.
    pub fn on_resize(&mut self, width: f32, height: f32) {
        self.window_size = (width, height);
        self.compute_slot_rects();
        self.assign_panel_rects();
        // notify each visible panel
        let rects: Vec<(usize, Rect)> = self.panels.iter()
            .enumerate()
            .filter(|(_, e)| e.visible)
            .map(|(i, e)| (i, e.rect))
            .collect();
        for (i, rect) in rects {
            self.panels[i].panel.on_resize(rect);
        }
    }

    /// Show or hide a panel by id.  Triggers layout recomputation.
    pub fn set_visible(&mut self, id: &str, visible: bool) {
        if let Some(e) = self.panels.iter_mut().find(|e| e.panel.id() == id) {
            e.visible = visible;
        }
        self.compute_slot_rects();
        self.assign_panel_rects();
    }

    /// Set the active `Theme` for all subsequent `paint()` calls.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// Active theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Whether a panel with `id` is registered.
    pub fn has_panel(&self, id: &str) -> bool {
        self.panels.iter().any(|e| e.panel.id() == id)
    }

    /// Number of registered panels.
    pub fn panel_count(&self) -> usize {
        self.panels.len()
    }

    /// Current window size (CSS px).
    pub fn window_size(&self) -> (f32, f32) {
        self.window_size
    }

    /// Rect of a registered panel, or `None` if not found / hidden.
    pub fn panel_rect(&self, id: &str) -> Option<Rect> {
        self.panels.iter()
            .find(|e| e.panel.id() == id && e.visible)
            .map(|e| e.rect)
    }

    // ── Mouse event routing ───────────────────────────────────────────────

    /// Route a mouse-move event and return the combined response.
    ///
    /// Float panels are tested first (highest z-order wins), then docked panels
    /// in reverse registration order (last registered = visually topmost).
    pub fn route_mouse_move(&mut self, pos: Point) -> EventResponse {
        self.route_mouse(pos, |local| PanelEvent::MouseMove { pos: local })
    }

    /// Route a mouse-down event.
    pub fn route_mouse_down(&mut self, pos: Point, button: MouseButton) -> EventResponse {
        self.route_mouse(pos, |local| PanelEvent::MouseDown { pos: local, button })
    }

    /// Route a mouse-up event.
    pub fn route_mouse_up(&mut self, pos: Point, button: MouseButton) -> EventResponse {
        self.route_mouse(pos, |local| PanelEvent::MouseUp { pos: local, button })
    }

    /// Route a click (press + release in the same panel).
    pub fn route_click(&mut self, pos: Point, button: MouseButton) -> EventResponse {
        self.route_mouse(pos, |local| PanelEvent::Click { pos: local, button })
    }

    /// Route a scroll event.
    pub fn route_scroll(&mut self, pos: Point, delta: ScrollDelta) -> EventResponse {
        self.route_mouse(pos, |_local| PanelEvent::Scroll { delta })
    }

    // ── Private ───────────────────────────────────────────────────────────────

    /// Recompute the five docked slot rects from window size and visible panels.
    fn compute_slot_rects(&mut self) {
        let (w, h) = self.window_size;

        let top_h = self.docked_axis_size("top", false, h);
        let bottom_h = self.docked_axis_size("bottom", false, h);
        let mid_h = (h - top_h - bottom_h).max(0.0);
        let left_w = self.docked_axis_size("left", true, w);
        let right_w = self.docked_axis_size("right", true, w);
        let content_w = (w - left_w - right_w).max(0.0);

        self.docked_rects.insert("top",     Rect::new(0.0,             0.0,      w,         top_h));
        self.docked_rects.insert("bottom",  Rect::new(0.0,             h - bottom_h, w,     bottom_h));
        self.docked_rects.insert("left",    Rect::new(0.0,             top_h,    left_w,    mid_h));
        self.docked_rects.insert("right",   Rect::new(w - right_w,     top_h,    right_w,   mid_h));
        self.docked_rects.insert("content", Rect::new(left_w,          top_h,    content_w, mid_h));
    }

    /// Assign rects to each panel entry based on its `Surface`.
    fn assign_panel_rects(&mut self) {
        let (w, h) = self.window_size;

        // Collect new rects without borrowing self.panels mutably yet.
        let new_rects: Vec<Rect> = self.panels.iter().map(|entry| {
            if !entry.visible {
                return Rect::ZERO;
            }
            match entry.panel.surface() {
                Surface::Docked { slot } => {
                    self.docked_rects.get(slot).copied().unwrap_or(Rect::ZERO)
                }
                Surface::Float { anchor, .. } => {
                    let pw = entry.panel.width().resolve(w);
                    let ph = entry.panel.height().resolve(h);
                    resolve_float_rect(anchor, pw, ph, w, h)
                }
                Surface::Modal { .. } => {
                    let pw = entry.panel.width().resolve(w).min(w);
                    let ph = entry.panel.height().resolve(h).min(h);
                    Rect::new(
                        ((w - pw) * 0.5).max(0.0),
                        ((h - ph) * 0.5).max(0.0),
                        pw,
                        ph,
                    )
                }
                Surface::OsWindow { .. } => Rect::ZERO,
            }
        }).collect();

        for (entry, rect) in self.panels.iter_mut().zip(new_rects) {
            entry.rect = rect;
        }
    }

    /// Resolved size (width or height) for the first visible panel in `slot`.
    fn docked_axis_size(&self, slot: &str, for_width: bool, available: f32) -> f32 {
        self.panels.iter()
            .find(|e| {
                e.visible
                    && matches!(e.panel.surface(), Surface::Docked { slot: s } if s == slot)
            })
            .map(|e| {
                if for_width { e.panel.width() } else { e.panel.height() }
                    .resolve(available)
            })
            .unwrap_or(0.0)
    }

    /// Generic mouse routing: build a PanelEvent from local pos, find topmost
    /// hit panel, dispatch.
    fn route_mouse<F>(&mut self, pos: Point, make_event: F) -> EventResponse
    where
        F: Fn(Point) -> PanelEvent,
    {
        // Collect candidate (idx, z) for float/modal, highest z first.
        let mut overlay: Vec<(usize, i32)> = self.panels.iter()
            .enumerate()
            .filter(|(_, e)| {
                e.visible
                    && matches!(e.panel.surface(), Surface::Float { .. } | Surface::Modal { .. })
                    && rect_hit(e.rect, pos)
            })
            .map(|(i, e)| (i, z_order_of(e.panel.as_ref())))
            .collect();
        overlay.sort_by_key(|(_, z)| std::cmp::Reverse(*z));

        for (idx, _) in overlay {
            let local = local_pos(self.panels[idx].rect, pos);
            let event = make_event(local);
            let mut ctx = EventCtx::new();
            let resp = self.panels[idx].panel.on_event(&event, &mut ctx);
            if !matches!(resp, EventResponse::Ignored) {
                return resp;
            }
        }

        // Docked panels: last registered = topmost visually.
        let docked: Vec<usize> = self.panels.iter()
            .enumerate()
            .filter(|(_, e)| {
                e.visible
                    && matches!(e.panel.surface(), Surface::Docked { .. })
                    && rect_hit(e.rect, pos)
            })
            .map(|(i, _)| i)
            .rev()
            .collect();

        for idx in docked {
            let local = local_pos(self.panels[idx].rect, pos);
            let event = make_event(local);
            let mut ctx = EventCtx::new();
            let resp = self.panels[idx].panel.on_event(&event, &mut ctx);
            if !matches!(resp, EventResponse::Ignored) {
                return resp;
            }
        }

        EventResponse::Ignored
    }

    fn make_paint_ctx<'a>(&self, idx: usize, theme: &'a Theme) -> PaintCtx<'a> {
        let entry = &self.panels[idx];
        let mut ctx = PaintCtx::new(entry.rect, theme);
        ctx.focused = self.focused == Some(idx);
        ctx
    }
}

// ── Free helpers ──────────────────────────────────────────────────────────────

fn z_order_of(panel: &dyn Panel) -> i32 {
    match panel.surface() {
        Surface::Float { z_order, .. } => z_order,
        Surface::Modal { .. } => i32::MAX - 1,
        _ => 0,
    }
}

fn rect_hit(rect: Rect, p: Point) -> bool {
    p.x >= rect.x && p.x < rect.x + rect.width
        && p.y >= rect.y && p.y < rect.y + rect.height
}

fn local_pos(rect: Rect, window_pos: Point) -> Point {
    Point::new(window_pos.x - rect.x, window_pos.y - rect.y)
}

fn resolve_float_rect(anchor: FloatAnchor, pw: f32, ph: f32, win_w: f32, win_h: f32) -> Rect {
    match anchor {
        FloatAnchor::Center => Rect::new(
            ((win_w - pw) * 0.5).max(0.0),
            ((win_h - ph) * 0.5).max(0.0),
            pw, ph,
        ),
        FloatAnchor::Corner(corner) => {
            let (x, y) = match corner {
                Corner::TopLeft     => (0.0,              0.0),
                Corner::TopRight    => (win_w - pw,       0.0),
                Corner::BottomLeft  => (0.0,              win_h - ph),
                Corner::BottomRight => (win_w - pw,       win_h - ph),
            };
            Rect::new(x.max(0.0), y.max(0.0), pw, ph)
        }
        FloatAnchor::Absolute(p) => Rect::new(p.x, p.y, pw, ph),
        FloatAnchor::Below(r) => {
            let x = r.x.min(win_w - pw).max(0.0);
            let y_below = r.y + r.height;
            let y = if y_below + ph <= win_h { y_below } else { (r.y - ph).max(0.0) };
            Rect::new(x, y, pw, ph)
        }
        FloatAnchor::Above(r) => {
            let x = r.x.min(win_w - pw).max(0.0);
            Rect::new(x, (r.y - ph).max(0.0), pw, ph)
        }
        FloatAnchor::Cursor => {
            // No cursor position at layout time; default to center.
            Rect::new(((win_w - pw) * 0.5).max(0.0), ((win_h - ph) * 0.5).max(0.0), pw, ph)
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use lumen_paint::DisplayCommand;

    use super::*;
    use crate::surface::{
        ctx::EventCtx,
        types::{EventResponse, SizeRule, Surface},
        Panel, PaintCtx,
    };

    // ── Minimal test panels ───────────────────────────────────────────────────

    struct FixedPanel {
        id: &'static str,
        slot: &'static str,
        w: f32,
        h: f32,
    }

    impl Panel for FixedPanel {
        fn id(&self) -> &'static str { self.id }
        fn surface(&self) -> Surface { Surface::Docked { slot: self.slot } }
        fn width(&self) -> SizeRule { SizeRule::Fixed(self.w) }
        fn height(&self) -> SizeRule { SizeRule::Fixed(self.h) }
        fn paint(&self, _ctx: &PaintCtx) -> DisplayList { vec![] }
        fn on_event(&mut self, _ev: &PanelEvent, _ctx: &mut EventCtx) -> EventResponse {
            EventResponse::Ignored
        }
    }

    struct ClickPanel {
        id: &'static str,
        slot: &'static str,
    }

    impl Panel for ClickPanel {
        fn id(&self) -> &'static str { self.id }
        fn surface(&self) -> Surface { Surface::Docked { slot: self.slot } }
        fn width(&self) -> SizeRule { SizeRule::Fixed(200.0) }
        fn height(&self) -> SizeRule { SizeRule::Fixed(50.0) }
        fn paint(&self, _ctx: &PaintCtx) -> DisplayList { vec![] }
        fn on_event(&mut self, ev: &PanelEvent, _ctx: &mut EventCtx) -> EventResponse {
            if matches!(ev, PanelEvent::Click { .. }) {
                EventResponse::Consumed
            } else {
                EventResponse::Ignored
            }
        }
    }

    struct FloatTestPanel {
        id: &'static str,
        anchor: FloatAnchor,
        w: f32,
        h: f32,
    }

    impl Panel for FloatTestPanel {
        fn id(&self) -> &'static str { self.id }
        fn surface(&self) -> Surface {
            Surface::Float { anchor: self.anchor.clone(), z_order: 100, close_on_outside_click: false }
        }
        fn width(&self) -> SizeRule { SizeRule::Fixed(self.w) }
        fn height(&self) -> SizeRule { SizeRule::Fixed(self.h) }
        fn paint(&self, _ctx: &PaintCtx) -> DisplayList { vec![] }
        fn on_event(&mut self, _ev: &PanelEvent, _ctx: &mut EventCtx) -> EventResponse {
            EventResponse::Consumed
        }
    }

    struct PaintingPanel {
        id: &'static str,
        slot: &'static str,
    }

    impl Panel for PaintingPanel {
        fn id(&self) -> &'static str { self.id }
        fn surface(&self) -> Surface { Surface::Docked { slot: self.slot } }
        fn width(&self) -> SizeRule { SizeRule::Fixed(40.0) }
        fn height(&self) -> SizeRule { SizeRule::Fixed(36.0) }
        fn paint(&self, ctx: &PaintCtx) -> DisplayList {
            vec![DisplayCommand::FillRect {
                rect: ctx.rect,
                color: lumen_layout::Color { r: 255, g: 0, b: 0, a: 255 },
            }]
        }
        fn on_event(&mut self, _ev: &PanelEvent, _ctx: &mut EventCtx) -> EventResponse {
            EventResponse::Ignored
        }
    }

    // ── Constructor / basic state ─────────────────────────────────────────────

    #[test]
    fn new_is_empty() {
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

    // ── Docked layout ─────────────────────────────────────────────────────────

    #[test]
    fn top_bar_takes_full_width() {
        let mut mgr = SurfaceManager::new(1024.0, 768.0);
        mgr.register(Box::new(FixedPanel { id: "top", slot: "top", w: 0.0, h: 36.0 }));
        let r = mgr.panel_rect("top").unwrap();
        assert_eq!(r.width, 1024.0);
        assert_eq!(r.height, 36.0);
        assert_eq!(r.x, 0.0);
        assert_eq!(r.y, 0.0);
    }

    #[test]
    fn content_fills_remaining_space() {
        let mut mgr = SurfaceManager::new(1000.0, 600.0);
        mgr.register(Box::new(FixedPanel { id: "top-bar", slot: "top", w: 0.0, h: 40.0 }));
        mgr.register(Box::new(FixedPanel { id: "sidebar", slot: "left", w: 200.0, h: 0.0 }));
        let content = mgr.slot_rect("content").unwrap();
        assert_eq!(content.rect.x, 200.0);
        assert_eq!(content.rect.y, 40.0);
        assert_eq!(content.rect.width, 800.0);
        assert_eq!(content.rect.height, 560.0);
    }

    #[test]
    fn bottom_panel_at_window_bottom() {
        let mut mgr = SurfaceManager::new(800.0, 600.0);
        mgr.register(Box::new(FixedPanel { id: "status", slot: "bottom", w: 0.0, h: 24.0 }));
        let r = mgr.panel_rect("status").unwrap();
        assert_eq!(r.y, 600.0 - 24.0);
        assert_eq!(r.width, 800.0);
    }

    #[test]
    fn right_panel_at_window_right() {
        let mut mgr = SurfaceManager::new(1024.0, 768.0);
        mgr.register(Box::new(FixedPanel { id: "inspector", slot: "right", w: 300.0, h: 0.0 }));
        let r = mgr.panel_rect("inspector").unwrap();
        assert_eq!(r.x, 1024.0 - 300.0);
    }

    #[test]
    fn resize_updates_all_rects() {
        let mut mgr = SurfaceManager::new(800.0, 600.0);
        mgr.register(Box::new(FixedPanel { id: "top-bar", slot: "top", w: 0.0, h: 36.0 }));
        mgr.on_resize(1280.0, 800.0);
        let r = mgr.panel_rect("top-bar").unwrap();
        assert_eq!(r.width, 1280.0);
        assert_eq!(mgr.slot_rect("content").unwrap().rect.height, 800.0 - 36.0);
    }

    // ── Visibility ────────────────────────────────────────────────────────────

    #[test]
    fn hidden_panel_has_zero_rect() {
        let mut mgr = SurfaceManager::new(1024.0, 768.0);
        mgr.register(Box::new(FixedPanel { id: "sidebar", slot: "left", w: 240.0, h: 0.0 }));
        mgr.set_visible("sidebar", false);
        let r = mgr.panel_rect("sidebar");
        assert!(r.is_none()); // panel_rect returns None for hidden panels
    }

    #[test]
    fn hiding_panel_reclaims_layout_space() {
        let mut mgr = SurfaceManager::new(1000.0, 600.0);
        mgr.register(Box::new(FixedPanel { id: "sidebar", slot: "left", w: 200.0, h: 0.0 }));
        mgr.set_visible("sidebar", false);
        let content = mgr.slot_rect("content").unwrap();
        assert_eq!(content.rect.x, 0.0);
        assert_eq!(content.rect.width, 1000.0);
    }

    // ── Float panels ──────────────────────────────────────────────────────────

    #[test]
    fn float_center_anchor() {
        let mut mgr = SurfaceManager::new(1000.0, 600.0);
        mgr.register(Box::new(FloatTestPanel { id: "cmd", anchor: FloatAnchor::Center, w: 560.0, h: 400.0 }));
        let r = mgr.panel_rect("cmd").unwrap();
        assert_eq!(r.x, (1000.0 - 560.0) / 2.0);
        assert_eq!(r.y, (600.0 - 400.0) / 2.0);
    }

    #[test]
    fn float_corner_bottom_right() {
        let mut mgr = SurfaceManager::new(1024.0, 768.0);
        mgr.register(Box::new(FloatTestPanel {
            id: "pip",
            anchor: FloatAnchor::Corner(Corner::BottomRight),
            w: 320.0,
            h: 180.0,
        }));
        let r = mgr.panel_rect("pip").unwrap();
        assert_eq!(r.x, 1024.0 - 320.0);
        assert_eq!(r.y, 768.0 - 180.0);
    }

    #[test]
    fn float_below_anchor_flips_when_overflow() {
        let mut mgr = SurfaceManager::new(800.0, 600.0);
        // button near the bottom; dropdown would overflow → should flip above
        let btn = Rect::new(100.0, 560.0, 200.0, 30.0);
        mgr.register(Box::new(FloatTestPanel {
            id: "dropdown",
            anchor: FloatAnchor::Below(btn),
            w: 200.0,
            h: 100.0,
        }));
        let r = mgr.panel_rect("dropdown").unwrap();
        // y_below=590, 590+100=690 > 600, so flip above: y = 560-100 = 460
        assert_eq!(r.y, 460.0);
    }

    // ── Composite ─────────────────────────────────────────────────────────────

    #[test]
    fn composite_includes_docked_panel_commands() {
        let mut mgr = SurfaceManager::new(1024.0, 768.0);
        mgr.register(Box::new(PaintingPanel { id: "top-bar", slot: "top" }));
        let dl = mgr.composite();
        assert!(!dl.is_empty(), "PaintingPanel should emit at least one FillRect");
    }

    #[test]
    fn composite_empty_when_no_panels() {
        let mgr = SurfaceManager::new(1024.0, 768.0);
        assert!(mgr.composite().is_empty());
    }

    // ── Mouse routing ─────────────────────────────────────────────────────────

    #[test]
    fn click_on_docked_panel_consumed() {
        let mut mgr = SurfaceManager::new(1024.0, 768.0);
        // top bar 36px tall, full width
        mgr.register(Box::new(ClickPanel { id: "top-bar", slot: "top" }));
        // register a content panel below it
        mgr.register(Box::new(FixedPanel { id: "content-bg", slot: "content", w: 0.0, h: 0.0 }));

        let resp = mgr.route_click(Point::new(100.0, 18.0), MouseButton::Left);
        assert!(matches!(resp, EventResponse::Consumed));
    }

    #[test]
    fn click_outside_all_panels_is_ignored() {
        let mut mgr = SurfaceManager::new(1024.0, 768.0);
        mgr.register(Box::new(ClickPanel { id: "top-bar", slot: "top" }));
        // click in empty content area (ClickPanel only has 50px height = top bar)
        let resp = mgr.route_click(Point::new(100.0, 500.0), MouseButton::Left);
        assert!(matches!(resp, EventResponse::Ignored));
    }

    #[test]
    fn float_panel_takes_priority_over_docked() {
        let mut mgr = SurfaceManager::new(1024.0, 768.0);
        mgr.register(Box::new(ClickPanel { id: "top-bar", slot: "top" }));
        // Float panel centered over top bar area
        mgr.register(Box::new(FloatTestPanel {
            id: "overlay",
            anchor: FloatAnchor::Absolute(Point::new(0.0, 0.0)),
            w: 1024.0,
            h: 768.0,
        }));
        // Float panel's on_event returns Consumed for any event
        let resp = mgr.route_click(Point::new(100.0, 18.0), MouseButton::Left);
        assert!(matches!(resp, EventResponse::Consumed));
    }

    // ── Theme ─────────────────────────────────────────────────────────────────

    #[test]
    fn set_theme_changes_active_theme() {
        let mut mgr = SurfaceManager::new(1024.0, 768.0);
        assert_eq!(mgr.theme().name, "sand-indigo");
        mgr.set_theme(Theme::graphite_amber());
        assert_eq!(mgr.theme().name, "graphite-amber");
    }

    // ── Layout snapshot ───────────────────────────────────────────────────────

    #[test]
    fn layout_snapshot_has_five_slots() {
        let mgr = SurfaceManager::new(1024.0, 768.0);
        assert_eq!(mgr.layout_snapshot().len(), 5);
    }
}
