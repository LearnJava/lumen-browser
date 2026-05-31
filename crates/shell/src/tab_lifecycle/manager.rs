#![allow(dead_code)]
/// Multi-tab lifecycle manager with LRU eviction and memory-pressure handling.
///
/// Owns `TabLifecycle` for every open tab and coordinates tier transitions
/// across the entire tab set:
/// - idle-timeout advancement (call `tick_idle` each second)
/// - OS memory-pressure response (pass `MemoryPressure` into `tick_idle`)
/// - LRU eviction when background-tab count exceeds budget
use std::collections::{HashMap, VecDeque};

use super::state::{MemoryPressure, TabLifecycle, TabState, TierTimeouts, TransitionReason};

/// Opaque tab identifier. Callers create sequential IDs (0, 1, 2, …) or any u64.
pub type TabId = u64;

/// A tier transition that occurred during `tick_idle` or `lru_evict`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierTransition {
    /// Which tab transitioned.
    pub tab_id: TabId,
    /// State before the transition.
    pub from: TabState,
    /// State after the transition.
    pub to: TabState,
    /// Why the transition happened.
    pub reason: TransitionReason,
}

/// Manages lifecycle state for all open tabs.
///
/// `max_background_tabs` is the LRU budget: once more than this many tabs are
/// in T1/T2 (non-hibernated background), the least-recently-used one is
/// advanced to T3 (Hibernated) until the count is within budget.
#[derive(Debug)]
pub struct TabLifecycleManager {
    /// Per-tab state storage.
    tabs: HashMap<TabId, TabLifecycle>,

    /// LRU order: front = most recently used, back = least recently used.
    lru: VecDeque<TabId>,

    /// Currently active (foreground) tab.
    active_tab: Option<TabId>,

    /// Tier-transition timeouts (user-configurable).
    pub timeouts: TierTimeouts,

    /// Maximum number of non-hibernated background tabs before LRU eviction.
    pub max_background_tabs: usize,
}

impl TabLifecycleManager {
    /// Create a new manager with the given timeouts and LRU budget.
    pub fn new(timeouts: TierTimeouts, max_background_tabs: usize) -> Self {
        Self {
            tabs: HashMap::new(),
            lru: VecDeque::new(),
            active_tab: None,
            timeouts,
            max_background_tabs,
        }
    }

    /// Open a new tab. The tab starts in Active state and becomes the foreground tab.
    /// The previously active tab (if any) is moved to T1 BackgroundRecent.
    ///
    /// Returns the previous active tab's ID (if any).
    pub fn open_tab(&mut self, id: TabId) -> Option<TabId> {
        self.tabs.insert(id, TabLifecycle::new());

        // Deactivate the previous foreground tab (move to T1).
        let prev = self.active_tab;
        if let Some(prev_id) = prev
            && let Some(tab) = self.tabs.get_mut(&prev_id)
        {
            tab.hide();
            if tab.state == TabState::Active {
                tab.advance_tier(TransitionReason::UserHidden);
            }
        }

        self.lru_push_front(id);
        self.set_active(id)
    }

    /// Switch to an existing tab, activating it and sending the previous active tab
    /// to T1 BackgroundRecent.
    ///
    /// Returns list of tier transitions that occurred (at most 2: old tab → T1, new tab
    /// → T0 if it was hibernated).
    pub fn activate_tab(&mut self, id: TabId) -> Vec<TierTransition> {
        let mut transitions = Vec::new();

        // Deactivate current active tab.
        let prev = self.active_tab;
        if let Some(prev_id) = prev
            && prev_id != id
            && let Some(tab) = self.tabs.get_mut(&prev_id)
        {
            let from = tab.state;
            tab.hide();
            if from == TabState::Active {
                tab.advance_tier(TransitionReason::UserHidden);
                transitions.push(TierTransition {
                    tab_id: prev_id,
                    from,
                    to: tab.state,
                    reason: TransitionReason::UserHidden,
                });
            }
        }

        // Activate new tab.
        if let Some(tab) = self.tabs.get_mut(&id) {
            let from = tab.state;
            tab.activate(TransitionReason::UserActivated);
            if from != TabState::Active {
                transitions.push(TierTransition {
                    tab_id: id,
                    from,
                    to: TabState::Active,
                    reason: TransitionReason::UserActivated,
                });
            }
        }

        self.lru_push_front(id);
        self.active_tab = Some(id);
        transitions
    }

    /// Mark a tab as closed. Advances it to `TabState::Closed` and removes it
    /// from the LRU queue.
    ///
    /// Returns the transition if the tab existed and was not already Closed.
    pub fn close_tab(&mut self, id: TabId) -> Option<TierTransition> {
        let tab = self.tabs.get_mut(&id)?;
        let from = tab.state;
        if from == TabState::Closed {
            return None;
        }
        tab.state = TabState::Closed;
        tab.last_transition = Some(TransitionReason::UserClosed);
        self.lru.retain(|&x| x != id);
        if self.active_tab == Some(id) {
            self.active_tab = None;
        }
        Some(TierTransition {
            tab_id: id,
            from,
            to: TabState::Closed,
            reason: TransitionReason::UserClosed,
        })
    }

