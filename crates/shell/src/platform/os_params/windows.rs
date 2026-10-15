//! Windows reader and change watcher for [`super::OsParams`].
//!
//! Reading is a handful of `user32` calls. Watching needs a top-level window of
//! our own: the OS *broadcasts* `WM_SETTINGCHANGE` to top-level windows with a
//! sent (not posted) message, so it never goes through the queue that winit's
//! `with_msg_hook` sees, and a message-only window is not a broadcast target.
//! A never-shown popup on a dedicated thread with its own message loop
//! receives it and leaves winit's window procedure untouched.

use std::ffi::c_void;
use std::sync::{Once, OnceLock};

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCaretBlinkTime, GetMessageW,
    RegisterClassW, SystemParametersInfoW, TranslateMessage, MSG, SM_CXDOUBLECLK, SM_CXDRAG,
    SM_CYDOUBLECLK, SM_CYDRAG, SPI_GETCARETTIMEOUT, SPI_GETCARETWIDTH, SPI_GETCLIENTAREAANIMATION,
    SPI_GETMOUSEHOVERTIME, SPI_GETWHEELSCROLLLINES, SYSTEM_METRICS_INDEX,
    SYSTEM_PARAMETERS_INFO_ACTION, WM_SETTINGCHANGE, WNDCLASSW, WS_POPUP,
};

use super::{OsParams, WheelScroll};

/// `INFINITE` / `WHEEL_PAGESCROLL`: the "no limit" / "whole page" sentinel the
/// OS stores in the unsigned settings.
const SENTINEL: u32 = u32::MAX;

/// A system metric at the 96-dpi baseline, which is CSS px by definition, or
/// `fallback` when the call fails.
fn metric_css(index: SYSTEM_METRICS_INDEX, fallback: f32) -> f32 {
    // SAFETY: plain value call into user32; no pointers, any index is accepted
    // (an unknown one returns 0).
    let v = unsafe { GetSystemMetricsForDpi(index, 96) };
    if v > 0 { v as f32 } else { fallback }
}

/// One `u32`-valued `SPI_GET*` setting, `None` when the call fails.
fn spi_u32(action: SYSTEM_PARAMETERS_INFO_ACTION) -> Option<u32> {
    let mut value: u32 = 0;
    // SAFETY: every action passed here writes exactly one `u32` through
    // `pvParam`; `value` is a live, aligned local that outlives the call, and
    // `uiParam` is 0 as these actions require.
    let ok = unsafe { SystemParametersInfoW(action, 0, (&mut value as *mut u32).cast::<c_void>(), 0) };
    (ok != 0).then_some(value)
}

/// Reads every parameter; anything the OS refuses to report keeps its default.
pub(super) fn read() -> OsParams {
    let d = OsParams::DEFAULT;
    // SAFETY: no arguments, no pointers.
    let (double_click_ms, blink) = unsafe { (GetDoubleClickTime(), GetCaretBlinkTime()) };
    OsParams {
        double_click_ms: if double_click_ms > 0 { double_click_ms } else { d.double_click_ms },
        double_click_slop: (
            metric_css(SM_CXDOUBLECLK, d.double_click_slop.0),
            metric_css(SM_CYDOUBLECLK, d.double_click_slop.1),
        ),
        drag_threshold: (
            metric_css(SM_CXDRAG, d.drag_threshold.0),
            metric_css(SM_CYDRAG, d.drag_threshold.1),
        ),
        wheel: match spi_u32(SPI_GETWHEELSCROLLLINES) {
            Some(SENTINEL) => WheelScroll::Page,
            Some(n) => WheelScroll::Lines(n),
            None => d.wheel,
        },
        caret_width_px: spi_u32(SPI_GETCARETWIDTH).map_or(d.caret_width_px, |w| w as f32),
        // 0 means the call failed, `INFINITE` that the caret does not blink.
        caret_blink_ms: match blink {
            0 => d.caret_blink_ms,
            SENTINEL => None,
            ms => Some(ms),
        },
        caret_timeout_ms: match spi_u32(SPI_GETCARETTIMEOUT) {
            Some(0 | SENTINEL) | None => d.caret_timeout_ms,
            Some(ms) => Some(ms),
        },
        hover_ms: spi_u32(SPI_GETMOUSEHOVERTIME).unwrap_or(d.hover_ms),
        // The setting is "client-area animation enabled"; reduced motion is
        // its negation. A failed call keeps animations on.
        reduce_motion: spi_u32(SPI_GETCLIENTAREAANIMATION).is_some_and(|on| on == 0),
    }
}

/// Runs when a re-read changed the registry; set once by [`start_watcher`].
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
static SPAWN: Once = Once::new();

pub(super) fn start_watcher(on_change: Box<dyn Fn() + Send + Sync>) {
    // The first callback wins; later calls (a second window) are no-ops.
    let _ = WAKE.set(on_change);
    SPAWN.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("lumen-os-params".into())
            .spawn(run_message_loop);
    });
}

/// Window procedure of the hidden watcher window.
unsafe extern "system" fn wnd_proc(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize {
    if msg == WM_SETTINGCHANGE {
        let changed = super::reload();
        #[cfg(test)]
        HANDLED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if changed && let Some(wake) = WAKE.get() {
            wake();
        }
    }
    // SAFETY: forwards the exact arguments the OS gave this procedure.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn run_message_loop() {
    let class: Vec<u16> = "LumenOsParamsWatcher\0".encode_utf16().collect();
    // SAFETY: `class` is NUL-terminated and outlives every call below; the
    // WNDCLASSW is fully initialised and only read by `RegisterClassW`;
    // `wnd_proc` matches the required signature; `MSG` is plain old data for
    // which all-zero is valid; the loop runs on the thread that created the
    // window, as the message APIs require.
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: 0,
            hCursor: 0,
            hbrBackground: 0,
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 {
            return;
        }
        let hwnd = CreateWindowExW(
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
            hinst,
            std::ptr::null(),
        );
        if hwnd == 0 {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Number of `WM_SETTINGCHANGE` messages the watcher window handled — lets a
/// test prove the broadcast target really receives the message.
#[cfg(test)]
static HANDLED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, SendMessageW, WM_SETTINGCHANGE};

    /// The hidden window exists after `start_watcher` and its procedure runs on
    /// `WM_SETTINGCHANGE` (a sent message, as the OS broadcasts it).
    #[test]
    fn watcher_window_receives_settingchange() {
        super::super::start_watcher(|| {});
        let class: Vec<u16> = "LumenOsParamsWatcher\0".encode_utf16().collect();
        let deadline = Instant::now() + Duration::from_secs(5);
        let hwnd = loop {
            // SAFETY: `class` is NUL-terminated and outlives the call; a null
            // window name matches any title.
            let h = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
            if h != 0 || Instant::now() > deadline {
                break h;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_ne!(hwnd, 0, "the watcher window was never created");
        let before = super::HANDLED.load(Ordering::SeqCst);
        // SAFETY: `hwnd` was just found; the message carries no pointers.
        unsafe { SendMessageW(hwnd, WM_SETTINGCHANGE, 0, 0) };
        assert_eq!(super::HANDLED.load(Ordering::SeqCst), before + 1);
    }
}
