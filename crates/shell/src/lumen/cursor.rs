//! Choosing the OS cursor for whatever is under the pointer.
//!
//! `CursorMoved` can fire hundreds of times a second, so the resolved icon is
//! compared against `last_cursor_icon` and `Window::set_cursor` is only called
//! when it actually changes - the FFI call is the expensive part, not the
//! lookup. The CSS `cursor` value to winit mapping is
//! `crate::input::winit_events`.

use crate::*;

impl Lumen {
    /// Геометрия горизонтальной полосы страницы: `(ширина области страницы,
    /// её левый край в окне, есть ли вертикальная полоса)`.
    pub(crate) fn hbar_layout(&self) -> (f32, f32, bool) {
        (
            self.page_content_width_css(),
            self.docked_panel_offsets().0,
            self.content_height > self.viewport_height_css(),
        )
    }

    /// Куда попала точка (сырые оконные CSS px) относительно горизонтальной
    /// полосы страницы. `None` при `--no-scrollbar`.
    pub(crate) fn hscroll_hit(&self, x_css: f32, y_css: f32) -> scrollbar::TrackClick {
        if self.no_scrollbar {
            return scrollbar::TrackClick::None;
        }
        let (vw, origin_x, v_present) = self.hbar_layout();
        scrollbar::classify_hscroll_click(
            x_css,
            y_css,
            self.scroll_x,
            self.content_width,
            vw,
            self.viewport_height_css(),
            origin_x,
            v_present,
        )
    }

    /// Окраска thumb-ов страницы `(вертикальный, горизонтальный)`: drag —
    /// `Active`, курсор над thumb-ом — `Hover`, иначе `Idle`.
    pub(crate) fn page_thumb_states(&self) -> (scrollbar::ThumbState, scrollbar::ThumbState) {
        use scrollbar::{ThumbState, TrackClick};
        let hover = self.renderer.as_ref().zip(self.cursor_position).map(|(r, pos)| {
            let dpr = (r.scale_factor() as f32).max(1e-6);
            let (x, y) = ((pos.x as f32) / dpr, (pos.y as f32) / dpr);
            let v = scrollbar::classify_track_click(
                x,
                y,
                self.scroll_y,
                self.content_height,
                self.viewport_width_css(),
                self.viewport_height_css(),
            );
            (v == TrackClick::Thumb, self.hscroll_hit(x, y) == TrackClick::Thumb)
        });
        let (v_hover, h_hover) = hover.unwrap_or((false, false));
        let pick = |dragging: bool, hovering: bool| {
            if dragging {
                ThumbState::Active
            } else if hovering {
                ThumbState::Hover
            } else {
                ThumbState::Idle
            }
        };
        (
            pick(self.scroll_drag.is_some(), v_hover),
            pick(self.hscroll_drag.is_some(), h_hover),
        )
    }

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
        let hscroll_icon = cursor_icon_for_hover(
            self.hscroll_hit(x_css, y_css),
            self.hscroll_drag.is_some(),
        );
        let scrollbar_icon = match cursor_icon_for_hover(hover, self.scroll_drag.is_some()) {
            CursorIcon::Default => hscroll_icon,
            icon => icon,
        };

        // UX-SCROLLBAR: hover/active окраска thumb-а — перерисовать при смене.
        let thumb_states = self.page_thumb_states();
        if thumb_states != self.last_thumb_states {
            self.last_thumb_states = thumb_states;
            self.request_redraw();
        }

        // FRAME-3 remainder: собственный scrollbar фрейма — та же
        // приоритетность, что у страничного, но проверяется РАНЬШЕ (мышь
        // над курсором может одновременно быть "внутри" страничного трека
        // справа И над фреймом, если фрейм растянут до самого края; самый
        // глубокий скроллер должен выигрывать, как и у клика).
        let frame_hover = self
            .classify_frame_scrollbar_click(x_css, y_css)
            .map_or(scrollbar::TrackClick::None, |(_, click, _)| click);
        let frame_scrollbar_icon =
            cursor_icon_for_hover(frame_hover, self.frame_scroll_drag.is_some());

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
        } else if frame_scrollbar_icon != CursorIcon::Default {
            frame_scrollbar_icon
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