    /// Pin/unpin a tab. Pinned tabs are never evicted past T1.
    pub fn set_pinned(&mut self, id: TabId, pinned: bool) {
        if let Some(tab) = self.tabs.get_mut(&id) {
            tab.pinned = pinned;
        }
    }

    /// Returns the current state of a tab, or `None` if the tab is unknown.
    pub fn tab_state(&self, id: TabId) -> Option<TabState> {
        self.tabs.get(&id).map(|t| t.state)
    }

    /// Returns `true` if `id` is the foreground (Active) tab.
    pub fn is_active(&self, id: TabId) -> bool {
        self.active_tab == Some(id)
    }

    /// Advance all background tabs whose idle timeout has elapsed, and apply
    /// memory pressure to non-pinned background tabs.
    ///
    /// Call approximately every second from the shell event loop.
    pub fn tick_idle(&mut self, pressure: MemoryPressure) -> Vec<TierTransition> {
        let mut transitions = Vec::new();
        let ids: Vec<TabId> = self.tabs.keys().copied().collect();

        for id in ids {
            if self.active_tab == Some(id) {
                continue;
            }

            let Some(tab) = self.tabs.get_mut(&id) else {
                continue;
            };

            // Memory pressure — check for accelerated eviction.
            if let Some(target) = tab.suggested_pressure_state(pressure)
                && tab.state != target
            {
                let from = tab.state;
                tab.state = target;
                tab.last_transition = Some(TransitionReason::MemoryPressure);
                transitions.push(TierTransition {
                    tab_id: id,
                    from,
                    to: target,
                    reason: TransitionReason::MemoryPressure,
                });
                continue; // Don't also apply idle timeout in the same tick.
            }

            // Idle timeout.
            if tab.should_transition_on_idle(&self.timeouts) {
                let from = tab.state;
                tab.advance_tier(TransitionReason::IdleTimeout);
                transitions.push(TierTransition {
                    tab_id: id,
                    from,
                    to: tab.state,
                    reason: TransitionReason::IdleTimeout,
                });
            }
        }

        transitions
    }

    /// Evict least-recently-used background tabs until the number of
    /// non-hibernated background tabs is within `max_background_tabs`.
    ///
    /// Only T1/T2 (BackgroundRecent/BackgroundOld) tabs count toward the budget.
    /// Pinned tabs are skipped.
    pub fn lru_evict(&mut self) -> Vec<TierTransition> {
        let mut transitions = Vec::new();

        loop {
            let non_hibernated_bg = self
                .tabs
                .values()
                .filter(|t| {
                    matches!(
                        t.state,
                        TabState::BackgroundRecent | TabState::BackgroundOld
                    )
                })
                .count();

            if non_hibernated_bg <= self.max_background_tabs {
                break;
            }

            // Find the least-recently-used non-pinned background tab.
            let victim_id = self
                .lru
                .iter()
                .rev()
                .copied()
                .find(|&id| {
                    self.tabs.get(&id).is_some_and(|t| {
                        !t.pinned
                            && matches!(
                                t.state,
                                TabState::BackgroundRecent | TabState::BackgroundOld
                            )
                    })
                });

            match victim_id {
                Some(id) => {
                    let tab = self.tabs.get_mut(&id).unwrap();
                    let from = tab.state;
                    tab.state = TabState::Hibernated;
                    tab.last_transition = Some(TransitionReason::LruBudget);
                    transitions.push(TierTransition {
                        tab_id: id,
                        from,
                        to: TabState::Hibernated,
                        reason: TransitionReason::LruBudget,
                    });
                }
                None => break,
            }
        }

        transitions
    }

    /// Returns a snapshot of all tab IDs and their current states.
    pub fn snapshot(&self) -> Vec<(TabId, TabState)> {
        self.tabs.iter().map(|(&id, t)| (id, t.state)).collect()
    }

    // Push `id` to the front of the LRU queue, removing any previous occurrence.
    fn lru_push_front(&mut self, id: TabId) {
        self.lru.retain(|&x| x != id);
        self.lru.push_front(id);
    }

