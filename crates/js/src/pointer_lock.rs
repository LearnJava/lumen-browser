//! Pointer Lock API (W3C Pointer Lock L2 §2-4).
//! Provides low-level mouse tracking with relative movement (movementX/Y).
//! Phase 0: requestPointerLock → Promise, exitPointerLock, pointerLockElement getter,
//! pointerlockchange/pointerlockerror events, movementX/Y on MouseEvent.
//! Phase 1: integrate with shell winit — CursorGrabMode::Locked + DeviceEvent::MouseMotion.

use std::sync::Mutex;

struct PointerLockState {
    /// DOM node ID of the locked element (None if unlock).
    locked_element_nid: Option<u32>,
    /// Relative movement in pixels since last mousemove event.
    movement_x: f64,
    movement_y: f64,
    /// Pending OS cursor grab change consumed by shell in `about_to_wait`.
    /// Some(true) = grab cursor, Some(false) = release cursor, None = no change.
    pending_grab: Option<bool>,
}

// Pointer lock is a browser-wide singleton (only one element may hold the lock),
// and it is coordinated across threads: JS bindings (`requestPointerLock` /
// `exitPointerLock`) run on the dedicated JS thread, while the shell sets
// movement and reads the lock/grab state from the UI thread (`DeviceEvent`,
// `about_to_wait`). A process-global `Mutex` therefore gives correct shared
// access from any thread — a `thread_local` would split this state in two once
// the JS runtime moved off the UI thread (B-1).
static POINTER_LOCK_STATE: Mutex<PointerLockState> = Mutex::new(PointerLockState {
    locked_element_nid: None,
    movement_x: 0.0,
    movement_y: 0.0,
    pending_grab: None,
});

/// Lock the global state, recovering the inner value if a previous holder
/// panicked (the data has no broken invariant to protect).
fn state() -> std::sync::MutexGuard<'static, PointerLockState> {
    POINTER_LOCK_STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Request pointer lock for element with given node ID.
/// Sets pending_grab=true so the shell calls winit set_cursor_grab(Locked).
pub fn request_pointer_lock(element_nid: u32) {
    let mut s = state();
    s.locked_element_nid = Some(element_nid);
    s.pending_grab = Some(true);
}

/// Exit pointer lock.
/// Sets pending_grab=false so the shell calls winit set_cursor_grab(None).
pub fn exit_pointer_lock() {
    let mut s = state();
    s.locked_element_nid = None;
    s.pending_grab = Some(false);
}

/// Set relative mouse movement delta (called from shell DeviceEvent::MouseMotion).
/// Only accumulates when pointer is locked.
pub fn set_movement(dx: f64, dy: f64) {
    let mut s = state();
    if s.locked_element_nid.is_some() {
        s.movement_x = dx;
        s.movement_y = dy;
    }
}

/// Get current pointer lock state: (is_locked, locked_element_nid, movement_x, movement_y).
pub fn get_lock_state() -> (bool, Option<u32>, f64, f64) {
    let s = state();
    (
        s.locked_element_nid.is_some(),
        s.locked_element_nid,
        s.movement_x,
        s.movement_y,
    )
}

/// Check if pointer is locked.
pub fn is_pointer_locked() -> bool {
    state().locked_element_nid.is_some()
}

/// Get the DOM node ID of the locked element, or None.
pub fn get_locked_element_nid() -> Option<u32> {
    state().locked_element_nid
}

/// Get the current movement delta and reset it to zero.
/// Called by shell after each DeviceEvent::MouseMotion when pointer is locked.
pub fn take_movement() -> (f64, f64) {
    let mut s = state();
    let (dx, dy) = (s.movement_x, s.movement_y);
    s.movement_x = 0.0;
    s.movement_y = 0.0;
    (dx, dy)
}

/// Take pending OS cursor grab request, resetting it to None.
/// Returns Some(true) to grab cursor, Some(false) to release, None if no change.
/// Called by shell in `about_to_wait` to apply winit CursorGrabMode changes.
pub fn take_pending_grab() -> Option<bool> {
    state().pending_grab.take()
}

#[cfg(test)]
mod tests {
    use super::*;

    // POINTER_LOCK_STATE is now a process-global singleton, so these tests share
    // it. Serialise them with a guard and reset to a known state at the start of
    // each so parallel `cargo test` execution stays deterministic.
    static TEST_GUARD: Mutex<()> = Mutex::new(());

    fn reset() {
        let mut s = state();
        s.locked_element_nid = None;
        s.movement_x = 0.0;
        s.movement_y = 0.0;
        s.pending_grab = None;
    }

    #[test]
    fn test_initial_state() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let (locked, nid, dx, dy) = get_lock_state();
        assert!(!locked);
        assert_eq!(nid, None);
        assert_eq!(dx, 0.0);
        assert_eq!(dy, 0.0);
    }

    #[test]
    fn test_request_pointer_lock() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        request_pointer_lock(42);
        assert!(is_pointer_locked());
        assert_eq!(get_locked_element_nid(), Some(42));
    }

    #[test]
    fn test_exit_pointer_lock() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        request_pointer_lock(42);
        assert!(is_pointer_locked());
        exit_pointer_lock();
        assert!(!is_pointer_locked());
        assert_eq!(get_locked_element_nid(), None);
    }

    #[test]
    fn test_movement_only_when_locked() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        set_movement(10.0, 20.0);
        let (_, _, dx, dy) = get_lock_state();
        assert_eq!(dx, 0.0);
        assert_eq!(dy, 0.0);

        request_pointer_lock(1);
        set_movement(10.0, 20.0);
        let (_, _, dx, dy) = get_lock_state();
        assert_eq!(dx, 10.0);
        assert_eq!(dy, 20.0);
    }

    #[test]
    fn test_take_movement() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        request_pointer_lock(1);
        set_movement(5.5, -3.2);
        let (dx, dy) = take_movement();
        assert_eq!(dx, 5.5);
        assert_eq!(dy, -3.2);

        // After take, values are reset.
        let (_, _, dx2, dy2) = get_lock_state();
        assert_eq!(dx2, 0.0);
        assert_eq!(dy2, 0.0);
    }

    #[test]
    fn test_pending_grab_on_request() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        assert_eq!(take_pending_grab(), None);

        request_pointer_lock(5);
        assert_eq!(take_pending_grab(), Some(true));
        // Second take returns None.
        assert_eq!(take_pending_grab(), None);
    }

    #[test]
    fn test_pending_grab_on_exit() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        request_pointer_lock(5);
        let _ = take_pending_grab(); // consume the grab request
        exit_pointer_lock();
        assert_eq!(take_pending_grab(), Some(false));
        assert_eq!(take_pending_grab(), None);
    }
}
