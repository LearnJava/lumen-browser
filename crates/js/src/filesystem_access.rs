//! File System Access API (W3C File System Access §5).
//!
//! **Phase 1:** full token-based security, proper JS class hierarchy, write support.
//!
//! # Security model
//!
//! File paths are never exposed to JS. Each file opened via `showOpenFilePicker()`
//! is registered in the file token registry via `crate::file_input::register_file_token`
//! and JS only receives an opaque token. `FileSystemFileHandle.getFile()`
//! constructs a `File` object whose `.text()` / `.arrayBuffer()` / `.stream()`
//! methods use the same `__lumen_file_read_text` / `__lumen_file_read_base64`
//! bindings already installed by `file_input::install_file_input_bindings`.
//!
//! Write paths from `showSaveFilePicker()` are stored in a separate write-handle
//! registry and are only used by `FileSystemWritableFileStream.close()`.
//!
//! Both registries here follow the same three rules `file_input` adopted for
//! [BUG-371] — see that module's header for the full story:
//!
//! * ids are 128-bit unguessable strings, not `1, 2, 3, …` counters (directory
//!   ids used to be enumerable, and `_lumen_dir_get_file(1, name)` *minted a new
//!   read token*, so guessing one id escalated to reading a whole subtree; a
//!   guessed write id let any page overwrite what another page was saving);
//! * every entry records the origin it was issued to, checked on each call
//!   against the origin captured in Rust at install time;
//! * loading a document revokes the grants of the previous document on that
//!   same origin.
//!
//! # JS classes
//!
//! The WebIDL hierarchy of FS §4-§7, none of it constructible from page script
//! (BUG-374) — the only way to a handle is an API that hands one out.
//!
//! | Class | Description |
//! |---|---|
//! | `FileSystemHandle` | base interface: readonly `kind`/`name`, `isSameEntry()`, `queryPermission()`, `requestPermission()`, `remove()`, `getUniqueId()` |
//! | `FileSystemFileHandle` | `.getFile()`, `.createWritable()`, `.createSyncAccessHandle()`, `.move()` |
//! | `FileSystemDirectoryHandle` | async iterable; `.entries()`, `.values()`, `.keys()`, `.getFileHandle()`, `.getDirectoryHandle()`, `.removeEntry()`, `.resolve()` |
//! | `FileSystemWritableFileStream` | `extends WritableStream`; `.write(data\|WriteParams)`, `.seek(pos)`, `.truncate(size)`, `.close()` |
//! | `FileSystemSyncAccessHandle` | unbuffered OPFS access: `.read()`, `.write()`, `.truncate()`, `.getSize()`, `.flush()`, `.close()` |
//!
//! # Native bindings registered here
//!
//! | Name | Signature | Description |
//! |---|---|---|
//! | `_lumen_show_open_file_picker` | `() → Option<String>` | Open file dialog → JSON `{name,token,size}` or null |
//! | `_lumen_show_save_file_picker` | `(name: String) → Option<String>` | Save dialog → write-handle id or null |
//! | `_lumen_show_directory_picker` | `() → Option<String>` | Directory dialog → JSON `{name,path_id}` or null |
//! | `_lumen_dir_entries` | `(path_id: String) → String` | List directory → JSON `[{name,kind,token\|path_id,size}]`, one grant per entry |
//! | `_lumen_dir_get_file` | `(path_id, name, create: bool) → String` | Get/create file → JSON `{name,token,size}` or `{error}` |
//! | `_lumen_dir_get_subdir` | `(path_id, name, create: bool) → String` | Get/create subdir → JSON `{name,path_id}` or `{error}` |
//! | `_lumen_dir_remove_entry` | `(path_id, name, recursive: bool) → String` | Remove entry → `""` or a DOMException name |
//! | `_lumen_fs_resolve` | `(parent_id, child_dir_id, child_token) → Option<String>` | Relative path → JSON `[segment, …]` or null |
//! | `_lumen_fs_permission` | `(path_id, token, mode) → String` | `granted` / `prompt` / `denied` for `query`/`requestPermission` |
//! | `_lumen_fs_unique_id` | `(path_id, token) → Option<String>` | Stable opaque per-entry label (`getUniqueId`, `isSameEntry`) |
//! | `_lumen_fs_remove` | `(path_id, token, recursive: bool) → String` | Remove the handle's own entry → `""` or a DOMException name |
//! | `_lumen_fs_move` | `(token, dest_dir_id, new_name) → String` | Rename/move a file → JSON `{name,token,size}` or `{error}` |
//! | `_lumen_fs_file_size` | `(token: String) → f64` | Current size of the entry, or `-1` |
//! | `_lumen_sync_open` | `(token: String) → Option<String>` | Open an OPFS file for sync access → handle id |
//! | `_lumen_sync_size` / `_lumen_sync_flush` / `_lumen_sync_close` | `(id) → f64` / `bool` / `bool` | Size, flush, close |
//! | `_lumen_sync_read` | `(id, at: f64, len: f64) → String` | Read at a position → base64 |
//! | `_lumen_sync_write` | `(id, at: f64, data_b64) → f64` | Write at a position → byte count, or `-1` |
//! | `_lumen_sync_truncate` | `(id: String, size: f64) → bool` | Resize the open file |
//! | `_lumen_writable_write_bytes` | `(handle_id, position: f64, data_b64) → bool` | Write base64 bytes at a file position |
//! | `_lumen_writable_truncate` | `(handle_id: String, size: f64) → bool` | Resize the pending buffer |
//! | `_lumen_writable_close` | `(handle_id: String) → bool` | Flush and close writable stream |
//! | `_lumen_writable_from_token` | `(token: String) → Option<String>` | Write-handle id for an OPFS file token |
//!
//! All of them are deleted from the global object by
//! [`crate::file_input::seal_file_natives_v8`] once the shim below has captured
//! them into closure variables.
//!
//! # Origin private file system
//!
//! `navigator.storage.getDirectory()` ([`crate::storage_manager`]) resolves a
//! handle of the class defined *here*, over a real directory under
//! `<exe_dir>/data/opfs/<origin>/` registered in the same `DIR_REG` as a picked
//! directory — see [`opfs_root_entry_json`]. It used to resolve a same-named
//! stub class private to `storage_manager.rs` whose methods answered without
//! touching a file system at all ([BUG-372]).

// All items below are reachable only through `install_filesystem_access_v8`
// (the rquickjs twin was removed in S12b-B20) — gated so a default (non-v8)
// build doesn't trip `dead_code`.
#[cfg(feature = "v8-backend")]
use std::collections::HashMap;
#[cfg(feature = "v8-backend")]
use std::path::PathBuf;
#[cfg(feature = "v8-backend")]
use std::sync::{Mutex, OnceLock};

// ── Write-handle registry ──────────────────────────────────────────────────────

/// Pending write buffer for an open `FileSystemWritableFileStream`.
#[cfg(feature = "v8-backend")]
struct WriteHandle {
    /// Origin the save grant was issued to — checked on every append/close.
    origin: String,
    /// Target file path (caller-confirmed via the save picker).
    path: PathBuf,
    /// Accumulated write data.
    data: Vec<u8>,
}

#[cfg(feature = "v8-backend")]
struct WriteRegistry {
    handles: HashMap<String, WriteHandle>,
}

#[cfg(feature = "v8-backend")]
impl WriteRegistry {
    fn new() -> Self {
        Self { handles: HashMap::new() }
    }

    /// Issue an unguessable write-handle id for `origin`. Empty string (which is
    /// never a valid id) if the OS entropy source failed.
    fn allocate(&mut self, path: PathBuf, origin: &str) -> String {
        let Some(id) = crate::file_input::new_grant_id() else {
            return String::new();
        };
        self.handles.insert(
            id.clone(),
            WriteHandle { origin: origin.to_string(), path, data: Vec::new() },
        );
        id
    }

    /// Splice `bytes` into the pending buffer at `position`, zero-filling the gap
    /// if the write starts past the current end.
    ///
    /// FS §6.2 defines every write in terms of a file position, not of appending:
    /// `seek()` moves it, `write({type:'write', position})` writes at an explicit
    /// one. The buffer used to be append-only, which is why `seek()`/`truncate()`
    /// could only be no-ops (BUG-374 point 7).
    fn write_bytes(&mut self, id: &str, origin: &str, position: u64, bytes: &[u8]) -> bool {
        match self.handles.get_mut(id) {
            Some(h) if h.origin == origin => {
                let pos = position as usize;
                let end = pos.saturating_add(bytes.len());
                if h.data.len() < end {
                    h.data.resize(end, 0);
                }
                h.data[pos..end].copy_from_slice(bytes);
                true
            }
            _ => false,
        }
    }

    /// Resize the pending buffer to `size`, zero-filling if it grows (FS §6.2).
    fn truncate(&mut self, id: &str, origin: &str, size: u64) -> bool {
        match self.handles.get_mut(id) {
            Some(h) if h.origin == origin => {
                h.data.resize(size as usize, 0);
                true
            }
            _ => false,
        }
    }

    fn close(&mut self, id: &str, origin: &str) -> bool {
        if !matches!(self.handles.get(id), Some(h) if h.origin == origin) {
            return false;
        }
        match self.handles.remove(id) {
            Some(h) => std::fs::write(&h.path, &h.data).is_ok(),
            None => false,
        }
    }

    fn revoke_origin(&mut self, origin: &str) {
        self.handles.retain(|_, h| h.origin != origin);
    }
}

#[cfg(feature = "v8-backend")]
static WRITE_REG: OnceLock<Mutex<WriteRegistry>> = OnceLock::new();

#[cfg(feature = "v8-backend")]
fn write_reg() -> &'static Mutex<WriteRegistry> {
    WRITE_REG.get_or_init(|| Mutex::new(WriteRegistry::new()))
}

// ── Sync-access-handle registry ────────────────────────────────────────────────

/// One open `FileSystemSyncAccessHandle`: an actual OS file handle, since the
/// interface's whole point is that reads and writes take effect immediately
/// rather than being buffered until `close()` (FS §7.2).
#[cfg(feature = "v8-backend")]
struct SyncHandle {
    /// Origin the grant was issued to — checked on every call.
    origin: String,
    /// Open read-write file. Never exposed to JS; only the id is.
    file: std::fs::File,
}

#[cfg(feature = "v8-backend")]
static SYNC_REG: OnceLock<Mutex<HashMap<String, SyncHandle>>> = OnceLock::new();

#[cfg(feature = "v8-backend")]
fn sync_reg() -> &'static Mutex<HashMap<String, SyncHandle>> {
    SYNC_REG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Run `f` over the open file behind `id`, or `None` if `id` was not issued to
/// `origin`.
#[cfg(feature = "v8-backend")]
fn with_sync_file<T>(
    id: &str,
    origin: &str,
    f: impl FnOnce(&mut std::fs::File) -> Option<T>,
) -> Option<T> {
    let mut reg = sync_reg().lock().unwrap_or_else(|e| e.into_inner());
    let handle = reg.get_mut(id)?;
    if handle.origin != origin {
        return None;
    }
    f(&mut handle.file)
}

// ── Directory-handle registry ──────────────────────────────────────────────────

/// One directory grant: the directory it unlocks and the origin it was issued to.
#[cfg(feature = "v8-backend")]
struct DirGrant {
    /// Origin the directory grant was issued to — checked on every traversal.
    origin: String,
    /// Directory path. Never exposed to JS.
    path: PathBuf,
    /// Whether entries may be created and removed under `path`.
    ///
    /// False for a directory the user picked: the picker asks for access to
    /// what is already there, not for permission to rewrite it. True only for
    /// the origin's own sandbox (OPFS, [BUG-372]) and everything below it,
    /// where the origin owns the whole tree.
    writable: bool,
}

#[cfg(feature = "v8-backend")]
struct DirRegistry {
    paths: HashMap<String, DirGrant>,
}

#[cfg(feature = "v8-backend")]
impl DirRegistry {
    fn new() -> Self {
        Self { paths: HashMap::new() }
    }

    /// Issue an unguessable directory id for `origin`. Empty string (which is
    /// never a valid id) if the OS entropy source failed.
    ///
    /// `writable` decides whether the id also unlocks creating and removing
    /// entries — see [`DirGrant::writable`].
    fn allocate(&mut self, path: PathBuf, origin: &str, writable: bool) -> String {
        let Some(id) = crate::file_input::new_grant_id() else {
            return String::new();
        };
        self.paths
            .insert(id.clone(), DirGrant { origin: origin.to_string(), path, writable });
        id
    }

    fn get(&self, id: &str, origin: &str) -> Option<&PathBuf> {
        match self.paths.get(id) {
            Some(g) if g.origin == origin => Some(&g.path),
            _ => None,
        }
    }

    /// Directory and its write permission, or `None` if `id` was not issued to
    /// `origin`.
    fn get_grant(&self, id: &str, origin: &str) -> Option<(PathBuf, bool)> {
        match self.paths.get(id) {
            Some(g) if g.origin == origin => Some((g.path.clone(), g.writable)),
            _ => None,
        }
    }

    fn revoke_origin(&mut self, origin: &str) {
        self.paths.retain(|_, g| g.origin != origin);
    }
}

#[cfg(feature = "v8-backend")]
static DIR_REG: OnceLock<Mutex<DirRegistry>> = OnceLock::new();

#[cfg(feature = "v8-backend")]
fn dir_reg() -> &'static Mutex<DirRegistry> {
    DIR_REG.get_or_init(|| Mutex::new(DirRegistry::new()))
}

// ── OS file/directory dialogs ──────────────────────────────────────────────────

#[cfg(all(target_os = "windows", feature = "v8-backend"))]
fn os_open_file_picker() -> Option<PathBuf> {
    let ps = r#"
Add-Type -AssemblyName System.Windows.Forms
$dlg = New-Object System.Windows.Forms.OpenFileDialog
$dlg.Filter = "All files (*.*)|*.*"
if ($dlg.ShowDialog() -eq 'OK') { $dlg.FileName }
"#;
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", ps])
        .output()
        .ok()?;
    if out.status.success() {
        let p = String::from_utf8(out.stdout).ok()?.trim().to_string();
        if p.is_empty() { None } else { Some(PathBuf::from(p)) }
    } else {
        None
    }
}

#[cfg(all(target_os = "linux", feature = "v8-backend"))]
fn os_open_file_picker() -> Option<PathBuf> {
    let out = std::process::Command::new("zenity")
        .args(["--file-selection", "--title=Open File"])
        .output()
        .ok()?;
    if out.status.success() {
        let p = String::from_utf8(out.stdout).ok()?.trim().to_string();
        if p.is_empty() { None } else { Some(PathBuf::from(p)) }
    } else {
        None
    }
}

#[cfg(all(target_os = "macos", feature = "v8-backend"))]
fn os_open_file_picker() -> Option<PathBuf> {
    let out = std::process::Command::new("osascript")
        .args(["-e", "POSIX path of (choose file without invisibles)"])
        .output()
        .ok()?;
    if out.status.success() {
        let p = String::from_utf8(out.stdout).ok()?.trim().to_string();
        if p.is_empty() { None } else { Some(PathBuf::from(p)) }
    } else {
        None
    }
}

#[cfg(all(not(any(target_os = "windows", target_os = "linux", target_os = "macos")), feature = "v8-backend"))]
fn os_open_file_picker() -> Option<PathBuf> {
    None
}

