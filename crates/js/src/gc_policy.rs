//! JS heap GC tuning per lifecycle tier (10L).
//!
//! Background tabs waste memory by keeping the JS heap at its default growth
//! threshold. `GcLevel` encodes how aggressively the runtime should reclaim
//! memory based on the tab's current lifecycle tier.
//!
//! Predates the V8 migration (rquickjs is gone from the workspace) — the
//! per-tier trigger the doc comments below describe is real
//! (`crates/shell/src/lumen/hibernation.rs` calls the trait method on every
//! `BackgroundRecent`/`BackgroundOld` transition), but the two
//! `GC_THRESHOLD_*` constants were a QuickJS-specific tuning knob
//! (`set_gc_threshold`) with no V8 equivalent — `V8JsRuntime::run_gc_pass`
//! (GAP-P3GCJSDOM срез 3) uses `Isolate::low_memory_notification()` instead,
//! which has no threshold parameter, so they stay unused pending a real V8
//! consumer or removal.

/// GC aggressiveness level for [`crate::v8_runtime::V8JsRuntime::run_gc_pass`]
/// (called via the raw `u8` the shared `PersistentJs::run_gc_pass` trait
/// method takes — see `crates/shell/src/persistent_js.rs`).
///
/// Matches the five-tier memory model (T0–T4): active tabs get `Soft` (no
/// forced collection), background tabs get `Moderate` or `Aggressive`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcLevel {
    /// T0 (Active): no forced GC.
    ///
    /// Resets the GC threshold to the normal operating value so the heap can
    /// grow freely while the tab is in the foreground. Calling `run_gc`
    /// on an active tab would stall JS execution unnecessarily.
    Soft = 0,

    /// T1 (BackgroundRecent): one full collection cycle.
    ///
    /// Reclaims objects that became unreachable since the tab was hidden.
    /// Threshold is kept at the default so occasional GC runs don't dominate
    /// CPU during a short background period.
    Moderate = 1,

    /// T2 (BackgroundOld): full collection + lowered GC threshold.
    ///
    /// Runs a full GC cycle and then lowers `gc_threshold` so the heap stays
    /// small for the duration of the background stay. CPU cost amortises over
    /// the tab's long idle period.
    Aggressive = 2,
}

/// Default QuickJS GC threshold (bytes) for active/T1 tabs.
///
/// QuickJS defaults to 256 KiB; we bump it to 1 MiB so active tabs trigger
/// collection less often, reducing jank.
pub const GC_THRESHOLD_ACTIVE: usize = 1024 * 1024; // 1 MiB

/// Lowered GC threshold for T2 (BackgroundOld) tabs.
///
/// 64 KiB forces more frequent collection when the tab is idle, keeping the
/// retained heap small without full hibernation cost.
pub const GC_THRESHOLD_IDLE: usize = 64 * 1024; // 64 KiB
