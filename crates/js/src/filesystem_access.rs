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
//! | `_lumen_fs_observe` | `(path_id, token, recursive: bool) → Option<String>` | Snapshot a subtree for `FileSystemObserver` → observation id |
//! | `_lumen_fs_poll_changes` | `(id: String) → String` | Re-snapshot and diff → JSON array of change records |
//! | `_lumen_fs_unobserve` | `(id: String) → bool` | Forget one observation |
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

// ── Observation registry (FileSystemObserver) ──────────────────────────────────

/// One entry of an observed tree as it looked at the previous poll.
///
/// Deliberately not the full `Metadata`: these three fields are what the change
/// types of FS Observer §2 are derived from, and comparing anything else (access
/// time, permissions) would report `modified` for a plain read.
#[cfg(feature = "v8-backend")]
#[derive(Clone, PartialEq, Eq)]
struct ObsEntry {
    /// Whether the entry is a directory. A symlink is never followed, so a
    /// symlinked directory counts as a plain entry and is not descended into.
    is_dir: bool,
    /// File length in bytes; always 0 for a directory.
    len: u64,
    /// Last-modified time in milliseconds since the epoch, 0 if unavailable.
    mtime_ms: u64,
}

/// One live `FileSystemObserver.observe()` registration.
///
/// Lumen has no OS file-watching dependency, so an observation is a *snapshot*
/// of the observed subtree plus the diff taken against it on the next poll (the
/// shim drives that poll from a shared `setInterval` while at least one
/// observation is live). Everything FS Observer §2 describes is derivable from
/// two consecutive snapshots — the one thing that is not is the identity of a
/// moved entry, see [`diff_snapshots`].
#[cfg(feature = "v8-backend")]
struct Observation {
    /// Origin the observation was issued to — checked on every poll.
    origin: String,
    /// Root of the observed subtree. Never exposed to JS.
    root: PathBuf,
    /// The observed handle is a file, not a directory: the snapshot then holds
    /// exactly one entry, keyed by the empty path (the root is what changed).
    root_is_file: bool,
    /// Whether subdirectories are descended into (`observe(h, {recursive:true})`).
    recursive: bool,
    /// Write permission of the root grant, inherited by every grant minted for a
    /// change record — the same rule `_lumen_dir_entries` applies to a listing.
    writable: bool,
    /// The tree as of the previous poll, keyed by path components relative to
    /// [`Observation::root`].
    snapshot: std::collections::BTreeMap<Vec<String>, ObsEntry>,
    /// Set once the root itself became unreadable: an `errored` record was
    /// delivered and this observation answers nothing further (FS Observer §3).
    ended: bool,
}

#[cfg(feature = "v8-backend")]
static OBS_REG: OnceLock<Mutex<HashMap<String, Observation>>> = OnceLock::new();

#[cfg(feature = "v8-backend")]
fn obs_reg() -> &'static Mutex<HashMap<String, Observation>> {
    OBS_REG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Depth cap for a recursive observation walk.
///
/// `DirEntry::metadata` does not follow symlinks, so a directory symlink
/// pointing at an ancestor is already not descended into; this is the second
/// line of defence against a pathological tree costing unbounded stack on a
/// timer tick.
#[cfg(feature = "v8-backend")]
const OBS_MAX_DEPTH: usize = 32;

#[cfg(feature = "v8-backend")]
fn obs_entry_of(meta: &std::fs::Metadata) -> ObsEntry {
    let is_dir = meta.is_dir();
    ObsEntry {
        is_dir,
        len: if is_dir { 0 } else { meta.len() },
        mtime_ms: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    }
}

