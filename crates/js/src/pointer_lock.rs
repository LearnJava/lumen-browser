//! Pointer Lock API (W3C Pointer Lock L2 §2-4).
//! Provides low-level mouse tracking with relative movement (movementX/Y).
//! Phase 0: requestPointerLock → Promise, exitPointerLock, pointerLockElement getter,
//! pointerlockchange/pointerlockerror events, movementX/Y on MouseEvent.
//! Phase 1: integrate with shell winit to capture actual mouse cursor.

use std::cell::RefCell;

struct PointerLockState {
    /// DOM node ID of the locked element (None if unlock).
    locked_element_nid: Option<u32>,
    /// Relative movement in pixels since last mousemove event.
    movement_x: f64,
    movement_y: f64,
}

thread_local! {
    static POINTER_LOCK_STATE: RefCell<PointerLockState> = const {
        RefCell::new(PointerLockState {
            locked_element_nid: None,
            movement_x: 0.0,
            movement_y: 0.0,
        })
    };
}

/// Request pointer lock for element with given node ID.
/// Phase 0: immediately locks in-memory state.
/// Phase 1: will call shell to capture cursor via winit `set_cursor_grab`.
pub fn request_pointer_lock(element_nid: u32) {
    POINTER_LOCK_STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.locked_element_nid = Some(element_nid);
        // Phase 1: call _lumen_ptr_lock_grab(element_nid) here.
    });
}

/// Exit pointer lock.
/// Phase 0: immediately unlocks in-memory state.
/// Phase 1: will call shell to release cursor via winit `set_cursor_grab(None)`.
pub fn exit_pointer_lock() {
    POINTER_LOCK_STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.locked_element_nid = None;
        // Phase 1: call _lumen_ptr_lock_release() here.
    });
}

/// Set relative mouse movement delta (called from shell event loop for each mousemove).
/// Only accumulates when pointer is locked.
pub fn set_movement(dx: f64, dy: f64) {
    POINTER_LOCK_STATE.with(|state| {
        let mut s = state.borrow_mut();
        if s.locked_element_nid.is_some() {
            s.movement_x = dx;
            s.movement_y = dy;
        }
    });
}

/// Get current pointer lock state: (is_locked, locked_element_nid, movement_x, movement_y).
pub fn get_lock_state() -> (bool, Option<u32>, f64, f64) {
    POINTER_LOCK_STATE.with(|state| {
        let s = state.borrow();
        (
            s.locked_element_nid.is_some(),
            s.locked_element_nid,
            s.movement_x,
            s.movement_y,
        )
    })
}

/// Check if pointer is locked.
pub fn is_pointer_locked() -> bool {
    POINTER_LOCK_STATE.with(|state| state.borrow().locked_element_nid.is_some())
}

/// Get the DOM node ID of the locked element, or None.
pub fn get_locked_element_nid() -> Option<u32> {
    POINTER_LOCK_STATE.with(|state| state.borrow().locked_element_nid)
}

/// Get the current movement delta and reset it to zero.
/// Called by shell after each frame to apply to the next MouseEvent.
pub fn take_movement() -> (f64, f64) {
    POINTER_LOCK_STATE.with(|state| {
        let mut s = state.borrow_mut();
        let (dx, dy) = (s.movement_x, s.movement_y);
        s.movement_x = 0.0;
        s.movement_y = 0.0;
        (dx, dy)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let (locked, nid, dx, dy) = get_lock_state();
        assert!(!locked);
        assert_eq!(nid, None);
        assert_eq!(dx, 0.0);
        assert_eq!(dy, 0.0);
    }

    #[test]
    fn test_request_pointer_lock() {
        request_pointer_lock(42);
        assert!(is_pointer_locked());
        assert_eq!(get_locked_element_nid(), Some(42));
    }

    #[test]
    fn test_exit_pointer_lock() {
        request_pointer_lock(42);
        assert!(is_pointer_locked());
        exit_pointer_lock();
        assert!(!is_pointer_locked());
        assert_eq!(get_locked_element_nid(), None);
    }

    #[test]
    fn test_movement_only_when_locked() {
        exit_pointer_lock();
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
}
