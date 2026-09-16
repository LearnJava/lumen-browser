//! GAP-CSSANIM срез 1 — delivery of CSS Transitions L1 §3 lifecycle events
//! (`transitionrun`/`transitionstart`/`transitionend`/`transitioncancel`)
//! queued by `TransitionScheduler::sync`/`tick`.
//!
//! Same shape as `content_visibility.rs` (BUG-852): the scheduler itself has
//! no notion of JS or events, it only tracks interpolated values, so the
//! shell accumulates `lumen_layout::TransitionEventInfo` from every producer
//! (`apply_relayout_result`'s `sync()` calls, `RedrawRequested`'s `tick()`)
//! into one queue and drains it from a single point in `RedrawRequested`,
//! once a JS context is known to exist.

use crate::*;
use lumen_layout::TransitionEventKind;

impl Lumen {
    /// Deliver queued transition lifecycle events to JS as `TransitionEvent`s.
    ///
    /// Called once per frame from `RedrawRequested`, right after the page's
    /// `TransitionScheduler::tick()` — the same point `apply_relayout_result`'s
    /// `sync()` calls feed into via `self.transition_events`.
    #[cfg(feature = "v8")]
    pub(crate) fn deliver_transition_events(&mut self) {
        if self.transition_events.is_empty() || !self.js_present {
            // No JS context yet: keep the queue so the page's own listeners,
            // once attached, still see the transitions it declared in markup.
            return;
        }
        let payload: String = {
            let mut s = String::from("[");
            for (i, ev) in self.transition_events.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                let kind = match ev.kind {
                    TransitionEventKind::Run => "run",
                    TransitionEventKind::Start => "start",
                    TransitionEventKind::End => "end",
                    TransitionEventKind::Cancel => "cancel",
                };
                s.push_str(&format!(
                    "[{},{},{},{}]",
                    ev.node.index(),
                    js_string_literal(kind),
                    js_string_literal(&ev.property),
                    ev.elapsed_time
                ));
            }
            s.push(']');
            s
        };
        self.transition_events.clear();
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.deliver_transition_events(&payload);
        });
    }
}
