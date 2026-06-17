//! CPU-based rasterization using tiny-skia for deterministic pixel output on CI.
//!
//! Available only with feature="cpu-render"; no GPU dependencies, fully deterministic
//! across Windows/macOS/Linux.

use lumen_image::Image;
use lumen_layout::{BorderStyle, Color, FilterFn, GradientStop};
use crate::dash_math::{dashed_border_offsets, dotted_border_offsets};
use crate::gradient_math::{atan2_det, resolve_stop_positions, sample_gradient_color};
use crate::matrix_util::mat4_to_2d_affine;
use crate::{DisplayCommand, CornerRadii};
use crate::display_list::ResolvedClipShape;
use lumen_core::geom::Rect;

/// Bundled Inter Regular — the only face the deterministic CPU path can
/// rasterize. Mirrors `INTER_FONT` in `lumen-driver`; real font matching
/// (family/weight/style/fallback) is a GPU-renderer concern, so the CPU
/// snapshot path always renders text with this single face. Pure-Rust glyph
/// scanline fill (`lumen_font::Rasterizer`) keeps output cross-OS bit-identical.
const BUNDLED_FONT: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.ttf");

/// How a pushed off-screen layer is composited back onto the layer below when
/// its group closes (`PopOpacity` / `PopTransform`).
///
/// The two share the same full-size off-screen pixmap stack: the subtree draws
/// into a transparent layer in untransformed page coordinates, and the close op
/// decides *how* that layer merges down. Nesting (e.g. opacity wrapping a
/// transform — the emitter order is `PushOpacity → PushTransform → … →
/// PopTransform → PopOpacity`) is handled by successive composites, so each
/// layer carries only its own relative op, not an accumulated one.
enum LayerComposite {
    /// CSS Color L3 §3.2 group opacity — blend the whole layer with this alpha.
    Opacity(f32),
    /// CSS Transforms L1 §13 — map the layer through this 2D affine when
    /// compositing it down (`draw_pixmap` resamples). The transform is already
    /// in viewport/page coordinates (`forward_box_transform` bakes in
    /// `T(pivot)·M·T(-pivot)` from `transform-origin`), so a full-size layer at
    /// page coordinates composited through it lands exactly where the GPU
    /// vertex transform would place it.
    Transform(tiny_skia::Transform),
    /// CSS Compositing & Blending L1 §5 `mix-blend-mode` — composite the layer
    /// onto the backdrop below with this separable/non-separable blend formula
    /// instead of plain `SourceOver`. The subtree renders into a transparent
    /// full-size layer (page coordinates); `draw_pixmap` with this `blend_mode`
    /// then blends each layer pixel against the accumulated backdrop on the
    /// layer below — exactly the element-vs-backdrop blend CSS specifies.
    Blend(tiny_skia::BlendMode),
    /// CSS Filter Effects L1 §4 — `filter` chain (`PushFilter`/`PopFilter`,
    /// emitted for box-shadow blur, text-shadow blur and the element `filter`
    /// property). The subtree renders into a transparent full-size layer; on
    /// close the chain is applied pixel-wise to the layer (Gaussian blur +
    /// colour-matrix filters, left to right) and the filtered layer is
    /// composited onto the backdrop below with plain `SourceOver`.
    Filter(Vec<FilterFn>),
    /// CSS Masking L1 §4 — `mask-image` (`PushMask*`/`PopMask`). The element's
    /// fully-composited subtree renders into a transparent full-size layer; on
    /// close the mask is evaluated per-pixel (the gradient's alpha channel —
    /// mask-mode: alpha, mirroring the GPU `MASK_COMPOSITE_SHADER` `c.a * m.a`)
    /// and multiplied into the layer's alpha before it is composited down with
    /// plain `SourceOver`.
    Mask(MaskSpec),
    /// BUG-140 — `clip-path` basic-shape (`PushClipPath`/`PopClip`). The
    /// element's subtree renders into a transparent full-size layer (page
    /// coordinates, pre-transform — the command is emitted inside the
    /// element's `PushTransform`, so the enclosing transform layer carries
    /// the clipped result); on close the shape's anti-aliased coverage is
    /// rasterised (tiny-skia path fill — deterministic) and multiplied into
    /// the layer's alpha before plain `SourceOver` compositing.
    ClipShape(ResolvedClipShape),
}

/// Kind of clip opened by a `PushClip*` command, so the shared `PopClip`
/// closes rect clips (pop `clip_stack`) and shape clips (close the layer)
/// differently. Mirrors `ClipEntry` in the femtovg backend (BUG-140).
enum CpuClipKind {
    /// `PushClipRect` / `PushClipRoundedRect` — entry in `clip_stack`.
    Rect,
    /// `PushClipPath` — off-screen layer with `LayerComposite::ClipShape`.
    Shape,
}

/// How a `LayerComposite::Mask` group computes its per-pixel mask alpha.
///
/// Mirrors the GPU mask-composite path (`PushMaskImage` /
/// `PushMaskLinearGradient` / `PushMaskRadialGradient` / `PushMaskConicGradient`
/// → `PopMask`). The gradient variants carry the same parameters as the matching
/// `DrawLinearGradient` / `DrawRadialGradient` / `DrawConicGradient`, so the mask
/// is rasterised with the existing gradient routines and only its alpha channel
/// is consumed. The mask is the gradient's *alpha* (CSS Masking L1 §6.2 default
/// mask-mode for raster/gradient sources — the GPU shader applies `c.a * m.a`).
enum MaskSpec {
    /// No effective mask — the layer composites unchanged. Used for
    /// `PushMaskImage` because the deterministic CPU path decodes no images, so
    /// the mask source is unavailable; the GPU renderer likewise composites with
    /// alpha = 1.0 when the mask image is not registered.
    None,
    /// `linear-gradient(...)` mask: alpha taken from the gradient at each pixel.
    Linear {
        rect: Rect,
        angle_deg: f32,
        stops: Vec<GradientStop>,
        repeating: bool,
    },
    /// `radial-gradient(...)` mask.
    Radial {
        rect: Rect,
        center_x_pct: f32,
        center_y_pct: f32,
        stops: Vec<GradientStop>,
        repeating: bool,
    },
    /// `conic-gradient(...)` mask.
    Conic {
        rect: Rect,
        center_x_pct: f32,
        center_y_pct: f32,
        from_angle_deg: f32,
        stops: Vec<GradientStop>,
        repeating: bool,
    },
}

/// Rasterize display commands to an image using tiny-skia (CPU only, deterministic).
pub(crate) fn rasterize_cpu(
    width: u32,
    height: u32,
    commands: &[DisplayCommand],
    _scroll_x: f32,
    _scroll_y: f32,
) -> Result<Image, Box<dyn std::error::Error>> {
    use tiny_skia::Pixmap;

    let mut base = Pixmap::new(width, height)
        .ok_or("Failed to create pixmap")?;

    // Fill background with white.
    base.fill(tiny_skia::Color::from_rgba8(255, 255, 255, 255));

    // Off-screen layer stack for group effects: group opacity (`PushOpacity` /
    // `PopOpacity`, emitted for `opacity < 1`) and 2D transforms (`PushTransform`
    // / `PopTransform`). `layers[0]` is the white base; each open group pushes a
    // fully-transparent full-size layer that becomes the active draw target. On
    // the matching pop the top layer is composited onto the layer below per its
    // `LayerComposite` op (alpha-blend for opacity, affine `draw_pixmap` for
    // transform). Full-size layers keep the page coordinate space and clip masks
    // valid without translation.
    let mut layers: Vec<Pixmap> = vec![base];
    let mut layer_ops: Vec<LayerComposite> = Vec::new();

    // Active rectangular clip regions (CSS `overflow: hidden`, `PushClipRect`).
    // Stored as a stack of axis-aligned rects; the effective clip is their
    // intersection (`clip_rect`), realised as a tiny-skia mask (`clip_mask`).
    // Mirrors the GPU renderer pushing/popping scissor-style clip layers.
    // Transforms are not modelled in the CPU path, so the intersection of
    // axis-aligned rects is exact here.
    //
    // The mask is passed to a draw *only* when the draw's bounding box is not
    // fully inside `clip_rect` — i.e. only when it actually crosses a clip edge.
    // tiny-skia's masked-blend path rounds ±1 differently from the unmasked
    // path, so skipping the mask for fully-contained draws keeps non-overflowing
    // content byte-identical to the unclipped output (only genuinely overflowing
    // geometry is altered, exactly the visible effect of `overflow: hidden`).
    let mut clip_stack: Vec<Rect> = Vec::new();
    let mut clip_mask: Option<tiny_skia::Mask> = None;
    let mut clip_rect: Option<Rect> = None;
    // BUG-140: вид каждого открытого PushClip* — общий PopClip закрывает
    // rect-клипы (pop clip_stack) и shape-клипы (закрытие слоя) по-разному.
    let mut clip_kinds: Vec<CpuClipKind> = Vec::new();

    for cmd in commands {
        match cmd {
            DisplayCommand::FillRect { rect, color } => {
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), rect_bounds(rect));
                rasterize_fill_rect(layers.last_mut().expect("base layer"), rect, color, c)?;
            }
            DisplayCommand::FillRoundedRect { rect, color, radii } => {
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), rect_bounds(rect));
                rasterize_fill_rounded_rect(
                    layers.last_mut().expect("base layer"), rect, color, radii, c,
                )?;
            }
            DisplayCommand::DrawBorder { rect, widths, colors, styles, radii } => {
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), rect_bounds(rect));
                rasterize_draw_border(
                    layers.last_mut().expect("base layer"), rect, widths, colors, styles, radii, c,
                )?;
            }
            DisplayCommand::DrawOutline { rect, width, style: _, color, offset } => {
                // Outline expands the rect by `offset` on every side.
                let b = (
                    rect.x - offset,
                    rect.y - offset,
                    rect.x + rect.width + offset,
                    rect.y + rect.height + offset,
                );
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), b);
                rasterize_draw_outline(
                    layers.last_mut().expect("base layer"), rect, *width, color, *offset, c,
                )?;
            }
            DisplayCommand::DrawLinearGradient { rect, angle_deg, stops, repeating } => {
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), rect_bounds(rect));
                rasterize_linear_gradient(
                    layers.last_mut().expect("base layer"), rect, *angle_deg, stops, *repeating, c,
                )?;
            }
            DisplayCommand::DrawRadialGradient { rect, center_x_pct, center_y_pct, stops, repeating } => {
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), rect_bounds(rect));
                rasterize_radial_gradient(
                    layers.last_mut().expect("base layer"), rect, *center_x_pct, *center_y_pct,
                    stops, *repeating, c,
                )?;
            }
            DisplayCommand::DrawConicGradient {
                rect, center_x_pct, center_y_pct, from_angle_deg, stops, repeating,
            } => {
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), rect_bounds(rect));
                rasterize_conic_gradient(
                    layers.last_mut().expect("base layer"), rect, *center_x_pct, *center_y_pct,
                    *from_angle_deg, stops, *repeating, c,
                )?;
            }
            DisplayCommand::DrawSvgPath { vertices, color } => {
                let c = effective_clip(
                    clip_mask.as_ref(),
                    clip_rect.as_ref(),
                    vertices_bounds(vertices),
                );
                rasterize_svg_path(layers.last_mut().expect("base layer"), vertices, color, c)?;
            }
            DisplayCommand::PushClipRect { rect } => {
                clip_stack.push(*rect);
                clip_rect = clip_intersection(&clip_stack);
                clip_mask = build_clip_mask(width, height, clip_rect);
                clip_kinds.push(CpuClipKind::Rect);
            }
            // BUG-140: радиусы на CPU-пути не моделируются (как и scissor-
            // fallback femtovg, BUG-132) — rect-клип. Явный обработчик нужен
            // для баланса пар: раньше команда падала в `_ => {}`, а её парный
            // PopClip попал чужой rect из clip_stack.
            DisplayCommand::PushClipRoundedRect { rect, radii: _ } => {
                clip_stack.push(*rect);
                clip_rect = clip_intersection(&clip_stack);
                clip_mask = build_clip_mask(width, height, clip_rect);
                clip_kinds.push(CpuClipKind::Rect);
            }
            // BUG-140: shape-клип (clip-path circle/ellipse/polygon) — слой
            // + альфа-покрытие формы на закрытии.
            DisplayCommand::PushClipPath { shape } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create clip-path layer")?;
                layers.push(layer);
                layer_ops.push(LayerComposite::ClipShape(shape.clone()));
                clip_kinds.push(CpuClipKind::Shape);
            }
            DisplayCommand::PopClip => {
                match clip_kinds.pop() {
                    Some(CpuClipKind::Shape) => {
                        if let (Some(top), Some(op)) = (layers.pop(), layer_ops.pop())
                            && let Some(dst) = layers.last_mut()
                        {
                            close_layer(dst, &top, &op);
                        }
                    }
                    Some(CpuClipKind::Rect) | None => {
                        clip_stack.pop();
                        clip_rect = clip_intersection(&clip_stack);
                        clip_mask = build_clip_mask(width, height, clip_rect);
                    }
                }
            }
            // CSS Overflow L3 §3.2 — `overflow: scroll/auto` (and the `auto`
            // axis a mismatched `overflow` pair coerces to). Treated as a clip
            // to `clip_rect`; the scroll translation is not modelled, matching
            // the CPU path's handling of `PushTransform`. Offscreen snapshots
            // render a freshly-loaded page, so `scroll_x`/`scroll_y` are always
            // 0 and the clip is exact.
            DisplayCommand::PushScrollLayer { clip_rect: cr, .. } => {
                clip_stack.push(*cr);
                clip_rect = clip_intersection(&clip_stack);
                clip_mask = build_clip_mask(width, height, clip_rect);
            }
            DisplayCommand::PopScrollLayer => {
                clip_stack.pop();
                clip_rect = clip_intersection(&clip_stack);
                clip_mask = build_clip_mask(width, height, clip_rect);
            }
            DisplayCommand::DrawImage { rect, .. }
            | DisplayCommand::LazyImageSlot { rect, .. } => {
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), rect_bounds(rect));
                rasterize_image_placeholder(layers.last_mut().expect("base layer"), rect, c)?;
            }
            // CSS Backgrounds L3 §3.3 — background-image url(). The CPU path has
            // no image decoder, so this is a no-op — mirrors the GPU renderer
            // (`renderer.rs` line 4348: `let Some(gpu) = self.images.get(src) else { continue }`)
            // which skips unregistered images silently.
            DisplayCommand::DrawBackgroundImage { .. } => {}
            DisplayCommand::DrawText {
                rect, text, font_size, color, tab_size, ..
            } => {
                // Text uses the bundled Inter face only; family/weight/style are
                // ignored on the CPU path (no FontProvider here). Clip is the
                // active rectangular `overflow` region, applied per glyph pixel.
                rasterize_text(
                    layers.last_mut().expect("base layer"), rect, text, *font_size, color,
                    *tab_size, clip_rect.as_ref(),
                )?;
            }
            // CSS Color L3 §3.2 — `opacity < 1` renders the element's subtree as
            // an off-screen group, then alpha-blends it. Push a transparent
            // full-size layer that becomes the active draw target; draws between
            // here and the matching `PopOpacity` accumulate into it.
            DisplayCommand::PushOpacity { alpha } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create opacity layer")?;
                layers.push(layer);
                layer_ops.push(LayerComposite::Opacity(alpha.clamp(0.0, 1.0)));
            }
            // CSS Transforms L1 §13 — the box's subtree (own background/border +
            // children) renders as an off-screen group, then composites down
            // through the 2D affine. Like `PushOpacity`, push a transparent
            // full-size layer; the affine maps page coordinates and is applied at
            // composite time so geometry need not be transformed per-draw.
            DisplayCommand::PushTransform { matrix } => {
                let [a, b, c, d, e, f] = mat4_to_2d_affine(matrix);
                let t = tiny_skia::Transform::from_row(a, b, c, d, e, f);
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create transform layer")?;
                layers.push(layer);
                layer_ops.push(LayerComposite::Transform(t));
            }
            // CSS Compositing & Blending L1 §5 — `mix-blend-mode` on the box.
            // Like `PushOpacity`, the subtree renders into a transparent
            // full-size layer; the matching `PopBlendMode` composites it onto
            // the backdrop below with the CSS blend formula rather than plain
            // source-over, so the element blends against everything painted
            // beneath it within the stacking context.
            DisplayCommand::PushBlendMode { mode } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create blend layer")?;
                layers.push(layer);
                layer_ops.push(LayerComposite::Blend(map_blend_mode(*mode)));
            }
            // CSS Filter Effects L1 §4 — `filter` chain. Emitted by `walk` to
            // wrap box-shadow / text-shadow blur (`PushFilter { Blur(σ) }` around
            // the shadow FillRect / DrawText) and by the stacking-aware builder
            // for the element's own `filter` property. Like the other group
            // effects, the wrapped draws accumulate into a transparent full-size
            // layer; the matching `PopFilter` applies the chain to that layer and
            // composites it down.
            DisplayCommand::PushFilter { filters, bounds: _ } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create filter layer")?;
                layers.push(layer);
                layer_ops.push(LayerComposite::Filter(filters.clone()));
            }
            // CSS Masking L1 §4 — `mask-image`. Like `PushOpacity`, the element's
            // fully-composited subtree renders into a transparent full-size layer;
            // the matching `PopMask` evaluates the mask (the gradient's alpha) and
            // multiplies it into the layer's alpha before compositing down. Image
            // masks (`PushMaskImage`) have no decoded source on the CPU path, so
            // they record `MaskSpec::None` (composite unchanged), mirroring the GPU
            // fallback when the mask image is not registered.
            DisplayCommand::PushMaskImage { .. } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create mask layer")?;
                layers.push(layer);
                layer_ops.push(LayerComposite::Mask(MaskSpec::None));
            }
            DisplayCommand::PushMaskLinearGradient { rect, angle_deg, stops, repeating } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create mask layer")?;
                layers.push(layer);
                layer_ops.push(LayerComposite::Mask(MaskSpec::Linear {
                    rect: *rect,
                    angle_deg: *angle_deg,
                    stops: stops.clone(),
                    repeating: *repeating,
                }));
            }
            DisplayCommand::PushMaskRadialGradient {
                rect, center_x_pct, center_y_pct, stops, repeating,
            } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create mask layer")?;
                layers.push(layer);
                layer_ops.push(LayerComposite::Mask(MaskSpec::Radial {
                    rect: *rect,
                    center_x_pct: *center_x_pct,
                    center_y_pct: *center_y_pct,
                    stops: stops.clone(),
                    repeating: *repeating,
                }));
            }
            DisplayCommand::PushMaskConicGradient {
                rect, center_x_pct, center_y_pct, from_angle_deg, stops, repeating,
            } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create mask layer")?;
                layers.push(layer);
                layer_ops.push(LayerComposite::Mask(MaskSpec::Conic {
                    rect: *rect,
                    center_x_pct: *center_x_pct,
                    center_y_pct: *center_y_pct,
                    from_angle_deg: *from_angle_deg,
                    stops: stops.clone(),
                    repeating: *repeating,
                }));
            }
            // Close the current off-screen group (opacity, transform, blend, filter
            // or mask) and composite it onto the layer below per its recorded op.
            // The pops share the logic because the emitter guarantees balanced,
            // properly nested Push/Pop, so the top op always matches the
            // closing command.
            DisplayCommand::PopOpacity
            | DisplayCommand::PopTransform
            | DisplayCommand::PopBlendMode
            | DisplayCommand::PopFilter
            | DisplayCommand::PopMask => {
                if let (Some(top), Some(op)) = (layers.pop(), layer_ops.pop())
                    && let Some(dst) = layers.last_mut()
                {
                    close_layer(dst, &top, &op);
                }
            }
            // CSS Filter Effects L1 §6.2 — `backdrop-filter`. Unlike the group
            // effects above, this does *not* open a new layer: it filters the
            // content already painted behind the element (the active layer)
            // within `bounds`, in place. The element's own content then paints
            // on top via the draws emitted up to the matching `PopBackdropFilter`,
            // which is therefore a no-op.
            DisplayCommand::PushBackdropFilter { filters, bounds } => {
                let target = layers.last_mut().expect("base layer");
                apply_backdrop_filter(target, filters, bounds, width, height);
            }
            DisplayCommand::PopBackdropFilter => {
                // No-op: the backdrop was filtered in place at the matching Push.
            }
            // Remaining commands not implemented for CPU rasterization yet.
            _ => {
                // Skipped for now; will be implemented in later phases.
            }
        }
    }

    // Defensive: composite any group layers the emitter left unbalanced
    // (the display-list emitter always pairs Push/Pop, so this is a no-op there).
    while layers.len() > 1 {
        let top = layers.pop().expect("len > 1");
        let op = layer_ops.pop().unwrap_or(LayerComposite::Opacity(1.0));
        if let Some(dst) = layers.last_mut() {
            close_layer(dst, &top, &op);
        }
    }
    let pixmap = layers.pop().expect("base layer");
    let data = pixmap.data().to_vec();
    Ok(Image {
        width,
        height,
        format: lumen_image::PixelFormat::Rgba8,
        data,
        icc_profile: None,
    })
}

