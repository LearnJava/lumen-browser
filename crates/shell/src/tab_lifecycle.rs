/// Tab lifecycle and memory tier management (ADR-008).
///
/// Five-tier model: T0 (Active) → T1 (BackgroundRecent) → T2 (BackgroundOld)
/// → T3 (Hibernated) → T4 (Closed-recoverable).
///
/// Transitions are triggered by OR-of-conditions:
/// - Idle timeout (configurable per user)
/// - OS memory pressure
/// - LRU within budget
/// - User pin
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

/// Transition reason from one tier to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionReason {
    /// User hid the tab (switched to another tab).
    UserHidden,

    /// Idle timeout reached (configurable per tier).
    IdleTimeout,

    /// OS memory pressure event (Low/Medium/High).
    MemoryPressure,

    /// LRU budget exceeded (global RAM limit for renderer).
    LruBudget,

    /// User closed the tab.
    UserClosed,

    /// Restore from history (T4 → T0, new navigation).
    Restore,
}

/// Per-tab lifecycle state with timestamps.
#[derive(Debug, Clone)]
pub struct TabLifecycle {
    /// Current tier.
    pub state: TabState,

    /// When tab entered T0 (used for idle timeout calculation).
    pub activated_at: SystemTime,

    /// When tab last became inactive (for transition triggers).
    pub hidden_at: Option<SystemTime>,

    /// Is this tab pinned? (pinned tabs stay in T0 or T1 max).
    pub pinned: bool,

    /// Last reason for state transition.
    pub last_transition: Option<TransitionReason>,
}

/// User-configurable timeouts for tier transitions.
#[derive(Debug, Clone)]
pub struct TierTimeouts {
    /// T0 → T1 transition (immediate on hide by default, or delay in ms).
    pub t0_to_t1_ms: u64,

    /// T1 → T2 transition (default 5 min = 300_000 ms).
    pub t1_to_t2_ms: u64,

    /// T2 → T3 transition (default 30 min = 1_800_000 ms).
    pub t2_to_t3_ms: u64,
}

impl Default for TierTimeouts {
    fn default() -> Self {
        Self {
            t0_to_t1_ms: 0,                    // immediate on hide
            t1_to_t2_ms: 5 * 60 * 1000,        // 5 min
            t2_to_t3_ms: 30 * 60 * 1000,       // 30 min
        }
    }
}

impl TabLifecycle {
    /// Create a new tab in T0 (Active) state.
    pub fn new() -> Self {
        Self {
            state: TabState::Active,
            activated_at: SystemTime::now(),
            hidden_at: None,
            pinned: false,
            last_transition: None,
        }
    }

    /// Mark tab as visible/active (transition to T0).
    pub fn activate(&mut self, reason: TransitionReason) {
        self.state = TabState::Active;
        self.activated_at = SystemTime::now();
        self.hidden_at = None;
        self.last_transition = Some(reason);
    }

    /// Mark tab as hidden (start idle timeout countdown for T0→T1).
    pub fn hide(&mut self) {
        self.hidden_at = Some(SystemTime::now());
    }

    /// Transition to next tier due to idle timeout or memory pressure.
    /// Returns `true` if transition happened, `false` if already at final tier.
    pub fn advance_tier(&mut self, reason: TransitionReason) -> bool {
        let old_state = self.state;
        match self.state {
            TabState::Active => {
                self.state = TabState::BackgroundRecent;
            }
            TabState::BackgroundRecent => {
                self.state = TabState::BackgroundOld;
            }
            TabState::BackgroundOld => {
                self.state = TabState::Hibernated;
            }
            TabState::Hibernated => {
                self.state = TabState::Closed;
            }
            TabState::Closed => {
                // Cannot advance further
                return false;
            }
        }
        self.last_transition = Some(reason);
        old_state != self.state
    }

    /// Check if idle timeout has been exceeded for current tier.
    /// Returns `true` if transition is due.
    pub fn should_transition_on_idle(&self, timeouts: &TierTimeouts) -> bool {
        if self.hidden_at.is_none() {
            // Tab is still active
            return false;
        }

        let hidden_duration = match self.hidden_at.unwrap().elapsed() {
            Ok(d) => d,
            Err(_) => return false, // System time error, skip
        };

        match self.state {
            TabState::Active => {
                // Transition to T1 after t0_to_t1 delay
                hidden_duration >= Duration::from_millis(timeouts.t0_to_t1_ms)
            }
            TabState::BackgroundRecent => {
                // Transition to T2 after t1_to_t2 delay
                hidden_duration >= Duration::from_millis(timeouts.t1_to_t2_ms)
            }
            TabState::BackgroundOld => {
                // Transition to T3 after t2_to_t3 delay
                hidden_duration >= Duration::from_millis(timeouts.t2_to_t3_ms)
            }
            TabState::Hibernated | TabState::Closed => {
                // No further idle-based transitions
                false
            }
        }
    }

    /// Accelerate transitions due to memory pressure.
    /// On Medium pressure: T1→T2 for tabs hidden >1 min.
    /// On High pressure: T2→T3 for tabs hidden >5 min.
    pub fn accelerate_on_memory_pressure(&self, pressure: MemoryPressure) -> Option<TabState> {
        if self.pinned {
            // Pinned tabs never transition past T1
            return None;
        }

        let hidden_mins = match self.hidden_at {
            Some(t) => t.elapsed().ok()?.as_secs() / 60,
            None => return None,
        };

        match pressure {
            MemoryPressure::Low => None,
            MemoryPressure::Medium => {
                // Accelerate T1→T2 for tabs hidden >1 min
                if hidden_mins > 1 && self.state == TabState::BackgroundRecent {
                    Some(TabState::BackgroundOld)
                } else {
                    None
                }
            }
            MemoryPressure::High => {
                // Accelerate T2→T3 for tabs hidden >5 min
                if hidden_mins > 5 && self.state == TabState::BackgroundOld {
                    Some(TabState::Hibernated)
                } else {
                    None
                }
            }
        }
    }
}

impl Default for TabLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

/// OS memory pressure levels (from `MemoryPressureSource` trait).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    Low,
    Medium,
    High,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tab_is_active() {
        let tab = TabLifecycle::new();
        assert_eq!(tab.state, TabState::Active);
        assert!(!tab.pinned);
        assert_eq!(tab.hidden_at, None);
    }

    #[test]
    fn test_hide_sets_hidden_at() {
        let mut tab = TabLifecycle::new();
        tab.hide();
        assert!(tab.hidden_at.is_some());
    }

    #[test]
    fn test_advance_tier() {
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
    fn test_activate_from_background() {
        let mut tab = TabLifecycle::new();
        tab.advance_tier(TransitionReason::IdleTimeout);
        assert_eq!(tab.state, TabState::BackgroundRecent);

        tab.activate(TransitionReason::Restore);
        assert_eq!(tab.state, TabState::Active);
        assert_eq!(tab.hidden_at, None);
    }

    #[test]
    fn test_pinned_not_affected_by_pressure() {
        let mut tab = TabLifecycle::new();
        tab.pinned = true;
        tab.hide();
        tab.advance_tier(TransitionReason::IdleTimeout); // Move to T1

        let result = tab.accelerate_on_memory_pressure(MemoryPressure::High);
        assert_eq!(result, None); // Pinned, no transition
    }
}
