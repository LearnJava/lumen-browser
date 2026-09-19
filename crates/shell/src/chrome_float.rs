//! CC-18 / BUG-1059: the floating chrome control panel (`#demoBar`,
//! `#infoPanel`) — detaching it from the strip-clipped chrome box tree, and
//! (срез 3) dragging it by its header with a double-click reset.
//!
//! Two responsibilities, one subject:
//!
//! * **Detachment** ([`FloatingPanelDetachment`], [`take_floating_panel`],
//!   [`restore_floating_panel`], moved here verbatim from `chrome_ui.rs`) —
//!   why these nodes cannot paint through the ordinary chrome overlay at all.
//! * **Placement** ([`Lumen::floating_panel_press`] and friends) — a paint-time
//!   offset applied on top of the CSS-computed position, so a drag never
//!   touches the cascade, the box tree, or the incremental-layout basis.
//!
//! The placement half is deliberately *not* a general draggable-window
//! system: only the two panels above go through it, and only a node carrying
//! `data-action="drag-panel"` (stamped by `scripts/gen_chrome_assets.py`'s
//! `add_drag_handles`) can start a drag.

use crate::*;

/// CC-18 срез 3: how close to the window edge a dragged panel may be pushed,
/// in window CSS px. Same 4 px the design reference's own drag clamp uses
/// (`Math.max(4, Math.min(innerWidth - width - 4, …))`, `docs/design/
/// lumen-v3_3.html`), so a panel can never be dragged fully off-screen.
const DRAG_MARGIN: f32 = 4.0;

/// CC-18 срез 3: longest gap between two presses on the same drag handle that
/// still counts as a double-click, in milliseconds. Matches the Windows
/// default double-click time; there is no winit/OS-level double-click event to
/// read instead (winit reports raw press/release only).
const DOUBLE_CLICK_MS: f64 = 500.0;

/// CC-18 срез 3: how far the pointer may travel between the two presses of a
/// double-click, in window CSS px. Without it the second press of a
/// drag-then-press-again sequence would reset a position the user just chose.
const DOUBLE_CLICK_SLOP: f32 = 4.0;

/// BUG-1059: what [`take_floating_panel`] removed from a chrome box tree —
/// enough for [`restore_floating_panel`] to put it back exactly, mirroring
/// [`ContentAreaDetachment`]'s shape but without a salvage step (a floating
/// panel paints as a single unclipped unit; nothing needs to stay behind in
/// the strip-clipped main tree).
pub(crate) struct FloatingPanelDetachment {
    /// Element id this box was found by (`ids::DEMO_BAR`/`ids::INFO_PANEL`) —
    /// the key CC-18 срез 3's per-panel drag offset is stored under, and what
    /// makes a hit on this box attributable to one panel without a second
    /// document lookup.
    pub(crate) panel_id: &'static str,
    /// Child-index path from the tree root down to the box that held this
    /// node (empty when the root itself held it).
    holder_path: Vec<usize>,
    /// Index the node occupied among that holder's children.
    slot: usize,
    /// The node's own box, detached as-is. Always the CSS-computed geometry:
    /// a drag offset is applied to a *copy* at paint time
    /// ([`Lumen::rebuild_chrome_floating_dl`]), never written back here, so
    /// what goes back into the incremental basis stays what layout produced.
    pub(crate) removed: LayoutBox,
}

/// CC-18 срез 3: an in-progress header drag.
pub(crate) struct FloatingPanelDrag {
    /// Which panel is being dragged ([`FloatingPanelDetachment::panel_id`]).
    pub(crate) panel_id: &'static str,
    /// Pointer offset from the panel's *painted* top-left corner at the
    /// moment of the press — held constant for the whole drag so the panel
    /// does not jump to centre itself under the cursor (same invariant
    /// [`panels::pip_window::PipWindow::begin_drag`] keeps).
    grab: (f32, f32),
}

/// CC-18 срез 3: the last press on a drag handle, for double-click detection.
pub(crate) struct FloatingPanelPress {
    /// Panel whose handle was pressed.
    pub(crate) panel_id: &'static str,
    /// When, in milliseconds since [`Lumen::epoch`].
    pub(crate) at_ms: f64,
    /// Where, in window CSS px.
    pub(crate) pos: (f32, f32),
}