/// Composite an off-screen opacity layer `src` onto `dst` with group `alpha`.
///
/// Used by `PopOpacity`: the whole subtree was rendered into `src` (a full-size,
/// initially transparent pixmap), so blending it as a unit with a single alpha
/// reproduces CSS group opacity (the children are *not* individually faded).
/// tiny-skia's `draw_pixmap` blends premultiplied RGBA deterministically, so the
/// result is cross-OS bit-identical like the rest of the CPU path.
fn composite_layer(dst: &mut tiny_skia::Pixmap, src: &tiny_skia::Pixmap, alpha: f32) {
    let paint = tiny_skia::PixmapPaint {
        opacity: alpha.clamp(0.0, 1.0),
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    dst.draw_pixmap(0, 0, src.as_ref(), &paint, tiny_skia::Transform::identity(), None);
}

/// Composite an off-screen group layer `src` onto `dst` per its `LayerComposite`
/// op. Used by `PopOpacity` / `PopTransform` (and the trailing balance loop).
fn close_layer(dst: &mut tiny_skia::Pixmap, src: &tiny_skia::Pixmap, op: &LayerComposite) {
    match op {
        LayerComposite::Opacity(a) => composite_layer(dst, src, *a),
        LayerComposite::Transform(t) => composite_transform_layer(dst, src, *t),
        LayerComposite::Blend(mode) => composite_blend_layer(dst, src, *mode),
        LayerComposite::Filter(filters) => composite_filter_layer(dst, src, filters),
        LayerComposite::Mask(spec) => composite_mask_layer(dst, src, spec),
        LayerComposite::ClipShape(shape) => composite_clip_shape_layer(dst, src, shape),
    }
}

/// BUG-140: применяет shape-клип (`clip-path` circle/ellipse/polygon) к
/// off-screen слою `src` и композитит результат на `dst` (`SourceOver`).
///
/// Покрытие формы растеризуется заливкой tiny-skia пути (анти-алиасинг
/// детерминирован — pure Rust), альфа покрытия умножается в слой как у
/// `composite_mask_layer`. Координаты формы — page px (до transform
/// элемента): команда эмитится внутри `PushTransform`, поэтому слой
/// transform-группы выше переносит уже обрезанный результат.
fn composite_clip_shape_layer(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    shape: &ResolvedClipShape,
) {
    let (w, h) = (src.width(), src.height());
    let mut masked = src.clone();
    if let Some(coverage) = rasterize_clip_shape_coverage(shape, w, h) {
        multiply_alpha_by_mask(&mut masked, &coverage);
    } else {
        // Вырожденная форма (нулевой радиус / <3 вершин полигона) клиппит всё.
        masked.data_mut().fill(0);
    }
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    dst.draw_pixmap(0, 0, masked.as_ref(), &paint, tiny_skia::Transform::identity(), None);
}

/// Растеризует анти-алиасное покрытие формы клипа в transparent-pixmap
/// (альфа = покрытие). `None`, если форма вырождена и путь не строится.
fn rasterize_clip_shape_coverage(
    shape: &ResolvedClipShape,
    width: u32,
    height: u32,
) -> Option<tiny_skia::Pixmap> {
    let mut pb = tiny_skia::PathBuilder::new();
    match shape {
        ResolvedClipShape::Circle { cx, cy, r } => {
            pb.push_oval(tiny_skia::Rect::from_xywh(cx - r, cy - r, 2.0 * r, 2.0 * r)?);
        }
        ResolvedClipShape::Ellipse { cx, cy, rx, ry } => {
            pb.push_oval(tiny_skia::Rect::from_xywh(cx - rx, cy - ry, 2.0 * rx, 2.0 * ry)?);
        }
        ResolvedClipShape::Polygon { verts, .. } => {
            let mut iter = verts.iter();
            let (x0, y0) = iter.next()?;
            pb.move_to(*x0, *y0);
            for (x, y) in iter {
                pb.line_to(*x, *y);
            }
            pb.close();
        }
    }
    // CSS Shapes L1 §3/§4 — even-odd vs nonzero для самопересекающихся форм.
    let fill_rule = match shape {
        ResolvedClipShape::Polygon { even_odd: true, .. } => tiny_skia::FillRule::EvenOdd,
        _ => tiny_skia::FillRule::Winding,
    };
    let path = pb.finish()?;
    let mut coverage = tiny_skia::Pixmap::new(width, height)?;
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    coverage.fill_path(
        &path,
        &paint,
        fill_rule,
        tiny_skia::Transform::identity(),
        None,
    );
    Some(coverage)
}

/// Apply a `mask-image` (`MaskSpec`) to the off-screen element layer `src`, then
/// composite the masked result onto `dst` with plain `SourceOver`.
///
/// Used by `PopMask`: the element's fully-composited subtree (background, border,
/// children) was drawn into `src`, a full-size, initially-transparent pixmap. The
/// mask gradient is rasterised into a fresh pixmap with the existing gradient
/// routines, and its *alpha* channel becomes the mask value (CSS Masking L1 §6.2
/// default mask-mode — the GPU `MASK_COMPOSITE_SHADER` likewise applies `c.a *
/// m.a`). Each `src` pixel (premultiplied RGBA) is multiplied by the mask alpha,
/// which scales the premultiplied colour and the alpha together — exactly an
/// alpha-only mask. Integer-only multiplication keeps the result cross-OS
/// bit-identical, like the rest of the CPU path. `MaskSpec::None` (image masks,
/// no decoded source) composites the layer unchanged.
fn composite_mask_layer(dst: &mut tiny_skia::Pixmap, src: &tiny_skia::Pixmap, spec: &MaskSpec) {
    let (w, h) = (src.width(), src.height());
    let mut masked = src.clone();
    if let Some(mask) = render_mask(spec, w, h) {
        multiply_alpha_by_mask(&mut masked, &mask);
    }
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    dst.draw_pixmap(0, 0, masked.as_ref(), &paint, tiny_skia::Transform::identity(), None);
}

/// Rasterise a `MaskSpec` gradient into a fresh transparent pixmap whose alpha
/// channel is the per-pixel mask value. Returns `None` for `MaskSpec::None` (no
/// effective mask) — the caller then composites the layer unchanged.
fn render_mask(spec: &MaskSpec, width: u32, height: u32) -> Option<tiny_skia::Pixmap> {
    let mut mask = tiny_skia::Pixmap::new(width, height)?;
    match spec {
        MaskSpec::None => return None,
        MaskSpec::Linear { rect, angle_deg, stops, repeating } => {
            rasterize_linear_gradient(&mut mask, rect, *angle_deg, stops, *repeating, None).ok()?;
        }
        MaskSpec::Radial { rect, center_x_pct, center_y_pct, stops, repeating } => {
            rasterize_radial_gradient(
                &mut mask, rect, *center_x_pct, *center_y_pct, stops, *repeating, None,
            ).ok()?;
        }
        MaskSpec::Conic { rect, center_x_pct, center_y_pct, from_angle_deg, stops, repeating } => {
            rasterize_conic_gradient(
                &mut mask, rect, *center_x_pct, *center_y_pct, *from_angle_deg, stops, *repeating,
                None,
            ).ok()?;
        }
    }
    Some(mask)
}

/// Multiply each pixel's premultiplied RGBA in `layer` by the corresponding mask
/// alpha (0..=255) from `mask`. tiny-skia pixmaps store premultiplied RGBA, so
/// scaling all four channels by the mask alpha yields the premultiplied form of
/// the same colour with its alpha reduced — an alpha-only mask. Rounded integer
/// division (`(v·m + 127) / 255`) keeps the result deterministic across OSes.
fn multiply_alpha_by_mask(layer: &mut tiny_skia::Pixmap, mask: &tiny_skia::Pixmap) {
    let mp = mask.data().to_vec();
    let lp = layer.data_mut();
    for (px, mpx) in lp.chunks_exact_mut(4).zip(mp.chunks_exact(4)) {
        let m = u32::from(mpx[3]);
        for ch in px.iter_mut() {
            *ch = ((u32::from(*ch) * m + 127) / 255) as u8;
        }
    }
}

/// Apply the CSS `filter` chain `filters` to off-screen layer `src`, then
/// composite the filtered result onto `dst` with plain `SourceOver`.
///
/// Used by `PopFilter`: the wrapped subtree (a box-shadow / text-shadow rect or
/// the element's own painted content) was drawn into `src`, a full-size,
/// initially-transparent pixmap. The chain is applied left to right (CSS Filter
/// Effects L1 §4.1) — Gaussian blur reuses the SVG three-box-blur approximation
/// (`gaussian_blur`, integer-only so it is cross-OS bit-identical), the
/// colour-matrix filters mirror the GPU `apply_filter_fn` shader. The filtered
/// layer then blends over the backdrop accumulated in `dst`.
fn composite_filter_layer(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    filters: &[FilterFn],
) {
    let mut cur = src.clone();
    for f in filters {
        match f {
            FilterFn::Blur(sigma) => cur = gaussian_blur(&cur, *sigma),
            other => apply_color_filter(&mut cur, other),
        }
    }
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    dst.draw_pixmap(0, 0, cur.as_ref(), &paint, tiny_skia::Transform::identity(), None);
}

/// Apply a `backdrop-filter` chain (CSS Filter Effects L1 §6.2) to the content
/// already painted in `target` within `bounds`, in place.
///
/// The filter operates on the *backdrop* — the content painted behind the
/// element so far, i.e. the current active layer. The whole layer is cloned and
/// filtered (so Gaussian blur samples neighbouring backdrop pixels rather than
/// only those inside `bounds`), then only the `bounds` border-box region of the
/// filtered copy is written back over `target` with `Source` blend (replace).
/// The element's own background/border paint on top afterwards via subsequent
/// draws, so the matching `PopBackdropFilter` is a no-op. Reuses the same
/// integer-only `gaussian_blur` and un-premultiplied `apply_color_filter` as
/// `composite_filter_layer`, so the result is cross-OS bit-identical.
fn apply_backdrop_filter(
    target: &mut tiny_skia::Pixmap,
    filters: &[FilterFn],
    bounds: &Rect,
    width: u32,
    height: u32,
) {
    let mut filtered = target.clone();
    for f in filters {
        match f {
            FilterFn::Blur(sigma) => filtered = gaussian_blur(&filtered, *sigma),
            other => apply_color_filter(&mut filtered, other),
        }
    }
    // Write back only the element's border-box region. A hard rect mask scopes
    // the replace to `bounds`; `Source` blend overwrites the backdrop there with
    // its filtered counterpart (outside the mask the original target is kept).
    let mask = build_clip_mask(width, height, Some(*bounds));
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::Source,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    target.draw_pixmap(
        0,
        0,
        filtered.as_ref(),
        &paint,
        tiny_skia::Transform::identity(),
        mask.as_ref(),
    );
}

/// Gaussian blur of a premultiplied-RGBA pixmap via the SVG Filter Effects
/// three-box-blur approximation (CSS Filter Effects L1 §4.4 / SVG 1.1 §15.17).
///
/// `sigma` is the standard deviation in pixels. A box blur of radius `r` (window
/// `2r+1`) has variance `((2r+1)²−1)/12`; three successive box blurs add
/// variance, so matching `σ²` gives `r = round((√(4σ²+1) − 1) / 2)`. Three
/// separable passes per axis converge to a Gaussian. Only integer accumulation
/// and IEEE-754 add/sub/div are used (no `exp`/`erf`), so the output is
/// bit-identical across Windows/macOS/Linux — required by the exact-match
/// snapshot gate (same constraint the conic-gradient `atan2` approximation
/// solved). Edges replicate the border sample (clamp index), mirroring the GPU
/// blur shader's clamp-to-edge sampler.
fn gaussian_blur(src: &tiny_skia::Pixmap, sigma: f32) -> tiny_skia::Pixmap {
    let w = src.width() as usize;
    let h = src.height() as usize;
    let radius = (((4.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0).round() as i32;
    if radius <= 0 || w == 0 || h == 0 {
        return src.clone();
    }
    // Premultiplied RGBA8 → f32 working buffer (4 channels interleaved).
    let mut buf: Vec<f32> = src.data().iter().map(|&b| f32::from(b)).collect();
    let mut tmp = vec![0.0f32; buf.len()];
    for _ in 0..3 {
        box_blur_h(&buf, &mut tmp, w, h, radius);
        box_blur_v(&tmp, &mut buf, w, h, radius);
    }
    let mut out = tiny_skia::Pixmap::new(src.width(), src.height())
        .expect("filter layer dims are valid");
    for (dst_b, &v) in out.data_mut().iter_mut().zip(buf.iter()) {
        *dst_b = v.round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// One horizontal box-blur pass with replicate-edge sampling.
///
/// Sliding-window running sum (O(width) per row, radius-independent): the window
/// `[x−r, x+r]` averages `2r+1` clamped samples. The accumulator is updated by
/// adding the entering sample and subtracting the leaving one in a fixed order,
/// so the f32 result is deterministic across platforms.
fn box_blur_h(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: i32) {
    let win = (2 * r + 1) as f32;
    let last = (w - 1) as i32;
    for y in 0..h {
        let row = y * w * 4;
        for ch in 0..4 {
            let mut acc = 0.0f32;
            for i in -r..=r {
                let idx = i.clamp(0, last) as usize;
                acc += src[row + idx * 4 + ch];
            }
            for x in 0..w {
                dst[row + x * 4 + ch] = acc / win;
                let out_idx = (x as i32 - r).clamp(0, last) as usize;
                let in_idx = (x as i32 + r + 1).clamp(0, last) as usize;
                acc += src[row + in_idx * 4 + ch] - src[row + out_idx * 4 + ch];
            }
        }
    }
}

/// One vertical box-blur pass with replicate-edge sampling (column analogue of
/// [`box_blur_h`]).
fn box_blur_v(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: i32) {
    let win = (2 * r + 1) as f32;
    let last = (h - 1) as i32;
    for x in 0..w {
        for ch in 0..4 {
            let mut acc = 0.0f32;
            for i in -r..=r {
                let idx = i.clamp(0, last) as usize;
                acc += src[(idx * w + x) * 4 + ch];
            }
            for y in 0..h {
                dst[(y * w + x) * 4 + ch] = acc / win;
                let out_idx = (y as i32 - r).clamp(0, last) as usize;
                let in_idx = (y as i32 + r + 1).clamp(0, last) as usize;
                acc += src[(in_idx * w + x) * 4 + ch] - src[(out_idx * w + x) * 4 + ch];
            }
        }
    }
}

/// Apply a single colour-matrix CSS filter to a premultiplied-RGBA pixmap,
/// in place (CSS Filter Effects L1 §7). Mirrors the GPU `apply_filter_fn` shader
/// exactly: each formula operates on **straight** (un-premultiplied) sRGB
/// components in `[0,1]`, so pixels are un-premultiplied first and
/// re-premultiplied after. `Blur` is handled separately and is a no-op here.
fn apply_color_filter(pixmap: &mut tiny_skia::Pixmap, f: &FilterFn) {
    for px in pixmap.pixels_mut() {
        let a = f32::from(px.alpha());
        let (mut r, mut g, mut b) = if a > 0.0 {
            (
                f32::from(px.red()) / a,
                f32::from(px.green()) / a,
                f32::from(px.blue()) / a,
            )
        } else {
            (0.0, 0.0, 0.0)
        };
        let mut a_unit = a / 255.0;
        match f {
            FilterFn::Blur(_) => {}
            FilterFn::Brightness(amt) => {
                r = (r * amt).clamp(0.0, 1.0);
                g = (g * amt).clamp(0.0, 1.0);
                b = (b * amt).clamp(0.0, 1.0);
            }
            FilterFn::Contrast(amt) => {
                r = ((r - 0.5) * amt + 0.5).clamp(0.0, 1.0);
                g = ((g - 0.5) * amt + 0.5).clamp(0.0, 1.0);
                b = ((b - 0.5) * amt + 0.5).clamp(0.0, 1.0);
            }
            FilterFn::Grayscale(amt) => {
                let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                r = mix(r, lum, *amt);
                g = mix(g, lum, *amt);
                b = mix(b, lum, *amt);
            }
            FilterFn::HueRotate(rad) => {
                let (c, s) = (cos_approx(*rad), sin_approx(*rad));
                let nr = r * (0.213 + 0.787 * c - 0.213 * s)
                    + g * (0.715 - 0.715 * c - 0.715 * s)
                    + b * (0.072 - 0.072 * c + 0.928 * s);
                let ng = r * (0.213 - 0.213 * c + 0.143 * s)
                    + g * (0.715 + 0.285 * c + 0.140 * s)
                    + b * (0.072 - 0.072 * c - 0.283 * s);
                let nb = r * (0.213 - 0.213 * c - 0.787 * s)
                    + g * (0.715 - 0.715 * c + 0.715 * s)
                    + b * (0.072 + 0.928 * c + 0.072 * s);
                r = nr.clamp(0.0, 1.0);
                g = ng.clamp(0.0, 1.0);
                b = nb.clamp(0.0, 1.0);
            }
            FilterFn::Invert(amt) => {
                r = mix(r, 1.0 - r, *amt);
                g = mix(g, 1.0 - g, *amt);
                b = mix(b, 1.0 - b, *amt);
            }
            FilterFn::Opacity(amt) => {
                a_unit = (a_unit * amt).clamp(0.0, 1.0);
            }
            FilterFn::Saturate(amt) => {
                let nr = r * (0.213 + 0.787 * amt)
                    + g * (0.715 - 0.715 * amt)
                    + b * (0.072 - 0.072 * amt);
                let ng = r * (0.213 - 0.213 * amt)
                    + g * (0.715 + 0.285 * amt)
                    + b * (0.072 - 0.072 * amt);
                let nb = r * (0.213 - 0.213 * amt)
                    + g * (0.715 - 0.715 * amt)
                    + b * (0.072 + 0.928 * amt);
                r = nr.clamp(0.0, 1.0);
                g = ng.clamp(0.0, 1.0);
                b = nb.clamp(0.0, 1.0);
            }
            FilterFn::Sepia(amt) => {
                let sr = (0.393 * r + 0.769 * g + 0.189 * b).clamp(0.0, 1.0);
                let sg = (0.349 * r + 0.686 * g + 0.168 * b).clamp(0.0, 1.0);
                let sb = (0.272 * r + 0.534 * g + 0.131 * b).clamp(0.0, 1.0);
                r = mix(r, sr, *amt);
                g = mix(g, sg, *amt);
                b = mix(b, sb, *amt);
            }
        }
        let na = (a_unit * 255.0).round().clamp(0.0, 255.0);
        let to_u8 = |c: f32| (c * na).round().clamp(0.0, 255.0) as u8;
        *px = tiny_skia::PremultipliedColorU8::from_rgba(
            to_u8(r),
            to_u8(g),
            to_u8(b),
            na as u8,
        )
        .expect("premultiplied components ≤ alpha by construction");
    }
}

/// Linear interpolation `x·(1−a) + y·a` (GPU `mix`).
fn mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1.0 - a) + y * a
}

/// Deterministic, libm-free cosine for `hue-rotate`. Reuses the cross-OS
/// bit-identity requirement that ruled out `f32::cos` (platform libm differs in
/// the last ULP); a minimax polynomial on the range-reduced argument keeps the
/// snapshot gate reproducible. Range reduction is exact (`+`/`−`/`*` only).
fn cos_approx(rad: f32) -> f32 {
    sin_approx(rad + std::f32::consts::FRAC_PI_2)
}

/// Deterministic, libm-free sine (see [`cos_approx`]). Range-reduces to
/// `[−π, π]` then evaluates a 7th-order minimax polynomial (odd terms), using
/// only IEEE-754 `+`/`−`/`*`, so the result is identical on every platform.
fn sin_approx(rad: f32) -> f32 {
    use std::f32::consts::PI;
    // Range-reduce to [-PI, PI] with integer-multiple subtraction.
    let two_pi = 2.0 * PI;
    let k = (rad / two_pi + 0.5).floor();
    let x = rad - k * two_pi;
    // Minimax 7th-order odd polynomial for sin on [-PI, PI].
    let x2 = x * x;
    x * (1.0
        + x2 * (-1.666_665_7e-1
            + x2 * (8.332_161e-3 + x2 * (-1.951_529_6e-4 + x2 * 2.600_24e-6))))
}

/// Composite an off-screen `mix-blend-mode` layer `src` onto `dst` with `mode`.
///
/// Used by `PopBlendMode`: the whole subtree was rendered into `src` (a
/// full-size, initially transparent pixmap) in page coordinates, so blending it
/// onto the backdrop already accumulated in `dst` with the CSS blend formula
/// reproduces `mix-blend-mode` (CSS Compositing & Blending L1 §5). tiny-skia
/// applies the separable/non-separable blend per premultiplied pixel
/// deterministically, so the result is cross-OS bit-identical.
fn composite_blend_layer(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    mode: tiny_skia::BlendMode,
) {
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: mode,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    dst.draw_pixmap(0, 0, src.as_ref(), &paint, tiny_skia::Transform::identity(), None);
}

/// Map a paint-crate `BlendMode` (`mix-blend-mode` keyword) to its tiny-skia
/// equivalent. Every CSS separable/non-separable mode has a direct tiny-skia
/// counterpart; `PlusLighter` (additive, CSS Compositing & Blending L2 §6) maps
/// to tiny-skia `Plus`, and `Normal` to plain `SourceOver`.
fn map_blend_mode(mode: crate::BlendMode) -> tiny_skia::BlendMode {
    use tiny_skia::BlendMode as T;
    match mode {
        crate::BlendMode::Normal => T::SourceOver,
        crate::BlendMode::Multiply => T::Multiply,
        crate::BlendMode::Screen => T::Screen,
        crate::BlendMode::Overlay => T::Overlay,
        crate::BlendMode::Darken => T::Darken,
        crate::BlendMode::Lighten => T::Lighten,
        crate::BlendMode::ColorDodge => T::ColorDodge,
        crate::BlendMode::ColorBurn => T::ColorBurn,
        crate::BlendMode::HardLight => T::HardLight,
        crate::BlendMode::SoftLight => T::SoftLight,
        crate::BlendMode::Difference => T::Difference,
        crate::BlendMode::Exclusion => T::Exclusion,
        crate::BlendMode::Hue => T::Hue,
        crate::BlendMode::Saturation => T::Saturation,
        crate::BlendMode::Color => T::Color,
        crate::BlendMode::Luminosity => T::Luminosity,
        crate::BlendMode::PlusLighter => T::Plus,
    }
}

/// Composite an off-screen transform layer `src` onto `dst` through `transform`.
///
/// Used by `PopTransform`: the whole subtree was rendered into `src` (a
/// full-size, initially transparent pixmap) in untransformed page coordinates,
/// so resampling it through the box's viewport-space affine reproduces CSS
/// transforms — translate / rotate / scale / skew / matrix2d (CSS Transforms L1
/// §13). `draw_pixmap` with bilinear filtering is a pure-software path, so the
/// result is cross-OS bit-identical like the rest of the CPU rasterizer.
fn composite_transform_layer(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    transform: tiny_skia::Transform,
) {
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Bilinear,
    };
    dst.draw_pixmap(0, 0, src.as_ref(), &paint, transform, None);
}

/// Geometric intersection of every clip rect on the stack.
///
/// Each `PushClipRect` narrows the active clip (CSS `overflow: hidden` on a
/// descendant). Returns `None` when the stack is empty (no clipping). A
/// non-overlapping stack yields a zero-area `Rect` (width/height clamped to 0),
/// which contains nothing — so every subsequent draw is fully clipped.
fn clip_intersection(stack: &[Rect]) -> Option<Rect> {
    if stack.is_empty() {
        return None;
    }
    let mut left = f32::NEG_INFINITY;
    let mut top = f32::NEG_INFINITY;
    let mut right = f32::INFINITY;
    let mut bottom = f32::INFINITY;
    for r in stack {
        left = left.max(r.x);
        top = top.max(r.y);
        right = right.min(r.x + r.width);
        bottom = bottom.min(r.y + r.height);
    }
    Some(Rect {
        x: left,
        y: top,
        width: (right - left).max(0.0),
        height: (bottom - top).max(0.0),
    })
}

/// Build a tiny-skia clip mask covering `clip_rect`.
///
/// Returns `None` when there is no clip (so draws receive `None` and skip
/// masking). A zero-area `clip_rect` (empty intersection) yields an all-zero
/// mask, which clips everything out. tiny-skia masks are deterministic across
/// platforms, so the produced mask is identical on Windows/macOS/Linux.
fn build_clip_mask(width: u32, height: u32, clip_rect: Option<Rect>) -> Option<tiny_skia::Mask> {
    let cr = clip_rect?;
    let mut mask = tiny_skia::Mask::new(width, height)?;
    if cr.width <= 0.0 || cr.height <= 0.0 {
        // Empty intersection → leave the all-zero mask (everything clipped out).
        return Some(mask);
    }
    let rect = tiny_skia::Rect::from_xywh(cr.x, cr.y, cr.width, cr.height)?;
    let path = tiny_skia::PathBuilder::from_rect(rect);
    mask.fill_path(
        &path,
        tiny_skia::FillRule::Winding,
        true,
        tiny_skia::Transform::identity(),
    );
    Some(mask)
}

/// Axis-aligned bounding box `(left, top, right, bottom)` of a rect.
fn rect_bounds(r: &Rect) -> (f32, f32, f32, f32) {
    (r.x, r.y, r.x + r.width, r.y + r.height)
}

/// Axis-aligned bounding box `(left, top, right, bottom)` of a vertex list.
/// Empty input yields a degenerate box at the origin (never contained, but
/// `DrawSvgPath` with no vertices is a no-op anyway).
fn vertices_bounds(vertices: &[[f32; 2]]) -> (f32, f32, f32, f32) {
    let mut l = f32::INFINITY;
    let mut t = f32::INFINITY;
    let mut r = f32::NEG_INFINITY;
    let mut b = f32::NEG_INFINITY;
    for v in vertices {
        l = l.min(v[0]);
        t = t.min(v[1]);
        r = r.max(v[0]);
        b = b.max(v[1]);
    }
    (l, t, r, b)
}

/// Effective clip mask for a draw whose bounding box is `bounds`.
///
/// Returns the mask only when a clip is active *and* `bounds` is not fully
/// inside `clip_rect` — i.e. only when the draw actually crosses a clip edge.
/// A draw entirely inside the clip receives `None` and so renders byte-identical
/// to the unclipped path (avoids tiny-skia's ±1 masked-blend rounding). An empty
/// intersection (`clip_rect` zero-area) contains nothing, so the all-zero mask
/// is always returned and the draw is fully clipped out.
fn effective_clip<'a>(
    clip_mask: Option<&'a tiny_skia::Mask>,
    clip_rect: Option<&Rect>,
    bounds: (f32, f32, f32, f32),
) -> Option<&'a tiny_skia::Mask> {
    match (clip_mask, clip_rect) {
        (Some(m), Some(cr)) if !rect_contains(cr, bounds) => Some(m),
        _ => None,
    }
}