#[cfg(all(target_os = "windows", feature = "v8-backend"))]
fn os_save_file_picker(suggested: &str) -> Option<PathBuf> {
    let safe = suggested.replace('"', "\\\"");
    let ps = format!(
        r#"
Add-Type -AssemblyName System.Windows.Forms
$dlg = New-Object System.Windows.Forms.SaveFileDialog
$dlg.FileName = "{safe}"
$dlg.Filter = "All files (*.*)|*.*"
if ($dlg.ShowDialog() -eq 'OK') {{ $dlg.FileName }}
"#
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", ps.as_str()])
        .output()
        .ok()?;
    if out.status.success() {
        let p = String::from_utf8(out.stdout).ok()?.trim().to_string();
        if p.is_empty() { None } else { Some(PathBuf::from(p)) }
    } else {
        None
    }
}

#[cfg(all(target_os = "linux", feature = "v8-backend"))]
fn os_save_file_picker(suggested: &str) -> Option<PathBuf> {
    let out = std::process::Command::new("zenity")
        .args(["--file-selection", "--save", &format!("--filename={suggested}"), "--title=Save File"])
        .output()
        .ok()?;
    if out.status.success() {
        let p = String::from_utf8(out.stdout).ok()?.trim().to_string();
        if p.is_empty() { None } else { Some(PathBuf::from(p)) }
    } else {
        None
    }
}

#[cfg(all(target_os = "macos", feature = "v8-backend"))]
fn os_save_file_picker(_suggested: &str) -> Option<PathBuf> {
    let out = std::process::Command::new("osascript")
        .args(["-e", "POSIX path of (choose file name)"])
        .output()
        .ok()?;
    if out.status.success() {
        let p = String::from_utf8(out.stdout).ok()?.trim().to_string();
        if p.is_empty() { None } else { Some(PathBuf::from(p)) }
    } else {
        None
    }
}

#[cfg(all(not(any(target_os = "windows", target_os = "linux", target_os = "macos")), feature = "v8-backend"))]
fn os_save_file_picker(_suggested: &str) -> Option<PathBuf> {
    None
}

#[cfg(all(target_os = "windows", feature = "v8-backend"))]
fn os_dir_picker() -> Option<PathBuf> {
    let ps = r#"
Add-Type -AssemblyName System.Windows.Forms
$dlg = New-Object System.Windows.Forms.FolderBrowserDialog
if ($dlg.ShowDialog() -eq 'OK') { $dlg.SelectedPath }
"#;
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", ps])
        .output()
        .ok()?;
    if out.status.success() {
        let p = String::from_utf8(out.stdout).ok()?.trim().to_string();
        if p.is_empty() { None } else { Some(PathBuf::from(p)) }
    } else {
        None
    }
}

#[cfg(all(target_os = "linux", feature = "v8-backend"))]
fn os_dir_picker() -> Option<PathBuf> {
    let out = std::process::Command::new("zenity")
        .args(["--file-selection", "--directory", "--title=Choose Folder"])
        .output()
        .ok()?;
    if out.status.success() {
        let p = String::from_utf8(out.stdout).ok()?.trim().to_string();
        if p.is_empty() { None } else { Some(PathBuf::from(p)) }
    } else {
        None
    }
}

#[cfg(all(target_os = "macos", feature = "v8-backend"))]
fn os_dir_picker() -> Option<PathBuf> {
    let out = std::process::Command::new("osascript")
        .args(["-e", "POSIX path of (choose folder)"])
        .output()
        .ok()?;
    if out.status.success() {
        let p = String::from_utf8(out.stdout).ok()?.trim().to_string();
        if p.is_empty() { None } else { Some(PathBuf::from(p)) }
    } else {
        None
    }
}

#[cfg(all(not(any(target_os = "windows", target_os = "linux", target_os = "macos")), feature = "v8-backend"))]
fn os_dir_picker() -> Option<PathBuf> {
    None
}

// ── JSON helpers (no external dep) ────────────────────────────────────────────

#[cfg(feature = "v8-backend")]
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for ch in s.chars() {
        match ch {
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c    => out.push(c),
        }
    }
    out
}

#[cfg(feature = "v8-backend")]
fn file_entry_json(path: &std::path::Path, origin: &str, writable: bool) -> Option<String> {
    let name  = path.file_name()?.to_str()?.to_string();
    let size  = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let token = if writable {
        crate::file_input::register_writable_file_token(path.to_str()?, origin)
    } else {
        crate::file_input::register_file_token(path.to_str()?, origin)
    };
    Some(format!(
        r#"{{"name":"{}","token":"{}","size":{}}}"#,
        json_escape(&name),
        json_escape(&token),
        size
    ))
}

/// `{"error":"<DOMException name>"}` — the shape every directory native returns
/// instead of a handle when the operation is refused.
///
/// The natives report a *name*, not `null`: `getFileHandle('x', {create:true})`
/// failing because the entry is missing, because the grant is read-only, or
/// because `x` is a directory are three different exceptions to the caller, and
/// collapsing them into one falsy value is what let the old OPFS stub answer
/// every call with a plausible-looking lie (BUG-372).
#[cfg(feature = "v8-backend")]
fn fs_error_json(name: &str) -> String {
    format!(r#"{{"error":"{}"}}"#, json_escape(name))
}

/// Whether `name` may be used as a single entry name inside a directory.
///
/// File System Access §7.2 forbids the empty string, `.`, `..` and any name
/// containing a path separator. This is the only thing standing between page
/// script and `getFileHandle('../../secrets')`, so it is checked in Rust on
/// every call rather than in the shim.
#[cfg(feature = "v8-backend")]
fn valid_entry_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

/// Path behind a handle, whichever of the two grant kinds it carries.
///
/// A directory handle presents `path_id`, a file handle `token`; the base
/// interface's members (`remove`, `getUniqueId`, `isSameEntry`) are defined on
/// `FileSystemHandle` and therefore have to accept either.
#[cfg(feature = "v8-backend")]
fn resolve_handle_path(path_id: &str, token: &str, origin: &str) -> Option<PathBuf> {
    if !path_id.is_empty() {
        return dir_reg().lock().unwrap_or_else(|e| e.into_inner()).get(path_id, origin).cloned();
    }
    if !token.is_empty() {
        return crate::file_input::path_for_token(token, origin);
    }
    None
}

/// `origin`-scoped random label per file-system path, stable for the lifetime of
/// the process.
#[cfg(feature = "v8-backend")]
static UNIQUE_IDS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

/// Opaque, stable identifier of the entry at `path` as seen by `origin`.
///
/// Backs `FileSystemHandle.getUniqueId()` and, through it, `isSameEntry()`: two
/// handles on the same file get *different* grant tokens (every
/// `getFileHandle()` mints a fresh one), so comparing tokens answered "not the
/// same entry" for two handles that are, in fact, the same entry.
///
/// The label is drawn from the same CSPRNG as a grant id rather than hashed from
/// the path: a hash would let a page confirm a guessed absolute path by
/// comparing digests, which is exactly the information the token model exists to
/// withhold.
#[cfg(feature = "v8-backend")]
fn unique_id_for_path(path: &std::path::Path, origin: &str) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let key = format!("{origin}\u{0}{}", canonical.to_string_lossy());
    let mut map = UNIQUE_IDS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = map.get(&key) {
        return existing.clone();
    }
    let id = crate::file_input::new_grant_id().unwrap_or_default();
    map.insert(key, id.clone());
    id
}

// ── Origin private file system (OPFS) ──────────────────────────────────────────

/// Root of the OPFS tree for the whole installation: `<exe_dir>/data/opfs/`.
///
/// Follows the portable-data convention (CLAUDE.md, user decision 2026-06-16):
/// browser state lives in the browser folder, never in `%APPDATA%`/`~/.config`.
/// The shell's `browser_data_dir()` implements the same rule, but `lumen-js`
/// sits below `lumen-shell` in the dependency graph and cannot call it.
#[cfg(feature = "v8-backend")]
fn opfs_base_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("data").join("opfs"))
}