/// Snapshot of everything an observation covers, keyed by path components
/// relative to its root.
///
/// `None` means the root itself is gone or unreadable — the caller turns that
/// into the single `errored` record that ends the observation.
#[cfg(feature = "v8-backend")]
fn snapshot_tree(
    root: &std::path::Path,
    root_is_file: bool,
    recursive: bool,
) -> Option<std::collections::BTreeMap<Vec<String>, ObsEntry>> {
    use std::collections::BTreeMap;

    let mut out: BTreeMap<Vec<String>, ObsEntry> = BTreeMap::new();
    if root_is_file {
        // The observed entry *is* the root, so it is keyed by the empty relative
        // path — which is also what `relativePathComponents` must be for it.
        out.insert(Vec::new(), obs_entry_of(&std::fs::metadata(root).ok()?));
        return Some(out);
    }
    if !root.is_dir() {
        return None;
    }

    fn walk(
        dir: &std::path::Path,
        prefix: &mut Vec<String>,
        recursive: bool,
        depth: usize,
        out: &mut BTreeMap<Vec<String>, ObsEntry>,
    ) {
        if depth > OBS_MAX_DEPTH {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for entry in rd.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
            let Ok(meta) = entry.metadata() else { continue };
            let obs = obs_entry_of(&meta);
            let descend = obs.is_dir && recursive;
            prefix.push(name);
            out.insert(prefix.clone(), obs);
            if descend {
                walk(&entry.path(), prefix, recursive, depth + 1, out);
            }
            prefix.pop();
        }
    }

    walk(root, &mut Vec::new(), recursive, 0, &mut out);
    Some(out)
}

/// One change between two snapshots, in the vocabulary of `FileSystemChangeType`.
#[cfg(feature = "v8-backend")]
struct ObsChange {
    /// `appeared` / `disappeared` / `modified` / `moved`.
    kind: &'static str,
    /// Path components of the entry relative to the observation root.
    path: Vec<String>,
    /// Whether the entry is a directory — decides which kind of grant the
    /// change record's `changedHandle` gets.
    is_dir: bool,
    /// Where a `moved` entry came from; `None` for every other kind.
    moved_from: Option<Vec<String>>,
}

