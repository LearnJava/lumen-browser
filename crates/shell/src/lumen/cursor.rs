//! Choosing the OS cursor for whatever is under the pointer.
//!
//! `CursorMoved` can fire hundreds of times a second, so the resolved icon is
//! compared against `last_cursor_icon` and `Window::set_cursor` is only called
//! when it actually changes - the FFI call is the expensive part, not the
//! lookup. The CSS `cursor` value to winit mapping is
//! `crate::input::winit_events`.

use crate::*;

impl Lumen {
    /// Пересчитать желаемый `CursorIcon` по текущей позиции курсора и
    /// при изменении вызвать `Window::set_cursor`. CursorMoved может
    /// дёргаться сотни раз в секунду — `last_cursor_icon` кэширует
    /// предыдущее значение, чтобы не делать лишний FFI-вызов в winit.
    pub(crate) fn update_cursor_icon(&mut self) {
        let (Some(window), Some(renderer), Some(pos)) =
            (self.window.as_ref(), self.renderer.as_ref(), self.cursor_position)
        else {
            return;
        };
        let dpr = (renderer.scale_factor() as f32).max(1e-6);
        let x_css = (pos.x as f32) / dpr;
        let y_css = (pos.y as f32) / dpr;

        // Scrollbar takes highest priority.
        let hover = scrollbar::classify_track_click(
            x_css,
            y_css,
            self.scroll_y,
            self.content_height,
            self.viewport_width_css(),
            self.viewport_height_css(),
        );
        let scrollbar_icon = cursor_icon_for_hover(hover, self.scroll_drag.is_some());

        // F2-6: a docked-panel resize drag (or hovering an edge) shows the
        // horizontal-resize cursor, ahead of scrollbar/page/chrome hover.
        let desired = if self.panel_resize.is_some() || self.resize_edge_at(x_css, y_css).is_some() {
            CursorIcon::EwResize
        } else if self.point_over_chrome(x_css, y_css) {
            // CC-5: the engine-drawn chrome owns the cursor over its own
            // opaque area (sidebar, toolbar, tab strip) — ahead of
            // scrollbar/page hit-test below, which assume page coordinates.
            match self.chrome_hit_test(x_css, y_css) {
                Some(result) => css_cursor_to_winit(result.cursor),
                None => CursorIcon::Default,
            }
        } else if scrollbar_icon != CursorIcon::Default {
            scrollbar_icon
        } else if let Some(lb) = &self.layout_box {
            // Hit-test layout tree in page coordinates (viewport + scroll offset).
            let (offset_x, offset_y) = self.page_offset();
            let page_x = (x_css - offset_x) + self.scroll_x;
            let page_y = (y_css - offset_y) + self.scroll_y;
            match hit_test(Point::new(page_x, page_y), lb) {
                Some(result) => css_cursor_to_winit(result.cursor),
                None => CursorIcon::Default,
            }
        } else {
            CursorIcon::Default
        };

        if self.last_cursor_icon != Some(desired) {
            window.set_cursor(desired);
            self.last_cursor_icon = Some(desired);
        }
    }
}
