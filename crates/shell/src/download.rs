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
    /// Bytes written to disk so far. Drives the determinate progress bar.
    pub received: u64,
    /// Total expected size in bytes, once known (from the fetched body length).
    /// `None` while the HTTP response is still in flight — the bar then renders
    /// indeterminate.
    pub total: Option<u64>,
}

impl DownloadEntry {
    /// Fraction written so far in `0.0..=1.0`, or `None` when the total size is
    /// not yet known (indeterminate progress).
    pub fn progress_fraction(&self) -> Option<f32> {
        match self.total {
            Some(t) if t > 0 => Some((self.received as f32 / t as f32).clamp(0.0, 1.0)),
            Some(_) => Some(1.0), // zero-byte file is "complete"
            None => None,
        }
    }
}

// ── Click actions ───────────────────────────────────────────────────────────────

/// The result of hit-testing a click against the download panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadAction {
    /// Open the completed file in its default OS application.
    Open(DownloadId),
    /// Reveal the completed file in the OS file manager.
    Reveal(DownloadId),
    /// Cancel an in-flight download.
    Cancel(DownloadId),
    /// Close the panel (header × button).
    Close,
    /// Click landed on the panel but not on an actionable control — swallow it
    /// (do not fall through to the page).
    Inside,
    /// Click landed outside the panel — the caller should close the panel.
    Outside,
}

// ── Channel messages ──────────────────────────────────────────────────────────

enum DownloadEvent {
    /// Incremental progress: `received` bytes of `total` written to disk.
    Progress {
        id: DownloadId,
        received: u64,
        total: u64,
    },
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
            received: 0,
            total: None,
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

    /// Reveal the downloaded file in the OS file manager (Explorer / Finder /
    /// the default file manager), selecting it where supported.
    ///
    /// Returns `false` if the entry is unknown or the file is not on disk yet.
    pub fn show_in_folder(&self, id: DownloadId) -> bool {
        let Some(entry) = self.entries.iter().find(|e| e.id == id) else {
            return false;
        };
        if !matches!(entry.status, DownloadStatus::Done { .. }) {
            return false;
        }
        reveal_in_file_manager(&entry.dest)
    }

    /// Start a download of `url`, choosing a destination automatically.
    ///
    /// The file is saved into the OS Downloads directory under `suggested`
    /// (sanitised) when provided, otherwise a name derived from the URL path.
    /// Collisions are resolved by appending ` (1)`, ` (2)`, … to the stem.
    ///
    /// This is the entry point the shell uses when draining
    /// `_lumen_network_download` requests; the panel is shown automatically so
    /// the user sees the new download.
    pub fn start_url_download(&mut self, url: String, suggested: Option<String>) -> DownloadId {
        let base = suggested
            .as_deref()
            .map(sanitize_filename)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| derive_filename_from_url(&url));
        let dest = unique_dest(&default_download_dir(), &base);
        self.visible = true;
        self.start_download(url, dest)
    }

    /// Drain the internal mpsc channel and update entry statuses.
    ///
    /// Must be called regularly from the shell event loop (e.g. `about_to_wait`).
    pub fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                DownloadEvent::Progress { id, received, total } => {
                    if let Some(e) = self.entries.iter_mut().find(|e| e.id == id)
                        && matches!(e.status, DownloadStatus::InProgress)
                    {
                        e.received = received;
                        e.total = Some(total);
                    }
                }
                DownloadEvent::Done { id, bytes } => {
                    if let Some(e) = self.entries.iter_mut().find(|e| e.id == id)
                        && !matches!(e.status, DownloadStatus::Cancelled)
                    {
                        // Don't override an explicit cancel the user already saw.
                        e.status = DownloadStatus::Done { bytes };
                        e.received = bytes;
                        e.total = Some(bytes);
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

    let total = body.len() as u64;

    // The HTTP client returns the full body atomically (no streaming network API
    // yet), so network-phase progress is not observable. We surface determinate
    // progress over the *disk-write* phase by writing in chunks and reporting
    // after each — meaningful for large files on slow disks, and it gives the
    // panel a real fill ratio instead of an indeterminate bar.
    const CHUNK: usize = 256 * 1024;
    use std::io::Write as _;
    let file = match std::fs::File::create(&dest) {
        Ok(f) => f,
        Err(e) => {
            let _ = tx.send(DownloadEvent::Failed {
                id,
                reason: e.to_string(),
            });
            return;
        }
    };
    let mut writer = std::io::BufWriter::new(file);
    let mut written: u64 = 0;
    for chunk in body.chunks(CHUNK.max(1)) {
        if cancel.load(Ordering::Relaxed) {
            drop(writer);
            let _ = std::fs::remove_file(&dest);
            let _ = tx.send(DownloadEvent::Cancelled { id });
            return;
        }
        if let Err(e) = writer.write_all(chunk) {
            let _ = tx.send(DownloadEvent::Failed {
                id,
                reason: e.to_string(),
            });
            return;
        }
        written += chunk.len() as u64;
        let _ = tx.send(DownloadEvent::Progress {
            id,
            received: written,
            total,
        });
    }
    match writer.flush() {
        Ok(()) => {
            let _ = tx.send(DownloadEvent::Done { id, bytes: total });
        }
        Err(e) => {
            let _ = tx.send(DownloadEvent::Failed {
                id,
                reason: e.to_string(),
            });
        }
    }
}

/// Resolve the OS Downloads directory.
///
/// Windows: `%USERPROFILE%\Downloads`. Unix: `$HOME/Downloads`. Falls back to
/// the system temp dir when neither environment variable is set.
fn default_download_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let home = std::env::var_os("USERPROFILE");
    #[cfg(not(target_os = "windows"))]
    let home = std::env::var_os("HOME");

    match home {
        Some(h) => PathBuf::from(h).join("Downloads"),
        None => std::env::temp_dir(),
    }
}

