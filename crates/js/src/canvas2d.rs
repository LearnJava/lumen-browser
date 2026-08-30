//! HTML Canvas 2D JS bindings (HTML Living Standard §4.12.4).
//!
//! Wires `canvas.getContext('2d')` to the CPU-rasterized [`lumen_canvas::Context2D`].
//! Drawing operations: `fillRect`, `clearRect`, `strokeRect`, `beginPath`,
//! `moveTo`, `lineTo`, `closePath`, `arc`, `ellipse`, `arcTo`, `rect`,
//! `bezierCurveTo`, `quadraticCurveTo`, `fill`, `stroke`, `save`, `restore`.
//! Transforms: `translate`, `rotate`, `scale`, `transform`, `setTransform`, `resetTransform`.
//! Properties: `fillStyle`, `strokeStyle`, `lineWidth`, `globalAlpha`,
//! `globalCompositeOperation`, `lineCap`, `lineJoin`, `miterLimit`.
//! Phase 5: `Path2D` object bindings — `_lumen_canvas2d_path2d_*` native functions;
//! fill/stroke/clip with Path2D; `isPointInPath` with Path2D.
//!
//! Each `<canvas>` is keyed by its DOM node index (`__nid__` on the JS side,
//! `LayoutBox::node.index()` on the layout side). The display list emits a
//! `DrawImage` with `src = "canvas:{nid}"`; the shell uploads the dirty pixel
//! buffer to the renderer under the same key.
//!
//! After any draw operation the canvas is marked "dirty". The shell drains
//! dirty buffers via [`flush_dirty`] each frame and uploads them to the GPU.

use std::cell::RefCell;
#[cfg(feature = "v8-backend")]
use std::cell::Cell;
use std::collections::HashMap;
#[cfg(feature = "v8-backend")]
use std::collections::HashSet;

#[cfg(feature = "v8-backend")]
use crate::img_bitmap_store;

use lumen_canvas::Context2D;
// The rest of `lumen_canvas`'s paint model (gradients/patterns/paths/composite
// ops/text color) is reachable only from `install_canvas2d_bindings_v8` — the
// rquickjs installer that used to reach it too was removed in S12b-B30.
#[cfg(feature = "v8-backend")]
use lumen_canvas::{
    CanvasColor, CanvasGradient, CanvasPattern, Path2dData, PaintSource, RepeatMode,
    CompositeOperation, LineCap, LineJoin,
};

thread_local! {
    /// Per-thread registry of live 2D contexts, keyed by DOM node index.
    static CANVASES: RefCell<HashMap<u32, Context2D>> = RefCell::new(HashMap::new());
    /// Node indices whose pixel buffer changed since the last [`flush_dirty`].
    static DIRTY: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    /// In-flight gradients awaiting `setFillStyle`/`setStrokeStyle`, keyed by object ID.
    #[cfg(feature = "v8-backend")]
    static GRADIENTS: RefCell<HashMap<u32, CanvasGradient>> = RefCell::new(HashMap::new());
    /// In-flight patterns, keyed by object ID.
    #[cfg(feature = "v8-backend")]
    static PATTERNS: RefCell<HashMap<u32, CanvasPattern>> = RefCell::new(HashMap::new());
    /// Auto-increment for gradient/pattern object IDs.
    #[cfg(feature = "v8-backend")]
    static NEXT_PAINT_ID: Cell<u32> = const { Cell::new(1) };
    /// Live `Path2D` objects, keyed by Path2D instance ID.
    #[cfg(feature = "v8-backend")]
    static PATHS: RefCell<HashMap<u32, Path2dData>> = RefCell::new(HashMap::new());
    /// Auto-increment for Path2D object IDs.
    #[cfg(feature = "v8-backend")]
    static NEXT_PATH_ID: Cell<u32> = const { Cell::new(1) };
    /// DOM canvas node indices whose control has been transferred to an OffscreenCanvas.
    ///
    /// Once transferred, `getContext()` returns null for these nids (HTML LS §4.12.14).
    #[cfg(feature = "v8-backend")]
    static TRANSFERRED: RefCell<HashSet<u32>> = RefCell::new(HashSet::new());
}

/// Allocate a new unique object ID for a gradient or pattern.
#[cfg(feature = "v8-backend")]
fn next_paint_id() -> u32 {
    NEXT_PAINT_ID.with(|c| {
        let id = c.get();
        c.set(id.wrapping_add(1).max(1));
        id
    })
}

/// Allocate a new unique object ID for a `Path2D`.
#[cfg(feature = "v8-backend")]
fn next_path_id() -> u32 {
    NEXT_PATH_ID.with(|c| {
        let id = c.get();
        c.set(id.wrapping_add(1).max(1));
        id
    })
}

/// Decode a hex string (`"ff00aa"`) into bytes. Silently ignores odd-length or bad chars.
#[cfg(feature = "v8-backend")]
fn decode_hex(s: &str) -> Vec<u8> {
    let s = s.trim_start_matches("0x");
    let n = s.len() / 2;
    let mut out = Vec::with_capacity(n);
    let bytes = s.as_bytes();
    for i in 0..n {
        let hi = hex_nibble(bytes[i * 2]);
        let lo = hex_nibble(bytes[i * 2 + 1]);
        out.push((hi << 4) | lo);
    }
    out
}

#[cfg(feature = "v8-backend")]
fn hex_nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

/// Maximum canvas dimension in CSS pixels. Clamps hostile/oversized buffers.
const MAX_CANVAS_DIM: u32 = 4096;

// ── Canvas text rendering helpers (Phase 4) ───────────────────────────────────

/// Bundled Inter font for canvas text operations.
const BUNDLED_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");

/// Parse pixel size from a CSS font string, e.g. `"bold 16px sans-serif"` → 16.0.
///
/// Iterates space-separated tokens and takes the first one ending in `"px"`.
/// Falls back to the Canvas 2D spec default (10 px) if no token matches.
fn parse_canvas_font_size(font: &str) -> f32 {
    for part in font.split_ascii_whitespace() {
        if let Some(px) = part.strip_suffix("px")
            && let Ok(v) = px.parse::<f32>()
        {
            return v.max(1.0);
        }
    }
    10.0
}

/// Measure total advance width of `text` in pixels using the bundled Inter font at `pixel_size`.
///
/// Returns a fallback estimate (0.55 × pixel_size per char) when font parsing fails.
fn measure_text_width(text: &str, pixel_size: f32) -> f64 {
    let Ok(font) = lumen_font::Font::parse(BUNDLED_FONT) else {
        return text.chars().count() as f64 * f64::from(pixel_size) * 0.55;
    };
    let Ok(head) = font.head() else {
        return text.chars().count() as f64 * f64::from(pixel_size) * 0.55;
    };
    let Ok(cmap) = font.cmap() else {
        return text.chars().count() as f64 * f64::from(pixel_size) * 0.55;
    };
    let Ok(hmtx) = font.hmtx() else {
        return text.chars().count() as f64 * f64::from(pixel_size) * 0.55;
    };
    let advance_scale = f64::from(pixel_size) / f64::from(head.units_per_em);
    text.chars()
        .map(|ch| {
            let gid = cmap.glyph_index(ch as u32).unwrap_or(0);
            f64::from(hmtx.advance_width(gid).unwrap_or(0)) * advance_scale
        })
        .sum()
}

/// Vertical position of the line named by `text_baseline`, in pixels above the
/// alphabetic baseline. Mirrors `render_text_to_canvas`'s own baseline table —
/// the metrics a page reads must describe the text the rasterizer draws.
fn baseline_offset_px(text_baseline: &str, ascent_px: f32, pixel_size: f32) -> f32 {
    match text_baseline {
        "top" => ascent_px,
        "hanging" => ascent_px * 0.85,
        "middle" => ascent_px - pixel_size * 0.5,
        "ideographic" | "bottom" => ascent_px - pixel_size,
        _ => 0.0, // "alphabetic" — the baseline itself
    }
}

