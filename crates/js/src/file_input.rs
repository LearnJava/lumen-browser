//! `<input type="file">` support — File, FileList classes and OS file-picker delivery.
//!
//! W3C File API §4 (File) + §7 (FileList). Phase 0: File objects carry metadata
//! (name, size, type, lastModified) but no byte content; `text()` / `arrayBuffer()`
//! return empty. Phase 1: shell reads bytes from `file._path` via a native binding.
//!
//! Shell wiring (main.rs):
//!   - `FormClickAction::OpenFilePicker` triggers the OS dialog.
//!   - After dialog returns, shell evals `_lumen_deliver_file_list(nid, json)`.
//!   - JSON shape: `[{name, path, size, mime_type, last_modified_ms}, ...]`
use rquickjs::Ctx;

/// Install File / FileList classes and `_lumen_deliver_file_list` into the JS context.
///
/// Must run after `dom::install_dom_bindings` (needs `_lumen_make_element`,
/// `_lumen_set_attr`, `_lumen_get_attr`, `_lumen_dispatch_bubble`).
pub fn install_file_input_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(FILE_INPUT_SHIM)?;
    Ok(())
}

const FILE_INPUT_SHIM: &str = r#"
(function() {
'use strict';

// ── File class (W3C File API §4) ──────────────────────────────────────────────
function File(bits, name, options) {
  options = options || {};
  this.name = String(name || '');
  this.size = 0;
  this.type = String(options.type || '');
  this.lastModified = (typeof options.lastModified === 'number')
    ? options.lastModified
    : (typeof Date !== 'undefined' ? Date.now() : 0);
  // Internal path for Phase 1 native reads
  if (options._path) this._path = String(options._path);
  // Phase 0: optionally initialise from string bits
  if (Array.isArray(bits) && bits.length > 0) {
    var joined = bits.join('');
    this._content = joined;
    this.size = joined.length;
  }
}
File.prototype.text = function() {
  return Promise.resolve(this._content || '');
};
File.prototype.arrayBuffer = function() {
  var s = this._content || '';
  var buf = new ArrayBuffer(s.length);
  var v = new Uint8Array(buf);
  for (var i = 0; i < s.length; i++) v[i] = s.charCodeAt(i) & 0xff;
  return Promise.resolve(buf);
};
File.prototype.stream = function() { return null; };
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
// Called via eval_js: _lumen_deliver_file_list(nid, '[{name,path,size,...}]')
window._lumen_deliver_file_list = function(nid, filesJson) {
  var infos;
  try { infos = JSON.parse(filesJson); } catch(e) { infos = []; }
  if (!Array.isArray(infos)) infos = [];

  var objs = infos.map(function(f) {
    var fo = new File([], f.name || '', {
      type: f.mime_type || '',
      lastModified: f.last_modified_ms || 0,
      _path: f.path || ''
    });
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
// Monkey-patch so every element wrapper returned by the factory has a .files
// getter that reads from the persistent _lumen_file_lists map.
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
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    /// Install minimal DOM stubs then File/FileList bindings.
    fn with_file_input(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(r#"
                var window = globalThis;
                var _lumen_listeners = {};
                function _lumen_set_attr(nid, name, val) {}
                function _lumen_get_attr(nid, name) { return undefined; }
                function _lumen_dispatch_bubble(nid, type) {}
                function _lumen_make_element(nid) { return {__nid__: nid}; }
                window._lumen_make_element = _lumen_make_element;
            "#).unwrap();
            install_file_input_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn file_class_exists() {
        with_file_input(|ctx| {
            let ok: bool = ctx.eval("typeof File === 'function'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn file_name_and_size() {
        with_file_input(|ctx| {
            let ok: bool = ctx.eval(
                "var f = new File(['hello'], 'test.txt', {type:'text/plain', lastModified:12345}); \
                 f.name === 'test.txt' && f.size === 5 && f.type === 'text/plain' && f.lastModified === 12345"
            ).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn file_text_returns_promise() {
        with_file_input(|ctx| {
            let ok: bool = ctx.eval(
                "var f = new File(['abc'], 'a.txt'); f.text() instanceof Promise"
            ).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn filelist_length_and_item() {
        with_file_input(|ctx| {
            let ok: bool = ctx.eval(
                "var f = new File([], 'x.png'); \
                 var fl = new FileList([f]); \
                 fl.length === 1 && fl.item(0) === f && fl[0] === f && fl.item(1) === null"
            ).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn filelist_empty() {
        with_file_input(|ctx| {
            let ok: bool = ctx.eval(
                "var fl = new FileList([]); fl.length === 0 && fl.item(0) === null"
            ).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn deliver_file_list_creates_filelist() {
        with_file_input(|ctx| {
            ctx.eval::<(), _>(
                "_lumen_deliver_file_list(42, '[{\"name\":\"photo.jpg\",\"path\":\"/tmp/photo.jpg\",\"size\":2048,\"mime_type\":\"image/jpeg\",\"last_modified_ms\":1000}]')"
            ).unwrap();
            let ok: bool = ctx.eval(
                "_lumen_file_lists[42] instanceof FileList && \
                 _lumen_file_lists[42].length === 1 && \
                 _lumen_file_lists[42][0].name === 'photo.jpg' && \
                 _lumen_file_lists[42][0].size === 2048"
            ).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn deliver_file_list_empty_json() {
        with_file_input(|ctx| {
            ctx.eval::<(), _>("_lumen_deliver_file_list(7, '[]')").unwrap();
            let ok: bool = ctx.eval("_lumen_file_lists[7].length === 0").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn filelist_iterator() {
        with_file_input(|ctx| {
            let ok: bool = ctx.eval(
                "var fl = new FileList([new File([], 'a'), new File([], 'b')]); \
                 var names = []; \
                 for (var f of fl) { names.push(f.name); } \
                 names[0] === 'a' && names[1] === 'b' && names.length === 2"
            ).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn make_element_files_getter() {
        with_file_input(|ctx| {
            ctx.eval::<(), _>(
                "_lumen_get_attr = function(nid, name) { \
                   if (nid === 99 && name === 'type') return 'file'; \
                   return undefined; \
                 };"
            ).unwrap();
            let ok: bool = ctx.eval(
                "var el = _lumen_make_element(99); \
                 el.files instanceof FileList && el.files.length === 0"
            ).unwrap();
            assert!(ok, "files getter should return empty FileList for type=file input");

            ctx.eval::<(), _>(
                "_lumen_deliver_file_list(99, '[{\"name\":\"doc.pdf\",\"path\":\"/d/doc.pdf\",\"size\":512,\"mime_type\":\"application/pdf\",\"last_modified_ms\":0}]')"
            ).unwrap();
            let ok2: bool = ctx.eval(
                "_lumen_make_element(99).files.length === 1 && \
                 _lumen_make_element(99).files[0].name === 'doc.pdf'"
            ).unwrap();
            assert!(ok2);
        });
    }
}
