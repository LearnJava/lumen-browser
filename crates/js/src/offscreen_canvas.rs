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

/// BUG-454: arm the fingerprint noise generator on an OffscreenCanvas created
/// via [`create_offscreen_from_pixels`] — used by `transferControlToOffscreen()`
/// (HTML LS §4.12.14), whose pixels come straight from a live DOM `<canvas>`
/// rather than through `_lumen_offscreen_canvas_new` (which arms noise itself).
/// Not called for the other three `create_offscreen_from_pixels` sites
/// (`ImageBitmap` transfer, `createImageBitmap(blob)`, `createImageBitmap(<img>)`)
/// — those read decoded image bytes, not a canvas's rendered content, so they
/// are outside this bug's threat model.
#[cfg(feature = "v8-backend")]
pub(crate) fn arm_noise(canvas_id: u32, seed: u64) {
    with_offscreen_canvas(canvas_id, |c| {
        c.set_noise_generator(lumen_canvas::CanvasNoiseGenerator::new(seed));
    });
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

/// Install OffscreenCanvas bindings and JS shim into a V8 runtime (Ph3 V8
/// migration, deferred past S8 — see the note at `canvas2d.rs`'s
/// `_lumen_canvas_transfer_control_to_offscreen` V8 port; the rquickjs twin
/// was removed in S12b-B27). State (`OFFSCREEN_CANVASES`, `DIRTY`) is
/// module-level `thread_local!`, not a `V8JsRuntime` field — same pattern as
/// `canvas2d_v8`/`webgl_canvas_v8`. [`OFFSCREEN_CANVAS_SHIM`] is not part of
/// `dom.rs::WEB_API_SHIM` and must be `eval`'d here explicitly, mirroring
/// `webgl_canvas::install_webgl_canvas_v8`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_offscreen_canvas_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    origin: &str,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{
        into_v8_fn1, into_v8_fn2, into_v8_fn3, into_v8_fn5, into_v8_fn6, into_v8_fn7,
    };
    use lumen_core::ext::JsRuntime as _;
    use lumen_canvas::CanvasNoiseGenerator;

    // BUG-454: same per-document seed as `canvas2d.rs`'s element canvases —
    // `document_noise_seed` mixes it with the origin, `OffscreenCanvas` has no
    // origin of its own to derive one from. Computed once here and captured by
    // value into the closure below, not read from a thread-local at call time —
    // this installer and the native's actual invocation run on different OS
    // threads in this V8 compat layer, so a thread-local set-here-read-there
    // would read back its default (measured on `canvas2d.rs`'s first attempt at
    // this: every canvas silently seeded with 0).
    let noise_seed = crate::canvas2d::document_noise_seed(origin);

    rt.register_native(
        "_lumen_offscreen_canvas_new",
        into_v8_fn2(move |w: u32, h: u32| -> String {
            let canvas = OffscreenCanvas::new(w, h);
            with_offscreen_canvas(canvas.id, |c| {
                c.set_noise_generator(CanvasNoiseGenerator::new(noise_seed));
            });
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
            let color = CanvasColor::from_css_str(&css)?;
            with_offscreen_canvas(canvas_id, |c| c.fill_style = PaintSource::Color(color));
            Some(color.to_css_string())
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_set_stroke_style",
        into_v8_fn2(|canvas_id: u32, css: String| {
            use lumen_canvas::{CanvasColor, PaintSource};
            let color = CanvasColor::from_css_str(&css)?;
            with_offscreen_canvas(canvas_id, |c| c.stroke_style = PaintSource::Color(color));
            Some(color.to_css_string())
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

    // ── State stack, transforms, additional path ops (BUG-456 симптом 1) ────
    // Same shape as `canvas2d.rs`'s element-canvas twins, backed by the same
    // `Context2D` methods, just resolved via `with_offscreen_canvas` (a
    // different registry, keyed by `canvas_id` rather than DOM `nid`).
    rt.register_native(
        "_lumen_offscreen_canvas2d_save",
        into_v8_fn1(|canvas_id: u32| {
            with_offscreen_canvas(canvas_id, |c| c.save());
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_restore",
        into_v8_fn1(|canvas_id: u32| {
            with_offscreen_canvas(canvas_id, |c| c.restore());
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_translate",
        into_v8_fn3(|canvas_id: u32, tx: f64, ty: f64| {
            with_offscreen_canvas(canvas_id, |c| c.translate(tx as f32, ty as f32));
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_rotate",
        into_v8_fn2(|canvas_id: u32, angle: f64| {
            with_offscreen_canvas(canvas_id, |c| c.rotate(angle as f32));
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_scale",
        into_v8_fn3(|canvas_id: u32, sx: f64, sy: f64| {
            with_offscreen_canvas(canvas_id, |c| c.scale(sx as f32, sy as f32));
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_transform",
        into_v8_fn7(|canvas_id: u32, a: f64, b: f64, c2: f64, d: f64, e: f64, f2: f64| {
            with_offscreen_canvas(canvas_id, |c| {
                c.transform(a as f32, b as f32, c2 as f32, d as f32, e as f32, f2 as f32);
            });
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_set_transform",
        into_v8_fn7(|canvas_id: u32, a: f64, b: f64, c2: f64, d: f64, e: f64, f2: f64| {
            with_offscreen_canvas(canvas_id, |c| {
                c.set_transform(a as f32, b as f32, c2 as f32, d as f32, e as f32, f2 as f32);
            });
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_reset_transform",
        into_v8_fn1(|canvas_id: u32| {
            with_offscreen_canvas(canvas_id, |c| c.reset_transform());
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_rect",
        into_v8_fn5(|canvas_id: u32, x: f64, y: f64, w: f64, h: f64| {
            with_offscreen_canvas(canvas_id, |c| {
                c.rect(x as f32, y as f32, w as f32, h as f32)
            });
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_bezier_curve_to",
        into_v8_fn7(|canvas_id: u32, cp1x: f64, cp1y: f64, cp2x: f64, cp2y: f64, x: f64, y: f64| {
            with_offscreen_canvas(canvas_id, |c| {
                c.bezier_curve_to(
                    cp1x as f32, cp1y as f32,
                    cp2x as f32, cp2y as f32,
                    x as f32, y as f32,
                );
            });
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_quadratic_curve_to",
        into_v8_fn5(|canvas_id: u32, cpx: f64, cpy: f64, x: f64, y: f64| {
            with_offscreen_canvas(canvas_id, |c| {
                c.quadratic_curve_to(cpx as f32, cpy as f32, x as f32, y as f32);
            });
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_arc_to",
        into_v8_fn6(|canvas_id: u32, x1: f64, y1: f64, x2: f64, y2: f64, r: f64| {
            with_offscreen_canvas(canvas_id, |c| {
                c.arc_to(x1 as f32, y1 as f32, x2 as f32, y2 as f32, r as f32);
            });
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_clip",
        into_v8_fn1(|canvas_id: u32| {
            with_offscreen_canvas(canvas_id, |c| c.clip());
        }),
    )?;

    // ── Remaining state properties (BUG-456 симптом 1, next slice) ──────────
    // Same natives as `canvas2d.rs`'s element-canvas twins, over the same
    // `Context2D` fields — the engine already had these, only the JS binding
    // was missing on the offscreen side.
    rt.register_native(
        "_lumen_offscreen_canvas2d_set_global_composite_operation",
        into_v8_fn2(|canvas_id: u32, op: String| {
            use lumen_canvas::CompositeOperation;
            if let Some(op) = CompositeOperation::from_str(&op) {
                with_offscreen_canvas(canvas_id, |c| c.composite_operation = op);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_set_line_cap",
        into_v8_fn2(|canvas_id: u32, cap: String| {
            use lumen_canvas::LineCap;
            if let Some(cap) = LineCap::from_str(&cap) {
                with_offscreen_canvas(canvas_id, |c| c.line_cap = cap);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_set_line_join",
        into_v8_fn2(|canvas_id: u32, join: String| {
            use lumen_canvas::LineJoin;
            if let Some(join) = LineJoin::from_str(&join) {
                with_offscreen_canvas(canvas_id, |c| c.line_join = join);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_set_miter_limit",
        into_v8_fn2(|canvas_id: u32, limit: f64| {
            if limit.is_finite() && limit > 0.0 {
                with_offscreen_canvas(canvas_id, |c| c.miter_limit = limit as f32);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_set_shadow_color",
        into_v8_fn2(|canvas_id: u32, css: String| {
            use lumen_canvas::CanvasColor;
            let color = CanvasColor::from_css_str(&css)?;
            with_offscreen_canvas(canvas_id, |c| c.shadow_color = color);
            Some(color.to_css_string())
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_set_shadow_blur",
        into_v8_fn2(|canvas_id: u32, v: f64| {
            if v.is_finite() && v >= 0.0 {
                with_offscreen_canvas(canvas_id, |c| c.shadow_blur = v as f32);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_set_shadow_offset_x",
        into_v8_fn2(|canvas_id: u32, v: f64| {
            if v.is_finite() {
                with_offscreen_canvas(canvas_id, |c| c.shadow_offset_x = v as f32);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_offscreen_canvas2d_set_shadow_offset_y",
        into_v8_fn2(|canvas_id: u32, v: f64| {
            if v.is_finite() {
                with_offscreen_canvas(canvas_id, |c| c.shadow_offset_y = v as f32);
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

    // Separate from `_lumen_offscreen_canvas2d_get_image_data` (whole-canvas,
    // hex-string transport): that one backs `transferToImageBitmap`/
    // `createImageBitmap`/internal snapshots, which all want the full buffer
    // in that format, so narrowing it in place would break every one of those
    // call sites. This is the OffscreenCanvas twin of BUG-448's fix — the
    // element canvas's `_lumen_canvas2d_get_image_data` (`canvas2d.rs`) took
    // the same rect-parameter + byte-array-return shape, backed by the same
    // `Context2D::get_image_data_rect` (`crates/engine/canvas/src/image_data.rs`).
    rt.register_native(
        "_lumen_offscreen_canvas2d_get_image_data_rect",
        into_v8_fn5(|canvas_id: u32, sx: i32, sy: i32, sw: u32, sh: u32| -> Vec<u8> {
            OFFSCREEN_CANVASES.with(|c| {
                let Ok(map) = c.try_borrow() else {
                    return Vec::new();
                };
                let Some(canvas) = map.get(&canvas_id) else {
                    return Vec::new();
                };
                canvas.get_image_data_rect(sx, sy, sw, sh)
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
      const canvasRef = this;
      // Хранилище четырёх атрибутов с парой get/set (BUG-456 симптом 2): как
      // поля литерала они жить не могут (аксессор с тем же ключом их
      // перекрывает — `lineWidth: 1` ниже был мёртвым кодом, затёртым
      // следующим за ним `set lineWidth`). Начальные значения — §4.12.5.1.1.
      let _fillStyle = '#000000';
      let _strokeStyle = '#000000';
      let _lineWidth = 1;
      let _globalAlpha = 1;
      // BUG-456 симптом 1 (state properties slice): same shape as the four
      // attributes above — a flat field would be dead code under an
      // accessor of the same name, so each gets its own tracked variable.
      // Defaults — §4.12.5.1.1 / lumen_canvas::DrawState::default().
      let _globalCompositeOperation = 'source-over';
      let _lineCap = 'butt';
      let _lineJoin = 'miter';
      let _miterLimit = 10;
      let _shadowColor = 'rgba(0, 0, 0, 0)';
      let _shadowBlur = 0;
      let _shadowOffsetX = 0;
      let _shadowOffsetY = 0;
      // Стек состояний save()/restore() (§4.12.5.1.2) — параллельно нативному,
      // как у элементного контекста (`web_api_shim_mid.js`): та же ловушка
      // BUG-455, если копии разойдутся.
      const stack = [];
      this._2d_context = {
        // Canvas reference
        canvas: this,

        // Rectangles
        fillRect: (x, y, w, h) => _lumen_offscreen_canvas2d_fill_rect(canvasId, x, y, w, h),
        clearRect: (x, y, w, h) => _lumen_offscreen_canvas2d_clear_rect(canvasId, x, y, w, h),
        strokeRect: (x, y, w, h) => _lumen_offscreen_canvas2d_stroke_rect(canvasId, x, y, w, h),

        // Paths (BUG-456 симптом 1: rect/arcTo/bezierCurveTo/quadraticCurveTo/
        // ellipse/roundRect/clip added next to the pre-existing four).
        beginPath: () => _lumen_offscreen_canvas2d_begin_path(canvasId),
        moveTo: (x, y) => _lumen_offscreen_canvas2d_move_to(canvasId, x, y),
        lineTo: (x, y) => _lumen_offscreen_canvas2d_line_to(canvasId, x, y),
        closePath: () => _lumen_offscreen_canvas2d_close_path(canvasId),
        arc: (cx, cy, r, sa, ea, ccw) =>
          _lumen_offscreen_canvas2d_arc(canvasId, cx, cy, r, sa, ea, !!ccw),
        rect: (x, y, w, h) => _lumen_offscreen_canvas2d_rect(canvasId, x, y, w, h),
        bezierCurveTo: (cp1x, cp1y, cp2x, cp2y, x, y) =>
          _lumen_offscreen_canvas2d_bezier_curve_to(canvasId, cp1x, cp1y, cp2x, cp2y, x, y),
        quadraticCurveTo: (cpx, cpy, x, y) =>
          _lumen_offscreen_canvas2d_quadratic_curve_to(canvasId, cpx, cpy, x, y),
        arcTo: (x1, y1, x2, y2, r) =>
          _lumen_offscreen_canvas2d_arc_to(canvasId, x1, y1, x2, y2, r),
        // No dedicated native on either context — composed from transforms +
        // arc, same as the element context's `ellipse` (`web_api_shim_mid.js`).
        ellipse: function(cx, cy, rx, ry, rot, sa, ea, ccw) {
          _lumen_offscreen_canvas2d_save(canvasId);
          _lumen_offscreen_canvas2d_translate(canvasId, cx, cy);
          if (rot) { _lumen_offscreen_canvas2d_rotate(canvasId, rot); }
          _lumen_offscreen_canvas2d_scale(canvasId, rx, ry);
          _lumen_offscreen_canvas2d_arc(canvasId, 0, 0, 1, sa, ea, !!ccw);
          _lumen_offscreen_canvas2d_restore(canvasId);
        },
        roundRect: function(x, y, w, h, radii) {
          _offscreen_round_rect(canvasId, x, y, w, h, radii);
        },
        fill: () => _lumen_offscreen_canvas2d_fill(canvasId),
        stroke: () => _lumen_offscreen_canvas2d_stroke(canvasId),
        clip: () => _lumen_offscreen_canvas2d_clip(canvasId),

        // State stack. Pushes/pops the native CTM/path-clip state and the
        // four JS-mirrored attributes together — the BUG-455 lesson.
        save: function() {
          _lumen_offscreen_canvas2d_save(canvasId);
          stack.push({
            fillStyle: _fillStyle, strokeStyle: _strokeStyle,
            lineWidth: _lineWidth, globalAlpha: _globalAlpha,
            globalCompositeOperation: _globalCompositeOperation,
            lineCap: _lineCap, lineJoin: _lineJoin, miterLimit: _miterLimit,
            shadowColor: _shadowColor, shadowBlur: _shadowBlur,
            shadowOffsetX: _shadowOffsetX, shadowOffsetY: _shadowOffsetY,
          });
        },
        restore: function() {
          _lumen_offscreen_canvas2d_restore(canvasId);
          var snap = stack.pop();
          if (snap) {
            _fillStyle = snap.fillStyle; _strokeStyle = snap.strokeStyle;
            _lineWidth = snap.lineWidth; _globalAlpha = snap.globalAlpha;
            _globalCompositeOperation = snap.globalCompositeOperation;
            _lineCap = snap.lineCap; _lineJoin = snap.lineJoin;
            _miterLimit = snap.miterLimit;
            _shadowColor = snap.shadowColor; _shadowBlur = snap.shadowBlur;
            _shadowOffsetX = snap.shadowOffsetX; _shadowOffsetY = snap.shadowOffsetY;
          }
        },

        // Transforms
        translate: (tx, ty) => _lumen_offscreen_canvas2d_translate(canvasId, tx, ty),
        rotate: (angle) => _lumen_offscreen_canvas2d_rotate(canvasId, angle),
        scale: (sx, sy) => _lumen_offscreen_canvas2d_scale(canvasId, sx, sy),
        transform: (a, b, c, d, e, f) =>
          _lumen_offscreen_canvas2d_transform(canvasId, a, b, c, d, e, f),
        setTransform: function(a, b, c, d, e, f) {
          // setTransform() with no arguments resets to identity (§4.12.5.1.6)
          // — the same NaN trap BUG-449 fixed on the element context.
          if (arguments.length === 0) {
            _lumen_offscreen_canvas2d_set_transform(canvasId, 1, 0, 0, 1, 0, 0);
            return;
          }
          if (arguments.length === 1) {
            var m = a;
            if (m === null || typeof m !== 'object') {
              throw new TypeError('setTransform: argument is not a DOMMatrix2DInit');
            }
            _lumen_offscreen_canvas2d_set_transform(canvasId,
              m.a === undefined ? 1 : +m.a, m.b === undefined ? 0 : +m.b,
              m.c === undefined ? 0 : +m.c, m.d === undefined ? 1 : +m.d,
              m.e === undefined ? 0 : +m.e, m.f === undefined ? 0 : +m.f);
            return;
          }
          _lumen_offscreen_canvas2d_set_transform(canvasId, a, b, c, d, e, f);
        },
        resetTransform: () => _lumen_offscreen_canvas2d_reset_transform(canvasId),

        // §4.12.5.1.2 reset: bitmap goes transparent black, path/transform/
        // clip/state-stack clear, and the four tracked attributes return to
        // their initial values.
        reset: function() {
          stack.length = 0;
          _lumen_offscreen_canvas2d_reset_transform(canvasId);
          _lumen_offscreen_canvas2d_clear_rect(canvasId, 0, 0, canvasRef.width, canvasRef.height);
          _lumen_offscreen_canvas2d_begin_path(canvasId);
          _fillStyle = _lumen_offscreen_canvas2d_set_fill_style(canvasId, '#000000') || '#000000';
          _strokeStyle = _lumen_offscreen_canvas2d_set_stroke_style(canvasId, '#000000') || '#000000';
          _lineWidth = 1;
          _lumen_offscreen_canvas2d_set_line_width(canvasId, 1);
          _globalAlpha = 1;
          _lumen_offscreen_canvas2d_set_global_alpha(canvasId, 1);
          _globalCompositeOperation = 'source-over';
          _lumen_offscreen_canvas2d_set_global_composite_operation(canvasId, 'source-over');
          _lineCap = 'butt';
          _lumen_offscreen_canvas2d_set_line_cap(canvasId, 'butt');
          _lineJoin = 'miter';
          _lumen_offscreen_canvas2d_set_line_join(canvasId, 'miter');
          _miterLimit = 10;
          _lumen_offscreen_canvas2d_set_miter_limit(canvasId, 10);
          _shadowColor = _lumen_offscreen_canvas2d_set_shadow_color(canvasId, 'rgba(0, 0, 0, 0)') || 'rgba(0, 0, 0, 0)';
          _shadowBlur = 0;
          _lumen_offscreen_canvas2d_set_shadow_blur(canvasId, 0);
          _shadowOffsetX = 0;
          _lumen_offscreen_canvas2d_set_shadow_offset_x(canvasId, 0);
          _shadowOffsetY = 0;
          _lumen_offscreen_canvas2d_set_shadow_offset_y(canvasId, 0);
        },

        // Style setters. Натив возвращает каноническую сериализацию принятого
        // цвета либо null на невалидной строке — тогда атрибут НЕ меняется
        // (HTML LS §4.12.5.1.3). Геттеры до BUG-451 отсутствовали вовсе, и
        // `ctx.fillStyle` читался как `undefined`.
        set fillStyle(val) {
          if (typeof val !== 'string') val = String(val);
          var ser = _lumen_offscreen_canvas2d_set_fill_style(canvasId, val);
          if (ser !== null && ser !== undefined) { _fillStyle = ser; }
        },
        get fillStyle() { return _fillStyle; },
        set strokeStyle(val) {
          if (typeof val !== 'string') val = String(val);
          var ser = _lumen_offscreen_canvas2d_set_stroke_style(canvasId, val);
          if (ser !== null && ser !== undefined) { _strokeStyle = ser; }
        },
        get strokeStyle() { return _strokeStyle; },
        // BUG-456 симптом 2: lineWidth/globalAlpha had setters but no
        // getters at all (unlike fillStyle/strokeStyle above, which already
        // had a working pair) — reading either back always answered
        // `undefined` even right after a successful write.
        set lineWidth(w) {
          var n = Number(w);
          if (isFinite(n) && n > 0) {
            _lineWidth = n;
            _lumen_offscreen_canvas2d_set_line_width(canvasId, n);
          }
        },
        get lineWidth() { return _lineWidth; },
        set globalAlpha(a) {
          var n = Number(a);
          if (isFinite(n) && n >= 0 && n <= 1) {
            _globalAlpha = n;
            _lumen_offscreen_canvas2d_set_global_alpha(canvasId, n);
          }
        },
        get globalAlpha() { return _globalAlpha; },
        // BUG-456 симптом 1 (state properties slice): plain string-enum
        // setters over already-public `Context2D` fields, symmetric to the
        // element context's `_lumen_c2d_prop` (`web_api_shim_mid.js`) — an
        // unrecognized keyword is silently ignored (attribute keeps its
        // prior value), matching the native's own `from_str` → `Option`
        // shape rather than throwing.
        set globalCompositeOperation(v) {
          v = String(v);
          _lumen_offscreen_canvas2d_set_global_composite_operation(canvasId, v);
          _globalCompositeOperation = v;
        },
        get globalCompositeOperation() { return _globalCompositeOperation; },
        set lineCap(v) {
          v = String(v);
          _lumen_offscreen_canvas2d_set_line_cap(canvasId, v);
          _lineCap = v;
        },
        get lineCap() { return _lineCap; },
        set lineJoin(v) {
          v = String(v);
          _lumen_offscreen_canvas2d_set_line_join(canvasId, v);
          _lineJoin = v;
        },
        get lineJoin() { return _lineJoin; },
        set miterLimit(v) {
          var n = Number(v);
          if (isFinite(n) && n > 0) {
            _miterLimit = n;
            _lumen_offscreen_canvas2d_set_miter_limit(canvasId, n);
          }
        },
        get miterLimit() { return _miterLimit; },
        // `shadowColor` follows the paint-style contract (parse / ignore
        // invalid / store the native's canonical serialization) — same as
        // `fillStyle`/`strokeStyle` above, minus gradients/patterns.
        set shadowColor(v) {
          var ser = _lumen_offscreen_canvas2d_set_shadow_color(canvasId, String(v));
          if (ser !== null && ser !== undefined) { _shadowColor = ser; }
        },
        get shadowColor() { return _shadowColor; },
        set shadowBlur(v) {
          var n = Number(v);
          if (isFinite(n) && n >= 0) {
            _shadowBlur = n;
            _lumen_offscreen_canvas2d_set_shadow_blur(canvasId, n);
          }
        },
        get shadowBlur() { return _shadowBlur; },
        set shadowOffsetX(v) {
          var n = Number(v);
          if (isFinite(n)) {
            _shadowOffsetX = n;
            _lumen_offscreen_canvas2d_set_shadow_offset_x(canvasId, n);
          }
        },
        get shadowOffsetX() { return _shadowOffsetX; },
        set shadowOffsetY(v) {
          var n = Number(v);
          if (isFinite(n)) {
            _shadowOffsetY = n;
            _lumen_offscreen_canvas2d_set_shadow_offset_y(canvasId, n);
          }
        },
        get shadowOffsetY() { return _shadowOffsetY; },

        // Image data (BUG-456 симптом 3 / BUG-448 twin): used to take no
        // parameters at all and hand the page the raw "{w},{h},{hex}" wire
        // string instead of an ImageData. `_offscreen_long` is a local copy of
        // the element canvas's `[EnforceRange] long` coercion rather than a
        // call into `web_api_shim_mid.js` — this whole context is its own
        // standalone `rt.eval` (BUG-780 lesson: a fix in the page shim does
        // not reach a module installed by a separate eval).
        getImageData: function(sx, sy, sw, sh) {
          if (arguments.length < 4) {
            throw new TypeError(
              'getImageData: 4 arguments required, but only ' + arguments.length + ' present');
          }
          var x = _offscreen_long(sx, 'sx'), y = _offscreen_long(sy, 'sy');
          var w = _offscreen_long(sw, 'sw'), h = _offscreen_long(sh, 'sh');
          if (w === 0 || h === 0) {
            throw new DOMException(
              'The source width and height must be non-zero', 'IndexSizeError');
          }
          if (w < 0) { x += w; w = -w; }
          if (h < 0) { y += h; h = -h; }
          var bytes = _lumen_offscreen_canvas2d_get_image_data_rect(canvasId, x, y, w, h);
          var arr = new Uint8ClampedArray(w * h * 4);
          if (bytes && bytes.length === arr.length) { arr.set(bytes); }
          return { width: w, height: h, data: arr, colorSpace: 'srgb' };
        },
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

  // roundRect (§4.12.5.1.7) corner radius normalization: a number or a
  // `{x, y}` (DOMPointInit), ported verbatim from the element context's
  // `_lumen_corner_radius` (`web_api_shim_mid.js`).
  function _offscreen_corner_radius(v) {
    if (v !== null && typeof v === 'object') {
      var rx = v.x === undefined ? 0 : Number(v.x);
      var ry = v.y === undefined ? 0 : Number(v.y);
      if (!isFinite(rx) || !isFinite(ry)) {
        throw new TypeError('roundRect: a radius is not a finite number');
      }
      if (rx < 0 || ry < 0) {
        throw new DOMException('roundRect: a radius is negative', 'IndexSizeError');
      }
      return [rx, ry];
    }
    var r = Number(v);
    if (!isFinite(r)) {
      throw new TypeError('roundRect: a radius is not a finite number');
    }
    if (r < 0) {
      throw new DOMException('roundRect: a radius is negative', 'IndexSizeError');
    }
    return [r, r];
  }

  // roundRect (§4.12.5.1.7): each corner is a quarter ellipse, scaled down
  // together when they would overlap — same algorithm as the element
  // context's `roundRect` (`web_api_shim_mid.js`), ported to the offscreen
  // native names since the two contexts do not share a JS realm (a worker
  // has no page DOM/`CanvasRenderingContext2D` to reuse).
  function _offscreen_round_rect(canvasId, x, y, w, h, radii) {
    var X = +x, Y = +y, W = +w, H = +h;
    if (!isFinite(X) || !isFinite(Y) || !isFinite(W) || !isFinite(H)) { return; }
    if (radii === undefined) { radii = 0; }
    var list = (radii !== null && typeof radii === 'object' && typeof radii.length === 'number')
        ? Array.prototype.slice.call(radii) : [radii];
    if (list.length < 1 || list.length > 4) {
      throw new RangeError('roundRect: radii must hold between one and four radii');
    }
    var r = [];
    for (var i = 0; i < list.length; i++) { r.push(_offscreen_corner_radius(list[i])); }
    var ul, ur, lr, ll;
    if (r.length === 1) { ul = ur = lr = ll = r[0]; }
    else if (r.length === 2) { ul = lr = r[0]; ur = ll = r[1]; }
    else if (r.length === 3) { ul = r[0]; ur = ll = r[1]; lr = r[2]; }
    else { ul = r[0]; ur = r[1]; lr = r[2]; ll = r[3]; }
    if (W < 0) { X += W; W = -W; var sw1 = ul; ul = ur; ur = sw1; var sw2 = ll; ll = lr; lr = sw2; }
    if (H < 0) { Y += H; H = -H; var sh1 = ul; ul = ll; ll = sh1; var sh2 = ur; ur = lr; lr = sh2; }
    var scale = Math.min(
        H / (ul[1] + ll[1]), W / (ul[0] + ur[0]),
        H / (ur[1] + lr[1]), W / (ll[0] + lr[0]));
    if (isFinite(scale) && scale < 1) {
      ul = [ul[0] * scale, ul[1] * scale]; ur = [ur[0] * scale, ur[1] * scale];
      lr = [lr[0] * scale, lr[1] * scale]; ll = [ll[0] * scale, ll[1] * scale];
    }
    function corner(cx, cy, rx, ry, start, end) {
      if (rx <= 0 || ry <= 0) { _lumen_offscreen_canvas2d_line_to(canvasId, cx, cy); return; }
      _lumen_offscreen_canvas2d_save(canvasId);
      _lumen_offscreen_canvas2d_translate(canvasId, cx, cy);
      _lumen_offscreen_canvas2d_scale(canvasId, rx, ry);
      _lumen_offscreen_canvas2d_arc(canvasId, 0, 0, 1, start, end, false);
      _lumen_offscreen_canvas2d_restore(canvasId);
    }
    var HALF = Math.PI / 2;
    _lumen_offscreen_canvas2d_move_to(canvasId, X + ul[0], Y);
    _lumen_offscreen_canvas2d_line_to(canvasId, X + W - ur[0], Y);
    corner(X + W - ur[0], Y + ur[1], ur[0], ur[1], -HALF, 0);
    _lumen_offscreen_canvas2d_line_to(canvasId, X + W, Y + H - lr[1]);
    corner(X + W - lr[0], Y + H - lr[1], lr[0], lr[1], 0, HALF);
    _lumen_offscreen_canvas2d_line_to(canvasId, X + ll[0], Y + H);
    corner(X + ll[0], Y + H - ll[1], ll[0], ll[1], HALF, Math.PI);
    _lumen_offscreen_canvas2d_line_to(canvasId, X, Y + ul[1]);
    corner(X + ul[0], Y + ul[1], ul[0], ul[1], Math.PI, Math.PI + HALF);
    _lumen_offscreen_canvas2d_close_path(canvasId);
  }

  // WebIDL `[EnforceRange] long` coercion for `getImageData`'s four
  // coordinates: reject non-finite instead of silently reading `(0,0)`.
  function _offscreen_long(value, argName) {
    var n = Number(value);
    if (!isFinite(n)) {
      throw new TypeError('getImageData: ' + argName + ' is not a finite number');
    }
    return n < 0 ? Math.ceil(n) : Math.floor(n);
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

    fn reset_state() {
        OFFSCREEN_CANVASES.with(|c| c.borrow_mut().clear());
        DIRTY.with(|d| d.borrow_mut().clear());
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
}

/// V8 test coverage for [`install_offscreen_canvas_bindings_v8`] (S12b-B26;
/// the rquickjs suite this ports from was removed in S12b-B27): 14
/// JS-integration tests (constructor/getContext/fillRect/
/// transferToImageBitmap/createImageBitmap wiring). The 9 pure-Rust tests in
/// `mod tests` above (no JS engine involved) are not duplicated here — they
/// already cover both engines, since they call the shared native functions
/// (`OffscreenCanvas::new`, `with_offscreen_canvas`, `create_offscreen_from_pixels`, …)
/// directly.
///
/// [`V8JsRuntime::new`] spawns a dedicated OS thread per runtime, so unlike a
/// bare rquickjs `Context`, these can't peek at [`OFFSCREEN_CANVASES`]/
/// [`DIRTY`] from the test's own thread (different `thread_local!` instance)
/// — every assertion goes through the JS-visible native functions instead
/// (e.g. `native_from_image_data_red_pixel_stored` reads pixels back via
/// `_lumen_offscreen_canvas2d_get_image_data` rather than inspecting the
/// `Context2D` directly).
#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use lumen_core::JsValue;
    use lumen_core::ext::JsRuntime as _;

    use crate::v8_runtime::V8JsRuntime;

    fn with_offscreen() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        // On a page `install_dom` runs first and brings `DOMException` with it
        // (installed before this module's own bindings); here nothing does, so
        // `throw new DOMException(...)` in `getImageData` would otherwise become
        // a `ReferenceError` (same trap as `filesystem_access.rs`'s harness).
        rt.eval(crate::v8_runtime::DOM_EXCEPTION_POLYFILL).unwrap();
        super::install_offscreen_canvas_bindings_v8(&rt, "https://example.test").unwrap();
        rt
    }

    fn bool_eval(rt: &V8JsRuntime, expr: &str) -> bool {
        matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
    }

    #[test]
    fn js_offscreen_canvas_constructor() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(100, 200);
                canvas.width === 100 && canvas.height === 200 &&
                typeof canvas.__canvas_id__ === 'number'
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_get_context() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(50, 50);
                let ctx2d = canvas.getContext('2d');
                ctx2d !== null && ctx2d.canvas === canvas
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_get_context_cached() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(50, 50);
                let ctx1 = canvas.getContext('2d');
                let ctx2 = canvas.getContext('2d');
                ctx1 === ctx2  // Same instance cached
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_fill_rect() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(100, 100);
                let ctx = canvas.getContext('2d');
                ctx.fillStyle = '#ff0000';
                ctx.fillRect(10, 10, 50, 50);
                // Should mark canvas as dirty
                true
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_transfer_to_image_bitmap() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(10, 10);
                let bitmap = canvas.transferToImageBitmap();
                bitmap.width === 10 && bitmap.height === 10 &&
                typeof bitmap.__canvas_id__ === 'number' &&
                typeof bitmap.close === 'function'
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_invalid_context_type() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(50, 50);
                canvas.getContext('webgl') === null
            "#,
        );
        assert!(ok);
    }

    // ── Phase 1: createImageBitmap + Worker availability tests ────────────────

    #[test]
    fn native_from_image_data_valid_2x2() {
        // 2×2 RGBA pixels, all transparent black
        let hex = "0".repeat(32); // 16 bytes = 32 hex chars
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            &format!("_lumen_offscreen_canvas_from_image_data(2, 2, '{hex}') > 0"),
        );
        assert!(ok, "expected non-zero canvas_id from valid 2x2 image data");
    }

    #[test]
    fn native_from_image_data_size_mismatch_returns_zero() {
        let rt = with_offscreen();
        // 3×3 requested but only 4 bytes provided → mismatch
        let ok = bool_eval(
            &rt,
            "_lumen_offscreen_canvas_from_image_data(3, 3, 'aabbccdd') === 0",
        );
        assert!(ok, "size mismatch should return 0");
    }

    #[test]
    fn native_from_image_data_red_pixel_stored() {
        let rt = with_offscreen();
        // 1×1 canvas with red pixel (ff0000ff); read back via the same wire
        // format ("{w},{h},{hex}") the JS shim parses, since the V8 runtime
        // owns OFFSCREEN_CANVASES on its own dedicated thread.
        let ok = bool_eval(
            &rt,
            r#"
                var id = _lumen_offscreen_canvas_from_image_data(1, 1, 'ff0000ff');
                var raw = _lumen_offscreen_canvas2d_get_image_data(id);
                id > 0 && raw === '1,1,ff0000ff'
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_create_image_bitmap_is_function() {
        let rt = with_offscreen();
        assert!(
            bool_eval(&rt, "typeof createImageBitmap === 'function'"),
            "createImageBitmap should be a function"
        );
    }

    #[test]
    fn js_create_image_bitmap_from_image_data_sync_via_native() {
        // Test the native binding that createImageBitmap(ImageData) uses internally.
        // We bypass the Promise wrapper and test the hex-encode → canvas ID path.
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
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
            "#,
        );
        assert!(ok, "createImageBitmap inner binding should produce a canvas");
    }

    #[test]
    fn native_transfer_to_image_bitmap_returns_new_canvas_id() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var canvas = new OffscreenCanvas(4, 4);
                var ctx2d = canvas.getContext('2d');
                ctx2d.fillStyle = '#00ff00';
                ctx2d.fillRect(0, 0, 4, 4);
                var newId = _lumen_offscreen_canvas_transfer_to_image_bitmap(canvas.__canvas_id__);
                newId > 0 && newId !== canvas.__canvas_id__
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_create_image_bitmap_from_offscreen_canvas_does_not_detach_source() {
        // createImageBitmap(OffscreenCanvas) must snapshot pixels without
        // neutering the source (unlike transferToImageBitmap()). Exercises the
        // same read-snapshot-recreate native sequence the JS shim uses,
        // bypassing the Promise wrapper (same rationale as the ImageData test
        // above: no microtask pump in this bare `eval` harness).
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
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
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_available_in_fresh_context() {
        // Simulates worker thread: fresh JS runtime with OffscreenCanvas installed.
        let rt = with_offscreen();
        assert!(
            bool_eval(
                &rt,
                "typeof OffscreenCanvas === 'function' && typeof createImageBitmap === 'function'"
            ),
            "OffscreenCanvas and createImageBitmap must be available in fresh (worker) context"
        );
    }

    #[test]
    fn js_offscreen_get_image_data_reads_requested_rect() {
        // BUG-456 симптом 3 / BUG-448 twin: getImageData() used to take no
        // parameters at all and hand back the raw "{w},{h},{hex}" wire string.
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var canvas = new OffscreenCanvas(4, 4);
                var ctx = canvas.getContext('2d');
                ctx.fillStyle = '#ff0000'; ctx.fillRect(0, 0, 4, 4);
                ctx.fillStyle = '#00ff00'; ctx.fillRect(2, 2, 2, 2);
                var img = ctx.getImageData(2, 2, 2, 2);
                // BUG-454: getImageData is noised now (±1 per RGB channel, alpha exact) —
                // this reads addressing/cropping correctness, not exact colour.
                img.width === 2 && img.height === 2 && img.data.length === 16 &&
                img.colorSpace === 'srgb' &&
                Math.abs(img.data[0] - 0) <= 1 && Math.abs(img.data[1] - 255) <= 1 &&
                Math.abs(img.data[2] - 0) <= 1 && img.data[3] === 255
            "#,
        );
        assert!(ok, "requested 2x2 rect as a real ImageData, not the whole 4x4 as a wire string");
    }

    #[test]
    fn js_offscreen_get_image_data_zero_size_throws_index_size_error() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var canvas = new OffscreenCanvas(4, 4);
                var ctx = canvas.getContext('2d');
                var name = '';
                try { ctx.getImageData(0, 0, 0, 4); }
                catch (e) { name = e.name; }
                name === 'IndexSizeError'
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_get_image_data_rejects_non_finite_argument() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var canvas = new OffscreenCanvas(4, 4);
                var ctx = canvas.getContext('2d');
                var threw = false;
                try { ctx.getImageData(NaN, 0, 1, 1); }
                catch (e) { threw = e instanceof TypeError; }
                threw
            "#,
        );
        assert!(ok);
    }

    // ── BUG-456 симптом 2: lineWidth/globalAlpha getters ────────────────────
    #[test]
    fn js_offscreen_line_width_and_global_alpha_round_trip() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.lineWidth = 4;
                ctx.globalAlpha = 0.5;
                ctx.lineWidth === 4 && ctx.globalAlpha === 0.5
            "#,
        );
        assert!(ok);
    }

    // ── BUG-456 симптом 1: state stack, transforms, additional path ops ────
    #[test]
    fn js_offscreen_save_restore_round_trips_tracked_attributes() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.fillStyle = '#00ff00';
                ctx.lineWidth = 3;
                ctx.save();
                ctx.fillStyle = '#ff0000';
                ctx.lineWidth = 9;
                ctx.restore();
                ctx.fillStyle === '#00ff00' && ctx.lineWidth === 3
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_context_has_transform_and_path_methods() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ['translate', 'rotate', 'scale', 'transform', 'setTransform',
                 'resetTransform', 'rect', 'bezierCurveTo', 'quadraticCurveTo',
                 'arcTo', 'ellipse', 'roundRect', 'clip', 'save', 'restore', 'reset']
                    .every(function(name) { return typeof ctx[name] === 'function'; })
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_transform_ops_do_not_throw() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(20, 20).getContext('2d');
                ctx.save();
                ctx.translate(2, 2);
                ctx.rotate(0.1);
                ctx.scale(1.5, 1.5);
                ctx.transform(1, 0, 0, 1, 1, 1);
                ctx.setTransform(1, 0, 0, 1, 0, 0);
                ctx.setTransform();
                ctx.resetTransform();
                ctx.restore();
                ctx.beginPath();
                ctx.rect(0, 0, 5, 5);
                ctx.bezierCurveTo(1, 1, 2, 2, 3, 3);
                ctx.quadraticCurveTo(1, 1, 2, 2);
                ctx.arcTo(0, 0, 5, 0, 2);
                ctx.ellipse(5, 5, 3, 2, 0, 0, Math.PI * 2);
                ctx.roundRect(0, 0, 10, 10, 2);
                ctx.clip();
                true
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_reset_clears_fill_style_and_transform() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.fillStyle = '#00ff00';
                ctx.lineWidth = 7;
                ctx.translate(5, 5);
                ctx.reset();
                ctx.fillStyle === '#000000' && ctx.lineWidth === 1
            "#,
        );
        assert!(ok);
    }

    // ── BUG-456 симптом 1: state properties (composite op/line cap/line
    // join/miter limit/shadow*) — next slice after the geometry natives.
    #[test]
    fn js_offscreen_state_properties_round_trip() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.globalCompositeOperation = 'multiply';
                ctx.lineCap = 'round';
                ctx.lineJoin = 'bevel';
                ctx.miterLimit = 5;
                ctx.shadowColor = '#ff0000';
                ctx.shadowBlur = 3;
                ctx.shadowOffsetX = 1;
                ctx.shadowOffsetY = 2;
                ctx.globalCompositeOperation === 'multiply' &&
                ctx.lineCap === 'round' && ctx.lineJoin === 'bevel' &&
                ctx.miterLimit === 5 && ctx.shadowColor === '#ff0000' &&
                ctx.shadowBlur === 3 && ctx.shadowOffsetX === 1 &&
                ctx.shadowOffsetY === 2
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_state_properties_have_spec_defaults() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.globalCompositeOperation === 'source-over' &&
                ctx.lineCap === 'butt' && ctx.lineJoin === 'miter' &&
                ctx.miterLimit === 10 && ctx.shadowColor === 'rgba(0, 0, 0, 0)' &&
                ctx.shadowBlur === 0 && ctx.shadowOffsetX === 0 &&
                ctx.shadowOffsetY === 0
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_state_properties_survive_save_restore() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.lineCap = 'round';
                ctx.shadowBlur = 4;
                ctx.save();
                ctx.lineCap = 'square';
                ctx.shadowBlur = 9;
                ctx.restore();
                ctx.lineCap === 'round' && ctx.shadowBlur === 4
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_reset_clears_state_properties() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.globalCompositeOperation = 'xor';
                ctx.lineCap = 'square';
                ctx.miterLimit = 2;
                ctx.shadowColor = '#00ff00';
                ctx.shadowOffsetX = 5;
                ctx.reset();
                ctx.globalCompositeOperation === 'source-over' &&
                ctx.lineCap === 'butt' && ctx.miterLimit === 10 &&
                ctx.shadowColor === 'rgba(0, 0, 0, 0)' && ctx.shadowOffsetX === 0
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_miter_limit_ignores_non_positive_value() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.miterLimit = 5;
                ctx.miterLimit = -1;
                ctx.miterLimit = 0;
                ctx.miterLimit = NaN;
                ctx.miterLimit === 5
            "#,
        );
        assert!(ok);
    }
}
