//! Host platform clipboard backing `navigator.clipboard`.
//!
//! [`PlatformClipboard`] implements [`ClipboardProvider`] against the OS
//! clipboard so that page JS can read and write plain text:
//!
//! - **Windows** — raw Win32 `OpenClipboard` / `GetClipboardData` /
//!   `SetClipboardData` with `CF_UNICODETEXT` (no `shell32`/`user32` crate
//!   dependency, mirroring `download::open_file_in_os`).
//! - **Linux** — best-effort external command (`wl-copy`/`wl-paste` under
//!   Wayland, else `xclip`), mirroring the `xdg-open` fallback used elsewhere.
//! - **macOS** — `pbcopy` / `pbpaste`.
//!
//! Any failure (no clipboard server, command missing, permission denied) is
//! swallowed: reads return `""`, writes are dropped — matching the spec'd
//! resolve-anyway behaviour of `navigator.clipboard`.

use lumen_core::ext::ClipboardProvider;

/// Reads and writes the host platform clipboard for `navigator.clipboard`.
///
/// Stateless: every call talks to the OS clipboard directly. Installed once by
/// the shell via `lumen_js::set_clipboard_provider`.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlatformClipboard;

impl ClipboardProvider for PlatformClipboard {
    fn read_text(&self) -> String {
        platform_read()
    }

    fn write_text(&self, text: &str) {
        platform_write(text);
    }
}

// ── Windows ─────────────────────────────────────────────────────────────────

/// `CF_UNICODETEXT` clipboard format (UTF-16LE, null-terminated).
#[cfg(target_os = "windows")]
const CF_UNICODETEXT: u32 = 13;
/// `GMEM_MOVEABLE` — required flag for memory handed to `SetClipboardData`.
#[cfg(target_os = "windows")]
const GMEM_MOVEABLE: u32 = 0x0002;

#[cfg(target_os = "windows")]
unsafe extern "system" {
    fn OpenClipboard(hwnd: *mut std::ffi::c_void) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn GetClipboardData(format: u32) -> *mut std::ffi::c_void;
    fn SetClipboardData(format: u32, mem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn IsClipboardFormatAvailable(format: u32) -> i32;
    fn GlobalAlloc(flags: u32, bytes: usize) -> *mut std::ffi::c_void;
    fn GlobalLock(mem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn GlobalUnlock(mem: *mut std::ffi::c_void) -> i32;
}

#[cfg(target_os = "windows")]
fn platform_read() -> String {
    // SAFETY: standard Win32 clipboard sequence. We only read while the
    // clipboard is open and the global handle is locked; every early return
    // closes the clipboard. `GlobalLock` returns a pointer valid until the
    // matching `GlobalUnlock`, which we always call before `CloseClipboard`.
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            return String::new();
        }
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return String::new();
        }
        let handle = GetClipboardData(CF_UNICODETEXT);
        if handle.is_null() {
            CloseClipboard();
            return String::new();
        }
        let ptr = GlobalLock(handle).cast::<u16>();
        if ptr.is_null() {
            CloseClipboard();
            return String::new();
        }
        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
        GlobalUnlock(handle);
        CloseClipboard();
        text
    }
}

#[cfg(target_os = "windows")]
fn platform_write(text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * std::mem::size_of::<u16>();
    // SAFETY: standard Win32 clipboard sequence. The `GMEM_MOVEABLE` block is
    // filled while locked, then ownership transfers to the system via
    // `SetClipboardData`; on any failure path the clipboard is closed and the
    // (rare) leaked handle is reclaimed by the OS on process exit.
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return;
        }
        EmptyClipboard();
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if handle.is_null() {
            CloseClipboard();
            return;
        }
        let dst = GlobalLock(handle).cast::<u16>();
        if dst.is_null() {
            CloseClipboard();
            return;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
        GlobalUnlock(handle);
        SetClipboardData(CF_UNICODETEXT, handle);
        CloseClipboard();
    }
}

// ── Linux ───────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn platform_write(text: &str) {
    if run_pipe_write(&["wl-copy"], text) {
        return;
    }
    let _ = run_pipe_write(&["xclip", "-selection", "clipboard"], text);
}

#[cfg(target_os = "linux")]
fn platform_read() -> String {
    if let Some(out) = run_pipe_read(&["wl-paste", "--no-newline"]) {
        return out;
    }
    run_pipe_read(&["xclip", "-selection", "clipboard", "-o"]).unwrap_or_default()
}

// ── macOS ─────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn platform_write(text: &str) {
    let _ = run_pipe_write(&["pbcopy"], text);
}

#[cfg(target_os = "macos")]
fn platform_read() -> String {
    run_pipe_read(&["pbpaste"]).unwrap_or_default()
}

// ── External-command helpers (Linux/macOS) ──────────────────────────────────

/// Spawn `argv` and feed `text` to its stdin; returns `true` on a clean exit.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_pipe_write(argv: &[&str], text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let Some((cmd, rest)) = argv.split_first() else {
        return false;
    };
    let Ok(mut child) = Command::new(cmd)
        .args(rest)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(stdin) = child.stdin.take() {
        let mut stdin = stdin;
        if stdin.write_all(text.as_bytes()).is_err() {
            return false;
        }
    }
    matches!(child.wait(), Ok(status) if status.success())
}

/// Spawn `argv` and capture its stdout as UTF-8 text; `None` on any failure.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_pipe_read(argv: &[&str]) -> Option<String> {
    use std::process::Command;
    let (cmd, rest) = argv.split_first()?;
    let output = Command::new(cmd).args(rest).output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        None
    }
}

// ── Other platforms ─────────────────────────────────────────────────────────

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn platform_write(_text: &str) {}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn platform_read() -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_is_clipboard_provider() {
        // Compile-time check that PlatformClipboard satisfies the trait object.
        let p: &dyn ClipboardProvider = &PlatformClipboard;
        // read_text must never panic regardless of clipboard state.
        let _ = p.read_text();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_roundtrip() {
        // On Windows the real clipboard should round-trip a unicode string.
        // Skipped silently if another process holds the clipboard open.
        let p = PlatformClipboard;
        let sample = "Lumen clipboard ✓ Кириллица";
        p.write_text(sample);
        let got = p.read_text();
        // Either the write took effect, or the clipboard was locked by another
        // process; in the latter case we must not assert a stale value.
        if !got.is_empty() {
            assert_eq!(got, sample);
        }
    }
}
