//! GAP-CSSANIM срез 2 — delivery of CSS Animations L1 §4.5.1 lifecycle events
//! (`animationstart`/`animationiteration`/`animationend`/`animationcancel`)
//! queued by `animation_scheduler::AnimationScheduler::tick`.
//!
//! Same shape as `transition_events.rs` (GAP-CSSANIM срез 1): the scheduler
//! itself has no notion of JS, it only tracks interpolated values plus a
//! small per-instance lifecycle state machine, so the shell accumulates
//! `crate::animation_scheduler::AnimationEventInfo` into one queue and
//! drains it from a single point in `RedrawRequested`, once a JS context is
//! known to exist.

use crate::*;
use crate::animation_scheduler::AnimationEventKind;

impl Lumen {
    /// Deliver queued animation lifecycle events to JS as `AnimationEvent`s.
    ///
    /// Called once per frame from `RedrawRequested`, right after
    /// `deliver_transition_events` — the same point `animation_scheduler.tick()`
    /// feeds into via `self.animation_events`.
    #[cfg(feature = "v8")]
    pub(crate) fn deliver_animation_events(&mut self) {
        if self.animation_events.is_empty() || !self.js_present {
            // No JS context yet: keep the queue so the page's own listeners,
            // once attached, still see the animations it declared in markup.
            return;
        }
        let payload: String = {
            let mut s = String::from("[");
            for (i, ev) in self.animation_events.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                let kind = match ev.kind {
                    AnimationEventKind::Start => "start",
                    AnimationEventKind::Iteration => "iteration",
                    AnimationEventKind::End => "end",
                    AnimationEventKind::Cancel => "cancel",
                };
                s.push_str(&format!(
                    "[{},{},{},{}]",
                    ev.node.index(),
                    js_string_literal(kind),
                    js_string_literal(&ev.animation_name),
                    ev.elapsed_time
                ));
            }
            s.push(']');
            s
        };
        self.animation_events.clear();
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.deliver_animation_events(&payload);
        });
    }
}