/// 64-bit FNV-1a — enough to keep two origins in distinct directories without
/// pulling in a hash dependency for a name that is never security-critical on
/// its own (the grant registry, not the path, is what gates access).
#[cfg(feature = "v8-backend")]
fn fnv1a64(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Directory name holding one origin's OPFS tree.
///
/// A readable prefix for anyone looking at the folder, plus the hash of the
/// *full* origin so that two origins sharing a prefix — or an opaque origin,
/// which here is the whole document URL and can be arbitrarily long — never
/// collide after the prefix is capped.
#[cfg(feature = "v8-backend")]
fn origin_slug(origin: &str) -> String {
    let prefix: String = origin
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.')
        .take(32)
        .collect();
    let prefix = if prefix.is_empty() { "origin".to_string() } else { prefix };
    format!("{prefix}-{:016x}", fnv1a64(origin))
}

/// Create (if needed) and return the OPFS root directory of `origin`.
#[cfg(feature = "v8-backend")]
fn opfs_root_for_origin(origin: &str) -> Option<PathBuf> {
    let dir = opfs_base_dir()?.join(origin_slug(origin));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Allocate a writable directory grant for `origin`'s OPFS root and describe it
/// as `{"name":"","path_id":"…"}`, the same JSON shape the directory picker
/// returns.
///
/// Called by [`crate::storage_manager`] to back `navigator.storage.getDirectory()`.
/// The grant is minted here, next to `DIR_REG`, so that the OPFS root is an
/// ordinary entry of the one directory registry — the previous stub handle
/// carried no grant at all, which is why every method on it had to lie
/// (BUG-372).
#[cfg(feature = "v8-backend")]
pub(crate) fn opfs_root_entry_json(origin: &str) -> Option<String> {
    let root = opfs_root_for_origin(origin)?;
    let id = dir_reg()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .allocate(root, origin, true);
    if id.is_empty() {
        return None;
    }
    // Per FS §3, the root's name is the empty string.
    Some(format!(r#"{{"name":"","path_id":"{}"}}"#, json_escape(&id)))
}

// ── install ────────────────────────────────────────────────────────────────────

/// Install File System Access API bindings and JS class shim into a V8 runtime
/// (Ph3 V8 migration S5-S7 batch 2; the rquickjs twin was removed in S12b-B20):
/// all eight natives go through the compat layer, the JS shim evaluates
/// unchanged. Must be called after [`crate::file_input::install_file_input_bindings_v8`].
///
/// `origin` is the installing document's origin
/// ([`crate::file_input::origin_for_url`]), captured in Rust and compared
/// against every directory/write id the page presents. Installing also revokes
/// the grants of the previous document on that origin (BUG-371 point 3).
#[cfg(feature = "v8-backend")]
pub(crate) fn install_filesystem_access_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    origin: &str,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn2, into_v8_fn3};
    use lumen_core::ext::JsRuntime as _;

    // Reload of the same origin: nothing the previous document was granted
    // stays reachable, including a save-handle it left open.
    dir_reg().lock().unwrap_or_else(|e| e.into_inner()).revoke_origin(origin);
    write_reg().lock().unwrap_or_else(|e| e.into_inner()).revoke_origin(origin);
    // Including any file the previous document still had open for sync access.
    sync_reg().lock().unwrap_or_else(|e| e.into_inner()).retain(|_, h| h.origin != origin);

    let open_origin = origin.to_string();
    let open_picker = into_v8_fn0(move || -> Option<String> {
        let path = os_open_file_picker()?;
        file_entry_json(&path, &open_origin, false)
    });
    rt.register_native("_lumen_show_open_file_picker", open_picker)?;

    let save_origin = origin.to_string();
    let save_picker = into_v8_fn1(move |suggested: String| -> Option<String> {
        let path = os_save_file_picker(&suggested)?;
        let id = write_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .allocate(path, &save_origin);
        if id.is_empty() { None } else { Some(id) }
    });
    rt.register_native("_lumen_show_save_file_picker", save_picker)?;

    let dir_pick_origin = origin.to_string();
    let dir_picker = into_v8_fn0(move || -> Option<String> {
        let path = os_dir_picker()?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("folder")
            .to_string();
        // A picked directory is a read grant: see `DirGrant::writable`.
        let id = dir_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .allocate(path, &dir_pick_origin, false);
        if id.is_empty() {
            return None;
        }
        Some(format!(
            r#"{{"name":"{}","path_id":"{}"}}"#,
            json_escape(&name),
            json_escape(&id)
        ))
    });
    rt.register_native("_lumen_show_directory_picker", dir_picker)?;

    let entries_origin = origin.to_string();
    let dir_entries = into_v8_fn1(move |path_id: String| -> String {
        let grant = dir_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_grant(&path_id, &entries_origin);
        let Some((dir, writable)) = grant else { return "[]".to_string() };
        let Ok(rd) = std::fs::read_dir(&dir) else { return "[]".to_string() };
        let mut items = Vec::new();
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Each listed entry carries a grant of its own, inheriting the
            // parent's write permission. Listing used to hand back handles built
            // on an empty id, so iterating a directory produced handles with the
            // right `name`/`kind` and no contents at all — `getFile()` read
            // nothing and `entries()` on a listed subdirectory returned `[]`
            // (BUG-374 point 6, a silent wrong answer rather than an error).
            let item = if entry.path().is_dir() {
                let sub_id = dir_reg()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .allocate(entry.path(), &entries_origin, writable);
                format!(
                    r#"{{"name":"{}","kind":"directory","path_id":"{}"}}"#,
                    json_escape(&name),
                    json_escape(&sub_id)
                )
            } else {
                match file_entry_json(&entry.path(), &entries_origin, writable) {
                    // `{"name":…,"token":…,"size":…}` plus the discriminator the
                    // shim switches on.
                    Some(json) => format!(r#"{{"kind":"file",{}"#, &json[1..]),
                    None => continue,
                }
            };
            items.push(item);
        }
        format!("[{}]", items.join(","))
    });
    rt.register_native("_lumen_dir_entries", dir_entries)?;

    let get_file_origin = origin.to_string();
    let dir_get_file = into_v8_fn3(move |path_id: String, name: String, create: bool| -> String {
        if !valid_entry_name(&name) {
            return fs_error_json("TypeError");
        }
        let Some((dir, writable)) = dir_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_grant(&path_id, &get_file_origin)
        else {
            return fs_error_json("NotAllowedError");
        };
        let file_path = dir.join(&name);
        if file_path.is_dir() {
            return fs_error_json("TypeMismatchError");
        }
        if !file_path.is_file() {
            if !create {
                return fs_error_json("NotFoundError");
            }
            if !writable {
                return fs_error_json("NotAllowedError");
            }
            if std::fs::File::create(&file_path).is_err() {
                return fs_error_json("NoModificationAllowedError");
            }
        }
        file_entry_json(&file_path, &get_file_origin, writable)
            .unwrap_or_else(|| fs_error_json("NoModificationAllowedError"))
    });
    rt.register_native("_lumen_dir_get_file", dir_get_file)?;

    let subdir_origin = origin.to_string();
    let dir_get_subdir = into_v8_fn3(move |path_id: String, name: String, create: bool| -> String {
        if !valid_entry_name(&name) {
            return fs_error_json("TypeError");
        }
        let Some((parent, writable)) = dir_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_grant(&path_id, &subdir_origin)
        else {
            return fs_error_json("NotAllowedError");
        };
        let sub = parent.join(&name);
        if sub.is_file() {
            return fs_error_json("TypeMismatchError");
        }
        if !sub.is_dir() {
            if !create {
                return fs_error_json("NotFoundError");
            }
            if !writable {
                return fs_error_json("NotAllowedError");
            }
            if std::fs::create_dir(&sub).is_err() {
                return fs_error_json("NoModificationAllowedError");
            }
        }
        // A subdirectory inherits its parent's permission: the OPFS tree stays
        // writable all the way down, a picked tree stays read-only all the way
        // down.
        let sub_id = dir_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .allocate(sub, &subdir_origin, writable);
        if sub_id.is_empty() {
            return fs_error_json("NoModificationAllowedError");
        }
        format!(
            r#"{{"name":"{}","path_id":"{}"}}"#,
            json_escape(&name),
            json_escape(&sub_id)
        )
    });
    rt.register_native("_lumen_dir_get_subdir", dir_get_subdir)?;

    let remove_origin = origin.to_string();
    let dir_remove_entry =
        into_v8_fn3(move |path_id: String, name: String, recursive: bool| -> String {
            if !valid_entry_name(&name) {
                return "TypeError".to_string();
            }
            let Some((dir, writable)) = dir_reg()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_grant(&path_id, &remove_origin)
            else {
                return "NotAllowedError".to_string();
            };
            if !writable {
                return "NotAllowedError".to_string();
            }
            let target = dir.join(&name);
            if target.is_file() {
                return match std::fs::remove_file(&target) {
                    Ok(()) => String::new(),
                    Err(_) => "NoModificationAllowedError".to_string(),
                };
            }
            if target.is_dir() {
                if recursive {
                    return match std::fs::remove_dir_all(&target) {
                        Ok(()) => String::new(),
                        Err(_) => "NoModificationAllowedError".to_string(),
                    };
                }
                return match std::fs::remove_dir(&target) {
                    Ok(()) => String::new(),
                    // FS §7.5: a non-empty directory without `recursive` is the
                    // one failure the caller is expected to recover from.
                    Err(_) => "InvalidModificationError".to_string(),
                };
            }
            "NotFoundError".to_string()
        });
    rt.register_native("_lumen_dir_remove_entry", dir_remove_entry)?;

    // `resolve()` compares two handles the page holds. Both sides are grant ids,
    // never paths — the comparison happens here so neither path leaves Rust.
    let resolve_origin = origin.to_string();
    let fs_resolve = into_v8_fn3(
        move |parent_id: String, child_dir_id: String, child_token: String| -> Option<String> {
            let parent = dir_reg()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&parent_id, &resolve_origin)
                .cloned()?;
            let child = if !child_dir_id.is_empty() {
                dir_reg()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&child_dir_id, &resolve_origin)
                    .cloned()?
            } else if !child_token.is_empty() {
                crate::file_input::path_for_token(&child_token, &resolve_origin)?
            } else {
                return None;
            };
            // Not a descendant → `null`, which per FS §5.2 is a real answer, not
            // an error.
            let rel = child.strip_prefix(&parent).ok()?;
            let segments: Vec<String> = rel
                .components()
                .map(|c| format!(r#""{}""#, json_escape(&c.as_os_str().to_string_lossy())))
                .collect();
            Some(format!("[{}]", segments.join(",")))
        },
    );
    rt.register_native("_lumen_fs_resolve", fs_resolve)?;

    let from_token_origin = origin.to_string();
    let writable_from_token = into_v8_fn1(move |token: String| -> Option<String> {
        let path = crate::file_input::writable_path_for_token(&token, &from_token_origin)?;
        let id = write_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .allocate(path, &from_token_origin);
        if id.is_empty() { None } else { Some(id) }
    });
    rt.register_native("_lumen_writable_from_token", writable_from_token)?;

    // Data crosses as base64 rather than as a JS string: `write()` accepts
    // `ArrayBuffer`/typed arrays/`Blob`, whose bytes are not text and do not
    // survive a UTF-8 round trip (BUG-374 point 7 — a `Blob` used to land in the
    // file as the literal `[object Blob]`).
    let write_origin = origin.to_string();
    let writable_write_bytes =
        into_v8_fn3(move |handle_id: String, position: f64, data_b64: String| -> bool {
            let Some(bytes) = crate::sw_worker::base64_decode(&data_b64) else {
                return false;
            };
            if !(0.0..=(u64::MAX as f64)).contains(&position) {
                return false;
            }
            write_reg().lock().unwrap_or_else(|e| e.into_inner()).write_bytes(
                &handle_id,
                &write_origin,
                position as u64,
                &bytes,
            )
        });
    rt.register_native("_lumen_writable_write_bytes", writable_write_bytes)?;

    let truncate_origin = origin.to_string();
    let writable_truncate = into_v8_fn2(move |handle_id: String, size: f64| -> bool {
        if !(0.0..=(u64::MAX as f64)).contains(&size) {
            return false;
        }
        write_reg().lock().unwrap_or_else(|e| e.into_inner()).truncate(
            &handle_id,
            &truncate_origin,
            size as u64,
        )
    });
    rt.register_native("_lumen_writable_truncate", writable_truncate)?;

    // ── FileSystemHandle members (FS §4) ──────────────────────────────────────
    // All four take `(path_id, token)` — a directory handle presents the first,
    // a file handle the second — so one native shape serves both subclasses and
    // the base interface's methods stay on the base prototype (BUG-374 point 5).

    let perm_origin = origin.to_string();
    let fs_permission =
        into_v8_fn3(move |path_id: String, token: String, mode: String| -> String {
            let readwrite = mode == "readwrite";
            if !path_id.is_empty() {
                return match dir_reg()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_grant(&path_id, &perm_origin)
                {
                    // A picked directory is a read grant and there is no dialog
                    // that would upgrade it, so `readwrite` stays 'prompt'
                    // rather than claiming a permission the engine cannot give.
                    Some((_, writable)) if readwrite && !writable => "prompt",
                    Some(_) => "granted",
                    None => "denied",
                }
                .to_string();
            }
            if !token.is_empty() {
                if crate::file_input::writable_path_for_token(&token, &perm_origin).is_some() {
                    return "granted".to_string();
                }
                if crate::file_input::path_for_token(&token, &perm_origin).is_some() {
                    return if readwrite { "prompt" } else { "granted" }.to_string();
                }
            }
            "denied".to_string()
        });
    rt.register_native("_lumen_fs_permission", fs_permission)?;

    let unique_origin = origin.to_string();
    let fs_unique_id = into_v8_fn2(move |path_id: String, token: String| -> Option<String> {
        let path = resolve_handle_path(&path_id, &token, &unique_origin)?;
        Some(unique_id_for_path(&path, &unique_origin))
    });
    rt.register_native("_lumen_fs_unique_id", fs_unique_id)?;

    let remove_origin = origin.to_string();
    let fs_remove =
        into_v8_fn3(move |path_id: String, token: String, recursive: bool| -> String {
            if !path_id.is_empty() {
                let Some((dir, writable)) = dir_reg()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_grant(&path_id, &remove_origin)
                else {
                    return "NotAllowedError".to_string();
                };
                if !writable {
                    return "NotAllowedError".to_string();
                }
                let res = if recursive {
                    std::fs::remove_dir_all(&dir)
                } else {
                    std::fs::remove_dir(&dir)
                };
                return match res {
                    Ok(()) => String::new(),
                    // FS §7.5: a non-empty directory without `recursive` is the
                    // one failure the caller is expected to recover from.
                    Err(_) if !recursive => "InvalidModificationError".to_string(),
                    Err(_) => "NoModificationAllowedError".to_string(),
                };
            }
            let Some(path) = crate::file_input::writable_path_for_token(&token, &remove_origin)
            else {
                return "NotAllowedError".to_string();
            };
            match std::fs::remove_file(&path) {
                Ok(()) => String::new(),
                Err(_) => "NoModificationAllowedError".to_string(),
            }
        });
    rt.register_native("_lumen_fs_remove", fs_remove)?;

    let move_origin = origin.to_string();
    let fs_move =
        into_v8_fn3(move |token: String, dest_dir_id: String, new_name: String| -> String {
            let Some(src) = crate::file_input::writable_path_for_token(&token, &move_origin)
            else {
                return fs_error_json("NotAllowedError");
            };
            let dest_dir = if dest_dir_id.is_empty() {
                match src.parent() {
                    Some(p) => p.to_path_buf(),
                    None => return fs_error_json("NotAllowedError"),
                }
            } else {
                match dir_reg()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_grant(&dest_dir_id, &move_origin)
                {
                    Some((dir, true)) => dir,
                    _ => return fs_error_json("NotAllowedError"),
                }
            };
            let name = if new_name.is_empty() {
                match src.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => return fs_error_json("TypeError"),
                }
            } else {
                new_name
            };
            if !valid_entry_name(&name) {
                return fs_error_json("TypeError");
            }
            let target = dest_dir.join(&name);
            if std::fs::rename(&src, &target).is_err() {
                return fs_error_json("NoModificationAllowedError");
            }
            // The handle now represents the entry at its new location (FS §4),
            // so it needs a grant for the new path — the old token still points
            // at a name that no longer exists.
            file_entry_json(&target, &move_origin, true)
                .unwrap_or_else(|| fs_error_json("NoModificationAllowedError"))
        });
    rt.register_native("_lumen_fs_move", fs_move)?;

    // A handle caches the size it was created with, and the file changes under
    // it: after `truncate(5)` the page's own `getFile().size` still reported the
    // length the entry had when `getFileHandle()` ran, while `text()` already
    // returned the new contents (BUG-374, found by the live probe).
    let size_origin = origin.to_string();
    let fs_file_size = into_v8_fn1(move |token: String| -> f64 {
        match crate::file_input::path_for_token(&token, &size_origin) {
            Some(path) => std::fs::metadata(&path).map(|m| m.len() as f64).unwrap_or(-1.0),
            None => -1.0,
        }
    });
    rt.register_native("_lumen_fs_file_size", fs_file_size)?;

    // ── FileSystemSyncAccessHandle (FS §7.2) ──────────────────────────────────
    // Unbuffered, synchronous access to one OPFS file. Every call answers from
    // the open file itself, so a page can read back what it just wrote without
    // closing anything.

    let sync_open_origin = origin.to_string();
    let sync_open = into_v8_fn1(move |token: String| -> Option<String> {
        // Only a file the origin owns outright: a picked file carries a read
        // grant, and the spec exposes this interface on OPFS handles only.
        let path = crate::file_input::writable_path_for_token(&token, &sync_open_origin)?;
        let file = std::fs::OpenOptions::new().read(true).write(true).open(&path).ok()?;
        let id = crate::file_input::new_grant_id()?;
        sync_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone(), SyncHandle { origin: sync_open_origin.clone(), file });
        Some(id)
    });
    rt.register_native("_lumen_sync_open", sync_open)?;

    let sync_size_origin = origin.to_string();
    let sync_size = into_v8_fn1(move |id: String| -> f64 {
        with_sync_file(&id, &sync_size_origin, |f| f.metadata().ok().map(|m| m.len() as f64))
            .unwrap_or(-1.0)
    });
    rt.register_native("_lumen_sync_size", sync_size)?;

    let sync_read_origin = origin.to_string();
    let sync_read = into_v8_fn3(move |id: String, at: f64, len: f64| -> String {
        use std::io::{Read as _, Seek as _, SeekFrom};
        if !(0.0..=(u64::MAX as f64)).contains(&at) || !(0.0..=(u32::MAX as f64)).contains(&len) {
            return String::new();
        }
        with_sync_file(&id, &sync_read_origin, |f| {
            f.seek(SeekFrom::Start(at as u64)).ok()?;
            let mut buf = vec![0u8; len as usize];
            let mut filled = 0usize;
            // `read` may stop short of the buffer without being at EOF.
            while filled < buf.len() {
                match f.read(&mut buf[filled..]) {
                    Ok(0) => break,
                    Ok(n) => filled += n,
                    Err(_) => return None,
                }
            }
            buf.truncate(filled);
            Some(crate::sw_worker::base64_encode(&buf))
        })
        .unwrap_or_default()
    });
    rt.register_native("_lumen_sync_read", sync_read)?;

    let sync_write_origin = origin.to_string();
    let sync_write = into_v8_fn3(move |id: String, at: f64, data_b64: String| -> f64 {
        use std::io::{Seek as _, SeekFrom, Write as _};
        let Some(bytes) = crate::sw_worker::base64_decode(&data_b64) else {
            return -1.0;
        };
        if !(0.0..=(u64::MAX as f64)).contains(&at) {
            return -1.0;
        }
        with_sync_file(&id, &sync_write_origin, |f| {
            f.seek(SeekFrom::Start(at as u64)).ok()?;
            f.write_all(&bytes).ok()?;
            Some(bytes.len() as f64)
        })
        .unwrap_or(-1.0)
    });
    rt.register_native("_lumen_sync_write", sync_write)?;

    let sync_truncate_origin = origin.to_string();
    let sync_truncate = into_v8_fn2(move |id: String, size: f64| -> bool {
        if !(0.0..=(u64::MAX as f64)).contains(&size) {
            return false;
        }
        with_sync_file(&id, &sync_truncate_origin, |f| f.set_len(size as u64).ok()).is_some()
    });
    rt.register_native("_lumen_sync_truncate", sync_truncate)?;

    let sync_flush_origin = origin.to_string();
    let sync_flush = into_v8_fn1(move |id: String| -> bool {
        use std::io::Write as _;
        with_sync_file(&id, &sync_flush_origin, |f| f.flush().ok()).is_some()
    });
    rt.register_native("_lumen_sync_flush", sync_flush)?;

    let sync_close_origin = origin.to_string();
    let sync_close = into_v8_fn1(move |id: String| -> bool {
        let mut reg = sync_reg().lock().unwrap_or_else(|e| e.into_inner());
        match reg.get(&id) {
            Some(h) if h.origin == sync_close_origin => reg.remove(&id).is_some(),
            _ => false,
        }
    });
    rt.register_native("_lumen_sync_close", sync_close)?;

    let close_origin = origin.to_string();
    let writable_close = into_v8_fn1(move |handle_id: String| -> bool {
        write_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .close(&handle_id, &close_origin)
    });
    rt.register_native("_lumen_writable_close", writable_close)?;

    rt.eval(FSAL_SHIM)?;
    Ok(())
}

// ── JS shim ────────────────────────────────────────────────────────────────────

