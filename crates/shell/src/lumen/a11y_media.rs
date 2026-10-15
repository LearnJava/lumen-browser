//! Telling the page that an accessibility preference changed.
//!
//! The a11y panel writes `prefers-reduced-motion` (and the rest of
//! `a11y_store`) without touching the document, so nothing in the ordinary
//! relayout path re-evaluates the page's media queries. This pushes the same
//! five values `crate::persistent_js` delivers on a resize, which is what makes
//! a standing `matchMedia` listener fire when the panel closes.

use crate::*;

impl Lumen {
    /// `prefers-reduced-motion: reduce` — the user's a11y panel choice, or the
    /// OS "show animations" switch turned off (UX-OSPARAMS).
    pub(crate) fn effective_reduced_motion(&self) -> bool {
        self.a11y_store.reduced_motion() || platform::os_params::current().reduce_motion
    }

    /// Reacts to the OS input settings having changed (`WM_SETTINGCHANGE`
    /// re-read by the `platform::os_params` watcher). Consumers read the
    /// registry on every use, so only the media queries need a push.
    pub(crate) fn apply_os_params_change(&mut self) {
        let generation = platform::os_params::generation();
        if generation == self.os_params_gen {
            return;
        }
        self.os_params_gen = generation;
        self.relayout_chrome();
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    /// Re-deliver media query changes to JS after accessibility prefs change.
    ///
    /// Called when the a11y panel closes so `prefers-reduced-motion` and `forced-colors` MQLs fire.
    pub(crate) fn deliver_a11y_media_changes(&self) {
        #[cfg(feature = "v8")]
        {
            let w = self.viewport_width_css();
            let h = self.viewport_height_css();
            let dark = if self.dark_mode { "true" } else { "false" };
            let rm = if self.effective_reduced_motion() { "true" } else { "false" };
            let fc = if self.a11y_store.forced_colors() { "true" } else { "false" };
            // ADR-016 M2.2d: fire-and-forget eval via route_eval_js (off-UI-thread
            // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                format!(
                    "if(typeof _lumen_deliver_media_changes==='function')\
                     _lumen_deliver_media_changes({w},{h},{dark},{rm},{fc});"
                ),
            );
        }
    }
}
