//! Functional `canvas.getContext('webgl')` bindings (task #28, §7F).
//!
//! Wires the WebGL 1.0 JS API to [`lumen_paint::SoftwareWebGl`], the CPU
//! "GPU pipeline" backend. Unlike the fingerprint-only shim in
//! [`crate::webgl_bindings`], the context returned here is *functional*:
//! `createBuffer`/`bindBuffer`/`bufferData`, `createShader`/`compileShader`/
//! `createProgram`/`linkProgram`/`useProgram`, `vertexAttribPointer`/
//! `enableVertexAttribArray`, `uniform4f`, `clearColor`/`clear`, `viewport`,
//! `drawArrays` and `readPixels` all drive a real software rasterizer whose
//! pixels can be read back.
//!
//! # State model
//!
//! Each `getContext('webgl')` call allocates a [`SoftwareWebGl`] in a
//! per-thread registry, keyed by an opaque context id. The QuickJS runtime is
//! single-threaded per context, and Web Workers each run on their own thread
//! with their own runtime, so a `thread_local` registry gives correct
//! per-runtime isolation. Contexts are not freed on canvas GC — acceptable for
//! a software stub; long-lived pages create few WebGL contexts.
//!
//! # Fingerprint normalization (ADR-007 Layer 4)
//!
//! The JS context preserves the anti-fingerprinting guarantees of the old
//! shim: `getParameter(UNMASKED_VENDOR_WEBGL / UNMASKED_RENDERER_WEBGL)` and
//! `getParameter(VENDOR / RENDERER)` return the normalized `GpuFingerprint`
//! strings, and `canvas.toDataURL()` / `toBlob()` stay blank to defeat pixel
//! hashing.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use lumen_paint::webgl::{self, SoftwareWebGl};
use rquickjs::{Ctx, Function};

thread_local! {
    /// Per-thread WebGL context registry, keyed by opaque context id.
    static CONTEXTS: RefCell<HashMap<u32, SoftwareWebGl>> = RefCell::new(HashMap::new());
    /// Monotonic context-id allocator (shared across runtimes on one thread).
    static NEXT_ID: Cell<u32> = const { Cell::new(1) };
}

/// Run `f` against the `SoftwareWebGl` for `id`, returning `default` if absent.
fn with_ctx<R>(id: u32, default: R, f: impl FnOnce(&mut SoftwareWebGl) -> R) -> R {
    CONTEXTS.with(|c| match c.borrow_mut().get_mut(&id) {
        Some(gl) => f(gl),
        None => default,
    })
}