/// Changes between two snapshots of the same observation.
///
/// A rename is the one thing two snapshots cannot show directly: it looks
/// exactly like a disappearance plus an appearance. It is reported as `moved`
/// only when the poll saw *exactly one* of each and the two carry identical
/// metadata (kind, length, mtime — all preserved by `rename`, none of them by a
/// fresh write), so the ambiguous cases stay two honest records rather than one
/// invented one.
#[cfg(feature = "v8-backend")]
fn diff_snapshots(
    old: &std::collections::BTreeMap<Vec<String>, ObsEntry>,
    new: &std::collections::BTreeMap<Vec<String>, ObsEntry>,
) -> Vec<ObsChange> {
    let mut out = Vec::new();
    let mut appeared = Vec::new();
    for (path, entry) in new {
        match old.get(path) {
            None => appeared.push((path.clone(), entry.clone())),
            Some(prev) if prev != entry => out.push(ObsChange {
                kind: "modified",
                path: path.clone(),
                is_dir: entry.is_dir,
                moved_from: None,
            }),
            Some(_) => {}
        }
    }
    let mut disappeared = Vec::new();
    for (path, entry) in old {
        if !new.contains_key(path) {
            disappeared.push((path.clone(), entry.clone()));
        }
    }

    if appeared.len() == 1 && disappeared.len() == 1 && appeared[0].1 == disappeared[0].1 {
        out.push(ObsChange {
            kind: "moved",
            path: appeared[0].0.clone(),
            is_dir: appeared[0].1.is_dir,
            moved_from: Some(disappeared[0].0.clone()),
        });
        return out;
    }
    out.extend(appeared.into_iter().map(|(path, entry)| ObsChange {
        kind: "appeared",
        path,
        is_dir: entry.is_dir,
        moved_from: None,
    }));
    out.extend(disappeared.into_iter().map(|(path, entry)| ObsChange {
        kind: "disappeared",
        path,
        is_dir: entry.is_dir,
        moved_from: None,
    }));
    out
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

/// JSON array of strings — how a change record carries its relative path.
#[cfg(feature = "v8-backend")]
fn json_string_array(parts: &[String]) -> String {
    let quoted: Vec<String> =
        parts.iter().map(|p| format!("\"{}\"", json_escape(p))).collect();
    format!("[{}]", quoted.join(","))
}

/// `changedHandle` of one change record: the same `{kind, …grant}` shape
/// `_lumen_dir_entries` hands out, so the shim builds it with the same factory.
///
/// A grant is minted for a `disappeared` entry too — the record's handle names
/// the entry that went away, and FS Observer §2 makes `changedHandle`
/// non-nullable, so answering `null` there would be a shape the page cannot
/// feature-detect around.
#[cfg(feature = "v8-backend")]
fn obs_handle_json(
    path: &std::path::Path,
    is_dir: bool,
    origin: &str,
    writable: bool,
) -> Option<String> {
    if is_dir {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let id = dir_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .allocate(path.to_path_buf(), origin, writable);
        if id.is_empty() {
            return None;
        }
        return Some(format!(
            r#"{{"kind":"directory","name":"{}","path_id":"{}"}}"#,
            json_escape(&name),
            json_escape(&id)
        ));
    }
    let json = file_entry_json(path, origin, writable)?;
    Some(format!(r#"{{"kind":"file",{}"#, &json[1..]))
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
    // Including any file the previous document still had open for sync access,
    // and any tree it left an observer polling.
    sync_reg().lock().unwrap_or_else(|e| e.into_inner()).retain(|_, h| h.origin != origin);
    obs_reg().lock().unwrap_or_else(|e| e.into_inner()).retain(|_, o| o.origin != origin);

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

    // ── FileSystemObserver (FS Observer §2-§3) ────────────────────────────────
    // No OS file-watcher: an observation is a snapshot, and `_lumen_fs_poll_changes`
    // is the diff against it. The shim ticks that poll from a shared interval —
    // which is why the *state* lives here and not in JS: a page that drops its
    // observer must stop costing directory walks, and the origin check has to
    // happen where the grant registries are.

    let observe_origin = origin.to_string();
    let fs_observe =
        into_v8_fn3(move |path_id: String, token: String, recursive: bool| -> Option<String> {
            let (root, writable, root_is_file) = if !path_id.is_empty() {
                let (dir, writable) = dir_reg()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_grant(&path_id, &observe_origin)?;
                (dir, writable, false)
            } else if !token.is_empty() {
                let path = crate::file_input::path_for_token(&token, &observe_origin)?;
                let writable =
                    crate::file_input::writable_path_for_token(&token, &observe_origin).is_some();
                (path, writable, true)
            } else {
                return None;
            };
            let snapshot = snapshot_tree(&root, root_is_file, recursive)?;
            let id = crate::file_input::new_grant_id()?;
            obs_reg().lock().unwrap_or_else(|e| e.into_inner()).insert(
                id.clone(),
                Observation {
                    origin: observe_origin.clone(),
                    root,
                    root_is_file,
                    recursive,
                    writable,
                    snapshot,
                    ended: false,
                },
            );
            Some(id)
        });
    rt.register_native("_lumen_fs_observe", fs_observe)?;

    let poll_origin = origin.to_string();
    let fs_poll = into_v8_fn1(move |id: String| -> String {
        // The snapshot swap happens under the lock; building the records does
        // not, because minting a grant takes the directory/token registries and
        // holding two of them at once is how a deadlock gets written.
        let taken = {
            let mut reg = obs_reg().lock().unwrap_or_else(|e| e.into_inner());
            let Some(obs) = reg.get_mut(&id) else { return "[]".to_string() };
            if obs.origin != poll_origin || obs.ended {
                return "[]".to_string();
            }
            match snapshot_tree(&obs.root, obs.root_is_file, obs.recursive) {
                Some(next) => {
                    let changes = diff_snapshots(&obs.snapshot, &next);
                    obs.snapshot = next;
                    Some((obs.root.clone(), obs.writable, changes))
                }
                None => {
                    // The observed root itself is gone: FS Observer §3 ends the
                    // observation on `errored` rather than reporting every
                    // descendant as separately disappeared.
                    obs.ended = true;
                    None
                }
            }
        };
        let Some((root, writable, changes)) = taken else {
            return r#"[{"type":"errored","path":[],"movedFrom":null,"handle":null}]"#.to_string();
        };
        let mut records = Vec::with_capacity(changes.len());
        for change in changes {
            let path = change.path.iter().fold(root.clone(), |acc, part| acc.join(part));
            let handle = obs_handle_json(&path, change.is_dir, &poll_origin, writable)
                .unwrap_or_else(|| "null".to_string());
            let moved_from = match &change.moved_from {
                Some(from) => json_string_array(from),
                None => "null".to_string(),
            };
            records.push(format!(
                r#"{{"type":"{}","path":{},"movedFrom":{},"handle":{}}}"#,
                change.kind,
                json_string_array(&change.path),
                moved_from,
                handle
            ));
        }
        format!("[{}]", records.join(","))
    });
    rt.register_native("_lumen_fs_poll_changes", fs_poll)?;

    let unobserve_origin = origin.to_string();
    let fs_unobserve = into_v8_fn1(move |id: String| -> bool {
        let mut reg = obs_reg().lock().unwrap_or_else(|e| e.into_inner());
        match reg.get(&id) {
            Some(o) if o.origin == unobserve_origin => reg.remove(&id).is_some(),
            _ => false,
        }
    });
    rt.register_native("_lumen_fs_unobserve", fs_unobserve)?;

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
  return (typeof __lumen_C[name] === 'function') ? __lumen_C[name] : null;
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
var NAT_FS_OBSERVE   = nat('_lumen_fs_observe');
var NAT_FS_POLL      = nat('_lumen_fs_poll_changes');
var NAT_FS_UNOBSERVE = nat('_lumen_fs_unobserve');
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
var FS_INTERNAL = (typeof __lumen_C.__lumen_fs_internal === 'object')
  ? __lumen_C.__lumen_fs_internal : null;

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
  var st = {
    id: String(handleId == null ? '' : handleId),
    position: 0,
    closed: false,
    queue: Promise.resolve(),
  };
  var stream;
  if (WritableStreamBase) {
    // The stream really is a `WritableStream`: its sink is the FS write
    // algorithm, so `getWriter().write(chunk)` and `stream.write(chunk)` commit
    // the same bytes through the same path. Constructed, not `.call`ed: the
    // base refuses a call without `new` (BUG-684), and `Reflect.construct`
    // hands it `new.target` while keeping this subclass's prototype.
    stream = Reflect.construct(WritableStreamBase, [{
      write: function(chunk) { return writeCommand(st, chunk); },
      close: function() { return enqueue(st, function() { return commitWrite(st); }); },
      abort: function() { st.closed = true; },
    }], FileSystemWritableFileStream);
  } else {
    stream = new FileSystemWritableFileStream(BRAND);
  }
  WRITE_STATE.set(stream, st);
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

// ── FileSystemObserver (FS Observer §2-§3) ───────────────────────────────────

// There is no OS file-watcher under this: `_lumen_fs_observe` snapshots the
// observed subtree and `_lumen_fs_poll_changes` diffs the next snapshot against
// it, so "when does a change surface" is really "how often does someone look".
// One shared interval drives every live observation — a timer per observation
// would multiply directory walks by the number of observers watching the same
// tree.
var OBS_POLL_MS = 100;

// Captured at eval time like the natives: a bare runtime (the unit-test harness)
// has no timers, and the classes must still exist and answer there.
var OBS_SET_INTERVAL =
  (typeof globalThis.setInterval === 'function') ? globalThis.setInterval : null;
var OBS_CLEAR_INTERVAL =
  (typeof globalThis.clearInterval === 'function') ? globalThis.clearInterval : null;

// observer -> {callback, entries: [{id, handle, uniqueId}]}
var OBS_STATE = new WeakMap();

// Every observer with at least one live observation, held *strongly* on purpose:
// FS Observer §2 keeps an observer alive while it is observing, so a page that
// kept no reference to it must still get its records.
var OBS_LIVE = [];
var OBS_TIMER = null;

function FileSystemChangeRecord(brand) {
  if (brand !== BRAND) illegalConstructor();
}
defineToStringTag(FileSystemChangeRecord, 'FileSystemChangeRecord');

// record -> {root, changedHandle, relativePathComponents, type, relativePathMovedFrom}
var RECORD_STATE = new WeakMap();

function recordStateOf(obj) {
  var st = (obj !== null && typeof obj === 'object') ? RECORD_STATE.get(obj) : undefined;
  if (st === undefined) throw new TypeError('Illegal invocation');
  return st;
}

defineAttribute(FileSystemChangeRecord.prototype, 'root',
  function() { return recordStateOf(this).root; });
defineAttribute(FileSystemChangeRecord.prototype, 'changedHandle',
  function() { return recordStateOf(this).changedHandle; });
defineAttribute(FileSystemChangeRecord.prototype, 'relativePathComponents',
  function() { return recordStateOf(this).relativePathComponents; });
defineAttribute(FileSystemChangeRecord.prototype, 'type',
  function() { return recordStateOf(this).type; });
defineAttribute(FileSystemChangeRecord.prototype, 'relativePathMovedFrom',
  function() { return recordStateOf(this).relativePathMovedFrom; });

function frozenPath(parts) {
  var out = [];
  for (var i = 0; i < (parts || []).length; i++) out.push(String(parts[i]));
  // `FrozenArray<USVString>`: a page must not be able to edit the path inside a
  // record it was handed and have the next reader of that record see the edit.
  return Object.freeze(out);
}

function makeChangeRecord(root, raw) {
  var record = new FileSystemChangeRecord(BRAND);
  RECORD_STATE.set(record, {
    root: root,
    // An `errored` record names no entry of its own, and `changedHandle` is not
    // nullable — §3 makes the observation root what such a record is about.
    changedHandle: raw.handle ? handleFromEntry(raw.handle) : root,
    relativePathComponents: frozenPath(raw.path),
    type: String(raw.type),
    relativePathMovedFrom: raw.movedFrom ? frozenPath(raw.movedFrom) : null,
  });
  return record;
}

function FileSystemObserver(callback) {
  if (!(this instanceof FileSystemObserver)) {
    throw new TypeError(
      "Failed to construct 'FileSystemObserver': please use the 'new' operator");
  }
  if (typeof callback !== 'function') {
    throw new TypeError(
      "Failed to construct 'FileSystemObserver': parameter 1 is not of type 'Function'");
  }
  OBS_STATE.set(this, { callback: callback, entries: [] });
}
defineToStringTag(FileSystemObserver, 'FileSystemObserver');

function observerStateOf(obj) {
  var st = (obj !== null && typeof obj === 'object') ? OBS_STATE.get(obj) : undefined;
  if (st === undefined) throw new TypeError('Illegal invocation');
  return st;
}

// Drop every observation the predicate selects, release it on the native side,
// and stop the shared timer once nothing is left to poll.
function dropEntries(st, observer, pred) {
  var kept = [];
  for (var i = 0; i < st.entries.length; i++) {
    if (pred(st.entries[i])) {
      if (NAT_FS_UNOBSERVE) NAT_FS_UNOBSERVE(st.entries[i].id);
    } else {
      kept.push(st.entries[i]);
    }
  }
  st.entries = kept;
  if (kept.length) return;
  var at = OBS_LIVE.indexOf(observer);
  if (at >= 0) OBS_LIVE.splice(at, 1);
  if (!OBS_LIVE.length && OBS_TIMER !== null) {
    if (OBS_CLEAR_INTERVAL) OBS_CLEAR_INTERVAL(OBS_TIMER);
    OBS_TIMER = null;
  }
}

// Two handles on the same entry are different objects with different grant
// tokens (see `uniqueIdOf`), so `unobserve(h)` has to match on the entry, not on
// the object — a page that re-derived its handle would otherwise be unable to
// stop the observation it started. Identity is the fallback for an entry that no
// longer resolves, which is exactly when the unique id is gone.
function sameEntryAs(handle, target) {
  var wanted = uniqueIdOf(target);
  return function(entry) {
    if (entry.handle === handle) return true;
    return wanted != null && entry.uniqueId != null && entry.uniqueId === wanted;
  };
}

function deliver(observer, st, entry) {
  var raw = [];
  if (NAT_FS_POLL) {
    try { raw = JSON.parse(NAT_FS_POLL(entry.id)) || []; } catch (e) { raw = []; }
  }
  if (!raw.length) return;
  var records = [];
  var ended = false;
  for (var i = 0; i < raw.length; i++) {
    records.push(makeChangeRecord(entry.handle, raw[i]));
    if (raw[i].type === 'errored') ended = true;
  }
  // §3: `errored` is the last thing an observation ever reports. The native side
  // has already stopped tracking it, so forget it here too rather than polling a
  // dead id on every tick for the life of the document.
  if (ended) {
    dropEntries(st, observer, function(candidate) { return candidate === entry; });
  }
  try {
    st.callback.call(undefined, records, observer);
  } catch (err) {
    // One page's throwing callback must not take down the tick that every other
    // observer's delivery rides on — but it must not vanish either.
    if (typeof globalThis.reportError === 'function') {
      globalThis.reportError(err);
    } else {
      Promise.reject(err);
    }
  }
}

function pollAllObservations() {
  // A callback is free to call `observe()` / `unobserve()` / `disconnect()`
  // while it runs, so both levels iterate a copy rather than the live array.
  var observers = OBS_LIVE.slice();
  for (var i = 0; i < observers.length; i++) {
    var st = OBS_STATE.get(observers[i]);
    if (st === undefined) continue;
    var entries = st.entries.slice();
    for (var j = 0; j < entries.length; j++) {
      deliver(observers[i], st, entries[j]);
    }
  }
}

FileSystemObserver.prototype.observe = function(handle, options) {
  var self = this;
  return promiseTry(function() {
    var st = observerStateOf(self);
    var target = (handle !== null && typeof handle === 'object') ? STATE.get(handle) : undefined;
    if (target === undefined) {
      throw new TypeError(
        "Failed to execute 'observe' on 'FileSystemObserver': parameter 1 is not of type 'FileSystemHandle'");
    }
    var recursive = !!(options && options.recursive);
    if (!NAT_FS_OBSERVE) fsThrow('NotSupportedError', 'observe() is unavailable');
    var id = NAT_FS_OBSERVE(target.pathId, target.token, recursive);
    if (!id) fsThrow('NotFoundError', 'Cannot observe ' + target.name);
    // §2: observing an entry this observer already observes replaces the earlier
    // observation. Dropped after the new one exists, so a failure above leaves
    // the old observation running instead of silently ending it.
    dropEntries(st, self, sameEntryAs(handle, target));
    st.entries.push({ id: String(id), handle: handle, uniqueId: uniqueIdOf(target) });
    if (OBS_LIVE.indexOf(self) < 0) OBS_LIVE.push(self);
    if (OBS_TIMER === null && OBS_SET_INTERVAL) {
      OBS_TIMER = OBS_SET_INTERVAL(pollAllObservations, OBS_POLL_MS);
    }
  });
};

FileSystemObserver.prototype.unobserve = function(handle) {
  var st = observerStateOf(this);
  var target = (handle !== null && typeof handle === 'object') ? STATE.get(handle) : undefined;
  if (target === undefined) {
    throw new TypeError(
      "Failed to execute 'unobserve' on 'FileSystemObserver': parameter 1 is not of type 'FileSystemHandle'");
  }
  dropEntries(st, this, sameEntryAs(handle, target));
};

FileSystemObserver.prototype.disconnect = function() {
  var st = observerStateOf(this);
  dropEntries(st, this, function() { return true; });
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
  // FS §8.1 "show a x picker" — consume user activation once the check passes
  // (GAP-USERACT).
  if (typeof _lumen_consume_user_activation === 'function') {
    _lumen_consume_user_activation();
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

if (__lumen_C.__lumen_platform_cloners) {
  __lumen_C.__lumen_platform_cloners.register(
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
Object.defineProperty(__lumen_C, '__lumen_fsa_internal', {
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
  window.FileSystemObserver            = FileSystemObserver;
  window.FileSystemChangeRecord        = FileSystemChangeRecord;
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
mod tests_v8;
