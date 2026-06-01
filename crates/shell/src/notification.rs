//! OS desktop notification delivery.
//!
//! Provides `show_os_notification(title, body)` which dispatches a system
//! notification on the platform where Lumen is running.
//!
//! Platform support:
//! - **Windows 10+**: Windows balloon tip via PowerShell + System.Windows.Forms.
//!   Title and body are passed through environment variables to avoid shell
//!   injection. Runs in a background thread so the event loop never blocks.
//! - **Linux**: `notify-send` (libnotify) spawned in a background thread.
//! - **other**: no-op.

/// Show a desktop notification asynchronously.
///
/// Spawns a background OS thread — the event loop returns immediately.
/// Errors (PowerShell not found, notify-send missing, etc.) are silently
/// swallowed: notifications are best-effort in Phase 0.
pub fn show_os_notification(title: &str, body: &str) {
    let title = title.to_owned();
    let body = body.to_owned();
    std::thread::Builder::new()
        .name("lumen-notif".into())
        .spawn(move || platform_show(&title, &body))
        .ok();
}

#[cfg(target_os = "windows")]
fn platform_show(title: &str, body: &str) {
    // Use System.Windows.Forms.NotifyIcon balloon tip.
    // Title and body are passed via env vars to avoid any quoting issues.
    // The script creates a transient tray icon, shows the balloon for 4 seconds,
    // waits for it to fade, then disposes the icon.
    let _ = std::process::Command::new("powershell")
        .env("LUMEN_NOTIF_TITLE", title)
        .env("LUMEN_NOTIF_BODY", body)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            "Add-Type -AssemblyName System.Windows.Forms; \
             Add-Type -AssemblyName System.Drawing; \
             $n = New-Object System.Windows.Forms.NotifyIcon; \
             $n.Icon = [System.Drawing.SystemIcons]::Information; \
             $n.Visible = $true; \
             $n.ShowBalloonTip(4000, $env:LUMEN_NOTIF_TITLE, $env:LUMEN_NOTIF_BODY, \
               [System.Windows.Forms.ToolTipIcon]::Info); \
             Start-Sleep 5; \
             $n.Dispose()",
        ])
        .spawn();
}

#[cfg(target_os = "linux")]
fn platform_show(title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .arg(title)
        .arg(body)
        .spawn();
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn platform_show(_title: &str, _body: &str) {}
