//! HTML Canvas 2D JS bindings (HTML Living Standard §4.12.4).
//!
//! Wires `canvas.getContext('2d')` to the CPU-rasterized [`lumen_canvas::Context2D`].
//! Drawing operations: `fillRect`, `clearRect`, `strokeRect`, `beginPath`,
//! `moveTo`, `lineTo`, `closePath`, `arc`, `ellipse`, `arcTo`, `rect`,
//! `bezierCurveTo`, `quadraticCurveTo`, `fill`, `stroke`, `save`, `restore`.
//! Transforms: `translate`, `rotate`, `scale`, `transform`, `setTransform`, `resetTransform`.
//! Properties: `fillStyle`, `strokeStyle`, `lineWidth`, `globalAlpha`,
//! `globalCompositeOperation`, `lineCap`, `lineJoin`, `miterLimit`.
//!
//! Each `<canvas>` is keyed by its DOM node index (`__nid__` on the JS side,
//! `LayoutBox::node.index()` on the layout side). The display list emits a
//! `DrawImage` with `src = "canvas:{nid}"`; the shell uploads the dirty pixel
//! buffer to the renderer under the same key.
//!
//! After any draw operation the canvas is marked "dirty". The shell drains
//! dirty buffers via [`flush_dirty`] each frame and uploads them to the GPU.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use lumen_canvas::{
    CanvasColor, CanvasGradient, CanvasPattern, PaintSource, RepeatMode,
    CompositeOperation, LineCap, LineJoin, Context2D,
};
use rquickjs::Ctx;

thread_local! {
    /// Per-thread registry of live 2D contexts, keyed by DOM node index.
    static CANVASES: RefCell<HashMap<u32, Context2D>> = RefCell::new(HashMap::new());
    /// Node indices whose pixel buffer changed since the last [`flush_dirty`].
    static DIRTY: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    /// In-flight gradients awaiting `setFillStyle`/`setStrokeStyle`, keyed by object ID.
    static GRADIENTS: RefCell<HashMap<u32, CanvasGradient>> = RefCell::new(HashMap::new());
    /// In-flight patterns, keyed by object ID.
    static PATTERNS: RefCell<HashMap<u32, CanvasPattern>> = RefCell::new(HashMap::new());
    /// Auto-increment for gradient/pattern object IDs.
    static NEXT_PAINT_ID: Cell<u32> = const { Cell::new(1) };
}

/// Allocate a new unique object ID for a gradient or pattern.
fn next_paint_id() -> u32 {
    NEXT_PAINT_ID.with(|c| {
        let id = c.get();
        c.set(id.wrapping_add(1).max(1));
        id
    })
}

/// Decode a hex string (`"ff00aa"`) into bytes. Silently ignores odd-length or bad chars.
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

/// Run `f` against the context for `nid`, returning `R::default()` if absent.
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