/// Whether the draw bounds `(left, top, right, bottom)` lie fully inside `outer`.
///
/// Used to skip clip masking for draws that don't touch a clip edge, keeping
/// their pixels byte-identical to the unclipped path. A small epsilon absorbs
/// float rounding so a draw flush against the clip edge still counts as inside.
fn rect_contains(outer: &Rect, bounds: (f32, f32, f32, f32)) -> bool {
    const EPS: f32 = 0.01;
    let (l, t, r, b) = bounds;
    l >= outer.x - EPS
        && t >= outer.y - EPS
        && r <= outer.x + outer.width + EPS
        && b <= outer.y + outer.height + EPS
}

fn rasterize_fill_rect(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    color: &Color,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::Paint;

    let paint = Paint {
        shader: tiny_skia::Shader::SolidColor(color_to_skia(*color)),
        anti_alias: true,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };

    let skia_rect = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height)
        .ok_or("Invalid rect dimensions")?;

    pixmap.fill_rect(skia_rect, &paint, tiny_skia::Transform::identity(), clip);
    Ok(())
}

fn rasterize_fill_rounded_rect(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    color: &Color,
    radii: &CornerRadii,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::Paint;

    let paint = Paint {
        shader: tiny_skia::Shader::SolidColor(color_to_skia(*color)),
        anti_alias: true,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };

    let mut pb = tiny_skia::PathBuilder::new();

    // Build rounded rect path: start from top-left, go clockwise.
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;

    let tl_x = radii.tl;
    let tl_y = radii.tl_y;
    let tr_x = radii.tr;
    let tr_y = radii.tr_y;
    let br_x = radii.br;
    let br_y = radii.br_y;
    let bl_x = radii.bl;
    let bl_y = radii.bl_y;

    // Top-left corner.
    pb.move_to(x0 + tl_x, y0);
    // Top edge.
    pb.line_to(x1 - tr_x, y0);
    // Top-right corner (use Bézier curve for rounded corner).
    pb.cubic_to(
        x1 - tr_x * 0.55,
        y0,
        x1,
        y0 + tr_y * 0.55,
        x1,
        y0 + tr_y,
    );
    // Right edge.
    pb.line_to(x1, y1 - br_y);
    // Bottom-right corner.
    pb.cubic_to(
        x1,
        y1 - br_y * 0.55,
        x1 - br_x * 0.55,
        y1,
        x1 - br_x,
        y1,
    );
    // Bottom edge.
    pb.line_to(x0 + bl_x, y1);
    // Bottom-left corner.
    pb.cubic_to(
        x0 + bl_x * 0.55,
        y1,
        x0,
        y1 - bl_y * 0.55,
        x0,
        y1 - bl_y,
    );
    // Left edge.
    pb.line_to(x0, y0 + tl_y);
    // Top-left corner (close).
    pb.cubic_to(
        x0,
        y0 + tl_y * 0.55,
        x0 + tl_x * 0.55,
        y0,
        x0 + tl_x,
        y0,
    );

    pb.close();

    if let Some(path) = pb.finish() {
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            clip,
        );
    }

    Ok(())
}

