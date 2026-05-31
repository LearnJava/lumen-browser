#![allow(dead_code)]
/// Per-tab lifecycle state with timestamps.
///
/// Five-tier memory model: T0 (Active) → T1 (BackgroundRecent) → T2 (BackgroundOld)
/// → T3 (Hibernated) → T4 (Closed-recoverable).
use std::time::{Duration, SystemTime};

/// Tab lifecycle state (memory tier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabState {
    /// T0: Foreground, visible. ~100-200 MB per tab.
    /// JS event loop running, all resources retained.
    Active,

    /// T1: Background, recently hidden (<5 min). ~30-60 MB per tab.
    /// JS event loop paused (heap retained), layout retained, images retained.
    BackgroundRecent,

    /// T2: Background, older (5-30 min). ~10-20 MB per tab.
    /// JS heap snapshot on disk, freed in RAM; images dropped, layout retained.
    BackgroundOld,

    /// T3: Hibernated (>30 min or memory pressure). ~50-200 KB per tab.
    /// DOM snapshot on disk; in RAM only TabMetadata (URL, title, scroll, favicon).
    Hibernated,

    /// T4: Closed, recoverable via history. 0 RAM.
    /// Only history entry + FTS index.
    Closed,
}

/// Reason for a lifecycle tier transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionReason {
    /// User hid the tab (switched to another tab).
    UserHidden,

    /// Idle timeout reached for this tier.
    IdleTimeout,

    /// OS memory pressure event triggered early eviction.
    MemoryPressure,

    /// LRU budget exceeded — tab was the least-recently-used background tab.
    LruBudget,

    /// User closed the tab.
    UserClosed,

    /// Tab was restored from history (Closed → Active).
    Restore,

    /// Tab was explicitly activated by user.
    UserActivated,
}

/// Per-tab lifecycle state tracking.
#[derive(Debug, Clone)]
pub struct TabLifecycle {
    /// Current memory tier.
    pub state: TabState,

    /// When tab last entered T0 Active.
    pub activated_at: SystemTime,

    /// When tab last became inactive (for idle timeout calculation).
    pub hidden_at: Option<SystemTime>,

    /// Pinned tabs stay at T0 or T1 at most — never Hibernated by policy.
    pub pinned: bool,

    /// Reason for most recent state transition.
    pub last_transition: Option<TransitionReason>,
}

/// User-configurable timeouts for tier transitions.
#[derive(Debug, Clone)]
pub struct TierTimeouts {
    /// T0 → T1: delay after hiding (ms). Default 0 = immediate.
    pub t0_to_t1_ms: u64,

    /// T1 → T2: after this many ms hidden. Default 5 min.
    pub t1_to_t2_ms: u64,

    /// T2 → T3: after this many ms hidden. Default 30 min.
    pub t2_to_t3_ms: u64,
}

impl Default for TierTimeouts {
    fn default() -> Self {
        Self {
            t0_to_t1_ms: 0,
            t1_to_t2_ms: 5 * 60 * 1000,
            t2_to_t3_ms: 30 * 60 * 1000,
        }
    }
}

/// OS memory pressure levels (mirrors `MemoryPressureLevel` from lumen-core).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemoryPressure {
    Low,
    Medium,
    High,
}

impl TabLifecycle {
    /// New tab starts in T0 Active.
    pub fn new() -> Self {
        Self {
            state: TabState::Active,
            activated_at: SystemTime::now(),
            hidden_at: None,
            pinned: false,
            last_transition: None,
        }
    }

    /// Transition to Active (T0), resetting idle counters.
    pub fn activate(&mut self, reason: TransitionReason) {
        self.state = TabState::Active;
        self.activated_at = SystemTime::now();
        self.hidden_at = None;
        self.last_transition = Some(reason);
    }

    /// Record the moment the tab was hidden, starting the idle countdown.
    /// Idempotent — only records the first call.
    pub fn hide(&mut self) {
        if self.hidden_at.is_none() {
            self.hidden_at = Some(SystemTime::now());
        }
    }

    /// Advance to the next tier. Returns `true` if a transition occurred.
    pub fn advance_tier(&mut self, reason: TransitionReason) -> bool {
        let next = match self.state {
            TabState::Active => TabState::BackgroundRecent,
            TabState::BackgroundRecent => TabState::BackgroundOld,
            TabState::BackgroundOld => TabState::Hibernated,
            TabState::Hibernated => TabState::Closed,
            TabState::Closed => return false,
        };
        self.state = next;
        self.last_transition = Some(reason);
        true
    }

