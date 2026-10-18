//! Answering `WM_GETOBJECT` on the main window (BUG-1627).
//!
//! A screen reader or MSAA inspector *sends* `WM_GETOBJECT` to the window; a sent
//! message never goes through the queue winit's `with_msg_hook` sees, and winit's
//! own window procedure does not answer it. So the shell puts a thin window
//! procedure in front of winit's: `WM_GETOBJECT` with `OBJID_CLIENT` is answered
//! with the MSAA root from [`lumen_a11y::platform::windows::msaa_client_root`],
//! everything else is forwarded untouched.
//!
//! `SetWindowLongPtrW(GWLP_WNDPROC)` rather than `SetWindowSubclass`: the latter
//! is exported by ordinal from the v5 `comctl32` that a manifest-less process
//! loads. The call must run on the thread that owns the window (the winit
//! thread, from `resumed`).

use std::sync::atomic::{AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, GetWindowLongPtrW, SetWindowLongPtrW, GWLP_WNDPROC, WM_GETOBJECT,
};

/// The window the procedure is installed on, `0` before [`install`].
static HOOKED: AtomicIsize = AtomicIsize::new(0);
/// The window procedure that was in place before ours (winit's).
static PREVIOUS: AtomicIsize = AtomicIsize::new(0);

/// Puts the `WM_GETOBJECT` procedure in front of `hwnd`'s own. A second call
/// for the same window is a no-op; the shell has exactly one such window.
pub fn install(hwnd: isize) {
    if hwnd == 0 || HOOKED.load(Ordering::Acquire) == hwnd {
        return;
    }
    // SAFETY: `hwnd` is the shell's live top-level window and this runs on its
    // owning thread. `wnd_proc` has the `WNDPROC` signature; the previous
    // procedure is stored before ours becomes reachable, and is only invoked
    // through `CallWindowProcW`, which accepts exactly this value.
    unsafe {
        let previous = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
        if previous == 0 {
            return;
        }
        PREVIOUS.store(previous, Ordering::Release);
        HOOKED.store(hwnd, Ordering::Release);
        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, wnd_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT as usize as isize);
    }
}

/// The answer for one message, or `None` to let the previous procedure handle it.
fn answer(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    if msg != WM_GETOBJECT {
        return None;
    }
    lumen_a11y::platform::windows::msaa_client_root(hwnd, wparam, lparam)
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(result) = answer(hwnd, msg, wparam, lparam) {
        return result;
    }
    let previous = PREVIOUS.load(Ordering::Acquire);
    // SAFETY: `previous` was read from `GWLP_WNDPROC` of this very window in
    // `install` (the `WNDPROC` value is stored as an integer and transmuted back
    // by `CallWindowProcW`'s own contract); the message arguments are the ones
    // the system gave us.
    unsafe {
        CallWindowProcW(
            std::mem::transmute::<isize, windows_sys::Win32::UI::WindowsAndMessaging::WNDPROC>(
                previous,
            ),
            hwnd,
            msg,
            wparam,
            lparam,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, SendMessageW, WS_POPUP,
    };

    const OBJID_CLIENT: isize = -4;
    const OBJID_WINDOW: isize = 0;

    /// BUG-1627: a sent `WM_GETOBJECT(OBJID_CLIENT)` comes back with an MSAA
    /// object, any other object id and any other message reach the previous
    /// procedure (a `STATIC` window answers 0 to `WM_GETOBJECT`).
    #[test]
    fn sent_wm_get_object_returns_msaa_root() {
        // winit initialises COM on its thread; a bare test thread does not.
        #[link(name = "ole32")]
        unsafe extern "system" {
            fn CoInitializeEx(reserved: *const std::ffi::c_void, flags: u32) -> i32;
        }
        // SAFETY: plain call, apartment-threaded init of this test thread.
        unsafe { CoInitializeEx(std::ptr::null(), 2) };

        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        // SAFETY: `class` is NUL-terminated and outlives the call; a zero-sized
        // never-shown popup of a system class, created, used and destroyed on
        // this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                0,
                0,
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        assert_ne!(hwnd, 0, "CreateWindowExW");

        // SAFETY: `hwnd` is the live window created above.
        let before = unsafe { SendMessageW(hwnd, WM_GETOBJECT, 0, OBJID_CLIENT) };
        assert_eq!(before, 0, "a plain STATIC window has no MSAA answer of its own");

        install(hwnd);
        // SAFETY: as above.
        let (client, other) = unsafe {
            (
                SendMessageW(hwnd, WM_GETOBJECT, 0, OBJID_CLIENT),
                SendMessageW(hwnd, WM_GETOBJECT, 0, OBJID_WINDOW),
            )
        };
        assert!(client > 0, "OBJID_CLIENT must return an LRESULT for the MSAA root, got {client}");
        assert_eq!(other, 0, "other object ids fall through to the previous procedure");

        // SAFETY: the window belongs to this thread.
        unsafe { DestroyWindow(hwnd) };
    }
}
