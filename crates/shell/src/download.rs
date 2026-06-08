//! Download manager: background HTTP downloads with progress tracking and
//! a viewport-locked bottom panel UI.
//!
//! # Architecture
//!
//! Each download runs in its own `std::thread`. The thread fetches the full
//! response body via `HttpClient::fetch`, writes the file, and reports
//! completion or failure over an `mpsc` channel.  `DownloadManager::poll()`
//! drains that channel and updates entry status; it must be called from the
//! shell event loop (e.g. `about_to_wait`).
//!
//! # Panel
//!
//! `build_download_bar` returns a viewport-locked `DisplayList` that renders
//! a collapsible panel at the bottom of the window.  The caller appends it to
//! `overlay_buf` before the tab strip (so downloads appear below content but
//! above nothing).
//!
//! Toggle visibility with `Ctrl+Shift+J`.
//!
//! # Wiring status
//!
//! `poll()` and `toggle_visible()` are wired into the shell event loop.
//! `start_download`, `cancel`, and `open_download` are part of the public API
//! but not yet triggered automatically (Content-Disposition / download attr
//! wiring is a follow-up task).
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{DisplayCommand, DisplayList};

// ── IDs and status ────────────────────────────────────────────────────────────

/// Opaque identifier for a single download entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DownloadId(u32);

/// Current state of a download entry.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum DownloadStatus {
    /// Queued but the thread hasn't started yet.
    Pending,
    /// Download thread is active; bytes_done / total may both be 0 while
    /// the initial request is in flight (content-length unknown).
    InProgress,
    /// File written successfully.
    Done {
        /// Total bytes written to disk.
        bytes: u64,
    },
    /// Network or I/O error.
    Failed(String),
    /// Cancelled by the user before completion.
    Cancelled,
}

// ── Entry ─────────────────────────────────────────────────────────────────────

/// A single download: source URL, destination path, and current status.
#[derive(Debug, Clone)]
pub struct DownloadEntry {
    /// Unique ID within this session.
    pub id: DownloadId,
    /// Original request URL (string — might be redirected internally).
    pub url: String,
    /// Absolute path on disk where the file will be (or was) written.
    pub dest: PathBuf,
    /// Display name for the UI (file_name() of `dest`).
    pub filename: String,
    /// Current download state.
    pub status: DownloadStatus,
}

// ── Channel messages ──────────────────────────────────────────────────────────

enum DownloadEvent {
    Done { id: DownloadId, bytes: u64 },
    Failed { id: DownloadId, reason: String },
    Cancelled { id: DownloadId },
}

// ── Manager ───────────────────────────────────────────────────────────────────

/// Manages concurrent background downloads and the visibility of the download
/// panel.
///
/// Call `poll()` each event-loop iteration to update entry statuses from the
/// background threads.
pub struct DownloadManager {
    entries: Vec<DownloadEntry>,
    rx: mpsc::Receiver<DownloadEvent>,
    tx: mpsc::Sender<DownloadEvent>,
    next_id: u32,
    /// Per-download cancellation flags — set by `cancel()`, checked in the
    /// download thread before and after the HTTP fetch.
    cancel_flags: HashMap<DownloadId, Arc<AtomicBool>>,
    /// Whether the download panel is currently visible.
    pub visible: bool,
}