/// Defines the File System Access interface hierarchy — `FileSystemHandle` and
/// its two subclasses, plus `FileSystemWritableFileStream` — and wraps the picker
/// globals as Promise-returning APIs.
///
/// BUG-371 moved the whole shim inside an IIFE. It used to run at top level so
/// its classes were global bindings *and* `window.X`; but the natives it calls
/// are deleted from the global object right after install
/// ([`crate::file_input::seal_file_natives_v8`]), so they have to be captured in
/// closure scope first. The classes are still reachable exactly as before —
/// `window.X = X` on a real page makes them globals anyway.
///
/// BUG-374 gave the three classes their WebIDL shape. They used to be three
/// unrelated ES5 constructor functions: no common `FileSystemHandle` ancestor
/// (so `if (window.FileSystemHandle)` feature-detects answered "unsupported"),
/// publicly constructible (spec: `TypeError: Illegal constructor`), `kind`/`name`
/// held as enumerable *writable own data properties* — `fileHandle.kind =
/// 'directory'` was accepted and the object then lied about its own type — no
/// `Symbol.toStringTag`, no `queryPermission`/`requestPermission`/`remove`/
/// `getUniqueId`, no async iteration of a directory, and a writable stream that
/// was not a `WritableStream` and whose `seek`/`truncate` did nothing at all.
#[cfg(feature = "v8-backend")]
const FSAL_SHIM: &str = r#"
(function() {
'use strict';

// BUG-371 point 1: the natives live here from now on, not on `window`.
function nat(name) {
  return (typeof globalThis[name] === 'function') ? globalThis[name] : null;
}
var NAT_OPEN_PICKER  = nat('_lumen_show_open_file_picker');
var NAT_SAVE_PICKER  = nat('_lumen_show_save_file_picker');
var NAT_DIR_PICKER   = nat('_lumen_show_directory_picker');
var NAT_DIR_ENTRIES  = nat('_lumen_dir_entries');
var NAT_DIR_GET_FILE = nat('_lumen_dir_get_file');
var NAT_DIR_GET_SUB  = nat('_lumen_dir_get_subdir');
var NAT_DIR_REMOVE   = nat('_lumen_dir_remove_entry');
var NAT_FS_RESOLVE   = nat('_lumen_fs_resolve');
var NAT_FS_PERM      = nat('_lumen_fs_permission');
var NAT_FS_UNIQUE    = nat('_lumen_fs_unique_id');
var NAT_FS_REMOVE    = nat('_lumen_fs_remove');
var NAT_FS_MOVE      = nat('_lumen_fs_move');
var NAT_FS_SIZE      = nat('_lumen_fs_file_size');
var NAT_SYNC_OPEN    = nat('_lumen_sync_open');
var NAT_SYNC_SIZE    = nat('_lumen_sync_size');
var NAT_SYNC_READ    = nat('_lumen_sync_read');
var NAT_SYNC_WRITE   = nat('_lumen_sync_write');
var NAT_SYNC_TRUNC   = nat('_lumen_sync_truncate');
var NAT_SYNC_FLUSH   = nat('_lumen_sync_flush');
var NAT_SYNC_CLOSE   = nat('_lumen_sync_close');
var NAT_WRITE_BYTES  = nat('_lumen_writable_write_bytes');
var NAT_WRITE_TRUNC  = nat('_lumen_writable_truncate');
var NAT_WRITE_CLOSE  = nat('_lumen_writable_close');
var NAT_WRITE_TOKEN  = nat('_lumen_writable_from_token');

// Every directory native answers with JSON: the entry on success, or
// `{"error":"<DOMException name>"}`. Turning that back into the right exception
// is one step, so it lives in one place.
function fsThrow(name, message) {
  if (name === 'TypeError') throw new TypeError(message);
  throw new DOMException(message, name);
}
function fsUnwrap(raw, message) {
  if (raw == null) fsThrow('NotSupportedError', message);
  var parsed = JSON.parse(raw);
  if (parsed && parsed.error) fsThrow(parsed.error, message);
  return parsed;
}

// Bridge into `file_input.rs`'s shim: the only way to attach a read grant to a
// `File`, since the token lives in that shim's private WeakMap. Also deleted
// from the global object by the sealing step.
var FS_INTERNAL = (typeof globalThis.__lumen_fs_internal === 'object')
  ? globalThis.__lumen_fs_internal : null;

// ── WebIDL plumbing ──────────────────────────────────────────────────────────

// None of these interfaces declares a constructor, so page script must not be
// able to build one: `new FileSystemFileHandle(name, token, size)` used to work
// and took the internal grant id straight back as an argument. Internal
// construction goes through the factories below, which hand this token in.
var BRAND = {};

// BUG-371 point 4 (same reasoning as `File._token`): a handle's grant id is a
// private slot, not an own property. Left web-visible, `handle._token` /
// `_pathId` / `_id` were the values an attacker needed. BUG-374 point 3 moved
// `kind`/`name` in here too — as own data properties they were writable, so a
// handle could be made to misreport its own kind.
//
//   handle -> {kind, name, token, size, pathId}
var STATE = new WeakMap();
// stream -> {id, position, closed}
var WRITE_STATE = new WeakMap();

function illegalConstructor() {
  throw new TypeError('Illegal constructor');
}

function stateOf(obj) {
  var st = (obj !== null && typeof obj === 'object') ? STATE.get(obj) : undefined;
  if (st === undefined) throw new TypeError('Illegal invocation');
  return st;
}

// WebIDL: a `readonly attribute` is an accessor on the interface prototype
// (enumerable, configurable, getter only), never an own property of the
// instance — which is why `Object.keys(handle)` must come back empty.
function defineAttribute(proto, name, getter) {
  Object.defineProperty(proto, name, { get: getter, enumerable: true, configurable: true });
}

function defineToStringTag(ctor, name) {
  Object.defineProperty(ctor.prototype, Symbol.toStringTag,
    { value: name, writable: false, enumerable: false, configurable: true });
}

// WebIDL inheritance: the prototype chain *and* the interface objects
// themselves (`Object.getPrototypeOf(FileSystemFileHandle) === FileSystemHandle`).
function inherit(sub, base, name) {
  sub.prototype = Object.create(base.prototype);
  Object.defineProperty(sub.prototype, 'constructor',
    { value: sub, writable: true, enumerable: false, configurable: true });
  Object.setPrototypeOf(sub, base);
  defineToStringTag(sub, name);
}

// Every operation here returns a promise, so an argument that fails validation
// must *reject* rather than throw synchronously.
function promiseTry(fn) {
  return Promise.resolve().then(fn);
}

// ── FileSystemHandle (FS §4) ─────────────────────────────────────────────────

function FileSystemHandle() {
  illegalConstructor();
}
defineToStringTag(FileSystemHandle, 'FileSystemHandle');

defineAttribute(FileSystemHandle.prototype, 'kind', function() { return stateOf(this).kind; });
defineAttribute(FileSystemHandle.prototype, 'name', function() { return stateOf(this).name; });

// Opaque per-entry label from Rust. Two handles on the same file carry
// *different* grant tokens — every `getFileHandle()` mints a fresh one — so
// comparing tokens used to report "not the same entry" for two handles that
// are.
function uniqueIdOf(st) {
  return NAT_FS_UNIQUE ? NAT_FS_UNIQUE(st.pathId, st.token) : null;
}

FileSystemHandle.prototype.isSameEntry = function(other) {
  var self = this;
  return promiseTry(function() {
    var mine = stateOf(self);
    var theirs = (other !== null && typeof other === 'object') ? STATE.get(other) : undefined;
    if (theirs === undefined || theirs.kind !== mine.kind) return false;
    var a = uniqueIdOf(mine);
    var b = uniqueIdOf(theirs);
    return a != null && a === b;
  });
};

FileSystemHandle.prototype.getUniqueId = function() {
  var self = this;
  return promiseTry(function() {
    var id = uniqueIdOf(stateOf(self));
    if (id == null) fsThrow('NotFoundError', 'The entry is no longer reachable');
    return id;
  });
};

function permissionMode(descriptor) {
  var mode = (descriptor && descriptor.mode !== undefined) ? String(descriptor.mode) : 'read';
  if (mode !== 'read' && mode !== 'readwrite') {
    throw new TypeError(
      "Failed to read the 'mode' property: '" + mode + "' is not a valid FileSystemPermissionMode");
  }
  return mode;
}

FileSystemHandle.prototype.queryPermission = function(descriptor) {
  var self = this;
  return promiseTry(function() {
    var st = stateOf(self);
    var mode = permissionMode(descriptor);
    return NAT_FS_PERM ? NAT_FS_PERM(st.pathId, st.token, mode) : 'denied';
  });
};

// There is no permission prompt to raise: a grant is minted by the picker (read
// only) or by the origin's own sandbox (read-write), and nothing in between can
// upgrade one. So a request answers exactly what a query answers rather than
// pretending to ask — an honest 'prompt' beats a 'granted' that the next write
// would contradict.
FileSystemHandle.prototype.requestPermission = function(descriptor) {
  return FileSystemHandle.prototype.queryPermission.call(this, descriptor);
};

FileSystemHandle.prototype.remove = function(options) {
  var self = this;
  return promiseTry(function() {
    var st = stateOf(self);
    var recursive = !!(options && options.recursive);
    if (!NAT_FS_REMOVE) fsThrow('NotSupportedError', 'remove() is unavailable');
    var err = NAT_FS_REMOVE(st.pathId, st.token, recursive);
    if (err) fsThrow(err, 'Cannot remove ' + st.name);
  });
};

// ── FileSystemFileHandle (FS §5) ─────────────────────────────────────────────

function FileSystemFileHandle(brand) {
  if (brand !== BRAND) illegalConstructor();
}
inherit(FileSystemFileHandle, FileSystemHandle, 'FileSystemFileHandle');

function makeFileHandle(name, token, size) {
  var handle = new FileSystemFileHandle(BRAND);
  STATE.set(handle, {
    kind: 'file',
    name: String(name == null ? '' : name),
    token: String(token == null ? '' : token),
    size: Number(size) || 0,
    pathId: '',
  });
  return handle;
}

FileSystemFileHandle.prototype.getFile = function() {
  var self = this;
  return promiseTry(function() {
    var st = stateOf(self);
    // Re-stat rather than trust the size the handle was created with: the file
    // changes under a handle (a `truncate()` through this very API is enough),
    // and a `File` that reads the new contents while reporting the old length
    // is a silent wrong answer.
    if (NAT_FS_SIZE && st.token) {
      var current = NAT_FS_SIZE(st.token);
      if (current >= 0) st.size = current;
    }
    if (FS_INTERNAL) return FS_INTERNAL.makeTokenFile(st.name, st.token, st.size, '', 0);
    // No bridge (the file-input shim failed to install): hand back a plain,
    // grant-less File rather than inventing a second token path.
    return new File([], st.name, { type: '' });
  });
};

FileSystemFileHandle.prototype.createWritable = function(_options) {
  var self = this;
  return promiseTry(function() {
    var st = stateOf(self);
    // BUG-372: a handle from the origin's own sandbox writes straight to the
    // file it names. Sending it through the save picker would ask the user to
    // choose a destination for a file they never see, and then write the bytes
    // somewhere other than where the page's own `getFile()` reads them from.
    var sandboxed = (NAT_WRITE_TOKEN && st.token) ? NAT_WRITE_TOKEN(st.token) : null;
    if (sandboxed != null) return makeWritable(sandboxed);
    var handleId = NAT_SAVE_PICKER ? NAT_SAVE_PICKER(st.name) : null;
    if (handleId == null) {
      fsThrow('NotAllowedError', 'Write permission denied or user cancelled');
    }
    return makeWritable(handleId);
  });
};

// FS §5.1 `move()`: `move(newName)`, `move(destination)` or
// `move(destination, newName)`. The handle keeps representing the entry, so its
// state is repointed at the new location — the old grant names a path that no
// longer exists.
FileSystemFileHandle.prototype.move = function(destination, newName) {
  var self = this;
  return promiseTry(function() {
    var st = stateOf(self);
    if (!NAT_FS_MOVE) fsThrow('NotSupportedError', 'move() is unavailable');
    var destId = '';
    var name = '';
    if (typeof destination === 'string') {
      name = destination;
    } else if (destination !== null && typeof destination === 'object') {
      var dst = STATE.get(destination);
      if (dst === undefined || dst.kind !== 'directory') {
        throw new TypeError('move(): the destination is not a FileSystemDirectoryHandle');
      }
      destId = dst.pathId;
      if (newName !== undefined && newName !== null) name = String(newName);
    } else if (destination !== undefined) {
      throw new TypeError('move(): invalid destination');
    }
    var moved = fsUnwrap(NAT_FS_MOVE(st.token, destId, name), 'Cannot move ' + st.name);
    st.name = String(moved.name);
    st.token = String(moved.token);
    st.size = Number(moved.size) || 0;
  });
};

// FS §7.2 — unbuffered synchronous access to one OPFS file. Defined further
// down, once `fromBase64` and the state map it needs exist.
FileSystemFileHandle.prototype.createSyncAccessHandle = function() {
  var self = this;
  return promiseTry(function() {
    var st = stateOf(self);
    var id = (NAT_SYNC_OPEN && st.token) ? NAT_SYNC_OPEN(st.token) : null;
    if (id == null) {
      fsThrow('NotAllowedError',
        'A sync access handle is only available for a file in the origin private file system');
    }
    return makeSyncAccessHandle(id);
  });
};

// ── FileSystemDirectoryHandle (FS §6) ────────────────────────────────────────

function FileSystemDirectoryHandle(brand) {
  if (brand !== BRAND) illegalConstructor();
}
inherit(FileSystemDirectoryHandle, FileSystemHandle, 'FileSystemDirectoryHandle');

function makeDirHandle(name, pathId) {
  var handle = new FileSystemDirectoryHandle(BRAND);
  STATE.set(handle, {
    kind: 'directory',
    name: String(name == null ? '' : name),
    token: '',
    size: 0,
    pathId: String(pathId == null ? '' : pathId),
  });
  return handle;
}

function handleFromEntry(entry) {
  return entry.kind === 'directory'
    ? makeDirHandle(entry.name, entry.path_id)
    : makeFileHandle(entry.name, entry.token, entry.size);
}

function asyncIterator(next) {
  var iter = { next: next };
  iter[Symbol.asyncIterator] = function() { return this; };
  return iter;
}

FileSystemDirectoryHandle.prototype.entries = function() {
  var st = stateOf(this);
  var raw = [];
  if (NAT_DIR_ENTRIES) {
    try { raw = JSON.parse(NAT_DIR_ENTRIES(st.pathId)) || []; } catch (e) { raw = []; }
  }
  var idx = 0;
  return asyncIterator(function() {
    if (idx >= raw.length) return Promise.resolve({ done: true, value: undefined });
    var entry = raw[idx++];
    return Promise.resolve({ done: false, value: [entry.name, handleFromEntry(entry)] });
  });
};

FileSystemDirectoryHandle.prototype.values = function() {
  var it = this.entries();
  return asyncIterator(function() {
    return it.next().then(function(r) {
      return r.done ? r : { done: false, value: r.value[1] };
    });
  });
};

FileSystemDirectoryHandle.prototype.keys = function() {
  var it = this.entries();
  return asyncIterator(function() {
    return it.next().then(function(r) {
      return r.done ? r : { done: false, value: r.value[0] };
    });
  });
};

// `async iterable<USVString, FileSystemHandle>` — `for await (const [name, h] of
// dir)` iterates the directory itself, not only `dir.entries()`.
FileSystemDirectoryHandle.prototype[Symbol.asyncIterator] =
  FileSystemDirectoryHandle.prototype.entries;

FileSystemDirectoryHandle.prototype.getFileHandle = function(name, opts) {
  var self = this;
  return promiseTry(function() {
    var st = stateOf(self);
    var create = !!(opts && opts.create);
    var entryName = String(name);
    var raw = NAT_DIR_GET_FILE ? NAT_DIR_GET_FILE(st.pathId, entryName, create) : null;
    var p = fsUnwrap(raw, 'Cannot open file: ' + entryName);
    return makeFileHandle(p.name, p.token, p.size);
  });
};

FileSystemDirectoryHandle.prototype.getDirectoryHandle = function(name, opts) {
  var self = this;
  return promiseTry(function() {
    var st = stateOf(self);
    var create = !!(opts && opts.create);
    var entryName = String(name);
    var raw = NAT_DIR_GET_SUB ? NAT_DIR_GET_SUB(st.pathId, entryName, create) : null;
    var p = fsUnwrap(raw, 'Cannot open directory: ' + entryName);
    return makeDirHandle(p.name, p.path_id);
  });
};

FileSystemDirectoryHandle.prototype.removeEntry = function(name, opts) {
  var self = this;
  return promiseTry(function() {
    var st = stateOf(self);
    var recursive = !!(opts && opts.recursive);
    var entryName = String(name);
    // BUG-372: this used to resolve without removing anything, so a caller
    // could not tell a deletion from a no-op. It now either removes the entry
    // or says which rule stopped it.
    if (!NAT_DIR_REMOVE) {
      fsThrow('NotSupportedError', 'removeEntry is unavailable');
    }
    var err = NAT_DIR_REMOVE(st.pathId, entryName, recursive);
    if (err) fsThrow(err, 'Cannot remove entry: ' + entryName);
  });
};

// FS §6.2: `[]` for the handle itself, the path segments for a descendant,
// `null` for anything else.
FileSystemDirectoryHandle.prototype.resolve = function(possibleDescendant) {
  var self = this;
  return promiseTry(function() {
    var st = stateOf(self);
    var child = (possibleDescendant !== null && typeof possibleDescendant === 'object')
      ? STATE.get(possibleDescendant) : undefined;
    if (child === undefined || !NAT_FS_RESOLVE) return null;
    var raw = NAT_FS_RESOLVE(st.pathId, child.pathId, child.token);
    return raw == null ? null : JSON.parse(raw);
  });
};

// ── FileSystemWritableFileStream (FS §7) ─────────────────────────────────────

var WritableStreamBase = (typeof globalThis.WritableStream === 'function')
  ? globalThis.WritableStream : null;

function FileSystemWritableFileStream(brand) {
  if (brand !== BRAND) illegalConstructor();
}
if (WritableStreamBase) {
  inherit(FileSystemWritableFileStream, WritableStreamBase, 'FileSystemWritableFileStream');
} else {
  // No streams in this runtime (bare unit-test harness): the class still works,
  // it just has no `getWriter`/`locked`/`abort` to inherit.
  defineToStringTag(FileSystemWritableFileStream, 'FileSystemWritableFileStream');
}

var B64_CHARS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

// File bytes cross into Rust as base64: a JS string is UTF-16 and cannot carry
// arbitrary bytes through the string boundary intact. Written out by hand rather
// than through `btoa` so the shim keeps working in a runtime that has no
// `btoa` (the unit-test harness).
function toBase64(bytes) {
  var out = '';
  for (var i = 0; i < bytes.length; i += 3) {
    var b0 = bytes[i];
    var has1 = (i + 1) < bytes.length;
    var has2 = (i + 2) < bytes.length;
    var b1 = has1 ? bytes[i + 1] : 0;
    var b2 = has2 ? bytes[i + 2] : 0;
    var n = (b0 << 16) | (b1 << 8) | b2;
    out += B64_CHARS[(n >> 18) & 63];
    out += B64_CHARS[(n >> 12) & 63];
    out += has1 ? B64_CHARS[(n >> 6) & 63] : '=';
    out += has2 ? B64_CHARS[n & 63] : '=';
  }
  return out;
}

// FS §7.1 accepts `BufferSource`, `Blob` or `USVString`. Everything but a string
// is raw bytes, and a `Blob` only yields them asynchronously — which is why the
// old `String(data)` wrote a `Blob` out as the literal `[object Blob]`.
function bytesOf(data) {
  if (data === null || data === undefined) return Promise.resolve(new Uint8Array(0));
  if (typeof Blob === 'function' && data instanceof Blob) {
    return data.arrayBuffer().then(function(buf) { return new Uint8Array(buf); });
  }
  if (data instanceof ArrayBuffer) return Promise.resolve(new Uint8Array(data.slice(0)));
  if (ArrayBuffer.isView(data)) {
    return Promise.resolve(new Uint8Array(
      data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength)));
  }
  return Promise.resolve(new TextEncoder().encode(String(data)));
}

function toOffset(value, what) {
  var n = Number(value);
  if (!isFinite(n) || n < 0) {
    throw new TypeError('write(): ' + what + ' must be a non-negative number');
  }
  return Math.floor(n);
}

function writeStateOf(stream) {
  var st = (stream !== null && typeof stream === 'object') ? WRITE_STATE.get(stream) : undefined;
  if (st === undefined) throw new TypeError('Illegal invocation');
  return st;
}

// True for a `WriteParams` dictionary rather than for the data itself.
function isWriteParams(data) {
  return data !== null && typeof data === 'object'
    && !(data instanceof ArrayBuffer)
    && !ArrayBuffer.isView(data)
    && !(typeof Blob === 'function' && data instanceof Blob)
    && data.type !== undefined;
}

// Commands run one after another in call order, even when the caller does not
// await them: a stream is a queue, and `w.write('a'); w.truncate(1); w.close();`
// must not let the close commit before the write has landed. Each operation
// resolves with its own outcome; the queue itself absorbs failures so one
// rejected write does not strand every command behind it.
function enqueue(st, operation) {
  var result = st.queue.then(operation);
  st.queue = result.then(function() {}, function() {});
  return result;
}

// The one place implementing FS §7.1's write algorithm — `write()`, `seek()`,
// `truncate()` and the underlying sink's `write()` all route through it, so a
// `{type:'seek'}` command and a `seek()` call cannot drift apart.
function writeCommand(st, data) {
  return enqueue(st, function() {
    if (st.closed) throw new TypeError('FileSystemWritableFileStream is closed');
    var type = 'write';
    var payload = data;
    var position;
    var size;
    if (isWriteParams(data)) {
      type = String(data.type);
      payload = data.data;
      position = data.position;
      size = data.size;
    }
    if (type === 'seek') {
      if (position === undefined || position === null) {
        fsThrow('SyntaxError', "write(): a 'seek' command requires a position");
      }
      st.position = toOffset(position, 'position');
      return undefined;
    }
    if (type === 'truncate') {
      if (size === undefined || size === null) {
        fsThrow('SyntaxError', "write(): a 'truncate' command requires a size");
      }
      var end = toOffset(size, 'size');
      if (!NAT_WRITE_TRUNC || !NAT_WRITE_TRUNC(st.id, end)) {
        fsThrow('NotAllowedError', 'The write grant is no longer valid');
      }
      if (st.position > end) st.position = end;
      return undefined;
    }
    if (type !== 'write') {
      throw new TypeError("write(): '" + type + "' is not a valid write command type");
    }
    if (position !== undefined && position !== null) {
      st.position = toOffset(position, 'position');
    }
    return bytesOf(payload).then(function(bytes) {
      if (!NAT_WRITE_BYTES || !NAT_WRITE_BYTES(st.id, st.position, toBase64(bytes))) {
        fsThrow('NotAllowedError', 'The write grant is no longer valid');
      }
      st.position += bytes.length;
    });
  });
}

function commitWrite(st) {
  if (st.closed) return undefined;
  st.closed = true;
  if (!NAT_WRITE_CLOSE || !NAT_WRITE_CLOSE(st.id)) {
    fsThrow('NotAllowedError', 'The write grant is no longer valid');
  }
  return undefined;
}

function makeWritable(handleId) {
  var stream = new FileSystemWritableFileStream(BRAND);
  var st = {
    id: String(handleId == null ? '' : handleId),
    position: 0,
    closed: false,
    queue: Promise.resolve(),
  };
  WRITE_STATE.set(stream, st);
  if (WritableStreamBase) {
    // The stream really is a `WritableStream`: its sink is the FS write
    // algorithm, so `getWriter().write(chunk)` and `stream.write(chunk)` commit
    // the same bytes through the same path.
    WritableStreamBase.call(stream, {
      write: function(chunk) { return writeCommand(st, chunk); },
      close: function() { return enqueue(st, function() { return commitWrite(st); }); },
      abort: function() { st.closed = true; },
    });
  }
  return stream;
}

FileSystemWritableFileStream.prototype.write = function(data) {
  var self = this;
  var st;
  try {
    st = writeStateOf(self);
    if (self.locked) throw new TypeError('FileSystemWritableFileStream is locked');
  } catch (e) {
    return Promise.reject(e);
  }
  return writeCommand(st, data);
};

// Every entry point below joins the queue *synchronously*, so the order the
// commands run in is the order the page issued them — deferring the `enqueue`
// call by even one microtask would let a later command overtake an earlier one.
FileSystemWritableFileStream.prototype.seek = function(position) {
  var st;
  try { st = writeStateOf(this); } catch (e) { return Promise.reject(e); }
  return writeCommand(st, { type: 'seek', position: position });
};

FileSystemWritableFileStream.prototype.truncate = function(size) {
  var st;
  try { st = writeStateOf(this); } catch (e) { return Promise.reject(e); }
  return writeCommand(st, { type: 'truncate', size: size });
};

FileSystemWritableFileStream.prototype.close = function() {
  var st;
  try { st = writeStateOf(this); } catch (e) { return Promise.reject(e); }
  // Go through the base class where there is one, so the stream ends up in the
  // 'closed' state its own `locked`/`getWriter` contract talks about; the base
  // reaches the same commit through the sink.
  if (WritableStreamBase && typeof this._ws_state === 'string') {
    return WritableStreamBase.prototype.close.call(this);
  }
  return enqueue(st, function() { return commitWrite(st); });
};

// ── FileSystemSyncAccessHandle (FS §7.2) ─────────────────────────────────────

function FileSystemSyncAccessHandle(brand) {
  if (brand !== BRAND) illegalConstructor();
}
defineToStringTag(FileSystemSyncAccessHandle, 'FileSystemSyncAccessHandle');

// stream -> {id, closed}
var SYNC_STATE = new WeakMap();

function fromBase64(text) {
  var lookup = {};
  for (var c = 0; c < B64_CHARS.length; c++) lookup[B64_CHARS[c]] = c;
  var bytes = [];
  var buffer = 0;
  var bits = 0;
  for (var i = 0; i < text.length; i++) {
    var value = lookup[text[i]];
    if (value === undefined) continue;
    buffer = (buffer << 6) | value;
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      bytes.push((buffer >> bits) & 255);
    }
  }
  return new Uint8Array(bytes);
}

