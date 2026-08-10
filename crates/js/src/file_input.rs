//! `<input type="file">` support — File, FileList classes and OS file-picker delivery.
//!
//! W3C File API §4 (File) + §7 (FileList).
//!
//! **Phase 1 (this file):** `File.text()` / `File.arrayBuffer()` return real byte content
//! for OS-picked files via a secure token registry.  JS never sees raw file paths —
//! the shell registers each selected path (calling `register_file_token`) before
//! delivering the file list; JS only holds an opaque `u64` token.
//!
//! # Security model
//!
//! Tokens are created **only** by `register_file_token` which is called from Rust (the
//! shell's `open_file_picker`).  JS can call `__lumen_file_read_text(token)` or
//! `__lumen_file_read_base64(token)` but those only work for pre-registered tokens —
//! they cannot access arbitrary paths.
//!
//! [BUG-371] made that model actually hold. It previously did not: the two read
//! bindings were plain `window` properties and the token space was the dense
//! integer range starting at 1, shared process-wide across every page and every
//! navigation — so any page could read every file the user had ever picked by
//! enumerating `1, 2, 3, …`. Three things changed:
//!
//! 1. **Unguessable tokens.** A token is 128 bits of OS entropy rendered as
//!    32 hex characters ([`new_grant_id`]), not a counter. It is a JS *string*,
//!    not a number — an `f64` cannot carry 128 bits, and the earlier `u64`
//!    counter could not be widened without silent precision loss.
//! 2. **Grants are bound to the granting origin.** Every registry entry records
//!    the origin it was issued to ([`origin_for_url`]); the read bindings capture
//!    the installing document's origin in Rust at install time (never taken from
//!    a JS argument) and refuse tokens issued to any other origin.
//! 3. **Grants die on navigation.** Installing the bindings for a document
//!    revokes every grant previously issued to that same origin
//!    ([`revoke_grants_for_origin`]), so a token does not outlive the page it
//!    was handed to.
//!
//! On top of that the bindings themselves are removed from the global object
//! once both file-API shims have captured them ([`seal_file_natives_v8`]), and
//! the token lives in a `WeakMap` private slot rather than as a web-visible
//! `File._token` property.
//!
//! # Registered native bindings
//!
//! | Name | Signature | Description |
//! |---|---|---|
//! | `__lumen_file_read_text` | `(token: String) → String` | Read file bytes as UTF-8 (lossy) |
//! | `__lumen_file_read_base64` | `(token: String) → String` | Read file bytes as base64 |
//!
//! Both are deleted from the global object by [`seal_file_natives_v8`] after the
//! shims capture them into closure variables.
//!
//! # Shell wiring (main.rs)
//!
//! 1. `open_file_picker` calls `register_file_token(path, origin)` for each selected
//!    file, where `origin` is [`origin_for_url`] of the page the `<input>` lives in.
//! 2. Tokens are included in the JSON passed to `_lumen_deliver_file_list(nid, json)`.
//! 3. JSON shape: `[{name, token, size, mime_type, last_modified_ms}, ...]`
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

// ── Grant identifiers ─────────────────────────────────────────────────────────

/// Generate an unguessable grant id: 128 bits of OS entropy as 32 hex chars.
///
/// Returns `None` if the OS entropy source fails — callers must then refuse to
/// issue a grant rather than fall back to anything predictable, which is exactly
/// the defect [BUG-371] was about.
pub(crate) fn new_grant_id() -> Option<String> {
    let mut bytes = [0u8; 16];
    if getrandom::getrandom(&mut bytes).is_err() {
        eprintln!("lumen-js: OS entropy unavailable — refusing to issue a file grant");
        return None;
    }
    let mut out = String::with_capacity(32);
    for b in bytes {
        out.push(char::from_digit((b >> 4) as u32, 16)?);
        out.push(char::from_digit((b & 0x0f) as u32, 16)?);
    }
    Some(out)
}

/// Origin string a file grant is bound to, derived from the document's URL.
///
/// For URLs with a real host this is `scheme://host[:port]` — the usual tuple
/// origin. Everything else (`file:`, `data:`, `about:`) has an opaque origin per
/// spec; approximating that with the full URL string is stricter than a shared
/// `"file://"` bucket, so two local pages never inherit each other's grants.
///
/// Both sides of the grant — the shell that registers a path and the JS bindings
/// that redeem the token — must derive the origin through this one function, or
/// every redemption fails.
pub fn origin_for_url(url: &str) -> String {
    match lumen_core::url::Url::parse(url) {
        Ok(u) if !u.host().is_empty() => {
            let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
            format!("{}://{}{}", u.scheme(), u.host().to_ascii_lowercase(), port)
        }
        _ => url.to_string(),
    }
}

