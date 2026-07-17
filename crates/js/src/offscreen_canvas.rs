//! OffscreenCanvas + `createImageBitmap` (HTML Living Standard §4.12.14, §4.12.5, Workers §4.2).
//!
//! Provides `new OffscreenCanvas(width, height)` constructor for off-DOM canvas
//! rendering, supports `getContext('2d')` returning a `Context2D`, and implements
//! `transferToImageBitmap()` to convert pixel buffers to `ImageBitmap` objects.
//!
//! Each OffscreenCanvas is keyed by a globally-unique ID generated at construction.
//! `transferToImageBitmap()` moves ownership of the pixel buffer to a new `ImageBitmap`,
//! and the original canvas becomes empty (reusable with `resize`).
//!
//! `globalThis.createImageBitmap(source[, sx, sy, sw, sh])` accepts ImageData,
//! OffscreenCanvas (non-destructive snapshot), `<img>` (via [`crate::img_bitmap_store`])
//! and `Blob` (decoded via [`lumen_image::decode`]) sources; every source resolves to
//! the same bitmap shape, `{width, height, __canvas_id__, close()}`, backed by an
//! entry in [`OFFSCREEN_CANVASES`]. `ImageBitmapRenderingContext` (`canvas.getContext
//! ('bitmaprenderer')`, HTML LS §4.12.5.1) and its `transferFromImageBitmap` are wired
//! in `dom.rs`'s `getContext` shim + `canvas2d.rs`'s `_lumen_bitmaprenderer_transfer_from_image_bitmap`
//! native (presents onto a page `<canvas>` via [`crate::canvas2d::present_rgba`]).

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

/// Create a new OffscreenCanvas pre-filled with existing RGBA8 pixel data.
///
/// Used by `transferControlToOffscreen()` to snapshot a DOM canvas into an
/// OffscreenCanvas without going through JS hex encoding. Returns the new canvas ID.
pub fn create_offscreen_from_pixels(w: u32, h: u32, pixels: Vec<u8>) -> u32 {
    let id = NEXT_OFFSCREEN_ID.fetch_add(1, Ordering::Relaxed);
    OFFSCREEN_CANVASES.with(|c| {
        if let Ok(mut map) = c.try_borrow_mut() {
            map.insert(id, Context2D::from_pixels(w, h, pixels));
        }
    });
    id
}

/// Remove `canvas_id` from the registry and return its `(width, height, pixels)`.
///
/// Used by `ImageBitmapRenderingContext.transferFromImageBitmap` (via
/// [`crate::canvas2d`]'s `_lumen_bitmaprenderer_transfer_from_image_bitmap` native)
/// to take ownership of a bitmap's pixels — the ImageBitmap is neutered
/// (`__canvas_id__` no longer resolves) once its pixels have been transferred,
/// per HTML LS §4.12.5.1.
pub(crate) fn take_offscreen_pixels(canvas_id: u32) -> Option<(u32, u32, Vec<u8>)> {
    OFFSCREEN_CANVASES.with(|c| {
        c.try_borrow_mut()
            .ok()?
            .remove(&canvas_id)
            .map(|ctx| (ctx.width(), ctx.height(), ctx.pixels().to_vec()))
    })
}

/// Native for `OffscreenCanvas.transferToImageBitmap()`: pops `canvas_id`'s
/// pixels and re-homes them under a fresh canvas ID, unifying the return shape
/// with the other `createImageBitmap` sources (`{__canvas_id__}`, not raw hex).
/// Returns `0` if `canvas_id` is already transferred/invalid.
fn transfer_to_image_bitmap_native(canvas_id: u32) -> u32 {
    match take_offscreen_pixels(canvas_id) {
        Some((w, h, pixels)) => create_offscreen_from_pixels(w, h, pixels),
        None => 0,
    }
}

/// Native for `ImageBitmap.close()`: releases a bitmap's backing pixel buffer
/// (HTML LS §4.12.5.2). No-op if already closed.
fn close_bitmap_native(canvas_id: u32) {
    OFFSCREEN_CANVASES.with(|c| {
        if let Ok(mut map) = c.try_borrow_mut() {
            map.remove(&canvas_id);
        }
    });
}

