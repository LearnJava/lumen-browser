//! Page zoom commands: keyboard, Ctrl+wheel and the omnibox indicator all land
//! here, and the zoom is remembered per host (UX-ZOOM).

use crate::*;

impl Lumen {
    /// Applies `zoom` to the current tab: transform-first preview now, debounced
    /// relayout later, remembered for the host, indicator refreshed.
    pub(crate) fn set_page_zoom(&mut self, zoom: f32) {
        let zoom = zoom.clamp(zoom::ZOOM_MIN, zoom::ZOOM_MAX);
        if (zoom - self.zoom_factor).abs() < f32::EPSILON {
            return;
        }
        self.zoom_factor = zoom;
        if let Some(host) = self.zoom_host.clone() {
            zoom::remember_site_zoom(&host, zoom);
        }
        self.begin_zoom_preview();
        self.relayout_chrome_host();
    }

    /// One zoom step; `up` enlarges.
    pub(crate) fn step_page_zoom(&mut self, up: bool) {
        let next = if up { zoom::zoom_in(self.zoom_factor) } else { zoom::zoom_out(self.zoom_factor) };
        self.set_page_zoom(next);
    }

    /// Back to 100 % (Ctrl+0, click on the omnibox indicator).
    pub(crate) fn reset_page_zoom(&mut self) {
        self.set_page_zoom(zoom::zoom_reset());
    }

    /// Follows the tab's host: when it changes (navigation, tab switch) the
    /// zoom becomes the one remembered for the new host. Cheap when the host
    /// is the same, so `about_to_wait` calls it every turn.
    pub(crate) fn sync_site_zoom(&mut self) {
        let host = self.source.url_str().and_then(zoom::host_of);
        if host == self.zoom_host {
            return;
        }
        let target = host.as_deref().map_or(zoom::ZOOM_DEFAULT, zoom::site_zoom);
        self.zoom_host = host;
        if (target - self.zoom_factor).abs() >= f32::EPSILON {
            self.zoom_factor = target;
            self.begin_zoom_preview();
            self.relayout_chrome_host();
        }
    }
}