    /// Returns `true` if the idle timeout for the current tier has elapsed.
    pub fn should_transition_on_idle(&self, timeouts: &TierTimeouts) -> bool {
        let elapsed = match self.hidden_at.and_then(|t| t.elapsed().ok()) {
            Some(d) => d,
            None => return false,
        };
        match self.state {
            TabState::Active => elapsed >= Duration::from_millis(timeouts.t0_to_t1_ms),
            TabState::BackgroundRecent => elapsed >= Duration::from_millis(timeouts.t1_to_t2_ms),
            TabState::BackgroundOld => elapsed >= Duration::from_millis(timeouts.t2_to_t3_ms),
            TabState::Hibernated | TabState::Closed => false,
        }
    }

    /// If memory pressure justifies an earlier-than-scheduled tier advance, returns
    /// the suggested next state; `None` otherwise.
    ///
    /// Pinned tabs are never evicted past T1.
    pub fn suggested_pressure_state(&self, pressure: MemoryPressure) -> Option<TabState> {
        let hidden_secs = self.hidden_at?.elapsed().ok()?.as_secs();

        if self.pinned {
            return None;
        }

        match pressure {
            MemoryPressure::Low => None,
            MemoryPressure::Medium => {
                // T1 → T2 for tabs hidden >1 min.
                if self.state == TabState::BackgroundRecent && hidden_secs > 60 {
                    Some(TabState::BackgroundOld)
                } else {
                    None
                }
            }
            MemoryPressure::High => match self.state {
                // T2 → T3 for tabs hidden >5 min.
                TabState::BackgroundOld if hidden_secs > 5 * 60 => Some(TabState::Hibernated),
                // T1 → T3 for very old T1 tabs (>10 min) under high pressure.
                TabState::BackgroundRecent if hidden_secs > 10 * 60 => Some(TabState::Hibernated),
                _ => None,
            },
        }
    }
}

impl Default for TabLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tab_is_active() {
        let tab = TabLifecycle::new();
        assert_eq!(tab.state, TabState::Active);
        assert!(!tab.pinned);
        assert!(tab.hidden_at.is_none());
    }

    #[test]
    fn hide_records_timestamp() {
        let mut tab = TabLifecycle::new();
        tab.hide();
        assert!(tab.hidden_at.is_some());
    }

    #[test]
    fn hide_idempotent() {
        let mut tab = TabLifecycle::new();
        tab.hide();
        let t1 = tab.hidden_at.unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        tab.hide();
        // Second hide must not overwrite the first timestamp.
        assert_eq!(tab.hidden_at.unwrap(), t1);
    }

    #[test]
    fn advance_tier_full_chain() {
        let mut tab = TabLifecycle::new();
        assert!(tab.advance_tier(TransitionReason::IdleTimeout));
        assert_eq!(tab.state, TabState::BackgroundRecent);
        assert!(tab.advance_tier(TransitionReason::IdleTimeout));
        assert_eq!(tab.state, TabState::BackgroundOld);
        assert!(tab.advance_tier(TransitionReason::IdleTimeout));
        assert_eq!(tab.state, TabState::Hibernated);
        assert!(tab.advance_tier(TransitionReason::IdleTimeout));
        assert_eq!(tab.state, TabState::Closed);
        assert!(!tab.advance_tier(TransitionReason::IdleTimeout));
    }

    #[test]
    fn activate_resets_idle() {
        let mut tab = TabLifecycle::new();
        tab.advance_tier(TransitionReason::IdleTimeout);
        tab.hide();
        tab.activate(TransitionReason::UserActivated);
        assert_eq!(tab.state, TabState::Active);
        assert!(tab.hidden_at.is_none());
    }

    #[test]
    fn idle_timeout_triggers_correctly() {
        let mut tab = TabLifecycle::new();
        tab.hide();
        let timeouts = TierTimeouts {
            t0_to_t1_ms: 0, // immediate
            t1_to_t2_ms: 1_000_000,
            t2_to_t3_ms: 1_000_000,
        };
        assert!(tab.should_transition_on_idle(&timeouts));
    }

    #[test]
    fn idle_timeout_not_yet_due() {
        let mut tab = TabLifecycle::new();
        tab.hide();
        let timeouts = TierTimeouts {
            t0_to_t1_ms: 1_000_000, // very long
            t1_to_t2_ms: 1_000_000,
            t2_to_t3_ms: 1_000_000,
        };
        assert!(!tab.should_transition_on_idle(&timeouts));
    }

    #[test]
    fn active_tab_no_pressure_eviction() {
        let tab = TabLifecycle::new(); // hidden_at = None
        assert_eq!(tab.suggested_pressure_state(MemoryPressure::High), None);
    }

    #[test]
    fn pinned_tab_not_evicted_by_pressure() {
        let mut tab = TabLifecycle::new();
        tab.pinned = true;
        tab.hide();
        tab.advance_tier(TransitionReason::UserHidden); // T1
        assert_eq!(tab.suggested_pressure_state(MemoryPressure::High), None);
    }
}
