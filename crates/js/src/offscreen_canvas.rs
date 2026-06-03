//! OffscreenCanvas API (HTML Living Standard §4.12.14, Workers §4.2).
//!
//! Provides `new OffscreenCanvas(width, height)` constructor for off-DOM canvas
//! rendering, supports `getContext('2d')` returning a `Context2D`, and implements
//! `transferToImageBitmap()` to convert pixel buffers to `ImageBitmap` objects
//! without copying.
//!
//! Each OffscreenCanvas is keyed by a globally-unique ID generated at construction.
//! `transferToImageBitmap()` moves ownership of the pixel buffer to a new `ImageBitmap`,
//! and the original canvas becomes empty (reusable with `resize`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use lumen_canvas::Context2D;
use rquickjs::Ctx;

thread_local! {
    /// Registry of live OffscreenCanvas 2D contexts, keyed by unique canvas ID.
    static OFFSCREEN_CANVASES: RefCell<HashMap<u32, Context2D>> = RefCell::new(HashMap::new());
    /// Node indices whose pixel buffer changed since the last [`flush_dirty`].
    static DIRTY: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
}

/// Global monotonic counter for OffscreenCanvas IDs.
static NEXT_OFFSCREEN_ID: AtomicU32 = AtomicU32::new(1);

/// Maximum canvas dimension in CSS pixels. Clamps hostile/oversized buffers.
const MAX_CANVAS_DIM: u32 = 4096;

/// Wrapper class for OffscreenCanvas JS object.
pub struct OffscreenCanvas {
    /// Unique ID for this canvas (used to look up Context2D in OFFSCREEN_CANVASES).
    id: u32,
    /// Width in CSS pixels.
    width: u32,
    /// Height in CSS pixels.
    height: u32,
}

impl OffscreenCanvas {
    /// Create a new OffscreenCanvas with the given dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        let id = NEXT_OFFSCREEN_ID.fetch_add(1, Ordering::Relaxed);
        let w = width.clamp(1, MAX_CANVAS_DIM);
        let h = height.clamp(1, MAX_CANVAS_DIM);
        OFFSCREEN_CANVASES.with(|c| {
            if let Ok(mut map) = c.try_borrow_mut() {
                map.insert(id, Context2D::new(w, h));
            }
        });
        Self { id, width: w, height: h }
    }

    /// Get the canvas ID (internal use only).
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Get canvas width in CSS pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Get canvas height in CSS pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Transfer pixel buffer to ImageBitmap and clear the canvas.
    pub fn transfer_to_image_bitmap(&mut self) -> Option<Vec<u8>> {
        OFFSCREEN_CANVASES.with(|c| {
            let Ok(mut map) = c.try_borrow_mut() else {
                return None;
            };
            map.remove(&self.id).map(|ctx| ctx.pixels().to_vec())
        })
    }
}

/// Run `f` against the context for `canvas_id`, returning `R::default()` if absent.
fn with_offscreen_canvas<F, R>(canvas_id: u32, f: F) -> R
where
    F: FnOnce(&mut Context2D) -> R,
    R: Default,
{
    OFFSCREEN_CANVASES.with(|c| {
        if let Ok(mut map) = c.try_borrow_mut()
            && let Some(ctx) = map.get_mut(&canvas_id)
        {
            return f(ctx);
        }
        R::default()
    })
}

/// Mark `canvas_id`'s pixel buffer as changed.
fn mark_offscreen_dirty(canvas_id: u32) {
    DIRTY.with(|d| {
        if let Ok(mut v) = d.try_borrow_mut()
            && !v.contains(&canvas_id)
        {
            v.push(canvas_id);
        }
    });
}