impl Default for DownloadManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadManager {
    /// Create a new, empty download manager.
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            entries: Vec::new(),
            rx,
            tx,
            next_id: 1,
            cancel_flags: HashMap::new(),
            visible: false,
        }
    }

    /// Start a background download of `url` into `dest`.
    ///
    /// A new `HttpClient` is created for each download so that downloads are
    /// independent of the page-level client (separate connection pool, no
    /// mixed-content enforcement for user-initiated downloads).
    ///
    /// Returns the `DownloadId` that was assigned; use it with `cancel`.
    pub fn start_download(&mut self, url: String, dest: PathBuf) -> DownloadId {
        let id = DownloadId(self.next_id);
        self.next_id += 1;

        let filename = dest
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("download")
            .to_string();

        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel_flags.insert(id, Arc::clone(&cancel));

        self.entries.push(DownloadEntry {
            id,
            url: url.clone(),
            dest: dest.clone(),
            filename,
            status: DownloadStatus::InProgress,
        });

        let tx = self.tx.clone();
        std::thread::spawn(move || {
            run_download(id, url, dest, cancel, tx);
        });

        id
    }

    /// Request cancellation of download `id`.
    ///
    /// If the thread is still waiting for the HTTP response, the cancel flag
    /// is set and the thread will skip writing the file once the fetch
    /// completes. The entry status is set to `Cancelled` immediately so the
    /// UI can react without waiting for the thread.
    pub fn cancel(&mut self, id: DownloadId) {
        if let Some(flag) = self.cancel_flags.get(&id) {
            flag.store(true, Ordering::Relaxed);
        }
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id)
            && matches!(e.status, DownloadStatus::InProgress | DownloadStatus::Pending)
        {
            e.status = DownloadStatus::Cancelled;
        }
    }

    /// Open the file in the default OS application.
    ///
    /// On Windows this calls `ShellExecuteW` via the `open` verb.
    /// On other platforms it falls back to `xdg-open`.
    ///
    /// Returns `false` if the entry is not found or the file does not exist.
    pub fn open_download(&self, id: DownloadId) -> bool {
        let Some(entry) = self.entries.iter().find(|e| e.id == id) else {
            return false;
        };
        if !matches!(entry.status, DownloadStatus::Done { .. }) {
            return false;
        }
        open_file_in_os(&entry.dest)
    }

    /// Drain the internal mpsc channel and update entry statuses.
    ///
    /// Must be called regularly from the shell event loop (e.g. `about_to_wait`).
    pub fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                DownloadEvent::Done { id, bytes } => {
                    if let Some(e) = self.entries.iter_mut().find(|e| e.id == id)
                        && !matches!(e.status, DownloadStatus::Cancelled)
                    {
                        // Don't override an explicit cancel the user already saw.
                        e.status = DownloadStatus::Done { bytes };
                    }
                    self.cancel_flags.remove(&id);
                }
                DownloadEvent::Failed { id, reason } => {
                    if let Some(e) = self.entries.iter_mut().find(|e| e.id == id)
                        && !matches!(e.status, DownloadStatus::Cancelled)
                    {
                        e.status = DownloadStatus::Failed(reason);
                    }
                    self.cancel_flags.remove(&id);
                }
                DownloadEvent::Cancelled { id } => {
                    if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
                        e.status = DownloadStatus::Cancelled;
                    }
                    self.cancel_flags.remove(&id);
                }
            }
        }
    }

    /// All entries in insertion order (most recent last).
    pub fn entries(&self) -> &[DownloadEntry] {
        &self.entries
    }

    /// Number of entries whose status is `InProgress` or `Pending`.
    pub fn active_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| {
                matches!(e.status, DownloadStatus::InProgress | DownloadStatus::Pending)
            })
            .count()
    }

    /// Toggle panel visibility.
    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
    }

    /// Show the panel.
    pub fn open(&mut self) {
        self.visible = true;
    }

    /// Hide the panel.
    pub fn close(&mut self) {
        self.visible = false;
    }
}

// ── Background thread ─────────────────────────────────────────────────────────

fn run_download(
    id: DownloadId,
    url: String,
    dest: PathBuf,
    cancel: Arc<AtomicBool>,
    tx: mpsc::Sender<DownloadEvent>,
) {
    if cancel.load(Ordering::Relaxed) {
        let _ = tx.send(DownloadEvent::Cancelled { id });
        return;
    }

    let parsed = match lumen_core::url::Url::parse(&url) {
        Ok(u) => u,
        Err(e) => {
            let _ = tx.send(DownloadEvent::Failed {
                id,
                reason: e.to_string(),
            });
            return;
        }
    };

    use lumen_core::ext::NetworkTransport as _;
    use lumen_network::{BrotliContentDecoder, HttpClient};

    let client = crate::config::global()
        .apply_http(HttpClient::new().with_content_decoder(Arc::new(BrotliContentDecoder::new())));

    let body = match client.fetch(&parsed) {
        Ok(b) => b,
        Err(e) => {
            let _ = tx.send(DownloadEvent::Failed {
                id,
                reason: e.to_string(),
            });
            return;
        }
    };

    if cancel.load(Ordering::Relaxed) {
        let _ = tx.send(DownloadEvent::Cancelled { id });
        return;
    }

    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
    {
        let _ = std::fs::create_dir_all(parent);
    }

    let bytes = body.len() as u64;
    match std::fs::write(&dest, &body) {
        Ok(()) => {
            let _ = tx.send(DownloadEvent::Done { id, bytes });
        }
        Err(e) => {
            let _ = tx.send(DownloadEvent::Failed {
                id,
                reason: e.to_string(),
            });
        }
    }
}

