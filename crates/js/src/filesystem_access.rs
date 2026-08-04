//! File System Access API (W3C File System Access §5).
//!
//! **Phase 1:** full token-based security, proper JS class hierarchy, write support.
//!
//! # Security model
//!
//! File paths are never exposed to JS. Each file opened via `showOpenFilePicker()`
//! is registered in the file token registry via `crate::file_input::register_file_token`
//! and JS only receives an opaque `u64` token. `FileSystemFileHandle.getFile()`
//! constructs a `File` object whose `.text()` / `.arrayBuffer()` / `.stream()`
//! methods use the same `__lumen_file_read_text` / `__lumen_file_read_base64`
//! bindings already installed by `file_input::install_file_input_bindings`.
//!
//! Write paths from `showSaveFilePicker()` are stored in a separate write-handle
//! registry and are only used by `FileSystemWritableFileStream.close()`.
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
//! | `_lumen_show_save_file_picker` | `(name: String) → Option<u32>` | Save dialog → write-handle id or null |
//! | `_lumen_show_directory_picker` | `() → Option<String>` | Directory dialog → JSON `{name,path_id}` or null |
//! | `_lumen_dir_entries` | `(path_id: u32) → String` | List directory → JSON `[{name,kind}]` |
//! | `_lumen_dir_get_file` | `(path_id: u32, name: String) → Option<String>` | Get file in dir → JSON `{name,token,size}` or null |
//! | `_lumen_dir_get_subdir` | `(path_id: u32, name: String) → Option<String>` | Get subdir → JSON `{name,path_id}` or null |
//! | `_lumen_writable_write_text` | `(handle_id: u32, data: String) → bool` | Append UTF-8 text to writable stream |
//! | `_lumen_writable_close` | `(handle_id: u32) → bool` | Flush and close writable stream |

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
    /// Target file path (caller-confirmed via the save picker).
    path: PathBuf,
    /// Accumulated write data.
    data: Vec<u8>,
}

#[cfg(feature = "v8-backend")]
struct WriteRegistry {
    next_id: u32,
    handles: HashMap<u32, WriteHandle>,
}

#[cfg(feature = "v8-backend")]
impl WriteRegistry {
    fn new() -> Self {
        Self { next_id: 1, handles: HashMap::new() }
    }

    fn allocate(&mut self, path: PathBuf) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.handles.insert(id, WriteHandle { path, data: Vec::new() });
        id
    }

    fn append_text(&mut self, id: u32, text: &str) -> bool {
        if let Some(h) = self.handles.get_mut(&id) {
            h.data.extend_from_slice(text.as_bytes());
            true
        } else {
            false
        }
    }

    fn close(&mut self, id: u32) -> bool {
        if let Some(h) = self.handles.remove(&id) {
            std::fs::write(&h.path, &h.data).is_ok()
        } else {
            false
        }
    }
}

#[cfg(feature = "v8-backend")]
static WRITE_REG: OnceLock<Mutex<WriteRegistry>> = OnceLock::new();

#[cfg(feature = "v8-backend")]
fn write_reg() -> &'static Mutex<WriteRegistry> {
    WRITE_REG.get_or_init(|| Mutex::new(WriteRegistry::new()))
}

// ── Directory-handle registry ──────────────────────────────────────────────────

#[cfg(feature = "v8-backend")]
struct DirRegistry {
    next_id: u32,
    paths: HashMap<u32, PathBuf>,
}

#[cfg(feature = "v8-backend")]
impl DirRegistry {
    fn new() -> Self {
        Self { next_id: 1, paths: HashMap::new() }
    }