/// CC-18 срез 3: the pure double-click decision, split out of
/// [`Lumen::floating_panel_press`] so it is testable without a live shell.
///
/// `prev` is the previous press on *any* drag handle; a press counts as the
/// second half of a double-click only when it lands on the same panel, within
/// [`DOUBLE_CLICK_MS`] and [`DOUBLE_CLICK_SLOP`] of it.
pub(crate) fn is_double_press(
    prev: Option<&FloatingPanelPress>,
    panel_id: &str,
    now_ms: f64,
    x: f32,
    y: f32,
) -> bool {
    prev.is_some_and(|p| {
        p.panel_id == panel_id
            && (now_ms - p.at_ms) <= DOUBLE_CLICK_MS
            && (now_ms - p.at_ms) >= 0.0
            && (p.pos.0 - x).abs() <= DOUBLE_CLICK_SLOP
            && (p.pos.1 - y).abs() <= DOUBLE_CLICK_SLOP
    })
}

/// CC-18 срез 3: the pure placement arithmetic behind a drag step, split out
/// for the same reason as [`is_double_press`].
///
/// `base` is the panel's CSS-computed rect, `grab` the pointer offset recorded
/// at press time, `(x, y)` the current pointer position and `(win_w, win_h)`
/// the window in CSS px. Returns the offset to apply to `base` at paint time,
/// clamped so at least [`DRAG_MARGIN`] px of the panel stays inside the
/// window on every side. A window smaller than the panel collapses the clamp
/// range to the margin itself rather than inverting it.
pub(crate) fn drag_offset(
    base: Rect,
    grab: (f32, f32),
    x: f32,
    y: f32,
    win_w: f32,
    win_h: f32,
) -> (f32, f32) {
    let max_x = (win_w - base.width - DRAG_MARGIN).max(DRAG_MARGIN);
    let max_y = (win_h - base.height - DRAG_MARGIN).max(DRAG_MARGIN);
    let nx = (x - grab.0).clamp(DRAG_MARGIN, max_x);
    let ny = (y - grab.1).clamp(DRAG_MARGIN, max_y);
    (nx - base.x, ny - base.y)
}

/// BUG-1059: `#demoBar`/`#infoPanel` (CC-18) are `position:fixed` chrome
/// content deliberately positioned inside [`Lumen::chrome_page_host_rect`] —
/// a floating panel over the live page. `build_chrome_overlay_strips`'s
/// 4-strip clip discards anything entirely *inside* that rect by design (see
/// that rect's own doc comment), so these nodes never reach the screen
/// unless detached from the tree before the clip is built and painted
/// through a separate, unclipped display list (`Lumen::chrome_floating_dl`,
/// appended to `overlay_buf` in `RedrawRequested` the same way the omnibox
/// caret already paints unclipped on top of the strip-clipped segment).
/// Mirrors [`take_content_area`]'s walk, without its salvage step.
pub(crate) fn take_floating_panel(
    lb: &mut LayoutBox,
    node: lumen_dom::NodeId,
    panel_id: &'static str,
) -> Option<(Rect, FloatingPanelDetachment)> {
    let mut path = Vec::new();
    take_floating_panel_at(lb, node, panel_id, &mut path)
}

/// [`take_floating_panel`]'s recursion — same walk as `take_content_area_at`.
fn take_floating_panel_at(
    lb: &mut LayoutBox,
    node: lumen_dom::NodeId,
    panel_id: &'static str,
    path: &mut Vec<usize>,
) -> Option<(Rect, FloatingPanelDetachment)> {
    if let Some(slot) = lb.children.iter().position(|c| c.node == node) {
        let removed = lb.children.remove(slot);
        let rect = removed.rect;
        return Some((
            rect,
            FloatingPanelDetachment { panel_id, holder_path: path.clone(), slot, removed },
        ));
    }
    for (i, child) in lb.children.iter_mut().enumerate() {
        path.push(i);
        if let Some(found) = take_floating_panel_at(child, node, panel_id, path) {
            return Some(found);
        }
        path.pop();
    }
    None
}

/// Inverse of [`take_floating_panel`] — re-inserts the detached box at its
/// former slot. Returns `false` if the recorded path no longer addresses a
/// box, mirroring `restore_content_area`'s same-shaped guard: the caller
/// treats that as "no usable `prev`" and takes the full-layout path.
pub(crate) fn restore_floating_panel(
    root: &mut LayoutBox,
    detached: FloatingPanelDetachment,
) -> bool {
    let FloatingPanelDetachment { panel_id: _, holder_path, slot, removed } = detached;
    let Some(holder) = chrome_ui::follow_box_path_mut(root, &holder_path) else { return false };
    if slot > holder.children.len() {
        return false;
    }
    holder.children.insert(slot, removed);
    true
}