/// Native for `createImageBitmap(blob)`: decodes `bytes` via
/// [`lumen_image::decode`] (PNG/JPEG/WebP/GIF/AVIF, HTML LS §4.12.5.4 step 5)
/// and stores the resulting RGBA8 pixels as a new offscreen canvas. Returns
/// `0` on decode failure (unrecognised signature or malformed data).
fn decode_image_to_canvas_native(bytes: Vec<u8>) -> u32 {
    match lumen_image::decode(&bytes) {
        Ok(image) => {
            let (w, h) = (image.width, image.height);
            create_offscreen_from_pixels(w, h, image.to_rgba8())
        }
        Err(_) => 0,
    }
}

/// Native for `createImageBitmap(imgElement)`: looks up the `<img>`'s already
/// decoded pixels in [`crate::img_bitmap_store`] (populated by the shell after
/// `fetch_and_decode_images`) and stores them as a new offscreen canvas.
/// Returns `0` when the image has not finished decoding yet.
fn image_bitmap_from_img_nid_native(nid: u32) -> u32 {
    crate::img_bitmap_store::with_img_bitmap(nid, |w, h, pixels| {
        create_offscreen_from_pixels(w, h, pixels.to_vec())
    })
    .unwrap_or(0)
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
            use lumen_canvas::{CanvasColor, PaintSource};
            with_offscreen_canvas(canvas_id, |c| {
                if let Some(color) = CanvasColor::from_css_str(&css) {
                    c.fill_style = PaintSource::Color(color);
                }
            });
        }),
    )?;

    // _lumen_offscreen_canvas2d_set_stroke_style(canvas_id, css)
    g.set(
        "_lumen_offscreen_canvas2d_set_stroke_style",
        rquickjs::Function::new(ctx.clone(), |canvas_id: u32, css: String| {
            use lumen_canvas::{CanvasColor, PaintSource};
            with_offscreen_canvas(canvas_id, |c| {
                if let Some(color) = CanvasColor::from_css_str(&css) {
                    c.stroke_style = PaintSource::Color(color);
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

    // _lumen_offscreen_canvas_transfer_to_image_bitmap(canvas_id) -> new_canvas_id (0 on error)
    g.set(
        "_lumen_offscreen_canvas_transfer_to_image_bitmap",
        rquickjs::Function::new(ctx.clone(), transfer_to_image_bitmap_native),
    )?;

    // _lumen_offscreen_canvas_bitmap_close(canvas_id) — releases a bitmap's pixels.
    g.set(
        "_lumen_offscreen_canvas_bitmap_close",
        rquickjs::Function::new(ctx.clone(), close_bitmap_native),
    )?;

    // _lumen_decode_image_to_canvas(bytes) -> canvas_id (0 on decode failure)
    g.set(
        "_lumen_decode_image_to_canvas",
        rquickjs::Function::new(ctx.clone(), decode_image_to_canvas_native),
    )?;

    // _lumen_image_bitmap_from_img_nid(nid) -> canvas_id (0 if not yet decoded)
    g.set(
        "_lumen_image_bitmap_from_img_nid",
        rquickjs::Function::new(ctx.clone(), image_bitmap_from_img_nid_native),
    )?;

    // _lumen_offscreen_canvas_from_image_data(width, height, hex_rgba) → canvas_id (0 on error)
    //
    // Creates a new OffscreenCanvas pre-filled from RGBA8 hex bytes.
    // Used by createImageBitmap(ImageData) to snapshot pixel data into a bitmap.
    g.set(
        "_lumen_offscreen_canvas_from_image_data",
        rquickjs::Function::new(ctx.clone(), |w: u32, h: u32, hex: String| -> u32 {
            let w = w.clamp(1, MAX_CANVAS_DIM);
            let h = h.clamp(1, MAX_CANVAS_DIM);
            let expected = (w * h * 4) as usize;
            let bytes: Vec<u8> = hex
                .as_bytes()
                .chunks(2)
                .filter_map(|pair| {
                    let s = std::str::from_utf8(pair).ok()?;
                    u8::from_str_radix(s, 16).ok()
                })
                .collect();
            if bytes.len() != expected {
                return 0;
            }
            let id = NEXT_OFFSCREEN_ID.fetch_add(1, Ordering::Relaxed);
            OFFSCREEN_CANVASES.with(|c| {
                if let Ok(mut map) = c.try_borrow_mut() {
                    map.insert(id, Context2D::from_pixels(w, h, bytes));
                }
            });
            id
        }),
    )?;

    // Install the JS shim that defines the public OffscreenCanvas class
    ctx.eval::<(), _>(OFFSCREEN_CANVAS_SHIM)?;

    Ok(())
}

/// V8 port of [`install_offscreen_canvas_bindings`] (Ph3 V8 migration, deferred
/// past S8 — see the note at `canvas2d.rs`'s `_lumen_canvas_transfer_control_to_offscreen`
/// V8 port). State (`OFFSCREEN_CANVASES`, `DIRTY`) is module-level `thread_local!`,
/// not a `V8JsRuntime` field — same pattern as `canvas2d_v8`/`webgl_canvas_v8`.
/// [`OFFSCREEN_CANVAS_SHIM`] is not part of `dom.rs::WEB_API_SHIM` and must be
/// `eval`'d here explicitly, mirroring `webgl_canvas::install_webgl_canvas_v8`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_offscreen_canvas_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn1, into_v8_fn2, into_v8_fn3, into_v8_fn5, into_v8_fn7};
    use lumen_core::ext::JsRuntime as _;

    rt.register_native(
        "_lumen_offscreen_canvas_new",
        into_v8_fn2(|w: u32, h: u32| -> String {
            let canvas = OffscreenCanvas::new(w, h);
            format!(
                "{{\"__canvas_id__\":{},\"width\":{},\"height\":{}}}",
                canvas.id, canvas.width, canvas.height
            )
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas_resize",
        into_v8_fn3(|canvas_id: u32, w: u32, h: u32| {
            let w = w.clamp(1, MAX_CANVAS_DIM);
            let h = h.clamp(1, MAX_CANVAS_DIM);
            with_offscreen_canvas(canvas_id, |c| c.resize(w, h));
            mark_offscreen_dirty(canvas_id);
        }),
    )?;

    rt.register_native(
        "_lumen_offscreen_canvas2d_fill_rect",
        into_v8_fn5(|canvas_id: u32, x: f64, y: f64, w: f64, h: f64| {
            with_offscreen_canvas(canvas_id, |c| {
                c.fill_rect(x as f32, y as f32, w as f32, h as f32)
            });
            mark_offscreen_dirty(canvas_id);
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_clear_rect",
        into_v8_fn5(|canvas_id: u32, x: f64, y: f64, w: f64, h: f64| {
            with_offscreen_canvas(canvas_id, |c| {
                c.clear_rect(x as f32, y as f32, w as f32, h as f32)
            });
            mark_offscreen_dirty(canvas_id);
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_stroke_rect",
        into_v8_fn5(|canvas_id: u32, x: f64, y: f64, w: f64, h: f64| {
            with_offscreen_canvas(canvas_id, |c| {
                c.stroke_rect(x as f32, y as f32, w as f32, h as f32)
            });
            mark_offscreen_dirty(canvas_id);
        }),
    )?;

    rt.register_native(
        "_lumen_offscreen_canvas2d_begin_path",
        into_v8_fn1(|canvas_id: u32| {
            with_offscreen_canvas(canvas_id, |c| c.begin_path());
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_move_to",
        into_v8_fn3(|canvas_id: u32, x: f64, y: f64| {
            with_offscreen_canvas(canvas_id, |c| c.move_to(x as f32, y as f32));
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_line_to",
        into_v8_fn3(|canvas_id: u32, x: f64, y: f64| {
            with_offscreen_canvas(canvas_id, |c| c.line_to(x as f32, y as f32));
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_close_path",
        into_v8_fn1(|canvas_id: u32| {
            with_offscreen_canvas(canvas_id, |c| c.close_path());
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_arc",
        into_v8_fn7(|canvas_id: u32, cx: f64, cy: f64, r: f64, sa: f64, ea: f64, ccw: bool| {
            with_offscreen_canvas(canvas_id, |c| {
                c.arc(cx as f32, cy as f32, r as f32, sa as f32, ea as f32, ccw)
            });
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_fill",
        into_v8_fn1(|canvas_id: u32| {
            with_offscreen_canvas(canvas_id, |c| c.fill());
            mark_offscreen_dirty(canvas_id);
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_stroke",
        into_v8_fn1(|canvas_id: u32| {
            with_offscreen_canvas(canvas_id, |c| c.stroke());
            mark_offscreen_dirty(canvas_id);
        }),
    )?;

    rt.register_native(
        "_lumen_offscreen_canvas2d_set_fill_style",
        into_v8_fn2(|canvas_id: u32, css: String| {
            use lumen_canvas::{CanvasColor, PaintSource};
            with_offscreen_canvas(canvas_id, |c| {
                if let Some(color) = CanvasColor::from_css_str(&css) {
                    c.fill_style = PaintSource::Color(color);
                }
            });
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_set_stroke_style",
        into_v8_fn2(|canvas_id: u32, css: String| {
            use lumen_canvas::{CanvasColor, PaintSource};
            with_offscreen_canvas(canvas_id, |c| {
                if let Some(color) = CanvasColor::from_css_str(&css) {
                    c.stroke_style = PaintSource::Color(color);
                }
            });
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_set_line_width",
        into_v8_fn2(|canvas_id: u32, w: f64| {
            if w.is_finite() && w > 0.0 {
                with_offscreen_canvas(canvas_id, |c| c.line_width = w as f32);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_set_global_alpha",
        into_v8_fn2(|canvas_id: u32, a: f64| {
            if a.is_finite() && (0.0..=1.0).contains(&a) {
                with_offscreen_canvas(canvas_id, |c| c.global_alpha = a as f32);
            }
        }),
    )?;

    rt.register_native(
        "_lumen_offscreen_canvas2d_get_image_data",
        into_v8_fn1(|canvas_id: u32| -> String {
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

    rt.register_native(
        "_lumen_offscreen_canvas_transfer_to_image_bitmap",
        into_v8_fn1(transfer_to_image_bitmap_native),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas_bitmap_close",
        into_v8_fn1(close_bitmap_native),
    )?;
    rt.register_native(
        "_lumen_decode_image_to_canvas",
        into_v8_fn1(decode_image_to_canvas_native),
    )?;
    rt.register_native(
        "_lumen_image_bitmap_from_img_nid",
        into_v8_fn1(image_bitmap_from_img_nid_native),
    )?;

    rt.register_native(
        "_lumen_offscreen_canvas_from_image_data",
        into_v8_fn3(|w: u32, h: u32, hex: String| -> u32 {
            let w = w.clamp(1, MAX_CANVAS_DIM);
            let h = h.clamp(1, MAX_CANVAS_DIM);
            let expected = (w * h * 4) as usize;
            let bytes: Vec<u8> = hex
                .as_bytes()
                .chunks(2)
                .filter_map(|pair| {
                    let s = std::str::from_utf8(pair).ok()?;
                    u8::from_str_radix(s, 16).ok()
                })
                .collect();
            if bytes.len() != expected {
                return 0;
            }
            create_offscreen_from_pixels(w, h, bytes)
        }),
    )?;

    rt.eval(OFFSCREEN_CANVAS_SHIM)?;

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
      // Neuters this OffscreenCanvas's backing store and re-homes its pixels
      // under a fresh bitmap ID (HTML LS §4.12.14). Unified shape: {__canvas_id__}.
      var cid = _lumen_offscreen_canvas_transfer_to_image_bitmap(this.__canvas_id__);
      if (!cid) {
        throw new Error('transferToImageBitmap: canvas already transferred or invalid');
      }
      var width = this.width, height = this.height;
      return {
        width: width,
        height: height,
        __canvas_id__: cid,
        close: function() { _lumen_offscreen_canvas_bitmap_close(cid); }
      };
    }

    convertToBlob(options) {
      // TODO: Implement PNG/JPEG encoding
      return Promise.reject(new Error('convertToBlob not yet implemented'));
    }
  };

  // Parses the "{w},{h},{hex_rgba}" wire format shared by
  // `_lumen_offscreen_canvas2d_get_image_data`/`_lumen_canvas2d_get_image_data`.
  function _parseWHHex(raw) {
    var c1 = raw.indexOf(','), c2 = raw.indexOf(',', c1 + 1);
    return { w: parseInt(raw.substring(0, c1), 10), h: parseInt(raw.substring(c1 + 1, c2), 10), hex: raw.substring(c2 + 1) };
  }

  // createImageBitmap(source[, sx, sy, sw, sh])
  // Supports: ImageData, OffscreenCanvas, Blob, HTMLImageElement (HTML LS §4.12.5.4).
  // All sources resolve to the same bitmap shape: {width, height, __canvas_id__, close()}.
  if (!globalThis.createImageBitmap) {
    globalThis.createImageBitmap = function(source, sx, sy, sw, sh) {
      return new Promise(function(resolve, reject) {
        if (!source) {
          reject(new TypeError('createImageBitmap: source is null'));
          return;
        }
        var cropGiven = arguments.length >= 5 || typeof sx === 'number';

        // Crops (or passes through) a bitmap already stored at `cid` with
        // known dimensions `w`×`h`, then resolves the final ImageBitmap.
        function finish(cid, w, h) {
          if (!cid) {
            reject(new Error('createImageBitmap: failed to create bitmap'));
            return;
          }
          if (!cropGiven) {
            resolve({ width: w, height: h, __canvas_id__: cid, close: function() { _lumen_offscreen_canvas_bitmap_close(cid); } });
            return;
          }
          var csx = Math.max(0, Math.min(w, sx | 0));
          var csy = Math.max(0, Math.min(h, sy | 0));
          var csw = Math.max(1, Math.min(w - csx, (sw | 0) || 1));
          var csh = Math.max(1, Math.min(h - csy, (sh | 0) || 1));
          var raw = _lumen_offscreen_canvas2d_get_image_data(cid);
          _lumen_offscreen_canvas_bitmap_close(cid);
          var parsed = _parseWHHex(raw);
          var croppedHex = '';
          for (var row = 0; row < csh; row++) {
            var rowStart = ((csy + row) * parsed.w + csx) * 8;
            croppedHex += parsed.hex.substr(rowStart, csw * 8);
          }
          var newCid = _lumen_offscreen_canvas_from_image_data(csw, csh, croppedHex);
          if (!newCid) {
            reject(new Error('createImageBitmap: crop failed'));
            return;
          }
          resolve({ width: csw, height: csh, __canvas_id__: newCid, close: function() { _lumen_offscreen_canvas_bitmap_close(newCid); } });
        }

        // ImageData: has .data (Uint8ClampedArray), .width, .height
        if (source.data && typeof source.width === 'number' && typeof source.height === 'number') {
          var w = source.width >>> 0;
          var h = source.height >>> 0;
          if (w === 0 || h === 0) {
            reject(new Error('createImageBitmap: ImageData has zero dimensions'));
            return;
          }
          var data = source.data;
          // Encode RGBA bytes as lowercase hex string for native binding
          var hex = '';
          for (var i = 0; i < data.length; i++) {
            var b = data[i] & 0xff;
            hex += (b < 16 ? '0' : '') + b.toString(16);
          }
          var cid = _lumen_offscreen_canvas_from_image_data(w, h, hex);
          if (cid === 0) {
            reject(new Error('createImageBitmap: pixel data size mismatch'));
            return;
          }
          finish(cid, w, h);
          return;
        }

        // OffscreenCanvas: snapshot its current pixels without detaching the source
        // (unlike transferToImageBitmap(), createImageBitmap() must leave it usable).
        if (typeof source.__canvas_id__ === 'number') {
          var srcRaw = _lumen_offscreen_canvas2d_get_image_data(source.__canvas_id__);
          if (!srcRaw) {
            reject(new Error('createImageBitmap: OffscreenCanvas is empty or already transferred'));
            return;
          }
          var srcParsed = _parseWHHex(srcRaw);
          var snapCid = _lumen_offscreen_canvas_from_image_data(srcParsed.w, srcParsed.h, srcParsed.hex);
          finish(snapCid, srcParsed.w, srcParsed.h);
          return;
        }

        // HTMLImageElement: pixels come from img_bitmap_store, keyed by DOM node id.
        if (source.__nid__ !== undefined && typeof _lumen_get_tag_name === 'function' && _lumen_get_tag_name(source.__nid__) === 'IMG') {
          var icid = _lumen_image_bitmap_from_img_nid(source.__nid__);
          if (!icid) {
            reject(new Error('createImageBitmap from HTMLImageElement: image not yet decoded'));
            return;
          }
          finish(icid, +source.width || 0, +source.height || 0);
          return;
        }

        // Blob: decode via lumen_image::decode (PNG/JPEG/WebP/GIF/AVIF).
        if (source._bytes instanceof Uint8Array) {
          var bcid = _lumen_decode_image_to_canvas(Array.from(source._bytes));
          if (!bcid) {
            reject(new Error('createImageBitmap: unable to decode Blob image data'));
            return;
          }
          var bd = _parseWHHex(_lumen_offscreen_canvas2d_get_image_data(bcid));
          finish(bcid, bd.w, bd.h);
          return;
        }

        reject(new TypeError('createImageBitmap: unsupported source type'));
      });
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
                        typeof bitmap.__canvas_id__ === 'number' &&
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

    // ── Phase 1: createImageBitmap + Worker availability tests ────────────────

    #[test]
    fn native_from_image_data_valid_2x2() {
        // 2×2 RGBA pixels, all transparent black
        let hex = "0".repeat(32); // 16 bytes = 32 hex chars
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_offscreen_canvas_bindings(&ctx).unwrap();

            let result: u32 = ctx
                .eval(format!("_lumen_offscreen_canvas_from_image_data(2, 2, '{hex}')"))
                .unwrap();
            assert!(result > 0, "expected non-zero canvas_id from valid 2x2 image data");
        });
    }

    #[test]
    fn native_from_image_data_size_mismatch_returns_zero() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_offscreen_canvas_bindings(&ctx).unwrap();

            // 3×3 requested but only 4 bytes provided → mismatch
            let result: u32 = ctx
                .eval("_lumen_offscreen_canvas_from_image_data(3, 3, 'aabbccdd')")
                .unwrap();
            assert_eq!(result, 0, "size mismatch should return 0");
        });
    }

    #[test]
    fn native_from_image_data_red_pixel_stored() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_offscreen_canvas_bindings(&ctx).unwrap();

            // 1×1 canvas with red pixel (ff0000ff)
            let result: u32 = ctx
                .eval("_lumen_offscreen_canvas_from_image_data(1, 1, 'ff0000ff')")
                .unwrap();
            assert!(result > 0);
            // Verify pixel is stored
            OFFSCREEN_CANVASES.with(|c| {
                if let Ok(map) = c.try_borrow() {
                    let ctx2d = map.get(&result).expect("canvas should be registered");
                    let pixels = ctx2d.pixels();
                    assert_eq!(pixels[0], 0xff, "R=255");
                    assert_eq!(pixels[1], 0x00, "G=0");
                    assert_eq!(pixels[2], 0x00, "B=0");
                    assert_eq!(pixels[3], 0xff, "A=255");
                }
            });
        });
    }

    #[test]
    fn js_create_image_bitmap_is_function() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_offscreen_canvas_bindings(&ctx).unwrap();

            let result: bool = ctx
                .eval("typeof createImageBitmap === 'function'")
                .unwrap();
            assert!(result, "createImageBitmap should be a function");
        });
    }

    #[test]
    fn js_create_image_bitmap_from_image_data_sync_via_native() {
        // Test the native binding that createImageBitmap(ImageData) uses internally.
        // We bypass the Promise wrapper and test the hex-encode → canvas ID path.
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_offscreen_canvas_bindings(&ctx).unwrap();

            // Build a 2×2 RGBA hex string from a typed array (same code path as createImageBitmap)
            let result: bool = ctx.eval(r#"
                var data = new Uint8Array([
                  100, 150, 200, 255,
                  50,  80,  120, 200,
                  10,  20,  30,  100,
                  0,   0,   0,   0
                ]);
                var hex = '';
                for (var i = 0; i < data.length; i++) {
                  var b = data[i] & 0xff;
                  hex += (b < 16 ? '0' : '') + b.toString(16);
                }
                var cid = _lumen_offscreen_canvas_from_image_data(2, 2, hex);
                cid > 0
            "#).unwrap();
            assert!(result, "createImageBitmap inner binding should produce a canvas");
        });
    }

    #[test]
    fn native_transfer_to_image_bitmap_returns_new_canvas_id() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_offscreen_canvas_bindings(&ctx).unwrap();

            let result: bool = ctx.eval(r#"
                var canvas = new OffscreenCanvas(4, 4);
                var ctx2d = canvas.getContext('2d');
                ctx2d.fillStyle = '#00ff00';
                ctx2d.fillRect(0, 0, 4, 4);
                var newId = _lumen_offscreen_canvas_transfer_to_image_bitmap(canvas.__canvas_id__);
                newId > 0 && newId !== canvas.__canvas_id__
            "#).unwrap();
            assert!(result);
        });
    }

    #[test]
    fn js_create_image_bitmap_from_offscreen_canvas_does_not_detach_source() {
        // createImageBitmap(OffscreenCanvas) must snapshot pixels without
        // neutering the source (unlike transferToImageBitmap()). Exercises the
        // same read-snapshot-recreate native sequence the JS shim uses,
        // bypassing the Promise wrapper (same rationale as the ImageData test
        // above: no microtask pump in this bare `ctx.eval` harness).
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_offscreen_canvas_bindings(&ctx).unwrap();

            let result: bool = ctx.eval(r#"
                var canvas = new OffscreenCanvas(4, 4);
                var ctx2d = canvas.getContext('2d');
                ctx2d.fillStyle = '#00ff00';
                ctx2d.fillRect(0, 0, 4, 4);
                var raw = _lumen_offscreen_canvas2d_get_image_data(canvas.__canvas_id__);
                var parts = raw.split(',');
                var snapCid = _lumen_offscreen_canvas_from_image_data(parseInt(parts[0], 10), parseInt(parts[1], 10), parts[2]);
                var bitmapCreated = snapCid > 0 && snapCid !== canvas.__canvas_id__;
                var srcStillReadable = _lumen_offscreen_canvas2d_get_image_data(canvas.__canvas_id__).length > 0;
                bitmapCreated && srcStillReadable
            "#).unwrap();
            assert!(result);
        });
    }

    #[test]
    fn create_offscreen_from_pixels_correct_id() {
        reset_state();
        // 1×1 opaque blue pixel
        let pixels = vec![0u8, 0, 255, 255];
        let id = super::create_offscreen_from_pixels(1, 1, pixels.clone());
        assert!(id > 0);
        OFFSCREEN_CANVASES.with(|c| {
            if let Ok(map) = c.try_borrow() {
                let ctx2d = map.get(&id).expect("canvas should be registered");
                assert_eq!(ctx2d.width(), 1);
                assert_eq!(ctx2d.height(), 1);
                // Pixel data should match what we passed in
                let stored = ctx2d.pixels();
                assert_eq!(stored[0], 0,   "R=0");
                assert_eq!(stored[1], 0,   "G=0");
                assert_eq!(stored[2], 255, "B=255");
                assert_eq!(stored[3], 255, "A=255");
            }
        });
    }

    #[test]
    fn create_offscreen_from_pixels_unique_ids() {
        reset_state();
        let id1 = super::create_offscreen_from_pixels(2, 2, vec![0u8; 16]);
        let id2 = super::create_offscreen_from_pixels(2, 2, vec![0u8; 16]);
        assert_ne!(id1, id2, "each transfer should yield a distinct canvas ID");
    }

    #[test]
    fn js_offscreen_canvas_available_in_fresh_context() {
        // Simulates worker thread: fresh JS context with OffscreenCanvas installed.
        let (_rt2, ctx2) = make_ctx();
        ctx2.with(|ctx| {
            // No reset_state — fresh thread-local for this context
            install_offscreen_canvas_bindings(&ctx).unwrap();

            let result: bool = ctx
                .eval("typeof OffscreenCanvas === 'function' && typeof createImageBitmap === 'function'")
                .unwrap();
            assert!(result, "OffscreenCanvas and createImageBitmap must be available in fresh (worker) context");
        });
    }
}