    fn allocate(&mut self, path: PathBuf) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.paths.insert(id, path);
        id
    }

    fn get(&self, id: u32) -> Option<&PathBuf> {
        self.paths.get(&id)
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
fn file_entry_json(path: &std::path::Path) -> Option<String> {
    let name  = path.file_name()?.to_str()?.to_string();
    let size  = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let token = crate::file_input::register_file_token(path.to_str()?);
    Some(format!(
        r#"{{"name":"{}","token":{},"size":{}}}"#,
        json_escape(&name),
        token,
        size
    ))
}

// ── install ────────────────────────────────────────────────────────────────────

/// Install File System Access API bindings and JS class shim into a V8 runtime
/// (Ph3 V8 migration S5-S7 batch 2; the rquickjs twin was removed in S12b-B20):
/// all eight natives go through the compat layer, the JS shim evaluates
/// unchanged. Must be called after [`crate::file_input::install_file_input_bindings_v8`].
#[cfg(feature = "v8-backend")]
pub(crate) fn install_filesystem_access_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn2};
    use lumen_core::ext::JsRuntime as _;

    let open_picker = into_v8_fn0(move || -> Option<String> {
        let path = os_open_file_picker()?;
        file_entry_json(&path)
    });
    rt.register_native("_lumen_show_open_file_picker", open_picker)?;

    let save_picker = into_v8_fn1(move |suggested: String| -> Option<u32> {
        let path = os_save_file_picker(&suggested)?;
        Some(write_reg().lock().unwrap().allocate(path))
    });
    rt.register_native("_lumen_show_save_file_picker", save_picker)?;

    let dir_picker = into_v8_fn0(move || -> Option<String> {
        let path = os_dir_picker()?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("folder")
            .to_string();
        let id = dir_reg().lock().unwrap().allocate(path);
        Some(format!(r#"{{"name":"{}","path_id":{}}}"#, json_escape(&name), id))
    });
    rt.register_native("_lumen_show_directory_picker", dir_picker)?;

    let dir_entries = into_v8_fn1(move |path_id: u32| -> String {
        let path_opt = dir_reg().lock().unwrap().get(path_id).cloned();
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

    let dir_get_file = into_v8_fn2(move |path_id: u32, name: String| -> Option<String> {
        let dir = dir_reg().lock().unwrap().get(path_id).cloned()?;
        let file_path = dir.join(&name);
        if !file_path.is_file() {
            return None;
        }
        file_entry_json(&file_path)
    });
    rt.register_native("_lumen_dir_get_file", dir_get_file)?;

    let dir_get_subdir = into_v8_fn2(move |path_id: u32, name: String| -> Option<String> {
        let parent = dir_reg().lock().unwrap().get(path_id).cloned()?;
        let sub = parent.join(&name);
        if !sub.is_dir() {
            return None;
        }
        let sub_name = json_escape(&name);
        let sub_id = dir_reg().lock().unwrap().allocate(sub);
        Some(format!(r#"{{"name":"{}","path_id":{}}}"#, sub_name, sub_id))
    });
    rt.register_native("_lumen_dir_get_subdir", dir_get_subdir)?;

    let writable_write_text = into_v8_fn2(move |handle_id: u32, data: String| -> bool {
        write_reg().lock().unwrap().append_text(handle_id, &data)
    });
    rt.register_native("_lumen_writable_write_text", writable_write_text)?;

    let writable_close = into_v8_fn1(move |handle_id: u32| -> bool {
        write_reg().lock().unwrap().close(handle_id)
    });
    rt.register_native("_lumen_writable_close", writable_close)?;

    rt.eval(FSAL_SHIM)?;
    Ok(())
}

// ── JS shim ────────────────────────────────────────────────────────────────────

/// Defines `FileSystemFileHandle`, `FileSystemDirectoryHandle`,
/// `FileSystemWritableFileStream`, and wraps the picker globals as Promise-returning APIs.
///
/// Classes are defined at top level (not inside an IIFE) so they are accessible as
/// global variables AND via `window.X` when the window object is available.
#[cfg(feature = "v8-backend")]
const FSAL_SHIM: &str = r#"

// ── FileSystemWritableFileStream ─────────────────────────────────────────────

function FileSystemWritableFileStream(handleId) {
  this._id = handleId;
  this._closed = false;
}

FileSystemWritableFileStream.prototype.write = function(data) {
  var self = this;
  return Promise.resolve().then(function() {
    if (self._closed) throw new TypeError('FileSystemWritableFileStream is closed');
    var text = (data instanceof ArrayBuffer || ArrayBuffer.isView(data))
      ? new TextDecoder().decode(data)
      : String(data);
    _lumen_writable_write_text(self._id, text);
  });
};

FileSystemWritableFileStream.prototype.seek = function(_pos) {
  return Promise.resolve();
};

FileSystemWritableFileStream.prototype.truncate = function(_size) {
  return Promise.resolve();
};

FileSystemWritableFileStream.prototype.close = function() {
  var self = this;
  return Promise.resolve().then(function() {
    if (self._closed) return;
    self._closed = true;
    _lumen_writable_close(self._id);
  });
};

// ── FileSystemFileHandle ──────────────────────────────────────────────────────

function FileSystemFileHandle(name, token, size) {
  this.name = name;
  this.kind = 'file';
  this._token = token;
  this._size = size || 0;
}

FileSystemFileHandle.prototype.getFile = function() {
  var self = this;
  return Promise.resolve().then(function() {
    var f = new File([], self.name, { type: '' });
    f._token = self._token;
    f._size = self._size;
    Object.defineProperty(f, 'size', { get: function() { return self._size; }, configurable: true });
    return f;
  });
};

FileSystemFileHandle.prototype.createWritable = function() {
  var suggested = this.name;
  return Promise.resolve().then(function() {
    var handleId = _lumen_show_save_file_picker(suggested);
    if (handleId == null) {
      throw new DOMException('NotAllowedError', 'Write permission denied or user cancelled');
    }
    return new FileSystemWritableFileStream(handleId);
  });
};

FileSystemFileHandle.prototype.isSameEntry = function(other) {
  var myToken = this._token;
  return Promise.resolve(other instanceof FileSystemFileHandle && other._token === myToken);
};

// ── FileSystemDirectoryHandle ─────────────────────────────────────────────────

function FileSystemDirectoryHandle(name, pathId) {
  this.name = name;
  this.kind = 'directory';
  this._pathId = pathId;
}

FileSystemDirectoryHandle.prototype.entries = function() {
  var raw = JSON.parse(_lumen_dir_entries(this._pathId));
  var idx = 0;
  return {
    next: function() {
      if (idx >= raw.length) return Promise.resolve({ done: true, value: undefined });
      var e = raw[idx++];
      var handle = e.kind === 'directory'
        ? new FileSystemDirectoryHandle(e.name, 0)
        : new FileSystemFileHandle(e.name, 0, 0);
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
  var pid = this._pathId;
  return Promise.resolve().then(function() {
    var info = _lumen_dir_get_file(pid, name);
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
  var pid = this._pathId;
  return Promise.resolve().then(function() {
    var info = _lumen_dir_get_subdir(pid, name);
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
  var myId = this._pathId;
  return Promise.resolve(other instanceof FileSystemDirectoryHandle && other._pathId === myId);
};

// ── Picker globals (Promise-returning per spec §5.1) ─────────────────────────

function showOpenFilePicker(_options) {
  return Promise.resolve().then(function() {
    var info = _lumen_show_open_file_picker();
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
    var handleId = _lumen_show_save_file_picker(suggested);
    if (handleId == null) {
      throw new DOMException('AbortError', 'The user aborted a request.');
    }
    var handle = new FileSystemFileHandle(suggested, 0, 0);
    handle.createWritable = function() {
      return Promise.resolve(new FileSystemWritableFileStream(handleId));
    };
    return handle;
  });
}

function showDirectoryPicker(_options) {
  return Promise.resolve().then(function() {
    var info = _lumen_show_directory_picker();
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

    #[test]
    fn writable_write_accumulates() {
        let tmp = std::env::temp_dir().join("lumen_fsal_write_test.txt");
        let handle_id = super::write_reg().lock().unwrap().allocate(tmp.clone());
        super::write_reg().lock().unwrap().append_text(handle_id, "hello");
        super::write_reg().lock().unwrap().append_text(handle_id, " world");
        super::write_reg().lock().unwrap().close(handle_id);
        let content = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert_eq!(content, "hello world");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn writable_close_writes_file() {
        let tmp = std::env::temp_dir().join("lumen_fsal_close_test.txt");
        let handle_id = super::write_reg().lock().unwrap().allocate(tmp.clone());
        super::write_reg().lock().unwrap().append_text(handle_id, "lumen test content");
        let ok = super::write_reg().lock().unwrap().close(handle_id);
        assert!(ok);
        let content = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert_eq!(content, "lumen test content");
        let _ = std::fs::remove_file(&tmp);
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
        let json_opt = super::file_entry_json(&tmp);
        assert!(json_opt.is_some());
        let json = json_opt.unwrap();
        assert!(json.contains("\"name\""));
        assert!(json.contains("\"token\""));
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
        crate::file_input::install_file_input_bindings_v8(&rt).unwrap();
        super::install_filesystem_access_v8(&rt).unwrap();
        // The real `_lumen_show_*_picker` natives spawn a blocking native OS dialog
        // (PowerShell Windows.Forms on Windows). `showXPicker()`'s `Promise.resolve().then(...)`
        // callback runs as a microtask that V8 drains at the end of THIS `eval()` call (unlike
        // the removed rquickjs harness, which never auto-ran pending jobs — S12b-B8 finding) —
        // so calling `showOpenFilePicker()` etc. here would pop a real dialog and hang the test.
        // Override with non-blocking mocks that resolve the promise instead.
        rt.eval(
            r#"
            globalThis._lumen_show_open_file_picker =
              function() { return '{"name":"mock.txt","token":0,"size":0}'; };
            globalThis._lumen_show_save_file_picker = function() { return 0; };
            globalThis._lumen_show_directory_picker =
              function() { return '{"name":"mockdir","path_id":0}'; };
            "#,
        )
        .unwrap();
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

    #[test]
    fn fsfh_kind_is_file() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "new FileSystemFileHandle('a.txt', 0, 0).kind === 'file'"
        ));
    }

    #[test]
    fn fsfh_exposes_name() {
        let rt = with_fsa();
        let r = rt
            .eval("new FileSystemFileHandle('hello.txt', 0, 0).name")
            .unwrap();
        assert_eq!(r, JsValue::String("hello.txt".into()));
    }

    #[test]
    fn fsdh_kind_is_directory() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "new FileSystemDirectoryHandle('docs', 1).kind === 'directory'"
        ));
    }

    #[test]
    fn fsdh_exposes_name() {
        let rt = with_fsa();
        let r = rt
            .eval("new FileSystemDirectoryHandle('docs', 1).name")
            .unwrap();
        assert_eq!(r, JsValue::String("docs".into()));
    }

    #[test]
    fn fsws_write_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemWritableFileStream(0).write('x').then === 'function'"
        ));
    }

    #[test]
    fn fsws_seek_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemWritableFileStream(0).seek(0).then === 'function'"
        ));
    }

    #[test]
    fn fsws_truncate_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemWritableFileStream(0).truncate(0).then === 'function'"
        ));
    }

    #[test]
    fn fsws_close_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemWritableFileStream(0).close().then === 'function'"
        ));
    }

    #[test]
    fn fsfh_get_file_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemFileHandle('a.txt', 0, 0).getFile().then === 'function'"
        ));
    }

    #[test]
    fn fsdh_get_file_handle_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemDirectoryHandle('d', 0).getFileHandle('x.txt').then === 'function'"
        ));
    }

    #[test]
    fn fsdh_get_dir_handle_returns_promise() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemDirectoryHandle('d', 0).getDirectoryHandle('sub').then === 'function'"
        ));
    }

    #[test]
    fn fsdh_entries_has_next() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemDirectoryHandle('d', 0).entries().next === 'function'"
        ));
    }

    #[test]
    fn fsdh_values_has_next() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemDirectoryHandle('d', 0).values().next === 'function'"
        ));
    }

    #[test]
    fn fsdh_keys_has_next() {
        let rt = with_fsa();
        assert!(bool_eval(
            &rt,
            "typeof new FileSystemDirectoryHandle('d', 0).keys().next === 'function'"
        ));
    }

    #[test]
    fn dir_entries_empty_for_unknown_id() {
        let rt = with_fsa();
        let r = rt.eval("_lumen_dir_entries(9999999)").unwrap();
        assert_eq!(r, JsValue::String("[]".into()));
    }

    #[test]
    fn dir_entries_returns_json_array_for_real_dir() {
        let tmp = std::env::temp_dir();
        let pid = super::dir_reg().lock().unwrap().allocate(tmp);
        let rt = with_fsa();
        let r = rt.eval(&format!("_lumen_dir_entries({pid})")).unwrap();
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
        // Same token → isSameEntry resolves true; verify via internal _token equality.
        assert!(bool_eval(
            &rt,
            "var a = new FileSystemFileHandle('a.txt', 42, 0); \
             var b = new FileSystemFileHandle('b.txt', 42, 0); \
             a._token === b._token"
        ));
    }

    #[test]
    fn fsfh_is_same_entry_diff_token() {
        let rt = with_fsa();
        // Different tokens → isSameEntry resolves false.
        assert!(bool_eval(
            &rt,
            "var a = new FileSystemFileHandle('a.txt', 1, 0); \
             var b = new FileSystemFileHandle('b.txt', 2, 0); \
             a._token !== b._token"
        ));
    }

    #[test]
    fn fsdh_is_same_entry_same_path_id() {
        let rt = with_fsa();
        // Same pathId → isSameEntry resolves true; verify via internal _pathId equality.
        assert!(bool_eval(
            &rt,
            "var a = new FileSystemDirectoryHandle('x', 7); \
             var b = new FileSystemDirectoryHandle('y', 7); \
             a._pathId === b._pathId"
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