fn rasterize_draw_border(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    widths: &[f32; 4],
    colors: &[Color; 4],
    styles: &[BorderStyle; 4],
    radii: &CornerRadii,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    // BUG-175: rounded border. A box with border-radius whose sides are all
    // uniform-coloured solid borders is painted as an even-odd ring between the
    // outer and inner rounded rects, so corners follow the radius. Mismatched
    // colours / dashed-dotted-double styles fall back to axis-aligned side quads.
    let uniform_solid = widths.iter().all(|&w| w > 0.0)
        && styles.iter().all(|s| matches!(s, BorderStyle::Solid))
        && colors[1] == colors[0]
        && colors[2] == colors[0]
        && colors[3] == colors[0];
    if !radii.all_zero() && uniform_solid {
        return rasterize_rounded_border_ring(pixmap, rect, *widths, colors[0], radii, clip);
    }

    // Border edges are axis-aligned quads. `anti_alias: false` is both visually
    // correct (GPU renderer draws without per-edge AA) and required to avoid
    // tiny-skia's hairline_aa::fill_dot8 debug_assert for sub-pixel rects (BUG-052).
    let [top_w, right_w, bottom_w, left_w] = widths;
    let [top_c, right_c, bottom_c, left_c] = colors;
    let [top_s, right_s, bottom_s, left_s] = styles;

    draw_border_side_h(pixmap, rect.x, rect.y, rect.width, *top_w, *top_c, *top_s, clip)?;
    draw_border_side_v(pixmap, rect.x + rect.width - right_w, rect.y, *right_w, rect.height, *right_c, *right_s, clip)?;
    draw_border_side_h(pixmap, rect.x, rect.y + rect.height - bottom_w, rect.width, *bottom_w, *bottom_c, *bottom_s, clip)?;
    draw_border_side_v(pixmap, rect.x, rect.y, *left_w, rect.height, *left_c, *left_s, clip)?;
    Ok(())
}

/// Appends a closed rounded-rectangle contour to `pb` using cubic-Bézier
/// quarter-ellipse corners (kappa ≈ 0.5523). `r` holds per-corner (x, y) radii,
/// assumed already clamped to the box. Shared by both contours of the border
/// ring (BUG-175).
fn push_rounded_rect_outline(
    pb: &mut tiny_skia::PathBuilder,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: &CornerRadii,
) {
    const K: f32 = 0.5523;
    let x1 = x + w;
    let y1 = y + h;
    pb.move_to(x + r.tl, y);
    pb.line_to(x1 - r.tr, y);
    pb.cubic_to(x1 - r.tr + K * r.tr, y, x1, y + r.tr_y - K * r.tr_y, x1, y + r.tr_y);
    pb.line_to(x1, y1 - r.br_y);
    pb.cubic_to(x1, y1 - r.br_y + K * r.br_y, x1 - r.br + K * r.br, y1, x1 - r.br, y1);
    pb.line_to(x + r.bl, y1);
    pb.cubic_to(x + r.bl - K * r.bl, y1, x, y1 - r.bl_y + K * r.bl_y, x, y1 - r.bl_y);
    pb.line_to(x, y + r.tl_y);
    pb.cubic_to(x, y + r.tl_y - K * r.tl_y, x + r.tl - K * r.tl, y, x + r.tl, y);
    pb.close();
}

/// Paints a uniform-coloured solid rounded border as the even-odd ring between
/// the outer rounded-rect (border box, outer radii) and the inner rounded-rect
/// (padding box, inner radii = outer − side width, CSS Backgrounds L3 §5.5).
/// `widths` is `[top, right, bottom, left]`; when the border swallows the whole
/// box the ring degenerates to a solid rounded rect.
fn rasterize_rounded_border_ring(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    widths: [f32; 4],
    color: Color,
    radii: &CornerRadii,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
    if w <= 0.0 || h <= 0.0 {
        return Ok(());
    }
    let [top, right, bottom, left] = widths;

    let outer = radii.clamped_to_box(w, h);
    let mut pb = tiny_skia::PathBuilder::new();
    push_rounded_rect_outline(&mut pb, x, y, w, h, &outer);

    let iw = w - left - right;
    let ih = h - top - bottom;
    if iw > 0.0 && ih > 0.0 {
        let inner = radii.inner_for_border(widths).clamped_to_box(iw, ih);
        push_rounded_rect_outline(&mut pb, x + left, y + top, iw, ih, &inner);
    }

    if let Some(path) = pb.finish() {
        let paint = tiny_skia::Paint {
            shader: tiny_skia::Shader::SolidColor(color_to_skia(color)),
            anti_alias: true,
            force_hq_pipeline: false,
            blend_mode: tiny_skia::BlendMode::SourceOver,
        };
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::EvenOdd,
            tiny_skia::Transform::identity(),
            clip,
        );
    }
    Ok(())
}

/// Draw one horizontal border side using segments from `dash_math` for
/// Dashed/Dotted, or a single fill_rect for Solid/Double.
fn draw_border_side_h(
    pixmap: &mut tiny_skia::Pixmap,
    x0: f32,
    y: f32,
    total: f32,
    h: f32,
    color: Color,
    style: BorderStyle,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    if h <= 0.0 || total <= 0.0 {
        return Ok(());
    }
    let paint = border_paint(color);
    match style {
        BorderStyle::None => {}
        BorderStyle::Dashed => {
            for (off, len) in dashed_border_offsets(total, h) {
                if let Some(r) = tiny_skia::Rect::from_xywh(x0 + off, y, len, h) {
                    pixmap.fill_rect(r, &paint, tiny_skia::Transform::identity(), clip);
                }
            }
        }
        BorderStyle::Dotted => {
            for (off, len) in dotted_border_offsets(total, h) {
                if let Some(r) = tiny_skia::Rect::from_xywh(x0 + off, y, len, h) {
                    pixmap.fill_rect(r, &paint, tiny_skia::Transform::identity(), clip);
                }
            }
        }
        _ => {
            if let Some(r) = tiny_skia::Rect::from_xywh(x0, y, total, h) {
                pixmap.fill_rect(r, &paint, tiny_skia::Transform::identity(), clip);
            }
        }
    }
    Ok(())
}

/// Draw one vertical border side using segments from `dash_math` for
/// Dashed/Dotted, or a single fill_rect for Solid/Double.
fn draw_border_side_v(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y0: f32,
    w: f32,
    total: f32,
    color: Color,
    style: BorderStyle,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    if w <= 0.0 || total <= 0.0 {
        return Ok(());
    }
    let paint = border_paint(color);
    match style {
        BorderStyle::None => {}
        BorderStyle::Dashed => {
            for (off, len) in dashed_border_offsets(total, w) {
                if let Some(r) = tiny_skia::Rect::from_xywh(x, y0 + off, w, len) {
                    pixmap.fill_rect(r, &paint, tiny_skia::Transform::identity(), clip);
                }
            }
        }
        BorderStyle::Dotted => {
            for (off, len) in dotted_border_offsets(total, w) {
                if let Some(r) = tiny_skia::Rect::from_xywh(x, y0 + off, w, len) {
                    pixmap.fill_rect(r, &paint, tiny_skia::Transform::identity(), clip);
                }
            }
        }
        _ => {
            if let Some(r) = tiny_skia::Rect::from_xywh(x, y0, w, total) {
                pixmap.fill_rect(r, &paint, tiny_skia::Transform::identity(), clip);
            }
        }
    }
    Ok(())
}

fn border_paint(color: Color) -> tiny_skia::Paint<'static> {
    tiny_skia::Paint {
        shader: tiny_skia::Shader::SolidColor(color_to_skia(color)),
        anti_alias: false,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    }
}

