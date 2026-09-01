//! The handful of one-line answers the rest of the shell asks `Lumen` for:
//! ask the OS window for a frame, what URL the address bar is showing, what
//! colour space to render into, and how many CSS pixels of viewport the page
//! actually gets.
//!
//! The viewport figures are not `window.inner_size()` divided by the scale
//! factor - every piece of chrome that eats vertical space (the tab strip, a
//! docked sidebar, the find bar) and the page zoom are folded in here, which
//! is why layout, paint, scrolling and the automation surfaces all read the
//! page's size through these four methods rather than computing it again.

use crate::*;

impl Lumen {
    pub(crate) fn request_redraw(&self) {
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    /// Returns the URL to display in the address bar and use for history / bookmarks.
    ///
    /// When `history.pushState` / `history.replaceState` has updated the virtual
    /// URL without a page load, `display_url` overrides the real `source` URL.
    pub(crate) fn current_display_url(&self) -> &str {
        self.display_url
            .as_deref()
            .or_else(|| self.source.url_str())
            .unwrap_or("")
    }

    /// Returns the detected target `ColorSpace` for the active display.
    ///
    /// Used by the paint layer to decide wide-gamut output (Step 4) and
    /// by ICC transforms (Step 2). Defaults to `ColorSpace::Srgb` when
    /// the OS query fails or the display is sRGB-only.
    #[allow(dead_code)] // consumer: ph3-color-management Steps 2+4
    pub(crate) fn target_color_space(&self) -> ColorSpace {
        self.display_color_profile.active_profile()
    }

    /// Текущая логическая (CSS px) высота viewport-а. Если окно ещё не создано —
    /// fallback на layout-viewport 720 px, который у нас hardcoded в pipeline.
    pub(crate) fn viewport_height_css(&self) -> f32 {
        let total = match (self.window.as_ref(), self.renderer.as_ref()) {
            (Some(w), Some(r)) => {
                let phys = w.inner_size().height as f32;
                let dpr = (r.scale_factor() as f32).max(1e-6);
                phys / dpr
            }
            _ => 720.0,
        };
        let ws_bar = if self.workspace_panel.visible {
            panels::workspace_panel::SWITCHER_HEIGHT
        } else {
            0.0
        };
        (total - toolbar::CHROME_H - ws_bar).max(0.0)
    }

    /// Full logical (CSS px) window height including the tab bar. Used to
    /// clamp the tab context menu (CC-4) so it stays on-screen. Fallback 720.
    pub(crate) fn window_height_css(&self) -> f32 {
        match (self.window.as_ref(), self.renderer.as_ref()) {
            (Some(w), Some(r)) => {
                let phys = w.inner_size().height as f32;
                let dpr = (r.scale_factor() as f32).max(1e-6);
                phys / dpr
            }
            _ => 720.0,
        }
    }

    /// CSS px ширина viewport-а — полная ширина окна, нужна scrollbar-overlay-у
    /// для размещения у правого края. Fallback на layout-viewport 1024 px (тот
    /// же hardcoded размер, что и в pipeline до создания окна).
    pub(crate) fn viewport_width_css(&self) -> f32 {
        match (self.window.as_ref(), self.renderer.as_ref()) {
            (Some(w), Some(r)) => {
                let phys = w.inner_size().width as f32;
                let dpr = (r.scale_factor() as f32).max(1e-6);
                phys / dpr
            }
            _ => 1024.0,
        }
    }

    /// CSS px ширина области контента страницы — полная ширина окна минус
    /// ширина вертикальных панелей вкладок (слева) и sidebar (справа), если
    /// они видимы. Используется для клампинга горизонтального скролла.
    pub(crate) fn page_content_width_css(&self) -> f32 {
        let (left_offset, right_offset) = self.docked_panel_offsets();
        (self.viewport_width_css() - left_offset - right_offset).max(0.0)
    }
}