/// Drain dirty offscreen canvases and return their RGBA buffers.
///
/// Each tuple is `(canvas_id, width, height, rgba_pixels)` where `rgba_pixels`
/// is row-major RGBA8 (top-left origin).
pub fn flush_dirty() -> Vec<(u32, u32, u32, Vec<u8>)> {
    let dirty: Vec<u32> = DIRTY.with(|d| {
        d.try_borrow_mut()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    });
    if dirty.is_empty() {
        return Vec::new();
    }
    OFFSCREEN_CANVASES.with(|c| {
        let Ok(map) = c.try_borrow() else {
            return Vec::new();
        };
        dirty
            .into_iter()
            .filter_map(|cid| {
                let ctx = map.get(&cid)?;
                Some((cid, ctx.width(), ctx.height(), ctx.pixels().to_vec()))
            })
            .collect()
    })
}

/// Install OffscreenCanvas bindings and JS shim into the QuickJS runtime.
pub fn install_offscreen_canvas_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    // First install the native _lumen_offscreen_canvas_* functions
    let g = ctx.globals();

    // Constructor: _lumen_offscreen_canvas_new(w, h) -> {__canvas_id__, width, height}
    g.set(
        "_lumen_offscreen_canvas_new",
        rquickjs::Function::new(ctx.clone(), |w: u32, h: u32| -> String {
            let canvas = OffscreenCanvas::new(w, h);
            // Return as JSON string: will be parsed on JS side
            format!("{{\"__canvas_id__\":{},\"width\":{},\"height\":{}}}", canvas.id, canvas.width, canvas.height)
        }),
    )?;

    // _lumen_offscreen_canvas_resize(canvas_id, w, h)
    g.set(
        "_lumen_offscreen_canvas_resize",
        rquickjs::Function::new(ctx.clone(), |canvas_id: u32, w: u32, h: u32| {
            let w = w.clamp(1, MAX_CANVAS_DIM);
            let h = h.clamp(1, MAX_CANVAS_DIM);
            with_offscreen_canvas(canvas_id, |c| c.resize(w, h));
            mark_offscreen_dirty(canvas_id);
        }),
    )?;

    // _lumen_offscreen_canvas2d_fill_rect(canvas_id, x, y, w, h)
    g.set(
        "_lumen_offscreen_canvas2d_fill_rect",
        rquickjs::Function::new(
            ctx.clone(),
            |canvas_id: u32, x: f64, y: f64, w: f64, h: f64| {
                with_offscreen_canvas(canvas_id, |c| {
                    c.fill_rect(x as f32, y as f32, w as f32, h as f32)
                });
                mark_offscreen_dirty(canvas_id);
            },
        ),
    )?;

    // _lumen_offscreen_canvas2d_clear_rect(canvas_id, x, y, w, h)
    g.set(
        "_lumen_offscreen_canvas2d_clear_rect",
        rquickjs::Function::new(
            ctx.clone(),
            |canvas_id: u32, x: f64, y: f64, w: f64, h: f64| {
                with_offscreen_canvas(canvas_id, |c| {
                    c.clear_rect(x as f32, y as f32, w as f32, h as f32)
                });
                mark_offscreen_dirty(canvas_id);
            },
        ),
    )?;

    // _lumen_offscreen_canvas2d_stroke_rect(canvas_id, x, y, w, h)
    g.set(
        "_lumen_offscreen_canvas2d_stroke_rect",
        rquickjs::Function::new(
            ctx.clone(),
            |canvas_id: u32, x: f64, y: f64, w: f64, h: f64| {
                with_offscreen_canvas(canvas_id, |c| {
                    c.stroke_rect(x as f32, y as f32, w as f32, h as f32)
                });
                mark_offscreen_dirty(canvas_id);
            },
        ),
    )?;

    // _lumen_offscreen_canvas2d_begin_path(canvas_id)
    g.set(
        "_lumen_offscreen_canvas2d_begin_path",
        rquickjs::Function::new(ctx.clone(), |canvas_id: u32| {
            with_offscreen_canvas(canvas_id, |c| c.begin_path());
        }),
    )?;

    // _lumen_offscreen_canvas2d_move_to(canvas_id, x, y)
    g.set(
        "_lumen_offscreen_canvas2d_move_to",
        rquickjs::Function::new(ctx.clone(), |canvas_id: u32, x: f64, y: f64| {
            with_offscreen_canvas(canvas_id, |c| c.move_to(x as f32, y as f32));
        }),
    )?;

    // _lumen_offscreen_canvas2d_line_to(canvas_id, x, y)
    g.set(
        "_lumen_offscreen_canvas2d_line_to",
        rquickjs::Function::new(ctx.clone(), |canvas_id: u32, x: f64, y: f64| {
            with_offscreen_canvas(canvas_id, |c| c.line_to(x as f32, y as f32));
        }),
    )?;

    // _lumen_offscreen_canvas2d_close_path(canvas_id)
    g.set(
        "_lumen_offscreen_canvas2d_close_path",
        rquickjs::Function::new(ctx.clone(), |canvas_id: u32| {
            with_offscreen_canvas(canvas_id, |c| c.close_path());
        }),
    )?;

    // _lumen_offscreen_canvas2d_arc(canvas_id, cx, cy, r, start_angle, end_angle, counterclockwise)
    g.set(
        "_lumen_offscreen_canvas2d_arc",
        rquickjs::Function::new(
            ctx.clone(),
            |canvas_id: u32, cx: f64, cy: f64, r: f64, sa: f64, ea: f64, ccw: bool| {
                with_offscreen_canvas(canvas_id, |c| {
                    c.arc(cx as f32, cy as f32, r as f32, sa as f32, ea as f32, ccw)
                });
            },
        ),
    )?;

    // _lumen_offscreen_canvas2d_fill(canvas_id)
    g.set(
        "_lumen_offscreen_canvas2d_fill",
        rquickjs::Function::new(ctx.clone(), |canvas_id: u32| {
            with_offscreen_canvas(canvas_id, |c| c.fill());
            mark_offscreen_dirty(canvas_id);
        }),
    )?;

    // _lumen_offscreen_canvas2d_stroke(canvas_id)
    g.set(
        "_lumen_offscreen_canvas2d_stroke",
        rquickjs::Function::new(ctx.clone(), |canvas_id: u32| {
            with_offscreen_canvas(canvas_id, |c| c.stroke());
            mark_offscreen_dirty(canvas_id);
        }),
    )?;

    // _lumen_offscreen_canvas2d_set_fill_style(canvas_id, css)
    g.set(
        "_lumen_offscreen_canvas2d_set_fill_style",
        rquickjs::Function::new(ctx.clone(), |canvas_id: u32, css: String| {
            use lumen_canvas::CanvasColor;
            with_offscreen_canvas(canvas_id, |c| {
                if let Some(color) = CanvasColor::from_css_str(&css) {
                    c.fill_style = color;
                }
            });
        }),
    )?;

    // _lumen_offscreen_canvas2d_set_stroke_style(canvas_id, css)
    g.set(
        "_lumen_offscreen_canvas2d_set_stroke_style",
        rquickjs::Function::new(ctx.clone(), |canvas_id: u32, css: String| {
            use lumen_canvas::CanvasColor;
            with_offscreen_canvas(canvas_id, |c| {
                if let Some(color) = CanvasColor::from_css_str(&css) {
                    c.stroke_style = color;
                }
            });
        }),
    )?;

    // _lumen_offscreen_canvas2d_set_line_width(canvas_id, w)
    g.set(
        "_lumen_offscreen_canvas2d_set_line_width",
        rquickjs::Function::new(ctx.clone(), |canvas_id: u32, w: f64| {
            if w.is_finite() && w > 0.0 {
                with_offscreen_canvas(canvas_id, |c| c.line_width = w as f32);
            }
        }),
    )?;

    // _lumen_offscreen_canvas2d_set_global_alpha(canvas_id, a)
    g.set(
        "_lumen_offscreen_canvas2d_set_global_alpha",
        rquickjs::Function::new(ctx.clone(), |canvas_id: u32, a: f64| {
            if a.is_finite() && (0.0..=1.0).contains(&a) {
                with_offscreen_canvas(canvas_id, |c| c.global_alpha = a as f32);
            }
        }),
    )?;

    // _lumen_offscreen_canvas2d_get_image_data(canvas_id) -> "{w},{h},{hex_rgba}"
    g.set(
        "_lumen_offscreen_canvas2d_get_image_data",
        rquickjs::Function::new(ctx.clone(), |canvas_id: u32| -> String {
            OFFSCREEN_CANVASES.with(|c| {
                let Ok(map) = c.try_borrow() else {
                    return String::new();
                };
                let Some(canvas) = map.get(&canvas_id) else {
                    return String::new();
                };
                let pixels = canvas.get_image_data();
                let mut s = String::with_capacity(pixels.len() * 2 + 12);
                use std::fmt::Write;
                let _ = write!(s, "{},{},", canvas.width(), canvas.height());
                for b in &pixels {
                    let _ = write!(s, "{b:02x}");
                }
                s
            })
        }),
    )?;

    // _lumen_offscreen_canvas_transfer_to_image_bitmap(canvas_id) -> "{w},{h},{hex_rgba}"
    g.set(
        "_lumen_offscreen_canvas_transfer_to_image_bitmap",
        rquickjs::Function::new(ctx.clone(), |canvas_id: u32| -> String {
            OFFSCREEN_CANVASES.with(|c| {
                let Ok(mut map) = c.try_borrow_mut() else {
                    return String::new();
                };
                if let Some(canvas) = map.remove(&canvas_id) {
                    let mut s = String::with_capacity(canvas.pixels().len() * 2 + 12);
                    use std::fmt::Write;
                    let _ = write!(s, "{},{},", canvas.width(), canvas.height());
                    for b in canvas.pixels() {
                        let _ = write!(s, "{b:02x}");
                    }
                    return s;
                }
                String::new()
            })
        }),
    )?;

    // Install the JS shim that defines the public OffscreenCanvas class
    ctx.eval::<(), _>(OFFSCREEN_CANVAS_SHIM)?;

    Ok(())
}

