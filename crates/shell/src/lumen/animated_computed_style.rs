//! GAP-CSSANIM срез 3 — `getComputedStyle()` during an active CSS
//! transition/animation now reflects the live interpolated `opacity`/
//! `transform`, not just the last full-relayout snapshot.
//!
//! Same single-delivery-point shape as `transition_events.rs`/
//! `animation_events.rs`: the schedulers themselves only produce an
//! `AnimationFrame` of interpolated values (already used to patch the
//! display list without relayout, BUG-231), so the shell reads that same
//! frame once per tick and pushes its `opacity`/`transform` overrides into
//! the JS-visible computed-style snapshot `getComputedStyle()` reads.

use crate::*;

impl Lumen {
    /// Patch the JS-visible computed-style snapshot with this frame's live
    /// interpolated `opacity`/`transform` overrides.
    ///
    /// Called once per frame from `RedrawRequested`, right after
    /// `self.anim_frame` is set from this tick's scheduler output — unlike
    /// the transition/animation event queues, there is nothing to drain: a
    /// frame with no active animation just has an empty `anim_frame` and
    /// this is a no-op.
    #[cfg(feature = "v8")]
    pub(crate) fn patch_animated_computed_styles(&mut self) {
        let Some(frame) = &self.anim_frame else {
            return;
        };
        let patches = frame.to_computed_style_patches();
        if patches.is_empty() {
            return;
        }
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.patch_animated_computed_styles(&patches);
        });
    }
}