fn rasterize_draw_outline(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    width: f32,
    color: &Color,
    offset: f32,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::Paint;

    if width <= 0.0 {
        return Ok(());
    }

    let paint = Paint {
        shader: tiny_skia::Shader::SolidColor(color_to_skia(*color)),
        anti_alias: true,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };

    // Expand rect by offset.
    let x = rect.x - offset;
    let y = rect.y - offset;
    let w = rect.width + 2.0 * offset;
    let h = rect.height + 2.0 * offset;

    // Draw outline as a stroked rectangle.
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x, y);
    pb.line_to(x + w, y);
    pb.line_to(x + w, y + h);
    pb.line_to(x, y + h);
    pb.close();

    let stroke = tiny_skia::Stroke {
        width,
        ..Default::default()
    };

    if let Some(path) = pb.finish() {
        pixmap.stroke_path(
            &path,
            &paint,
            &stroke,
            tiny_skia::Transform::identity(),
            clip,
        );
    }

    Ok(())
}

/// Build tiny-skia `GradientStop`s from resolved `(position, color)` pairs.
///
/// For repeating gradients the resolved positions span `[first, last]` with
/// `last < 1`; rescaling to fill `[0,1]` turns that span into one tile so
/// `SpreadMode::Repeat` tiles it across the whole line. Returns the rescaled
/// stops plus the `(first, last)` fractions of the original line that the tile
/// occupies (caller shortens the gradient line to that sub-segment).
fn skia_gradient_stops(
    resolved: &[(f32, Color)],
    repeating: bool,
) -> Option<(Vec<tiny_skia::GradientStop>, f32, f32)> {
    if resolved.len() < 2 {
        return None;
    }
    let first = resolved.first().map(|s| s.0).unwrap_or(0.0);
    let last = resolved.last().map(|s| s.0).unwrap_or(1.0);
    let span = (last - first).max(1e-6);
    let (rescale, lo, hi) = if repeating {
        (true, first, last)
    } else {
        (false, 0.0, 1.0)
    };
    let stops = resolved
        .iter()
        .map(|&(pos, color)| {
            let p = if rescale { ((pos - first) / span).clamp(0.0, 1.0) } else { pos.clamp(0.0, 1.0) };
            tiny_skia::GradientStop::new(p, color_to_skia(color))
        })
        .collect();
    Some((stops, lo, hi))
}

/// CSS Images L3 §3.4 — linear gradient line endpoints in box-relative UV [0,1].
///
/// Mirrors the GPU renderer's `linear_gradient_uv_endpoints`. CSS angle
/// convention: 0° = "to top", 90° = "to right", 180° = "to bottom". Returns
/// `(start_uv, end_uv)` and the gradient-line pixel length (for px stops).
fn linear_uv_endpoints(w: f32, h: f32, angle_deg: f32) -> ([f32; 2], [f32; 2], f32) {
    if w <= 0.0 || h <= 0.0 {
        return ([0.0, 0.5], [1.0, 0.5], w.max(1.0));
    }
    let theta = angle_deg.to_radians();
    let dx = theta.sin();
    let dy = -theta.cos();
    let half_len = (w * dx.abs() + h * dy.abs()) / 2.0;
    if half_len < 1e-6 {
        return ([0.5, 0.5], [0.5, 0.5], 1.0);
    }
    let cx = w / 2.0;
    let cy = h / 2.0;
    let sx = (cx - dx * half_len) / w;
    let sy = (cy - dy * half_len) / h;
    let ex = (cx + dx * half_len) / w;
    let ey = (cy + dy * half_len) / h;
    ([sx, sy], [ex, ey], 2.0 * half_len)
}

/// CSS Images L3 §3.4 — `linear-gradient(...)` via tiny-skia `LinearGradient`.
fn rasterize_linear_gradient(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    angle_deg: f32,
    stops: &[GradientStop],
    repeating: bool,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::{LinearGradient, Paint, Point, SpreadMode, Transform};

    let (start_uv, end_uv, line_len) = linear_uv_endpoints(rect.width, rect.height, angle_deg);
    let resolved = resolve_stop_positions(stops, line_len);
    let Some((skia_stops, lo, hi)) = skia_gradient_stops(&resolved, repeating) else {
        return Ok(());
    };

    // UV → pixel space; for repeating, clip the line to the [lo,hi] sub-segment.
    let px = |u: [f32; 2]| Point::from_xy(rect.x + u[0] * rect.width, rect.y + u[1] * rect.height);
    let full_start = start_uv;
    let dir = [end_uv[0] - start_uv[0], end_uv[1] - start_uv[1]];
    let seg = |t: f32| [full_start[0] + dir[0] * t, full_start[1] + dir[1] * t];
    let start = px(seg(lo));
    let end = px(seg(hi));

    let mode = if repeating { SpreadMode::Repeat } else { SpreadMode::Pad };
    let shader = LinearGradient::new(start, end, skia_stops, mode, Transform::identity())
        .ok_or("degenerate linear gradient")?;

    let paint = Paint {
        shader,
        anti_alias: true,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };
    let skia_rect = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height)
        .ok_or("Invalid rect dimensions")?;
    pixmap.fill_rect(skia_rect, &paint, Transform::identity(), clip);
    Ok(())
}

/// CSS Images L3 §3.3 — `radial-gradient(...)` via tiny-skia `RadialGradient`.
///
/// Reproduces the GPU renderer's "farthest-corner" anisotropic ellipse: the
/// semi-axes are `rx = max(cx, 1-cx)`, `ry = max(cy, 1-cy)` in box-relative
/// units. tiny-skia radials are isotropic, so the ellipse is produced by
/// rendering a unit-ish circle and stretching it with a post-scale transform.
fn rasterize_radial_gradient(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    center_x_pct: f32,
    center_y_pct: f32,
    stops: &[GradientStop],
    repeating: bool,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::{Paint, Point, RadialGradient, SpreadMode, Transform};

    let rx_px = center_x_pct.max(1.0 - center_x_pct).max(1e-3) * rect.width;
    let ry_px = center_y_pct.max(1.0 - center_y_pct).max(1e-3) * rect.height;
    let line_len = rx_px.max(ry_px).max(1.0);
    let resolved = resolve_stop_positions(stops, line_len);
    let Some((skia_stops, lo, hi)) = skia_gradient_stops(&resolved, repeating) else {
        return Ok(());
    };

    // Render the gradient in a normalized space where the ellipse is a unit
    // circle of radius `radius`, then scale x by rx and y by ry around the
    // centre to recover the ellipse. For repeating, shrink the radius to the
    // [lo,hi] sub-segment so SpreadMode::Repeat tiles outward.
    let cx = rect.x + center_x_pct * rect.width;
    let cy = rect.y + center_y_pct * rect.height;
    let radius = (hi - lo).max(1e-3);
    let center_norm = Point::from_xy(0.0, 0.0);
    let mode = if repeating { SpreadMode::Repeat } else { SpreadMode::Pad };
    let shader = RadialGradient::new(
        center_norm,
        center_norm,
        radius,
        skia_stops,
        mode,
        // Map normalized circle space to pixel ellipse: translate to centre,
        // scale by (rx, ry). `lo` offset for repeating handled via radius span.
        Transform::from_row(rx_px, 0.0, 0.0, ry_px, cx, cy),
    )
    .ok_or("degenerate radial gradient")?;

    let paint = Paint {
        shader,
        anti_alias: true,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };
    let skia_rect = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height)
        .ok_or("Invalid rect dimensions")?;
    pixmap.fill_rect(skia_rect, &paint, Transform::identity(), clip);
    Ok(())
}

/// CSS Images L4 §3.7 — `conic-gradient(...)` rasterized per-pixel.
///
/// tiny-skia has no native conic (angular) shader, so the angular sweep is
/// computed directly: for every pixel centre inside `rect` the polar angle
/// around `(center_x_pct, center_y_pct)` is measured in box-space and mapped to
/// gradient position `t`. Mirrors the GPU conic shader: CSS convention 0° = top
/// (-y), clockwise, with `from_angle_deg` as the starting angle; `repeating`
/// tiles the resolved-stop span within one revolution.
///
/// All math uses only IEEE-exact primitive ops (no platform `atan2`/`sin`), so
/// the output is bit-identical across Windows/macOS/Linux — required for the
/// exact-match CPU snapshot gate. Colours are composited `SourceOver` onto the
/// premultiplied RGBA8 backing buffer.
///
/// `clip`, when present, is the active rectangular clip coverage mask (one byte
/// per pixel): each composited source alpha is scaled by that coverage so the
/// per-pixel path honours `overflow`/scroll clipping exactly like the tiny-skia
/// draws. A fully-contained draw is handed `None` and stays unclipped.
#[allow(clippy::too_many_arguments)]
fn rasterize_conic_gradient(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    center_x_pct: f32,
    center_y_pct: f32,
    from_angle_deg: f32,
    stops: &[GradientStop],
    repeating: bool,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return Ok(());
    }
    // Conic stop positions are revolution fractions (angle stops already
    // converted to percent on parse); GPU resolves with line_len = 1.0.
    let resolved = resolve_stop_positions(stops, 1.0);
    if resolved.is_empty() {
        return Ok(());
    }

    let from_rad = from_angle_deg.to_radians();
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;

    // Integer pixel-center bounds of the rect, clamped to the pixmap.
    let x0 = (rect.x.floor() as i32).max(0);
    let y0 = (rect.y.floor() as i32).max(0);
    let x1 = ((rect.x + rect.width).ceil() as i32).min(pw);
    let y1 = ((rect.y + rect.height).ceil() as i32).min(ph);

    let cx = rect.x + center_x_pct * rect.width;
    let cy = rect.y + center_y_pct * rect.height;

    let first_pos = resolved.first().map(|s| s.0).unwrap_or(0.0);
    let span = (resolved.last().map(|s| s.0).unwrap_or(1.0) - first_pos).max(0.0);

    // One coverage byte per pixel when a clip is active; `None` means unclipped.
    let clip_data = clip.map(tiny_skia::Mask::data);
    let data = pixmap.data_mut();
    for py in y0..y1 {
        let fy = py as f32 + 0.5;
        if fy < rect.y || fy >= rect.y + rect.height {
            continue;
        }
        for px in x0..x1 {
            let fx = px as f32 + 0.5;
            if fx < rect.x || fx >= rect.x + rect.width {
                continue;
            }
            let idx = (py * pw + px) as usize;
            let coverage = clip_data.map_or(255u8, |m| m[idx]);
            if coverage == 0 {
                continue;
            }
            // CSS convention: 0° = top (-y), angles grow clockwise.
            let raw = atan2_det(fx - cx, -(fy - cy)) - from_rad;
            let frac = raw / std::f32::consts::TAU;
            let norm = frac - frac.floor(); // [0, 1)
            let t = if repeating && resolved.len() > 1 && span > 1e-4 {
                let mod_s = norm - (norm / span).floor() * span;
                first_pos + mod_s
            } else {
                norm
            };
            let mut color = sample_gradient_color(&resolved, t, repeating);
            if coverage != 255 {
                color.a = ((color.a as u16 * coverage as u16 + 127) / 255) as u8;
            }
            composite_over(data, idx, color);
        }
    }
    Ok(())
}

/// Composite a straight-alpha `src` colour `SourceOver` onto the premultiplied
/// RGBA8 pixel at `pixel_idx` in tiny-skia's backing buffer.
fn composite_over(data: &mut [u8], pixel_idx: usize, src: Color) {
    let i = pixel_idx * 4;
    let sa = src.a as f32 / 255.0;
    let inv = 1.0 - sa;
    let out = |s: u8, d: u8| (s as f32 * sa + d as f32 * inv).round().clamp(0.0, 255.0) as u8;
    data[i] = out(src.r, data[i]);
    data[i + 1] = out(src.g, data[i + 1]);
    data[i + 2] = out(src.b, data[i + 2]);
    data[i + 3] = (src.a as f32 + data[i + 3] as f32 * inv).round().clamp(0.0, 255.0) as u8;
}

/// SVG 1.1 §11 — pre-tessellated SVG shape (flat triangle list) filled with a
/// solid colour.
///
/// `vertices.len()` is a multiple of 3; every consecutive triple is one triangle
/// in page-pixel coordinates, and `fill-opacity` / `stroke-opacity` is already
/// baked into `color` (strokes arrive tessellated into filled triangles too).
/// All triangles are merged into one path and filled in a single `SourceOver`
/// pass (Winding rule) so the union of the tessellation composites exactly once
/// — this avoids antialiasing seams along the shared internal edges that
/// per-triangle filling would produce, and matches the GPU renderer drawing the
/// whole shape in one `Fill` op.
fn rasterize_svg_path(
    pixmap: &mut tiny_skia::Pixmap,
    vertices: &[[f32; 2]],
    color: &Color,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::Paint;

    let mut pb = tiny_skia::PathBuilder::new();
    for tri in vertices.chunks_exact(3) {
        pb.move_to(tri[0][0], tri[0][1]);
        pb.line_to(tri[1][0], tri[1][1]);
        pb.line_to(tri[2][0], tri[2][1]);
        pb.close();
    }

    let Some(path) = pb.finish() else {
        return Ok(());
    };

    let paint = Paint {
        shader: tiny_skia::Shader::SolidColor(color_to_skia(*color)),
        anti_alias: true,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };
    pixmap.fill_path(
        &path,
        &paint,
        tiny_skia::FillRule::Winding,
        tiny_skia::Transform::identity(),
        clip,
    );
    Ok(())
}

/// `<img>` placeholder fill (CSS Images L3 — unloaded replaced element).
///
/// The deterministic CPU path never registers decoded image pixels, so — exactly
/// like the GPU renderer's headless fallback (`renderer.rs`, `DrawImage` arm) —
/// every image box paints as the solid light-grey placeholder quad. The GPU uses
/// the linear-float colour `[0.85, 0.85, 0.85, 1.0]`; `0.85 × 255 ≈ 217`, so the
/// placeholder is `rgba8(217, 217, 217, 255)`. Alt text is *not* drawn here (the
/// CPU rasterizer has no text primitive yet), so only pages with empty `alt`
/// reproduce the GPU output exactly.
fn rasterize_image_placeholder(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    let placeholder = Color { r: 217, g: 217, b: 217, a: 255 };
    rasterize_fill_rect(pixmap, rect, &placeholder, clip)
}

#[inline]
fn color_to_skia(color: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(color.r, color.g, color.b, color.a)
}

/// Parsed bundled face plus the tables `rasterize_text` needs. `None` if the
/// embedded font fails to parse (should never happen for committed Inter).
struct CpuFace<'a> {
    font: lumen_font::Font<'a>,
    units_per_em: u16,
    ascent: f32,
    descent: f32,
    cmap: lumen_font::Cmap<'a>,
    hmtx: lumen_font::Hmtx<'a>,
}