/// Strip path separators and reserved characters from a suggested file name so
/// it cannot escape the Downloads directory or break the filesystem.
///
/// Returns just the final path component with `/ \\ : * ? " < > |` and control
/// characters removed; leading/trailing dots and spaces are trimmed.
fn sanitize_filename(name: &str) -> String {
    let last = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name);
    let cleaned: String = last
        .chars()
        .filter(|c| !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') && !c.is_control())
        .collect();
    cleaned.trim_matches(['.', ' ']).to_string()
}

/// Derive a file name from the URL path, falling back to `"download"`.
///
/// Takes the last non-empty path segment (query and fragment stripped) and
/// sanitises it.
fn derive_filename_from_url(url: &str) -> String {
    let no_frag = url.split('#').next().unwrap_or(url);
    let no_query = no_frag.split('?').next().unwrap_or(no_frag);
    // Strip `scheme://authority` so the host is never mistaken for a file name
    // (a URL with no path component has no derivable name → "download").
    let path = match no_query.find("://") {
        Some(i) => {
            let after = &no_query[i + 3..];
            match after.find('/') {
                Some(j) => &after[j..],
                None => "",
            }
        }
        None => no_query,
    };
    let seg = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let name = sanitize_filename(seg);
    if name.is_empty() {
        "download".to_string()
    } else {
        name
    }
}