impl Lumen {
    /// CC-18 срез 3: the paint-time offset currently applied to `panel_id`,
    /// `(0, 0)` when the panel sits at its CSS-computed default position.
    pub(crate) fn floating_panel_offset(&self, panel_id: &str) -> (f32, f32) {
        self.chrome_float_offsets.get(panel_id).copied().unwrap_or((0.0, 0.0))
    }

    /// CC-18 срез 3: the panel's CSS-computed rect, i.e. where it would paint
    /// with no drag offset. `None` when the panel has no box this pass (e.g.
    /// `#infoPanel` while closed).
    pub(crate) fn floating_panel_base_rect(&self, panel_id: &str) -> Option<Rect> {
        self.chrome_floating_detached
            .iter()
            .find(|d| d.panel_id == panel_id)
            .map(|d| d.removed.rect)
    }

    /// BUG-1059 + CC-18 срез 3: (re)builds [`Self::chrome_floating_dl`] from
    /// the boxes [`take_floating_panel`] detached this pass, translating each
    /// by its drag offset.
    ///
    /// The translation is applied to a **clone** of the detached subtree, not
    /// to the stored one: [`Self::chrome_floating_detached`] is put back into
    /// the incremental-layout basis at the top of the next
    /// [`Self::relayout_chrome_host`], and a basis carrying a drag offset
    /// would make the next pass's box reuse graft geometry the cascade never
    /// produced. With no offset (the default for every panel) nothing is
    /// cloned and the emitted commands are byte-identical to what срез 2
    /// produced — the no-drag render path is untouched.
    ///
    /// Cheap enough to call on every drag step: it walks two small subtrees,
    /// no layout and no cascade.
    pub(crate) fn rebuild_chrome_floating_dl(&mut self) {
        let mut dl = lumen_paint::DisplayList::new();
        for detached in &self.chrome_floating_detached {
            match self.chrome_float_offsets.get(detached.panel_id).copied() {
                Some((dx, dy)) if dx != 0.0 || dy != 0.0 => {
                    let mut moved = detached.removed.clone();
                    lumen_layout::translate_subtree(&mut moved, dx, dy);
                    dl.extend_from_slice(&paint_ordered(&moved));
                }
                _ => dl.extend_from_slice(&paint_ordered(&detached.removed)),
            }
        }
        self.chrome_floating_dl = (!dl.is_empty()).then_some(dl);
    }

    /// CC-18 срез 3: hit-tests the floating panels at window CSS coordinates,
    /// topmost first.
    ///
    /// Necessary as a separate entry point rather than a case of
    /// [`Self::chrome_hit_test`]: these boxes are no longer *in*
    /// `chrome_layout` (BUG-1059 detached them), and they sit inside
    /// `chrome_page_host_rect`, where [`Self::point_over_chrome`] answers
    /// "not chrome". The query point is moved by the inverse drag offset
    /// instead of the tree by the offset — same result, no clone.
    ///
    /// Later entries in [`Self::chrome_floating_detached`] paint later, hence
    /// the reverse iteration: `#infoPanel` is modal over `#demoBar`.
    pub(crate) fn floating_panel_hit(
        &self,
        x_css: f32,
        y_css: f32,
    ) -> Option<(&'static str, lumen_paint::HitTestResult)> {
        self.chrome_floating_detached.iter().rev().find_map(|d| {
            let (dx, dy) = self.floating_panel_offset(d.panel_id);
            let (px, py) = (x_css - dx, y_css - dy);
            let r = d.removed.rect;
            if px < r.x || px >= r.right() || py < r.y || py >= r.bottom() {
                return None;
            }
            hit_test(Point::new(px, py), &d.removed).map(|h| (d.panel_id, h))
        })
    }

