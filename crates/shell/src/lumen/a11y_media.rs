//! Telling the page that an accessibility preference changed.
//!
//! The a11y panel writes `prefers-reduced-motion` (and the rest of
//! `a11y_store`) without touching the document, so nothing in the ordinary
//! relayout path re-evaluates the page's media queries. This pushes the same
//! four values `crate::persistent_js` delivers on a resize, which is what makes
//! a standing `matchMedia` listener fire when the panel closes.

use crate::*;

impl Lumen {
    /// Re-deliver media query changes to JS after accessibility prefs change.
    ///
    /// Called when the a11y panel closes so `prefers-reduced-motion` MQLs fire.
    pub(crate) fn deliver_a11y_media_changes(&self) {
        #[cfg(feature = "v8")]
        {
            let w = self.viewport_width_css();
            let h = self.viewport_height_css();
            let dark = if self.dark_mode { "true" } else { "false" };
            let rm = if self.a11y_store.reduced_motion() { "true" } else { "false" };
            // ADR-016 M2.2d: fire-and-forget eval via route_eval_js (off-UI-thread
            // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                format!(
                    "if(typeof _lumen_deliver_media_changes==='function')\
                     _lumen_deliver_media_changes({w},{h},{dark},{rm});"
                ),
            );
        }
    }
}
