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
//! | `_lumen_dir_get_file` | `(path_id: String, name: String) → Option<String>` | Get file in dir → JSON `{name,token,size}` or null |
//! | `_lumen_dir_get_subdir` | `(path_id: String, name: String) → Option<String>` | Get subdir → JSON `{name,path_id}` or null |
//! | `_lumen_writable_write_text` | `(handle_id: String, data: String) → bool` | Append UTF-8 text to writable stream |
//! | `_lumen_writable_close` | `(handle_id: String) → bool` | Flush and close writable stream |
//!
//! All eight are deleted from the global object by
//! [`crate::file_input::seal_file_natives_v8`] once the shim below has captured
//! them into closure variables.

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
    fn allocate(&mut self, path: PathBuf, origin: &str) -> String {
        let Some(id) = crate::file_input::new_grant_id() else {
            return String::new();
        };
        self.paths.insert(id.clone(), DirGrant { origin: origin.to_string(), path });
        id
    }

    fn get(&self, id: &str, origin: &str) -> Option<&PathBuf> {
        match self.paths.get(id) {
            Some(g) if g.origin == origin => Some(&g.path),
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
fn file_entry_json(path: &std::path::Path, origin: &str) -> Option<String> {
    let name  = path.file_name()?.to_str()?.to_string();
    let size  = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let token = crate::file_input::register_file_token(path.to_str()?, origin);
    Some(format!(
        r#"{{"name":"{}","token":"{}","size":{}}}"#,
        json_escape(&name),
        json_escape(&token),
        size
    ))
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
    use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn2};
    use lumen_core::ext::JsRuntime as _;

    // Reload of the same origin: nothing the previous document was granted
    // stays reachable, including a save-handle it left open.
    dir_reg().lock().unwrap_or_else(|e| e.into_inner()).revoke_origin(origin);
    write_reg().lock().unwrap_or_else(|e| e.into_inner()).revoke_origin(origin);

    let open_origin = origin.to_string();
    let open_picker = into_v8_fn0(move || -> Option<String> {
        let path = os_open_file_picker()?;
        file_entry_json(&path, &open_origin)
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
        let id = dir_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .allocate(path, &dir_pick_origin);
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
    let dir_get_file = into_v8_fn2(move |path_id: String, name: String| -> Option<String> {
        let dir = dir_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&path_id, &get_file_origin)
            .cloned()?;
        let file_path = dir.join(&name);
        if !file_path.is_file() {
            return None;
        }
        file_entry_json(&file_path, &get_file_origin)
    });
    rt.register_native("_lumen_dir_get_file", dir_get_file)?;

    let subdir_origin = origin.to_string();
    let dir_get_subdir = into_v8_fn2(move |path_id: String, name: String| -> Option<String> {
        let parent = dir_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&path_id, &subdir_origin)
            .cloned()?;
        let sub = parent.join(&name);
        if !sub.is_dir() {
            return None;
        }
        let sub_name = json_escape(&name);
        let sub_id = dir_reg()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .allocate(sub, &subdir_origin);
        if sub_id.is_empty() {
            return None;
        }
        Some(format!(
            r#"{{"name":"{}","path_id":"{}"}}"#,
            sub_name,
            json_escape(&sub_id)
        ))
    });
    rt.register_native("_lumen_dir_get_subdir", dir_get_subdir)?;

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
var NAT_WRITE_TEXT   = nat('_lumen_writable_write_text');
var NAT_WRITE_CLOSE  = nat('_lumen_writable_close');

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
  return Promise.resolve().then(function() {
    var handleId = NAT_SAVE_PICKER ? NAT_SAVE_PICKER(suggested) : null;
    if (handleId == null) {
      throw new DOMException('NotAllowedError', 'Write permission denied or user cancelled');
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
  return Promise.resolve().then(function() {
    var info = NAT_DIR_GET_FILE ? NAT_DIR_GET_FILE(st.pathId, name) : null;
    if (info == null) {
      if (opts && opts.create) {
        throw new DOMException('NotSupportedError', 'create not supported in Phase 1');
      }
      throw new DOMException('NotFoundError', 'File not found: ' + name);
    }
    var p = JSON.parse(info);
    return new FileSystemFileHandle(p.name, p.token, p.size);
  });
};

FileSystemDirectoryHandle.prototype.getDirectoryHandle = function(name, opts) {
  var st = DIR_STATE.get(this) || { pathId: '' };
  return Promise.resolve().then(function() {
    var info = NAT_DIR_GET_SUB ? NAT_DIR_GET_SUB(st.pathId, name) : null;
    if (info == null) {
      if (opts && opts.create) {
        throw new DOMException('NotSupportedError', 'create not supported in Phase 1');
      }
      throw new DOMException('NotFoundError', 'Directory not found: ' + name);
    }
    var p = JSON.parse(info);
    return new FileSystemDirectoryHandle(p.name, p.path_id);
  });
};

FileSystemDirectoryHandle.prototype.removeEntry = function(_name, _opts) {
  return Promise.reject(new DOMException('NotSupportedError', 'removeEntry not supported in Phase 1'));
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
      throw new DOMException('AbortError', 'The user aborted a request.');
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
      throw new DOMException('AbortError', 'The user aborted a request.');
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
      throw new DOMException('AbortError', 'The user aborted a request.');
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
        let id = super::dir_reg().lock().unwrap().allocate(tmp, "https://honest.example");
        assert!(super::dir_reg().lock().unwrap().get(&id, "https://honest.example").is_some());
        assert!(super::dir_reg().lock().unwrap().get(&id, "https://evil.example").is_none());
    }

    /// BUG-371: ids were `1, 2, 3, …`, so `_lumen_dir_entries(1)` listed
    /// whatever directory the user had last granted.
    #[test]
    fn registry_ids_are_unguessable() {
        let tmp = std::env::temp_dir();
        let id = super::dir_reg().lock().unwrap().allocate(tmp.clone(), TEST_ORIGIN);
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
        let keep = super::dir_reg().lock().unwrap().allocate(tmp.clone(), "https://keep.example");
        let drop = super::dir_reg().lock().unwrap().allocate(tmp, "https://drop.example");
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
        let json_opt = super::file_entry_json(&tmp, TEST_ORIGIN);
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
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(STUBS).unwrap();
        crate::file_input::install_file_input_bindings_v8(&rt, TEST_ORIGIN).unwrap();
        super::install_filesystem_access_v8(&rt, TEST_ORIGIN).unwrap();
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
        let pid = super::dir_reg().lock().unwrap().allocate(tmp, TEST_ORIGIN);
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
}