    /// CC-18 срез 3: routes a left-button press that landed on a floating
    /// panel. Returns `true` when the press belonged to a panel and must not
    /// fall through to the page or the legacy overlays underneath.
    ///
    /// Three outcomes, resolved through the ordinary
    /// [`Self::chrome_action_at`] nearest-ancestor `data-action` lookup:
    ///
    /// * `drag-panel` (the header) — starts a drag, or resets the panel to
    ///   its CSS default when this is the second press of a double-click.
    /// * any other action — dispatched exactly as a press on non-floating
    ///   chrome would be. This is what keeps the panel's own controls working:
    ///   the header's `≡`/`ⓘ` buttons carry their own `data-action`, so the
    ///   bubble walk finds *those* before the header's `drag-panel`, which is
    ///   precisely the `e.target.closest('.demo-switch, .demo-info-btn,
    ///   .demo-expand')` guard the design reference's own `mousedown` handler
    ///   opens with.
    /// * no action — swallowed: the panel is opaque, a press on its
    ///   background must not reach the page behind it.
    pub(crate) fn floating_panel_press(
        &mut self,
        x_css: f32,
        y_css: f32,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) -> bool {
        let Some((panel_id, hit)) = self.floating_panel_hit(x_css, y_css) else { return false };
        match self.chrome_action_at(&hit) {
            Some((_, lumen_chrome::ChromeAction::DragPanel)) => {
                let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
                if is_double_press(self.chrome_float_last_press.as_ref(), panel_id, now_ms, x_css, y_css)
                {
                    // Дабл-клик по шапке — сброс позиции к дефолту текущей
                    // формы (CC-18's task line). Dropping the offset is the
                    // whole reset: the CSS-computed rect is what the panel
                    // paints at with no offset, and layout already has it.
                    self.chrome_float_offsets.remove(panel_id);
                    self.chrome_float_drag = None;
                    self.chrome_float_last_press = None;
                } else if let Some(base) = self.floating_panel_base_rect(panel_id) {
                    // Always `Some` in practice — the hit came out of that
                    // very detachment — but the offset arithmetic needs the
                    // base rect, and a press is not worth a panic over.
                    let (dx, dy) = self.floating_panel_offset(panel_id);
                    self.chrome_float_drag = Some(FloatingPanelDrag {
                        panel_id,
                        grab: (x_css - (base.x + dx), y_css - (base.y + dy)),
                    });
                    self.chrome_float_last_press =
                        Some(FloatingPanelPress { panel_id, at_ms: now_ms, pos: (x_css, y_css) });
                }
                self.rebuild_chrome_floating_dl();
                self.request_redraw();
            }
            Some((nid, action)) => {
                self.chrome_float_last_press = None;
                self.dispatch_chrome_action(nid, action, event_loop);
                self.request_redraw();
            }
            None => {
                self.chrome_float_last_press = None;
            }
        }
        true
    }

    /// CC-18 срез 3: advances an in-progress header drag to the pointer's new
    /// position. Returns `true` when a drag is live (the caller then owns the
    /// motion and skips page hover work, mirroring `panel_resize`).
    ///
    /// Only the offset and the floating display list are rebuilt — no
    /// relayout: the panel's position is a paint-time property here, so the
    /// cascade has nothing to say about it and the box tree stays exactly as
    /// layout left it.
    pub(crate) fn floating_panel_drag_to(&mut self, x_css: f32, y_css: f32) -> bool {
        let Some(drag) = self.chrome_float_drag.as_ref() else { return false };
        let (panel_id, grab) = (drag.panel_id, drag.grab);
        // A panel that lost its box mid-drag (shape switch, `#infoPanel`
        // closed) has nothing to move; the drag ends rather than writing an
        // offset against a rect that no longer exists.
        let Some(base) = self.floating_panel_base_rect(panel_id) else {
            self.chrome_float_drag = None;
            return true;
        };
        let win_w = self.viewport_width_css();
        let win_h = self.window_height_css();
        let offset = drag_offset(base, grab, x_css, y_css, win_w, win_h);
        self.chrome_float_offsets.insert(panel_id, offset);
        self.rebuild_chrome_floating_dl();
        self.request_redraw();
        true
    }

    /// CC-18 срез 3: ends an in-progress header drag. Returns `true` if one
    /// was running. The offset it produced stays — only the tracking stops.
    pub(crate) fn floating_panel_end_drag(&mut self) -> bool {
        self.chrome_float_drag.take().is_some()
    }

    /// CC-18 срез 3: drops `panel_id`'s drag offset, returning it to the
    /// CSS-computed position for its current shape.
    ///
    /// Called by `ChromeAction::SetDemoVariant` as well as by the double-click
    /// reset: an offset is a delta from *one* shape's default `top/left/
    /// right/bottom`, so carrying it across a shape switch would place the new
    /// shape relative to the old shape's anchor — e.g. the 18 px-from-the-
    /// bottom-left "Карточка" offset applied to "Полоса"'s top-centre anchor.
    /// The task line's own wording ("дабл-клик по шапке — сброс позиции")
    /// makes the reset an explicit action, not a promise that a custom
    /// position survives everything else.
    pub(crate) fn floating_panel_reset(&mut self, panel_id: &str) {
        if self.chrome_float_offsets.remove(panel_id).is_some() {
            self.chrome_float_drag = None;
        }
    }
}