/// The twelve `TextMetrics` attributes of `text` under the context's current
/// font, `textAlign` and `textBaseline` (canvas §4.12.5.1.13), in IDL order:
/// width, actualBoundingBox{Left,Right,Ascent,Descent},
/// fontBoundingBox{Ascent,Descent}, emHeight{Ascent,Descent},
/// hanging/alphabetic/ideographic baseline.
///
/// BUG-449: the shim used to report three of these and derive them from the
/// font size alone. Horizontal extents come from the real glyph bounding boxes,
/// vertical ones from `hhea`; every value is relative to the alignment point and
/// to the `textBaseline` line, as the spec measures them.
#[cfg(feature = "v8-backend")]
fn text_metrics(nid: u32, text: &str) -> Vec<f64> {
    let (font_str, text_align, text_baseline) = CANVASES.with(|c| {
        c.borrow()
            .get(&nid)
            .map(|ctx| (ctx.font.clone(), ctx.text_align.clone(), ctx.text_baseline.clone()))
            .unwrap_or_default()
    });
    let pixel_size = parse_canvas_font_size(&font_str);
    let width = measure_text_width(text, pixel_size);

    let parsed = lumen_font::Font::parse(BUNDLED_FONT).ok();
    let tables = parsed.as_ref().and_then(|f| {
        match (f.head(), f.hhea(), f.cmap(), f.hmtx()) {
            (Ok(head), Ok(hhea), Ok(cmap), Ok(hmtx)) => Some((f, head, hhea, cmap, hmtx)),
            _ => None,
        }
    });
    let Some((font, head, hhea, cmap, hmtx)) = tables else {
        // No font: report the advance width and leave every box empty rather
        // than invent a shape for glyphs nothing could measure.
        return vec![width, 0.0, width, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    };
    let upem = f32::from(head.units_per_em).max(1.0);
    let scale = pixel_size / upem;
    let font_ascent = f32::from(hhea.ascent) * scale;
    let font_descent = f32::from(hhea.descent) * scale; // negative, y-up
    let baseline = baseline_offset_px(&text_baseline, font_ascent, pixel_size);

    // Ink box of the whole run, in y-up pixels around the alphabetic baseline.
    let mut pen = 0.0f32;
    let (mut x_min, mut x_max, mut y_min, mut y_max) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for ch in text.chars() {
        let gid = cmap.glyph_index(ch as u32).unwrap_or(0);
        if let Ok(Some(glyph)) = font.glyph_resolved(gid)
            && !glyph.bbox.is_inverted()
            && (glyph.bbox.x_min != glyph.bbox.x_max || glyph.bbox.y_min != glyph.bbox.y_max)
        {
            // A blank glyph (a space) carries a degenerate box and contributes
            // nothing to the ink extents, only to the advance below.
            let bb = glyph.bbox;
            x_min = x_min.min(pen + f32::from(bb.x_min) * scale);
            x_max = x_max.max(pen + f32::from(bb.x_max) * scale);
            y_min = y_min.min(f32::from(bb.y_min) * scale);
            y_max = y_max.max(f32::from(bb.y_max) * scale);
        }
        pen += f32::from(hmtx.advance_width(gid).unwrap_or(0)) * scale;
    }
    if x_min > x_max {
        // Empty string, or a run of glyphs with no outline at all (a space).
        x_min = 0.0;
        x_max = 0.0;
        y_min = 0.0;
        y_max = 0.0;
    }
    // The horizontal extents are measured from the alignment point, which is
    // where the text is anchored — not from the pen start.
    let anchor = match text_align.as_str() {
        "center" => width as f32 * 0.5,
        "right" | "end" => width as f32,
        _ => 0.0,
    };
    // Em square: its top sits at the ascent's share of the em, so that the box
    // spans exactly one em however the font splits ascent and descent.
    let em_top = pixel_size * (font_ascent / (font_ascent - font_descent).max(f32::EPSILON));
    let em_bottom = em_top - pixel_size;
    vec![
        width,
        f64::from(anchor - x_min),
        f64::from(x_max - anchor),
        f64::from(y_max - baseline),
        f64::from(baseline - y_min),
        f64::from(font_ascent - baseline),
        f64::from(baseline - font_descent),
        f64::from(em_top - baseline),
        f64::from(baseline - em_bottom),
        f64::from(font_ascent * 0.8 - baseline),
        f64::from(-baseline),
        f64::from(font_descent - baseline),
    ]
}

/// Render `text` at canvas position `(x, y)` with the given fill `color`.
///
/// `x` is the pen start; `y` is adjusted by `text_align` / `text_baseline` before use.
/// The baseline model matches HTML Canvas 2D §4.12.4: for the default `"alphabetic"` baseline,
/// `y` IS the baseline position (not the top of the glyph).
#[cfg(feature = "v8-backend")]
fn render_text_to_canvas(nid: u32, text: &str, x: f32, y: f32, color: CanvasColor) {
    if text.is_empty() {
        return;
    }
    let Ok(font) = lumen_font::Font::parse(BUNDLED_FONT) else { return };
    let (Ok(head), Ok(hhea), Ok(cmap), Ok(hmtx)) = (
        font.head(), font.hhea(), font.cmap(), font.hmtx(),
    ) else { return };

    let (font_str, text_align, text_baseline) = CANVASES.with(|c| {
        c.borrow()
            .get(&nid)
            .map(|ctx| (ctx.font.clone(), ctx.text_align.clone(), ctx.text_baseline.clone()))
            .unwrap_or_default()
    });

    let pixel_size = parse_canvas_font_size(&font_str);
    let units_per_em = head.units_per_em;
    let advance_scale = pixel_size / f32::from(units_per_em);
    let ascent_px = f32::from(hhea.ascent) / f32::from(units_per_em) * pixel_size;

    // Compute start_x accounting for textAlign (HTML Canvas 2D §4.12.4).
    let text_w = measure_text_width(text, pixel_size) as f32;
    let start_x = match text_align.as_str() {
        "center" => x - text_w * 0.5,
        "right" | "end" => x - text_w,
        _ => x,  // "left" | "start" (default)
    };

    // Compute baseline_y from textBaseline.
    let baseline_y = match text_baseline.as_str() {
        "top"      => y + ascent_px,
        "hanging"  => y + ascent_px * 0.85,
        "middle"   => y + ascent_px - pixel_size * 0.5,
        "ideographic" | "bottom" => y + ascent_px - pixel_size,
        _          => y,  // "alphabetic" (default) — y IS the baseline
    };

    let rasterizer = lumen_font::Rasterizer::new(pixel_size, units_per_em);
    // Collect (x_offset, baseline_y, w, h, pixels, color) for every glyph.
    let mut glyph_bufs: Vec<(f32, f32, u32, u32, Vec<u8>, CanvasColor)> = Vec::new();
    let mut cursor_x = start_x;
    for ch in text.chars() {
        let gid = cmap.glyph_index(ch as u32).unwrap_or(0);
        if let Ok(Some(glyph)) = font.glyph_resolved(gid)
            && let Some(bm) = rasterizer.rasterize(&glyph)
        {
            glyph_bufs.push((
                cursor_x + bm.left,
                baseline_y - bm.top,
                bm.width,
                bm.height,
                bm.pixels,
                color,
            ));
        }
        let adv = f32::from(hmtx.advance_width(gid).unwrap_or(0));
        cursor_x += adv * advance_scale;
    }

    if glyph_bufs.is_empty() {
        return;
    }
    // Build slice references and call into the canvas (separate borrow from above).
    #[allow(clippy::type_complexity)]
    let glyphs: Vec<(f32, f32, u32, u32, &[u8], CanvasColor)> = glyph_bufs
        .iter()
        .map(|(gx, gy, gw, gh, px, c)| (*gx, *gy, *gw, *gh, px.as_slice(), *c))
        .collect();
    with_canvas(nid, |ctx| ctx.fill_text_glyphs(&glyphs));
}

/// Run `f` against the context for `nid`, returning `R::default()` if absent.
#[cfg(feature = "v8-backend")]
fn with_canvas<F, R>(nid: u32, f: F) -> R
where
    F: FnOnce(&mut Context2D) -> R,
    R: Default,
{
    CANVASES.with(|c| {
        if let Ok(mut map) = c.try_borrow_mut()
            && let Some(ctx) = map.get_mut(&nid)
        {
            return f(ctx);
        }
        R::default()
    })
}

/// Mark `nid`'s pixel buffer as changed so the shell re-uploads it.
fn mark_dirty(nid: u32) {
    DIRTY.with(|d| {
        if let Ok(mut v) = d.try_borrow_mut()
            && !v.contains(&nid)
        {
            v.push(nid);
        }
    });
}

/// Present a WebGPU-rendered RGBA8 frame into the `<canvas>` `nid`'s CPU buffer.
///
/// Used by the WebGPU canvas-present path (`getContext('webgpu')` → render → `queue.submit`):
/// `lumen_paint::webgpu_compute::texture_read_rgba` reads the rendered texture back to dense
/// top-left-origin RGBA8, and this writes it into the same `Context2D` buffer the shell uploads
/// as `canvas:{nid}`. Creates the backing context if absent and resizes it to match the frame,
/// then marks the canvas dirty so `flush_dirty` re-uploads it next frame.
pub fn present_rgba(nid: u32, width: u32, height: u32, rgba: &[u8]) {
    let w = width.clamp(1, MAX_CANVAS_DIM);
    let h = height.clamp(1, MAX_CANVAS_DIM);
    CANVASES.with(|c| {
        if let Ok(mut map) = c.try_borrow_mut() {
            let ctx = map.entry(nid).or_insert_with(|| Context2D::new(w, h));
            if ctx.width() != w || ctx.height() != h {
                ctx.resize(w, h);
            }
            ctx.put_image_data(rgba, w, h, 0, 0);
        }
    });
    mark_dirty(nid);
}

/// Drain dirty canvases and return their current RGBA buffers.
///
/// Each tuple is `(node_index, width, height, rgba_pixels)` where `rgba_pixels`
/// is row-major RGBA8 (top-left origin). The shell uploads each as
/// `Renderer::register_image("canvas:{nid}", ...)` and requests a repaint.
///
/// Called from `QuickJsRuntime::flush_canvas_updates` once per frame.
pub fn flush_dirty() -> Vec<(u32, u32, u32, Vec<u8>)> {
    let dirty: Vec<u32> = DIRTY.with(|d| {
        d.try_borrow_mut()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    });
    if dirty.is_empty() {
        return Vec::new();
    }
    CANVASES.with(|c| {
        let Ok(map) = c.try_borrow() else {
            return Vec::new();
        };
        dirty
            .into_iter()
            .filter_map(|nid| {
                map.get(&nid)
                    .map(|ctx| (nid, ctx.width(), ctx.height(), ctx.pixels().to_vec()))
            })
            .collect()
    })
}

/// Native for `ImageBitmapRenderingContext.transferFromImageBitmap`. See its
/// native registration in [`install_canvas2d_bindings_v8`] for the contract.
#[cfg(feature = "v8-backend")]
fn bitmaprenderer_transfer_native(nid: u32, canvas_id: u32) -> bool {
    match crate::offscreen_canvas::take_offscreen_pixels(canvas_id) {
        Some((w, h, pixels)) => {
            present_rgba(nid, w, h, &pixels);
            true
        }
        None => false,
    }
}

/// Registers Canvas 2D natives on `rt` (Ph3 V8 migration S8; the rquickjs
/// `install_canvas2d_bindings` this ported from was removed in S12b-B30). All state
/// (`CANVASES`, `DIRTY`, `GRADIENTS`, `PATTERNS`, `PATHS`, `TRANSFERRED`) is
/// module-level `thread_local!`, keyed by DOM node index / auto-increment
/// object ID — not a `V8JsRuntime` field — so this needs no new runtime
/// plumbing, same pattern as `video_bindings_v8`/`audio_element_v8`. The
/// `getContext('2d')` JS shim already lives in `dom.rs::WEB_API_SHIM`, shared
/// by both engines, so this only registers the `_lumen_canvas2d_*`/
/// `_lumen_canvas_*` natives.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_canvas2d_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{
        into_v8_fn1, into_v8_fn2, into_v8_fn3, into_v8_fn4, into_v8_fn5, into_v8_fn6,
        into_v8_fn7,
    };

    rt.register_native(
        "_lumen_canvas2d_create",
        into_v8_fn3(|nid: u32, w: u32, h: u32| {
            let w = w.clamp(1, MAX_CANVAS_DIM);
            let h = h.clamp(1, MAX_CANVAS_DIM);
            CANVASES.with(|c| {
                if let Ok(mut map) = c.try_borrow_mut() {
                    map.entry(nid).or_insert_with(|| Context2D::new(w, h));
                }
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_resize",
        into_v8_fn3(|nid: u32, w: u32, h: u32| {
            let w = w.clamp(1, MAX_CANVAS_DIM);
            let h = h.clamp(1, MAX_CANVAS_DIM);
            with_canvas(nid, |c| c.resize(w, h));
            mark_dirty(nid);
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_scale_resize",
        into_v8_fn3(|nid: u32, w: u32, h: u32| {
            let w = w.clamp(1, MAX_CANVAS_DIM);
            let h = h.clamp(1, MAX_CANVAS_DIM);
            with_canvas(nid, |c| c.scale_resize(w, h));
            mark_dirty(nid);
        }),
    )?;

    // ── Rectangles ──────────────────────────────────────────────────────────
    rt.register_native(
        "_lumen_canvas2d_fill_rect",
        into_v8_fn5(|nid: u32, x: f64, y: f64, w: f64, h: f64| {
            with_canvas(nid, |c| c.fill_rect(x as f32, y as f32, w as f32, h as f32));
            mark_dirty(nid);
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_clear_rect",
        into_v8_fn5(|nid: u32, x: f64, y: f64, w: f64, h: f64| {
            with_canvas(nid, |c| c.clear_rect(x as f32, y as f32, w as f32, h as f32));
            mark_dirty(nid);
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_stroke_rect",
        into_v8_fn5(|nid: u32, x: f64, y: f64, w: f64, h: f64| {
            with_canvas(nid, |c| c.stroke_rect(x as f32, y as f32, w as f32, h as f32));
            mark_dirty(nid);
        }),
    )?;

    // ── Paths ───────────────────────────────────────────────────────────────
    rt.register_native(
        "_lumen_canvas2d_begin_path",
        into_v8_fn1(|nid: u32| {
            with_canvas(nid, |c| c.begin_path());
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_move_to",
        into_v8_fn3(|nid: u32, x: f64, y: f64| {
            with_canvas(nid, |c| c.move_to(x as f32, y as f32));
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_line_to",
        into_v8_fn3(|nid: u32, x: f64, y: f64| {
            with_canvas(nid, |c| c.line_to(x as f32, y as f32));
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_close_path",
        into_v8_fn1(|nid: u32| {
            with_canvas(nid, |c| c.close_path());
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_arc",
        into_v8_fn7(|nid: u32, cx: f64, cy: f64, r: f64, sa: f64, ea: f64, ccw: bool| {
            with_canvas(nid, |c| {
                c.arc(cx as f32, cy as f32, r as f32, sa as f32, ea as f32, ccw)
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_fill",
        into_v8_fn1(|nid: u32| {
            with_canvas(nid, |c| c.fill());
            mark_dirty(nid);
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_stroke",
        into_v8_fn1(|nid: u32| {
            with_canvas(nid, |c| c.stroke());
            mark_dirty(nid);
        }),
    )?;

    // ── Style setters ─────────────────────────────────────────────────────────
    rt.register_native(
        "_lumen_canvas2d_set_fill_style",
        into_v8_fn2(|nid: u32, css: String| {
            with_canvas(nid, |c| {
                if let Some(color) = CanvasColor::from_css_str(&css) {
                    c.fill_style = PaintSource::Color(color);
                }
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_stroke_style",
        into_v8_fn2(|nid: u32, css: String| {
            with_canvas(nid, |c| {
                if let Some(color) = CanvasColor::from_css_str(&css) {
                    c.stroke_style = PaintSource::Color(color);
                }
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_line_width",
        into_v8_fn2(|nid: u32, w: f64| {
            if w.is_finite() && w > 0.0 {
                with_canvas(nid, |c| c.line_width = w as f32);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_global_alpha",
        into_v8_fn2(|nid: u32, a: f64| {
            if a.is_finite() && (0.0..=1.0).contains(&a) {
                with_canvas(nid, |c| c.global_alpha = a as f32);
            }
        }),
    )?;

    // ── State stack ───────────────────────────────────────────────────────────
    rt.register_native(
        "_lumen_canvas2d_save",
        into_v8_fn1(|nid: u32| {
            with_canvas(nid, |c| c.save());
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_restore",
        into_v8_fn1(|nid: u32| {
            with_canvas(nid, |c| c.restore());
        }),
    )?;

    // ── Transforms ────────────────────────────────────────────────────────────
    rt.register_native(
        "_lumen_canvas2d_translate",
        into_v8_fn3(|nid: u32, tx: f64, ty: f64| {
            with_canvas(nid, |c| c.translate(tx as f32, ty as f32));
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_rotate",
        into_v8_fn2(|nid: u32, angle: f64| {
            with_canvas(nid, |c| c.rotate(angle as f32));
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_scale",
        into_v8_fn3(|nid: u32, sx: f64, sy: f64| {
            with_canvas(nid, |c| c.scale(sx as f32, sy as f32));
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_transform",
        into_v8_fn7(|nid: u32, a: f64, b: f64, c2: f64, d: f64, e: f64, f2: f64| {
            with_canvas(nid, |c| {
                c.transform(a as f32, b as f32, c2 as f32, d as f32, e as f32, f2 as f32);
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_transform",
        into_v8_fn7(|nid: u32, a: f64, b: f64, c2: f64, d: f64, e: f64, f2: f64| {
            with_canvas(nid, |c| {
                c.set_transform(a as f32, b as f32, c2 as f32, d as f32, e as f32, f2 as f32);
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_reset_transform",
        into_v8_fn1(|nid: u32| {
            with_canvas(nid, |c| c.reset_transform());
        }),
    )?;

    // ── Bézier curves and additional path operations ───────────────────────────
    rt.register_native(
        "_lumen_canvas2d_bezier_curve_to",
        into_v8_fn7(|nid: u32, cp1x: f64, cp1y: f64, cp2x: f64, cp2y: f64, x: f64, y: f64| {
            with_canvas(nid, |c| {
                c.bezier_curve_to(
                    cp1x as f32, cp1y as f32,
                    cp2x as f32, cp2y as f32,
                    x as f32, y as f32,
                );
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_quadratic_curve_to",
        into_v8_fn5(|nid: u32, cpx: f64, cpy: f64, x: f64, y: f64| {
            with_canvas(nid, |c| {
                c.quadratic_curve_to(cpx as f32, cpy as f32, x as f32, y as f32);
            });
        }),
    )?;
    // Note: `ellipse` is implemented in the JS shim (dom.rs, shared by both engines).
    rt.register_native(
        "_lumen_canvas2d_arc_to",
        into_v8_fn6(|nid: u32, x1: f64, y1: f64, x2: f64, y2: f64, r: f64| {
            with_canvas(nid, |c| {
                c.arc_to(x1 as f32, y1 as f32, x2 as f32, y2 as f32, r as f32);
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_rect",
        into_v8_fn5(|nid: u32, x: f64, y: f64, w: f64, h: f64| {
            with_canvas(nid, |c| c.rect(x as f32, y as f32, w as f32, h as f32));
        }),
    )?;

    // ── Additional property setters ───────────────────────────────────────────
    rt.register_native(
        "_lumen_canvas2d_set_global_composite_operation",
        into_v8_fn2(|nid: u32, op: String| {
            if let Some(op) = CompositeOperation::from_str(&op) {
                with_canvas(nid, |c| c.composite_operation = op);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_line_cap",
        into_v8_fn2(|nid: u32, cap: String| {
            if let Some(cap) = LineCap::from_str(&cap) {
                with_canvas(nid, |c| c.line_cap = cap);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_line_join",
        into_v8_fn2(|nid: u32, join: String| {
            if let Some(join) = LineJoin::from_str(&join) {
                with_canvas(nid, |c| c.line_join = join);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_miter_limit",
        into_v8_fn2(|nid: u32, limit: f64| {
            if limit.is_finite() && limit > 0.0 {
                with_canvas(nid, |c| c.miter_limit = limit as f32);
            }
        }),
    )?;

    // ── Gradients ────────────────────────────────────────────────────────────
    rt.register_native(
        "_lumen_canvas2d_create_linear_gradient",
        into_v8_fn5(|_nid: u32, x0: f64, y0: f64, x1: f64, y1: f64| -> u32 {
            let id = next_paint_id();
            GRADIENTS.with(|gs| {
                if let Ok(mut map) = gs.try_borrow_mut() {
                    map.insert(id, CanvasGradient::linear(x0 as f32, y0 as f32, x1 as f32, y1 as f32));
                }
            });
            id
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_create_radial_gradient",
        into_v8_fn7(|_nid: u32, x0: f64, y0: f64, r0: f64, x1: f64, y1: f64, r1: f64| -> u32 {
            let id = next_paint_id();
            GRADIENTS.with(|gs| {
                if let Ok(mut map) = gs.try_borrow_mut() {
                    map.insert(id, CanvasGradient::radial(
                        x0 as f32, y0 as f32, r0 as f32,
                        x1 as f32, y1 as f32, r1 as f32,
                    ));
                }
            });
            id
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_create_conic_gradient",
        into_v8_fn4(|_nid: u32, angle: f64, cx: f64, cy: f64| -> u32 {
            let id = next_paint_id();
            GRADIENTS.with(|gs| {
                if let Ok(mut map) = gs.try_borrow_mut() {
                    map.insert(id, CanvasGradient::conic(angle as f32, cx as f32, cy as f32));
                }
            });
            id
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_gradient_add_color_stop",
        into_v8_fn3(|grad_id: u32, offset: f64, css: String| {
            if let Some(color) = CanvasColor::from_css_str(&css) {
                GRADIENTS.with(|gs| {
                    if let Ok(mut map) = gs.try_borrow_mut()
                        && let Some(g) = map.get_mut(&grad_id)
                    {
                        g.add_color_stop(offset as f32, color);
                    }
                });
            }
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_fill_style_gradient",
        into_v8_fn2(|nid: u32, grad_id: u32| {
            let grad = GRADIENTS.with(|gs| gs.try_borrow().ok()?.get(&grad_id).cloned());
            if let Some(g) = grad {
                with_canvas(nid, |c| c.fill_style = PaintSource::Gradient(g));
            }
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_stroke_style_gradient",
        into_v8_fn2(|nid: u32, grad_id: u32| {
            let grad = GRADIENTS.with(|gs| gs.try_borrow().ok()?.get(&grad_id).cloned());
            if let Some(g) = grad {
                with_canvas(nid, |c| c.stroke_style = PaintSource::Gradient(g));
            }
        }),
    )?;

    // ── Patterns ─────────────────────────────────────────────────────────────
    rt.register_native(
        "_lumen_canvas2d_create_pattern",
        into_v8_fn2(|src_nid: u32, repeat_str: String| -> u32 {
            let repeat = match repeat_str.as_str() {
                "repeat-x"  => RepeatMode::RepeatX,
                "repeat-y"  => RepeatMode::RepeatY,
                "no-repeat" => RepeatMode::NoRepeat,
                _            => RepeatMode::Repeat,
            };
            let pat = CANVASES.with(|c| {
                let map = c.try_borrow().ok()?;
                let src = map.get(&src_nid)?;
                Some(CanvasPattern::new(src.pixels().to_vec(), src.width(), src.height(), repeat))
            });
            let Some(p) = pat else { return 0; };
            let id = next_paint_id();
            PATTERNS.with(|ps| {
                if let Ok(mut map) = ps.try_borrow_mut() {
                    map.insert(id, p);
                }
            });
            id
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_fill_style_pattern",
        into_v8_fn2(|nid: u32, pat_id: u32| {
            let pat = PATTERNS.with(|ps| ps.try_borrow().ok()?.get(&pat_id).cloned());
            if let Some(p) = pat {
                with_canvas(nid, |c| c.fill_style = PaintSource::Pattern(p));
            }
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_stroke_style_pattern",
        into_v8_fn2(|nid: u32, pat_id: u32| {
            let pat = PATTERNS.with(|ps| ps.try_borrow().ok()?.get(&pat_id).cloned());
            if let Some(p) = pat {
                with_canvas(nid, |c| c.stroke_style = PaintSource::Pattern(p));
            }
        }),
    )?;

    // ── Shadow ───────────────────────────────────────────────────────────────
    rt.register_native(
        "_lumen_canvas2d_set_shadow_color",
        into_v8_fn2(|nid: u32, css: String| {
            with_canvas(nid, |c| {
                if let Some(color) = CanvasColor::from_css_str(&css) {
                    c.shadow_color = color;
                }
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_shadow_blur",
        into_v8_fn2(|nid: u32, v: f64| {
            if v.is_finite() && v >= 0.0 {
                with_canvas(nid, |c| c.shadow_blur = v as f32);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_shadow_offset_x",
        into_v8_fn2(|nid: u32, v: f64| {
            if v.is_finite() {
                with_canvas(nid, |c| c.shadow_offset_x = v as f32);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_shadow_offset_y",
        into_v8_fn2(|nid: u32, v: f64| {
            if v.is_finite() {
                with_canvas(nid, |c| c.shadow_offset_y = v as f32);
            }
        }),
    )?;

    // ── Clip ─────────────────────────────────────────────────────────────────
    rt.register_native(
        "_lumen_canvas2d_clip",
        into_v8_fn1(|nid: u32| {
            with_canvas(nid, |c| c.clip());
        }),
    )?;

    // ── drawImage ────────────────────────────────────────────────────────────
    rt.register_native(
        "_lumen_canvas2d_draw_image",
        into_v8_fn6(|dst_nid: u32, src_nid: u32, dx: f64, dy: f64, dw: f64, dh: f64| {
            let (pixels, sw, sh) = CANVASES.with(|c| {
                let map = c.try_borrow().ok()?;
                let src = map.get(&src_nid)?;
                Some((src.pixels().to_vec(), src.width(), src.height()))
            }).unwrap_or_default();
            if sw > 0 && sh > 0 {
                with_canvas(dst_nid, |c| {
                    c.draw_image(&pixels, sw, sh, dx as f32, dy as f32, dw as f32, dh as f32);
                });
                mark_dirty(dst_nid);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_draw_image_crop",
        into_v8_fn3(|dst_nid: u32, src_nid: u32, coords_csv: String| {
            let r: Vec<f32> = coords_csv.split(',').filter_map(|s| s.parse().ok()).collect();
            if r.len() != 8 {
                return;
            }
            let (pixels, sw, sh) = CANVASES.with(|c| {
                let map = c.try_borrow().ok()?;
                let src = map.get(&src_nid)?;
                Some((src.pixels().to_vec(), src.width(), src.height()))
            }).unwrap_or_default();
            if sw > 0 && sh > 0 {
                with_canvas(dst_nid, |c| {
                    c.draw_image_cropped(
                        &pixels, sw, sh,
                        r[0], r[1], r[2], r[3],
                        r[4], r[5], r[6], r[7],
                    );
                });
                mark_dirty(dst_nid);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_draw_image_from_img",
        into_v8_fn6(|dst_nid: u32, img_nid: u32, dx: f64, dy: f64, dw: f64, dh: f64| {
            img_bitmap_store::with_img_bitmap(img_nid, |iw, ih, pixels| {
                let w = if dw > 0.0 { dw as f32 } else { iw as f32 };
                let h = if dh > 0.0 { dh as f32 } else { ih as f32 };
                with_canvas(dst_nid, |c| {
                    c.draw_image(pixels, iw, ih, dx as f32, dy as f32, w, h);
                });
                mark_dirty(dst_nid);
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_draw_image_crop_from_img",
        into_v8_fn3(|dst_nid: u32, img_nid: u32, coords_csv: String| {
            let r: Vec<f32> = coords_csv.split(',').filter_map(|s| s.parse().ok()).collect();
            if r.len() != 8 {
                return;
            }
            img_bitmap_store::with_img_bitmap(img_nid, |iw, ih, pixels| {
                with_canvas(dst_nid, |c| {
                    c.draw_image_cropped(
                        pixels, iw, ih,
                        r[0], r[1], r[2], r[3],
                        r[4], r[5], r[6], r[7],
                    );
                });
                mark_dirty(dst_nid);
            });
        }),
    )?;

    // ── ImageData ────────────────────────────────────────────────────────────
    rt.register_native(
        "_lumen_canvas2d_put_image_data",
        into_v8_fn6(|nid: u32, hex: String, sw: u32, sh: u32, dx: i32, dy: i32| {
            let data = decode_hex(&hex);
            with_canvas(nid, |c| c.put_image_data(&data, sw, sh, dx, dy));
            mark_dirty(nid);
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_create_image_data",
        into_v8_fn2(|sw: u32, sh: u32| -> String {
            let data = Context2D::create_image_data(sw, sh);
            let mut s = String::with_capacity(data.len() * 2);
            use std::fmt::Write;
            for b in &data {
                let _ = write!(s, "{b:02x}");
            }
            s
        }),
    )?;

    // ── Text / Font ──────────────────────────────────────────────────────────
    rt.register_native(
        "_lumen_canvas2d_set_font",
        into_v8_fn2(|nid: u32, font: String| {
            with_canvas(nid, |c| c.font = font);
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_text_align",
        into_v8_fn2(|nid: u32, align: String| {
            with_canvas(nid, |c| c.text_align = align);
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_set_text_baseline",
        into_v8_fn2(|nid: u32, baseline: String| {
            with_canvas(nid, |c| c.text_baseline = baseline);
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_fill_text",
        into_v8_fn4(|nid: u32, text: String, x: f64, y: f64| {
            let color = CANVASES.with(|c| {
                c.borrow().get(&nid).map(|ctx| match &ctx.fill_style {
                    PaintSource::Color(col) => *col,
                    other => other.sample(x as f32, y as f32),
                })
            }).unwrap_or(CanvasColor::rgba(0, 0, 0, 255));
            render_text_to_canvas(nid, &text, x as f32, y as f32, color);
            mark_dirty(nid);
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_stroke_text",
        into_v8_fn4(|nid: u32, text: String, x: f64, y: f64| {
            let color = CANVASES.with(|c| {
                c.borrow().get(&nid).map(|ctx| match &ctx.stroke_style {
                    PaintSource::Color(col) => *col,
                    other => other.sample(x as f32, y as f32),
                })
            }).unwrap_or(CanvasColor::rgba(0, 0, 0, 255));
            render_text_to_canvas(nid, &text, x as f32, y as f32, color);
            mark_dirty(nid);
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_measure_text",
        into_v8_fn2(|nid: u32, text: String| -> f64 {
            let font_str = CANVASES.with(|c| {
                c.borrow().get(&nid).map(|ctx| ctx.font.clone()).unwrap_or_default()
            });
            let pixel_size = parse_canvas_font_size(&font_str);
            measure_text_width(&text, pixel_size)
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_text_metrics",
        into_v8_fn2(|nid: u32, text: String| -> Vec<f64> { text_metrics(nid, &text) }),
    )?;
    // BUG-448: the rectangle is a parameter, not a suggestion — the crop happens
    // here, where the bitmap already is, so a one-pixel read transports four
    // bytes rather than the whole canvas. Returns the RGBA8 bytes of the rect
    // (empty when the canvas is unknown or the area is unallocatable); the
    // shim owns the spec's argument checks and builds the `ImageData`.
    rt.register_native(
        "_lumen_canvas2d_get_image_data",
        into_v8_fn5(|nid: u32, sx: i32, sy: i32, sw: u32, sh: u32| -> Vec<u8> {
            CANVASES.with(|c| {
                let Ok(map) = c.try_borrow() else {
                    return Vec::new();
                };
                let Some(ctx) = map.get(&nid) else {
                    return Vec::new();
                };
                ctx.get_image_data_rect(sx, sy, sw, sh)
            })
        }),
    )?;

    // ── Path2D bindings ──────────────────────────────────────────────────────
    rt.register_native(
        "_lumen_canvas2d_path2d_new",
        into_v8_fn1(|svg: String| -> u32 {
            let path = if svg.is_empty() { Path2dData::new() } else { Path2dData::from_svg_str(&svg) };
            let id = next_path_id();
            PATHS.with(|p| p.borrow_mut().insert(id, path));
            id
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_path2d_free",
        into_v8_fn1(|path_id: u32| {
            PATHS.with(|p| p.borrow_mut().remove(&path_id));
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_path2d_move_to",
        into_v8_fn3(|path_id: u32, x: f64, y: f64| {
            PATHS.with(|p| {
                if let Some(pd) = p.borrow_mut().get_mut(&path_id) {
                    pd.move_to(x as f32, y as f32);
                }
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_path2d_line_to",
        into_v8_fn3(|path_id: u32, x: f64, y: f64| {
            PATHS.with(|p| {
                if let Some(pd) = p.borrow_mut().get_mut(&path_id) {
                    pd.line_to(x as f32, y as f32);
                }
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_path2d_close",
        into_v8_fn1(|path_id: u32| {
            PATHS.with(|p| {
                if let Some(pd) = p.borrow_mut().get_mut(&path_id) {
                    pd.close_path();
                }
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_path2d_bezier",
        into_v8_fn7(|path_id: u32, cp1x: f64, cp1y: f64, cp2x: f64, cp2y: f64, x: f64, y: f64| {
            PATHS.with(|p| {
                if let Some(pd) = p.borrow_mut().get_mut(&path_id) {
                    pd.bezier_curve_to(
                        cp1x as f32, cp1y as f32,
                        cp2x as f32, cp2y as f32,
                        x as f32, y as f32,
                    );
                }
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_path2d_quadratic",
        into_v8_fn5(|path_id: u32, cpx: f64, cpy: f64, x: f64, y: f64| {
            PATHS.with(|p| {
                if let Some(pd) = p.borrow_mut().get_mut(&path_id) {
                    pd.quadratic_curve_to(cpx as f32, cpy as f32, x as f32, y as f32);
                }
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_path2d_arc",
        into_v8_fn7(|path_id: u32, x: f64, y: f64, r: f64, start: f64, end: f64, ccw: bool| {
            PATHS.with(|p| {
                if let Some(pd) = p.borrow_mut().get_mut(&path_id) {
                    pd.arc(x as f32, y as f32, r as f32, start as f32, end as f32, ccw);
                }
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_path2d_arc_to",
        into_v8_fn6(|path_id: u32, x1: f64, y1: f64, x2: f64, y2: f64, r: f64| {
            PATHS.with(|p| {
                if let Some(pd) = p.borrow_mut().get_mut(&path_id) {
                    pd.arc_to(x1 as f32, y1 as f32, x2 as f32, y2 as f32, r as f32);
                }
            });
        }),
    )?;
    // _lumen_canvas2d_path2d_ellipse is exposed from the JS shim (dom.rs), same as QuickJS.
    rt.register_native(
        "_lumen_canvas2d_path2d_rect",
        into_v8_fn5(|path_id: u32, x: f64, y: f64, w: f64, h: f64| {
            PATHS.with(|p| {
                if let Some(pd) = p.borrow_mut().get_mut(&path_id) {
                    pd.rect(x as f32, y as f32, w as f32, h as f32);
                }
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_path2d_add_path",
        into_v8_fn3(|dst_id: u32, src_id: u32, transform_csv: String| {
            let transform: Option<[f32; 6]> = if transform_csv.is_empty() {
                None
            } else {
                let parts: Vec<f32> = transform_csv.split(',').filter_map(|s| s.parse().ok()).collect();
                if parts.len() == 6 {
                    Some([parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]])
                } else {
                    None
                }
            };
            PATHS.with(|p| {
                let map = p.borrow();
                if let Some(src) = map.get(&src_id) {
                    let src_clone = src.clone();
                    drop(map);
                    if let Some(dst) = p.borrow_mut().get_mut(&dst_id) {
                        dst.add_path(&src_clone, transform);
                    }
                }
            });
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_fill_path",
        into_v8_fn2(|nid: u32, path_id: u32| {
            let path = PATHS.with(|p| p.borrow().get(&path_id).cloned());
            if let Some(pd) = path {
                CANVASES.with(|c| {
                    if let Some(ctx2d) = c.borrow_mut().get_mut(&nid) {
                        ctx2d.fill_with_path2d(&pd);
                    }
                });
                mark_dirty(nid);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_stroke_path",
        into_v8_fn2(|nid: u32, path_id: u32| {
            let path = PATHS.with(|p| p.borrow().get(&path_id).cloned());
            if let Some(pd) = path {
                CANVASES.with(|c| {
                    if let Some(ctx2d) = c.borrow_mut().get_mut(&nid) {
                        ctx2d.stroke_with_path2d(&pd);
                    }
                });
                mark_dirty(nid);
            }
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_clip_path",
        into_v8_fn2(|nid: u32, path_id: u32| {
            let path = PATHS.with(|p| p.borrow().get(&path_id).cloned());
            if let Some(pd) = path {
                CANVASES.with(|c| {
                    if let Some(ctx2d) = c.borrow_mut().get_mut(&nid) {
                        ctx2d.clip_with_path2d(&pd);
                    }
                });
            }
        }),
    )?;
    rt.register_native(
        "_lumen_canvas2d_is_point_in_path",
        into_v8_fn4(|nid: u32, path_id: u32, x: f64, y: f64| -> bool {
            let path = PATHS.with(|p| p.borrow().get(&path_id).cloned());
            path.is_some_and(|pd| {
                CANVASES.with(|c| {
                    c.borrow().get(&nid)
                        .is_some_and(|ctx2d| ctx2d.is_point_in_path2d(&pd, x as f32, y as f32))
                })
            })
        }),
    )?;

    // ── transferControlToOffscreen (HTML LS §4.12.14) ─────────────────────────
    //
    // `offscreen_canvas.rs` is V8-ported (P1-imagebitmap,
    // `offscreen_canvas::install_offscreen_canvas_bindings_v8`), so the
    // resulting OffscreenCanvas object is fully functional under v8-backend too.
    rt.register_native(
        "_lumen_canvas_transfer_control_to_offscreen",
        into_v8_fn1(|nid: u32| -> String {
            let (w, h, pixels) = CANVASES.with(|c| {
                let Ok(mut map) = c.try_borrow_mut() else {
                    return (1u32, 1u32, vec![0u8; 4]);
                };
                let ctx2d = map.entry(nid).or_insert_with(|| Context2D::new(1, 1));
                (ctx2d.width(), ctx2d.height(), ctx2d.pixels().to_vec())
            });
            let offscreen_id = crate::offscreen_canvas::create_offscreen_from_pixels(w, h, pixels);
            TRANSFERRED.with(|t| {
                t.borrow_mut().insert(nid);
            });
            format!("{{\"__canvas_id__\":{offscreen_id},\"width\":{w},\"height\":{h}}}")
        }),
    )?;
    rt.register_native(
        "_lumen_canvas_is_transferred",
        into_v8_fn1(|nid: u32| -> bool { TRANSFERRED.with(|t| t.borrow().contains(&nid)) }),
    )?;

    // ── ImageBitmapRenderingContext.transferFromImageBitmap (HTML LS §4.12.5.1) ──
    rt.register_native(
        "_lumen_bitmaprenderer_transfer_from_image_bitmap",
        into_v8_fn2(bitmaprenderer_transfer_native),
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_canvas_font_size_extracts_px() {
        assert_eq!(parse_canvas_font_size("16px sans-serif"), 16.0);
        assert_eq!(parse_canvas_font_size("bold 12px Arial"), 12.0);
        assert_eq!(parse_canvas_font_size("italic 24px serif"), 24.0);
        assert_eq!(parse_canvas_font_size("10px sans-serif"), 10.0);
        // Default when no px found.
        assert_eq!(parse_canvas_font_size("sans-serif"), 10.0);
    }

    #[test]
    fn measure_text_width_returns_positive_for_ascii() {
        // Measure "A" at 16 px with bundled Inter — must be > 0 and < 16 px.
        let w = measure_text_width("A", 16.0);
        assert!(w > 0.0, "width should be positive, got {w}");
        assert!(w < 20.0, "single char at 16px should be < 20px, got {w}");
    }

    #[test]
    fn measure_text_width_proportional_to_length() {
        let w1 = measure_text_width("A", 16.0);
        let w3 = measure_text_width("AAA", 16.0);
        // 3× the same character should give 3× the width.
        assert!((w3 - w1 * 3.0).abs() < 0.1, "AAA should be 3× A: {w3} vs {}", w1 * 3.0);
    }

    #[test]
    fn measure_text_width_scales_with_font_size() {
        let w16 = measure_text_width("Hello", 16.0);
        let w32 = measure_text_width("Hello", 32.0);
        // 2× font size → 2× width.
        assert!((w32 - w16 * 2.0).abs() < 0.5, "32px should be 2× 16px: {w32} vs {}", w16 * 2.0);
    }

    /// [`present_rgba`] and [`flush_dirty`] are plain Rust functions with no JS
    /// engine involved — already engine-agnostic, no V8 port needed.
    #[test]
    fn present_rgba_writes_pixels_and_marks_dirty() {
        CANVASES.with(|c| c.borrow_mut().clear());
        DIRTY.with(|d| d.borrow_mut().clear());
        // 2×1 frame: red, green pixels — the WebGPU present path delivers dense RGBA8.
        let frame = [255u8, 0, 0, 255, 0, 255, 0, 255];
        present_rgba(77, 2, 1, &frame);
        let updates = flush_dirty();
        assert_eq!(updates.len(), 1, "present marks the canvas dirty exactly once");
        let (nid, w, h, pixels) = &updates[0];
        assert_eq!(*nid, 77);
        assert_eq!((*w, *h), (2, 1));
        assert_eq!(pixels, &frame, "presented pixels land in the canvas buffer verbatim");
    }
}

/// V8 test coverage for [`install_canvas2d_bindings_v8`] (S12b-B30; the rquickjs
/// suite this ports from was removed in the same batch): 26 of the 31 original
/// tests (the other 5 — `parse_canvas_font_size`/`measure_text_width`/
/// `present_rgba_writes_pixels_and_marks_dirty` — are pure Rust with no JS engine
/// involved, kept once in `mod tests` above, already covering both engines).
///
/// [`V8JsRuntime::new`] spawns a dedicated OS thread per runtime, so a bare
/// rquickjs-style peek at `CANVASES`/`DIRTY` from the test's own thread would see
/// an empty, unrelated `thread_local!` instance. Dirty-buffer assertions go
/// through the already-public [`V8JsRuntime::flush_canvas_updates`]; assertions
/// with no JS-visible getter native (`line_width`, `global_alpha`, `text_align`,
/// `text_baseline`) go through the new test-only [`V8JsRuntime::run_for_test`],
/// which runs an arbitrary closure on the JS thread.
#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    // `panic!` — штатный способ провалить тест; исключение из clippy.toml не
    // достаёт до хелперов модуля (docs/lint-policy.md §10).
    #![allow(clippy::panic, clippy::unwrap_used)]
    use lumen_core::JsValue;
    use lumen_core::ext::JsRuntime as _;

    use crate::v8_runtime::V8JsRuntime;

    fn with_canvas2d() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        super::install_canvas2d_bindings_v8(&rt).unwrap();
        rt
    }

    fn bool_eval(rt: &V8JsRuntime, expr: &str) -> bool {
        matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
    }

    fn str_eval(rt: &V8JsRuntime, expr: &str) -> String {
        match rt.eval(expr).unwrap() {
            JsValue::String(s) => s,
            other => panic!("expected string, got {other:?}"),
        }
    }

    /// Evaluate an expression yielding the RGBA byte array a `getImageData`
    /// native returns (`JsValue::Array` of numbers).
    fn bytes_eval(rt: &V8JsRuntime, expr: &str) -> Vec<u8> {
        match rt.eval(expr).unwrap() {
            JsValue::Array(items) => items
                .into_iter()
                .map(|v| match v {
                    JsValue::Number(n) => n as u8,
                    other => panic!("expected byte, got {other:?}"),
                })
                .collect(),
            other => panic!("expected array, got {other:?}"),
        }
    }

    fn num_eval(rt: &V8JsRuntime, expr: &str) -> f64 {
        match rt.eval(expr).unwrap() {
            JsValue::Number(n) => n,
            other => panic!("expected number, got {other:?}"),
        }
    }

    #[test]
    fn js_create_registers_context() {
        let rt = with_canvas2d();
        rt.eval("_lumen_canvas2d_create(7, 100, 50);").unwrap();
        let dims = rt.run_for_test(|| super::with_canvas(7, |c| (c.width(), c.height())));
        assert_eq!(dims, (100, 50));
    }

    #[test]
    fn js_create_clamps_dimensions() {
        let rt = with_canvas2d();
        rt.eval("_lumen_canvas2d_create(1, 0, 99999);").unwrap();
        let dims = rt.run_for_test(|| super::with_canvas(1, |c| (c.width(), c.height())));
        assert_eq!(
            dims,
            (1, super::MAX_CANVAS_DIM),
            "zero clamped up to 1, oversized clamped to max"
        );
    }

    #[test]
    fn js_create_is_idempotent_preserving_buffer() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(3, 10, 10);\
             _lumen_canvas2d_set_fill_style(3, '#ff0000');\
             _lumen_canvas2d_fill_rect(3, 0, 0, 10, 10);\
             _lumen_canvas2d_create(3, 10, 10);",
        )
        .unwrap();
        // Re-create must not wipe an existing buffer (entry().or_insert).
        assert_eq!(
            bytes_eval(&rt, "_lumen_canvas2d_get_image_data(3, 0, 0, 1, 1)"),
            vec![255, 0, 0, 255],
            "red preserved across re-create"
        );
    }

    #[test]
    fn js_fill_rect_marks_dirty_and_paints() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(2, 4, 4);\
             _lumen_canvas2d_set_fill_style(2, 'rgb(0,255,0)');\
             _lumen_canvas2d_fill_rect(2, 0, 0, 4, 4);",
        )
        .unwrap();
        let updates = rt.flush_canvas_updates();
        assert_eq!(updates.len(), 1);
        let (nid, w, h, rgba) = &updates[0];
        assert_eq!(*nid, 2);
        assert_eq!((*w, *h), (4, 4));
        assert_eq!(rgba[1], 255, "green channel painted");
    }

    #[test]
    fn js_flush_dirty_drains_once() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(5, 4, 4);\
             _lumen_canvas2d_fill_rect(5, 0, 0, 4, 4);",
        )
        .unwrap();
        assert_eq!(rt.flush_canvas_updates().len(), 1);
        assert!(rt.flush_canvas_updates().is_empty(), "second drain is empty");
    }

    #[test]
    fn js_clear_rect_marks_dirty() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(8, 4, 4);\
             _lumen_canvas2d_set_fill_style(8, '#0000ff');\
             _lumen_canvas2d_fill_rect(8, 0, 0, 4, 4);",
        )
        .unwrap();
        let _ = rt.flush_canvas_updates();
        rt.eval("_lumen_canvas2d_clear_rect(8, 0, 0, 4, 4);").unwrap();
        let updates = rt.flush_canvas_updates();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].3[3], 0, "alpha cleared to transparent");
    }

    #[test]
    fn js_path_fill_marks_dirty() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(9, 20, 20);\
             _lumen_canvas2d_set_fill_style(9, '#ffffff');\
             _lumen_canvas2d_begin_path(9);\
             _lumen_canvas2d_move_to(9, 0, 0);\
             _lumen_canvas2d_line_to(9, 20, 0);\
             _lumen_canvas2d_line_to(9, 20, 20);\
             _lumen_canvas2d_close_path(9);\
             _lumen_canvas2d_fill(9);",
        )
        .unwrap();
        assert_eq!(rt.flush_canvas_updates().len(), 1);
    }

    #[test]
    fn js_stroke_marks_dirty_without_path_ops() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(10, 8, 8);\
             _lumen_canvas2d_begin_path(10);\
             _lumen_canvas2d_move_to(10, 0, 0);\
             _lumen_canvas2d_line_to(10, 8, 8);\
             _lumen_canvas2d_stroke(10);",
        )
        .unwrap();
        assert_eq!(rt.flush_canvas_updates().len(), 1);
    }

    #[test]
    fn js_arc_does_not_mark_dirty_until_fill() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(11, 20, 20);\
             _lumen_canvas2d_begin_path(11);\
             _lumen_canvas2d_arc(11, 10, 10, 5, 0, 6.28, false);",
        )
        .unwrap();
        assert!(rt.flush_canvas_updates().is_empty(), "path building alone is not dirty");
    }

    #[test]
    fn js_line_width_rejects_invalid() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(12, 4, 4);\
             _lumen_canvas2d_set_line_width(12, 3.5);\
             _lumen_canvas2d_set_line_width(12, -1);\
             _lumen_canvas2d_set_line_width(12, 0);",
        )
        .unwrap();
        let line_width = rt.run_for_test(|| super::with_canvas(12, |c| c.line_width));
        assert_eq!(line_width, 3.5_f32, "invalid widths ignored");
    }

    #[test]
    fn js_global_alpha_clamped_to_unit_range() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(13, 4, 4);\
             _lumen_canvas2d_set_global_alpha(13, 0.5);\
             _lumen_canvas2d_set_global_alpha(13, 2.0);\
             _lumen_canvas2d_set_global_alpha(13, -0.5);",
        )
        .unwrap();
        let alpha = rt.run_for_test(|| super::with_canvas(13, |c| c.global_alpha));
        assert_eq!(alpha, 0.5_f32, "out-of-range ignored");
    }

    #[test]
    fn js_resize_clears_and_marks_dirty() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(14, 4, 4);\
             _lumen_canvas2d_resize(14, 16, 8);",
        )
        .unwrap();
        let (w, h) = rt.run_for_test(|| super::with_canvas(14, |c| (c.width(), c.height())));
        assert_eq!((w, h), (16, 8));
        assert_eq!(rt.flush_canvas_updates().len(), 1);
    }

    #[test]
    fn js_get_image_data_returns_only_the_requested_rect() {
        // BUG-448: the native used to take a bare `nid` and answer with the
        // whole bitmap, so every rectangle read the origin. The rect is a
        // parameter now, and its size decides the payload's size.
        let rt = with_canvas2d();
        rt.eval("_lumen_canvas2d_create(15, 4, 2);").unwrap();
        assert_eq!(
            bytes_eval(&rt, "_lumen_canvas2d_get_image_data(15, 0, 0, 4, 2)").len(),
            4 * 2 * 4,
            "whole canvas is 4x2 RGBA"
        );
        assert_eq!(
            bytes_eval(&rt, "_lumen_canvas2d_get_image_data(15, 1, 1, 1, 1)").len(),
            4,
            "a one-pixel read costs one pixel"
        );
    }

    #[test]
    fn js_get_image_data_reads_the_addressed_pixel() {
        // The bug's own repro: three non-overlapping stripes, each read at its
        // own x. Before the fix all three answered with pixel (0, 0).
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(16, 6, 1);\
             _lumen_canvas2d_set_fill_style(16, '#ff0000');\
             _lumen_canvas2d_fill_rect(16, 0, 0, 2, 1);\
             _lumen_canvas2d_set_fill_style(16, '#00ff00');\
             _lumen_canvas2d_fill_rect(16, 2, 0, 2, 1);\
             _lumen_canvas2d_set_fill_style(16, '#0000ff');\
             _lumen_canvas2d_fill_rect(16, 4, 0, 2, 1);",
        )
        .unwrap();
        assert_eq!(
            bytes_eval(&rt, "_lumen_canvas2d_get_image_data(16, 0, 0, 1, 1)"),
            vec![255, 0, 0, 255]
        );
        assert_eq!(
            bytes_eval(&rt, "_lumen_canvas2d_get_image_data(16, 2, 0, 1, 1)"),
            vec![0, 255, 0, 255]
        );
        assert_eq!(
            bytes_eval(&rt, "_lumen_canvas2d_get_image_data(16, 4, 0, 1, 1)"),
            vec![0, 0, 255, 255]
        );
    }

    #[test]
    fn js_get_image_data_outside_the_canvas_is_transparent_black() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(17, 2, 2);\
             _lumen_canvas2d_set_fill_style(17, '#ff0000');\
             _lumen_canvas2d_fill_rect(17, 0, 0, 2, 2);",
        )
        .unwrap();
        assert_eq!(
            bytes_eval(&rt, "_lumen_canvas2d_get_image_data(17, 5, 5, 1, 1)"),
            vec![0, 0, 0, 0],
            "wholly outside"
        );
        // Straddling the right edge: first pixel is real, second is outside.
        assert_eq!(
            bytes_eval(&rt, "_lumen_canvas2d_get_image_data(17, 1, 0, 2, 1)"),
            vec![255, 0, 0, 255, 0, 0, 0, 0]
        );
        // A negative origin is legal and pads on the near side.
        assert_eq!(
            bytes_eval(&rt, "_lumen_canvas2d_get_image_data(17, -1, 0, 2, 1)"),
            vec![0, 0, 0, 0, 255, 0, 0, 255]
        );
    }

    #[test]
    fn js_get_image_data_unknown_canvas_is_empty() {
        let rt = with_canvas2d();
        assert!(bytes_eval(&rt, "_lumen_canvas2d_get_image_data(999, 0, 0, 1, 1)").is_empty());
    }

    #[test]
    fn js_ops_on_unknown_canvas_are_noops() {
        let rt = with_canvas2d();
        // No create() — every op should silently no-op, no panic.
        rt.eval(
            "_lumen_canvas2d_fill_rect(404, 0, 0, 4, 4);\
             _lumen_canvas2d_set_fill_style(404, '#fff');\
             _lumen_canvas2d_fill(404);",
        )
        .unwrap();
        // fill_rect/fill mark dirty, but flush finds no context → empty.
        assert!(rt.flush_canvas_updates().is_empty());
    }

    #[test]
    fn js_two_canvases_isolated() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(20, 4, 4);\
             _lumen_canvas2d_create(21, 8, 8);\
             _lumen_canvas2d_set_fill_style(20, '#ff0000');\
             _lumen_canvas2d_fill_rect(20, 0, 0, 4, 4);",
        )
        .unwrap();
        let updates = rt.flush_canvas_updates();
        // Only canvas 20 was drawn; 21 stays clean.
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, 20);
    }

    #[test]
    fn js_fill_text_marks_canvas_dirty() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(30, 200, 50);\
             _lumen_canvas2d_fill_text(30, 'Hi', 10.0, 30.0);",
        )
        .unwrap();
        let updates = rt.flush_canvas_updates();
        assert_eq!(updates.len(), 1, "fillText should mark canvas dirty");
        assert_eq!(updates[0].0, 30);
    }

    #[test]
    fn js_fill_text_rasterizes_non_transparent_pixels() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(31, 200, 60);\
             _lumen_canvas2d_set_font(31, '20px sans-serif');\
             _lumen_canvas2d_set_fill_style(31, '#000000');\
             _lumen_canvas2d_fill_text(31, 'X', 10.0, 40.0);",
        )
        .unwrap();
        let updates = rt.flush_canvas_updates();
        assert!(!updates.is_empty(), "should produce a dirty buffer");
        let any_inked = updates[0].3.chunks(4).any(|px| px[3] > 0);
        assert!(any_inked, "fillText('X') should produce non-transparent pixels");
    }

    #[test]
    fn js_set_text_align_stored_in_canvas_state() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(32, 100, 50);\
             _lumen_canvas2d_set_text_align(32, 'center');",
        )
        .unwrap();
        let align = rt.run_for_test(|| super::with_canvas(32, |c| c.text_align.clone()));
        assert_eq!(align, "center");
    }

    #[test]
    fn js_set_text_baseline_stored_in_canvas_state() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(33, 100, 50);\
             _lumen_canvas2d_set_text_baseline(33, 'top');",
        )
        .unwrap();
        let baseline = rt.run_for_test(|| super::with_canvas(33, |c| c.text_baseline.clone()));
        assert_eq!(baseline, "top");
    }

    #[test]
    fn js_measure_text_via_binding_uses_font_size() {
        let rt = with_canvas2d();
        rt.eval("_lumen_canvas2d_create(34, 200, 50);").unwrap();
        // 10px (default)
        let w10 = num_eval(&rt, "_lumen_canvas2d_measure_text(34, 'A');");
        // 20px
        rt.eval("_lumen_canvas2d_set_font(34, '20px sans-serif');").unwrap();
        let w20 = num_eval(&rt, "_lumen_canvas2d_measure_text(34, 'A');");
        assert!(w10 > 0.0, "10px width should be positive");
        assert!(w20 > w10 * 1.5, "20px should be roughly 2× 10px: {w20} vs {w10}");
    }

    #[test]
    fn js_stroke_text_marks_canvas_dirty() {
        let rt = with_canvas2d();
        rt.eval(
            "_lumen_canvas2d_create(35, 200, 50);\
             _lumen_canvas2d_stroke_text(35, 'T', 10.0, 30.0);",
        )
        .unwrap();
        let updates = rt.flush_canvas_updates();
        assert_eq!(updates.len(), 1, "strokeText should mark canvas dirty");
    }

    #[test]
    fn js_transfer_control_creates_offscreen_canvas_id() {
        let rt = with_canvas2d();
        rt.eval("_lumen_canvas2d_create(50, 40, 30);").unwrap();
        let raw = str_eval(&rt, "_lumen_canvas_transfer_control_to_offscreen(50)");
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(parsed["__canvas_id__"].as_u64().unwrap() > 0, "should have a canvas ID");
        assert_eq!(parsed["width"].as_u64().unwrap(), 40);
        assert_eq!(parsed["height"].as_u64().unwrap(), 30);
    }

    #[test]
    fn js_transfer_control_marks_as_transferred() {
        let rt = with_canvas2d();
        rt.eval("_lumen_canvas2d_create(51, 10, 10);").unwrap();
        let before = bool_eval(&rt, "_lumen_canvas_is_transferred(51)");
        assert!(!before, "not transferred yet");
        rt.eval("_lumen_canvas_transfer_control_to_offscreen(51);").unwrap();
        let after = bool_eval(&rt, "_lumen_canvas_is_transferred(51)");
        assert!(after, "should be marked as transferred");
    }

    #[test]
    fn js_is_transferred_false_for_unknown_nid() {
        let rt = with_canvas2d();
        assert!(!bool_eval(&rt, "_lumen_canvas_is_transferred(999999)"));
    }

    #[test]
    fn js_present_rgba_resizes_existing_canvas() {
        let rt = with_canvas2d();
        rt.eval("_lumen_canvas2d_create(78, 4, 4);").unwrap();
        let _ = rt.flush_canvas_updates();
        // A present at a different size resizes the backing buffer to match the frame.
        let frame = [1u8, 2, 3, 4, 5, 6, 7, 8];
        rt.run_for_test(move || super::present_rgba(78, 2, 1, &frame));
        let updates = rt.flush_canvas_updates();
        assert_eq!(updates.len(), 1);
        let (_, w, h, pixels) = &updates[0];
        assert_eq!((*w, *h), (2, 1), "canvas resized to the presented frame");
        assert_eq!(pixels, &frame);
    }
}

