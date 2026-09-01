//! Tab / Shift+Tab sequential focus navigation on the page (FRAME-7 срез 2:
//! Tab-driven focus change did not exist at all before this slice).
//!
//! The order itself is pure DOM logic in [`crate::focus_nav`]; this module
//! is the side-effecting glue that mirrors what a click does to
//! `focused_node`/`focused_frame` (`click.rs::handle_click_at_inner`) so
//! `:focus`/`:focus-within` restyle, the platform accessibility bridge and
//! `document.activeElement` all stay in sync however focus moved.
//!
//! Frame-interior Tab navigation is a separate remainder (the frame has no
//! `set_interactive_state` call at all yet, same gap that blocks its caret —
//! see `STATUS-P1.md`'s FRAME-7 row): pressing Tab while `focused_frame` is
//! `Some` leaves the frame the same way a page click on the host does,
//! landing on the next page-level target after the `<iframe>`.

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
}
