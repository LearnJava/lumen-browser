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
//! | Class | Description |
//! |---|---|
//! | `FileSystemFileHandle` | `.name`, `.kind='file'`, `.getFile()`, `.createWritable()` |
//! | `FileSystemDirectoryHandle` | `.name`, `.kind='directory'`, `.entries()`, `.getFileHandle()`, `.getDirectoryHandle()` |
//! | `FileSystemWritableFileStream` | `.write(data)`, `.seek(pos)`, `.truncate(size)`, `.close()` |
//!
//! # Native bindings registered here
//!
//! | Name | Signature | Description |
//! |---|---|---|
//! | `_lumen_show_open_file_picker` | `() → Option<String>` | Open file dialog → JSON `{name,token,size}` or null |
//! | `_lumen_show_save_file_picker` | `(name: String) → Option<String>` | Save dialog → write-handle id or null |
//! | `_lumen_show_directory_picker` | `() → Option<String>` | Directory dialog → JSON `{name,path_id}` or null |
//! | `_lumen_dir_entries` | `(path_id: String) → String` | List directory → JSON `[{name,kind}]` |
//! | `_lumen_dir_get_file` | `(path_id, name, create: bool) → String` | Get/create file → JSON `{name,token,size}` or `{error}` |
//! | `_lumen_dir_get_subdir` | `(path_id, name, create: bool) → String` | Get/create subdir → JSON `{name,path_id}` or `{error}` |
//! | `_lumen_dir_remove_entry` | `(path_id, name, recursive: bool) → String` | Remove entry → `""` or a DOMException name |
//! | `_lumen_fs_resolve` | `(parent_id, child_dir_id, child_token) → Option<String>` | Relative path → JSON `[segment, …]` or null |
//! | `_lumen_writable_write_text` | `(handle_id: String, data: String) → bool` | Append UTF-8 text to writable stream |
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

    fn append_text(&mut self, id: &str, origin: &str, text: &str) -> bool {
        match self.handles.get_mut(id) {
            Some(h) if h.origin == origin => {
                h.data.extend_from_slice(text.as_bytes());
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
        let path_opt = dir_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&path_id, &entries_origin)
            .cloned();
        let Some(dir) = path_opt else { return "[]".to_string() };
        let Ok(rd) = std::fs::read_dir(&dir) else { return "[]".to_string() };
        let mut items = Vec::new();
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let kind = if entry.path().is_dir() { "directory" } else { "file" };
            items.push(format!(r#"{{"name":"{}","kind":"{}"}}"#, json_escape(&name), kind));
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

    let write_origin = origin.to_string();
    let writable_write_text = into_v8_fn2(move |handle_id: String, data: String| -> bool {
        write_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append_text(&handle_id, &write_origin, &data)
    });
    rt.register_native("_lumen_writable_write_text", writable_write_text)?;

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

/// Defines `FileSystemFileHandle`, `FileSystemDirectoryHandle`,
/// `FileSystemWritableFileStream`, and wraps the picker globals as Promise-returning APIs.
///
/// BUG-371 moved the whole shim inside an IIFE. It used to run at top level so
/// its classes were global bindings *and* `window.X`; but the eight natives it
/// calls are deleted from the global object right after install
/// ([`crate::file_input::seal_file_natives_v8`]), so they have to be captured in
/// closure scope first. The classes are still reachable exactly as before —
/// `window.X = X` on a real page makes them globals anyway.
#[cfg(feature = "v8-backend")]
const FSAL_SHIM: &str = r#"
(function() {

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
var NAT_WRITE_TEXT   = nat('_lumen_writable_write_text');
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

// BUG-371 point 4 (same reasoning as `File._token`): a handle's grant id is a
// private slot, not an own property. Left web-visible, `handle._token` /
// `_pathId` / `_id` were the values an attacker needed, and the public
// constructors accepted them straight back.
var FILE_STATE = new WeakMap();   // FileSystemFileHandle       -> {token, size}
var DIR_STATE  = new WeakMap();   // FileSystemDirectoryHandle  -> {pathId}
var WRITE_STATE = new WeakMap();  // FileSystemWritableFileStream -> {id, closed}

// ── FileSystemWritableFileStream ─────────────────────────────────────────────

function FileSystemWritableFileStream(handleId) {
  WRITE_STATE.set(this, { id: String(handleId == null ? '' : handleId), closed: false });
}

FileSystemWritableFileStream.prototype.write = function(data) {
  var st = WRITE_STATE.get(this);
  return Promise.resolve().then(function() {
    if (!st || st.closed) throw new TypeError('FileSystemWritableFileStream is closed');
    var text = (data instanceof ArrayBuffer || ArrayBuffer.isView(data))
      ? new TextDecoder().decode(data)
      : String(data);
    if (NAT_WRITE_TEXT) NAT_WRITE_TEXT(st.id, text);
  });
};

FileSystemWritableFileStream.prototype.seek = function(_pos) {
  return Promise.resolve();
};

FileSystemWritableFileStream.prototype.truncate = function(_size) {
  return Promise.resolve();
};

FileSystemWritableFileStream.prototype.close = function() {
  var st = WRITE_STATE.get(this);
  return Promise.resolve().then(function() {
    if (!st || st.closed) return;
    st.closed = true;
    if (NAT_WRITE_CLOSE) NAT_WRITE_CLOSE(st.id);
  });
};

// ── FileSystemFileHandle ──────────────────────────────────────────────────────

function FileSystemFileHandle(name, token, size) {
  this.name = name;
  this.kind = 'file';
  FILE_STATE.set(this, { token: String(token == null ? '' : token), size: size || 0 });
}

FileSystemFileHandle.prototype.getFile = function() {
  var st = FILE_STATE.get(this) || { token: '', size: 0 };
  var name = this.name;
  return Promise.resolve().then(function() {
    if (FS_INTERNAL) return FS_INTERNAL.makeTokenFile(name, st.token, st.size, '', 0);
    // No bridge (the file-input shim failed to install): hand back a plain,
    // grant-less File rather than inventing a second token path.
    var f = new File([], name, { type: '' });
    f.size = st.size;
    return f;
  });
};

FileSystemFileHandle.prototype.createWritable = function() {
  var suggested = this.name;
  var st = FILE_STATE.get(this) || { token: '', size: 0 };
  return Promise.resolve().then(function() {
    // BUG-372: a handle from the origin's own sandbox writes straight to the
    // file it names. Sending it through the save picker would ask the user to
    // choose a destination for a file they never see, and then write the bytes
    // somewhere other than where the page's own `getFile()` reads them from.
    var sandboxed = (NAT_WRITE_TOKEN && st.token) ? NAT_WRITE_TOKEN(st.token) : null;
    if (sandboxed != null) {
      return new FileSystemWritableFileStream(sandboxed);
    }
    var handleId = NAT_SAVE_PICKER ? NAT_SAVE_PICKER(suggested) : null;
    if (handleId == null) {
      throw new DOMException('Write permission denied or user cancelled', 'NotAllowedError');
    }
    return new FileSystemWritableFileStream(handleId);
  });
};

FileSystemFileHandle.prototype.isSameEntry = function(other) {
  var mine = FILE_STATE.get(this);
  var theirs = (other && typeof other === 'object') ? FILE_STATE.get(other) : undefined;
  return Promise.resolve(
    !!mine && !!theirs && mine.token !== '' && mine.token === theirs.token);
};

// ── FileSystemDirectoryHandle ─────────────────────────────────────────────────

function FileSystemDirectoryHandle(name, pathId) {
  this.name = name;
  this.kind = 'directory';
  DIR_STATE.set(this, { pathId: String(pathId == null ? '' : pathId) });
}

FileSystemDirectoryHandle.prototype.entries = function() {
  var st = DIR_STATE.get(this) || { pathId: '' };
  var raw = [];
  if (NAT_DIR_ENTRIES) {
    try { raw = JSON.parse(NAT_DIR_ENTRIES(st.pathId)) || []; } catch (e) { raw = []; }
  }
  var idx = 0;
  return {
    next: function() {
      if (idx >= raw.length) return Promise.resolve({ done: true, value: undefined });
      var e = raw[idx++];
      // Phase 1: listed entries carry no grant of their own — the caller has to
      // re-request them by name through getFileHandle/getDirectoryHandle.
      var handle = e.kind === 'directory'
        ? new FileSystemDirectoryHandle(e.name, '')
        : new FileSystemFileHandle(e.name, '', 0);
      return Promise.resolve({ done: false, value: [e.name, handle] });
    },
    [Symbol.asyncIterator || '_asyncIter']: function() { return this; },
  };
};

FileSystemDirectoryHandle.prototype.values = function() {
  var it = this.entries();
  return {
    next: function() {
      return it.next().then(function(r) {
        if (r.done) return r;
        return { done: false, value: r.value[1] };
      });
    },
    [Symbol.asyncIterator || '_asyncIter']: function() { return this; },
  };
};

FileSystemDirectoryHandle.prototype.keys = function() {
  var it = this.entries();
  return {
    next: function() {
      return it.next().then(function(r) {
        if (r.done) return r;
        return { done: false, value: r.value[0] };
      });
    },
    [Symbol.asyncIterator || '_asyncIter']: function() { return this; },
  };
};

FileSystemDirectoryHandle.prototype.getFileHandle = function(name, opts) {
  var st = DIR_STATE.get(this) || { pathId: '' };
  var create = !!(opts && opts.create);
  var entryName = String(name);
  return Promise.resolve().then(function() {
    var raw = NAT_DIR_GET_FILE ? NAT_DIR_GET_FILE(st.pathId, entryName, create) : null;
    var p = fsUnwrap(raw, 'Cannot open file: ' + entryName);
    return new FileSystemFileHandle(p.name, p.token, p.size);
  });
};

FileSystemDirectoryHandle.prototype.getDirectoryHandle = function(name, opts) {
  var st = DIR_STATE.get(this) || { pathId: '' };
  var create = !!(opts && opts.create);
  var entryName = String(name);
  return Promise.resolve().then(function() {
    var raw = NAT_DIR_GET_SUB ? NAT_DIR_GET_SUB(st.pathId, entryName, create) : null;
    var p = fsUnwrap(raw, 'Cannot open directory: ' + entryName);
    return new FileSystemDirectoryHandle(p.name, p.path_id);
  });
};

FileSystemDirectoryHandle.prototype.removeEntry = function(name, opts) {
  var st = DIR_STATE.get(this) || { pathId: '' };
  var recursive = !!(opts && opts.recursive);
  var entryName = String(name);
  return Promise.resolve().then(function() {
    // BUG-372: this used to resolve without removing anything, so a caller
    // could not tell a deletion from a no-op. It now either removes the entry
    // or says which rule stopped it.
    if (!NAT_DIR_REMOVE) {
      throw new DOMException('removeEntry is unavailable', 'NotSupportedError');
    }
    var err = NAT_DIR_REMOVE(st.pathId, entryName, recursive);
    if (err) fsThrow(err, 'Cannot remove entry: ' + entryName);
  });
};

// FS §5.2: `[]` for the handle itself, the path segments for a descendant,
// `null` for anything else.
FileSystemDirectoryHandle.prototype.resolve = function(possibleDescendant) {
  var st = DIR_STATE.get(this) || { pathId: '' };
  var child = (possibleDescendant && typeof possibleDescendant === 'object')
    ? possibleDescendant : null;
  var childDir  = child ? DIR_STATE.get(child) : undefined;
  var childFile = child ? FILE_STATE.get(child) : undefined;
  return Promise.resolve().then(function() {
    if (!NAT_FS_RESOLVE) return null;
    var raw = NAT_FS_RESOLVE(
      st.pathId,
      childDir ? childDir.pathId : '',
      childFile ? childFile.token : '');
    return raw == null ? null : JSON.parse(raw);
  });
};

FileSystemDirectoryHandle.prototype.isSameEntry = function(other) {
  var mine = DIR_STATE.get(this);
  var theirs = (other && typeof other === 'object') ? DIR_STATE.get(other) : undefined;
  return Promise.resolve(
    !!mine && !!theirs && mine.pathId !== '' && mine.pathId === theirs.pathId);
};

// ── Picker globals (Promise-returning per spec §5.1) ─────────────────────────

function showOpenFilePicker(_options) {
  return Promise.resolve().then(function() {
    var info = NAT_OPEN_PICKER ? NAT_OPEN_PICKER() : null;
    if (info == null) {
      throw new DOMException('The user aborted a request.', 'AbortError');
    }
    var p = JSON.parse(info);
    return [new FileSystemFileHandle(p.name, p.token, p.size)];
  });
}

function showSaveFilePicker(options) {
  return Promise.resolve().then(function() {
    var suggested = (options && options.suggestedName) ? options.suggestedName : 'file.txt';
    var handleId = NAT_SAVE_PICKER ? NAT_SAVE_PICKER(suggested) : null;
    if (handleId == null) {
      throw new DOMException('The user aborted a request.', 'AbortError');
    }
    var handle = new FileSystemFileHandle(suggested, '', 0);
    handle.createWritable = function() {
      return Promise.resolve(new FileSystemWritableFileStream(handleId));
    };
    return handle;
  });
}

function showDirectoryPicker(_options) {
  return Promise.resolve().then(function() {
    var info = NAT_DIR_PICKER ? NAT_DIR_PICKER() : null;
    if (info == null) {
      throw new DOMException('The user aborted a request.', 'AbortError');
    }
    var p = JSON.parse(info);
    return new FileSystemDirectoryHandle(p.name, p.path_id);
  });
}

// ── Expose on window if available ────────────────────────────────────────────

if (typeof window !== 'undefined') {
  window.FileSystemFileHandle         = FileSystemFileHandle;
  window.FileSystemDirectoryHandle    = FileSystemDirectoryHandle;
  window.FileSystemWritableFileStream = FileSystemWritableFileStream;
  window.showOpenFilePicker           = showOpenFilePicker;
  window.showSaveFilePicker           = showSaveFilePicker;
  window.showDirectoryPicker          = showDirectoryPicker;
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
        super::write_reg().lock().unwrap().append_text(&handle_id, TEST_ORIGIN, "hello");
        super::write_reg().lock().unwrap().append_text(&handle_id, TEST_ORIGIN, " world");
        super::write_reg().lock().unwrap().close(&handle_id, TEST_ORIGIN);
        let content = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert_eq!(content, "hello world");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn writable_close_writes_file() {
        let tmp = std::env::temp_dir().join("lumen_fsal_close_test.txt");
        let handle_id = super::write_reg().lock().unwrap().allocate(tmp.clone(), TEST_ORIGIN);
        super::write_reg().lock().unwrap().append_text(&handle_id, TEST_ORIGIN, "lumen test content");
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
            !super::write_reg().lock().unwrap().append_text(&handle_id, "https://evil.example", "pwn"),
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
    const STUBS: &str = r#"
        var window = globalThis;
        function Blob(blobParts, options) {}
        function _lumen_set_attr(nid, name, val) {}
        function _lumen_get_attr(nid, name) { return undefined; }
        function _lumen_dispatch_bubble(nid, type) {}
        function _lumen_make_element(nid) { return {__nid__: nid}; }
        window._lumen_make_element = _lumen_make_element;
    "#;

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

    /// Resolve `expr` (a Promise) and read the settled value back. V8 drains
    /// pending microtasks at the end of each `eval`, so the second call sees the
    /// result of the first (S12b-B8 finding).
    fn await_bool(rt: &V8JsRuntime, expr: &str) -> bool {
        rt.eval(&format!("var __r = null; ({expr}).then(function(v) {{ __r = v; }});"))
            .unwrap();
        matches!(rt.eval("__r").unwrap(), JsValue::Bool(true))
    }

    #[test]
    fn fsfh_kind_is_file() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "new FileSystemFileHandle('a.txt', '', 0).kind === 'file'"
        ));
    }

    #[test]
    fn fsfh_exposes_name() {
        let rt = with_fsa();
        let r = rt
            .eval("new FileSystemFileHandle('hello.txt', '', 0).name")
            .unwrap();
        assert_eq!(r, JsValue::String("hello.txt".into()));
    }

    #[test]
    fn fsdh_kind_is_directory() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "new FileSystemDirectoryHandle('docs', 'x').kind === 'directory'"
        ));
    }

    #[test]
    fn fsdh_exposes_name() {
        let rt = with_fsa();
        let r = rt
            .eval("new FileSystemDirectoryHandle('docs', 'x').name")
            .unwrap();
        assert_eq!(r, JsValue::String("docs".into()));
    }

    #[test]
    fn fsws_write_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemWritableFileStream('x').write('x').then === 'function'"
        ));
    }

    #[test]
    fn fsws_seek_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemWritableFileStream('x').seek(0).then === 'function'"
        ));
    }

    #[test]
    fn fsws_truncate_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemWritableFileStream('x').truncate(0).then === 'function'"
        ));
    }

    #[test]
    fn fsws_close_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemWritableFileStream('x').close().then === 'function'"
        ));
    }

    #[test]
    fn fsfh_get_file_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemFileHandle('a.txt', '', 0).getFile().then === 'function'"
        ));
    }

    #[test]
    fn fsdh_get_file_handle_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemDirectoryHandle('d', 'x').getFileHandle('x.txt').then === 'function'"
        ));
    }

    #[test]
    fn fsdh_get_dir_handle_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemDirectoryHandle('d', 'x').getDirectoryHandle('sub').then === 'function'"
        ));
    }

    #[test]
    fn fsdh_entries_has_next() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemDirectoryHandle('d', 'x').entries().next === 'function'"
        ));
    }

    #[test]
    fn fsdh_values_has_next() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemDirectoryHandle('d', 'x').values().next === 'function'"
        ));
    }

    #[test]
    fn fsdh_keys_has_next() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemDirectoryHandle('d', 'x').keys().next === 'function'"
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
    fn fsfh_is_same_entry_same_token() {
        let rt = with_fsa();
        assert!(await_bool(
            &rt,
            "new FileSystemFileHandle('a.txt', 'tok', 0)\
             .isSameEntry(new FileSystemFileHandle('b.txt', 'tok', 0))"
        ));
    }

    #[test]
    fn fsfh_is_same_entry_diff_token() {
        let rt = with_fsa();
        assert!(!await_bool(
            &rt,
            "new FileSystemFileHandle('a.txt', 'one', 0)\
             .isSameEntry(new FileSystemFileHandle('b.txt', 'two', 0))"
        ));
    }

    #[test]
    fn fsdh_is_same_entry_same_path_id() {
        let rt = with_fsa();
        assert!(await_bool(
            &rt,
            "new FileSystemDirectoryHandle('x', 'pid')\
             .isSameEntry(new FileSystemDirectoryHandle('y', 'pid'))"
        ));
    }

    /// BUG-371 point 4: handle internals are private slots — the page can read
    /// neither the read token nor the directory/write ids off a handle.
    #[test]
    fn handle_internals_are_not_web_visible() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "var f = new FileSystemFileHandle('a.txt', 'tok', 1); \
             var d = new FileSystemDirectoryHandle('d', 'pid'); \
             var w = new FileSystemWritableFileStream('wid'); \
             f._token === undefined && d._pathId === undefined && w._id === undefined && \
             Object.getOwnPropertyNames(f).join(',') === 'name,kind' && \
             Object.getOwnPropertyNames(d).join(',') === 'name,kind' && \
             Object.getOwnPropertyNames(w).length === 0"
        ));
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
            var __i = JSON.parse(_lumen_dir_get_file('{pid}', 'out.txt', true));
            var __w = null;
            new FileSystemFileHandle(__i.name, __i.token, __i.size)
              .createWritable().then(function(s) {{ __w = s; }});
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
    #[test]
    fn refused_write_permission_rejects_with_not_allowed_error() {
        let rt = with_cancelling_pickers("https://abort-write.fsal.test");
        assert_eq!(
            await_rejection(&rt, "new FileSystemFileHandle('x.txt', '', 0).createWritable()"),
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
                &format!("new FileSystemDirectoryHandle('root', '{pid}').getFileHandle('nope.txt')")
            ),
            "true|NotFoundError|Cannot open file: nope.txt"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