/// Origin the most recently installed document bound its file grants to.
///
/// Written by `install_file_input_bindings_v8`, read by the shell — see
/// [`active_document_origin`] for why it exists.
static ACTIVE_ORIGIN: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new(String::new()));

/// Origin the file-API bindings of the current document were installed with.
///
/// The shell registers picked paths from the UI thread and has no handle on the
/// JS runtime, so it cannot ask it directly; re-deriving the origin from
/// `PageSource` instead would duplicate the shell's `page_url` construction and
/// drift from it silently (a mismatch does not fail loudly — every read just
/// returns an empty string). Reading back what the install path actually used
/// keeps the two ends in step by construction.
///
/// One document at a time: `install_dom` runs once per page load (main document
/// and bfcache thaw), and workers install their own bindings through
/// `worker.rs`, not through this path.
pub fn active_document_origin() -> String {
    ACTIVE_ORIGIN.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Record the origin the current document's file-API bindings are bound to.
///
/// Only the V8 install path binds anything, so on a build without that backend
/// there is nothing to record.
#[cfg(feature = "v8-backend")]
fn set_active_document_origin(origin: &str) {
    *ACTIVE_ORIGIN.lock().unwrap_or_else(|e| e.into_inner()) = origin.to_string();
}

// ── File token registry ───────────────────────────────────────────────────────

/// One file-read grant: the path it unlocks and the origin it was issued to.
struct FileGrant {
    /// Origin the grant was issued to — checked on every redemption.
    origin: String,
    /// Absolute path the token unlocks. Never exposed to JS.
    // Only the (v8-gated) read binding redeems a grant; the registry is still
    // written on a backend-less build, so the field is dead there, not unused.
    #[cfg_attr(not(feature = "v8-backend"), allow(dead_code))]
    path: PathBuf,
    /// Whether the grant also allows writing back to `path`.
    ///
    /// False for everything the user picked through a file dialog: those are
    /// read grants, and a save always goes through its own picker so the user
    /// confirms the destination. True only for files inside the origin's own
    /// sandbox (OPFS, [BUG-372]), where no confirmation exists to ask for
    /// because the origin already owns every byte in the tree.
    #[cfg_attr(not(feature = "v8-backend"), allow(dead_code))]
    writable: bool,
}

// The token registry is written by the shell (UI thread, `register_file_token`)
// and read by the JS file-read bindings (which run on the dedicated JS thread
// after B-1). A process-global `Mutex` shares it correctly across both threads;
// a `thread_local` would split it once the runtime moved off the UI thread.
// Being process-global is what made BUG-371 exploitable, so entries now carry
// the origin they belong to and are matched against the reader's own origin.
static FILE_REGISTRY: LazyLock<Mutex<HashMap<String, FileGrant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn registry() -> std::sync::MutexGuard<'static, HashMap<String, FileGrant>> {
    FILE_REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Register a file path for `origin` and return an opaque token for JS access.
///
/// Must be called **from Rust** (shell side) before delivering the file list to JS.
/// The returned token grants read access to this one file *and only from `origin`*
/// — pass the same [`origin_for_url`] value the document's JS runtime was
/// installed with. Returns an empty string if no unguessable token could be
/// generated; that token registers nothing and reads nothing.
pub fn register_file_token(path: &str, origin: &str) -> String {
    register_file_grant(path, origin, false)
}

/// Register a file path for `origin` and return a token that grants **read and
/// write** access to it.
///
/// Only [`crate::filesystem_access`] issues these, and only for paths inside the
/// origin's own OPFS sandbox — see [`FileGrant::writable`] for why a picked file
/// never gets one ([BUG-372]).
#[cfg(feature = "v8-backend")]
pub(crate) fn register_writable_file_token(path: &str, origin: &str) -> String {
    register_file_grant(path, origin, true)
}

/// Shared body of [`register_file_token`] and [`register_writable_file_token`].
fn register_file_grant(path: &str, origin: &str, writable: bool) -> String {
    let Some(token) = new_grant_id() else {
        return String::new();
    };
    registry().insert(
        token.clone(),
        FileGrant { origin: origin.to_string(), path: PathBuf::from(path), writable },
    );
    token
}

/// Path a read grant unlocks, or `None` if `token` was never issued to `origin`.
///
/// Used by `FileSystemDirectoryHandle.resolve()` to compare a child handle
/// against a directory without ever handing either path to JS (BUG-372).
#[cfg(feature = "v8-backend")]
pub(crate) fn path_for_token(token: &str, origin: &str) -> Option<PathBuf> {
    let reg = registry();
    let grant = reg.get(token)?;
    (grant.origin == origin).then(|| grant.path.clone())
}

/// Same as [`path_for_token`], but only for grants that also allow writing.
///
/// Backs `FileSystemFileHandle.createWritable()` on an OPFS handle: inside the
/// origin's own sandbox the write goes straight to the file the handle names,
/// with no save dialog to confirm a destination the user never chose (BUG-372).
#[cfg(feature = "v8-backend")]
pub(crate) fn writable_path_for_token(token: &str, origin: &str) -> Option<PathBuf> {
    let reg = registry();
    let grant = reg.get(token)?;
    (grant.origin == origin && grant.writable).then(|| grant.path.clone())
}

/// Revoke all tokens — should be called when a browsing context is torn down.
pub fn clear_file_registry() {
    registry().clear();
}

/// Revoke every file grant issued to `origin`.
///
/// Called when a document of that origin is (re)loaded: a grant must not outlive
/// the page it was handed to, otherwise the next document on the same origin
/// silently inherits the previous one's file access (BUG-371 point 3).
pub fn revoke_grants_for_origin(origin: &str) {
    registry().retain(|_, g| g.origin != origin);
}

#[cfg(feature = "v8-backend")]
fn read_file_bytes_for_token(token: &str, origin: &str) -> Option<Vec<u8>> {
    let path = {
        let reg = registry();
        let grant = reg.get(token)?;
        if grant.origin != origin {
            return None;
        }
        grant.path.clone()
    };
    std::fs::read(&path).ok()
}

// ── Base64 encoder (no external dependency) ───────────────────────────────────

#[cfg(feature = "v8-backend")]
fn to_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

// ── Public install function ───────────────────────────────────────────────────

/// V8 port of the former rquickjs `install_file_input_bindings` (Ph3 V8 migration
/// S5-S7 batch 2, rquickjs side removed in S12b-B14): both natives go through the
/// compat layer, the JS shim evaluates unchanged.
///
/// Must run after `dom::install_dom_bindings` (needs `_lumen_make_element`,
/// `_lumen_set_attr`, `_lumen_get_attr`, `_lumen_dispatch_bubble`, and `Blob`
/// — `File.prototype` extends `Blob.prototype`).
///
/// `origin` is the installing document's origin ([`origin_for_url`]). It is
/// captured here, in Rust, and compared against every redeemed token — a page
/// can neither read it nor pass a different one (BUG-371 point 3). Installing
/// also revokes any grant left over from the previous document on this origin.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_file_input_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    origin: &str,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::into_v8_fn1;
    use lumen_core::ext::JsRuntime as _;

    revoke_grants_for_origin(origin);
    set_active_document_origin(origin);

    let text_origin = origin.to_string();
    let read_text = into_v8_fn1(move |token: String| -> String {
        read_file_bytes_for_token(&token, &text_origin)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    });
    rt.register_native("__lumen_file_read_text", read_text)?;

    let b64_origin = origin.to_string();
    let read_base64 = into_v8_fn1(move |token: String| -> String {
        read_file_bytes_for_token(&token, &b64_origin)
            .map(|b| to_base64(&b))
            .unwrap_or_default()
    });
    rt.register_native("__lumen_file_read_base64", read_base64)?;

    rt.eval(FILE_INPUT_SHIM)?;
    Ok(())
}