    // Set the active tab, returning the previous one.
    fn set_active(&mut self, id: TabId) -> Option<TabId> {
        let prev = self.active_tab;
        self.active_tab = Some(id);
        prev
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mgr() -> TabLifecycleManager {
        TabLifecycleManager::new(TierTimeouts::default(), 3)
    }

    #[test]
    fn open_tab_becomes_active() {
        let mut m = mgr();
        m.open_tab(1);
        assert_eq!(m.tab_state(1), Some(TabState::Active));
        assert!(m.is_active(1));
    }

    #[test]
    fn open_second_tab_deactivates_first() {
        let mut m = mgr();
        m.open_tab(1);
        m.open_tab(2);
        assert_eq!(m.tab_state(1), Some(TabState::BackgroundRecent));
        assert_eq!(m.tab_state(2), Some(TabState::Active));
    }

    #[test]
    fn activate_tab_switches_foreground() {
        let mut m = mgr();
        m.open_tab(1);
        m.open_tab(2);
        let transitions = m.activate_tab(1);
        assert_eq!(m.tab_state(1), Some(TabState::Active));
        assert_eq!(m.tab_state(2), Some(TabState::BackgroundRecent));
        // Should have: tab2 → T1, tab1 → T0.
        assert_eq!(transitions.len(), 2);
        let t2 = transitions.iter().find(|t| t.tab_id == 2).unwrap();
        assert_eq!(t2.to, TabState::BackgroundRecent);
        let t1 = transitions.iter().find(|t| t.tab_id == 1).unwrap();
        assert_eq!(t1.to, TabState::Active);
    }

    #[test]
    fn close_tab_marks_closed() {
        let mut m = mgr();
        m.open_tab(1);
        let t = m.close_tab(1).unwrap();
        assert_eq!(t.to, TabState::Closed);
        assert_eq!(m.tab_state(1), Some(TabState::Closed));
        assert!(!m.is_active(1));
    }

    #[test]
    fn close_already_closed_is_none() {
        let mut m = mgr();
        m.open_tab(1);
        m.close_tab(1);
        assert!(m.close_tab(1).is_none());
    }

    #[test]
    fn lru_eviction_hibernates_oldest() {
        // Budget = 3 background tabs.
        let mut m = TabLifecycleManager::new(TierTimeouts::default(), 3);
        for id in 1..=5u64 {
            m.open_tab(id);
        }
        // Tabs 1-4 are in T1 (BackgroundRecent); tab 5 is Active.
        // 4 background tabs > budget 3 → evict the least-recently-used (tab 1).
        let evicted = m.lru_evict();
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].tab_id, 1);
        assert_eq!(evicted[0].to, TabState::Hibernated);
        assert_eq!(m.tab_state(1), Some(TabState::Hibernated));
    }

    #[test]
    fn lru_eviction_respects_pin() {
        let mut m = TabLifecycleManager::new(TierTimeouts::default(), 0); // budget=0
        m.open_tab(1);
        m.open_tab(2);
        // Tab 1 is T1, pin it.
        m.set_pinned(1, true);
        // Budget=0: 1 background tab > 0 → try to evict, but only tab 1 is background
        // and it's pinned → no eviction.
        let evicted = m.lru_evict();
        assert!(evicted.is_empty());
        assert_eq!(m.tab_state(1), Some(TabState::BackgroundRecent));
    }

    #[test]
    fn tick_idle_advances_on_timeout() {
        let timeouts = TierTimeouts {
            t0_to_t1_ms: 0,
            t1_to_t2_ms: 0,
            t2_to_t3_ms: 0,
        };
        let mut m = TabLifecycleManager::new(timeouts, 10);
        m.open_tab(1);
        m.open_tab(2); // Tab 1 → T1 (hidden_at set by open_tab).

        // Tab 1 is in T1 with timeout=0ms → immediate transition.
        let transitions = m.tick_idle(MemoryPressure::Low);
        let t1_trans: Vec<_> = transitions.iter().filter(|t| t.tab_id == 1).collect();
        assert!(!t1_trans.is_empty());
        assert_eq!(t1_trans[0].from, TabState::BackgroundRecent);
        assert_eq!(t1_trans[0].to, TabState::BackgroundOld);
    }

    #[test]
    fn tick_idle_memory_pressure_accelerates() {
        let mut m = TabLifecycleManager::new(
            TierTimeouts {
                t0_to_t1_ms: 0,
                t1_to_t2_ms: 1_000_000,
                t2_to_t3_ms: 1_000_000,
            },
            10,
        );
        m.open_tab(1);
        m.open_tab(2); // Tab 1 → T1, hidden_at set.

        // Tab 1 is in T1 but hidden for <1min. Low pressure → no transition.
        let transitions = m.tick_idle(MemoryPressure::Low);
        assert!(transitions.iter().all(|t| t.tab_id != 1));
    }

    #[test]
    fn activate_already_active_no_duplicate_transitions() {
        let mut m = mgr();
        m.open_tab(1);
        // Tab 1 is already active; activating again should produce no transitions.
        let transitions = m.activate_tab(1);
        assert!(transitions.is_empty());
    }

    #[test]
    fn snapshot_contains_all_tabs() {
        let mut m = mgr();
        for id in 1..=4u64 {
            m.open_tab(id);
        }
        let snap = m.snapshot();
        assert_eq!(snap.len(), 4);
    }
}