/// Build a non-colliding destination path in `dir` for `filename`.
///
/// If `dir/filename` already exists, inserts ` (1)`, ` (2)`, … before the
/// extension until a free path is found (capped at 9999 to avoid an unbounded
/// loop on a pathological directory).
fn unique_dest(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    let ext = path.extension().and_then(|s| s.to_str());
    for n in 1..=9999 {
        let name = match ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(filename)
}

/// Open the OS file manager with `path` selected (or its parent directory).
fn reveal_in_file_manager(path: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        // `explorer /select,<path>` opens the folder and highlights the file.
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .is_ok()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let target = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()
            .is_ok()
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
/// Action-button width / height and the gap between stacked buttons.
const BTN_W: f32 = 70.0;
const BTN_H: f32 = 22.0;
const BTN_GAP: f32 = 6.0;
/// Square header close (×) button side.
const CLOSE_BTN: f32 = 22.0;
const BTN_BG: Color = Color { r: 60, g: 64, b: 72, a: 255 };

/// Geometry of the panel for a given window size: top-left corner and size.
///
/// Mirrors the layout in [`build_download_bar`] so [`hit_test`] stays in sync.
/// Returns `(panel_x, panel_y, panel_w, panel_h, skip)` where `skip` is the
/// number of leading entries scrolled off the top (oldest hidden first).
fn panel_geometry(manager: &DownloadManager, win_w: u32, win_h: u32) -> (f32, f32, f32, f32, usize) {
    let entries = manager.entries();
    let panel_w = (win_w as f32 * PANEL_WIDTH_FRAC).clamp(PANEL_MIN_WIDTH, PANEL_MAX_WIDTH);
    let visible_count = entries.len().min(MAX_VISIBLE_ITEMS);
    let panel_h = HEADER_HEIGHT + (visible_count as f32) * ITEM_HEIGHT;
    let panel_x = win_w as f32 - panel_w - 8.0;
    let panel_y = win_h as f32 - panel_h - 8.0;
    let skip = entries.len().saturating_sub(MAX_VISIBLE_ITEMS);
    (panel_x, panel_y, panel_w, panel_h, skip)
}

/// Rect of the header close (×) button.
fn close_button_rect(panel_x: f32, panel_y: f32, panel_w: f32) -> Rect {
    Rect::new(
        panel_x + panel_w - CLOSE_BTN - 8.0,
        panel_y + (HEADER_HEIGHT - CLOSE_BTN) / 2.0,
        CLOSE_BTN,
        CLOSE_BTN,
    )
}

/// Action buttons for one entry, right-aligned and vertically arranged.
///
/// Completed downloads get Open + Reveal; in-flight ones get Cancel; finished
/// (failed/cancelled) entries get no buttons.
fn entry_buttons(entry: &DownloadEntry, panel_x: f32, item_y: f32, panel_w: f32) -> Vec<(DownloadAction, Rect, &'static str)> {
    let bx = panel_x + panel_w - H_PAD - BTN_W;
    match &entry.status {
        DownloadStatus::Done { .. } => {
            let y0 = item_y + (ITEM_HEIGHT - (BTN_H * 2.0 + BTN_GAP)) / 2.0;
            vec![
                (DownloadAction::Open(entry.id), Rect::new(bx, y0, BTN_W, BTN_H), "Открыть"),
                (
                    DownloadAction::Reveal(entry.id),
                    Rect::new(bx, y0 + BTN_H + BTN_GAP, BTN_W, BTN_H),
                    "Папка",
                ),
            ]
        }
        DownloadStatus::InProgress | DownloadStatus::Pending => {
            let y0 = item_y + (ITEM_HEIGHT - BTN_H) / 2.0;
            vec![(DownloadAction::Cancel(entry.id), Rect::new(bx, y0, BTN_W, BTN_H), "Отмена")]
        }
        _ => Vec::new(),
    }
}

fn rect_contains(r: &Rect, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// Hit-test a click at `(x, y)` (CSS px) against the download panel.
///
/// Returns `None` when the panel is hidden. Otherwise returns the action the
/// click maps to: a button, the close (×), `Inside` (swallow), or `Outside`
/// (caller should close the panel).
pub fn hit_test(manager: &DownloadManager, x: f32, y: f32, (win_w, win_h): (u32, u32)) -> Option<DownloadAction> {
    if !manager.visible {
        return None;
    }
    let (panel_x, panel_y, panel_w, panel_h, skip) = panel_geometry(manager, win_w, win_h);
    let panel_rect = Rect::new(panel_x, panel_y, panel_w, panel_h);
    if !rect_contains(&panel_rect, x, y) {
        return Some(DownloadAction::Outside);
    }
    if rect_contains(&close_button_rect(panel_x, panel_y, panel_w), x, y) {
        return Some(DownloadAction::Close);
    }
    for (i, entry) in manager.entries().iter().skip(skip).enumerate() {
        let item_y = panel_y + HEADER_HEIGHT + (i as f32) * ITEM_HEIGHT;
        for (action, rect, _) in entry_buttons(entry, panel_x, item_y, panel_w) {
            if rect_contains(&rect, x, y) {
                return Some(action);
            }
        }
    }
    Some(DownloadAction::Inside)
}

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
    let (panel_x, panel_y, panel_w, panel_h, skip) = panel_geometry(manager, win_w, win_h);
    let visible_count = entries.len().min(MAX_VISIBLE_ITEMS);

    let mut out: DisplayList = Vec::with_capacity(8 + visible_count * 14);

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
        panel_w - H_PAD * 2.0 - CLOSE_BTN - 12.0,
        FONT_SIZE,
        PANEL_FG,
    ));

    // Header close (×) button.
    let close = close_button_rect(panel_x, panel_y, panel_w);
    out.push(DisplayCommand::FillRect { rect: close, color: BTN_BG });
    out.push(make_text(
        "×".to_string(),
        close.x + 7.0,
        close.y + (CLOSE_BTN - FONT_SIZE) / 2.0,
        CLOSE_BTN,
        FONT_SIZE,
        PANEL_FG,
    ));

    // Entries (most recent first; oldest scrolled off the top).
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

    // Text column leaves room for the right-aligned action buttons.
    let text_w = panel_w - H_PAD * 2.0 - BTN_W - 10.0;
    let bar_w = text_w;

    // Filename
    out.push(make_text(
        entry.filename.clone(),
        panel_x + H_PAD,
        item_y + V_PAD,
        text_w,
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
                text_w,
                FONT_SIZE_SM,
                PANEL_DIM,
            ));
        }
        DownloadStatus::InProgress => {
            let bar_y = item_y + ITEM_HEIGHT - BAR_H - 4.0;
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(panel_x + H_PAD, bar_y, bar_w, BAR_H),
                color: PROGRESS_BG,
            });
            // Determinate fill once the total is known (disk-write phase);
            // before that, an indeterminate 60% block signals "in progress".
            let fill = entry.progress_fraction().unwrap_or(0.6);
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(panel_x + H_PAD, bar_y, bar_w * fill, BAR_H),
                color: PROGRESS_FG,
            });
            let label = match entry.total {
                Some(t) if t > 0 => format!(
                    "{} / {}",
                    human_bytes(entry.received),
                    human_bytes(t)
                ),
                _ => "Загрузка…".to_string(),
            };
            out.push(make_text(
                label,
                panel_x + H_PAD,
                item_y + V_PAD + FONT_SIZE + 4.0,
                text_w,
                FONT_SIZE_SM,
                PANEL_DIM,
            ));
        }
        DownloadStatus::Done { bytes } => {
            let bar_y = item_y + ITEM_HEIGHT - BAR_H - 4.0;
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(panel_x + H_PAD, bar_y, bar_w, BAR_H),
                color: STATUS_OK,
            });
            out.push(make_text(
                format!("Готово — {}", human_bytes(*bytes)),
                panel_x + H_PAD,
                item_y + V_PAD + FONT_SIZE + 4.0,
                text_w,
                FONT_SIZE_SM,
                STATUS_OK,
            ));
        }
        DownloadStatus::Failed(reason) => {
            out.push(make_text(
                format!("Ошибка: {reason}"),
                panel_x + H_PAD,
                item_y + V_PAD + FONT_SIZE + 4.0,
                text_w,
                FONT_SIZE_SM,
                STATUS_ERR,
            ));
        }
        DownloadStatus::Cancelled => {
            out.push(make_text(
                "Отменено".to_string(),
                panel_x + H_PAD,
                item_y + V_PAD + FONT_SIZE + 4.0,
                text_w,
                FONT_SIZE_SM,
                STATUS_CANCEL,
            ));
        }
    }

    // Action buttons (Open / Reveal / Cancel), right-aligned.
    for (_, rect, label) in entry_buttons(entry, panel_x, item_y, panel_w) {
        out.push(DisplayCommand::FillRect { rect, color: BTN_BG });
        out.push(make_text(
            label.to_string(),
            rect.x + 8.0,
            rect.y + (BTN_H - FONT_SIZE_SM) / 2.0,
            BTN_W - 12.0,
            FONT_SIZE_SM,
            PANEL_FG,
        ));
    }
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

    // ── Progress, destination resolution, hit-testing (CC-2) ──────────────────

    #[test]
    fn progress_fraction_known_and_unknown() {
        let mut e = DownloadEntry {
            id: DownloadId(1),
            url: "u".into(),
            dest: PathBuf::from("/tmp/x"),
            filename: "x".into(),
            status: DownloadStatus::InProgress,
            received: 25,
            total: None,
        };
        assert_eq!(e.progress_fraction(), None);
        e.total = Some(100);
        assert_eq!(e.progress_fraction(), Some(0.25));
        e.received = 200; // clamps to 1.0
        assert_eq!(e.progress_fraction(), Some(1.0));
        e.total = Some(0); // zero-byte file is complete
        assert_eq!(e.progress_fraction(), Some(1.0));
    }

    #[test]
    fn poll_progress_updates_received_total() {
        let mut dm = DownloadManager::new();
        let id = dm.start_download(
            "file:///tmp/p.bin".into(),
            PathBuf::from("/tmp/lumen_prog_test.bin"),
        );
        dm.tx
            .send(DownloadEvent::Progress { id, received: 512, total: 2048 })
            .unwrap();
        dm.poll();
        let e = dm.entries().iter().find(|e| e.id == id).unwrap();
        assert_eq!(e.received, 512);
        assert_eq!(e.total, Some(2048));
    }

    #[test]
    fn default_download_dir_nonempty() {
        let d = default_download_dir();
        assert!(!d.as_os_str().is_empty());
    }

    #[test]
    fn sanitize_filename_strips_path_and_reserved() {
        assert_eq!(sanitize_filename("/etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("a\\b\\c.txt"), "c.txt");
        assert_eq!(sanitize_filename("na:me?.bin"), "name.bin");
        assert_eq!(sanitize_filename("  ..hidden  "), "hidden");
    }

    #[test]
    fn derive_filename_from_url_cases() {
        assert_eq!(derive_filename_from_url("https://h/a/b/file.zip"), "file.zip");
        assert_eq!(
            derive_filename_from_url("https://h/file.pdf?x=1#frag"),
            "file.pdf"
        );
        assert_eq!(derive_filename_from_url("https://h/"), "download");
        assert_eq!(derive_filename_from_url("https://h"), "download");
    }

    #[test]
    fn unique_dest_dedups_existing() {
        let dir = std::env::temp_dir().join(format!("lumen_dl_uniq_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // First call: file doesn't exist → name as-is.
        let p1 = unique_dest(&dir, "a.txt");
        assert_eq!(p1, dir.join("a.txt"));
        std::fs::write(&p1, b"x").unwrap();
        // Second call: collision → " (1)".
        let p2 = unique_dest(&dir, "a.txt");
        assert_eq!(p2, dir.join("a (1).txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn start_url_download_resolves_and_shows() {
        let mut dm = DownloadManager::new();
        let id = dm.start_url_download("file:///nope/data.bin".into(), Some("save.bin".into()));
        assert!(dm.visible, "panel must open on programmatic download");
        let e = dm.entries().iter().find(|e| e.id == id).unwrap();
        assert_eq!(e.filename, "save.bin");
        assert!(e.dest.ends_with("Downloads/save.bin") || e.dest.ends_with("save.bin"));
    }

    #[test]
    fn start_url_download_derives_name_when_unsuggested() {
        let mut dm = DownloadManager::new();
        let id = dm.start_url_download("https://h/path/report.pdf".into(), None);
        let e = dm.entries().iter().find(|e| e.id == id).unwrap();
        assert_eq!(e.filename, "report.pdf");
    }

    fn done_entry(dm: &mut DownloadManager, id: DownloadId, bytes: u64) {
        dm.tx.send(DownloadEvent::Done { id, bytes }).unwrap();
        dm.poll();
    }

    #[test]
    fn hit_test_hidden_returns_none() {
        let dm = DownloadManager::new();
        assert_eq!(hit_test(&dm, 100.0, 100.0, (1280, 800)), None);
    }

    #[test]
    fn hit_test_outside_panel() {
        let mut dm = DownloadManager::new();
        dm.open();
        // Top-left corner is far from the bottom-right panel.
        assert_eq!(hit_test(&dm, 5.0, 5.0, (1280, 800)), Some(DownloadAction::Outside));
    }

    #[test]
    fn hit_test_close_button() {
        let mut dm = DownloadManager::new();
        dm.open();
        let (px, py, pw, _, _) = panel_geometry(&dm, 1280, 800);
        let r = close_button_rect(px, py, pw);
        let hit = hit_test(&dm, r.x + 2.0, r.y + 2.0, (1280, 800));
        assert_eq!(hit, Some(DownloadAction::Close));
    }

    #[test]
    fn hit_test_open_and_reveal_buttons_on_done() {
        let mut dm = DownloadManager::new();
        dm.open();
        let id = dm.start_download("file:///tmp/d.bin".into(), PathBuf::from("/tmp/d.bin"));
        done_entry(&mut dm, id, 100);
        let (px, py, pw, _, skip) = panel_geometry(&dm, 1280, 800);
        let item_y = py + HEADER_HEIGHT + 0.0 * ITEM_HEIGHT;
        assert_eq!(skip, 0);
        let buttons = entry_buttons(&dm.entries()[0], px, item_y, pw);
        assert_eq!(buttons.len(), 2);
        let (open_action, open_rect, _) = buttons[0];
        assert_eq!(open_action, DownloadAction::Open(id));
        let hit = hit_test(&dm, open_rect.x + 2.0, open_rect.y + 2.0, (1280, 800));
        assert_eq!(hit, Some(DownloadAction::Open(id)));
        let (reveal_action, reveal_rect, _) = buttons[1];
        assert_eq!(reveal_action, DownloadAction::Reveal(id));
        let hit2 = hit_test(&dm, reveal_rect.x + 2.0, reveal_rect.y + 2.0, (1280, 800));
        assert_eq!(hit2, Some(DownloadAction::Reveal(id)));
    }

    #[test]
    fn hit_test_cancel_button_on_in_progress() {
        let mut dm = DownloadManager::new();
        dm.open();
        let id = dm.start_download("file:///tmp/c.bin".into(), PathBuf::from("/tmp/c.bin"));
        let (px, py, pw, _, _) = panel_geometry(&dm, 1280, 800);
        let item_y = py + HEADER_HEIGHT;
        let buttons = entry_buttons(&dm.entries()[0], px, item_y, pw);
        assert_eq!(buttons.len(), 1);
        let (_, rect, label) = buttons[0];
        assert_eq!(label, "Отмена");
        let hit = hit_test(&dm, rect.x + 2.0, rect.y + 2.0, (1280, 800));
        assert_eq!(hit, Some(DownloadAction::Cancel(id)));
    }

    #[test]
    fn hit_test_inside_swallows() {
        let mut dm = DownloadManager::new();
        dm.open();
        let (px, py, pw, _, _) = panel_geometry(&dm, 1280, 800);
        // Header centre, away from the close button.
        let hit = hit_test(&dm, px + pw / 2.0, py + HEADER_HEIGHT / 2.0, (1280, 800));
        assert_eq!(hit, Some(DownloadAction::Inside));
    }

    #[test]
    fn done_entry_buttons_render_in_bar() {
        let mut dm = DownloadManager::new();
        dm.open();
        let id = dm.start_download("file:///tmp/r.bin".into(), PathBuf::from("/tmp/r.bin"));
        done_entry(&mut dm, id, 100);
        let dl = build_download_bar(&dm, (1280, 800));
        let has_open = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "Открыть")
        });
        let has_folder = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "Папка")
        });
        assert!(has_open && has_folder);
    }
}
