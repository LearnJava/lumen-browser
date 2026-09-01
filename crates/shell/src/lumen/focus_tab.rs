//! Tab / Shift+Tab sequential focus navigation on the page (FRAME-7 срез 2:
//! Tab-driven focus change did not exist at all before this slice) and,
//! since срез 4, WITHIN a frame's own document.
//!
//! The order itself is pure DOM logic in [`crate::focus_nav`]; this module
//! is the side-effecting glue that mirrors what a click does to
//! `focused_node`/`focused_frame` (`click.rs::handle_click_at_inner`) so
//! `:focus`/`:focus-within` restyle, the platform accessibility bridge and
//! `document.activeElement` all stay in sync however focus moved.
//!
//! [`Lumen::advance_frame_focus`] (срез 4) walks the frame's OWN document —
//! `set_interactive_state`/relayout-on-focus-change (BUG-480 срез 23) already
//! makes that restyle visible, so this slice only needed the traversal glue,
//! not a new painting path. The remaining FRAME-7 gap is the CARET bar
//! itself inside a frame field — a separate, larger slice (the
//! `CompositorOverride` channel a page caret rides is not wired to a frame's
//! `content_dl` at all, see `STATUS-P1.md`'s FRAME-7 row).

use crate::*;

impl Lumen {
    /// Move `focused_node` to the next (`forward`) or previous focusable
    /// element in the page's Tab order, wrapping around, and apply the same
    /// side effects a click does when it changes focus. Returns `true` iff
    /// focus actually moved (a redraw is warranted) — `false` for a page
    /// with no focusable elements, or one whose single focusable element was
    /// already focused.
    pub(crate) fn advance_page_focus(&mut self, forward: bool) -> bool {
        let Some(src) = self.layout_source.as_ref() else {
            return false;
        };
        let next = {
            let doc = src.document.lock().unwrap_or_else(|e| e.into_inner());
            let flat_tree = lumen_dom::build_flat_tree(&doc);
            focus_nav::next_focus_target(&doc, &flat_tree, doc.root(), self.focused_node, forward)
        };
        let Some(next) = next else {
            return false;
        };
        if Some(next) == self.focused_node && self.focused_frame.is_none() {
            return false;
        }
        self.focused_node = Some(next);
        let before_frame = self.focused_frame.take();
        if before_frame.is_some() {
            self.notify_frame_focus(before_frame, None);
            self.refresh_frames(None);
        }
        // Same reasoning as `click.rs`'s focus-change branch: a pure restyle,
        // so it goes off-thread under `LUMEN_ENGINE_THREAD=1` and stays a
        // synchronous UI-handle call otherwise.
        self.relayout_chrome();
        self.platform_bridge.focused_node_changed(self.focused_node);
        let focus_idx = self.focused_node.map(|n| n.index() as u32);
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
            js.notify_focus_changed(focus_idx);
        });
        true
    }

    /// Move focus to the next (`forward`) or previous focusable node WITHIN
    /// frame `idx`'s own document (FRAME-7 срез 4) — the nested browsing
    /// context's own composed-order walk, HTML Standard §6.6.6.
    ///
    /// Deliberately non-wrapping ([`focus_nav::next_focus_target_no_wrap`]):
    /// `false` once the frame's own order is exhausted (or it has no
    /// focusable node at all), so the caller (`keyboard.rs`'s Tab handler)
    /// falls back to [`Self::advance_page_focus`] to leave the frame —
    /// `self.focused_node` still addresses the `<iframe>` host at that point
    /// (set by the click that entered the frame), so that call lands on
    /// exactly the page-level sibling before/after it, same as it always did
    /// for a frame with zero focusable fields.
    pub(crate) fn advance_frame_focus(&mut self, idx: usize, forward: bool) -> bool {
        let Some(handle) = self.frames.get(idx) else {
            return false;
        };
        let current = self.focused_frame.and_then(|(i, n)| (i == idx).then_some(n));
        let next = {
            let doc = handle.doc.lock().unwrap_or_else(|e| e.into_inner());
            let flat_tree = lumen_dom::build_flat_tree(&doc);
            focus_nav::next_focus_target_no_wrap(&doc, &flat_tree, doc.root(), current, forward)
        };
        let Some(next) = next else {
            return false;
        };
        let before = self.focused_frame;
        self.focused_frame = Some((idx, next));
        self.notify_frame_focus(before, self.focused_frame);
        self.refresh_frames(None);
        true
    }
}