function makeSyncAccessHandle(id) {
  var handle = new FileSystemSyncAccessHandle(BRAND);
  SYNC_STATE.set(handle, { id: String(id), closed: false });
  return handle;
}

// Every member here is synchronous by design — that is the whole difference
// from `FileSystemWritableFileStream`, and why the natives behind it act on an
// open OS file rather than on a buffer flushed at close.
function syncStateOf(handle) {
  var st = (handle !== null && typeof handle === 'object') ? SYNC_STATE.get(handle) : undefined;
  if (st === undefined) throw new TypeError('Illegal invocation');
  if (st.closed) fsThrow('InvalidStateError', 'The sync access handle is closed');
  return st;
}

function syncOffset(options) {
  var at = (options && options.at !== undefined && options.at !== null) ? Number(options.at) : 0;
  if (!isFinite(at) || at < 0) throw new TypeError("Invalid 'at': not a file position");
  return Math.floor(at);
}

function viewBytes(buffer) {
  if (buffer instanceof ArrayBuffer) return new Uint8Array(buffer);
  if (ArrayBuffer.isView(buffer)) {
    return new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
  }
  throw new TypeError('The provided value is not of type BufferSource');
}

FileSystemSyncAccessHandle.prototype.read = function(buffer, options) {
  var st = syncStateOf(this);
  var target = viewBytes(buffer);
  var at = syncOffset(options);
  if (!NAT_SYNC_READ) fsThrow('InvalidStateError', 'read() is unavailable');
  var bytes = fromBase64(NAT_SYNC_READ(st.id, at, target.length));
  target.set(bytes.subarray(0, target.length));
  return bytes.length;
};

FileSystemSyncAccessHandle.prototype.write = function(buffer, options) {
  var st = syncStateOf(this);
  var source = viewBytes(buffer);
  var at = syncOffset(options);
  if (!NAT_SYNC_WRITE) fsThrow('InvalidStateError', 'write() is unavailable');
  var written = NAT_SYNC_WRITE(st.id, at, toBase64(source));
  if (written < 0) fsThrow('InvalidStateError', 'The file could not be written');
  return written;
};

FileSystemSyncAccessHandle.prototype.truncate = function(newSize) {
  var st = syncStateOf(this);
  var size = Number(newSize);
  if (!isFinite(size) || size < 0) throw new TypeError('truncate(): invalid size');
  if (!NAT_SYNC_TRUNC || !NAT_SYNC_TRUNC(st.id, Math.floor(size))) {
    fsThrow('InvalidStateError', 'The file could not be resized');
  }
};

FileSystemSyncAccessHandle.prototype.getSize = function() {
  var st = syncStateOf(this);
  var size = NAT_SYNC_SIZE ? NAT_SYNC_SIZE(st.id) : -1;
  if (size < 0) fsThrow('InvalidStateError', 'The file size is not available');
  return size;
};

FileSystemSyncAccessHandle.prototype.flush = function() {
  var st = syncStateOf(this);
  if (!NAT_SYNC_FLUSH || !NAT_SYNC_FLUSH(st.id)) {
    fsThrow('InvalidStateError', 'The file could not be flushed');
  }
};

// Idempotent per FS §7.2: closing a closed handle is not an error, unlike every
// other member, which throws once the handle is closed.
FileSystemSyncAccessHandle.prototype.close = function() {
  var st = (this !== null && typeof this === 'object') ? SYNC_STATE.get(this) : undefined;
  if (st === undefined) throw new TypeError('Illegal invocation');
  if (st.closed) return;
  st.closed = true;
  if (NAT_SYNC_CLOSE) NAT_SYNC_CLOSE(st.id);
};

// ── Picker option validation (FS §8.1) ───────────────────────────────────────

var WELL_KNOWN_DIRS = ['desktop', 'documents', 'downloads', 'music', 'pictures', 'videos'];
var MIME_TOKEN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
var PICKER_ID = /^[A-Za-z0-9_-]*$/;
var EXTENSION = /^\.[a-z0-9+\-.]+$/;

// A MIME type with no parameters: exactly one '/', both halves non-empty HTTP
// tokens. `text/plain;charset=utf-8` and `image` are both rejected.
function validMimeType(mime) {
  var parts = String(mime).split('/');
  return parts.length === 2 && MIME_TOKEN.test(parts[0]) && MIME_TOKEN.test(parts[1]);
}

function validExtension(ext) {
  if (typeof ext !== 'string') return false;
  if (ext.length > 16) return false;
  if (ext.charAt(ext.length - 1) === '.') return false;
  return EXTENSION.test(ext);
}

// The pickers used to take an options dictionary and ignore it whole — the
// parameter was even named `_options`. Every rule below is one the spec states
// as a `TypeError`, so accepting the call silently was the difference between
// "this build cannot filter by type" and "this build says it filtered".
function validatePickerOptions(options) {
  if (options === undefined || options === null) return;
  if (typeof options !== 'object') {
    throw new TypeError('The provided value is not of type FilePickerOptions');
  }
  if (options.id !== undefined && options.id !== null) {
    var id = String(options.id);
    if (id.length > 32 || !PICKER_ID.test(id)) {
      throw new TypeError("Invalid 'id': at most 32 characters of [A-Za-z0-9_-]");
    }
  }
  if (options.startIn !== undefined && options.startIn !== null) {
    var startIn = options.startIn;
    var isHandle = (typeof startIn === 'object') && STATE.has(startIn);
    if (!isHandle && WELL_KNOWN_DIRS.indexOf(String(startIn)) < 0) {
      throw new TypeError("Invalid 'startIn': not a well-known directory or a FileSystemHandle");
    }
  }
  var types = options.types;
  if (types !== undefined && types !== null) {
    if (typeof types.length !== 'number') {
      throw new TypeError("Invalid 'types': not a sequence");
    }
    for (var i = 0; i < types.length; i++) {
      var accept = types[i] ? types[i].accept : undefined;
      if (accept === null || typeof accept !== 'object') {
        throw new TypeError("Invalid 'types': each entry needs an 'accept' dictionary");
      }
      var mimes = Object.keys(accept);
      for (var m = 0; m < mimes.length; m++) {
        if (!validMimeType(mimes[m])) {
          throw new TypeError("Invalid 'types': '" + mimes[m] + "' is not a valid MIME type");
        }
        var exts = accept[mimes[m]];
        if (typeof exts === 'string') exts = [exts];
        if (exts === null || typeof exts !== 'object' || typeof exts.length !== 'number') {
          throw new TypeError("Invalid 'types': the extension list is not a sequence");
        }
        for (var e = 0; e < exts.length; e++) {
          if (!validExtension(exts[e])) {
            throw new TypeError("Invalid 'types': '" + exts[e] + "' is not a valid extension");
          }
        }
      }
    }
  }
  if (options.excludeAcceptAllOption && (types === undefined || types === null || types.length === 0)) {
    throw new TypeError("Invalid 'types': no accepted file types");
  }
}

// FS §8.1 requires transient activation, so a script cannot pop a file dialog
// on its own. `navigator.userActivation` is the engine's own answer to that
// question — the pickers used not to consult it at all.
function requireUserActivation(what) {
  var activation = (typeof navigator !== 'undefined') ? navigator.userActivation : undefined;
  if (activation && activation.isActive === false) {
    fsThrow('SecurityError', 'Must be handling a user gesture to show ' + what);
  }
}

// ── Picker globals (Promise-returning per FS §8.1) ───────────────────────────

function showOpenFilePicker(options) {
  return promiseTry(function() {
    validatePickerOptions(options);
    requireUserActivation('a file picker');
    var info = NAT_OPEN_PICKER ? NAT_OPEN_PICKER() : null;
    if (info == null) {
      fsThrow('AbortError', 'The user aborted a request.');
    }
    var p = JSON.parse(info);
    return [makeFileHandle(p.name, p.token, p.size)];
  });
}

function showSaveFilePicker(options) {
  return promiseTry(function() {
    validatePickerOptions(options);
    requireUserActivation('a file picker');
    var suggested = (options && options.suggestedName) ? String(options.suggestedName) : 'file.txt';
    var handleId = NAT_SAVE_PICKER ? NAT_SAVE_PICKER(suggested) : null;
    if (handleId == null) {
      fsThrow('AbortError', 'The user aborted a request.');
    }
    // The save dialog hands back a write grant, not a read token, so the handle
    // it produces writes to the confirmed destination and reads nothing.
    var handle = makeFileHandle(suggested, '', 0);
    Object.defineProperty(handle, 'createWritable', {
      value: function() { return Promise.resolve(makeWritable(handleId)); },
      writable: true, enumerable: false, configurable: true,
    });
    return handle;
  });
}

function showDirectoryPicker(options) {
  return promiseTry(function() {
    validatePickerOptions(options);
    requireUserActivation('a directory picker');
    var info = NAT_DIR_PICKER ? NAT_DIR_PICKER() : null;
    if (info == null) {
      fsThrow('AbortError', 'The user aborted a request.');
    }
    var p = JSON.parse(info);
    return makeDirHandle(p.name, p.path_id);
  });
}

// ── [Serializable] (FS §4: all three handles survive structuredClone) ────────

if (globalThis.__lumen_platform_cloners) {
  globalThis.__lumen_platform_cloners.register(
    function(value) { return STATE.has(value); },
    function(value) {
      var st = STATE.get(value);
      return st.kind === 'directory'
        ? makeDirHandle(st.name, st.pathId)
        : makeFileHandle(st.name, st.token, st.size);
    });
}

// ── Internal bridge for navigator.storage.getDirectory() ─────────────────────

// `storage_manager.rs` used to build the OPFS root by calling
// `new window.FileSystemDirectoryHandle(name, pathId)`, which is exactly the
// public constructor BUG-374 removes. It captures this bridge at its own eval
// time and deletes the global immediately, the same way it already treats
// `_lumen_storage_get_directory`.
Object.defineProperty(globalThis, '__lumen_fsa_internal', {
  value: Object.freeze({ makeDirectoryHandle: makeDirHandle }),
  enumerable: false, writable: false, configurable: true,
});

// ── Expose on window if available ────────────────────────────────────────────

if (typeof window !== 'undefined') {
  window.FileSystemHandle              = FileSystemHandle;
  window.FileSystemFileHandle          = FileSystemFileHandle;
  window.FileSystemDirectoryHandle     = FileSystemDirectoryHandle;
  window.FileSystemWritableFileStream  = FileSystemWritableFileStream;
  window.FileSystemSyncAccessHandle    = FileSystemSyncAccessHandle;
  window.showOpenFilePicker            = showOpenFilePicker;
  window.showSaveFilePicker            = showSaveFilePicker;
  window.showDirectoryPicker           = showDirectoryPicker;
}

})();
"#;

