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
//! # Registered native bindings
//!
//! | Name | Signature | Description |
//! |---|---|---|
//! | `__lumen_file_read_text` | `(token: f64) → String` | Read file bytes as UTF-8 (lossy) |
//! | `__lumen_file_read_base64` | `(token: f64) → String` | Read file bytes as base64 |
//!
//! # Shell wiring (main.rs)
//!
//! 1. `open_file_picker` calls `register_file_token(path)` for each selected file.
//! 2. Tokens are included in the JSON passed to `_lumen_deliver_file_list(nid, json)`.
//! 3. JSON shape: `[{name, token, size, mime_type, last_modified_ms}, ...]`
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

// ── File token registry ───────────────────────────────────────────────────────

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

// The token registry is written by the shell (UI thread, `register_file_token`)
// and read by the JS file-read bindings (which run on the dedicated JS thread
// after B-1). A process-global `Mutex` shares it correctly across both threads;
// a `thread_local` would split it once the runtime moved off the UI thread.
// Tokens are globally unique (`NEXT_TOKEN`), so a shared map is sound.
static FILE_REGISTRY: LazyLock<Mutex<HashMap<u64, PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn registry() -> std::sync::MutexGuard<'static, HashMap<u64, PathBuf>> {
    FILE_REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Register a file path and return an opaque token for JS access.
///
/// Must be called **from Rust** (shell side) before delivering the file list to JS.
/// The returned token is safe to pass to JS — it grants read access only to this
/// specific file, not to arbitrary paths.
pub fn register_file_token(path: &str) -> u64 {
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    registry().insert(token, PathBuf::from(path));
    token
}

/// Revoke all tokens — should be called when a browsing context is torn down.
pub fn clear_file_registry() {
    registry().clear();
}

#[cfg(feature = "v8-backend")]
fn read_file_bytes_for_token(token: u64) -> Option<Vec<u8>> {
    let path = registry().get(&token).cloned()?;
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
#[cfg(feature = "v8-backend")]
pub(crate) fn install_file_input_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::into_v8_fn1;
    use lumen_core::ext::JsRuntime as _;

    let read_text = into_v8_fn1(move |token: f64| -> String {
        let t = token as u64;
        read_file_bytes_for_token(t)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    });
    rt.register_native("__lumen_file_read_text", read_text)?;

    let read_base64 = into_v8_fn1(move |token: f64| -> String {
        let t = token as u64;
        read_file_bytes_for_token(t)
            .map(|b| to_base64(&b))
            .unwrap_or_default()
    });
    rt.register_native("__lumen_file_read_base64", read_base64)?;

    rt.eval(FILE_INPUT_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const FILE_INPUT_SHIM: &str = r#"
(function() {
'use strict';

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
  // Opaque token for Phase 1 native reads (set by _lumen_deliver_file_list)
  if (typeof options._token === 'number') this._token = options._token;
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
  if (typeof this._token === 'number') {
    if (typeof __lumen_file_read_text === 'function') {
      try {
        return Promise.resolve(__lumen_file_read_text(this._token));
      } catch(e) {}
    }
    return Promise.resolve('');
  }
  return Promise.resolve(this._content || '');
};

// File.prototype.arrayBuffer() — W3C File API §4.3
// Returns a Promise resolving to an ArrayBuffer with the raw file bytes.
File.prototype.arrayBuffer = function() {
  if (typeof this._token === 'number') {
    if (typeof __lumen_file_read_base64 === 'function') {
      try {
        var b64 = __lumen_file_read_base64(this._token);
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
// so filesJson contains opaque tokens rather than raw path strings.
window._lumen_deliver_file_list = function(nid, filesJson) {
  var infos;
  try { infos = JSON.parse(filesJson); } catch(e) { infos = []; }
  if (!Array.isArray(infos)) infos = [];

  var objs = infos.map(function(f) {
    var opts = {
      type: f.mime_type || '',
      lastModified: f.last_modified_ms || 0
    };
    if (typeof f.token === 'number') opts._token = f.token;
    var fo = new File([], f.name || '', opts);
    fo.size = f.size || 0;
    return fo;
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
        let t1 = register_file_token("/tmp/a.txt");
        let t2 = register_file_token("/tmp/b.txt");
        assert_ne!(t1, t2, "tokens must be unique");
        assert!(t1 > 0 && t2 > 0);
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

    fn with_file_input() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(STUBS).unwrap();
        install_file_input_bindings_v8(&rt).unwrap();
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
    fn deliver_file_list_token_stored() {
        let rt = with_file_input();
        rt.eval(
            "_lumen_deliver_file_list(42, '[{\"name\":\"photo.jpg\",\"token\":99,\"size\":2048,\"mime_type\":\"image/jpeg\",\"last_modified_ms\":1000}]')"
        ).unwrap();
        assert!(js_bool(
            &rt,
            "_lumen_file_lists[42] instanceof FileList && \
             _lumen_file_lists[42].length === 1 && \
             _lumen_file_lists[42][0].name === 'photo.jpg' && \
             _lumen_file_lists[42][0].size === 2048 && \
             _lumen_file_lists[42][0]._token === 99"
        ));
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
        let result = rt.eval("__lumen_file_read_text(999999)").unwrap();
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
        let token = register_file_token(tmp.to_str().unwrap());

        let rt = with_file_input();
        let result = rt
            .eval(&format!("__lumen_file_read_text({})", token))
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
        let token = register_file_token(tmp.to_str().unwrap());
        let expected_b64 = to_base64(b"\x00\x01\x02\xff");

        let rt = with_file_input();
        let result = rt
            .eval(&format!("__lumen_file_read_base64({})", token))
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