/// Register the `_lumen_canvas2d_*` native functions on `globals`.
///
/// The JS-side `getContext('2d')` shim lives in `dom.rs::_lumen_make_element`,
/// which calls these natives keyed by the element's `__nid__`.
pub fn install_canvas2d_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    let g = ctx.globals();

    // _lumen_canvas2d_create(nid, w, h) — idempotent: re-creating resets the buffer.
    g.set(
        "_lumen_canvas2d_create",
        rquickjs::Function::new(ctx.clone(), |nid: u32, w: u32, h: u32| {
            let w = w.clamp(1, MAX_CANVAS_DIM);
            let h = h.clamp(1, MAX_CANVAS_DIM);
            CANVASES.with(|c| {
                if let Ok(mut map) = c.try_borrow_mut() {
                    map.entry(nid).or_insert_with(|| Context2D::new(w, h));
                }
            });
        }),
    )?;

    // _lumen_canvas2d_resize(nid, w, h) — HTML LS: resizing clears the bitmap.
    g.set(
        "_lumen_canvas2d_resize",
        rquickjs::Function::new(ctx.clone(), |nid: u32, w: u32, h: u32| {
            let w = w.clamp(1, MAX_CANVAS_DIM);
            let h = h.clamp(1, MAX_CANVAS_DIM);
            with_canvas(nid, |c| c.resize(w, h));
            mark_dirty(nid);
        }),
    )?;

    // ── Rectangles ──────────────────────────────────────────────────────────
    g.set(
        "_lumen_canvas2d_fill_rect",
        rquickjs::Function::new(ctx.clone(), |nid: u32, x: f64, y: f64, w: f64, h: f64| {
            with_canvas(nid, |c| c.fill_rect(x as f32, y as f32, w as f32, h as f32));
            mark_dirty(nid);
        }),
    )?;
    g.set(
        "_lumen_canvas2d_clear_rect",
        rquickjs::Function::new(ctx.clone(), |nid: u32, x: f64, y: f64, w: f64, h: f64| {
            with_canvas(nid, |c| c.clear_rect(x as f32, y as f32, w as f32, h as f32));
            mark_dirty(nid);
        }),
    )?;
    g.set(
        "_lumen_canvas2d_stroke_rect",
        rquickjs::Function::new(ctx.clone(), |nid: u32, x: f64, y: f64, w: f64, h: f64| {
            with_canvas(nid, |c| c.stroke_rect(x as f32, y as f32, w as f32, h as f32));
            mark_dirty(nid);
        }),
    )?;

    // ── Paths ───────────────────────────────────────────────────────────────
    g.set(
        "_lumen_canvas2d_begin_path",
        rquickjs::Function::new(ctx.clone(), |nid: u32| {
            with_canvas(nid, |c| c.begin_path());
        }),
    )?;
    g.set(
        "_lumen_canvas2d_move_to",
        rquickjs::Function::new(ctx.clone(), |nid: u32, x: f64, y: f64| {
            with_canvas(nid, |c| c.move_to(x as f32, y as f32));
        }),
    )?;
    g.set(
        "_lumen_canvas2d_line_to",
        rquickjs::Function::new(ctx.clone(), |nid: u32, x: f64, y: f64| {
            with_canvas(nid, |c| c.line_to(x as f32, y as f32));
        }),
    )?;
    g.set(
        "_lumen_canvas2d_close_path",
        rquickjs::Function::new(ctx.clone(), |nid: u32| {
            with_canvas(nid, |c| c.close_path());
        }),
    )?;
    g.set(
        "_lumen_canvas2d_arc",
        rquickjs::Function::new(
            ctx.clone(),
            |nid: u32, cx: f64, cy: f64, r: f64, sa: f64, ea: f64, ccw: bool| {
                with_canvas(nid, |c| {
                    c.arc(cx as f32, cy as f32, r as f32, sa as f32, ea as f32, ccw)
                });
            },
        ),
    )?;
    g.set(
        "_lumen_canvas2d_fill",
        rquickjs::Function::new(ctx.clone(), |nid: u32| {
            with_canvas(nid, |c| c.fill());
            mark_dirty(nid);
        }),
    )?;
    g.set(
        "_lumen_canvas2d_stroke",
        rquickjs::Function::new(ctx.clone(), |nid: u32| {
            with_canvas(nid, |c| c.stroke());
            mark_dirty(nid);
        }),
    )?;

    // ── Style setters ─────────────────────────────────────────────────────────
    g.set(
        "_lumen_canvas2d_set_fill_style",
        rquickjs::Function::new(ctx.clone(), |nid: u32, css: String| {
            with_canvas(nid, |c| {
                if let Some(color) = CanvasColor::from_css_str(&css) {
                    c.fill_style = PaintSource::Color(color);
                }
            });
        }),
    )?;
    g.set(
        "_lumen_canvas2d_set_stroke_style",
        rquickjs::Function::new(ctx.clone(), |nid: u32, css: String| {
            with_canvas(nid, |c| {
                if let Some(color) = CanvasColor::from_css_str(&css) {
                    c.stroke_style = PaintSource::Color(color);
                }
            });
        }),
    )?;
    g.set(
        "_lumen_canvas2d_set_line_width",
        rquickjs::Function::new(ctx.clone(), |nid: u32, w: f64| {
            // HTML LS §4.12.4: ignore zero/negative/non-finite values.
            if w.is_finite() && w > 0.0 {
                with_canvas(nid, |c| c.line_width = w as f32);
            }
        }),
    )?;
    g.set(
        "_lumen_canvas2d_set_global_alpha",
        rquickjs::Function::new(ctx.clone(), |nid: u32, a: f64| {
            // HTML LS §4.12.4: ignore values outside [0, 1] or non-finite.
            if a.is_finite() && (0.0..=1.0).contains(&a) {
                with_canvas(nid, |c| c.global_alpha = a as f32);
            }
        }),
    )?;

    // ── State stack ───────────────────────────────────────────────────────────
    g.set(
        "_lumen_canvas2d_save",
        rquickjs::Function::new(ctx.clone(), |nid: u32| {
            with_canvas(nid, |c| c.save());
        }),
    )?;
    g.set(
        "_lumen_canvas2d_restore",
        rquickjs::Function::new(ctx.clone(), |nid: u32| {
            with_canvas(nid, |c| c.restore());
        }),
    )?;

    // ── Transforms ────────────────────────────────────────────────────────────
    g.set(
        "_lumen_canvas2d_translate",
        rquickjs::Function::new(ctx.clone(), |nid: u32, tx: f64, ty: f64| {
            with_canvas(nid, |c| c.translate(tx as f32, ty as f32));
        }),
    )?;
    g.set(
        "_lumen_canvas2d_rotate",
        rquickjs::Function::new(ctx.clone(), |nid: u32, angle: f64| {
            with_canvas(nid, |c| c.rotate(angle as f32));
        }),
    )?;
    g.set(
        "_lumen_canvas2d_scale",
        rquickjs::Function::new(ctx.clone(), |nid: u32, sx: f64, sy: f64| {
            with_canvas(nid, |c| c.scale(sx as f32, sy as f32));
        }),
    )?;
    g.set(
        "_lumen_canvas2d_transform",
        rquickjs::Function::new(
            ctx.clone(),
            |nid: u32, a: f64, b: f64, c2: f64, d: f64, e: f64, f2: f64| {
                with_canvas(nid, |c| {
                    c.transform(a as f32, b as f32, c2 as f32, d as f32, e as f32, f2 as f32);
                });
            },
        ),
    )?;
    g.set(
        "_lumen_canvas2d_set_transform",
        rquickjs::Function::new(
            ctx.clone(),
            |nid: u32, a: f64, b: f64, c2: f64, d: f64, e: f64, f2: f64| {
                with_canvas(nid, |c| {
                    c.set_transform(a as f32, b as f32, c2 as f32, d as f32, e as f32, f2 as f32);
                });
            },
        ),
    )?;
    g.set(
        "_lumen_canvas2d_reset_transform",
        rquickjs::Function::new(ctx.clone(), |nid: u32| {
            with_canvas(nid, |c| c.reset_transform());
        }),
    )?;

    // ── Bézier curves and additional path operations ───────────────────────────
    g.set(
        "_lumen_canvas2d_bezier_curve_to",
        rquickjs::Function::new(
            ctx.clone(),
            |nid: u32, cp1x: f64, cp1y: f64, cp2x: f64, cp2y: f64, x: f64, y: f64| {
                with_canvas(nid, |c| {
                    c.bezier_curve_to(
                        cp1x as f32, cp1y as f32,
                        cp2x as f32, cp2y as f32,
                        x as f32, y as f32,
                    );
                });
            },
        ),
    )?;
    g.set(
        "_lumen_canvas2d_quadratic_curve_to",
        rquickjs::Function::new(
            ctx.clone(),
            |nid: u32, cpx: f64, cpy: f64, x: f64, y: f64| {
                with_canvas(nid, |c| {
                    c.quadratic_curve_to(cpx as f32, cpy as f32, x as f32, y as f32);
                });
            },
        ),
    )?;
    // Note: `ellipse` is implemented in the JS shim via save/translate/scale/rotate/arc/restore
    // because rquickjs supports max 7 closure params and ellipse needs 8 (cx,cy,rx,ry,rot,sa,ea,ccw).
    g.set(
        "_lumen_canvas2d_arc_to",
        rquickjs::Function::new(
            ctx.clone(),
            |nid: u32, x1: f64, y1: f64, x2: f64, y2: f64, r: f64| {
                with_canvas(nid, |c| {
                    c.arc_to(x1 as f32, y1 as f32, x2 as f32, y2 as f32, r as f32);
                });
            },
        ),
    )?;
    g.set(
        "_lumen_canvas2d_rect",
        rquickjs::Function::new(ctx.clone(), |nid: u32, x: f64, y: f64, w: f64, h: f64| {
            with_canvas(nid, |c| c.rect(x as f32, y as f32, w as f32, h as f32));
        }),
    )?;

    // ── Additional property setters ───────────────────────────────────────────
    g.set(
        "_lumen_canvas2d_set_global_composite_operation",
        rquickjs::Function::new(ctx.clone(), |nid: u32, op: String| {
            if let Some(op) = CompositeOperation::from_str(&op) {
                with_canvas(nid, |c| c.composite_operation = op);
            }
        }),
    )?;
    g.set(
        "_lumen_canvas2d_set_line_cap",
        rquickjs::Function::new(ctx.clone(), |nid: u32, cap: String| {
            if let Some(cap) = LineCap::from_str(&cap) {
                with_canvas(nid, |c| c.line_cap = cap);
            }
        }),
    )?;
    g.set(
        "_lumen_canvas2d_set_line_join",
        rquickjs::Function::new(ctx.clone(), |nid: u32, join: String| {
            if let Some(join) = LineJoin::from_str(&join) {
                with_canvas(nid, |c| c.line_join = join);
            }
        }),
    )?;
    g.set(
        "_lumen_canvas2d_set_miter_limit",
        rquickjs::Function::new(ctx.clone(), |nid: u32, limit: f64| {
            // HTML LS §4.12.4: ignore zero/negative/non-finite values.
            if limit.is_finite() && limit > 0.0 {
                with_canvas(nid, |c| c.miter_limit = limit as f32);
            }
        }),
    )?;

    // ── Phase 3: Gradients ────────────────────────────────────────────────────

    // _lumen_canvas2d_create_linear_gradient(nid, x0, y0, x1, y1) -> grad_id
    g.set(
        "_lumen_canvas2d_create_linear_gradient",
        rquickjs::Function::new(ctx.clone(), |_nid: u32, x0: f64, y0: f64, x1: f64, y1: f64| -> u32 {
            let id = next_paint_id();
            GRADIENTS.with(|gs| {
                if let Ok(mut map) = gs.try_borrow_mut() {
                    map.insert(id, CanvasGradient::linear(x0 as f32, y0 as f32, x1 as f32, y1 as f32));
                }
            });
            id
        }),
    )?;

    // _lumen_canvas2d_create_radial_gradient(nid, x0, y0, r0, x1, y1, r1) -> grad_id
    g.set(
        "_lumen_canvas2d_create_radial_gradient",
        rquickjs::Function::new(
            ctx.clone(),
            |_nid: u32, x0: f64, y0: f64, r0: f64, x1: f64, y1: f64, r1: f64| -> u32 {
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
            },
        ),
    )?;

    // _lumen_canvas2d_create_conic_gradient(nid, angle, cx, cy) -> grad_id
    g.set(
        "_lumen_canvas2d_create_conic_gradient",
        rquickjs::Function::new(ctx.clone(), |_nid: u32, angle: f64, cx: f64, cy: f64| -> u32 {
            let id = next_paint_id();
            GRADIENTS.with(|gs| {
                if let Ok(mut map) = gs.try_borrow_mut() {
                    map.insert(id, CanvasGradient::conic(angle as f32, cx as f32, cy as f32));
                }
            });
            id
        }),
    )?;

    // _lumen_canvas2d_gradient_add_color_stop(grad_id, offset, css_color)
    g.set(
        "_lumen_canvas2d_gradient_add_color_stop",
        rquickjs::Function::new(ctx.clone(), |grad_id: u32, offset: f64, css: String| {
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

    // _lumen_canvas2d_set_fill_style_gradient(nid, grad_id) — clones gradient into fill_style
    g.set(
        "_lumen_canvas2d_set_fill_style_gradient",
        rquickjs::Function::new(ctx.clone(), |nid: u32, grad_id: u32| {
            let grad = GRADIENTS.with(|gs| {
                gs.try_borrow().ok()?.get(&grad_id).cloned()
            });
            if let Some(g) = grad {
                with_canvas(nid, |c| c.fill_style = PaintSource::Gradient(g));
            }
        }),
    )?;

    // _lumen_canvas2d_set_stroke_style_gradient(nid, grad_id)
    g.set(
        "_lumen_canvas2d_set_stroke_style_gradient",
        rquickjs::Function::new(ctx.clone(), |nid: u32, grad_id: u32| {
            let grad = GRADIENTS.with(|gs| {
                gs.try_borrow().ok()?.get(&grad_id).cloned()
            });
            if let Some(g) = grad {
                with_canvas(nid, |c| c.stroke_style = PaintSource::Gradient(g));
            }
        }),
    )?;

    // ── Phase 3: Patterns ─────────────────────────────────────────────────────

    // _lumen_canvas2d_create_pattern(src_nid, repeat_mode) -> pat_id
    // repeat_mode: "repeat"|"repeat-x"|"repeat-y"|"no-repeat"
    g.set(
        "_lumen_canvas2d_create_pattern",
        rquickjs::Function::new(ctx.clone(), |src_nid: u32, repeat_str: String| -> u32 {
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

    // _lumen_canvas2d_set_fill_style_pattern(nid, pat_id)
    g.set(
        "_lumen_canvas2d_set_fill_style_pattern",
        rquickjs::Function::new(ctx.clone(), |nid: u32, pat_id: u32| {
            let pat = PATTERNS.with(|ps| {
                ps.try_borrow().ok()?.get(&pat_id).cloned()
            });
            if let Some(p) = pat {
                with_canvas(nid, |c| c.fill_style = PaintSource::Pattern(p));
            }
        }),
    )?;

    // _lumen_canvas2d_set_stroke_style_pattern(nid, pat_id)
    g.set(
        "_lumen_canvas2d_set_stroke_style_pattern",
        rquickjs::Function::new(ctx.clone(), |nid: u32, pat_id: u32| {
            let pat = PATTERNS.with(|ps| {
                ps.try_borrow().ok()?.get(&pat_id).cloned()
            });
            if let Some(p) = pat {
                with_canvas(nid, |c| c.stroke_style = PaintSource::Pattern(p));
            }
        }),
    )?;

    // ── Phase 3: Shadow ───────────────────────────────────────────────────────

    g.set(
        "_lumen_canvas2d_set_shadow_color",
        rquickjs::Function::new(ctx.clone(), |nid: u32, css: String| {
            with_canvas(nid, |c| {
                if let Some(color) = CanvasColor::from_css_str(&css) {
                    c.shadow_color = color;
                }
            });
        }),
    )?;
    g.set(
        "_lumen_canvas2d_set_shadow_blur",
        rquickjs::Function::new(ctx.clone(), |nid: u32, v: f64| {
            if v.is_finite() && v >= 0.0 {
                with_canvas(nid, |c| c.shadow_blur = v as f32);
            }
        }),
    )?;
    g.set(
        "_lumen_canvas2d_set_shadow_offset_x",
        rquickjs::Function::new(ctx.clone(), |nid: u32, v: f64| {
            if v.is_finite() {
                with_canvas(nid, |c| c.shadow_offset_x = v as f32);
            }
        }),
    )?;
    g.set(
        "_lumen_canvas2d_set_shadow_offset_y",
        rquickjs::Function::new(ctx.clone(), |nid: u32, v: f64| {
            if v.is_finite() {
                with_canvas(nid, |c| c.shadow_offset_y = v as f32);
            }
        }),
    )?;

    // ── Phase 3: Clip ─────────────────────────────────────────────────────────

    g.set(
        "_lumen_canvas2d_clip",
        rquickjs::Function::new(ctx.clone(), |nid: u32| {
            with_canvas(nid, |c| c.clip());
        }),
    )?;

    // ── Phase 3: drawImage ────────────────────────────────────────────────────

    // _lumen_canvas2d_draw_image(dst_nid, src_nid, dx, dy, dw, dh)
    // Blits another canvas's pixels onto this canvas with scaling.
    g.set(
        "_lumen_canvas2d_draw_image",
        rquickjs::Function::new(
            ctx.clone(),
            |dst_nid: u32, src_nid: u32, dx: f64, dy: f64, dw: f64, dh: f64| {
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
            },
        ),
    )?;

    // ── Phase 3: ImageData ────────────────────────────────────────────────────

    // _lumen_canvas2d_put_image_data(nid, hex_data, sw, sh, dx, dy)
    g.set(
        "_lumen_canvas2d_put_image_data",
        rquickjs::Function::new(
            ctx.clone(),
            |nid: u32, hex: String, sw: u32, sh: u32, dx: i32, dy: i32| {
                let data = decode_hex(&hex);
                with_canvas(nid, |c| c.put_image_data(&data, sw, sh, dx, dy));
                mark_dirty(nid);
            },
        ),
    )?;

    // _lumen_canvas2d_create_image_data(sw, sh) -> hex string
    g.set(
        "_lumen_canvas2d_create_image_data",
        rquickjs::Function::new(ctx.clone(), |sw: u32, sh: u32| -> String {
            let data = Context2D::create_image_data(sw, sh);
            let mut s = String::with_capacity(data.len() * 2);
            use std::fmt::Write;
            for b in &data {
                let _ = write!(s, "{b:02x}");
            }
            s
        }),
    )?;

    // ── Phase 3: Text / Font ──────────────────────────────────────────────────

    // _lumen_canvas2d_set_font(nid, css_font) — stores font string for later use
    g.set(
        "_lumen_canvas2d_set_font",
        rquickjs::Function::new(ctx.clone(), |nid: u32, font: String| {
            with_canvas(nid, |c| c.font = font);
        }),
    )?;

    // _lumen_canvas2d_fill_text(nid, text, x, y) — Phase 3 stub: no glyph rendering yet.
    // Full text layout + rasterization is deferred to Phase 4 (needs lumen-font integration).
    g.set(
        "_lumen_canvas2d_fill_text",
        rquickjs::Function::new(ctx.clone(), |_nid: u32, _text: String, _x: f64, _y: f64| {
            // Stub: text rendering wired in Phase 4.
        }),
    )?;

    // _lumen_canvas2d_measure_text(nid, text) -> approximate width in pixels
    // Phase 3 stub: estimates 0.6× pixel_size per character (rough sans-serif approximation).
    g.set(
        "_lumen_canvas2d_measure_text",
        rquickjs::Function::new(ctx.clone(), |_nid: u32, text: String| -> f64 {
            // Very rough approximation until Phase 4 wires lumen-font.
            text.len() as f64 * 6.0
        }),
    )?;

    // _lumen_canvas2d_get_image_data(nid) -> "{w},{h},{hex_rgba}"
    // Applies per-session fingerprint noise via Context2D::get_image_data().
    g.set(
        "_lumen_canvas2d_get_image_data",
        rquickjs::Function::new(ctx.clone(), |nid: u32| -> String {
            CANVASES.with(|c| {
                let Ok(map) = c.try_borrow() else {
                    return String::new();
                };
                let Some(ctx) = map.get(&nid) else {
                    return String::new();
                };
                let pixels = ctx.get_image_data();
                let mut s = String::with_capacity(pixels.len() * 2 + 12);
                use std::fmt::Write;
                let _ = write!(s, "{},{},", ctx.width(), ctx.height());
                for b in &pixels {
                    let _ = write!(s, "{b:02x}");
                }
                s
            })
        }),
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    /// Clear thread-local state so tests don't leak into each other.
    fn reset_state() {
        CANVASES.with(|c| c.borrow_mut().clear());
        DIRTY.with(|d| d.borrow_mut().clear());
    }

    #[test]
    fn create_registers_context() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            ctx.eval::<(), _>("_lumen_canvas2d_create(7, 100, 50);").unwrap();
            CANVASES.with(|c| {
                let map = c.borrow();
                let g = map.get(&7).expect("context registered");
                assert_eq!(g.width(), 100);
                assert_eq!(g.height(), 50);
            });
        });
    }

    #[test]
    fn create_clamps_dimensions() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            ctx.eval::<(), _>("_lumen_canvas2d_create(1, 0, 99999);").unwrap();
            CANVASES.with(|c| {
                let map = c.borrow();
                let g = map.get(&1).unwrap();
                assert_eq!(g.width(), 1, "zero clamped up to 1");
                assert_eq!(g.height(), MAX_CANVAS_DIM, "oversized clamped to max");
            });
        });
    }

    #[test]
    fn create_is_idempotent_preserving_buffer() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            ctx.eval::<(), _>(
                "_lumen_canvas2d_create(3, 10, 10);\
                 _lumen_canvas2d_set_fill_style(3, '#ff0000');\
                 _lumen_canvas2d_fill_rect(3, 0, 0, 10, 10);\
                 _lumen_canvas2d_create(3, 10, 10);",
            )
            .unwrap();
            CANVASES.with(|c| {
                let map = c.borrow();
                let g = map.get(&3).unwrap();
                // Re-create must not wipe an existing buffer (entry().or_insert).
                assert_eq!(g.pixels()[0], 255, "red preserved across re-create");
            });
        });
    }

    #[test]
    fn fill_rect_marks_dirty_and_paints() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            ctx.eval::<(), _>(
                "_lumen_canvas2d_create(2, 4, 4);\
                 _lumen_canvas2d_set_fill_style(2, 'rgb(0,255,0)');\
                 _lumen_canvas2d_fill_rect(2, 0, 0, 4, 4);",
            )
            .unwrap();
            let updates = flush_dirty();
            assert_eq!(updates.len(), 1);
            let (nid, w, h, rgba) = &updates[0];
            assert_eq!(*nid, 2);
            assert_eq!((*w, *h), (4, 4));
            assert_eq!(rgba[1], 255, "green channel painted");
        });
    }

    #[test]
    fn flush_dirty_drains_once() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            ctx.eval::<(), _>(
                "_lumen_canvas2d_create(5, 4, 4);\
                 _lumen_canvas2d_fill_rect(5, 0, 0, 4, 4);",
            )
            .unwrap();
            assert_eq!(flush_dirty().len(), 1);
            assert!(flush_dirty().is_empty(), "second drain is empty");
        });
    }

    #[test]
    fn clear_rect_marks_dirty() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            ctx.eval::<(), _>(
                "_lumen_canvas2d_create(8, 4, 4);\
                 _lumen_canvas2d_set_fill_style(8, '#0000ff');\
                 _lumen_canvas2d_fill_rect(8, 0, 0, 4, 4);",
            )
            .unwrap();
            let _ = flush_dirty();
            ctx.eval::<(), _>("_lumen_canvas2d_clear_rect(8, 0, 0, 4, 4);").unwrap();
            let updates = flush_dirty();
            assert_eq!(updates.len(), 1);
            assert_eq!(updates[0].3[3], 0, "alpha cleared to transparent");
        });
    }

    #[test]
    fn path_fill_marks_dirty() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            ctx.eval::<(), _>(
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
            assert_eq!(flush_dirty().len(), 1);
        });
    }

    #[test]
    fn stroke_marks_dirty_without_path_ops() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            ctx.eval::<(), _>(
                "_lumen_canvas2d_create(10, 8, 8);\
                 _lumen_canvas2d_begin_path(10);\
                 _lumen_canvas2d_move_to(10, 0, 0);\
                 _lumen_canvas2d_line_to(10, 8, 8);\
                 _lumen_canvas2d_stroke(10);",
            )
            .unwrap();
            assert_eq!(flush_dirty().len(), 1);
        });
    }

    #[test]
    fn arc_does_not_mark_dirty_until_fill() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            ctx.eval::<(), _>(
                "_lumen_canvas2d_create(11, 20, 20);\
                 _lumen_canvas2d_begin_path(11);\
                 _lumen_canvas2d_arc(11, 10, 10, 5, 0, 6.28, false);",
            )
            .unwrap();
            assert!(flush_dirty().is_empty(), "path building alone is not dirty");
        });
    }

    #[test]
    fn line_width_rejects_invalid() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            ctx.eval::<(), _>(
                "_lumen_canvas2d_create(12, 4, 4);\
                 _lumen_canvas2d_set_line_width(12, 3.5);\
                 _lumen_canvas2d_set_line_width(12, -1);\
                 _lumen_canvas2d_set_line_width(12, 0);",
            )
            .unwrap();
            CANVASES.with(|c| {
                let map = c.borrow();
                assert_eq!(map.get(&12).unwrap().line_width, 3.5, "invalid widths ignored");
            });
        });
    }

    #[test]
    fn global_alpha_clamped_to_unit_range() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            ctx.eval::<(), _>(
                "_lumen_canvas2d_create(13, 4, 4);\
                 _lumen_canvas2d_set_global_alpha(13, 0.5);\
                 _lumen_canvas2d_set_global_alpha(13, 2.0);\
                 _lumen_canvas2d_set_global_alpha(13, -0.5);",
            )
            .unwrap();
            CANVASES.with(|c| {
                let map = c.borrow();
                assert_eq!(map.get(&13).unwrap().global_alpha, 0.5, "out-of-range ignored");
            });
        });
    }

    #[test]
    fn resize_clears_and_marks_dirty() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            ctx.eval::<(), _>(
                "_lumen_canvas2d_create(14, 4, 4);\
                 _lumen_canvas2d_resize(14, 16, 8);",
            )
            .unwrap();
            CANVASES.with(|c| {
                let map = c.borrow();
                let g = map.get(&14).unwrap();
                assert_eq!((g.width(), g.height()), (16, 8));
            });
            assert_eq!(flush_dirty().len(), 1);
        });
    }

    #[test]
    fn get_image_data_returns_dimensions_and_hex() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            ctx.eval::<(), _>("_lumen_canvas2d_create(15, 2, 2);").unwrap();
            let raw: String = ctx
                .eval("_lumen_canvas2d_get_image_data(15)")
                .unwrap();
            let parts: Vec<&str> = raw.splitn(3, ',').collect();
            assert_eq!(parts[0], "2");
            assert_eq!(parts[1], "2");
            // 2x2 RGBA = 16 bytes = 32 hex chars.
            assert_eq!(parts[2].len(), 32);
        });
    }

    #[test]
    fn get_image_data_unknown_canvas_is_empty() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            let raw: String = ctx
                .eval("_lumen_canvas2d_get_image_data(999)")
                .unwrap();
            assert!(raw.is_empty());
        });
    }

    #[test]
    fn ops_on_unknown_canvas_are_noops() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            // No create() — every op should silently no-op, no panic.
            ctx.eval::<(), _>(
                "_lumen_canvas2d_fill_rect(404, 0, 0, 4, 4);\
                 _lumen_canvas2d_set_fill_style(404, '#fff');\
                 _lumen_canvas2d_fill(404);",
            )
            .unwrap();
            // fill_rect/fill mark dirty, but flush finds no context → empty.
            assert!(flush_dirty().is_empty());
        });
    }

    #[test]
    fn two_canvases_isolated() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            reset_state();
            install_canvas2d_bindings(&ctx).unwrap();
            ctx.eval::<(), _>(
                "_lumen_canvas2d_create(20, 4, 4);\
                 _lumen_canvas2d_create(21, 8, 8);\
                 _lumen_canvas2d_set_fill_style(20, '#ff0000');\
                 _lumen_canvas2d_fill_rect(20, 0, 0, 4, 4);",
            )
            .unwrap();
            let updates = flush_dirty();
            // Only canvas 20 was drawn; 21 stays clean.
            assert_eq!(updates.len(), 1);
            assert_eq!(updates[0].0, 20);
        });
    }
}