/// Parse the bundled Inter face once per `DrawText` run.
fn load_bundled_face() -> Option<CpuFace<'static>> {
    let font = lumen_font::Font::parse(BUNDLED_FONT).ok()?;
    let head = font.head().ok()?;
    let hhea = font.hhea().ok()?;
    let cmap = font.cmap().ok()?;
    let hmtx = font.hmtx().ok()?;
    Some(CpuFace {
        font,
        units_per_em: head.units_per_em,
        ascent: f32::from(hhea.ascent),
        descent: f32::from(hhea.descent),
        cmap,
        hmtx,
    })
}

/// Render a `DrawText` run with the bundled Inter face, compositing each
/// glyph's coverage onto `pixmap`.
///
/// Geometry mirrors the GPU renderer (`push_text_glyphs`): the baseline sits
/// at `rect.y + font_size * ascent / (ascent − descent)`, the pen starts at
/// `rect.x`, and each glyph advances by `advance_width * font_size /
/// units_per_em`. Glyphs are rasterized directly at `font_size` (no atlas
/// size-binning), so the CPU output is sharper than the GPU path but stays
/// cross-OS bit-identical — the snapshot reference is generated from this same
/// path. `clip` is the active rectangular `overflow` region; pixels outside it
/// are dropped.
///
/// Coverage from all glyphs is accumulated into a single tiny-skia `Mask`,
/// then a one-shot `fill_rect` paints the text colour through it — the same
/// `SourceOver` blend as every other CPU primitive, so anti-aliased glyph
/// edges composite identically to fills.
fn rasterize_text(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    text: &str,
    font_size: f32,
    color: &Color,
    tab_size: f32,
    clip: Option<&Rect>,
) -> Result<(), Box<dyn std::error::Error>> {
    if text.is_empty() || font_size <= 0.0 || color.a == 0 {
        return Ok(());
    }
    let Some(face) = load_bundled_face() else {
        return Ok(());
    };
    let denom = face.ascent - face.descent;
    let ascent_ratio = if denom != 0.0 { face.ascent / denom } else { 0.8 };
    let baseline_y = rect.y + font_size * ascent_ratio;
    let advance_scale = font_size / f32::from(face.units_per_em);
    let rasterizer = lumen_font::Rasterizer::new(font_size, face.units_per_em);

    let width = pixmap.width();
    let height = pixmap.height();
    let mut mask = tiny_skia::Mask::new(width, height).ok_or("Failed to create glyph mask")?;
    let mut any_coverage = false;

    let mut cursor_x = rect.x;
    for ch in text.chars() {
        // CSS Text L3 §10.1 — tab advances by tab_size pixels, draws nothing.
        if ch == '\t' && tab_size > 0.0 {
            cursor_x += tab_size;
            continue;
        }
        // No fallback faces on the CPU path: a missing codepoint resolves to
        // glyph 0 (.notdef), matching the GPU renderer's `(primary, 0)` result.
        let glyph_id = face.cmap.glyph_index(ch as u32).unwrap_or(0);
        if let Ok(Some(glyph)) = face.font.glyph_resolved(glyph_id)
            && let Some(bitmap) = rasterizer.rasterize(&glyph)
            && blit_glyph_coverage(&mut mask, &bitmap, cursor_x, baseline_y, clip, width, height)
        {
            any_coverage = true;
        }
        let advance = face.hmtx.advance_width(glyph_id).unwrap_or(0);
        cursor_x += f32::from(advance) * advance_scale;
    }

    if !any_coverage {
        return Ok(());
    }

    let paint = tiny_skia::Paint {
        shader: tiny_skia::Shader::SolidColor(color_to_skia(*color)),
        anti_alias: false,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };
    let full = tiny_skia::Rect::from_xywh(0.0, 0.0, width as f32, height as f32)
        .ok_or("Invalid pixmap dimensions")?;
    pixmap.fill_rect(full, &paint, tiny_skia::Transform::identity(), Some(&mask));
    Ok(())
}