fn open_file_in_os(path: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        // SAFETY: ShellExecuteW is a standard Win32 API; pointer lifetimes are
        // valid for the duration of the call.
        let result = unsafe {
            windows_shell_execute(verb.as_ptr(), wide.as_ptr())
        };
        result > 32
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .is_ok()
    }
}

#[cfg(target_os = "windows")]
unsafe fn windows_shell_execute(verb: *const u16, path: *const u16) -> isize {
    // Use LoadLibrary + GetProcAddress to avoid a link-time dep on shell32.dll.
    // In practice shell32 is always present on Windows, but we want to keep
    // the dependency implicit (it's always loaded by winit anyway).
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: *mut std::ffi::c_void,
            lpOperation: *const u16,
            lpFile: *const u16,
            lpParameters: *const u16,
            lpDirectory: *const u16,
            nShowCmd: i32,
        ) -> isize;
    }
    // SAFETY: ShellExecuteW is a well-known Win32 API; all pointer lifetimes
    // are valid for the duration of this call.
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb,
            path,
            std::ptr::null(),
            std::ptr::null(),
            1, // SW_SHOWNORMAL
        )
    }
}

// ── Panel UI ──────────────────────────────────────────────────────────────────

const PANEL_BG: Color = Color { r: 28, g: 30, b: 34, a: 245 };
const PANEL_HEADER_BG: Color = Color { r: 35, g: 38, b: 44, a: 255 };
const ITEM_BG: Color = Color { r: 40, g: 43, b: 49, a: 255 };
const PANEL_FG: Color = Color { r: 220, g: 221, b: 225, a: 255 };
const PANEL_DIM: Color = Color { r: 140, g: 142, b: 150, a: 255 };
const PROGRESS_BG: Color = Color { r: 55, g: 58, b: 66, a: 255 };
const PROGRESS_FG: Color = Color { r: 66, g: 133, b: 244, a: 255 };
const STATUS_OK: Color = Color { r: 82, g: 196, b: 103, a: 255 };
const STATUS_ERR: Color = Color { r: 237, g: 80, b: 80, a: 255 };
const STATUS_CANCEL: Color = Color { r: 150, g: 152, b: 160, a: 255 };

const PANEL_WIDTH_FRAC: f32 = 0.42; // fraction of window width
const PANEL_MIN_WIDTH: f32 = 360.0;
const PANEL_MAX_WIDTH: f32 = 600.0;
const HEADER_HEIGHT: f32 = 36.0;
const ITEM_HEIGHT: f32 = 60.0;
/// Maximum number of entries shown before the panel clips.
const MAX_VISIBLE_ITEMS: usize = 5;
const BAR_H: f32 = 6.0; // progress bar height
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_SM: f32 = 11.0;
const H_PAD: f32 = 14.0;
const V_PAD: f32 = 10.0;

/// Build the viewport-locked download panel overlay.
///
/// Returns an empty `DisplayList` when the panel is closed (`!manager.visible`)
/// or when there are no entries.
///
/// The panel is anchored to the bottom-right corner and expands upward.
/// `(win_w, win_h)` are physical window dimensions in CSS pixels.
pub fn build_download_bar(manager: &DownloadManager, (win_w, win_h): (u32, u32)) -> DisplayList {
    if !manager.visible {
        return Vec::new();
    }

    let entries = manager.entries();
    let panel_w = (win_w as f32 * PANEL_WIDTH_FRAC)
        .clamp(PANEL_MIN_WIDTH, PANEL_MAX_WIDTH);
    let visible_count = entries.len().min(MAX_VISIBLE_ITEMS);
    let panel_h = HEADER_HEIGHT + (visible_count as f32) * ITEM_HEIGHT;

    let panel_x = win_w as f32 - panel_w - 8.0;
    let panel_y = win_h as f32 - panel_h - 8.0;

    let mut out: DisplayList = Vec::with_capacity(8 + visible_count * 12);

    // Panel background
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(panel_x, panel_y, panel_w, panel_h),
        color: PANEL_BG,
    });

    // Header
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(panel_x, panel_y, panel_w, HEADER_HEIGHT),
        color: PANEL_HEADER_BG,
    });

    let active = manager.active_count();
    let title = if active > 0 {
        format!("Загрузки ({active} активных)")
    } else {
        format!("Загрузки — {} файлов", entries.len())
    };

    out.push(make_text(
        title,
        panel_x + H_PAD,
        panel_y + (HEADER_HEIGHT - FONT_SIZE) / 2.0,
        panel_w - H_PAD * 2.0 - 40.0,
        FONT_SIZE,
        PANEL_FG,
    ));

    // "×" close hint
    out.push(make_text(
        "Ctrl+Shift+J".to_string(),
        panel_x + panel_w - 100.0,
        panel_y + (HEADER_HEIGHT - FONT_SIZE_SM) / 2.0,
        96.0,
        FONT_SIZE_SM,
        PANEL_DIM,
    ));

    // Entries (most recent first)
    let skip = if entries.len() > MAX_VISIBLE_ITEMS {
        entries.len() - MAX_VISIBLE_ITEMS
    } else {
        0
    };
    for (i, entry) in entries.iter().skip(skip).enumerate() {
        let item_y = panel_y + HEADER_HEIGHT + (i as f32) * ITEM_HEIGHT;
        append_entry(&mut out, entry, panel_x, item_y, panel_w);
    }

    out
}

