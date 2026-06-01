#![allow(unused_imports)]
/// Tab lifecycle and memory-tier management (ADR-008 §10A).
///
/// Five-tier model: T0 Active → T1 BackgroundRecent → T2 BackgroundOld
/// → T3 Hibernated → T4 Closed-recoverable.
///
/// Shell integration:
/// 1. Call `TabLifecycleManager::open_tab(id)` when a tab opens.
/// 2. Call `activate_tab(id)` on every tab switch.
/// 3. Poll `tick_idle(pressure)` once per second — collect returned transitions
///    and hibernate / restore resources accordingly.
/// 4. Call `lru_evict()` after `tick_idle` to enforce the background-tab budget.
pub mod manager;
pub mod restore;
pub mod state;

pub use manager::{TabId, TabLifecycleManager, TierTransition};
pub use restore::TabMetadata;
pub use state::{MemoryPressure, TabLifecycle, TabState, TierTimeouts, TransitionReason};
