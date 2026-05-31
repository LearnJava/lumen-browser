//! WebGL API bindings for normalized GPU fingerprinting (ADR-007 Layer 4, 9D.2).
//!
//! Intercepts `canvas.getContext('webgl'/'webgl2')` to return a stub context
//! whose `getParameter(UNMASKED_VENDOR_WEBGL)` and `getParameter(UNMASKED_RENDERER_WEBGL)`
//! return the normalized strings from `GpuFingerprint` ("WebKit" / "Generic GPU").
//!
//! Also intercepts `canvas.toDataURL()` / `canvas.toBlob()` to prevent canvas
//! pixel fingerprinting.
//!
//! The approach follows the same IIFE-shim pattern as `audio_bindings` and
//! `navigator_bindings`: inject JavaScript that wraps `document.createElement`
//! before any user script runs.

use lumen_paint::GpuFingerprint;
use rquickjs::Ctx;

/// Install WebGL fingerprint shim into the JS context.
///
/// Sets `_LUMEN_GPU_VENDOR` / `_LUMEN_GPU_RENDERER` globals and evaluates
/// a shim that intercepts `document.createElement('canvas')` so that
/// `canvas.getContext('webgl')` returns a stub with normalized
/// `getParameter(UNMASKED_VENDOR_WEBGL)` / `getParameter(UNMASKED_RENDERER_WEBGL)`.
///
/// Must be called **before** any user script that touches the WebGL API.
pub fn install_webgl_bindings(ctx: &Ctx, fingerprint: &GpuFingerprint) -> rquickjs::Result<()> {
    ctx.globals()
        .set("_LUMEN_GPU_VENDOR", fingerprint.vendor().to_string())?;
    ctx.globals()
        .set("_LUMEN_GPU_RENDERER", fingerprint.renderer().to_string())?;
    ctx.eval::<(), _>(WEBGL_SHIM)?;
    Ok(())
}