fn append_entry(
    out: &mut DisplayList,
    entry: &DownloadEntry,
    panel_x: f32,
    item_y: f32,
    panel_w: f32,
) {
    // Item background (alternating shade handled by the single color here)
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(panel_x + 1.0, item_y, panel_w - 2.0, ITEM_HEIGHT - 1.0),
        color: ITEM_BG,
    });

    // Filename
    let name_w = panel_w - H_PAD * 2.0 - 80.0;
    out.push(make_text(
        entry.filename.clone(),
        panel_x + H_PAD,
        item_y + V_PAD,
        name_w,
        FONT_SIZE,
        PANEL_FG,
    ));

    // Status label + progress bar
    match &entry.status {
        DownloadStatus::Pending => {
            out.push(make_text(
                "В очереди…".to_string(),
                panel_x + H_PAD,
                item_y + V_PAD + FONT_SIZE + 4.0,
                name_w,
                FONT_SIZE_SM,
                PANEL_DIM,
            ));
        }
        DownloadStatus::InProgress => {
            // Indeterminate progress bar (full width = "in progress")
            let bar_y = item_y + ITEM_HEIGHT - BAR_H - 4.0;
            let bar_w = panel_w - H_PAD * 2.0;
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(panel_x + H_PAD, bar_y, bar_w, BAR_H),
                color: PROGRESS_BG,
            });
            // Animate via a 60%-wide block; real animation requires shell ticking
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(panel_x + H_PAD, bar_y, bar_w * 0.6, BAR_H),
                color: PROGRESS_FG,
            });
            out.push(make_text(
                "Загрузка…".to_string(),
                panel_x + H_PAD,
                item_y + V_PAD + FONT_SIZE + 4.0,
                name_w,
                FONT_SIZE_SM,
                PANEL_DIM,
            ));
        }
        DownloadStatus::Done { bytes } => {
            let bar_y = item_y + ITEM_HEIGHT - BAR_H - 4.0;
            let bar_w = panel_w - H_PAD * 2.0;
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(panel_x + H_PAD, bar_y, bar_w, BAR_H),
                color: STATUS_OK,
            });
            out.push(make_text(
                format!("Готово — {}", human_bytes(*bytes)),
                panel_x + H_PAD,
                item_y + V_PAD + FONT_SIZE + 4.0,
                name_w,
                FONT_SIZE_SM,
                STATUS_OK,
            ));
        }
        DownloadStatus::Failed(reason) => {
            out.push(make_text(
                format!("Ошибка: {reason}"),
                panel_x + H_PAD,
                item_y + V_PAD + FONT_SIZE + 4.0,
                panel_w - H_PAD * 2.0,
                FONT_SIZE_SM,
                STATUS_ERR,
            ));
        }
        DownloadStatus::Cancelled => {
            out.push(make_text(
                "Отменено".to_string(),
                panel_x + H_PAD,
                item_y + V_PAD + FONT_SIZE + 4.0,
                name_w,
                FONT_SIZE_SM,
                STATUS_CANCEL,
            ));
        }
    }

    // URL (truncated, dimmed)
    let url_display = truncate_url(&entry.url, 55);
    out.push(make_text(
        url_display,
        panel_x + H_PAD,
        item_y + ITEM_HEIGHT - FONT_SIZE_SM - 6.0,
        panel_w - H_PAD * 2.0,
        FONT_SIZE_SM,
        PANEL_DIM,
    ));
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_text(
    text: String,
    x: f32,
    y: f32,
    w: f32,
    font_size: f32,
    color: Color,
) -> DisplayCommand {
    DisplayCommand::DrawText {
        rect: Rect::new(x, y, w, font_size * 1.4),
        text,
        font_size,
        color,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    }
}