/// Composite one glyph's coverage bitmap into `mask`, returning whether any
/// pixel was written. The bitmap's top-left maps to page coordinates
/// `(pen_x + bitmap.left, baseline_y − bitmap.top)`, rounded to the nearest
/// pixel for a deterministic pen-to-pixel snap. Pixels outside the pixmap or
/// the rectangular `clip` are dropped; overlapping glyph pixels keep the max
/// coverage (glyphs in a run rarely overlap, but kerning-tight pairs can).
fn blit_glyph_coverage(
    mask: &mut tiny_skia::Mask,
    bitmap: &lumen_font::Bitmap,
    pen_x: f32,
    baseline_y: f32,
    clip: Option<&Rect>,
    width: u32,
    height: u32,
) -> bool {
    if bitmap.width == 0 || bitmap.height == 0 {
        return false;
    }
    let origin_x = (pen_x + bitmap.left).round() as i32;
    let origin_y = (baseline_y - bitmap.top).round() as i32;
    let w = width as i32;
    let h = height as i32;
    let data = mask.data_mut();
    let mut wrote = false;
    for gy in 0..bitmap.height as i32 {
        let dy = origin_y + gy;
        if dy < 0 || dy >= h {
            continue;
        }
        if let Some(cr) = clip
            && ((dy as f32) < cr.y || (dy as f32) >= cr.y + cr.height)
        {
            continue;
        }
        for gx in 0..bitmap.width as i32 {
            let dx = origin_x + gx;
            if dx < 0 || dx >= w {
                continue;
            }
            if let Some(cr) = clip
                && ((dx as f32) < cr.x || (dx as f32) >= cr.x + cr.width)
            {
                continue;
            }
            let cov = bitmap.pixels[(gy as u32 * bitmap.width + gx as u32) as usize];
            if cov == 0 {
                continue;
            }
            let idx = (dy * w + dx) as usize;
            if cov > data[idx] {
                data[idx] = cov;
            }
            wrote = true;
        }
    }
    wrote
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sample the RGBA8 pixel at `(x, y)` from a rasterized [`Image`].
    fn px(img: &Image, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let i = ((y * img.width + x) * 4) as usize;
        (img.data[i], img.data[i + 1], img.data[i + 2], img.data[i + 3])
    }

    /// `DrawSvgPath` fills the tessellated triangle interior with the solid
    /// colour and leaves the background white outside it.
    #[test]
    fn svg_path_fills_triangle_interior() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        // One triangle with a large axis-aligned interior so the centroid sample
        // is unambiguously inside (avoids antialiased edge pixels).
        let cmds = vec![DisplayCommand::DrawSvgPath {
            vertices: vec![[10.0, 10.0], [50.0, 10.0], [30.0, 50.0]],
            color: red,
        }];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");

        // Centroid ≈ (30, 23): solidly inside the triangle.
        assert_eq!(px(&img, 30, 23), (255, 0, 0, 255), "interior should be red");
        // Far corner outside the triangle stays white.
        assert_eq!(px(&img, 1, 1), (255, 255, 255, 255), "exterior stays white");
    }

    /// `DrawImage` fills its box with the light-grey placeholder quad
    /// (`rgba8(217,217,217,255)`), mirroring the GPU renderer's headless fallback
    /// when no decoded pixels are registered.
    #[test]
    fn draw_image_fills_grey_placeholder() {
        use lumen_layout::{ObjectFit, ObjectPosition, ImageRendering};
        let cmds = vec![DisplayCommand::DrawImage {
            rect: Rect::new(10.0, 10.0, 40.0, 30.0),
            src: "missing.png".into(),
            alt: String::new(),
            object_fit: ObjectFit::Fill,
            object_position: ObjectPosition::default(),
            image_rendering: ImageRendering::Auto,
        }];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");

        // Interior of the box is the grey placeholder.
        assert_eq!(px(&img, 30, 25), (217, 217, 217, 255), "placeholder grey");
        // Outside the box stays white.
        assert_eq!(px(&img, 60, 60), (255, 255, 255, 255), "exterior stays white");
    }

    /// A degenerate path (fewer than 3 vertices) is a no-op, not a panic.
    #[test]
    fn svg_path_empty_is_noop() {
        let cmds = vec![DisplayCommand::DrawSvgPath {
            vertices: vec![],
            color: Color { r: 0, g: 0, b: 0, a: 255 },
        }];
        let img = rasterize_cpu(8, 8, &cmds, 0.0, 0.0).expect("rasterize");
        assert_eq!(px(&img, 4, 4), (255, 255, 255, 255), "background untouched");
    }

    /// A two-stop conic (red at 0° → blue at one revolution, centre, from 0°)
    /// sweeps clockwise: the top-centre is the start (red), the bottom-centre is
    /// half a revolution in (≈ midway red→blue).
    #[test]
    fn conic_sweeps_first_to_last_stop() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let cmds = vec![DisplayCommand::DrawConicGradient {
            rect: Rect { x: 0.0, y: 0.0, width: 64.0, height: 64.0 },
            center_x_pct: 0.5,
            center_y_pct: 0.5,
            from_angle_deg: 0.0,
            stops: vec![
                GradientStop { color: red, position: None },
                GradientStop { color: blue, position: None },
            ],
            repeating: false,
        }];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");

        // Top-centre column ≈ start of the sweep → essentially red.
        let (tr, _tg, tb, ta) = px(&img, 32, 2);
        assert!(tr > 200 && tb < 50 && ta == 255, "top is red-ish, got ({tr},{_tg},{tb},{ta})");

        // Bottom-centre column ≈ half a revolution → midway red→blue.
        let (br, _bg, bb, ba) = px(&img, 32, 61);
        assert!(
            (80..=180).contains(&br) && (80..=180).contains(&bb) && ba == 255,
            "bottom is midway, got ({br},{_bg},{bb},{ba})"
        );
    }

    /// A conic with no stops is a no-op, not a panic.
    #[test]
    fn conic_empty_stops_noop() {
        let cmds = vec![DisplayCommand::DrawConicGradient {
            rect: Rect { x: 0.0, y: 0.0, width: 8.0, height: 8.0 },
            center_x_pct: 0.5,
            center_y_pct: 0.5,
            from_angle_deg: 0.0,
            stops: vec![],
            repeating: false,
        }];
        let img = rasterize_cpu(8, 8, &cmds, 0.0, 0.0).expect("rasterize");
        assert_eq!(px(&img, 4, 4), (255, 255, 255, 255), "background untouched");
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, width: w, height: h }
    }

    /// `PushClipRect` confines a following `FillRect` to the clip region;
    /// pixels outside the clip keep the white background.
    #[test]
    fn push_clip_rect_clips_fill() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let cmds = vec![
            DisplayCommand::PushClipRect { rect: rect(10.0, 10.0, 20.0, 20.0) },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: blue },
            DisplayCommand::PopClip,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");

        // Inside the clip [10,30) — filled blue.
        assert_eq!(px(&img, 20, 20), (0, 0, 255, 255), "inside clip is blue");
        // Outside the clip — background white.
        assert_eq!(px(&img, 45, 45), (255, 255, 255, 255), "outside clip stays white");
        assert_eq!(px(&img, 4, 4), (255, 255, 255, 255), "above-left of clip stays white");
    }

    /// `PopClip` removes the clip so a later `FillRect` paints everywhere again.
    #[test]
    fn pop_clip_restores_full_drawing() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let green = Color { r: 0, g: 255, b: 0, a: 255 };
        let cmds = vec![
            DisplayCommand::PushClipRect { rect: rect(10.0, 10.0, 10.0, 10.0) },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: red },
            DisplayCommand::PopClip,
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: green },
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");

        // A point that was outside the first (clipped) fill is painted by the
        // second, unclipped fill → green, proving the clip was popped.
        assert_eq!(px(&img, 45, 45), (0, 255, 0, 255), "post-pop fill reaches outside old clip");
    }

    /// Nested `PushClipRect`s intersect: only the overlap of both clip rects is
    /// drawn; regions inside one but not the other are clipped out.
    #[test]
    fn nested_clip_intersects() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let cmds = vec![
            DisplayCommand::PushClipRect { rect: rect(10.0, 10.0, 40.0, 40.0) }, // x,y ∈ [10,50)
            DisplayCommand::PushClipRect { rect: rect(30.0, 30.0, 40.0, 40.0) }, // x,y ∈ [30,70)
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: blue },
            DisplayCommand::PopClip,
            DisplayCommand::PopClip,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");

        // Intersection [30,50) — blue.
        assert_eq!(px(&img, 40, 40), (0, 0, 255, 255), "intersection is blue");
        // Inside outer clip only (x=20 < 30) — clipped out by inner.
        assert_eq!(px(&img, 20, 20), (255, 255, 255, 255), "outer-only region clipped");
        // Inside inner clip only (x=60 ≥ 50) — clipped out by outer.
        assert_eq!(px(&img, 60, 60), (255, 255, 255, 255), "inner-only region clipped");
    }

    /// BUG-140 (TEST-109 c0): `PushClipPath` circle обрезает заливку по
    /// окружности, а не по её bounding box — точка внутри bbox, но вне круга,
    /// остаётся белой.
    #[test]
    fn clip_path_circle_clips_to_circle_not_bbox() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let cmds = vec![
            DisplayCommand::PushClipPath {
                shape: ResolvedClipShape::Circle { cx: 32.0, cy: 32.0, r: 10.0 },
            },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: blue },
            DisplayCommand::PopClip,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");

        // Центр круга — синий.
        assert_eq!(px(&img, 32, 32), (0, 0, 255, 255), "centre inside circle");
        // Угол bbox (24,24): расстояние до центра ≈ 11.3 > 10 — вне круга,
        // но внутри bounding box. Старое bbox-приближение красило его синим.
        assert_eq!(px(&img, 24, 24), (255, 255, 255, 255), "bbox corner outside circle");
        // Далеко за пределами — белый.
        assert_eq!(px(&img, 5, 5), (255, 255, 255, 255), "far exterior stays white");
    }

    /// BUG-140 (TEST-109 c2): `PushClipPath` polygon (треугольник) обрезает
    /// заливку по форме; вне треугольника (но внутри его bbox) — фон.
    #[test]
    fn clip_path_polygon_clips_to_triangle() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        // Треугольник (32,4) (60,60) (4,60) — как polygon(50% 0, 100% 100%, 0 100%).
        let cmds = vec![
            DisplayCommand::PushClipPath {
                shape: ResolvedClipShape::Polygon {
                    verts: vec![(32.0, 4.0), (60.0, 60.0), (4.0, 60.0)],
                    even_odd: false,
                },
            },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: red },
            DisplayCommand::PopClip,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");

        // Центроид (32, 41) — внутри треугольника.
        assert_eq!(px(&img, 32, 41), (255, 0, 0, 255), "triangle interior red");
        // Верхние углы bbox — вне треугольника.
        assert_eq!(px(&img, 8, 8), (255, 255, 255, 255), "top-left outside triangle");
        assert_eq!(px(&img, 56, 8), (255, 255, 255, 255), "top-right outside triangle");
    }

    /// CSS Shapes L1 §3/§4 — `clip-path` fill-rule. Два перекрывающихся
    /// квадрата одним контуром (одинаковая ориентация → область пересечения
    /// имеет winding 2). `even_odd: false` (nonzero) заливает её; `even_odd:
    /// true` оставляет «дырку» (чётное число пересечений).
    #[test]
    fn clip_path_polygon_even_odd_hole() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        // Квадрат A (8,8)-(40,40) и квадрат B (24,24)-(56,56), сшитые в один
        // замкнутый контур; пересечение — (24,24)-(40,40), центр (32,32).
        let verts = vec![
            (8.0, 8.0), (40.0, 8.0), (40.0, 40.0), (8.0, 40.0), (8.0, 8.0),
            (24.0, 24.0), (56.0, 24.0), (56.0, 56.0), (24.0, 56.0), (24.0, 24.0),
        ];
        let make = |even_odd: bool| {
            let cmds = vec![
                DisplayCommand::PushClipPath {
                    shape: ResolvedClipShape::Polygon { verts: verts.clone(), even_odd },
                },
                DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: red },
                DisplayCommand::PopClip,
            ];
            rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize")
        };

        let nz = make(false);
        let eo = make(true);

        // Зоны только-A (14,14) и только-B (50,50) заливаются при обоих правилах.
        assert_eq!(px(&nz, 14, 14), (255, 0, 0, 255), "nonzero: square A red");
        assert_eq!(px(&eo, 14, 14), (255, 0, 0, 255), "evenodd: square A red");
        assert_eq!(px(&nz, 50, 50), (255, 0, 0, 255), "nonzero: square B red");
        assert_eq!(px(&eo, 50, 50), (255, 0, 0, 255), "evenodd: square B red");

        // Центр пересечения (32,32): nonzero заливает, evenodd — дырка (фон).
        assert_eq!(px(&nz, 32, 32), (255, 0, 0, 255), "nonzero fills the overlap");
        assert_eq!(px(&eo, 32, 32), (255, 255, 255, 255), "evenodd leaves a hole in the overlap");
    }

    /// BUG-140 (TEST-109 c0/c1): clip-path эмитится внутри PushTransform —
    /// transform-слой переносит уже обрезанный результат, т.е. клип едет
    /// вместе с элементом.
    #[test]
    fn clip_path_carried_through_transform() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let cmds = vec![
            DisplayCommand::PushTransform { matrix: lumen_layout::Mat4::translation_2d(20.0, 0.0) },
            DisplayCommand::PushClipPath {
                shape: ResolvedClipShape::Circle { cx: 16.0, cy: 32.0, r: 8.0 },
            },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: blue },
            DisplayCommand::PopClip,
            DisplayCommand::PopTransform,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");

        // Круг задан вокруг (16,32); transform сдвигает результат на +20 по X
        // → синий центр в (36,32), а исходная позиция (16,32) — белая.
        assert_eq!(px(&img, 36, 32), (0, 0, 255, 255), "clipped circle translated to x=36");
        assert_eq!(px(&img, 16, 32), (255, 255, 255, 255), "original position stays white");
    }

    /// `DrawText` paints the bundled Inter glyphs: a large opaque-coloured run
    /// must darken/colour some pixels away from the white background. Exact
    /// glyph pixels are font-dependent, so we only assert "ink appeared" within
    /// the run box, which is enough to catch a regression to the no-op path.
    #[test]
    fn draw_text_renders_ink() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let cmds = vec![DisplayCommand::DrawText {
            rect: rect(2.0, 2.0, 120.0, 40.0),
            text: "Hi".to_string(),
            font_size: 32.0,
            color: blue,
            font_family: Vec::new(),
            font_weight: lumen_layout::FontWeight::default(),
            font_style: lumen_layout::FontStyle::default(),
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
        }];
        let img = rasterize_cpu(128, 48, &cmds, 0.0, 0.0).expect("rasterize");

        // At least one pixel in the run box must carry blue ink (not white bg).
        let mut inked = false;
        for y in 2..44 {
            for x in 2..120 {
                let (r, g, b, _) = px(&img, x, y);
                if b > r && b > g {
                    inked = true;
                }
            }
        }
        assert!(inked, "DrawText produced no blue ink");
    }

    /// Empty text is a no-op: the background stays pure white.
    #[test]
    fn draw_text_empty_is_noop() {
        let black = Color { r: 0, g: 0, b: 0, a: 255 };
        let cmds = vec![DisplayCommand::DrawText {
            rect: rect(0.0, 0.0, 64.0, 64.0),
            text: String::new(),
            font_size: 20.0,
            color: black,
            font_family: Vec::new(),
            font_weight: lumen_layout::FontWeight::default(),
            font_style: lumen_layout::FontStyle::default(),
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
        }];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        assert_eq!(px(&img, 10, 10), (255, 255, 255, 255), "empty text left bg white");
    }

    /// A rectangular clip drops glyph pixels outside it: text drawn fully to the
    /// left of a clip that starts at x=200 leaves the clipped sample untouched.
    #[test]
    fn draw_text_respects_clip() {
        let black = Color { r: 0, g: 0, b: 0, a: 255 };
        let cmds = vec![
            // Clip to the right half; the text sits in the left half → no ink.
            DisplayCommand::PushClipRect { rect: rect(200.0, 0.0, 100.0, 64.0) },
            DisplayCommand::DrawText {
                rect: rect(2.0, 2.0, 180.0, 40.0),
                text: "Hidden".to_string(),
                font_size: 32.0,
                color: black,
                font_family: Vec::new(),
                font_weight: lumen_layout::FontWeight::default(),
                font_style: lumen_layout::FontStyle::default(),
                font_variation_axes: Vec::new(),
                tab_size: 0.0,
                highlight_name: None,
            },
            DisplayCommand::PopClip,
        ];
        let img = rasterize_cpu(320, 64, &cmds, 0.0, 0.0).expect("rasterize");
        // Left half (outside clip) must remain white.
        for x in (5..180).step_by(10) {
            assert_eq!(
                px(&img, x, 20),
                (255, 255, 255, 255),
                "glyph pixel at x={x} should be clipped out",
            );
        }
    }

    /// `PushOpacity { 0.5 }` around an opaque blue fill blends it 50/50 with the
    /// white background: result ≈ (127, 127, 255).
    #[test]
    fn opacity_half_blends_blue_over_white() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let cmds = vec![
            DisplayCommand::PushOpacity { alpha: 0.5 },
            DisplayCommand::FillRect { rect: rect(10.0, 10.0, 40.0, 40.0), color: blue },
            DisplayCommand::PopOpacity,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");

        let (r, g, b, a) = px(&img, 30, 30);
        assert!(
            (120..=135).contains(&r) && (120..=135).contains(&g) && b > 250 && a == 255,
            "blue at 0.5 opacity over white ≈ (127,127,255), got ({r},{g},{b},{a})",
        );
        // Outside the group's box stays white.
        assert_eq!(px(&img, 60, 60), (255, 255, 255, 255), "exterior stays white");
    }

    /// `PushOpacity { 0.0 }` makes the whole group invisible: the background is
    /// untouched.
    #[test]
    fn opacity_zero_keeps_background() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let cmds = vec![
            DisplayCommand::PushOpacity { alpha: 0.0 },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: blue },
            DisplayCommand::PopOpacity,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        assert_eq!(px(&img, 32, 32), (255, 255, 255, 255), "fully transparent group");
    }

    /// Group opacity fades the *whole* subtree by one alpha: two sibling fills in
    /// the same group are each blended 50/50 with the background (not stacked or
    /// double-faded).
    #[test]
    fn opacity_group_fades_whole_subtree() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let green = Color { r: 0, g: 128, b: 0, a: 255 };
        let cmds = vec![
            DisplayCommand::PushOpacity { alpha: 0.5 },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 20.0, 20.0), color: red },
            DisplayCommand::FillRect { rect: rect(20.0, 0.0, 20.0, 20.0), color: green },
            DisplayCommand::PopOpacity,
        ];
        let img = rasterize_cpu(64, 32, &cmds, 0.0, 0.0).expect("rasterize");

        // Red box ≈ (255,127,127).
        let (rr, rg, rb, _) = px(&img, 10, 10);
        assert!(
            rr > 250 && (120..=135).contains(&rg) && (120..=135).contains(&rb),
            "red faded, got ({rr},{rg},{rb})",
        );
        // Green box ≈ (127,191,127): blended from (0,128,0) over white at 0.5.
        let (gr, gg, gb, _) = px(&img, 30, 10);
        assert!(
            (120..=135).contains(&gr) && (185..=197).contains(&gg) && (120..=135).contains(&gb),
            "green faded, got ({gr},{gg},{gb})",
        );
    }

    /// `PushTransform` with an integer translation shifts the group's ink: a blue
    /// fill at the origin lands at the translated position, and the original spot
    /// is left at the background colour. Integer translate makes the bilinear
    /// resample exact, so the moved fill is solid blue.
    #[test]
    fn transform_translate_shifts_fill() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let cmds = vec![
            DisplayCommand::PushTransform { matrix: lumen_layout::Mat4::translation_2d(20.0, 10.0) },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 20.0, 20.0), color: blue },
            DisplayCommand::PopTransform,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        // Translated destination (20+5, 10+5) is solid blue.
        assert_eq!(px(&img, 25, 15), (0, 0, 255, 255), "fill moved by (20,10)");
        // The untranslated origin is now empty → background white.
        assert_eq!(px(&img, 5, 5), (255, 255, 255, 255), "origin cleared by translate");
    }

    /// An identity `PushTransform` is a no-op: compositing the layer through the
    /// identity affine copies it back byte-for-byte, so the fill is unchanged.
    #[test]
    fn transform_identity_is_noop() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let cmds = vec![
            DisplayCommand::PushTransform { matrix: lumen_layout::Mat4::IDENTITY },
            DisplayCommand::FillRect { rect: rect(10.0, 10.0, 20.0, 20.0), color: blue },
            DisplayCommand::PopTransform,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        assert_eq!(px(&img, 20, 20), (0, 0, 255, 255), "identity transform unchanged");
    }

    /// `scale(2)` about the origin grows the group: a 10×10 fill at the origin
    /// covers roughly a 20×20 area, so a sample well inside the scaled region
    /// that was white before the scale is now blue.
    #[test]
    fn transform_scale_grows_fill() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let cmds = vec![
            DisplayCommand::PushTransform { matrix: lumen_layout::Mat4::scale_2d(2.0, 2.0) },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 10.0, 10.0), color: blue },
            DisplayCommand::PopTransform,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        // Source (6,6) — interior of the 10×10 fill — maps to dest (12,12).
        assert_eq!(px(&img, 12, 12), (0, 0, 255, 255), "scaled fill covers (12,12)");
    }

    /// `mix-blend-mode: multiply` multiplies the source against the backdrop
    /// (`result = s·d / 255` per channel). A magenta (255,0,255) layer blended
    /// over a yellow (255,255,0) backdrop yields red (255,0,0) where they
    /// overlap; outside the source rect the backdrop is unchanged.
    #[test]
    fn blend_multiply_darkens() {
        let yellow = Color { r: 255, g: 255, b: 0, a: 255 };
        let magenta = Color { r: 255, g: 0, b: 255, a: 255 };
        let cmds = vec![
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: yellow },
            DisplayCommand::PushBlendMode { mode: crate::BlendMode::Multiply },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 32.0, 32.0), color: magenta },
            DisplayCommand::PopBlendMode,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        // Overlap: multiply(yellow, magenta) = (255,0,0).
        assert_eq!(px(&img, 10, 10), (255, 0, 0, 255), "multiply yields red over overlap");
        // Outside the source rect: untouched yellow backdrop.
        assert_eq!(px(&img, 50, 50), (255, 255, 0, 255), "backdrop untouched outside source");
    }

    /// `mix-blend-mode: normal` is plain source-over: an opaque blue fill on
    /// white paints solid blue, identical to no blend mode at all.
    #[test]
    fn blend_normal_is_source_over() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let cmds = vec![
            DisplayCommand::PushBlendMode { mode: crate::BlendMode::Normal },
            DisplayCommand::FillRect { rect: rect(10.0, 10.0, 20.0, 20.0), color: blue },
            DisplayCommand::PopBlendMode,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        assert_eq!(px(&img, 20, 20), (0, 0, 255, 255), "normal blend = source-over");
        assert_eq!(px(&img, 50, 50), (255, 255, 255, 255), "exterior stays white");
    }

    /// `mix-blend-mode: difference` is `|s − d|` per channel. A red (255,0,0)
    /// layer over the white (255,255,255) background yields cyan (0,255,255).
    #[test]
    fn blend_difference_inverts() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let cmds = vec![
            DisplayCommand::PushBlendMode { mode: crate::BlendMode::Difference },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 32.0, 32.0), color: red },
            DisplayCommand::PopBlendMode,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        // |red − white| = (0,255,255) cyan.
        assert_eq!(px(&img, 10, 10), (0, 255, 255, 255), "difference of red over white = cyan");
    }

    /// `filter: blur` spreads ink past the source rect edge. A black square
    /// inside a `PushFilter { Blur }` group leaks non-white pixels into the
    /// neighbouring band that the unblurred fill never touched, and softens the
    /// rect's own interior corner toward grey.
    #[test]
    fn filter_blur_spreads_ink() {
        let black = Color { r: 0, g: 0, b: 0, a: 255 };
        let cmds = vec![
            DisplayCommand::PushFilter { filters: vec![FilterFn::Blur(4.0)], bounds: None },
            DisplayCommand::FillRect { rect: rect(20.0, 20.0, 24.0, 24.0), color: black },
            DisplayCommand::PopFilter,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        // Just outside the original right edge (x=44): unblurred would be pure
        // white; blur leaks darkness here.
        let (r, _, _, _) = px(&img, 46, 32);
        assert!(r < 250, "blur leaks ink past the rect edge (r={r})");
        // Far from the rect stays white.
        assert_eq!(px(&img, 60, 60), (255, 255, 255, 255), "far corner stays white");
    }

    /// `filter: blur(0)` (radius rounds to 0) is an identity: the fill composites
    /// exactly as if no filter were present.
    #[test]
    fn filter_blur_zero_is_identity() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let cmds = vec![
            DisplayCommand::PushFilter { filters: vec![FilterFn::Blur(0.0)], bounds: None },
            DisplayCommand::FillRect { rect: rect(10.0, 10.0, 20.0, 20.0), color: blue },
            DisplayCommand::PopFilter,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        assert_eq!(px(&img, 20, 20), (0, 0, 255, 255), "blur(0) leaves the fill intact");
        assert_eq!(px(&img, 50, 50), (255, 255, 255, 255), "exterior stays white");
    }

    /// `filter: grayscale(1)` collapses a saturated colour to its luminance.
    /// Pure red (1,0,0) → luma 0.2126 → ~54 on all three channels.
    #[test]
    fn filter_grayscale_full() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let cmds = vec![
            DisplayCommand::PushFilter { filters: vec![FilterFn::Grayscale(1.0)], bounds: None },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 32.0, 32.0), color: red },
            DisplayCommand::PopFilter,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        let (r, g, b, a) = px(&img, 10, 10);
        assert_eq!(a, 255);
        assert_eq!(r, g, "grayscale equalises channels");
        assert_eq!(g, b, "grayscale equalises channels");
        assert!((50..=58).contains(&r), "red luma ≈ 54 (got {r})");
    }

    /// `filter: invert(1)` flips each channel. Black (0,0,0) → white (255,255,255).
    #[test]
    fn filter_invert_full() {
        let black = Color { r: 0, g: 0, b: 0, a: 255 };
        let cmds = vec![
            DisplayCommand::PushFilter { filters: vec![FilterFn::Invert(1.0)], bounds: None },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 32.0, 32.0), color: black },
            DisplayCommand::PopFilter,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        assert_eq!(px(&img, 10, 10), (255, 255, 255, 255), "invert(black) = white");
    }

    /// `backdrop-filter: grayscale(1)` desaturates the content already painted
    /// behind the element, clipped to the element's border box. A red backdrop
    /// turns grey only inside `bounds`; outside `bounds` it stays red.
    #[test]
    fn backdrop_filter_grayscale_filters_backdrop_in_bounds() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let cmds = vec![
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: red },
            DisplayCommand::PushBackdropFilter {
                filters: vec![FilterFn::Grayscale(1.0)],
                bounds: rect(0.0, 0.0, 32.0, 32.0),
            },
            DisplayCommand::PopBackdropFilter,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        let (r, g, b, a) = px(&img, 10, 10);
        assert_eq!(a, 255);
        assert_eq!(r, g, "inside bounds: grayscale equalises channels");
        assert_eq!(g, b, "inside bounds: grayscale equalises channels");
        assert!((50..=58).contains(&r), "red luma ≈ 54 inside bounds (got {r})");
        assert_eq!(px(&img, 50, 50), (255, 0, 0, 255), "outside bounds backdrop stays red");
    }

    /// `backdrop-filter: invert(1)` flips the backdrop within `bounds`. A black
    /// backdrop becomes white inside the element's border box, stays black
    /// outside it.
    #[test]
    fn backdrop_filter_invert_filters_backdrop_in_bounds() {
        let black = Color { r: 0, g: 0, b: 0, a: 255 };
        let cmds = vec![
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: black },
            DisplayCommand::PushBackdropFilter {
                filters: vec![FilterFn::Invert(1.0)],
                bounds: rect(0.0, 0.0, 32.0, 32.0),
            },
            DisplayCommand::PopBackdropFilter,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        assert_eq!(px(&img, 10, 10), (255, 255, 255, 255), "inside bounds: invert(black) = white");
        assert_eq!(px(&img, 50, 50), (0, 0, 0, 255), "outside bounds backdrop stays black");
    }

    /// `backdrop-filter: blur` softens a sharp backdrop edge. With a black/white
    /// split at x=32 and a blur covering the whole frame, the column at the edge
    /// turns mid-grey while the far interior stays close to its original colour.
    #[test]
    fn backdrop_filter_blur_softens_backdrop_edge() {
        let black = Color { r: 0, g: 0, b: 0, a: 255 };
        let cmds = vec![
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 32.0, 64.0), color: black },
            DisplayCommand::PushBackdropFilter {
                filters: vec![FilterFn::Blur(4.0)],
                bounds: rect(0.0, 0.0, 64.0, 64.0),
            },
            DisplayCommand::PopBackdropFilter,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        // At the edge the blur mixes black and white toward grey.
        let (r, _, _, _) = px(&img, 32, 32);
        assert!((40..=215).contains(&r), "edge column blurred toward grey (r={r})");
        // Far inside the black half stays dark.
        let (rl, _, _, _) = px(&img, 2, 32);
        assert!(rl < 80, "left interior stays near black (r={rl})");
        // Far inside the white half stays light.
        let (rr, _, _, _) = px(&img, 62, 32);
        assert!(rr > 200, "right interior stays near white (r={rr})");
    }

    /// `mask-image: linear-gradient(to bottom, black, transparent)` fades the
    /// element by the gradient's *alpha*. A blue box masked top→bottom stays
    /// solid blue where the mask is opaque (top) and disappears toward the
    /// background where the mask is transparent (bottom).
    #[test]
    fn mask_linear_alpha_gradient_fades_box() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let black = Color { r: 0, g: 0, b: 0, a: 255 };
        let clear = Color { r: 0, g: 0, b: 0, a: 0 };
        let cmds = vec![
            DisplayCommand::PushMaskLinearGradient {
                rect: rect(0.0, 0.0, 40.0, 40.0),
                angle_deg: 180.0, // CSS "to bottom"
                stops: vec![
                    GradientStop { color: black, position: None },
                    GradientStop { color: clear, position: None },
                ],
                repeating: false,
            },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 40.0, 40.0), color: blue },
            DisplayCommand::PopMask,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        // Top: mask alpha ≈ 1 → solid blue.
        let (tr, tg, tb, ta) = px(&img, 20, 1);
        assert!(tr < 20 && tg < 20 && tb > 250 && ta == 255, "top stays blue, got ({tr},{tg},{tb},{ta})");
        // Bottom: mask alpha ≈ 0 → faded to the white background.
        let (br, bg, bb, _) = px(&img, 20, 38);
        assert!(br > 200 && bg > 200 && bb > 200, "bottom fades to white, got ({br},{bg},{bb})");
    }

    /// `mask-image: radial-gradient(black, transparent)` reveals the element at
    /// the centre (opaque mask) and hides it toward the corners (transparent
    /// mask).
    #[test]
    fn mask_radial_reveals_center_hides_corner() {
        let green = Color { r: 0, g: 200, b: 0, a: 255 };
        let black = Color { r: 0, g: 0, b: 0, a: 255 };
        let clear = Color { r: 0, g: 0, b: 0, a: 0 };
        let cmds = vec![
            DisplayCommand::PushMaskRadialGradient {
                rect: rect(0.0, 0.0, 40.0, 40.0),
                center_x_pct: 0.5,
                center_y_pct: 0.5,
                stops: vec![
                    GradientStop { color: black, position: None },
                    GradientStop { color: clear, position: None },
                ],
                repeating: false,
            },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 40.0, 40.0), color: green },
            DisplayCommand::PopMask,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        // Centre: mask opaque → green shows.
        let (cr, cg, cb, ca) = px(&img, 20, 20);
        assert!(cr < 10 && cg > 180 && cb < 10 && ca == 255, "centre shows green, got ({cr},{cg},{cb},{ca})");
        // Corner: mask transparent → faded to white.
        let (kr, kg, kb, _) = px(&img, 1, 1);
        assert!(kr > 200 && kg > 200 && kb > 200, "corner fades to white, got ({kr},{kg},{kb})");
    }

    /// BUG-052: thin border with sub-pixel position must not panic.
    ///
    /// `anti_alias: true` on tiny-skia `fill_rect` trips a `debug_assert!(false)`
    /// in `hairline_aa::fill_dot8` when the fixed-point inner span rounds to zero
    /// (which happens for thin rects at sub-pixel offsets). `anti_alias: false`
    /// avoids the AA path — axis-aligned solid border quads don't need AA (the
    /// GPU renderer draws them without per-edge AA anyway).
    #[test]
    fn draw_border_thin_subpixel_no_panic() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let cmds = vec![DisplayCommand::DrawBorder {
            rect: rect(10.3, 10.7, 50.0, 50.0),
            widths: [0.5, 0.5, 0.5, 0.5],
            colors: [red; 4],
            styles: [lumen_layout::BorderStyle::Solid; 4],
            radii: CornerRadii::default(),
        }];
        // Pre-fix this would panic via debug_assert! in tiny-skia hairline_aa::fill_dot8.
        let img = rasterize_cpu(128, 128, &cmds, 0.0, 0.0).expect("rasterize");
        assert_eq!(px(&img, 64, 64), (255, 255, 255, 255), "interior stays white");
    }

    /// Dashed border: top edge must have visible dashes (some red pixels) with
    /// gaps between them (some white pixels). Interior must remain white.
    #[test]
    fn draw_border_dashed_top_has_gaps() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        // 100px wide box, 4px top dash border; all other sides None.
        let cmds = vec![DisplayCommand::DrawBorder {
            rect: rect(10.0, 10.0, 100.0, 60.0),
            widths: [4.0, 0.0, 0.0, 0.0],
            colors: [red, red, red, red],
            styles: [
                lumen_layout::BorderStyle::Dashed,
                lumen_layout::BorderStyle::None,
                lumen_layout::BorderStyle::None,
                lumen_layout::BorderStyle::None,
            ],
            radii: CornerRadii::default(),
        }];
        let img = rasterize_cpu(200, 100, &cmds, 0.0, 0.0).expect("rasterize");
        // Interior must be white.
        assert_eq!(px(&img, 60, 40), (255, 255, 255, 255), "interior white");
        // The top edge (y=11) should have at least one red pixel and one white pixel
        // to confirm dashes (not a solid band) are drawn.
        let has_red: bool = (10..110).any(|x| {
            let (r, ..) = px(&img, x, 11);
            r > 200
        });
        let has_gap: bool = (10..110).any(|x| {
            let (r, g, b, _) = px(&img, x, 11);
            r > 200 && g > 200 && b > 200
        });
        assert!(has_red, "dashed top border: no red pixels found on top edge");
        assert!(has_gap, "dashed top border: no gaps (white pixels) found on top edge");
    }

    /// Dotted border: left vertical edge must have visible dots with gaps.
    #[test]
    fn draw_border_dotted_left_has_gaps() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        // 60px tall box, 4px left dotted border.
        let cmds = vec![DisplayCommand::DrawBorder {
            rect: rect(10.0, 10.0, 80.0, 60.0),
            widths: [0.0, 0.0, 0.0, 4.0],
            colors: [blue; 4],
            styles: [
                lumen_layout::BorderStyle::None,
                lumen_layout::BorderStyle::None,
                lumen_layout::BorderStyle::None,
                lumen_layout::BorderStyle::Dotted,
            ],
            radii: CornerRadii::default(),
        }];
        let img = rasterize_cpu(200, 100, &cmds, 0.0, 0.0).expect("rasterize");
        // Interior must be white.
        assert_eq!(px(&img, 60, 40), (255, 255, 255, 255), "interior white");
        // Left edge (x=11) should have at least one blue and one white pixel.
        let has_blue: bool = (10..70).any(|y| {
            let (.., b, _) = px(&img, 11, y);
            b > 200
        });
        let has_gap: bool = (10..70).any(|y| {
            let (r, g, b, _) = px(&img, 11, y);
            r > 200 && g > 200 && b > 200
        });
        assert!(has_blue, "dotted left border: no blue pixels found");
        assert!(has_gap, "dotted left border: no gaps (white pixels) found");
    }

    /// Dashed border: `BorderStyle::None` sides render nothing (zero colored pixels).
    #[test]
    fn draw_border_none_style_renders_nothing() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let cmds = vec![DisplayCommand::DrawBorder {
            rect: rect(10.0, 10.0, 80.0, 60.0),
            widths: [4.0, 4.0, 4.0, 4.0],
            colors: [red; 4],
            styles: [lumen_layout::BorderStyle::None; 4],
            radii: CornerRadii::default(),
        }];
        let img = rasterize_cpu(200, 100, &cmds, 0.0, 0.0).expect("rasterize");
        // All pixels should remain white.
        let colored = (0..200u32)
            .flat_map(|x| (0..100u32).map(move |y| (x, y)))
            .any(|(x, y)| px(&img, x, y).0 < 200);
        assert!(!colored, "BorderStyle::None must render nothing");
    }

    /// BUG-175: a uniform solid border with border-radius must follow the radius
    /// instead of forming a square frame. The extreme box corner is outside the
    /// rounded outline (stays white), the middle of each edge carries the border
    /// colour, and the interior (padding box) stays empty (the ring has a hole).
    #[test]
    fn draw_border_rounded_corner_is_not_square() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        // 60×60 box at (10,10), 6px uniform solid border, radius 20px, no bg.
        let r = 20.0;
        let cmds = vec![DisplayCommand::DrawBorder {
            rect: rect(10.0, 10.0, 60.0, 60.0),
            widths: [6.0, 6.0, 6.0, 6.0],
            colors: [red; 4],
            styles: [lumen_layout::BorderStyle::Solid; 4],
            radii: CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r },
        }];
        let img = rasterize_cpu(100, 100, &cmds, 0.0, 0.0).expect("rasterize");

        // Extreme top-left box corner: rounded outline curves away → white.
        // Pre-fix (square border) this pixel was solid red.
        let (cr, cg, cb, _) = px(&img, 12, 12);
        assert!(cr > 200 && cg > 200 && cb > 200,
            "rounded corner must be empty, got ({cr},{cg},{cb})");
        // Middle of the top edge sits inside the border band → red.
        let (tr_, tg, tb, _) = px(&img, 40, 12);
        assert!(tr_ > 200 && tg < 60 && tb < 60,
            "top edge must be border colour, got ({tr_},{tg},{tb})");
        // Middle of the left edge → red.
        let (lr, lg, lb, _) = px(&img, 12, 40);
        assert!(lr > 200 && lg < 60 && lb < 60,
            "left edge must be border colour, got ({lr},{lg},{lb})");
        // Interior (padding box centre): ring has a hole → white.
        assert_eq!(px(&img, 40, 40), (255, 255, 255, 255), "interior stays white");
    }

    /// `mask-image: url(...)` has no decoded source on the deterministic CPU path,
    /// so the mask is a no-op (alpha = 1.0 everywhere) — the element composites
    /// unchanged, mirroring the GPU fallback for an unregistered mask image.
    #[test]
    fn mask_image_no_source_is_noop() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let cmds = vec![
            DisplayCommand::PushMaskImage {
                rect: rect(10.0, 10.0, 20.0, 20.0),
                src: "missing.png".to_string(),
                size: lumen_layout::BackgroundSize::Auto,
                position: lumen_layout::ObjectPosition::background_initial(),
                repeat: lumen_layout::BackgroundRepeat::NoRepeat,
                image_rendering: lumen_layout::ImageRendering::Auto,
            },
            DisplayCommand::FillRect { rect: rect(10.0, 10.0, 20.0, 20.0), color: blue },
            DisplayCommand::PopMask,
        ];
        let img = rasterize_cpu(64, 64, &cmds, 0.0, 0.0).expect("rasterize");
        assert_eq!(px(&img, 20, 20), (0, 0, 255, 255), "no mask source → element unchanged");
        assert_eq!(px(&img, 50, 50), (255, 255, 255, 255), "exterior stays white");
    }
}
