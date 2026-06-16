//! Platform wake-lock backend for Screen Wake Lock API Phase 1 (PH3-13).
//!
//! Prevents the display from sleeping while at least one JS `WakeLockSentinel`
//! is active (e.g. while a `<video>` or `<audio>` element is playing).
//!
//! # Platform support
//!
//! | OS | Mechanism |
//! |---|---|
//! | Windows | `SetThreadExecutionState(ES_CONTINUOUS \| ES_DISPLAY_REQUIRED)` |
//! | Linux | `/sys/class/backlight` inhibit is not portable; no-op for Phase 1 |
//! | macOS | No-op for Phase 1 (IOPMAssertionCreate scheduled for Phase 2) |
//!
//! The `acquire` → `release` calls are reference-counted at the JS level (the
//! shim acquires once on the first sentinel and releases on the last), so the
//! OS API is called at most once per page.

use std::sync::atomic::{AtomicBool, Ordering};

use lumen_core::ext::WakeLockProvider;

/// Platform-backed wake-lock provider.
///
/// Installed at shell startup via `lumen_js::set_wake_lock_provider`.
pub struct PlatformWakeLock {
    /// Whether an OS-level wake lock is currently held.
    held: AtomicBool,
}

impl PlatformWakeLock {
    /// Create a new provider with no lock held initially.
    pub fn new() -> Self {
        Self { held: AtomicBool::new(false) }
    }
}

impl Default for PlatformWakeLock {
    fn default() -> Self {
        Self::new()
    }
}

impl WakeLockProvider for PlatformWakeLock {
    fn acquire(&self) -> bool {
        if self.held.swap(true, Ordering::SeqCst) {
            return true; // already held
        }
        os_acquire()
    }

    fn release(&self) {
        if self.held.swap(false, Ordering::SeqCst) {
            os_release();
        }
    }
}

// ── OS-specific implementations ───────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn os_acquire() -> bool {
    // SAFETY: SetThreadExecutionState is a pure OS call with no memory unsafety.
    // ES_CONTINUOUS | ES_DISPLAY_REQUIRED: keep display on until the flag is cleared.
    const ES_CONTINUOUS: u32 = 0x8000_0000;
    const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;
    unsafe extern "system" {
        fn SetThreadExecutionState(esflags: u32) -> u32;
    }
    // Returns 0 on failure.
    // SAFETY: simple Win32 call, valid flags.
    unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED) != 0 }
}

#[cfg(target_os = "windows")]
fn os_release() {
    const ES_CONTINUOUS: u32 = 0x8000_0000;
    unsafe extern "system" {
        fn SetThreadExecutionState(esflags: u32) -> u32;
    }
    // ES_CONTINUOUS alone clears the previous power requirement.
    // SAFETY: simple Win32 call.
    unsafe { SetThreadExecutionState(ES_CONTINUOUS); }
}

#[cfg(not(target_os = "windows"))]
fn os_acquire() -> bool {
    // Phase 1: no-op on non-Windows platforms.
    // Linux Phase 2: D-Bus org.freedesktop.PowerManagement.Inhibit
    // macOS Phase 2: IOPMAssertionCreateWithName(kIOPMAssertionTypeNoDisplaySleep)
    true
}

#[cfg(not(target_os = "windows"))]
fn os_release() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_returns_true() {
        let p = PlatformWakeLock::new();
        assert!(p.acquire());
        p.release();
    }

    #[test]
    fn double_acquire_is_idempotent() {
        let p = PlatformWakeLock::new();
        assert!(p.acquire());
        assert!(p.acquire()); // second call: held=true → early return
        p.release();
    }

    #[test]
    fn release_without_acquire_is_no_op() {
        let p = PlatformWakeLock::new();
        p.release(); // must not panic
    }

    #[test]
    fn acquire_release_cycle() {
        let p = PlatformWakeLock::new();
        assert!(!p.held.load(Ordering::SeqCst));
        assert!(p.acquire());
        assert!(p.held.load(Ordering::SeqCst));
        p.release();
        assert!(!p.held.load(Ordering::SeqCst));
    }

    #[test]
    fn held_flag_tracks_state() {
        let p = PlatformWakeLock::new();
        assert!(!p.held.load(Ordering::SeqCst));
        p.acquire();
        assert!(p.held.load(Ordering::SeqCst));
        p.release();
        assert!(!p.held.load(Ordering::SeqCst));
    }
}
