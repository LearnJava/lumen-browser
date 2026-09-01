//! Mouse-drag text selection inside a focused typeable field (FRAME-7
//! остаток: drag мышью — Shift+arrow selection is [`super::text_input`]/
//! [`super::frame_text_input`]'s `extend_focused_selection*` family, wired
//! from `keyboard.rs`).
//!
//! [`Lumen::begin_text_drag_select`] runs right after `handle_click_at` on a
//! left-button press, so `focused_node`/`focused_frame` already carry that
//! click's outcome: if it landed on a typeable field (page or frame), the
//! click point is hit-tested against the field's own layout box to place
//! BOTH the cursor and the selection anchor there, and `self.text_drag` is
//! armed. Every subsequent `CursorMoved` while it stays armed
//! ([`Lumen::update_text_drag_select`], called from `cursor_moved.rs`) moves
//! only the cursor — never the anchor — to the char under the new pointer
//! position, growing or shrinking the selection exactly like a Shift+arrow
//! extension does. `ElementState::Released` in `mouse_input.rs` disarms it;
//! the selection itself is left in place, same as every other text editor
//! leaves a selection standing once the button comes up.

use crate::*;

/// Which typeable field owns an in-progress mouse-drag selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextDragTarget {
    /// A page-level `<input>`/`<textarea>`, addressed by `focused_node`.
    Page(lumen_dom::NodeId),
    /// A typeable field INSIDE a frame's content, addressed the same way
    /// [`Lumen::focused_frame`] is — `(frame index, its own NodeId)`.
    Frame(usize, lumen_dom::NodeId),
}

/// Char-index under document-space `(x, y)` inside `field_lb`, dispatching on
/// `kind` — shared by the page and frame halves of both entry points below so
/// the `<textarea>` font/measurer setup (this crate has no bundled
/// `TextMeasurer`, same gap `redraw_requested.rs`'s caret/selection overlays
/// already work around) is written once. `None` only if the bundled font
/// fails to parse — never happens outside a corrupted binary, but a hit test
/// must not `unwrap` its way through a paint-adjacent path.
fn char_index_at(
    field_lb: &LayoutBox,
    kind: TypeableField,
    value: &str,
    x: f32,
    y: f32,
) -> Option<usize> {
    match kind {
        TypeableField::Input => Some(forms::input_char_index_at_x(field_lb, value, x)),
        TypeableField::Textarea => {
            let font = lumen_font::Font::parse(INTER_FONT).ok()?;
            let m = lumen_paint::FontMeasurer::new(&font).ok()?;
            let fs = field_lb.style.font_size;
            let measure = |s: &str| -> f32 {
                use lumen_layout::TextMeasurer;
                s.chars().map(|c| m.char_width(c, fs)).sum()
            };
            Some(forms::textarea_char_index_at_point(field_lb, value, x, y, &measure))
        }
    }
}

impl Lumen {
    /// Start a mouse-drag text selection if `(x_css, y_css)` lands on the
    /// field `handle_click_at` just focused — see the module doc comment for
    /// the calling convention. A frame field's box lives in the frame's OWN
    /// document space; [`frames::frame_page_origin`] translates the click
    /// point into it, the same offset the caret/selection paint site
    /// (`redraw_requested.rs`) already applies in the opposite direction. A
    /// page field's box and [`Self::page_point`]'s output already share one
    /// coordinate space (BUG-437), so no translation is needed there.
    pub(crate) fn begin_text_drag_select(&mut self, x_css: f32, y_css: f32) {
        self.text_drag = None;
        if let Some((idx, nid)) = self.focused_frame
            && let Some((kind, current)) = self.frame_typeable_field(idx, nid)
            && let Some((ox, oy)) = frames::frame_page_origin(&self.frames, idx)
        {
            let (page_x, page_y) = self.page_point(x_css, y_css);
            let (fx, fy) = (page_x - ox, page_y - oy);
            let Some(field_lb) = self
                .frames
                .get(idx)
                .and_then(|h| h.layout.as_ref())
                .and_then(|lb| forms::find_layout_box(lb, nid))
            else {
                return;
            };
            let Some(cursor) = char_index_at(field_lb, kind, &current, fx, fy) else { return };
            self.frame_text_cursor.insert((idx, nid), cursor);
            self.frame_text_selection_anchor.insert((idx, nid), cursor);
            self.text_drag = Some(TextDragTarget::Frame(idx, nid));
            self.request_redraw();
            return;
        }
        let Some(nid) = self.focused_node else { return };
        let Some((kind, current)) = self.typeable_field(nid) else { return };
        let (page_x, page_y) = self.page_point(x_css, y_css);
        let Some(field_lb) =
            self.layout_box.as_ref().and_then(|lb| forms::find_layout_box(lb, nid))
        else {
            return;
        };
        let Some(cursor) = char_index_at(field_lb, kind, &current, page_x, page_y) else {
            return;
        };
        let slot = self.form_state.entry(nid).or_default();
        slot.cursor = Some(cursor);
        slot.selection_anchor = Some(cursor);
        self.text_drag = Some(TextDragTarget::Page(nid));
        self.request_redraw();
    }

    /// Extend an in-progress mouse-drag selection to the char under
    /// `(x_css, y_css)` — called from `cursor_moved.rs` while `self.text_drag`
    /// is armed. Moves ONLY the cursor, never the anchor
    /// [`Self::begin_text_drag_select`] pinned, same rule
    /// `extend_focused_selection`/`extend_focused_frame_selection` follow for
    /// Shift+arrow. A no-op once the target field stops being typeable (e.g.
    /// a script disabled it mid-drag) — the drag simply stalls rather than
    /// panicking or reviving a stale field.
    pub(crate) fn update_text_drag_select(&mut self, x_css: f32, y_css: f32) {
        let Some(target) = self.text_drag else { return };
        match target {
            TextDragTarget::Page(nid) => {
                let Some((kind, current)) = self.typeable_field(nid) else { return };
                let (page_x, page_y) = self.page_point(x_css, y_css);
                let Some(field_lb) =
                    self.layout_box.as_ref().and_then(|lb| forms::find_layout_box(lb, nid))
                else {
                    return;
                };
                let Some(cursor) = char_index_at(field_lb, kind, &current, page_x, page_y) else {
                    return;
                };
                self.form_state.entry(nid).or_default().cursor = Some(cursor);
                self.request_redraw();
            }
            TextDragTarget::Frame(idx, nid) => {
                let Some((kind, current)) = self.frame_typeable_field(idx, nid) else { return };
                let Some((ox, oy)) = frames::frame_page_origin(&self.frames, idx) else { return };
                let (page_x, page_y) = self.page_point(x_css, y_css);
                let (fx, fy) = (page_x - ox, page_y - oy);
                let Some(field_lb) = self
                    .frames
                    .get(idx)
                    .and_then(|h| h.layout.as_ref())
                    .and_then(|lb| forms::find_layout_box(lb, nid))
                else {
                    return;
                };
                let Some(cursor) = char_index_at(field_lb, kind, &current, fx, fy) else {
                    return;
                };
                self.frame_text_cursor.insert((idx, nid), cursor);
                self.request_redraw();
            }
        }
    }
}