/// Install functional WebGL bindings into the JS context.
///
/// Registers the `_lumen_webgl_*` native functions and evaluates the JS shim
/// that intercepts `document.createElement('canvas')` so that
/// `canvas.getContext('webgl')` returns a functional context backed by
/// [`SoftwareWebGl`]. Must be called **before** any user script that touches
/// the WebGL API.
pub fn install_webgl_canvas(
    ctx: &Ctx,
    fingerprint: &lumen_paint::GpuFingerprint,
) -> rquickjs::Result<()> {
    ctx.globals()
        .set("_LUMEN_GPU_VENDOR", fingerprint.vendor().to_string())?;
    ctx.globals()
        .set("_LUMEN_GPU_RENDERER", fingerprint.renderer().to_string())?;

    macro_rules! reg {
        ($name:expr, $f:expr) => {
            ctx.globals().set($name, Function::new(ctx.clone(), $f)?)?;
        };
    }

    // Allocate a context with a `w × h` drawing buffer; returns its id.
    reg!("_lumen_webgl_create", |w: i32, h: i32| -> u32 {
        let id = NEXT_ID.with(|n| {
            let v = n.get();
            n.set(v + 1);
            v
        });
        let gl = SoftwareWebGl::new(w.max(1) as u32, h.max(1) as u32);
        CONTEXTS.with(|c| c.borrow_mut().insert(id, gl));
        id
    });

    // Release a context (called by the JS shim's `loseContext`).
    reg!("_lumen_webgl_destroy", |id: u32| {
        CONTEXTS.with(|c| {
            c.borrow_mut().remove(&id);
        });
    });

    reg!("_lumen_webgl_viewport", |id: u32, x: i32, y: i32, w: i32, h: i32| {
        with_ctx(id, (), |gl| gl.viewport(x, y, w, h));
    });
    reg!("_lumen_webgl_clear_color", |id: u32, r: f64, g: f64, b: f64, a: f64| {
        with_ctx(id, (), |gl| gl.clear_color(r as f32, g as f32, b as f32, a as f32));
    });
    reg!("_lumen_webgl_clear", |id: u32, mask: u32| {
        with_ctx(id, (), |gl| gl.clear(mask));
    });

    reg!("_lumen_webgl_create_buffer", |id: u32| -> u32 {
        with_ctx(id, 0, |gl| gl.create_buffer())
    });
    reg!("_lumen_webgl_bind_buffer", |id: u32, target: u32, buffer: u32| {
        with_ctx(id, (), |gl| gl.bind_buffer(target, buffer));
    });
    reg!("_lumen_webgl_buffer_data", |id: u32, target: u32, data: Vec<f64>| {
        let floats: Vec<f32> = data.into_iter().map(|v| v as f32).collect();
        with_ctx(id, (), |gl| gl.buffer_data_f32(target, floats));
    });

    reg!("_lumen_webgl_create_shader", |id: u32, kind: u32| -> u32 {
        with_ctx(id, 0, |gl| gl.create_shader(kind))
    });
    reg!("_lumen_webgl_shader_source", |id: u32, shader: u32, src: String| {
        with_ctx(id, (), |gl| gl.shader_source(shader, src));
    });
    reg!("_lumen_webgl_compile_shader", |id: u32, shader: u32| {
        with_ctx(id, (), |gl| gl.compile_shader(shader));
    });
    reg!("_lumen_webgl_shader_compiled", |id: u32, shader: u32| -> bool {
        with_ctx(id, false, |gl| gl.shader_compiled(shader))
    });

    reg!("_lumen_webgl_create_program", |id: u32| -> u32 {
        with_ctx(id, 0, |gl| gl.create_program())
    });
    reg!("_lumen_webgl_attach_shader", |id: u32, program: u32, shader: u32| {
        with_ctx(id, (), |gl| gl.attach_shader(program, shader));
    });
    reg!("_lumen_webgl_link_program", |id: u32, program: u32| {
        with_ctx(id, (), |gl| gl.link_program(program));
    });
    reg!("_lumen_webgl_program_linked", |id: u32, program: u32| -> bool {
        with_ctx(id, false, |gl| gl.program_linked(program))
    });
    reg!("_lumen_webgl_use_program", |id: u32, program: u32| {
        with_ctx(id, (), |gl| gl.use_program(program));
    });

    reg!("_lumen_webgl_attrib_location", |id: u32, program: u32, name: String| -> i32 {
        with_ctx(id, -1, |gl| gl.get_attrib_location(program, &name))
    });
    reg!("_lumen_webgl_uniform_location", |id: u32, program: u32, name: String| -> i32 {
        with_ctx(id, -1, |gl| gl.get_uniform_location(program, &name))
    });

    reg!("_lumen_webgl_enable_attrib", |id: u32, index: u32| {
        with_ctx(id, (), |gl| gl.enable_vertex_attrib_array(index));
    });
    reg!("_lumen_webgl_disable_attrib", |id: u32, index: u32| {
        with_ctx(id, (), |gl| gl.disable_vertex_attrib_array(index));
    });
    reg!(
        "_lumen_webgl_attrib_pointer",
        |id: u32, index: u32, size: i32, stride: i32, offset: i32| {
            with_ctx(id, (), |gl| {
                gl.vertex_attrib_pointer(
                    index,
                    size.max(0) as usize,
                    stride.max(0) as usize,
                    offset.max(0) as usize,
                );
            });
        }
    );
    reg!("_lumen_webgl_uniform4f", |id: u32, loc: i32, x: f64, y: f64, z: f64, w: f64| {
        with_ctx(id, (), |gl| gl.uniform4f(loc, x as f32, y as f32, z as f32, w as f32));
    });
    reg!("_lumen_webgl_uniform3f", |id: u32, loc: i32, x: f64, y: f64, z: f64| {
        with_ctx(id, (), |gl| gl.uniform3f(loc, x as f32, y as f32, z as f32));
    });
    reg!("_lumen_webgl_uniform2f", |id: u32, loc: i32, x: f64, y: f64| {
        with_ctx(id, (), |gl| gl.uniform2f(loc, x as f32, y as f32));
    });
    reg!("_lumen_webgl_uniform1f", |id: u32, loc: i32, x: f64| {
        with_ctx(id, (), |gl| gl.uniform1f(loc, x as f32));
    });
    reg!("_lumen_webgl_uniform1i", |id: u32, loc: i32, v: i32| {
        with_ctx(id, (), |gl| gl.uniform1i(loc, v));
    });
    reg!("_lumen_webgl_uniform_mat4fv", |id: u32, loc: i32, data: Vec<f64>| {
        let fs: Vec<f32> = data.into_iter().map(|v| v as f32).collect();
        with_ctx(id, (), |gl| gl.uniform_matrix4fv(loc, &fs));
    });
    reg!("_lumen_webgl_active_texture", |id: u32, unit: u32| {
        with_ctx(id, (), |gl| gl.active_texture(unit));
    });
    reg!("_lumen_webgl_bind_texture", |id: u32, target: u32, tex_id: u32| {
        with_ctx(id, (), |gl| gl.bind_texture(target, tex_id));
    });
    reg!("_lumen_webgl_tex_image_2d", |id: u32, tex_id: u32, w: u32, h: u32, data: Vec<u8>| {
        with_ctx(id, (), |gl| gl.tex_image_2d_rgba(tex_id, w, h, &data));
    });

    reg!("_lumen_webgl_draw_arrays", |id: u32, mode: u32, first: i32, count: i32| {
        with_ctx(id, (), |gl| gl.draw_arrays(mode, first, count));
    });

    // Full RGBA8 framebuffer readback (top-left origin). The JS `readPixels`
    // wrapper crops the requested sub-rect and flips to WebGL's bottom-left.
    reg!("_lumen_webgl_read_pixels", |id: u32| -> Vec<u8> {
        with_ctx(id, Vec::new(), |gl| gl.pixels().to_vec())
    });
    reg!("_lumen_webgl_dims", |id: u32| -> Vec<u32> {
        with_ctx(id, vec![0, 0], |gl| vec![gl.width(), gl.height()])
    });

    ctx.eval::<(), _>(WEBGL_SHIM)?;
    Ok(())
}

