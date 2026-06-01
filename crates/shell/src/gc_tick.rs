//! Idle DOM GC tick — drain dead node IDs every [`GC_INTERVAL`] seconds.
//!
//! The DOM arena is append-only in Phase 1 (physical compaction is Phase 3),
//! but JS-side maps (event listeners, input values) still hold per-node entries
//! that keep memory alive for detached nodes. `GcTick::poll` identifies those
//! nodes and returns their IDs so the shell can call `_lumen_gc_collect` in JS.

use std::time::{Duration, Instant};

use lumen_dom::{Document, NodeId};

/// How often to scan for dead nodes.
const GC_INTERVAL: Duration = Duration::from_secs(30);

/// Throttled idle GC poller.
///
/// Every [`GC_INTERVAL`] the shell calls [`GcTick::poll`], which reads
/// [`Document::dead_node_ids`] and returns them so the shell can purge
/// JS-side caches via `_lumen_gc_collect`.
pub struct GcTick {
    /// Instant of the last successful GC scan (or creation time).
    last_tick: Instant,
}

impl GcTick {
    /// Create a new `GcTick`. The first poll fires after [`GC_INTERVAL`] elapses.
    pub fn new() -> Self {
        Self {
            last_tick: Instant::now(),
        }
    }

    /// Poll the GC scheduler.
    ///
    /// Returns dead [`NodeId`]s when [`GC_INTERVAL`] has elapsed and the
    /// document contains at least one collectable node (detached + zero JS refs).
    /// Returns `None` when the interval has not elapsed or there are no dead nodes.
    ///
    /// Resets the internal timer on every call that passes the throttle check,
    /// regardless of whether dead nodes were found, so the next poll is
    /// rescheduled from this moment.
    pub fn poll(&mut self, doc: &Document) -> Option<Vec<NodeId>> {
        if self.last_tick.elapsed() < GC_INTERVAL {
            return None;
        }
        self.last_tick = Instant::now();
        let dead = doc.dead_node_ids();
        if dead.is_empty() {
            None
        } else {
            Some(dead)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_dom::{Document, QualName};
    use std::time::Duration;

    fn expired_tick() -> GcTick {
        GcTick {
            last_tick: Instant::now() - GC_INTERVAL - Duration::from_secs(1),
        }
    }

    #[test]
    fn throttled_on_fresh_tick() {
        let mut tick = GcTick::new();
        let doc = Document::new();
        assert!(tick.poll(&doc).is_none(), "should be throttled immediately after creation");
    }

    #[test]
    fn returns_none_when_no_dead_nodes() {
        let mut tick = expired_tick();
        let doc = Document::new();
        // Root is never dead.
        assert!(tick.poll(&doc).is_none());
    }

    #[test]
    fn returns_dead_node_after_interval() {
        let mut tick = expired_tick();
        let mut doc = Document::new();
        // Unattached node → immediately dead (detached, zero JS refs).
        let div = doc.create_element(QualName::html("div"));
        let dead = tick.poll(&doc);
        assert!(dead.is_some());
        assert!(dead.unwrap().contains(&div));
    }

    #[test]
    fn attached_node_not_returned() {
        let mut tick = expired_tick();
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        assert!(tick.poll(&doc).is_none(), "attached node must not be reported dead");
    }

    #[test]
    fn timer_resets_after_poll() {
        let mut tick = expired_tick();
        let doc = Document::new();
        // First expired poll — resets timer.
        let _ = tick.poll(&doc);
        // Immediate second poll — throttled.
        assert!(tick.poll(&doc).is_none(), "should be throttled after just firing");
    }

    #[test]
    fn node_with_js_ref_not_dead() {
        let mut tick = expired_tick();
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.acquire_js_ref(div);
        // Still has a JS ref → not dead even though unattached.
        assert!(tick.poll(&doc).is_none());
    }
}