/// Remove the file-API natives and the cross-shim bridge from the global object
/// (BUG-371 point 1).
///
/// Both shims copy the bindings they need into closure variables at install
/// time, so deleting the globals afterwards costs them nothing while taking the
/// whole surface — `__lumen_file_read_*`, every File System Access native and
/// the `__lumen_fs_internal` bridge — out of reach of page script.
///
/// `_lumen_storage_get_directory` is *not* in the list: `navigator.storage`
/// installs later than this pass runs, so [`crate::storage_manager`]'s shim
/// deletes that one itself (BUG-372).
///
/// Called by `install_dom` **after** both `install_file_input_bindings_v8` and
/// `install_filesystem_access_v8`, rather than from the tail of either shim: if
/// one of the two installs fails (they are best-effort, see the `install_v8!`
/// orchestration), the sealing must still happen.
#[cfg(feature = "v8-backend")]
pub(crate) fn seal_file_natives_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SEAL_FILE_NATIVES)?;
    Ok(())
}

/// Deletion list for [`seal_file_natives_v8`]. Kept as one script so a single
/// `eval` covers both modules' natives.
#[cfg(feature = "v8-backend")]
const SEAL_FILE_NATIVES: &str = r#"
(function() {
  var names = [
    '__lumen_file_read_text', '__lumen_file_read_base64', '__lumen_fs_internal',
    '_lumen_show_open_file_picker', '_lumen_show_save_file_picker',
    '_lumen_show_directory_picker', '_lumen_dir_entries',
    '_lumen_dir_get_file', '_lumen_dir_get_subdir', '_lumen_dir_remove_entry',
    '_lumen_fs_resolve',
    '_lumen_writable_write_text', '_lumen_writable_close',
    '_lumen_writable_from_token'
  ];
  for (var i = 0; i < names.length; i++) {
    try { delete globalThis[names[i]]; } catch (e) {}
  }
})();
"#;

