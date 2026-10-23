//! UX-CARET: glue between the blink clock ([`crate::caret_blink`]) and the
//! carets the shell paints — when one exists, whether this frame draws it, how
//! thick it is, when the loop must wake to flip it.

use std::hash::{Hash, Hasher};
use std::time::Instant;

use super::Lumen;
use crate::caret_blink::caret_width_css;
use crate::platform;

impl Lumen {
    /// Device pixels per CSS px of the window (monitor DPI), without page zoom.
    fn caret_dpi_scale(&self) -> f32 {
        self.renderer.as_ref().map_or(1.0, |r| r.scale_factor() as f32)
    }

    /// Bar width for a caret inside page content (device pixels per page CSS
    /// px = DPI × page zoom), at least one physical pixel at any zoom.
    pub(crate) fn page_caret_width(&self) -> f32 {
        let dpi = self.caret_dpi_scale();
        caret_width_css(&platform::os_params::current(), dpi, dpi * self.zoom_factor)
    }

    /// Bar width for the browser's own fields (address bar), which page zoom
    /// does not scale.
    pub(crate) fn chrome_caret_width(&self) -> f32 {
        let dpi = self.caret_dpi_scale();
        caret_width_css(&platform::os_params::current(), dpi, dpi)
    }

    /// CSS UI L4 §6.2 `caret-animation: manual` on the focused page field: the
    /// UA does not blink its caret (the author animates `caret-color`).
    fn focused_caret_manual(&self) -> bool {
        use lumen_layout::CaretAnimation;
        self.focused_node
            .zip(self.layout_box.as_ref())
            .and_then(|(nid, lb)| crate::forms::find_layout_box(lb, nid))
            .is_some_and(|b| b.style.caret_animation == CaretAnimation::Manual)
    }

    /// Whether some text caret exists to be painted: the window has focus and
    /// a page or frame field, or the address bar, is being edited.
    pub(crate) fn caret_present(&self) -> bool {
        self.window_focused
            && (self.focused_input_caret().is_some()
                || self.focused_textarea_caret().is_some()
                || self.focused_frame_input_caret().is_some()
                || self.focused_frame_textarea_caret().is_some()
                || (self.address_bar.is_open() && self.address_bar.selected_idx().is_none()))
    }

    /// Hash of what makes the caret "move": which field and where (typing
    /// moves the cursor, and a key press restarts the phase anyway), the
    /// address bar's text. A different value restarts the blink phase.
    fn caret_signature(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.focused_node.hash(&mut h);
        if let Some(nid) = self.focused_node {
            self.form_state.get(&nid).map(|s| (s.cursor, s.selection_anchor)).hash(&mut h);
        }
        self.focused_frame.hash(&mut h);
        if let Some((idx, nid)) = self.focused_frame {
            self.frame_text_cursor.get(&(idx, nid)).hash(&mut h);
            self.frame_text_selection_anchor.get(&(idx, nid)).hash(&mut h);
        }
        if self.address_bar.is_open() {
            self.address_bar.input().hash(&mut h);
            self.address_bar.cursor().hash(&mut h);
            self.address_bar.selection().hash(&mut h);
        }
        h.finish()
    }

    /// Restarts the visible phase: a key press or click, even when it leaves
    /// the caret where it was, shows the bar at once.
    pub(crate) fn caret_restart(&mut self) {
        self.caret_blink.restart(Instant::now());
    }

    /// The caret's phase for the frame being built: `true` — draw the bar.
    /// Deterministic runs (goldens, graphic tests) never blink, so a screenshot
    /// does not depend on when it is taken.
    pub(crate) fn caret_frame_visible(&mut self) -> bool {
        let now = Instant::now();
        let sig = self.caret_signature();
        self.caret_blink.observe(sig, now);
        let visible = self.deterministic.enabled
            || self.focused_caret_manual()
            || self.caret_blink.visible(now, &platform::os_params::current());
        self.caret_blink.note_painted(visible);
        visible && self.window_focused
    }

    /// Event-loop hook: asks for a repaint when the phase flipped since the
    /// last frame and returns the next instant the loop must wake at (`None`
    /// — nothing blinks: no caret, window in the background, solid caret).
    pub(crate) fn caret_next_wakeup(&mut self) -> Option<Instant> {
        if self.deterministic.enabled || !self.caret_present() || self.focused_caret_manual() {
            return None;
        }
        let now = Instant::now();
        let os = platform::os_params::current();
        self.caret_blink.observe(self.caret_signature(), now);
        if self.caret_blink.repaint_due(now, &os)
            && let Some(w) = self.window.as_ref()
        {
            w.request_redraw();
        }
        self.caret_blink.next_change(now, &os)
    }

    /// `WindowEvent::Focused`: a background window draws no caret (the page
    /// keeps its focus, so coming back shows the bar at once).
    pub(crate) fn set_window_focused(&mut self, focused: bool) {
        if self.window_focused == focused {
            return;
        }
        self.window_focused = focused;
        self.caret_restart();
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}