fn human_bytes(b: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if b >= GB {
        format!("{:.1} ГБ", b as f64 / GB as f64)
    } else if b >= MB {
        format!("{:.1} МБ", b as f64 / MB as f64)
    } else if b >= KB {
        format!("{:.0} КБ", b as f64 / KB as f64)
    } else {
        format!("{b} Б")
    }
}

fn truncate_url(url: &str, max_chars: usize) -> String {
    if url.chars().count() <= max_chars {
        return url.to_string();
    }
    let half = max_chars / 2;
    let start: String = url.chars().take(half).collect();
    let end: String = url.chars().rev().take(half).collect();
    let end: String = end.chars().rev().collect();
    format!("{start}…{end}")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── DownloadManager state ─────────────────────────────────────────────────

    #[test]
    fn new_manager_empty() {
        let dm = DownloadManager::new();
        assert_eq!(dm.entries().len(), 0);
        assert_eq!(dm.active_count(), 0);
        assert!(!dm.visible);
    }

    #[test]
    fn toggle_visible() {
        let mut dm = DownloadManager::new();
        assert!(!dm.visible);
        dm.toggle_visible();
        assert!(dm.visible);
        dm.toggle_visible();
        assert!(!dm.visible);
    }

    #[test]
    fn open_close() {
        let mut dm = DownloadManager::new();
        dm.open();
        assert!(dm.visible);
        dm.close();
        assert!(!dm.visible);
    }

    #[test]
    fn start_download_adds_entry() {
        let mut dm = DownloadManager::new();
        // Use a file:// URL so no real network is needed in unit tests.
        let id = dm.start_download(
            "file:///tmp/test.bin".to_string(),
            PathBuf::from("/tmp/lumen_dl_test.bin"),
        );
        assert_eq!(dm.entries().len(), 1);
        let entry = &dm.entries()[0];
        assert_eq!(entry.id, id);
        assert_eq!(entry.filename, "lumen_dl_test.bin");
        assert!(matches!(entry.status, DownloadStatus::InProgress));
    }

    #[test]
    fn active_count_in_progress() {
        let mut dm = DownloadManager::new();
        dm.start_download(
            "file:///tmp/a.bin".to_string(),
            PathBuf::from("/tmp/a.bin"),
        );
        dm.start_download(
            "file:///tmp/b.bin".to_string(),
            PathBuf::from("/tmp/b.bin"),
        );
        // Both start as InProgress (thread might already be done by now,
        // but before poll() runs the status is InProgress).
        assert_eq!(dm.active_count(), 2);
    }

    #[test]
    fn cancel_sets_cancelled_status() {
        let mut dm = DownloadManager::new();
        let id = dm.start_download(
            "file:///tmp/cancel_test.bin".to_string(),
            PathBuf::from("/tmp/cancel_test.bin"),
        );
        dm.cancel(id);
        let entry = dm.entries().iter().find(|e| e.id == id).unwrap();
        assert!(matches!(entry.status, DownloadStatus::Cancelled));
    }

    #[test]
    fn cancel_nonexistent_id_noop() {
        let mut dm = DownloadManager::new();
        // Should not panic.
        dm.cancel(DownloadId(999));
    }

    #[test]
    fn poll_receives_done_event() {
        // Manually inject a Done event via the internal channel.
        let mut dm = DownloadManager::new();
        let id = dm.start_download(
            "file:///tmp/poll_test.bin".to_string(),
            PathBuf::from("/tmp/poll_test.bin"),
        );
        // Inject a synthetic Done event.
        dm.tx.send(DownloadEvent::Done { id, bytes: 1024 }).unwrap();
        dm.poll();
        let entry = dm.entries().iter().find(|e| e.id == id).unwrap();
        assert!(matches!(entry.status, DownloadStatus::Done { bytes: 1024 }));
    }

    #[test]
    fn poll_receives_failed_event() {
        let mut dm = DownloadManager::new();
        let id = dm.start_download(
            "file:///tmp/fail_test.bin".to_string(),
            PathBuf::from("/tmp/fail_test.bin"),
        );
        dm.tx
            .send(DownloadEvent::Failed {
                id,
                reason: "connection refused".to_string(),
            })
            .unwrap();
        dm.poll();
        let entry = dm.entries().iter().find(|e| e.id == id).unwrap();
        assert!(matches!(entry.status, DownloadStatus::Failed(_)));
    }

    #[test]
    fn poll_cancelled_entry_not_overwritten_by_done() {
        let mut dm = DownloadManager::new();
        let id = dm.start_download(
            "file:///tmp/race.bin".to_string(),
            PathBuf::from("/tmp/race.bin"),
        );
        // User cancels before the thread sends Done.
        dm.cancel(id);
        // Thread still sends Done (race condition).
        dm.tx.send(DownloadEvent::Done { id, bytes: 512 }).unwrap();
        dm.poll();
        // Cancelled must win.
        let entry = dm.entries().iter().find(|e| e.id == id).unwrap();
        assert!(matches!(entry.status, DownloadStatus::Cancelled));
    }

    // ── UI helpers ────────────────────────────────────────────────────────────

    #[test]
    fn human_bytes_formatting() {
        assert_eq!(human_bytes(0), "0 Б");
        assert_eq!(human_bytes(512), "512 Б");
        assert_eq!(human_bytes(1024), "1 КБ");
        // 500 KiB = 500 * 1024 = 512_000 < 1 MiB → "500 КБ"
        assert_eq!(human_bytes(512_000), "500 КБ");
        // 1.5 MiB = 1_572_864 bytes
        assert_eq!(human_bytes(1_572_864), "1.5 МБ");
        // 1 GiB
        assert_eq!(human_bytes(1_073_741_824), "1.0 ГБ");
    }

    #[test]
    fn truncate_url_short_unchanged() {
        let url = "https://example.com/file.bin";
        assert_eq!(truncate_url(url, 60), url);
    }

    #[test]
    fn truncate_url_long_has_ellipsis() {
        let url = "https://very-long-hostname.example.com/path/to/a/deeply/nested/resource/file.bin";
        let t = truncate_url(url, 30);
        assert!(t.contains('…'));
        assert!(t.chars().count() <= 31);
    }

    // ── Panel rendering ───────────────────────────────────────────────────────

    #[test]
    fn build_bar_hidden_returns_empty() {
        let dm = DownloadManager::new(); // visible = false
        assert!(build_download_bar(&dm, (1280, 800)).is_empty());
    }

    #[test]
    fn build_bar_visible_no_entries_has_header() {
        let mut dm = DownloadManager::new();
        dm.open();
        let dl = build_download_bar(&dm, (1280, 800));
        // Must contain at least the background FillRect and a header DrawText.
        assert!(!dl.is_empty());
        let has_text = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("Загрузки"))
        });
        assert!(has_text);
    }

    #[test]
    fn build_bar_shows_filename() {
        let mut dm = DownloadManager::new();
        dm.open();
        dm.start_download(
            "file:///tmp/report.pdf".to_string(),
            PathBuf::from("/tmp/report.pdf"),
        );
        let dl = build_download_bar(&dm, (1280, 800));
        let has_name = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "report.pdf")
        });
        assert!(has_name);
    }

    #[test]
    fn build_bar_caps_at_max_visible_items() {
        let mut dm = DownloadManager::new();
        dm.open();
        for i in 0..10 {
            dm.start_download(
                format!("file:///tmp/file{i}.bin"),
                PathBuf::from(format!("/tmp/file{i}.bin")),
            );
        }
        let dl = build_download_bar(&dm, (1280, 800));
        // Exactly MAX_VISIBLE_ITEMS filename labels visible (file5..file9).
        // Match only the short filename (no '/'), not the URL which also ends in .bin.
        let name_count = dl
            .iter()
            .filter(|c| {
                matches!(c, DisplayCommand::DrawText { text, .. }
                    if text.starts_with("file") && text.ends_with(".bin") && !text.contains('/'))
            })
            .count();
        assert_eq!(name_count, MAX_VISIBLE_ITEMS);
    }
}