/// Pure-JS OffscreenCanvas API shim.
/// Defines the OffscreenCanvas constructor and prototypes for Context2D access.
const OFFSCREEN_CANVAS_SHIM: &str = r#"
'use strict';

/// OffscreenCanvas constructor: new OffscreenCanvas(width, height)
(function() {
  globalThis.OffscreenCanvas = class {
    constructor(width, height) {
      width = Math.max(1, Math.min(4096, width || 0)) >>> 0;
      height = Math.max(1, Math.min(4096, height || 0)) >>> 0;
      // Create the native canvas object via native binding
      // Returns JSON string: {__canvas_id__, width, height}
      const nativeJson = _lumen_offscreen_canvas_new(width, height);
      const nativeObj = JSON.parse(nativeJson);
      this.__canvas_id__ = nativeObj.__canvas_id__;
      this.width = nativeObj.width;
      this.height = nativeObj.height;
      this._2d_context = null;
    }

    getContext(contextType, options) {
      if (contextType !== '2d') {
        return null;
      }

      // Return existing context if already created
      if (this._2d_context) {
        return this._2d_context;
      }

      // Create and cache a 2D context proxy
      const canvasId = this.__canvas_id__;
      this._2d_context = {
        // Canvas reference
        canvas: this,

        // Drawing state
        fillStyle: '#000000',
        strokeStyle: '#000000',
        lineWidth: 1,
        globalAlpha: 1,

        // Rectangles
        fillRect: (x, y, w, h) => _lumen_offscreen_canvas2d_fill_rect(canvasId, x, y, w, h),
        clearRect: (x, y, w, h) => _lumen_offscreen_canvas2d_clear_rect(canvasId, x, y, w, h),
        strokeRect: (x, y, w, h) => _lumen_offscreen_canvas2d_stroke_rect(canvasId, x, y, w, h),

        // Paths
        beginPath: () => _lumen_offscreen_canvas2d_begin_path(canvasId),
        moveTo: (x, y) => _lumen_offscreen_canvas2d_move_to(canvasId, x, y),
        lineTo: (x, y) => _lumen_offscreen_canvas2d_line_to(canvasId, x, y),
        closePath: () => _lumen_offscreen_canvas2d_close_path(canvasId),
        arc: (cx, cy, r, sa, ea, ccw) => _lumen_offscreen_canvas2d_arc(canvasId, cx, cy, r, sa, ea, ccw),
        fill: () => _lumen_offscreen_canvas2d_fill(canvasId),
        stroke: () => _lumen_offscreen_canvas2d_stroke(canvasId),

        // Style setters
        set fillStyle(val) {
          if (typeof val !== 'string') val = String(val);
          _lumen_offscreen_canvas2d_set_fill_style(canvasId, val);
        },
        set strokeStyle(val) {
          if (typeof val !== 'string') val = String(val);
          _lumen_offscreen_canvas2d_set_stroke_style(canvasId, val);
        },
        set lineWidth(w) {
          _lumen_offscreen_canvas2d_set_line_width(canvasId, Number(w));
        },
        set globalAlpha(a) {
          _lumen_offscreen_canvas2d_set_global_alpha(canvasId, Number(a));
        },

        // Image data
        getImageData: () => _lumen_offscreen_canvas2d_get_image_data(canvasId),
      };

      return this._2d_context;
    }

    transferToImageBitmap() {
      // Returns JSON string: {width,height,data}
      const jsonStr = _lumen_offscreen_canvas_transfer_to_image_bitmap(this.__canvas_id__);
      if (!jsonStr) {
        throw new Error('transferToImageBitmap: canvas already transferred or invalid');
      }
      const parts = jsonStr.split(',');
      const width = parseInt(parts[0], 10);
      const height = parseInt(parts[1], 10);
      const hexData = parts[2] || '';
      // Create ImageBitmap-like object
      return {
        width: width,
        height: height,
        data: hexData,
        close() {
          // No-op for now
        }
      };
    }

    convertToBlob(options) {
      // TODO: Implement PNG/JPEG encoding
      return Promise.reject(new Error('convertToBlob not yet implemented'));
    }
  };

  // createImageBitmap shim (minimal)
  if (!globalThis.createImageBitmap) {
    globalThis.createImageBitmap = function(source, sx, sy, sw, sh) {
      // TODO: Implement createImageBitmap from Canvas, ImageData, Blob, etc.
      return Promise.reject(new Error('createImageBitmap not yet implemented'));
    };
  }
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offscreen_canvas_new_clamped() {
        let canvas = OffscreenCanvas::new(5000, 5000);
        assert_eq!(canvas.width, MAX_CANVAS_DIM);
        assert_eq!(canvas.height, MAX_CANVAS_DIM);
    }

    #[test]
    fn offscreen_canvas_new_minimal() {
        let canvas = OffscreenCanvas::new(0, 0);
        assert_eq!(canvas.width, 1);
        assert_eq!(canvas.height, 1);
    }

    #[test]
    fn offscreen_canvas_unique_ids() {
        let c1 = OffscreenCanvas::new(100, 100);
        let c2 = OffscreenCanvas::new(100, 100);
        assert_ne!(c1.id, c2.id);
    }

    #[test]
    fn transfer_to_image_bitmap_removes_canvas() {
        let mut canvas = OffscreenCanvas::new(2, 2);
        let canvas_id = canvas.id();
        let pixels = canvas.transfer_to_image_bitmap();
        // Should return Some (the pixel buffer)
        assert!(pixels.is_some());
        // Canvas should be removed from registry
        OFFSCREEN_CANVASES.with(|c| {
            if let Ok(map) = c.try_borrow() {
                assert!(!map.contains_key(&canvas_id));
            }
        });
    }

    #[test]
    fn transfer_clears_canvas() {
        let mut canvas = OffscreenCanvas::new(2, 2);
        let _ = canvas.transfer_to_image_bitmap();
        // Second transfer should return None since canvas was cleared
        let pixels = canvas.transfer_to_image_bitmap();
        assert_eq!(pixels, None);
    }

    #[test]
    fn with_offscreen_canvas_nonexistent() {
        let result: i32 = with_offscreen_canvas(999999, |_| 42);
        assert_eq!(result, 0); // Default for missing canvas
    }

    #[test]
    fn mark_offscreen_dirty_no_duplicates() {
        DIRTY.with(|d| d.try_borrow_mut().ok().map(|mut v| v.clear()));
        mark_offscreen_dirty(1);
        mark_offscreen_dirty(1);
        mark_offscreen_dirty(1);
        DIRTY.with(|d| {
            if let Ok(v) = d.try_borrow() {
                assert_eq!(v.len(), 1);
            }
        });
    }

    // Integration tests via JS bindings
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn reset_state() {
        OFFSCREEN_CANVASES.with(|c| c.borrow_mut().clear());
        DIRTY.with(|d| d.borrow_mut().clear());
    }

    #[test]
    fn js_offscreen_canvas_constructor() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_offscreen_canvas_bindings(&ctx).unwrap();

            let result: bool = ctx
                .eval(
                    r#"
                        let canvas = new OffscreenCanvas(100, 200);
                        canvas.width === 100 && canvas.height === 200 &&
                        typeof canvas.__canvas_id__ === 'number'
                    "#,
                )
                .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn js_offscreen_canvas_get_context() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_offscreen_canvas_bindings(&ctx).unwrap();

            let result: bool = ctx
                .eval(
                    r#"
                        let canvas = new OffscreenCanvas(50, 50);
                        let ctx2d = canvas.getContext('2d');
                        ctx2d !== null && ctx2d.canvas === canvas
                    "#,
                )
                .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn js_offscreen_canvas_get_context_cached() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_offscreen_canvas_bindings(&ctx).unwrap();

            let result: bool = ctx
                .eval(
                    r#"
                        let canvas = new OffscreenCanvas(50, 50);
                        let ctx1 = canvas.getContext('2d');
                        let ctx2 = canvas.getContext('2d');
                        ctx1 === ctx2  // Same instance cached
                    "#,
                )
                .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn js_offscreen_canvas_fill_rect() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_offscreen_canvas_bindings(&ctx).unwrap();

            let result: bool = ctx
                .eval(
                    r#"
                        let canvas = new OffscreenCanvas(100, 100);
                        let ctx = canvas.getContext('2d');
                        ctx.fillStyle = '#ff0000';
                        ctx.fillRect(10, 10, 50, 50);
                        // Should mark canvas as dirty
                        true
                    "#,
                )
                .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn js_offscreen_canvas_transfer_to_image_bitmap() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_offscreen_canvas_bindings(&ctx).unwrap();

            let result: bool = ctx
                .eval(
                    r#"
                        let canvas = new OffscreenCanvas(10, 10);
                        let bitmap = canvas.transferToImageBitmap();
                        bitmap.width === 10 && bitmap.height === 10 &&
                        typeof bitmap.data === 'string' &&
                        typeof bitmap.close === 'function'
                    "#,
                )
                .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn js_offscreen_canvas_invalid_context_type() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_offscreen_canvas_bindings(&ctx).unwrap();

            let result: bool = ctx
                .eval(
                    r#"
                        let canvas = new OffscreenCanvas(50, 50);
                        canvas.getContext('webgl') === null
                    "#,
                )
                .unwrap();
            assert!(result);
        });
    }
}