/// Re-export of backend mode constants for callers/tests that build draw calls
/// in Rust (mirrors `lumen_paint::webgl`).
pub use webgl::{ARRAY_BUFFER, COLOR_BUFFER_BIT, FRAGMENT_SHADER, TRIANGLES, VERTEX_SHADER};

/// JavaScript shim: builds a functional WebGL context object that forwards to
/// the `_lumen_webgl_*` natives, and intercepts
/// `document.createElement('canvas')` to attach `getContext`.
const WEBGL_SHIM: &str = r#"(function() {
  var _vendor   = (typeof _LUMEN_GPU_VENDOR   !== 'undefined') ? _LUMEN_GPU_VENDOR   : 'WebKit';
  var _renderer = (typeof _LUMEN_GPU_RENDERER !== 'undefined') ? _LUMEN_GPU_RENDERER : 'Generic GPU';

  // Opaque GL object wrappers carry the backend handle in `__wid`.
  function _wrap(n) { return n ? { __wid: n } : null; }
  function _unwrap(o) {
    if (o == null) return 0;
    if (typeof o === 'number') return o;
    return o.__wid || 0;
  }

  function _makeContext(cid) {
    var gl = {
      // ── Primitive modes ──
      POINTS: 0x0000, LINES: 0x0001, LINE_LOOP: 0x0002, LINE_STRIP: 0x0003,
      TRIANGLES: 0x0004, TRIANGLE_STRIP: 0x0005, TRIANGLE_FAN: 0x0006,
      // ── Buffers ──
      ARRAY_BUFFER: 0x8892, ELEMENT_ARRAY_BUFFER: 0x8893,
      STATIC_DRAW: 0x88E4, DYNAMIC_DRAW: 0x88E8, STREAM_DRAW: 0x88E0,
      // ── Clear bits ──
      DEPTH_BUFFER_BIT: 0x0100, STENCIL_BUFFER_BIT: 0x0400, COLOR_BUFFER_BIT: 0x4000,
      // ── Types ──
      BYTE: 0x1400, UNSIGNED_BYTE: 0x1401, SHORT: 0x1402, UNSIGNED_SHORT: 0x1403,
      INT: 0x1404, UNSIGNED_INT: 0x1405, FLOAT: 0x1406,
      // ── Shaders / programs ──
      FRAGMENT_SHADER: 0x8B30, VERTEX_SHADER: 0x8B31,
      COMPILE_STATUS: 0x8B81, LINK_STATUS: 0x8B82, VALIDATE_STATUS: 0x8B83,
      // ── Capabilities (accepted, no-op) ──
      DEPTH_TEST: 0x0B71, BLEND: 0x0BE2, CULL_FACE: 0x0B44, SCISSOR_TEST: 0x0C11,
      // ── Pixel formats ──
      RGB: 0x1907, RGBA: 0x1908,
      // ── getParameter pnames ──
      VENDOR: 0x1F00, RENDERER: 0x1F01, VERSION: 0x1F02, SHADING_LANGUAGE_VERSION: 0x8B8C,
      MAX_TEXTURE_SIZE: 0x0D33, MAX_VIEWPORT_DIMS: 0x0D3A,
      MAX_VERTEX_ATTRIBS: 0x8869, MAX_COMBINED_TEXTURE_IMAGE_UNITS: 0x8B4D,
      UNMASKED_VENDOR_WEBGL: 0x9245, UNMASKED_RENDERER_WEBGL: 0x9246
    };

    // ── State / capability no-ops ──
    gl.enable = function() {};
    gl.disable = function() {};
    gl.blendFunc = function() {};
    gl.depthFunc = function() {};
    gl.pixelStorei = function() {};
    gl.scissor = function() {};
    gl.flush = function() {};
    gl.finish = function() {};
    gl.isContextLost = function() { return false; };
    gl.getError = function() { return 0; };
    gl.getContextAttributes = function() {
      return { alpha: true, antialias: false, depth: true, premultipliedAlpha: true,
               preserveDrawingBuffer: false, stencil: false };
    };

    gl.viewport = function(x, y, w, h) { _lumen_webgl_viewport(cid, x|0, y|0, w|0, h|0); };
    gl.clearColor = function(r, g, b, a) { _lumen_webgl_clear_color(cid, +r, +g, +b, +a); };
    gl.clear = function(mask) { _lumen_webgl_clear(cid, mask>>>0); };
    gl.clearDepth = function() {};
    gl.clearStencil = function() {};

    // ── Buffers ──
    gl.createBuffer = function() { return _wrap(_lumen_webgl_create_buffer(cid)); };
    gl.deleteBuffer = function() {};
    gl.bindBuffer = function(target, buffer) {
      _lumen_webgl_bind_buffer(cid, target>>>0, _unwrap(buffer));
    };
    gl.bufferData = function(target, data /*, usage */) {
      var arr = [];
      if (data && typeof data.length === 'number') {
        for (var i = 0; i < data.length; i++) arr.push(data[i]);
      } else if (typeof data === 'number') {
        // Size-only allocation: zero-fill.
        for (var j = 0; j < data; j++) arr.push(0);
      }
      _lumen_webgl_buffer_data(cid, target>>>0, arr);
    };
    gl.bufferSubData = function() {};

    // ── Shaders ──
    gl.createShader = function(kind) { return _wrap(_lumen_webgl_create_shader(cid, kind>>>0)); };
    gl.deleteShader = function() {};
    gl.shaderSource = function(shader, src) { _lumen_webgl_shader_source(cid, _unwrap(shader), '' + src); };
    gl.compileShader = function(shader) { _lumen_webgl_compile_shader(cid, _unwrap(shader)); };
    gl.getShaderParameter = function(shader, pname) {
      if (pname === gl.COMPILE_STATUS) return _lumen_webgl_shader_compiled(cid, _unwrap(shader));
      return null;
    };
    gl.getShaderInfoLog = function() { return ''; };

    // ── Programs ──
    gl.createProgram = function() { return _wrap(_lumen_webgl_create_program(cid)); };
    gl.deleteProgram = function() {};
    gl.attachShader = function(program, shader) { _lumen_webgl_attach_shader(cid, _unwrap(program), _unwrap(shader)); };
    gl.detachShader = function() {};
    gl.linkProgram = function(program) { _lumen_webgl_link_program(cid, _unwrap(program)); };
    gl.getProgramParameter = function(program, pname) {
      if (pname === gl.LINK_STATUS) return _lumen_webgl_program_linked(cid, _unwrap(program));
      if (pname === gl.VALIDATE_STATUS) return true;
      return null;
    };
    gl.getProgramInfoLog = function() { return ''; };
    gl.validateProgram = function() {};
    gl.useProgram = function(program) { _lumen_webgl_use_program(cid, _unwrap(program)); };

    // ── Attributes / uniforms ──
    gl.getAttribLocation = function(program, name) { return _lumen_webgl_attrib_location(cid, _unwrap(program), '' + name); };
    gl.bindAttribLocation = function() {};
    gl.getUniformLocation = function(program, name) {
      var loc = _lumen_webgl_uniform_location(cid, _unwrap(program), '' + name);
      return loc < 0 ? null : { __loc: loc };
    };
    function _locVal(location) {
      if (location == null) return -1;
      if (typeof location === 'number') return location;
      return (typeof location.__loc === 'number') ? location.__loc : -1;
    }
    gl.enableVertexAttribArray = function(index) { _lumen_webgl_enable_attrib(cid, index>>>0); };
    gl.disableVertexAttribArray = function(index) { _lumen_webgl_disable_attrib(cid, index>>>0); };
    gl.vertexAttribPointer = function(index, size, type, normalized, stride, offset) {
      _lumen_webgl_attrib_pointer(cid, index>>>0, size|0, stride|0, offset|0);
    };
    gl.uniform4f = function(location, x, y, z, w) { _lumen_webgl_uniform4f(cid, _locVal(location), +x, +y, +z, +w); };
    gl.uniform4fv = function(location, v) { _lumen_webgl_uniform4f(cid, _locVal(location), +v[0], +v[1], +v[2], +v[3]); };
    gl.uniform3f = function(location, x, y, z) { _lumen_webgl_uniform3f(cid, _locVal(location), +x, +y, +z); };
    gl.uniform3fv = function(location, v) { _lumen_webgl_uniform3f(cid, _locVal(location), +v[0], +v[1], +v[2]); };
    gl.uniform2f = function(location, x, y) { _lumen_webgl_uniform2f(cid, _locVal(location), +x, +y); };
    gl.uniform2fv = function(location, v) { _lumen_webgl_uniform2f(cid, _locVal(location), +v[0], +v[1]); };
    gl.uniform1f = function(location, x) { _lumen_webgl_uniform1f(cid, _locVal(location), +x); };
    gl.uniform1fv = function(location, v) { _lumen_webgl_uniform1f(cid, _locVal(location), +v[0]); };
    gl.uniform1i = function(location, v) { _lumen_webgl_uniform1i(cid, _locVal(location), v|0); };
    gl.uniform1iv = function(location, v) { _lumen_webgl_uniform1i(cid, _locVal(location), (v[0])|0); };
    gl.uniformMatrix4fv = function(location, transpose, data) {
      var arr = [];
      for (var mi = 0; mi < 16; mi++) arr.push(+(data[mi] || 0));
      _lumen_webgl_uniform_mat4fv(cid, _locVal(location), arr);
    };
    gl.uniformMatrix3fv = function() {}; // mat3 not tracked

    // ── Draw ──
    gl.drawArrays = function(mode, first, count) { _lumen_webgl_draw_arrays(cid, mode>>>0, first|0, count|0); };
    gl.drawElements = function() {};

    // ── Readback (WebGL: bottom-left origin) ──
    gl.readPixels = function(x, y, width, height, format, type, pixels) {
      if (!pixels || typeof pixels.length !== 'number') return;
      var fb = _lumen_webgl_read_pixels(cid);
      var dims = _lumen_webgl_dims(cid);
      var fw = dims[0], fh = dims[1];
      x = x|0; y = y|0; width = width|0; height = height|0;
      for (var row = 0; row < height; row++) {
        // Flip: WebGL row 0 is the bottom of the framebuffer.
        var srcY = fh - 1 - (y + row);
        for (var col = 0; col < width; col++) {
          var srcX = x + col;
          var di = (row * width + col) * 4;
          if (srcX < 0 || srcX >= fw || srcY < 0 || srcY >= fh) {
            pixels[di] = 0; pixels[di+1] = 0; pixels[di+2] = 0; pixels[di+3] = 0;
            continue;
          }
          var si = (srcY * fw + srcX) * 4;
          pixels[di]   = fb[si];
          pixels[di+1] = fb[si+1];
          pixels[di+2] = fb[si+2];
          pixels[di+3] = fb[si+3];
        }
      }
    };

    // ── Parameters (fingerprint-normalized, ADR-007 Layer 4) ──
    gl.getParameter = function(pname) {
      switch (pname) {
        case gl.UNMASKED_VENDOR_WEBGL:
        case gl.VENDOR: return _vendor;
        case gl.UNMASKED_RENDERER_WEBGL:
        case gl.RENDERER: return _renderer;
        case gl.VERSION: return 'WebGL 1.0';
        case gl.SHADING_LANGUAGE_VERSION: return 'WebGL GLSL ES 1.0';
        case gl.MAX_TEXTURE_SIZE: return 4096;
        case gl.MAX_VIEWPORT_DIMS: return [4096, 4096];
        case gl.MAX_VERTEX_ATTRIBS: return 16;
        case gl.MAX_COMBINED_TEXTURE_IMAGE_UNITS: return 8;
        default: return null;
      }
    };
    gl.getExtension = function(name) {
      if (name === 'WEBGL_debug_renderer_info') {
        return { UNMASKED_VENDOR_WEBGL: 0x9245, UNMASKED_RENDERER_WEBGL: 0x9246 };
      }
      if (name === 'WEBGL_lose_context') {
        return { loseContext: function() { _lumen_webgl_destroy(cid); }, restoreContext: function() {} };
      }
      return null;
    };
    gl.getSupportedExtensions = function() {
      return ['WEBGL_debug_renderer_info', 'WEBGL_lose_context'];
    };

    // ── Textures ──────────────────────────────────────────────────────────
    var _nextTexId = 1;
    var _boundTex2D = 0; // currently bound TEXTURE_2D id
    gl.createTexture = function() { var id = _nextTexId++; return _wrap(id); };
    gl.deleteTexture = function() {};
    gl.bindTexture = function(target, tex) {
      var tid = _unwrap(tex);
      if ((target>>>0) === 0x0DE1) _boundTex2D = tid; // GL_TEXTURE_2D
      _lumen_webgl_bind_texture(cid, target>>>0, tid);
    };
    gl.activeTexture = function(unit) { _lumen_webgl_active_texture(cid, unit>>>0); };
    gl.texParameteri = function() {};
    gl.generateMipmap = function() {};
    gl.texImage2D = function(target, level, internalformat, width, height, border, format, type, pixels) {
      if (pixels == null || _boundTex2D === 0) return;
      var w = width|0, h = height|0;
      var arr = [];
      if (pixels && typeof pixels.length === 'number') {
        var expected = w * h * 4;
        for (var pi = 0; pi < expected && pi < pixels.length; pi++) arr.push(pixels[pi] & 0xFF);
      }
      _lumen_webgl_tex_image_2d(cid, _boundTex2D, w, h, arr);
    };

    return gl;
  }

  function _canvasDims(el) {
    var w = 300, h = 150;
    if (el) {
      if (typeof el.width === 'number' && el.width > 0) w = el.width;
      else if (typeof el.getAttribute === 'function') {
        var aw = parseInt(el.getAttribute('width'), 10);
        if (aw > 0) w = aw;
      }
      if (typeof el.height === 'number' && el.height > 0) h = el.height;
      else if (typeof el.getAttribute === 'function') {
        var ah = parseInt(el.getAttribute('height'), 10);
        if (ah > 0) h = ah;
      }
    }
    return [w, h];
  }

  function _addCanvasStubs(el) {
    var _ctx = null;
    el.getContext = function(contextType) {
      var t = ('' + (contextType || '')).toLowerCase();
      if (t === 'webgl' || t === 'webgl2' || t === 'experimental-webgl') {
        if (!_ctx) {
          var d = _canvasDims(el);
          var cid = _lumen_webgl_create(d[0], d[1]);
          _ctx = _makeContext(cid);
          _ctx.canvas = el;
          _ctx.drawingBufferWidth = d[0];
          _ctx.drawingBufferHeight = d[1];
        }
        return _ctx;
      }
      return null;
    };
    // Blank data URL — prevents canvas pixel-hash fingerprinting.
    el.toDataURL = function() { return 'data:,'; };
    el.toBlob = function(cb) { if (typeof cb === 'function') cb(null); };
  }

  if (typeof document !== 'undefined' && typeof document.createElement === 'function') {
    var _origCreate = document.createElement.bind(document);
    document.createElement = function(tag) {
      var el = _origCreate(tag);
      if (typeof tag === 'string' && tag.toLowerCase() === 'canvas') {
        _addCanvasStubs(el);
      }
      return el;
    };
  }
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

    fn fp() -> lumen_paint::GpuFingerprint {
        lumen_paint::GpuFingerprint {
            vendor: "WebKit".to_string(),
            renderer: "Generic GPU".to_string(),
        }
    }

    fn install_minimal_dom(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
            r#"var document = {
  createElement: function(tag) {
    return { _tag: tag, width: 8, height: 8,
             getAttribute: function(){ return ''; }, setAttribute: function(){} };
  }
};"#,
        )
        .unwrap();
    }

    #[test]
    fn get_context_returns_functional_object() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_webgl_canvas(&ctx, &fp()).unwrap();
            let ok: bool = ctx
                .eval(
                    r#"var c = document.createElement('canvas');
var gl = c.getContext('webgl');
gl !== null && typeof gl.drawArrays === 'function' && typeof gl.createBuffer === 'function'"#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn get_context_2d_returns_null() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_webgl_canvas(&ctx, &fp()).unwrap();
            let ok: bool = ctx
                .eval("document.createElement('canvas').getContext('2d') === null")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn same_context_returned_on_repeated_calls() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_webgl_canvas(&ctx, &fp()).unwrap();
            let ok: bool = ctx
                .eval(
                    r#"var c = document.createElement('canvas');
c.getContext('webgl') === c.getContext('webgl')"#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn fingerprint_vendor_is_normalized() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_webgl_canvas(&ctx, &fp()).unwrap();
            let vendor: String = ctx
                .eval(
                    r#"var gl = document.createElement('canvas').getContext('webgl');
var ext = gl.getExtension('WEBGL_debug_renderer_info');
gl.getParameter(ext.UNMASKED_VENDOR_WEBGL)"#,
                )
                .unwrap();
            assert_eq!(vendor, "WebKit");
        });
    }

    #[test]
    fn to_data_url_is_blank() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_webgl_canvas(&ctx, &fp()).unwrap();
            let url: String = ctx
                .eval("document.createElement('canvas').toDataURL()")
                .unwrap();
            assert_eq!(url, "data:,");
        });
    }

    #[test]
    fn clear_then_read_pixels_roundtrip() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_webgl_canvas(&ctx, &fp()).unwrap();
            // Clear to opaque red, read back the bottom-left pixel.
            let r: f64 = ctx
                .eval(
                    r#"var gl = document.createElement('canvas').getContext('webgl');
gl.clearColor(1.0, 0.0, 0.0, 1.0);
gl.clear(gl.COLOR_BUFFER_BIT);
var px = new Uint8Array(4);
gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
px[0]"#,
                )
                .unwrap();
            assert_eq!(r as i32, 255);
        });
    }

    #[test]
    fn full_draw_pipeline_paints_pixels() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_webgl_canvas(&ctx, &fp()).unwrap();
            // Compile a program, upload a fullscreen quad, draw green, read centre.
            let g: f64 = ctx
                .eval(
                    r#"var gl = document.createElement('canvas').getContext('webgl');
var vs = gl.createShader(gl.VERTEX_SHADER);
gl.shaderSource(vs, 'void main(){}'); gl.compileShader(vs);
var fs = gl.createShader(gl.FRAGMENT_SHADER);
gl.shaderSource(fs, 'void main(){}'); gl.compileShader(fs);
var prog = gl.createProgram();
gl.attachShader(prog, vs); gl.attachShader(prog, fs);
gl.linkProgram(prog); gl.useProgram(prog);
if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error('link');
var buf = gl.createBuffer();
gl.bindBuffer(gl.ARRAY_BUFFER, buf);
var verts = new Float32Array([-1,-1, 1,-1, -1,1, -1,1, 1,-1, 1,1]);
gl.bufferData(gl.ARRAY_BUFFER, verts, gl.STATIC_DRAW);
var loc = gl.getAttribLocation(prog, 'a_pos');
gl.enableVertexAttribArray(loc);
gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
var u = gl.getUniformLocation(prog, 'u_color');
gl.uniform4f(u, 0.0, 1.0, 0.0, 1.0);
gl.viewport(0, 0, 8, 8);
gl.drawArrays(gl.TRIANGLES, 0, 6);
var px = new Uint8Array(4);
gl.readPixels(4, 4, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
px[1]"#,
                )
                .unwrap();
            assert_eq!(g as i32, 255);
        });
    }

    #[test]
    fn attrib_location_is_nonnegative() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_webgl_canvas(&ctx, &fp()).unwrap();
            let loc: f64 = ctx
                .eval(
                    r#"var gl = document.createElement('canvas').getContext('webgl');
var p = gl.createProgram();
gl.getAttribLocation(p, 'a_pos')"#,
                )
                .unwrap();
            assert!(loc >= 0.0);
        });
    }

    #[test]
    fn non_canvas_has_no_get_context() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_webgl_canvas(&ctx, &fp()).unwrap();
            let has: bool = ctx
                .eval("typeof document.createElement('div').getContext === 'function'")
                .unwrap();
            assert!(!has);
        });
    }

    #[test]
    fn lose_context_extension_present() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_webgl_canvas(&ctx, &fp()).unwrap();
            let ok: bool = ctx
                .eval(
                    r#"var gl = document.createElement('canvas').getContext('webgl');
var e = gl.getExtension('WEBGL_lose_context');
e !== null && typeof e.loseContext === 'function'"#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
