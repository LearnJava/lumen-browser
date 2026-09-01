//! Two viewport changes the page has not been laid out for yet: an OS
//! fullscreen toggle and a zoom step.
//!
//! Neither can reflow at the moment it happens. The fullscreen size is applied
//! by the OS asynchronously, so it has to be waited for before the ordinary
//! resize + relayout path can run; a zoom step is shown at once as a scaled
//! display list, and the reflow it stands in for happens only after the key
//! presses stop. Both therefore leave state behind in `Lumen` for
//! `crate::app::about_to_wait` to finish.

use crate::*;

impl Lumen {
    /// Arm a viewport reconciliation after an OS fullscreen toggle (BUG-167).
    ///
    /// `prev` is the window's **physical** inner size captured right *before*
    /// `set_fullscreen` was called. The OS applies the new size asynchronously,
    /// so `poll_fullscreen_resize` (run from `about_to_wait`) waits until the
    /// real `inner_size()` differs from `prev`, then drives the resize +
    /// relayout path. The `240` attempt budget (~4 s at 60 fps) prevents a
    /// no-op toggle from spinning the event loop forever.
    pub(crate) fn arm_fullscreen_resize(&mut self, prev: winit::dpi::PhysicalSize<u32>) {
        self.fullscreen_resize_pending = Some((prev.width, prev.height, 240));
        // Wake the loop so `about_to_wait` polls even with ControlFlow::Wait.
        self.request_redraw();
    }

    /// Poll for the OS-applied fullscreen size and, once it differs from the
    /// pre-toggle size, run the same resize + relayout path as
    /// `WindowEvent::Resized` so the page viewport (`vw`/`vh`,
    /// `innerWidth`/`innerHeight`) follows the fullscreen area (BUG-167).
    ///
    /// No-op unless a toggle is pending. Called once per `about_to_wait`.
    pub(crate) fn poll_fullscreen_resize(&mut self) {
        let Some((prev_w, prev_h, attempts)) = self.fullscreen_resize_pending else {
            return;
        };
        // Read the current physical size; the immutable borrow of `self.window`
        // ends before the &mut calls below.
        let cur = match self.window.as_ref() {
            Some(w) => w.inner_size(),
            None => {
                self.fullscreen_resize_pending = None;
                return;
            }
        };
        match decide_fullscreen_poll((prev_w, prev_h), (cur.width, cur.height), attempts) {
            FullscreenPoll::Apply(w, h) => {
                // OS applied the new size: drive the normal resize + relayout path.
                self.fullscreen_resize_pending = None;
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(w, h);
                }
                self.relayout();
                self.runtime
                    .deliver_observer_records(runtime::ObserverKind::Resize);
                self.request_redraw();
            }
            FullscreenPoll::Wait(w, h, left) => {
                self.fullscreen_resize_pending = Some((w, h, left));
                self.request_redraw();
            }
            FullscreenPoll::Done => self.fullscreen_resize_pending = None,
        }
    }

    /// Transform-first zoom step (ADR-016 M0.3).
    ///
    /// Called after `zoom_factor` changed via Ctrl+/-/0. Instead of an immediate
    /// (expensive) relayout, scale the retained display list by
    /// `zoom_factor / laid_out_zoom_factor` on the backend for an instant
    /// response, then arm a debounced relayout so a burst of key presses reflows
    /// only once — `ZOOM_RELAYOUT_DEBOUNCE_MS` after the last press.
    pub(crate) fn begin_zoom_preview(&mut self) {
        let scale = zoom::preview_scale(self.zoom_factor, self.laid_out_zoom_factor);
        if let Some(r) = self.renderer.as_mut() {
            r.set_preview_scale(scale);
        }
        self.pending_zoom_relayout = Some(
            std::time::Instant::now()
                + std::time::Duration::from_millis(zoom::ZOOM_RELAYOUT_DEBOUNCE_MS),
        );
        self.request_redraw();
    }
}
