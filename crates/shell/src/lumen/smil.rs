//! GAP-SMIL — per-frame tick for the SVG SMIL timing model.
//!
//! Unlike CSS transitions/animations, SMIL's active-interval bookkeeping and
//! `beginEvent`/`repeatEvent`/`endEvent` dispatch live entirely in the JS
//! shim (`_lumen_tick_smil`, `crates/js/src/svg.rs`): the timing model is
//! DOM-structural (an `<animate>`'s target is its parent element, resolved
//! by walking `parentNode`), not derived from `ComputedStyle`, so there is
//! nothing for a Rust-side scheduler to own. This file only owns the
//! once-per-frame call into that JS function, mirroring
//! `transition_events.rs`'s single-delivery-point shape.

use crate::*;

impl Lumen {
    /// Advance the SMIL timing model one frame.
    ///
    /// Called once per frame from `RedrawRequested`, right after the CSS
    /// transition/animation tick. No-ops before a JS context exists — same
    /// reasoning as `deliver_transition_events`: a page whose markup already
    /// declares `<animate begin="0s">` before script runs must still see it
    /// begin once script attaches its listeners, not have the interval
    /// silently missed by a tick that ran too early.
    #[cfg(feature = "v8")]
    pub(crate) fn tick_smil(&mut self, now_s: f32) {
        if !self.js_present {
            return;
        }
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.tick_smil(now_s);
        });
    }
}