/// JavaScript shim: intercepts `document.createElement('canvas')` and adds a
/// WebGL stub context with normalized GPU strings.
///
/// The IIFE captures `_LUMEN_GPU_VENDOR` / `_LUMEN_GPU_RENDERER` at evaluation
/// time. The shim is injected before user scripts so that any code reading
/// `UNMASKED_VENDOR_WEBGL` / `UNMASKED_RENDERER_WEBGL` gets the spoofed value.
const WEBGL_SHIM: &str = r#"(function() {
  var _vendor   = (typeof _LUMEN_GPU_VENDOR   !== 'undefined') ? _LUMEN_GPU_VENDOR   : 'WebKit';
  var _renderer = (typeof _LUMEN_GPU_RENDERER !== 'undefined') ? _LUMEN_GPU_RENDERER : 'Generic GPU';

  // WEBGL_debug_renderer_info extension constants (WebGL spec).
  var UNMASKED_VENDOR_WEBGL   = 0x9245;
  var UNMASKED_RENDERER_WEBGL = 0x9246;

  // Standard WebGL parameter constants commonly probed by fingerprinters.
  var GL_VENDOR                   = 0x1F00;
  var GL_RENDERER                 = 0x1F01;
  var GL_VERSION                  = 0x1F02;
  var GL_SHADING_LANGUAGE_VERSION = 0x8B8C;
  var GL_MAX_TEXTURE_SIZE         = 0x0D33;
  var GL_MAX_VIEWPORT_DIMS        = 0x0D3A;

  function _makeWebGLContext() {
    return {
      getParameter: function(pname) {
        if (pname === UNMASKED_VENDOR_WEBGL)       return _vendor;
        if (pname === UNMASKED_RENDERER_WEBGL)     return _renderer;
        if (pname === GL_VENDOR)                   return _vendor;
        if (pname === GL_RENDERER)                 return _renderer;
        if (pname === GL_VERSION)                  return 'WebGL 1.0';
        if (pname === GL_SHADING_LANGUAGE_VERSION) return 'WebGL GLSL ES 1.0';
        if (pname === GL_MAX_TEXTURE_SIZE)         return 4096;
        if (pname === GL_MAX_VIEWPORT_DIMS)        return [4096, 4096];
        return null;
      },
      getExtension: function(name) {
        if (name === 'WEBGL_debug_renderer_info') {
          return {
            UNMASKED_VENDOR_WEBGL:   UNMASKED_VENDOR_WEBGL,
            UNMASKED_RENDERER_WEBGL: UNMASKED_RENDERER_WEBGL
          };
        }
        return null;
      },
      getSupportedExtensions: function() {
        return ['WEBGL_debug_renderer_info'];
      },
      isContextLost: function() { return false; }
    };
  }

  function _addCanvasStubs(el) {
    el.getContext = function(contextType) {
      var t = (contextType || '').toLowerCase();
      if (t === 'webgl' || t === 'webgl2' || t === 'experimental-webgl') {
        return _makeWebGLContext();
      }
      return null;
    };
    // Blank data URL — prevents canvas pixel-hash fingerprinting.
    el.toDataURL = function() { return 'data:,'; };
    el.toBlob    = function(cb) { if (typeof cb === 'function') cb(null); };
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

    fn make_fp(vendor: &str, renderer: &str) -> GpuFingerprint {
        GpuFingerprint {
            vendor: vendor.to_string(),
            renderer: renderer.to_string(),
        }
    }

    fn install_minimal_dom(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
            r#"var document = {
  createElement: function(tag) {
    return { _tag: tag, getAttribute: function(){ return ''; }, setAttribute: function(){} };
  }
};"#,
        )
        .unwrap();
    }

    #[test]
    fn globals_are_set() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            let fp = make_fp("WebKit", "Generic GPU");
            install_webgl_bindings(&ctx, &fp).unwrap();
            let vendor: String = ctx.eval("_LUMEN_GPU_VENDOR").unwrap();
            let renderer: String = ctx.eval("_LUMEN_GPU_RENDERER").unwrap();
            assert_eq!(vendor, "WebKit");
            assert_eq!(renderer, "Generic GPU");
        });
    }

    #[test]
    fn install_succeeds_without_document() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            let fp = make_fp("WebKit", "Generic GPU");
            install_webgl_bindings(&ctx, &fp).expect("should not fail without document");
        });
    }

    #[test]
    fn install_succeeds_with_minimal_dom() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            let fp = make_fp("WebKit", "Generic GPU");
            install_webgl_bindings(&ctx, &fp).expect("should not fail with minimal dom");
        });
    }

    #[test]
    fn get_context_webgl_returns_object() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            let fp = make_fp("WebKit", "Generic GPU");
            install_webgl_bindings(&ctx, &fp).unwrap();
            let ok: bool = ctx
                .eval(
                    r#"var canvas = document.createElement('canvas');
var gl = canvas.getContext('webgl');
gl !== null && typeof gl === 'object'"#,
                )
                .unwrap();
            assert!(ok, "getContext('webgl') should return a non-null object");
        });
    }

    #[test]
    fn get_context_webgl2_returns_object() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            let fp = make_fp("WebKit", "Generic GPU");
            install_webgl_bindings(&ctx, &fp).unwrap();
            let ok: bool = ctx
                .eval(
                    r#"var canvas = document.createElement('canvas');
canvas.getContext('webgl2') !== null"#,
                )
                .unwrap();
            assert!(ok, "getContext('webgl2') should return non-null");
        });
    }

    #[test]
    fn get_context_2d_returns_null() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            let fp = make_fp("WebKit", "Generic GPU");
            install_webgl_bindings(&ctx, &fp).unwrap();
            let ok: bool = ctx
                .eval(
                    r#"var canvas = document.createElement('canvas');
canvas.getContext('2d') === null"#,
                )
                .unwrap();
            assert!(ok, "getContext('2d') should return null (not a WebGL context)");
        });
    }

    #[test]
    fn get_extension_returns_constants() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            let fp = make_fp("WebKit", "Generic GPU");
            install_webgl_bindings(&ctx, &fp).unwrap();
            let ok: bool = ctx
                .eval(
                    r#"var canvas = document.createElement('canvas');
var gl = canvas.getContext('webgl');
var ext = gl.getExtension('WEBGL_debug_renderer_info');
ext !== null &&
ext.UNMASKED_VENDOR_WEBGL === 0x9245 &&
ext.UNMASKED_RENDERER_WEBGL === 0x9246"#,
                )
                .unwrap();
            assert!(ok, "WEBGL_debug_renderer_info should expose correct constants");
        });
    }

    #[test]
    fn get_parameter_unmasked_vendor() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            let fp = make_fp("WebKit", "Generic GPU");
            install_webgl_bindings(&ctx, &fp).unwrap();
            let vendor: String = ctx
                .eval(
                    r#"var canvas = document.createElement('canvas');
var gl = canvas.getContext('webgl');
var ext = gl.getExtension('WEBGL_debug_renderer_info');
gl.getParameter(ext.UNMASKED_VENDOR_WEBGL)"#,
                )
                .unwrap();
            assert_eq!(vendor, "WebKit");
        });
    }

    #[test]
    fn get_parameter_unmasked_renderer() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            let fp = make_fp("WebKit", "Generic GPU");
            install_webgl_bindings(&ctx, &fp).unwrap();
            let renderer: String = ctx
                .eval(
                    r#"var canvas = document.createElement('canvas');
var gl = canvas.getContext('webgl');
var ext = gl.getExtension('WEBGL_debug_renderer_info');
gl.getParameter(ext.UNMASKED_RENDERER_WEBGL)"#,
                )
                .unwrap();
            assert_eq!(renderer, "Generic GPU");
        });
    }

    #[test]
    fn custom_fingerprint_reflected() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            let fp = make_fp("Chromium", "ANGLE (NVIDIA)");
            install_webgl_bindings(&ctx, &fp).unwrap();
            let vendor: String = ctx
                .eval(
                    r#"var canvas = document.createElement('canvas');
var gl = canvas.getContext('experimental-webgl');
var ext = gl.getExtension('WEBGL_debug_renderer_info');
gl.getParameter(ext.UNMASKED_VENDOR_WEBGL)"#,
                )
                .unwrap();
            assert_eq!(vendor, "Chromium");
        });
    }

    #[test]
    fn to_data_url_returns_blank() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            let fp = make_fp("WebKit", "Generic GPU");
            install_webgl_bindings(&ctx, &fp).unwrap();
            let url: String = ctx
                .eval(
                    r#"var canvas = document.createElement('canvas');
canvas.toDataURL()"#,
                )
                .unwrap();
            assert_eq!(url, "data:,");
        });
    }

    #[test]
    fn non_canvas_element_unaffected() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            let fp = make_fp("WebKit", "Generic GPU");
            install_webgl_bindings(&ctx, &fp).unwrap();
            let has_get_context: bool = ctx
                .eval(
                    r#"var div = document.createElement('div');
typeof div.getContext === 'function'"#,
                )
                .unwrap();
            assert!(!has_get_context, "non-canvas elements should not have getContext");
        });
    }
}