#[cfg(feature = "v8-backend")]
const FILE_INPUT_SHIM: &str = r#"
(function() {
'use strict';

// BUG-371 point 1: copy the natives into closure variables now — `install_dom`
// deletes them from the global object right after the two file-API shims are
// installed, so from then on this closure is the only way to reach them.
var NAT_READ_TEXT = (typeof __lumen_file_read_text === 'function') ? __lumen_file_read_text : null;
var NAT_READ_B64  = (typeof __lumen_file_read_base64 === 'function') ? __lumen_file_read_base64 : null;

// BUG-371 point 4: the grant token is a private slot keyed by the File object,
// not a `_token` property. As an own property it was enumerable *and* writable,
// so `new File([], 'x', {_token: n}).text()` was a one-step call into
// `__lumen_file_read_text(n)` — the constructor took `_token` straight out of
// the public options dictionary.
var FILE_TOKENS = new WeakMap();

// ── File class (W3C File API §4) ──────────────────────────────────────────────
function File(bits, name, options) {
  options = options || {};
  this.name = String(name || '');
  // Own writable data properties, not plain assignment: `File.prototype` (below)
  // extends `Blob.prototype`, which defines `size`/`type` as getter-only accessors
  // (backed by `_bytes`/`_type`, unused here) — a bare `this.size = 0` would hit
  // that accessor's missing setter and throw under 'use strict' (this shim's mode).
  Object.defineProperty(this, 'size', { value: 0, writable: true, enumerable: true, configurable: true });
  Object.defineProperty(this, 'type', { value: String(options.type || ''), writable: true, enumerable: true, configurable: true });
  this.lastModified = (typeof options.lastModified === 'number')
    ? options.lastModified
    : (typeof Date !== 'undefined' ? Date.now() : 0);
  // No `_token` handling here on purpose (BUG-371 point 4): the public
  // constructor must not be able to mint a read grant. Tokens reach a File only
  // through `makeTokenFile` below, which the page cannot call once sealed.
  // Phase 0: optionally initialise from string bits
  if (Array.isArray(bits) && bits.length > 0) {
    var joined = bits.join('');
    this._content = joined;
    this.size = joined.length;
  }
}
// W3C File API §4: `interface File : Blob` — BUG-280 made `window.File = ...`
// (below) reach the real global `File`, which surfaced that this token-backed
// File never inherited from Blob, so `fileInput.files[0] instanceof Blob` was
// false. `size`/`type` stay own-properties (set above), shadowing Blob.prototype's
// getters, so this only adds the missing prototype link — no behaviour change.
File.prototype = Object.create(Blob.prototype);
File.prototype.constructor = File;

// File.prototype.text() — W3C File API §4.3
// Returns a Promise resolving to the file's contents as a UTF-8 string.
File.prototype.text = function() {
  var token = FILE_TOKENS.get(this);
  if (typeof token === 'string') {
    if (NAT_READ_TEXT) {
      try {
        return Promise.resolve(NAT_READ_TEXT(token));
      } catch(e) {}
    }
    return Promise.resolve('');
  }
  return Promise.resolve(this._content || '');
};

// File.prototype.arrayBuffer() — W3C File API §4.3
// Returns a Promise resolving to an ArrayBuffer with the raw file bytes.
File.prototype.arrayBuffer = function() {
  var token = FILE_TOKENS.get(this);
  if (typeof token === 'string') {
    if (NAT_READ_B64) {
      try {
        var b64 = NAT_READ_B64(token);
        var bin = (typeof atob === 'function') ? atob(b64) : '';
        var buf = new ArrayBuffer(bin.length);
        var v = new Uint8Array(buf);
        for (var i = 0; i < bin.length; i++) v[i] = bin.charCodeAt(i) & 0xff;
        return Promise.resolve(buf);
      } catch(e) {}
    }
    return Promise.resolve(new ArrayBuffer(0));
  }
  var s = this._content || '';
  var buf = new ArrayBuffer(s.length);
  var v = new Uint8Array(buf);
  for (var i = 0; i < s.length; i++) v[i] = s.charCodeAt(i) & 0xff;
  return Promise.resolve(buf);
};

// File.prototype.stream() — W3C Streams API integration
// Returns a ReadableStream-compatible object that emits a single Uint8Array chunk.
File.prototype.stream = function() {
  var self = this;
  var done = false;
  return {
    getReader: function() {
      return {
        read: function() {
          if (done) return Promise.resolve({ value: undefined, done: true });
          done = true;
          return self.arrayBuffer().then(function(buf) {
            return { value: new Uint8Array(buf), done: false };
          });
        },
        cancel: function() { done = true; return Promise.resolve(); }
      };
    }
  };
};

// File.prototype.slice() — W3C File API §4.4
File.prototype.slice = function(start, end, contentType) {
  return new File([], this.name, {
    type: String(contentType || ''),
    lastModified: this.lastModified
  });
};

window.File = File;

// ── Token-bearing File factory (internal) ────────────────────────────────────
// The only way a read grant gets attached to a File. Used below by
// `_lumen_deliver_file_list` and, through the `__lumen_fs_internal` bridge, by
// the File System Access shim's `FileSystemFileHandle.getFile()`.
function makeTokenFile(name, token, size, type, lastModified) {
  var f = new File([], name, { type: type || '', lastModified: lastModified || 0 });
  f.size = size || 0;
  if (typeof token === 'string' && token) FILE_TOKENS.set(f, token);
  return f;
}

// Cross-shim bridge: `filesystem_access.rs`'s shim is a separate script in the
// same context and has no other way to reach `FILE_TOKENS`. Non-enumerable and
// configurable — `install_dom` deletes it once that shim has captured it, so the
// page never observes it (BUG-371 point 1).
Object.defineProperty(globalThis, '__lumen_fs_internal', {
  value: { makeTokenFile: makeTokenFile },
  writable: true, enumerable: false, configurable: true
});

// ── FileList class (W3C File API §7) ─────────────────────────────────────────
function FileList(files) {
  this._files = files || [];
  this.length = this._files.length;
  for (var i = 0; i < this._files.length; i++) this[i] = this._files[i];
}
FileList.prototype.item = function(index) {
  var f = this._files[index];
  return f !== undefined ? f : null;
};
if (typeof Symbol !== 'undefined' && Symbol.iterator) {
  FileList.prototype[Symbol.iterator] = function() {
    var arr = this._files, i = 0;
    return { next: function() {
      return i < arr.length ? { value: arr[i++], done: false }
                            : { value: undefined, done: true };
    }};
  };
}
window.FileList = FileList;

// ── nid → FileList map (persists across _lumen_make_element calls) ────────────
window._lumen_file_lists = {};

// ── Deliver from shell after OS dialog closes ─────────────────────────────────
// Called via eval_js: _lumen_deliver_file_list(nid, '[{name,token,size,...}]')
// Shell registers paths via lumen_js::file_input::register_file_token() first,
// so filesJson carries opaque 128-bit hex tokens rather than raw path strings.
// A page calling this itself gains nothing: a token it did not receive is
// unguessable, and one from another origin is rejected Rust-side (BUG-371).
window._lumen_deliver_file_list = function(nid, filesJson) {
  var infos;
  try { infos = JSON.parse(filesJson); } catch(e) { infos = []; }
  if (!Array.isArray(infos)) infos = [];

  var objs = infos.map(function(f) {
    return makeTokenFile(
      f.name || '', f.token, f.size || 0, f.mime_type || '', f.last_modified_ms || 0);
  });

  window._lumen_file_lists[nid] = new FileList(objs);

  // Sync value attribute (HTML LS §4.10.5.1.16.3 — display name only)
  _lumen_set_attr(nid, 'value', objs.length > 0 ? objs[0].name : '');

  // Dispatch input + change events (bubbling, trusted)
  _lumen_dispatch_bubble(nid, 'input');
  _lumen_dispatch_bubble(nid, 'change');
};

// ── Patch _lumen_make_element to expose .files on <input type="file"> ─────────
var _origMakeElement = _lumen_make_element;
window._lumen_make_element = function(nid) {
  var el = _origMakeElement(nid);
  if (el && _lumen_get_attr(nid, 'type') === 'file') {
    Object.defineProperty(el, 'files', {
      get: function() {
        return window._lumen_file_lists[nid] || new FileList([]);
      },
      set: function() {},  // read-only per spec
      configurable: true
    });
  }
  return el;
};

})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_file_token_unique() {
        let t1 = register_file_token("/tmp/a.txt", "https://a.example");
        let t2 = register_file_token("/tmp/b.txt", "https://a.example");
        assert_ne!(t1, t2, "tokens must be unique");
        assert_eq!(t1.len(), 32, "token must be 128 bits of hex");
        assert!(t1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// BUG-371: the old token space was `1, 2, 3, …`, so a page could read every
    /// file the user had ever picked by enumerating small integers.
    #[test]
    fn register_file_token_is_not_a_counter() {
        let tokens: std::collections::HashSet<String> = (0..8)
            .map(|i| register_file_token(&format!("/tmp/{i}.txt"), "https://a.example"))
            .collect();
        assert_eq!(tokens.len(), 8, "tokens must not collide");
        for n in 1..=64u64 {
            assert!(
                !tokens.contains(&n.to_string()),
                "small integer {n} must never be a valid token"
            );
        }
    }

    #[test]
    fn revoke_grants_for_origin_only_hits_that_origin() {
        let keep = register_file_token("/tmp/keep.txt", "https://keep.example");
        let drop = register_file_token("/tmp/drop.txt", "https://drop.example");
        revoke_grants_for_origin("https://drop.example");
        let reg = registry();
        assert!(reg.contains_key(&keep), "other origins must survive a revoke");
        assert!(!reg.contains_key(&drop), "revoked origin's grants must be gone");
    }

    #[test]
    fn origin_for_url_tuple_and_opaque() {
        assert_eq!(origin_for_url("https://a.example/x/y?q=1"), "https://a.example");
        assert_eq!(origin_for_url("http://a.example:8080/x"), "http://a.example:8080");
        // No host → opaque; two local files must not share an origin.
        assert_ne!(origin_for_url("file:///c/a.html"), origin_for_url("file:///c/b.html"));
    }
}

/// JS-shim tests (S12b-B14): moved out of `mod tests` because they depend on
/// [`install_file_input_bindings_v8`] to evaluate `FILE_INPUT_SHIM`, unlike the
/// rest of `mod tests` which exercises plain Rust functions engine-agnostically.
/// `to_base64_*` moved here too: `to_base64`/`read_file_bytes_for_token` are now
/// `#[cfg(feature = "v8-backend")]`-gated (their only remaining caller is the V8
/// install path), so testing them unconditionally would be dead code under the
/// default (rquickjs) build.
#[cfg(all(test, feature = "v8-backend"))]
mod v8_tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    #[test]
    fn to_base64_empty() {
        assert_eq!(to_base64(b""), "");
    }

    #[test]
    fn to_base64_hello() {
        assert_eq!(to_base64(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn to_base64_binary() {
        assert_eq!(to_base64(b"\x00\x01\x02"), "AAEC");
    }

    /// Minimal DOM stubs the shim needs (`Blob`, `_lumen_*`, `btoa`/`atob`) — mirrors
    /// the rquickjs harness this replaces, not the real `dom.rs::WEB_API_SHIM`.
    const STUBS: &str = r#"
        var window = globalThis;
        var _lumen_listeners = {};
        function Blob(blobParts, options) {}
        function _lumen_set_attr(nid, name, val) {}
        function _lumen_get_attr(nid, name) { return undefined; }
        function _lumen_dispatch_bubble(nid, type) {}
        function _lumen_make_element(nid) { return {__nid__: nid}; }
        window._lumen_make_element = _lumen_make_element;
        function btoa(str) {
          var chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
          var result = '', i = 0;
          while (i < str.length) {
            var b0 = str.charCodeAt(i++);
            var b1 = i < str.length ? str.charCodeAt(i++) : 0;
            var b2 = i < str.length ? str.charCodeAt(i++) : 0;
            var n = (b0 << 16) | (b1 << 8) | b2;
            result += chars[(n >> 18) & 63] + chars[(n >> 12) & 63]
                    + chars[(n >> 6) & 63] + chars[n & 63];
          }
          var pad = str.length % 3;
          if (pad === 1) result = result.slice(0, -2) + '==';
          else if (pad === 2) result = result.slice(0, -1) + '=';
          return result;
        }
        function atob(b64) {
          var chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
          var result = '', buf = 0, bits = 0;
          for (var i = 0; i < b64.length; i++) {
            var v = chars.indexOf(b64[i]);
            if (v < 0) continue;
            buf = (buf << 6) | v; bits += 6;
            if (bits >= 8) { bits -= 8; result += String.fromCharCode((buf >> bits) & 0xff); }
          }
          return result;
        }
        window.btoa = btoa; window.atob = atob;
    "#;

    /// Origin every V8 test here installs under; grants must be registered with
    /// the same string or the read bindings reject them (BUG-371).
    const TEST_ORIGIN: &str = "https://file-input.test";

    fn with_file_input() -> V8JsRuntime {
        with_file_input_origin(TEST_ORIGIN)
    }

    /// Installing revokes every grant previously issued to `origin`, so a test
    /// that needs a live grant must build its runtime **first** and use an
    /// origin no concurrently running test shares.
    fn with_file_input_origin(origin: &str) -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(STUBS).unwrap();
        install_file_input_bindings_v8(&rt, origin).unwrap();
        rt
    }

    fn js_bool(rt: &V8JsRuntime, expr: &str) -> bool {
        match rt.eval(expr).unwrap() {
            JsValue::Bool(b) => b,
            other => panic!("expected bool, got {other:?}"),
        }
    }

    #[test]
    fn file_class_exists() {
        let rt = with_file_input();
        assert!(js_bool(&rt, "typeof File === 'function'"));
    }

    #[test]
    fn file_name_and_size() {
        let rt = with_file_input();
        assert!(js_bool(
            &rt,
            "var f = new File(['hello'], 'test.txt', {type:'text/plain', lastModified:12345}); \
             f.name === 'test.txt' && f.size === 5 && f.type === 'text/plain' && f.lastModified === 12345"
        ));
    }

    #[test]
    fn file_content_stored_from_bits() {
        let rt = with_file_input();
        assert!(
            js_bool(&rt, "var f = new File(['abc'], 'a.txt'); f._content === 'abc'"),
            "bits should be joined into _content"
        );
    }

    #[test]
    fn file_text_returns_promise() {
        let rt = with_file_input();
        assert!(js_bool(
            &rt,
            "var f = new File(['abc'], 'a.txt'); f.text() instanceof Promise"
        ));
    }

    #[test]
    fn filelist_length_and_item() {
        let rt = with_file_input();
        assert!(js_bool(
            &rt,
            "var f = new File([], 'x.png'); \
             var fl = new FileList([f]); \
             fl.length === 1 && fl.item(0) === f && fl[0] === f && fl.item(1) === null"
        ));
    }

    #[test]
    fn filelist_empty() {
        let rt = with_file_input();
        assert!(js_bool(
            &rt,
            "var fl = new FileList([]); fl.length === 0 && fl.item(0) === null"
        ));
    }

    #[test]
    fn deliver_file_list_builds_file_objects() {
        let rt = with_file_input();
        rt.eval(
            "_lumen_deliver_file_list(42, '[{\"name\":\"photo.jpg\",\"token\":\"deadbeef\",\"size\":2048,\"mime_type\":\"image/jpeg\",\"last_modified_ms\":1000}]')"
        ).unwrap();
        assert!(js_bool(
            &rt,
            "_lumen_file_lists[42] instanceof FileList && \
             _lumen_file_lists[42].length === 1 && \
             _lumen_file_lists[42][0].name === 'photo.jpg' && \
             _lumen_file_lists[42][0].size === 2048 && \
             _lumen_file_lists[42][0].type === 'image/jpeg'"
        ));
    }

    /// BUG-371 point 4: the delivered token must not be readable off the File,
    /// and the public constructor must not accept one.
    #[test]
    fn delivered_token_is_not_web_visible() {
        let rt = with_file_input();
        rt.eval(
            "_lumen_deliver_file_list(42, '[{\"name\":\"photo.jpg\",\"token\":\"deadbeef\",\"size\":1,\"mime_type\":\"\",\"last_modified_ms\":0}]')"
        ).unwrap();
        assert!(
            js_bool(
                &rt,
                "var f = _lumen_file_lists[42][0]; \
                 f._token === undefined && \
                 Object.getOwnPropertyNames(f).indexOf('_token') < 0 && \
                 Object.getOwnPropertySymbols(f).length === 0"
            ),
            "token must live in a private slot, not on the File object"
        );
        assert!(
            js_bool(
                &rt,
                "var g = new File([], 'x', {_token: 'deadbeef'}); g._token === undefined"
            ),
            "File constructor must ignore a page-supplied _token"
        );
    }

    /// BUG-371 point 1: after `install_dom` seals the file API, page script can
    /// no longer reach the read bindings, but `File.text()` still works.
    #[test]
    fn sealing_removes_natives_but_keeps_file_read_working() {
        use std::io::Write;
        let mut tmp = std::env::temp_dir();
        tmp.push("lumen_file_input_seal_test.txt");
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            f.write_all(b"sealed content").unwrap();
        }
        let rt = with_file_input_origin("https://seal.test");
        let token = register_file_token(tmp.to_str().unwrap(), "https://seal.test");
        seal_file_natives_v8(&rt).unwrap();
        assert!(
            js_bool(
                &rt,
                "typeof globalThis.__lumen_file_read_text === 'undefined' && \
                 typeof globalThis.__lumen_file_read_base64 === 'undefined' && \
                 typeof globalThis.__lumen_fs_internal === 'undefined' && \
                 Object.getOwnPropertyNames(globalThis).indexOf('__lumen_file_read_text') < 0"
            ),
            "natives must be gone from the global object after sealing"
        );

        rt.eval(&format!(
            "_lumen_deliver_file_list(1, '[{{\"name\":\"a.txt\",\"token\":\"{token}\",\"size\":14,\"mime_type\":\"\",\"last_modified_ms\":0}}]'); \
             var out = null; _lumen_file_lists[1][0].text().then(function(t) {{ out = t; }});"
        ))
        .unwrap();
        assert_eq!(
            rt.eval("out").unwrap(),
            JsValue::String("sealed content".into()),
            "the shim keeps its captured binding after sealing"
        );
        let _ = std::fs::remove_file(&tmp);
    }

    /// BUG-371 point 3: a token issued to one origin is worthless on another.
    #[test]
    fn read_rejects_token_from_another_origin() {
        use std::io::Write;
        let mut tmp = std::env::temp_dir();
        tmp.push("lumen_file_input_origin_test.txt");
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            f.write_all(b"secret").unwrap();
        }
        let token = register_file_token(tmp.to_str().unwrap(), "https://victim.example");

        let rt = with_file_input(); // installed under TEST_ORIGIN
        let result = rt
            .eval(&format!("__lumen_file_read_text('{token}')"))
            .unwrap();
        assert_eq!(
            result,
            JsValue::String(String::new()),
            "a foreign origin's grant must not be redeemable"
        );
        let _ = std::fs::remove_file(&tmp);
    }

    /// BUG-371 point 3: reloading a document drops the grants of its previous
    /// incarnation instead of letting them live for the whole process.
    #[test]
    fn install_revokes_previous_grants_of_same_origin() {
        use std::io::Write;
        let mut tmp = std::env::temp_dir();
        tmp.push("lumen_file_input_revoke_test.txt");
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            f.write_all(b"stale").unwrap();
        }
        let token = register_file_token(tmp.to_str().unwrap(), "https://reload.test");

        let rt = V8JsRuntime::new().unwrap();
        rt.eval(STUBS).unwrap();
        // Second document on the same origin — the first one's grant must die.
        install_file_input_bindings_v8(&rt, "https://reload.test").unwrap();
        let result = rt
            .eval(&format!("__lumen_file_read_text('{token}')"))
            .unwrap();
        assert_eq!(result, JsValue::String(String::new()));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn deliver_file_list_empty_json() {
        let rt = with_file_input();
        rt.eval("_lumen_deliver_file_list(7, '[]')").unwrap();
        assert!(js_bool(&rt, "_lumen_file_lists[7].length === 0"));
    }

    #[test]
    fn filelist_iterator() {
        let rt = with_file_input();
        assert!(js_bool(
            &rt,
            "var fl = new FileList([new File([], 'a'), new File([], 'b')]); \
             var names = []; \
             for (var f of fl) { names.push(f.name); } \
             names[0] === 'a' && names[1] === 'b' && names.length === 2"
        ));
    }

    #[test]
    fn make_element_files_getter() {
        let rt = with_file_input();
        rt.eval(
            "_lumen_get_attr = function(nid, name) { \
               if (nid === 99 && name === 'type') return 'file'; \
               return undefined; \
             };"
        ).unwrap();
        assert!(
            js_bool(
                &rt,
                "var el = _lumen_make_element(99); \
                 el.files instanceof FileList && el.files.length === 0"
            ),
            "files getter should return empty FileList for type=file input"
        );

        rt.eval(
            "_lumen_deliver_file_list(99, '[{\"name\":\"doc.pdf\",\"token\":55,\"size\":512,\"mime_type\":\"application/pdf\",\"last_modified_ms\":0}]')"
        ).unwrap();
        assert!(js_bool(
            &rt,
            "_lumen_make_element(99).files.length === 1 && \
             _lumen_make_element(99).files[0].name === 'doc.pdf'"
        ));
    }

    #[test]
    fn native_read_text_returns_empty_for_unknown_token() {
        let rt = with_file_input();
        let result = rt.eval("__lumen_file_read_text('999999')").unwrap();
        assert_eq!(
            result,
            JsValue::String(String::new()),
            "unknown token should return empty string"
        );
    }

    #[test]
    fn native_read_text_returns_file_content() {
        use std::io::Write;
        let mut tmp = std::env::temp_dir();
        tmp.push("lumen_file_input_test.txt");
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            f.write_all(b"hello lumen").unwrap();
        }
        let rt = with_file_input_origin("https://read-text.test");
        let token = register_file_token(tmp.to_str().unwrap(), "https://read-text.test");
        let result = rt
            .eval(&format!("__lumen_file_read_text('{token}')"))
            .unwrap();
        assert_eq!(result, JsValue::String("hello lumen".into()));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn native_read_base64_returns_file_content() {
        use std::io::Write;
        let mut tmp = std::env::temp_dir();
        tmp.push("lumen_file_input_base64_test.bin");
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            f.write_all(b"\x00\x01\x02\xff").unwrap();
        }
        let expected_b64 = to_base64(b"\x00\x01\x02\xff");

        let rt = with_file_input_origin("https://read-b64.test");
        let token = register_file_token(tmp.to_str().unwrap(), "https://read-b64.test");
        let result = rt
            .eval(&format!("__lumen_file_read_base64('{token}')"))
            .unwrap();
        assert_eq!(result, JsValue::String(expected_b64));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn file_stream_getreader() {
        let rt = with_file_input();
        assert!(
            js_bool(
                &rt,
                "var f = new File(['XY'], 'x.bin'); \
                 var s = f.stream(); \
                 typeof s === 'object' && typeof s.getReader === 'function'"
            ),
            "stream() should return an object with getReader"
        );
    }
}