/// V8 test coverage for the File System Access API shim (the rquickjs twin
/// was removed in S12b-B20; this module ports its 33 tests to V8 verbatim).
/// The registries/JSON helpers exercised here are themselves gated on
/// `v8-backend` (nothing else in the crate reaches them once the rquickjs
/// installer is gone), so their unit tests live here too rather than in a
/// separate engine-agnostic module.
#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Origin every V8 test here installs under; registry entries must be
    /// allocated with the same string or the natives reject them (BUG-371).
    const TEST_ORIGIN: &str = "https://fsal.test";

    #[test]
    fn writable_write_accumulates() {
        let tmp = std::env::temp_dir().join("lumen_fsal_write_test.txt");
        let handle_id = super::write_reg().lock().unwrap().allocate(tmp.clone(), TEST_ORIGIN);
        super::write_reg().lock().unwrap().write_bytes(&handle_id, TEST_ORIGIN, 0, b"hello");
        super::write_reg().lock().unwrap().write_bytes(&handle_id, TEST_ORIGIN, 5, b" world");
        super::write_reg().lock().unwrap().close(&handle_id, TEST_ORIGIN);
        let content = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert_eq!(content, "hello world");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn writable_close_writes_file() {
        let tmp = std::env::temp_dir().join("lumen_fsal_close_test.txt");
        let handle_id = super::write_reg().lock().unwrap().allocate(tmp.clone(), TEST_ORIGIN);
        super::write_reg().lock().unwrap().write_bytes(&handle_id, TEST_ORIGIN, 0, b"lumen test content");
        let ok = super::write_reg().lock().unwrap().close(&handle_id, TEST_ORIGIN);
        assert!(ok);
        let content = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert_eq!(content, "lumen test content");
        let _ = std::fs::remove_file(&tmp);
    }

    /// BUG-371 scenario 3: a save handle another page opened must not be
    /// writable — that let any page substitute the bytes the user agreed to
    /// save, at the path the user confirmed.
    #[test]
    fn writable_rejects_foreign_origin() {
        let tmp = std::env::temp_dir().join("lumen_fsal_foreign_write_test.txt");
        let _ = std::fs::write(&tmp, b"honest");
        let handle_id = super::write_reg()
            .lock()
            .unwrap()
            .allocate(tmp.clone(), "https://honest.example");
        assert!(
            !super::write_reg().lock().unwrap().write_bytes(&handle_id, "https://evil.example", 0, b"pwn"),
            "append from a foreign origin must be refused"
        );
        assert!(
            !super::write_reg().lock().unwrap().close(&handle_id, "https://evil.example"),
            "close from a foreign origin must be refused"
        );
        assert_eq!(std::fs::read_to_string(&tmp).unwrap_or_default(), "honest");
        // The legitimate owner still owns the handle after the failed attempts.
        assert!(super::write_reg().lock().unwrap().close(&handle_id, "https://honest.example"));
        let _ = std::fs::remove_file(&tmp);
    }

    /// BUG-371 scenario 2: a directory grant is not reachable from another
    /// origin, so `_lumen_dir_get_file` cannot mint read tokens for it.
    #[test]
    fn dir_grant_rejects_foreign_origin() {
        let tmp = std::env::temp_dir();
        let id = super::dir_reg().lock().unwrap().allocate(tmp, "https://honest.example", false);
        assert!(super::dir_reg().lock().unwrap().get(&id, "https://honest.example").is_some());
        assert!(super::dir_reg().lock().unwrap().get(&id, "https://evil.example").is_none());
    }

    /// BUG-371: ids were `1, 2, 3, …`, so `_lumen_dir_entries(1)` listed
    /// whatever directory the user had last granted.
    #[test]
    fn registry_ids_are_unguessable() {
        let tmp = std::env::temp_dir();
        let id = super::dir_reg().lock().unwrap().allocate(tmp.clone(), TEST_ORIGIN, false);
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        for n in 1..=64u32 {
            assert!(
                super::dir_reg().lock().unwrap().get(&n.to_string(), TEST_ORIGIN).is_none(),
                "small integer {n} must never be a valid directory id"
            );
        }
    }

    /// BUG-371 point 3: a reload revokes what the previous document held.
    #[test]
    fn revoke_origin_drops_only_that_origins_grants() {
        let tmp = std::env::temp_dir();
        let keep = super::dir_reg().lock().unwrap().allocate(tmp.clone(), "https://keep.example", false);
        let drop = super::dir_reg().lock().unwrap().allocate(tmp, "https://drop.example", false);
        super::dir_reg().lock().unwrap().revoke_origin("https://drop.example");
        assert!(super::dir_reg().lock().unwrap().get(&keep, "https://keep.example").is_some());
        assert!(super::dir_reg().lock().unwrap().get(&drop, "https://drop.example").is_none());
    }

    #[test]
    fn json_escape_quotes() {
        let s = r#"say "hello""#;
        let e = super::json_escape(s);
        // Every `"` must be preceded by `\`.
        assert_eq!(e, r#"say \"hello\""#);
    }

    #[test]
    fn json_escape_backslash() {
        let s = r"path\to\file";
        let e = super::json_escape(s);
        assert!(e.contains("\\\\"));
    }

    #[test]
    fn file_entry_json_for_existing_file() {
        use std::io::Write;
        let tmp = std::env::temp_dir().join("lumen_fsal_fej_test.txt");
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            f.write_all(b"data").unwrap();
        }
        let json_opt = super::file_entry_json(&tmp, TEST_ORIGIN, false);
        assert!(json_opt.is_some());
        let json = json_opt.unwrap();
        assert!(json.contains("\"name\""));
        // BUG-371: the token is a quoted 32-hex-char string, not a bare number.
        assert!(json.contains("\"token\":\""));
        assert!(json.contains("\"size\":4"));
        let _ = std::fs::remove_file(&tmp);
    }

    // Minimal DOM stubs `install_file_input_bindings_v8`'s shim needs — mirrors
    // `file_input::tests::STUBS` (private to that module, so duplicated here).
    //
    // `TextEncoder` is here for the same reason: on a page it arrives with
    // `dom.rs`'s shim, which this harness does not evaluate, and the writable
    // stream encodes every string chunk through it.
    const STUBS: &str = r#"
        var window = globalThis;
        function Blob(blobParts, options) {}
        function _lumen_set_attr(nid, name, val) {}
        function _lumen_get_attr(nid, name) { return undefined; }
        function _lumen_dispatch_bubble(nid, type) {}
        function _lumen_make_element(nid) { return {__nid__: nid}; }
        window._lumen_make_element = _lumen_make_element;
        function TextEncoder() {}
        TextEncoder.prototype.encode = function(str) {
            var out = [];
            for (var i = 0; i < str.length; i++) {
                var c = str.charCodeAt(i);
                if (c >= 0xD800 && c <= 0xDBFF && (i + 1) < str.length) {
                    var lo = str.charCodeAt(i + 1);
                    if (lo >= 0xDC00 && lo <= 0xDFFF) {
                        c = 0x10000 + ((c - 0xD800) << 10) + (lo - 0xDC00);
                        i++;
                    }
                }
                if (c < 0x80) { out.push(c); }
                else if (c < 0x800) { out.push(0xC0 | (c >> 6), 0x80 | (c & 63)); }
                else if (c < 0x10000) {
                    out.push(0xE0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
                } else {
                    out.push(0xF0 | (c >> 18), 0x80 | ((c >> 12) & 63),
                             0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
                }
            }
            return new Uint8Array(out);
        };
    "#;

    /// Two handles every shape test needs, resolved from the mocked pickers.
    ///
    /// BUG-374 removed the public constructors, so a test cannot say
    /// `new FileSystemFileHandle('a.txt', '', 0)` any more — which is the point:
    /// the only way to a handle is the way a page has, through an API that
    /// hands one out.
    const RESOLVE_HANDLES: &str = r#"
        var __fh = null, __dh = null;
        showOpenFilePicker().then(function(list) { __fh = list[0]; });
        showDirectoryPicker().then(function(dir) { __dh = dir; });
    "#;

    /// Private factory the OPFS root is built through (`__lumen_fsa_internal`,
    /// BUG-374) — the only way for a test to turn a directory grant it allocated
    /// itself into a handle, now that the constructor is gone. `storage_manager`
    /// deletes this global on a real page; nothing installs that shim here.
    const ROOT: &str = "__lumen_fsa_internal.makeDirectoryHandle";

    fn with_fsa() -> V8JsRuntime {
        with_fsa_for(TEST_ORIGIN)
    }

    /// Same harness under a caller-chosen origin.
    ///
    /// Tests that allocate their own grants need one: installing revokes every
    /// grant of the origin it installs under (BUG-371 point 3), and the test
    /// binary runs these in parallel over one process-wide registry — sharing
    /// `TEST_ORIGIN` would let one test revoke another's directory mid-run.
    fn with_fsa_for(origin: &str) -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(STUBS).unwrap();
        // On a page `install_dom` runs first and brings `DOMException` with it;
        // here nothing does, so every `throw new DOMException(...)` in the shim
        // used to become a `ReferenceError` and no test could look at what the
        // module actually rejects with (BUG-373).
        rt.eval(crate::v8_runtime::DOM_EXCEPTION_POLYFILL).unwrap();
        crate::file_input::install_file_input_bindings_v8(&rt, origin).unwrap();
        super::install_filesystem_access_v8(&rt, origin).unwrap();
        // The real `_lumen_show_*_picker` natives spawn a blocking native OS dialog
        // (PowerShell Windows.Forms on Windows). `showXPicker()`'s `Promise.resolve().then(...)`
        // callback runs as a microtask that V8 drains at the end of THIS `eval()` call (unlike
        // the removed rquickjs harness, which never auto-ran pending jobs — S12b-B8 finding) —
        // so calling `showOpenFilePicker()` etc. here would pop a real dialog and hang the test.
        // Override with non-blocking mocks that resolve the promise instead.
        //
        // BUG-371: since the shim now *captures* the natives at eval time
        // (they are deleted from the global object on a real page), overwriting
        // the globals alone no longer reaches it — the shim has to be
        // re-evaluated afterwards so its IIFE picks the mocks up.
        rt.eval(
            r#"
            globalThis._lumen_show_open_file_picker =
              function() { return '{"name":"mock.txt","token":"","size":0}'; };
            globalThis._lumen_show_save_file_picker = function() { return 'mock-write-id'; };
            globalThis._lumen_show_directory_picker =
              function() { return '{"name":"mockdir","path_id":"mock-dir-id"}'; };
            "#,
        )
        .unwrap();
        rt.eval(super::FSAL_SHIM).unwrap();
        rt.eval(RESOLVE_HANDLES).unwrap();
        rt
    }

    fn bool_eval(rt: &V8JsRuntime, expr: &str) -> bool {
        matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
    }

    #[test]
    fn fsfh_constructor_exists() {
        let rt = with_fsa();
        assert!(bool_eval(&rt, "typeof window.FileSystemFileHandle === 'function'"));
    }

    #[test]
    fn fsdh_constructor_exists() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof window.FileSystemDirectoryHandle === 'function'"
        ));
    }

    #[test]
    fn fsws_constructor_exists() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof window.FileSystemWritableFileStream === 'function'"
        ));
    }

    #[test]
    fn show_open_file_picker_is_function() {
        let rt = with_fsa();
        assert!(bool_eval(&rt, "typeof window.showOpenFilePicker === 'function'"));
    }

    #[test]
    fn show_save_file_picker_is_function() {
        let rt = with_fsa();
        assert!(bool_eval(&rt, "typeof window.showSaveFilePicker === 'function'"));
    }

    #[test]
    fn show_directory_picker_is_function() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof window.showDirectoryPicker === 'function'"
        ));
    }

    // ── BUG-374: the WebIDL shape of the interface hierarchy ─────────────────

    /// Point 1: `FileSystemHandle` is the base interface, exposed on the global,
    /// and both handle classes inherit from it — instance *and* interface object.
    /// Without it `if (window.FileSystemHandle)` feature-detects concluded the
    /// API was missing and the prototype chain ended at `Object`.
    #[test]
    fn handle_hierarchy_is_webidl() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof window.FileSystemHandle === 'function' && \
             __fh instanceof FileSystemHandle && __fh instanceof FileSystemFileHandle && \
             __dh instanceof FileSystemHandle && __dh instanceof FileSystemDirectoryHandle && \
             !(__fh instanceof FileSystemDirectoryHandle) && \
             Object.getPrototypeOf(FileSystemFileHandle.prototype) === FileSystemHandle.prototype && \
             Object.getPrototypeOf(FileSystemDirectoryHandle.prototype) === FileSystemHandle.prototype && \
             Object.getPrototypeOf(FileSystemFileHandle) === FileSystemHandle && \
             __fh.constructor === FileSystemFileHandle"
        ));
    }

    /// Point 2: none of the four interfaces declares a constructor. They used to
    /// be publicly constructible *and* took the internal grant id as an
    /// argument, so forging a handle was one line.
    #[test]
    fn constructors_are_illegal() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "function threw(f) { try { f(); return false; } catch (e) { return e instanceof TypeError; } } \
             threw(function() { new FileSystemHandle(); }) && \
             threw(function() { new FileSystemFileHandle('a.txt', 'tok', 0); }) && \
             threw(function() { new FileSystemDirectoryHandle('d', 'pid'); }) && \
             threw(function() { new FileSystemWritableFileStream('wid'); })"
        ));
    }

    /// Point 3: `kind`/`name` are `readonly attribute`s — accessors on the
    /// prototype, not own data properties. `fileHandle.kind = 'directory'` used
    /// to be accepted, after which the handle misreported its own type.
    #[test]
    fn kind_and_name_are_readonly_prototype_accessors() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "var d = Object.getOwnPropertyDescriptor(FileSystemHandle.prototype, 'kind'); \
             __fh.kind = 'directory'; __fh.name = 'other.txt'; \
             typeof d.get === 'function' && d.set === undefined && \
             Object.getOwnPropertyNames(__fh).length === 0 && \
             Object.getOwnPropertyNames(__dh).length === 0 && \
             __fh.kind === 'file' && __fh.name === 'mock.txt' && __dh.kind === 'directory'"
        ));
    }

    /// Point 3, second half: the grant ids stay in private slots (BUG-371).
    #[test]
    fn handle_internals_are_not_web_visible() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "__fh._token === undefined && __dh._pathId === undefined && \
             JSON.stringify(__fh) === '{}' && JSON.stringify(__dh) === '{}'"
        ));
    }

    /// Point 4.
    #[test]
    fn handles_have_a_to_string_tag() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "Object.prototype.toString.call(__fh) === '[object FileSystemFileHandle]' && \
             Object.prototype.toString.call(__dh) === '[object FileSystemDirectoryHandle]'"
        ));
    }

    /// Point 5: the base interface's members live on the base prototype, and
    /// `queryPermission`/`requestPermission`/`remove`/`getUniqueId` exist at all.
    #[test]
    fn base_members_are_on_the_base_prototype() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "var p = FileSystemHandle.prototype; \
             ['isSameEntry','queryPermission','requestPermission','remove','getUniqueId'] \
               .every(function(m) { return typeof p[m] === 'function' && \
                                          !Object.prototype.hasOwnProperty.call( \
                                             FileSystemFileHandle.prototype, m); }) && \
             typeof FileSystemFileHandle.prototype.getFile === 'function' && \
             typeof FileSystemFileHandle.prototype.createWritable === 'function' && \
             typeof FileSystemFileHandle.prototype.move === 'function' && \
             ['entries','values','keys','getFileHandle','getDirectoryHandle','removeEntry','resolve'] \
               .every(function(m) { \
                 return typeof FileSystemDirectoryHandle.prototype[m] === 'function'; })"
        ));
    }

    /// Point 6: `async iterable<USVString, FileSystemHandle>` — the handle
    /// itself is iterable, not only the object `entries()` returns.
    #[test]
    fn directory_handle_is_async_iterable() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof __dh[Symbol.asyncIterator] === 'function' && \
             typeof __dh.entries()[Symbol.asyncIterator] === 'function' && \
             typeof __dh.values()[Symbol.asyncIterator] === 'function' && \
             typeof __dh.keys()[Symbol.asyncIterator] === 'function'"
        ));
    }

    /// A method called on a foreign object must not read someone else's state —
    /// the brand check is what a private-slot design buys.
    #[test]
    fn base_members_reject_a_foreign_this() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "var thrown = false; \
             try { Object.getOwnPropertyDescriptor(FileSystemHandle.prototype, 'kind') \
                     .get.call({}); } catch (e) { thrown = e instanceof TypeError; } \
             var rejected = false; \
             FileSystemHandle.prototype.getUniqueId.call({}) \
               .catch(function(e) { rejected = e instanceof TypeError; }); \
             thrown"
        ));
    }

    #[test]
    fn dir_entries_empty_for_unknown_id() {
        let rt = with_fsa();
        let r = rt.eval("_lumen_dir_entries('9999999')").unwrap();
        assert_eq!(r, JsValue::String("[]".into()));
    }

    #[test]
    fn dir_entries_returns_json_array_for_real_dir() {
        let tmp = std::env::temp_dir();
        let pid = super::dir_reg().lock().unwrap().allocate(tmp, TEST_ORIGIN, false);
        let rt = with_fsa();
        let r = rt.eval(&format!("_lumen_dir_entries('{pid}')")).unwrap();
        match r {
            JsValue::String(s) => {
                assert!(s.starts_with('['), "expected JSON array, got: {s}");
            }
            other => panic!("expected string JSON, got {other:?}"),
        }
    }

    #[test]
    fn show_open_file_picker_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof window.showOpenFilePicker().then === 'function'"
        ));
    }

    #[test]
    fn show_save_file_picker_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof window.showSaveFilePicker({suggestedName:'out.txt'}).then === 'function'"
        ));
    }

    #[test]
    fn show_directory_picker_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof window.showDirectoryPicker().then === 'function'"
        ));
    }

    // ── BUG-372: the origin private file system ──────────────────────────────

    /// Origin, runtime, fresh empty directory and a grant on it, as OPFS hands
    /// one out. Allocated *after* the install that would have revoked it.
    fn opfs_case(tag: &str, writable: bool) -> (V8JsRuntime, std::path::PathBuf, String) {
        let origin = format!("https://{tag}.opfs.test");
        let rt = with_fsa_for(&origin);
        let dir = std::env::temp_dir().join(format!("lumen_opfs_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let id = super::dir_reg()
            .lock()
            .unwrap()
            .allocate(dir.clone(), &origin, writable);
        (rt, dir, id)
    }

    fn string_eval(rt: &V8JsRuntime, expr: &str) -> String {
        match rt.eval(expr).unwrap() {
            JsValue::String(s) => s,
            other => panic!("expected a string, got {other:?}"),
        }
    }

    /// Two origins never share an OPFS directory, however similar they look
    /// once the unusable characters are stripped out.
    #[test]
    fn origin_slug_separates_similar_origins() {
        let a = super::origin_slug("https://example.com");
        let b = super::origin_slug("https://example.com:8443");
        let c = super::origin_slug("http://example.com");
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
        // Long opaque origins (a `file:` URL is its own origin here) stay usable
        // as a directory name.
        let long = super::origin_slug(&format!("file:///{}", "d/".repeat(200)));
        assert!(long.len() <= 49, "slug too long for a path component: {long}");
        assert!(!long.contains('/'), "slug must be a single path component");
    }

    /// A writable grant really creates and really removes. The OPFS stub this
    /// replaces resolved `removeEntry()` while removing nothing, and answered
    /// `getFileHandle()` with an object literal that had no `getFile()`.
    #[test]
    fn writable_grant_creates_and_removes_entries() {
        let (rt, dir, pid) = opfs_case("create_remove", true);

        let created = string_eval(&rt, &format!("_lumen_dir_get_file('{pid}', 'made.txt', true)"));
        assert!(created.contains(r#""token":""#), "no token in {created}");
        assert!(dir.join("made.txt").is_file(), "the file was never created");

        let removed = string_eval(&rt, &format!("_lumen_dir_remove_entry('{pid}', 'made.txt', false)"));
        assert_eq!(removed, "", "removeEntry reported {removed}");
        assert!(!dir.join("made.txt").exists(), "the file was never removed");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A non-empty directory needs `{recursive:true}` — the one removal failure
    /// the caller is expected to handle.
    #[test]
    fn remove_entry_needs_recursive_for_a_non_empty_directory() {
        let (rt, dir, pid) = opfs_case("recursive", true);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("leaf.txt"), b"x").unwrap();

        let err = string_eval(&rt, &format!("_lumen_dir_remove_entry('{pid}', 'sub', false)"));
        assert_eq!(err, "InvalidModificationError");
        assert!(dir.join("sub").is_dir());

        let ok = string_eval(&rt, &format!("_lumen_dir_remove_entry('{pid}', 'sub', true)"));
        assert_eq!(ok, "");
        assert!(!dir.join("sub").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A picked directory stays a read grant: `{create:true}` on it is refused,
    /// and nothing appears on disk.
    #[test]
    fn read_only_grant_refuses_to_create_or_remove() {
        let (rt, dir, pid) = opfs_case("read_only", false);
        std::fs::write(dir.join("present.txt"), b"x").unwrap();

        assert_eq!(
            string_eval(&rt, &format!("_lumen_dir_get_file('{pid}', 'new.txt', true)")),
            r#"{"error":"NotAllowedError"}"#
        );
        assert!(!dir.join("new.txt").exists());
        assert_eq!(
            string_eval(&rt, &format!("_lumen_dir_remove_entry('{pid}', 'present.txt', false)")),
            "NotAllowedError"
        );
        assert!(dir.join("present.txt").is_file());
        // Reading what the grant does cover still works.
        assert!(
            string_eval(&rt, &format!("_lumen_dir_get_file('{pid}', 'present.txt', false)"))
                .contains(r#""token":""#)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An entry name is one path component. Without this check `{create:true}`
    /// would let page script write anywhere the process can reach.
    #[test]
    fn entry_names_cannot_escape_the_directory() {
        let (rt, dir, pid) = opfs_case("traversal", true);
        for name in ["", ".", "..", "../escaped.txt", "sub/child.txt", "sub\\\\child.txt"] {
            assert_eq!(
                string_eval(&rt, &format!("_lumen_dir_get_file('{pid}', '{name}', true)")),
                r#"{"error":"TypeError"}"#,
                "name {name:?} was accepted"
            );
            assert_eq!(
                string_eval(&rt, &format!("_lumen_dir_get_subdir('{pid}', '{name}', true)")),
                r#"{"error":"TypeError"}"#,
                "name {name:?} was accepted as a directory"
            );
        }
        assert!(!dir.parent().unwrap().join("escaped.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `resolve()` answers with the segments between the two handles, `[]` for
    /// the handle itself and `null` for anything that is not below it. The stub
    /// answered `null` to all four, so a caller could not tell "not a
    /// descendant" from "not implemented".
    #[test]
    fn resolve_reports_the_path_of_a_descendant() {
        let (rt, dir, pid) = opfs_case("resolve", true);
        let out = string_eval(
            &rt,
            &format!(
                r#"
                var sub = JSON.parse(_lumen_dir_get_subdir('{pid}', 'sub', true));
                var leaf = JSON.parse(_lumen_dir_get_file(sub.path_id, 'leaf.txt', true));
                var r1 = _lumen_fs_resolve('{pid}', sub.path_id, '');
                var r2 = _lumen_fs_resolve('{pid}', '', leaf.token);
                var r3 = _lumen_fs_resolve(sub.path_id, '{pid}', '');
                var r4 = _lumen_fs_resolve('{pid}', '{pid}', '');
                [r1, r2, (r3 == null ? 'null' : r3), r4].join(';')
                "#
            ),
        );
        assert_eq!(out, r#"["sub"];["sub","leaf.txt"];null;[]"#);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A handle from the origin's own sandbox writes straight to its file. If
    /// it fell through to the save picker instead, the mocked picker id would
    /// belong to no write handle and the bytes would go nowhere — which is what
    /// the assertion on the file content catches.
    #[test]
    fn opfs_file_handle_writes_without_the_save_picker() {
        let (rt, dir, pid) = opfs_case("writable_handle", true);
        rt.eval(&format!(
            r#"
            var __w = null;
            {ROOT}('root', '{pid}')
              .getFileHandle('out.txt', {{ create: true }})
              .then(function(fh) {{ return fh.createWritable(); }})
              .then(function(s) {{ __w = s; }});
            "#
        ))
        .unwrap();
        assert!(bool_eval(&rt, "__w !== null"), "createWritable never resolved");
        rt.eval("__w.write('hello opfs');").unwrap();
        rt.eval("__w.close();").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("out.txt")).unwrap_or_default(),
            "hello opfs"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A picked file keeps needing the save picker: a read grant must not turn
    /// into a write grant just because the page asked twice.
    #[test]
    fn picked_file_token_grants_no_direct_write() {
        const ORIGIN: &str = "https://picked.opfs.test";
        let rt = with_fsa_for(ORIGIN);
        let tmp = std::env::temp_dir().join("lumen_opfs_picked_read_only.txt");
        std::fs::write(&tmp, b"original").unwrap();
        let token = crate::file_input::register_file_token(tmp.to_str().unwrap(), ORIGIN);
        assert!(bool_eval(
            &rt,
            &format!("_lumen_writable_from_token('{token}') == null")
        ));
        let _ = std::fs::remove_file(&tmp);
    }

    /// The root handed to `navigator.storage.getDirectory()` is an ordinary
    /// entry of the one directory registry, writable, under `data/opfs/`.
    #[test]
    fn opfs_root_entry_is_a_writable_dir_grant() {
        const ORIGIN: &str = "https://opfs-root.test";
        let json = super::opfs_root_entry_json(ORIGIN).expect("no OPFS root");
        assert!(json.starts_with(r#"{"name":"","path_id":""#), "{json}");
        let id = json
            .rsplit_once(r#""path_id":""#)
            .and_then(|(_, rest)| rest.split('"').next())
            .unwrap_or_default()
            .to_string();
        let grant = super::dir_reg().lock().unwrap().get_grant(&id, ORIGIN);
        let (path, writable) = grant.expect("root id is not a grant of that origin");
        assert!(writable, "the OPFS root must be writable");
        assert!(path.is_dir(), "the OPFS root was not created: {path:?}");
        assert!(path.ends_with(super::origin_slug(ORIGIN)));
        assert!(super::dir_reg().lock().unwrap().get_grant(&id, "https://other.test").is_none());
        let _ = std::fs::remove_dir_all(&path);
    }

    // ── BUG-373: which field carries the error name ──────────────────────────

    /// The harness of `with_fsa_for`, but every picker native answers `null` —
    /// the "user closed the dialog" path. The shim captures its natives at eval
    /// time (BUG-371), so the mocks go in before `FSAL_SHIM` is re-evaluated.
    fn with_cancelling_pickers(origin: &str) -> V8JsRuntime {
        let rt = with_fsa_for(origin);
        rt.eval(
            r#"
            globalThis._lumen_show_open_file_picker  = function() { return null; };
            globalThis._lumen_show_save_file_picker  = function() { return null; };
            globalThis._lumen_show_directory_picker  = function() { return null; };
            "#,
        )
        .unwrap();
        rt.eval(super::FSAL_SHIM).unwrap();
        rt
    }

    /// Settle `expr` (a promise the caller expects to reject) and report the
    /// rejection as `"<is a DOMException>|<name>|<message>"`.
    ///
    /// Every older test here stops at "the call returned a promise", which is
    /// exactly why nine swapped constructor arguments stayed green (BUG-373):
    /// a rejection can only be told apart by `name`, so a test that never reads
    /// it cannot see the two fields trade places.
    fn await_rejection(rt: &V8JsRuntime, expr: &str) -> String {
        rt.eval(&format!(
            r#"
            var __rej = 'never settled';
            ({expr}).then(
              function() {{ __rej = 'resolved'; }},
              function(e) {{
                __rej = (e instanceof DOMException) + '|' + e.name + '|' + e.message;
              }});
            "#
        ))
        .unwrap();
        string_eval(rt, "String(__rej)")
    }

    /// A cancelled open picker is the case the spec expects callers to swallow
    /// (`if (e.name === 'AbortError') return;`). With the arguments swapped the
    /// name was the human sentence, so a cancel was indistinguishable from a
    /// real failure and every dialog dismissal reached the error branch.
    #[test]
    fn cancelled_open_picker_rejects_with_abort_error() {
        let rt = with_cancelling_pickers("https://abort-open.fsal.test");
        assert_eq!(
            await_rejection(&rt, "window.showOpenFilePicker()"),
            "true|AbortError|The user aborted a request."
        );
    }

    #[test]
    fn cancelled_save_picker_rejects_with_abort_error() {
        let rt = with_cancelling_pickers("https://abort-save.fsal.test");
        assert_eq!(
            await_rejection(&rt, "window.showSaveFilePicker()"),
            "true|AbortError|The user aborted a request."
        );
    }

    #[test]
    fn cancelled_directory_picker_rejects_with_abort_error() {
        let rt = with_cancelling_pickers("https://abort-dir.fsal.test");
        assert_eq!(
            await_rejection(&rt, "window.showDirectoryPicker()"),
            "true|AbortError|The user aborted a request."
        );
    }

    /// The one non-picker site: a handle with no sandbox grant falls through to
    /// the save dialog, and a refused dialog must say `NotAllowedError`.
    ///
    /// Only the *save* picker is cancelled here: the open picker still has to
    /// hand out the grant-less handle the test then tries to write through.
    #[test]
    fn refused_write_permission_rejects_with_not_allowed_error() {
        let rt = with_fsa_for("https://abort-write.fsal.test");
        rt.eval("globalThis._lumen_show_save_file_picker = function() { return null; };")
            .unwrap();
        rt.eval(super::FSAL_SHIM).unwrap();
        rt.eval(RESOLVE_HANDLES).unwrap();
        assert_eq!(
            await_rejection(&rt, "__fh.createWritable()"),
            "true|NotAllowedError|Write permission denied or user cancelled"
        );
    }

    /// The `fsThrow` path already ordered its arguments correctly; this pins it
    /// so the two ways this module raises a DOMException cannot drift apart.
    /// It also covers the live probe from the bug report verbatim:
    /// `getFileHandle('nope.txt')` reported `name='File not found: nope.txt'`.
    #[test]
    fn missing_entry_rejects_with_not_found_error() {
        let (rt, dir, pid) = opfs_case("missing_entry", true);
        assert_eq!(
            await_rejection(
                &rt,
                &format!("{ROOT}('root', '{pid}').getFileHandle('nope.txt')")
            ),
            "true|NotFoundError|Cannot open file: nope.txt"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── BUG-374 points 7, 9, 10: what the objects actually do ────────────────

    /// Settle `expr` and report the rejection's `name` (or `resolved`).
    ///
    /// Picker-option validation raises plain `TypeError`s, which
    /// [`await_rejection`] cannot tell apart from each other by message alone.
    fn rejection_name(rt: &V8JsRuntime, expr: &str) -> String {
        rt.eval(&format!(
            "var __n = 'never settled'; \
             ({expr}).then(function() {{ __n = 'resolved'; }}, \
                           function(e) {{ __n = String(e && e.name); }});"
        ))
        .unwrap();
        string_eval(rt, "String(__n)")
    }

    /// A writable stream over a real file in a writable grant.
    fn opfs_writable(tag: &str) -> (V8JsRuntime, std::path::PathBuf) {
        let (rt, dir, pid) = opfs_case(tag, true);
        rt.eval(&format!(
            r#"
            var __w = null;
            {ROOT}('root', '{pid}')
              .getFileHandle('out.bin', {{ create: true }})
              .then(function(fh) {{ return fh.createWritable(); }})
              .then(function(s) {{ __w = s; }});
            "#
        ))
        .unwrap();
        assert!(bool_eval(&rt, "__w !== null"), "createWritable never resolved");
        (rt, dir)
    }

    /// Point 7: `truncate()` used to return a resolved promise and do nothing,
    /// so code that truncated a file before rewriting it got the old bytes with
    /// the new ones appended.
    #[test]
    fn truncate_shortens_the_written_file() {
        let (rt, dir) = opfs_writable("truncate");
        rt.eval("__w.write('hello world'); __w.truncate(5); __w.close();").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("out.bin")).unwrap_or_default(),
            "hello"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Point 7: `seek()` was the other no-op — every write appended, wherever
    /// the caller had positioned the stream.
    #[test]
    fn seek_writes_at_the_position() {
        let (rt, dir) = opfs_writable("seek");
        rt.eval("__w.write('abc'); __w.seek(0); __w.write('X'); __w.close();").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("out.bin")).unwrap_or_default(),
            "Xbc"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// FS §7.1's `WriteParams` dictionary — `write({type, position, size, data})`
    /// used to fall into `String(data)` whole and land in the file as
    /// `[object Object]`.
    #[test]
    fn write_params_dictionary_is_understood() {
        let (rt, dir) = opfs_writable("write_params");
        rt.eval(
            "__w.write({type:'write', data:'abcdef'}); \
             __w.write({type:'write', position:1, data:'ZZ'}); \
             __w.write({type:'truncate', size:4}); \
             __w.close();",
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("out.bin")).unwrap_or_default(),
            "aZZd"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `seek` command with no position and an unknown command type are the two
    /// shapes the spec rejects rather than silently ignores.
    #[test]
    fn malformed_write_commands_reject() {
        let (rt, dir) = opfs_writable("write_params_bad");
        assert_eq!(rejection_name(&rt, "__w.write({type:'seek'})"), "SyntaxError");
        assert_eq!(rejection_name(&rt, "__w.write({type:'truncate'})"), "SyntaxError");
        assert_eq!(rejection_name(&rt, "__w.write({type:'nope', data:'x'})"), "TypeError");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Point 7: bytes are bytes. `write()` used to push everything through
    /// `String(data)`, so a typed array became the text `0,1,255` and a `Blob`
    /// became the literal `[object Blob]`.
    #[test]
    fn binary_chunks_are_written_verbatim() {
        let (rt, dir) = opfs_writable("binary");
        rt.eval("__w.write(new Uint8Array([0, 1, 255, 65])); __w.close();").unwrap();
        assert_eq!(
            std::fs::read(dir.join("out.bin")).unwrap_or_default(),
            vec![0u8, 1, 255, 65]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Non-ASCII text survives the base64 hop to Rust as UTF-8.
    #[test]
    fn utf8_text_round_trips() {
        let (rt, dir) = opfs_writable("utf8");
        rt.eval(r#"__w.write('привет \u{1F600}'); __w.close();"#).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("out.bin")).unwrap_or_default(),
            "привет \u{1F600}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Point 6, the half that mattered at runtime: iterating a directory yields
    /// handles that *work*. They used to be built on an empty grant id, so the
    /// names were right and everything behind them was empty.
    #[test]
    fn iterated_entries_carry_a_working_grant() {
        let (rt, dir, pid) = opfs_case("iterate", true);
        std::fs::write(dir.join("a.txt"), b"payload").unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("inner.txt"), b"deep").unwrap();
        rt.eval(&format!(
            r#"
            var __names = [], __sizes = [], __inner = 'none';
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              for await (var pair of root) {{
                __names.push(pair[0] + ':' + pair[1].kind);
                if (pair[1].kind === 'file') {{
                  var f = await pair[1].getFile();
                  __sizes.push(pair[0] + '=' + f.size);
                }} else {{
                  for await (var child of pair[1]) {{ __inner = child[0]; }}
                }}
              }}
            }})();
            "#
        ))
        .unwrap();
        assert_eq!(string_eval(&rt, "__names.sort().join(',')"), "a.txt:file,sub:directory");
        assert_eq!(string_eval(&rt, "__sizes.join(',')"), "a.txt=7");
        assert_eq!(string_eval(&rt, "__inner"), "inner.txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Point 5: two handles on the same entry compare equal even though each
    /// carries its own freshly minted grant token.
    #[test]
    fn is_same_entry_sees_through_distinct_tokens() {
        let (rt, dir, pid) = opfs_case("same_entry", true);
        rt.eval(&format!(
            r#"
            var __same = null, __diff = null, __uid = null;
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              var a = await root.getFileHandle('a.txt', {{ create: true }});
              var b = await root.getFileHandle('a.txt', {{ create: true }});
              var c = await root.getFileHandle('c.txt', {{ create: true }});
              __same = await a.isSameEntry(b);
              __diff = await a.isSameEntry(c);
              __uid  = (await a.getUniqueId()) === (await b.getUniqueId());
            }})();
            "#
        ))
        .unwrap();
        assert!(bool_eval(&rt, "__same === true"), "two handles on one file compared unequal");
        assert!(bool_eval(&rt, "__diff === false"));
        assert!(bool_eval(&rt, "__uid === true"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Point 5: `queryPermission` answers from the grant behind the handle — a
    /// writable sandbox grant is `granted` for both modes, a picked read-only
    /// directory says `prompt` for `readwrite` rather than claiming a write
    /// permission the next call would contradict.
    #[test]
    fn permission_answers_follow_the_grant() {
        let (rt, dir, pid) = opfs_case("perm_rw", true);
        let (rt_ro, dir_ro, pid_ro) = opfs_case("perm_ro", false);
        rt.eval(&format!("var __d = {ROOT}('root', '{pid}');")).unwrap();
        rt_ro.eval(&format!("var __d = {ROOT}('root', '{pid_ro}');")).unwrap();

        assert_eq!(string_eval_await(&rt, "__d.queryPermission()"), "granted");
        assert_eq!(
            string_eval_await(&rt, "__d.queryPermission({mode:'readwrite'})"),
            "granted"
        );
        assert_eq!(string_eval_await(&rt_ro, "__d.queryPermission()"), "granted");
        assert_eq!(
            string_eval_await(&rt_ro, "__d.requestPermission({mode:'readwrite'})"),
            "prompt"
        );
        assert_eq!(rejection_name(&rt, "__d.queryPermission({mode:'write'})"), "TypeError");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir_ro);
    }

    /// Settle `expr` and read the fulfilled value back as a string.
    fn string_eval_await(rt: &V8JsRuntime, expr: &str) -> String {
        rt.eval(&format!(
            "var __s = 'never settled'; \
             ({expr}).then(function(v) {{ __s = String(v); }}, \
                           function(e) {{ __s = 'rejected:' + (e && e.name); }});"
        ))
        .unwrap();
        string_eval(rt, "String(__s)")
    }

    /// Point 5: `remove()` and `move()` — both were missing outright.
    #[test]
    fn remove_and_move_act_on_the_file_system() {
        let (rt, dir, pid) = opfs_case("remove_move", true);
        rt.eval(&format!(
            r#"
            var __moved = 'none', __removed = 'none';
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              var f = await root.getFileHandle('before.txt', {{ create: true }});
              await f.move('after.txt');
              __moved = f.name;
              var g = await root.getFileHandle('gone.txt', {{ create: true }});
              await g.remove();
              __removed = 'ok';
            }})();
            "#
        ))
        .unwrap();
        assert_eq!(string_eval(&rt, "__moved"), "after.txt");
        assert_eq!(string_eval(&rt, "__removed"), "ok");
        assert!(dir.join("after.txt").is_file(), "move() did not rename the file");
        assert!(!dir.join("before.txt").exists(), "the old name survived move()");
        assert!(!dir.join("gone.txt").exists(), "remove() did not delete the file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A read-only grant refuses both, rather than reporting success.
    #[test]
    fn remove_refuses_a_read_only_grant() {
        let (rt, dir, pid) = opfs_case("remove_ro", false);
        std::fs::write(dir.join("keep.txt"), b"x").unwrap();
        rt.eval(&format!(
            r#"
            var __p = null;
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              __p = await root.getFileHandle('keep.txt');
            }})();
            "#
        ))
        .unwrap();
        assert_eq!(rejection_name(&rt, "__p.remove()"), "NotAllowedError");
        assert!(dir.join("keep.txt").is_file(), "a read-only grant deleted a file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Point 10: the pickers took an options dictionary and ignored it whole.
    /// Every case here is a `TypeError` the spec states, and every one of them
    /// used to open the dialog instead.
    #[test]
    fn picker_options_are_validated() {
        let rt = with_fsa();
        let cases = [
            "showOpenFilePicker({types:[], excludeAcceptAllOption:true})",
            "showOpenFilePicker({types:[{accept:{'image':['.png']}}]})",
            "showOpenFilePicker({types:[{accept:{'text/plain;charset=utf-8':['.txt']}}]})",
            "showOpenFilePicker({types:[{accept:{'text/plain':['txt']}}]})",
            "showOpenFilePicker({types:[{accept:{'text/plain':['.TXT']}}]})",
            "showOpenFilePicker({types:[{accept:{'text/plain':['.txt.']}}]})",
            "showOpenFilePicker({types:[{accept:{'text/plain':['.abcdefghijklmnop']}}]})",
            "showOpenFilePicker({types:[{}]})",
            "showOpenFilePicker({startIn:'nowhere'})",
            "showOpenFilePicker({id:'has spaces'})",
            "showSaveFilePicker({id:'0123456789012345678901234567890123'})",
            "showDirectoryPicker({startIn:'nowhere'})",
        ];
        for case in cases {
            assert_eq!(rejection_name(&rt, case), "TypeError", "accepted: {case}");
        }
    }

    /// …and the shapes the spec allows still reach the picker.
    #[test]
    fn valid_picker_options_are_accepted() {
        let rt = with_fsa();
        let cases = [
            "showOpenFilePicker({types:[{description:'Text', accept:{'text/plain':['.txt','.md']}}]})",
            "showOpenFilePicker({startIn:'documents', id:'lumen_test-1'})",
            "showOpenFilePicker({types:[], excludeAcceptAllOption:false})",
            "showDirectoryPicker({startIn:__dh})",
        ];
        for case in cases {
            assert_eq!(rejection_name(&rt, case), "resolved", "rejected: {case}");
        }
    }

    /// Point 9: all three interfaces are `[Serializable]`. A clone used to come
    /// back as a plain `{}` — same properties, no class, no grant.
    ///
    /// The registry is the one `dom.rs`'s shim publishes; this harness does not
    /// evaluate that shim, so the test stands one in of the same shape and
    /// checks what the *cloner* produces.
    #[test]
    fn handles_register_a_structured_clone() {
        let (rt, dir, pid) = opfs_case("serializable", true);
        rt.eval(
            r#"
            var CLONERS = [];
            Object.defineProperty(window, '__lumen_platform_cloners', {
              value: {
                register: function(test, clone) { CLONERS.push([test, clone]); },
                find: function(v) {
                  for (var i = 0; i < CLONERS.length; i++) {
                    if (CLONERS[i][0](v)) return CLONERS[i][1];
                  }
                  return null;
                }
              },
              configurable: true
            });
            "#,
        )
        .unwrap();
        rt.eval(super::FSAL_SHIM).unwrap();
        rt.eval(&format!(
            r#"
            var __ok = null;
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              var f = await root.getFileHandle('cloned.txt', {{ create: true }});
              var clone = window.__lumen_platform_cloners.find(f)(f);
              __ok = (clone instanceof FileSystemFileHandle)
                  && (clone !== f) && clone.name === 'cloned.txt'
                  && (await clone.isSameEntry(f))
                  && window.__lumen_platform_cloners.find(root)(root) instanceof
                       FileSystemDirectoryHandle;
            }})();
            "#
        ))
        .unwrap();
        assert!(bool_eval(&rt, "__ok === true"), "the clone lost its class or its grant");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Point 7, the inheritance half: the class extends whatever
    /// `WritableStream` the runtime has, and the stream's sink is the FS write
    /// algorithm — so `getWriter().write(chunk)` commits the same bytes as
    /// `stream.write(chunk)`.
    ///
    /// The base here is a stand-in (the real one comes from `dom.rs`'s shim,
    /// which this harness does not evaluate). What it can prove is the wiring:
    /// the prototype chain, and that the sink handed to the base is the one that
    /// reaches Rust.
    #[test]
    fn writable_extends_the_runtime_writable_stream() {
        let (rt, dir, pid) = opfs_case("stream_base", true);
        rt.eval(
            r#"
            function WritableStream(sink) { this._ws_sink = sink; this._ws_state = 'writable'; }
            WritableStream.prototype.getWriter = function() {
              var sink = this._ws_sink;
              return { write: function(chunk) { return sink.write(chunk); } };
            };
            WritableStream.prototype.close = function() {
              var sink = this._ws_sink;
              return Promise.resolve().then(function() { return sink.close(); });
            };
            globalThis.WritableStream = WritableStream;
            "#,
        )
        .unwrap();
        rt.eval(super::FSAL_SHIM).unwrap();
        rt.eval(&format!(
            r#"
            var __w = null, __chain = null;
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              var f = await root.getFileHandle('stream.txt', {{ create: true }});
              __w = await f.createWritable();
              __chain = (__w instanceof FileSystemWritableFileStream)
                     && (__w instanceof WritableStream);
              await __w.getWriter().write('through the writer');
              await __w.close();
            }})();
            "#
        ))
        .unwrap();
        assert!(bool_eval(&rt, "__chain === true"), "not a WritableStream subclass");
        assert_eq!(
            std::fs::read_to_string(dir.join("stream.txt")).unwrap_or_default(),
            "through the writer"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }


    // ── BUG-374 point 8: FileSystemSyncAccessHandle ──────────────────────────

    /// The interface was missing outright. It is the OPFS write path every
    /// wasm-backed database uses, and unlike the writable stream it is
    /// unbuffered: what one call writes, the next call reads back.
    #[test]
    fn sync_access_handle_reads_back_what_it_wrote() {
        let (rt, dir, pid) = opfs_case("sync_rw", true);
        rt.eval(&format!(
            r#"
            var __shape = null, __written = null, __size = null, __read = null;
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              var f = await root.getFileHandle('sync.bin', {{ create: true }});
              var h = await f.createSyncAccessHandle();
              __shape = (h instanceof FileSystemSyncAccessHandle) + ',' +
                        Object.prototype.toString.call(h);
              __written = h.write(new Uint8Array([115, 121, 110, 99, 33]), {{ at: 0 }});
              __size = h.getSize();
              var buf = new Uint8Array(2);
              var n = h.read(buf, {{ at: 2 }});
              __read = n + ':' + buf[0] + ',' + buf[1];
              h.flush();
              h.close();
            }})();
            "#
        ))
        .unwrap();
        assert_eq!(
            string_eval(&rt, "String(__shape)"),
            "true,[object FileSystemSyncAccessHandle]"
        );
        assert_eq!(string_eval(&rt, "String(__written)"), "5");
        assert_eq!(string_eval(&rt, "String(__size)"), "5");
        assert_eq!(string_eval(&rt, "String(__read)"), "2:110,99");
        assert_eq!(std::fs::read(dir.join("sync.bin")).unwrap_or_default(), b"sync!".to_vec());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `truncate()` is real here too, `close()` is idempotent, and every other
    /// member throws `InvalidStateError` once the handle is closed.
    #[test]
    fn sync_access_handle_truncates_and_closes() {
        let (rt, dir, pid) = opfs_case("sync_close", true);
        rt.eval(&format!(
            r#"
            var __after = null, __closed = null, __again = null;
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              var f = await root.getFileHandle('sync.bin', {{ create: true }});
              var h = await f.createSyncAccessHandle();
              h.write(new Uint8Array([1, 2, 3, 4, 5, 6]), {{ at: 0 }});
              h.truncate(3);
              __after = h.getSize();
              h.close();
              try {{ h.getSize(); __closed = 'no throw'; }} catch (e) {{ __closed = e.name; }}
              try {{ h.close(); __again = 'ok'; }} catch (e) {{ __again = e.name; }}
            }})();
            "#
        ))
        .unwrap();
        assert_eq!(string_eval(&rt, "String(__after)"), "3");
        assert_eq!(string_eval(&rt, "String(__closed)"), "InvalidStateError");
        assert_eq!(string_eval(&rt, "String(__again)"), "ok");
        assert_eq!(std::fs::read(dir.join("sync.bin")).unwrap_or_default(), vec![1u8, 2, 3]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// FS §7.2 puts this interface on OPFS handles only. A picked file carries a
    /// read grant, so it must be refused rather than silently opened for writing.
    #[test]
    fn sync_access_handle_refuses_a_read_only_grant() {
        let (rt, dir, pid) = opfs_case("sync_ro", false);
        std::fs::write(dir.join("keep.txt"), b"x").unwrap();
        rt.eval(&format!(
            r#"
            var __h = null;
            (async function() {{
              __h = await {ROOT}('root', '{pid}').getFileHandle('keep.txt');
            }})();
            "#
        ))
        .unwrap();
        assert_eq!(
            rejection_name(&rt, "__h.createSyncAccessHandle()"),
            "NotAllowedError"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The size a handle was created with goes stale the moment anything writes
    /// through it: `getFile()` reported the old length while `text()` already
    /// returned the new contents (found by the live probe, `hello/0`).
    #[test]
    fn get_file_reports_the_current_size() {
        let (rt, dir, pid) = opfs_case("stale_size", true);
        rt.eval(&format!(
            r#"
            var __size = null;
            (async function() {{
              var root = {ROOT}('root', '{pid}');
              var f = await root.getFileHandle('grow.txt', {{ create: true }});
              var w = await f.createWritable();
              await w.write('0123456789');
              await w.close();
              __size = (await f.getFile()).size;
            }})();
            "#
        ))
        .unwrap();
        assert_eq!(string_eval(&rt, "String(__size)"), "10");
        let _ = std::fs::remove_dir_all(&dir);
    }

}
