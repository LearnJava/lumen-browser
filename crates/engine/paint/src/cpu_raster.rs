//! CPU-based rasterization using tiny-skia for deterministic pixel output on CI.
//!
//! Available only with feature="cpu-render"; no GPU dependencies, fully deterministic
//! across Windows/macOS/Linux.

use lumen_image::Image;
use lumen_layout::{
    BackgroundRepeat, BackgroundSize, BorderStyle, Color, FilterFn, GradientStop, ObjectFit,
    ObjectPosition,
};
use lumen_layout::style::TextOrientation;
use crate::border_bevel::paint_bevel_sides;
use crate::dash_math::{dashed_border_offsets, dotted_border_offsets};
use crate::gradient_math::{atan2_det, resolve_stop_positions, sample_gradient_color};
use crate::matrix_util::mat4_to_2d_affine;
use crate::{DisplayCommand, CornerRadii};
use crate::display_list::{ResolvedClipShape, bg_tile_geometry, extend_start_to_paint_area};
use lumen_core::geom::Rect;
use lumen_core::FontProvider;
use crate::cpu_font_resolve::resolve_face_candidates;

pub(crate) mod text_stroke;

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
    /// BUG-1290 — `Transform` для проективной матрицы (`perspective`,
    /// `rotateX/Y`): tiny-skia умеет только аффинное, поэтому слой уходит на
    /// родителя обратным выборщиком по гомографии ([`crate::cpu_projective`]).
    /// `clip` — внешний клип в координатах родителя: внутри слоя он сброшен
    /// (обратной проекцией его не вернуть), поэтому режет сам композит.
    Projective { h: crate::cpu_projective::Homography, clip: Option<Rect> },
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
    ///
    /// `bounds` — border box элемента (CSS px): bbox для `url(#id)` на SVG-фильтр
    /// ([`crate::svg_filter`]).
    Filter { filters: Vec<FilterFn>, bounds: Rect },
    /// CSS Masking L1 §4 — `mask-image` (`PushMask*`/`PopMask`). The element's
    /// fully-composited subtree renders into a transparent full-size layer; on
    /// close the mask is evaluated per-pixel (the gradient's alpha channel —
    /// mask-mode: alpha, mirroring the GPU `MASK_COMPOSITE_SHADER` `c.a * m.a`)
    /// and multiplied into the layer's alpha before it is composited down with
    /// plain `SourceOver`.
    Mask(MaskSpec),
    /// LIB-9 — CSS Masking L1 §5 `PushMaskLayer`/`PopMaskLayer`: an SVG
    /// `<mask>` element's *content* renders into the layer (not the masked
    /// element's own content, unlike every other variant above); on close it
    /// is read as a per-pixel mask value (`mode`) and multiplied into the
    /// layer **below** in place, within `rect` only — the below layer already
    /// holds the masked shape's own paint (emitted before `PushMaskLayer`, at
    /// the same level via the caller's isolating `PushOpacity`). See
    /// `composite_mask_layer_rendered`.
    MaskLayer {
        /// Border-box of the masked element, in page px. Pixels outside it
        /// are left unmodified.
        rect: Rect,
        /// Alpha (mask layer's own alpha channel) or luminance (weighted RGB
        /// × alpha) — mirrors the wgpu `MASK_LAYER_SHADER_SRC` `fs_alpha`/`fs_luma`.
        mode: crate::display_list::MaskMode,
    },
    /// BUG-140 — `clip-path` basic-shape (`PushClipPath`/`PopClip`). The
    /// element's subtree renders into a transparent full-size layer (page
    /// coordinates, pre-transform — the command is emitted inside the
    /// element's `PushTransform`, so the enclosing transform layer carries
    /// the clipped result); on close the shape's anti-aliased coverage is
    /// rasterised (tiny-skia path fill — deterministic) and multiplied into
    /// the layer's alpha before plain `SourceOver` compositing.
    ClipShape(ResolvedClipShape),
    /// BUG-249 — скруглённый клип (`overflow:hidden` + `border-radius`,
    /// `PushClipRoundedRect`/`PopClip`). Как `ClipShape`, но форма — rounded-rect
    /// `rect`/`radii` (`[tl, tr, br, bl]` px). Раньше клип был прямоугольным
    /// (square scissor) — углы детей расходились с Edge (TEST-101). Subtree
    /// рендерится в full-size слой, на закрытии умножается на анти-алиасное
    /// покрытие скруглённого контура.
    ClipRoundedRect {
        /// Клип-прямоугольник (padding-box) в page-координатах.
        rect: Rect,
        /// Радиусы углов `[tl, tr, br, bl]` в px.
        radii: [f32; 4],
    },
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
    /// `PushMaskImage` whose `src` has no decoded pixels; the GPU renderer
    /// likewise composites with alpha = 1.0 when the mask image is not registered.
    None,
    /// `url(...)` mask: the decoded image, already tiled per `mask-size` /
    /// `mask-position` / `mask-repeat` into a full-canvas pixmap when the mask
    /// group opened; its alpha channel is the mask value.
    Raster(tiny_skia::Pixmap),
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

/// Axis-aligned draw bounds `(left, top, right, bottom)` in page px — the
/// shape produced by `rect_bounds` / `vertices_bounds` / `contours_bounds`.
type DrawBounds = (f32, f32, f32, f32);

/// One off-screen pixmap on the group-layer stack plus the running
/// axis-aligned bounding box of everything drawn into it.
///
/// The pixmap is always full-canvas (page coordinates), so nested groups, clip
/// masks and composite ops need no coordinate translation. `dirty` tracks the
/// union bbox `(left, top, right, bottom)` of every draw targeted at this
/// layer — a superset of the actual ink (draws mark their geometric bounds;
/// anti-aliasing cannot bleed beyond them by more than ~1px, absorbed by the
/// crop slack in [`composite_filter_layer`]). BUG-267: `PopFilter` /
/// `PushBackdropFilter` use it to blur and composite only the inked window
/// instead of the whole canvas — the full-canvas Gaussian blur per shadow is
/// what made shadow-heavy pages take minutes on the CPU path.
struct CpuLayer {
    /// Full-canvas draw target for this group (page coordinates).
    pm: tiny_skia::Pixmap,
    /// Union bbox `(left, top, right, bottom)` in page px of all draws into
    /// `pm`; `None` while the layer is untouched (fully transparent).
    dirty: Option<DrawBounds>,
}

impl CpuLayer {
    /// Wrap a freshly-allocated (transparent) pixmap with an empty dirty box.
    fn new(pm: tiny_skia::Pixmap) -> Self {
        Self { pm, dirty: None }
    }

    /// Union draw bounds `(left, top, right, bottom)` into the dirty box.
    fn mark(&mut self, bounds: DrawBounds) {
        self.dirty = Some(match self.dirty {
            None => bounds,
            Some((l, t, r, b)) => (
                l.min(bounds.0),
                t.min(bounds.1),
                r.max(bounds.2),
                b.max(bounds.3),
            ),
        });
    }
}

/// Rasterize display commands to an image using tiny-skia (CPU only,
/// deterministic). `DrawText` always resolves fonts as before this function's
/// [`FONTLOAD-18`](crate::cpu_font_resolve) sibling was added — see
/// [`rasterize_cpu_with_fonts`] for the page-`@font-face`-aware entry point.
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn rasterize_cpu(
    width: u32,
    height: u32,
    commands: &[DisplayCommand],
    images: &[(String, std::sync::Arc<Image>)],
    scroll_x: f32,
    scroll_y: f32,
) -> Result<Image, Box<dyn std::error::Error>> {
    rasterize_cpu_with_fonts(width, height, commands, images, scroll_x, scroll_y, None, None)
}

/// Same as [`rasterize_cpu`], but `DrawText` resolves `@font-face` bytes and CSS Fonts L4 §14
/// overrides through `font_provider` (FONTLOAD-18, [`crate::cpu_font_resolve::resolve_face`]),
/// and the surface clears to `canvas_background` (`lumen_layout::canvas_background_color` —
/// the only carrier of `<body>`'s propagated background since BUG-1103; `None` → UA white).
#[allow(clippy::expect_used, clippy::too_many_arguments)]  // expect_used унаследовано, docs/lint-policy.md §10
pub(crate) fn rasterize_cpu_with_fonts(
    width: u32,
    height: u32,
    commands: &[DisplayCommand],
    images: &[(String, std::sync::Arc<Image>)],
    _scroll_x: f32,
    _scroll_y: f32,
    font_provider: Option<&dyn FontProvider>,
    canvas_background: Option<Color>,
) -> Result<Image, Box<dyn std::error::Error>> {
    // Decoded `<img>` pixels keyed by `src`, supplied by the shell's headless
    // render pass (`render_source_to_png`). Lets `DrawImage`/`LazyImageSlot`
    // paint the real picture instead of a grey placeholder (BUG-221, TEST-18).
    // BUG-272 срез 17: `images` carries `Arc<Image>`; deref to `&Image` for the map.
    let image_map: std::collections::HashMap<&str, &Image> =
        images.iter().map(|(s, img)| (s.as_str(), img.as_ref())).collect();

    let clear = canvas_background.unwrap_or(Color::WHITE);
    let pixmap = rasterize_cpu_pixmap(width, height, commands, &image_map, font_provider, Some(clear))?;
    let data = pixmap.data().to_vec();
    Ok(Image {
        width,
        height,
        format: lumen_image::PixelFormat::Rgba8,
        data,
        icc_profile: None,
        vector_source: None,
    })
}

/// Тело растеризатора: рисует `commands` на полотно `width`×`height` и отдаёт
/// пиксели. `clear == None` — прозрачное полотно (слой группы `PushTransform`,
/// BUG-1288), иначе заливка этим цветом.
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
fn rasterize_cpu_pixmap(
    width: u32,
    height: u32,
    commands: &[DisplayCommand],
    image_map: &std::collections::HashMap<&str, &Image>,
    font_provider: Option<&dyn FontProvider>,
    clear: Option<Color>,
) -> Result<tiny_skia::Pixmap, Box<dyn std::error::Error>> {
    use tiny_skia::Pixmap;
    // `-webkit-text-stroke`: обводка глифов штрихуется как `DrawSvgStroke`.
    let commands = &*crate::text_stroke::expand_all(commands, font_provider);
    let mut base = Pixmap::new(width, height)
        .ok_or("Failed to create pixmap")?;

    if let Some(clear) = clear {
        base.fill(tiny_skia::Color::from_rgba8(clear.r, clear.g, clear.b, clear.a));
    }

    // Off-screen layer stack for group effects: group opacity (`PushOpacity` /
    // `PopOpacity`, emitted for `opacity < 1`) and 2D transforms (`PushTransform`
    // / `PopTransform`). `layers[0]` is the white base; each open group pushes a
    // fully-transparent full-size layer that becomes the active draw target. On
    // the matching pop the top layer is composited onto the layer below per its
    // `LayerComposite` op (alpha-blend for opacity, affine `draw_pixmap` for
    // transform). Full-size layers keep the page coordinate space and clip masks
    // valid without translation. Each layer carries its dirty bbox (BUG-267) so
    // filter groups blur/composite only the inked window.
    let mut layers: Vec<CpuLayer> = vec![CpuLayer::new(base)];
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
    // BUG-424: клип-стек, сохранённый на каждый `PushTransform`. Содержимое
    // слоя трансформации рисуется в ДО-трансформных координатах (аффинность
    // применяется на композите), а внешние клипы заданы в координатах
    // документа — поэтому внутри слоя они пересчитываются обратной матрицей,
    // а на `PopTransform` восстанавливаются отсюда.
    let mut clip_saves: Vec<Vec<Rect>> = Vec::new();
    // One flag per open `PushScrollLayer`: did it open a translated layer (non-zero scroll).
    let mut scroll_layer_shifted: Vec<bool> = Vec::new();
    // BUG-1290: матрица `A`, действующая на прямых потомков открытой группы
    // `PushTransform`. Слой потомка композитится в слой родителя матрицей
    // `H(A)⁻¹·H(A·M)`, чтобы после композита родителя (`H(A)`) вышло
    // `H(A·M)`. Ненулевая `A` — только у обёртки `perspective` (она на
    // плоскости z = 0 тождественна, но перспектива должна дойти до
    // `translateZ`/`rotateX` ребёнка); у остальных групп `A` — единичная:
    // без `preserve-3d` потомки плоские и проецируются каждый в плоскость
    // своего родителя (CSS Transforms L2 §6), цепочка не перемножается.
    let mut xf_acc: Vec<lumen_layout::Mat4> = Vec::new();

    // BUG-1288: индекс команды, до которой цикл пропускает уже отрисованное
    // содержимое группы `PushTransform` (включая её `PopTransform`).
    let mut skip_until = 0usize;
    for (cmd_idx, cmd) in commands.iter().enumerate() {
        if cmd_idx < skip_until {
            continue;
        }
        match cmd {
            DisplayCommand::FillRect { rect, color } => {
                let b = rect_bounds(rect);
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), b);
                let layer = layers.last_mut().expect("base layer");
                rasterize_fill_rect(&mut layer.pm, rect, color, c)?;
                layer.mark(b);
            }
            DisplayCommand::FillRoundedRect { rect, color, radii } => {
                let b = rect_bounds(rect);
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), b);
                let layer = layers.last_mut().expect("base layer");
                rasterize_fill_rounded_rect(&mut layer.pm, rect, color, radii, c)?;
                layer.mark(b);
            }
            DisplayCommand::DrawBorder { rect, widths, colors, styles, radii } => {
                let b = rect_bounds(rect);
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), b);
                let layer = layers.last_mut().expect("base layer");
                rasterize_draw_border(&mut layer.pm, rect, widths, colors, styles, radii, c)?;
                layer.mark(b);
            }
            DisplayCommand::DrawOutline { rect, width, style: _, color, offset } => {
                // Outline expands the rect by `offset` on every side.
                let b = (
                    rect.x - offset,
                    rect.y - offset,
                    rect.x + rect.width + offset,
                    rect.y + rect.height + offset,
                );
                // The stroke band extends `width` beyond the offset rect: that outer box,
                // not the offset rect, decides whether the clip can be skipped.
                let outer = (b.0 - width, b.1 - width, b.2 + width, b.3 + width);
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), outer);
                let layer = layers.last_mut().expect("base layer");
                rasterize_draw_outline(&mut layer.pm, rect, *width, color, *offset, c)?;
                layer.mark(outer);
            }
            DisplayCommand::DrawLinearGradient { rect, angle_deg, stops, repeating } => {
                let b = rect_bounds(rect);
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), b);
                let layer = layers.last_mut().expect("base layer");
                rasterize_linear_gradient(&mut layer.pm, rect, *angle_deg, stops, *repeating, c)?;
                layer.mark(b);
            }
            DisplayCommand::DrawRadialGradient {
                rect, center_x_pct, center_y_pct, radius_x, radius_y, stops, repeating,
            } => {
                let b = rect_bounds(rect);
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), b);
                let layer = layers.last_mut().expect("base layer");
                rasterize_radial_gradient(
                    &mut layer.pm, rect, *center_x_pct, *center_y_pct,
                    *radius_x, *radius_y, stops, *repeating, c,
                )?;
                layer.mark(b);
            }
            DisplayCommand::DrawConicGradient {
                rect, center_x_pct, center_y_pct, from_angle_deg, stops, repeating,
            } => {
                let b = rect_bounds(rect);
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), b);
                let layer = layers.last_mut().expect("base layer");
                rasterize_conic_gradient(
                    &mut layer.pm, rect, *center_x_pct, *center_y_pct,
                    *from_angle_deg, stops, *repeating, c,
                )?;
                layer.mark(b);
            }
            DisplayCommand::DrawSvgPath { vertices, color } => {
                let b = vertices_bounds(vertices);
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), b);
                let layer = layers.last_mut().expect("base layer");
                rasterize_svg_path(&mut layer.pm, vertices, color, c)?;
                layer.mark(b);
            }
            DisplayCommand::DrawSvgFill { contours, color } => {
                let b = contours_bounds(contours);
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), b);
                let layer = layers.last_mut().expect("base layer");
                rasterize_svg_fill(&mut layer.pm, contours, color, c)?;
                layer.mark(b);
            }
            // BUG-247: the CPU rasteriser has no native stroker, so re-tessellate
            // the stroke contours into the same triangle soup the emitter used to
            // produce (`DrawSvgPath`) — CPU snapshots stay byte-identical while
            // the femtovg window gets seam-free native strokes.
            DisplayCommand::DrawSvgStroke { contours, color, params } => {
                let vertices = crate::svg_path::tessellate_stroke_ex(contours, params);
                if !vertices.is_empty() {
                    let b = vertices_bounds(&vertices);
                    let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), b);
                    let layer = layers.last_mut().expect("base layer");
                    rasterize_svg_path(&mut layer.pm, &vertices, color, c)?;
                    layer.mark(b);
                }
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
            DisplayCommand::PushClipRoundedRect { rect, radii } => {
                // BUG-249: скруглённый клип через off-screen слой + покрытие
                // (как PushClipPath/BUG-140), а не прямоугольный scissor — углы
                // детей следуют border-radius контейнера (TEST-101).
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create rounded-clip layer")?;
                layers.push(CpuLayer::new(layer));
                layer_ops.push(LayerComposite::ClipRoundedRect { rect: *rect, radii: *radii });
                clip_kinds.push(CpuClipKind::Shape);
            }
            // BUG-140: shape-клип (clip-path circle/ellipse/polygon) — слой
            // + альфа-покрытие формы на закрытии.
            DisplayCommand::PushClipPath { shape } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create clip-path layer")?;
                layers.push(CpuLayer::new(layer));
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
            // axis a mismatched `overflow` pair coerces to): a clip to `clip_rect`
            // plus a `(-scroll_x, -scroll_y)` translation of the content. A freshly
            // loaded page has both offsets at 0 and this is a plain clip; a page
            // script that scrolled a container (`el.scrollBy()` before the
            // snapshot) makes the content an off-screen group translated at
            // composite time, exactly like `PushTransform` — the clip is mapped
            // through the inverse so it still gates the pre-translation draws.
            DisplayCommand::PushScrollLayer { clip_rect: cr, scroll_x, scroll_y, .. } => {
                // BUG-1479: the scrolled content is drawn in its pre-scroll
                // coordinates, so a canvas-sized layer lost whatever lies below
                // or right of the canvas before the shift. Same remedy as the
                // `PushTransform` group above: a layer of the content's own
                // extent, composited with the scroll translation.
                if (*scroll_x != 0.0 || *scroll_y != 0.0)
                    && let Some(end) = matching_pop_scroll(commands, cmd_idx)
                {
                    let mut ambient = clip_stack.clone();
                    ambient.push(*cr);
                    let matrix = lumen_layout::Mat4::translation_2d(-*scroll_x, -*scroll_y);
                    if let GroupLayer::Rendered(group, group_t) = render_offcanvas_transform_group(
                        width, height, &matrix, &commands[cmd_idx + 1..end], clip_intersection(&ambient),
                        image_map, font_provider,
                    )? {
                        skip_until = end + 1;
                        if let Some(group) = group
                            && let Some(dst) = layers.last_mut()
                        {
                            let (gw, gh) = (group.width() as f32, group.height() as f32);
                            let src = CpuLayer { pm: group, dirty: Some((0.0, 0.0, gw, gh)) };
                            close_layer(dst, &src, &LayerComposite::Transform(group_t));
                        }
                        continue;
                    }
                }
                clip_stack.push(*cr);
                clip_rect = clip_intersection(&clip_stack);
                clip_mask = build_clip_mask(width, height, clip_rect);
                let shifted = *scroll_x != 0.0 || *scroll_y != 0.0;
                scroll_layer_shifted.push(shifted);
                if shifted {
                    let matrix = lumen_layout::Mat4::translation_2d(-*scroll_x, -*scroll_y);
                    let [a, b, c, d, e, f] = mat4_to_2d_affine(&matrix);
                    let t = tiny_skia::Transform::from_row(a, b, c, d, e, f);
                    let layer = tiny_skia::Pixmap::new(width, height)
                        .ok_or("Failed to create scroll layer")?;
                    layers.push(CpuLayer::new(layer));
                    layer_ops.push(LayerComposite::Transform(t));
                    clip_saves.push(clip_stack.clone());
                    if let Some(active) = clip_rect
                        && let Some(inv) = matrix.invert_2d_affine()
                    {
                        clip_stack = vec![transform_rect_bbox(active, &inv)];
                        clip_rect = clip_intersection(&clip_stack);
                        clip_mask = build_clip_mask(width, height, clip_rect);
                    }
                }
            }
            DisplayCommand::PopScrollLayer => {
                if scroll_layer_shifted.pop() == Some(true)
                    && let (Some(top), Some(op)) = (layers.pop(), layer_ops.pop())
                {
                    if let Some(saved) = clip_saves.pop() {
                        clip_stack = saved;
                    }
                    if let Some(dst) = layers.last_mut() {
                        close_layer(dst, &top, &op);
                    }
                }
                clip_stack.pop();
                clip_rect = clip_intersection(&clip_stack);
                clip_mask = build_clip_mask(width, height, clip_rect);
            }
            DisplayCommand::DrawImage { rect, src, object_fit, object_position, .. } => {
                let b = rect_bounds(rect);
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), b);
                let layer = layers.last_mut().expect("base layer");
                match image_map.get(src.as_str()) {
                    Some(img) => {
                        rasterize_image(&mut layer.pm, &snap_border_box(*rect), img, *object_fit, *object_position, c)?;
                    }
                    // FLEX-VWM-6: an undrawn `<canvas>` is transparent, not a grey box.
                    None if src.starts_with("canvas:") => {}
                    None => rasterize_image_placeholder(&mut layer.pm, rect, c)?,
                }
                layer.mark(b);
            }
            DisplayCommand::LazyImageSlot { rect, src, object_fit, object_position, .. } => {
                let b = rect_bounds(rect);
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), b);
                let layer = layers.last_mut().expect("base layer");
                match image_map.get(src.as_str()) {
                    Some(img) => {
                        rasterize_image(&mut layer.pm, &snap_border_box(*rect), img, *object_fit, *object_position, c)?;
                    }
                    None => rasterize_image_placeholder(&mut layer.pm, rect, c)?,
                }
                layer.mark(b);
            }
            // CSS Backgrounds L3 §3.3 — background-image url() (also the resolved
            // `image-set()` candidate). Tiled per background-size/position/repeat
            // (shared `bg_tile_geometry` with the femtovg backend), clipped to the
            // background painting area. Unregistered `src` (no decoded pixels) is
            // skipped silently, mirroring the GPU renderer (`renderer.rs`).
            DisplayCommand::DrawBackgroundImage {
                rect, origin_rect, src, size, position, repeat, ..
            } => {
                let b = rect_bounds(rect);
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), b);
                let layer = layers.last_mut().expect("base layer");
                if let Some(img) = image_map.get(src.as_str()) {
                    rasterize_background_image(
                        &mut layer.pm, rect, origin_rect, img, *size, position, *repeat, c,
                    )?;
                    layer.mark(b);
                }
            }
            // CSS Images L4 §4 — cross-fade(a, b, p): the two images are stretched
            // to fill `dest` and mixed as `(1−p)·a + p·b`. Both are opaque layers
            // of the *same* result, so `a` is laid down at full alpha and `b`
            // composited over it at `p` — source-over then yields exactly that
            // mix for opaque sources. Drawing `a` at `1−p` instead (BUG-101)
            // squared its weight and let the backdrop bleed through: the result
            // became `p·b + (1−p)²·a + p(1−p)·backdrop`, i.e. washed out and
            // darkened by whatever was underneath. Mirrors the wgpu shader's
            // `mix(a, b, t)`. Missing sources are skipped.
            DisplayCommand::DrawCrossFade { dest, src_a, src_b, progress } => {
                let b = rect_bounds(dest);
                let c = effective_clip(clip_mask.as_ref(), clip_rect.as_ref(), b);
                let layer = layers.last_mut().expect("base layer");
                let p = progress.clamp(0.0, 1.0);
                if let Some(img) = image_map.get(src_a.as_str()) {
                    blit_image_into_rect(&mut layer.pm, *dest, *dest, img, 1.0, c)?;
                    layer.mark(b);
                }
                if let Some(img) = image_map.get(src_b.as_str()) {
                    blit_image_into_rect(&mut layer.pm, *dest, *dest, img, p, c)?;
                    layer.mark(b);
                }
            }
            DisplayCommand::DrawText {
                rect, text, font_size, color, tab_size, font_weight, font_style,
                font_features, font_family, text_orientation, letter_spacing, word_spacing, ..
            } => {
                // Text uses the bundled Inter face by default (family ignored,
                // weight/style emulated as synthetic bold/italic); a real
                // `font_family` match happens against `font_provider`'s
                // registered `@font-face` faces when given (FONTLOAD-18), or
                // against the OS font index under the opt-in
                // `LUMEN_CPU_SYSTEM_FONTS` — see
                // `crate::cpu_font_resolve::resolve_face`. Clip is the active
                // rectangular `overflow` region, applied per glyph pixel.
                let layer = layers.last_mut().expect("base layer");
                if let Some(ink) = rasterize_text(
                    &mut layer.pm, rect, text, *font_size, color,
                    *tab_size, clip_rect.as_ref(), font_weight.0, *font_style,
                    font_features, font_family, font_provider, *text_orientation,
                    (crate::display_list::spacing_px(*letter_spacing), crate::display_list::spacing_px(*word_spacing)),
                )? {
                    layer.mark(ink);
                }
            }
            // CSS Color L3 §3.2 — `opacity < 1` renders the element's subtree as
            // an off-screen group, then alpha-blends it. Push a transparent
            // full-size layer that becomes the active draw target; draws between
            // here and the matching `PopOpacity` accumulate into it.
            DisplayCommand::PushOpacity { alpha, .. } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create opacity layer")?;
                layers.push(CpuLayer::new(layer));
                layer_ops.push(LayerComposite::Opacity(alpha.clamp(0.0, 1.0)));
            }
            // CSS Transforms L1 §13 — the box's subtree (own background/border +
            // children) renders as an off-screen group, then composites down
            // through the 2D affine. Like `PushOpacity`, push a transparent
            // full-size layer; the affine maps page coordinates and is applied at
            // composite time so geometry need not be transformed per-draw.
            DisplayCommand::PushTransform { matrix } => {
                let acc = xf_acc.last().copied().unwrap_or(lumen_layout::Mat4::IDENTITY);
                // BUG-1290: для 3D-матрицы — композит слоя как гомография.
                let projective = if matrix.is_2d_affine() {
                    None
                } else {
                    Some(crate::cpu_projective::homography(matrix))
                };
                // Обёртка `perspective` (тождество на z = 0) передаёт `P` детям;
                // иная группа плоская.
                let acc_next = if projective
                    .and_then(|h| crate::cpu_projective::as_affine(&h))
                    .is_some_and(|a| a == [1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
                {
                    acc.multiply(matrix)
                } else {
                    lumen_layout::Mat4::IDENTITY
                };
                let layer_h = projective.map(|own| {
                    use crate::cpu_projective::{homography, invert, mul};
                    let parent = homography(&acc);
                    let total = homography(&acc.multiply(matrix));
                    invert(&parent).map_or(own, |inv| mul(&inv, &total))
                });
                if let Some(h) = layer_h
                    && crate::cpu_projective::as_affine(&h).is_none()
                {
                    let layer = tiny_skia::Pixmap::new(width, height)
                        .ok_or("Failed to create transform layer")?;
                    layers.push(CpuLayer::new(layer));
                    layer_ops.push(LayerComposite::Projective { h, clip: clip_rect });
                    xf_acc.push(acc_next);
                    clip_saves.push(clip_stack.clone());
                    clip_stack.clear();
                    clip_rect = None;
                    clip_mask = None;
                    continue;
                }
                // Аффинный образ (в т.ч. `perspective` на плоскости z = 0
                // и `translateZ` под `perspective`) — обычный путь.
                let matrix_2d;
                let matrix = match layer_h.and_then(|h| crate::cpu_projective::as_affine(&h)) {
                    Some([a, b, c, d, e, f]) => {
                        matrix_2d = lumen_layout::Mat4::from_2d_affine(a, b, c, d, e, f);
                        &matrix_2d
                    }
                    None => matrix,
                };
                let [a, b, c, d, e, f] = mat4_to_2d_affine(matrix);
                let t = tiny_skia::Transform::from_row(a, b, c, d, e, f);
                // BUG-1288: содержимое, лежащее до трансформации вне полотна
                // (`left:900px` + `translate(-700px)`, повёрнутый блок с
                // `left:-50px`), в слой размером с полотно не помещается —
                // такую группу рисуем в отдельный слой по её собственному
                // габариту и композитим целиком.
                if let Some(end) = matching_pop_transform(commands, cmd_idx)
                    && let GroupLayer::Rendered(group, group_t) = render_offcanvas_transform_group(
                        width, height, matrix, &commands[cmd_idx + 1..end], clip_rect,
                        image_map, font_provider,
                    )?
                {
                    skip_until = end + 1;
                    if let Some(group) = group
                        && let Some(dst) = layers.last_mut()
                    {
                        let (gw, gh) = (group.width() as f32, group.height() as f32);
                        let src = CpuLayer { pm: group, dirty: Some((0.0, 0.0, gw, gh)) };
                        close_layer(dst, &src, &LayerComposite::Transform(group_t));
                    }
                    continue;
                }
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create transform layer")?;
                layers.push(CpuLayer::new(layer));
                layer_ops.push(LayerComposite::Transform(t));
                xf_acc.push(acc_next);
                // BUG-424: draws inside this layer use pre-transform coordinates,
                // so an ambient clip expressed in document space must be mapped
                // back through the inverse before it can gate them. Without this
                // the clip rejected the whole layer: a `<use>`-scaled SVG icon
                // (`viewBox="0 0 24 24"` painted at 14px) draws its path at local
                // 0…24 while the clip sat at the icon's document rect — disjoint,
                // so every stroke was culled and the icon rendered blank.
                clip_saves.push(clip_stack.clone());
                if let Some(active) = clip_rect
                    && let Some(inv) = matrix.invert_2d_affine()
                {
                    clip_stack = vec![transform_rect_bbox(active, &inv)];
                    clip_rect = clip_intersection(&clip_stack);
                    clip_mask = build_clip_mask(width, height, clip_rect);
                }
            }
            // CSS Compositing & Blending L1 §5 — `mix-blend-mode` on the box.
            // Like `PushOpacity`, the subtree renders into a transparent
            // full-size layer; the matching `PopBlendMode` composites it onto
            // the backdrop below with the CSS blend formula rather than plain
            // source-over, so the element blends against everything painted
            // beneath it within the stacking context.
            DisplayCommand::PushBlendMode { mode, .. } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create blend layer")?;
                layers.push(CpuLayer::new(layer));
                layer_ops.push(LayerComposite::Blend(map_blend_mode(*mode)));
            }
            // CSS Filter Effects L1 §4 — `filter` chain. Emitted by `walk` to
            // wrap box-shadow / text-shadow blur (`PushFilter { Blur(σ) }` around
            // the shadow FillRect / DrawText) and by the stacking-aware builder
            // for the element's own `filter` property. Like the other group
            // effects, the wrapped draws accumulate into a transparent full-size
            // layer; the matching `PopFilter` applies the chain to that layer and
            // composites it down. BUG-267: the emitter's `bounds` hint is ignored
            // in favour of the layer's tracked dirty box — the hint is the border
            // box for the element `filter` case and would under-cover overflowing
            // children, while the dirty box is a guaranteed ink superset.
            DisplayCommand::PushFilter { filters, bounds } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create filter layer")?;
                layers.push(CpuLayer::new(layer));
                layer_ops.push(LayerComposite::Filter {
                    filters: filters.clone(),
                    bounds: bounds.unwrap_or(Rect::ZERO),
                });
            }
            // CSS Masking L1 §4 — `mask-image`. Like `PushOpacity`, the element's
            // fully-composited subtree renders into a transparent full-size layer;
            // the matching `PopMask` evaluates the mask (the gradient's alpha) and
            // multiplies it into the layer's alpha before compositing down. Image
            // masks (`PushMaskImage`) have no decoded source on the CPU path, so
            // they record `MaskSpec::None` (composite unchanged), mirroring the GPU
            // fallback when the mask image is not registered.
            DisplayCommand::PushMaskImage { rect, src, size, position, repeat, .. } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create mask layer")?;
                layers.push(CpuLayer::new(layer));
                // The decoded image is tiled into a mask pixmap right away (the
                // image map is at hand here, not at `PopMask`); an unregistered
                // `src` leaves the layer unmasked like the GPU fallback.
                let spec = match image_map.get(src.as_str()) {
                    Some(img) => {
                        let mut mask = tiny_skia::Pixmap::new(width, height)
                            .ok_or("Failed to create mask image layer")?;
                        rasterize_background_image(
                            &mut mask, rect, rect, img, *size, position, *repeat, None,
                        )?;
                        MaskSpec::Raster(mask)
                    }
                    None => MaskSpec::None,
                };
                layer_ops.push(LayerComposite::Mask(spec));
            }
            DisplayCommand::PushMaskLinearGradient { rect, angle_deg, stops, repeating } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create mask layer")?;
                layers.push(CpuLayer::new(layer));
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
                layers.push(CpuLayer::new(layer));
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
                layers.push(CpuLayer::new(layer));
                layer_ops.push(LayerComposite::Mask(MaskSpec::Conic {
                    rect: *rect,
                    center_x_pct: *center_x_pct,
                    center_y_pct: *center_y_pct,
                    from_angle_deg: *from_angle_deg,
                    stops: stops.clone(),
                    repeating: *repeating,
                }));
            }
            // LIB-9 — SVG `<mask>` element content (`PushMaskLayer`/
            // `PopMaskLayer`, CSS Masking L1 §5). Opens a transparent layer for
            // the mask's *content* — the caller emits the masked shape's own
            // paint BEFORE this command (into the layer below, opened by its
            // own isolating `PushOpacity`), so `PopMaskLayer` reads down into
            // already-painted content rather than up into a copy of it.
            DisplayCommand::PushMaskLayer { rect, mode } => {
                let layer = tiny_skia::Pixmap::new(width, height)
                    .ok_or("Failed to create mask-content layer")?;
                layers.push(CpuLayer::new(layer));
                layer_ops.push(LayerComposite::MaskLayer { rect: *rect, mode: *mode });
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
            | DisplayCommand::PopMaskLayer
            | DisplayCommand::PopMask => {
                if let (Some(top), Some(op)) = (layers.pop(), layer_ops.pop()) {
                    // BUG-424: restore the document-space clip the matching
                    // `PushTransform` swapped for its local-space image.
                    if matches!(op, LayerComposite::Transform(_) | LayerComposite::Projective { .. })
                        && let Some(saved) = clip_saves.pop()
                    {
                        xf_acc.pop();
                        clip_stack = saved;
                        clip_rect = clip_intersection(&clip_stack);
                        clip_mask = build_clip_mask(width, height, clip_rect);
                    }
                    if let Some(dst) = layers.last_mut() {
                        close_layer(dst, &top, &op);
                    }
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
                apply_backdrop_filter(&mut target.pm, filters, bounds, width, height);
                // Blur can spread existing backdrop ink into previously-blank
                // pixels, but only within the written-back `bounds` region.
                target.mark(rect_bounds(bounds));
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
    Ok(layers.pop().expect("base layer").pm)
}

/// Composite an off-screen opacity layer `src` onto `dst` with group `alpha`.
///
/// Used by `PopOpacity`: the whole subtree was rendered into `src` (a full-size,
/// initially transparent pixmap), so blending it as a unit with a single alpha
/// reproduces CSS group opacity (the children are *not* individually faded).
/// tiny-skia's `draw_pixmap` blends premultiplied RGBA deterministically, so the
/// result is cross-OS bit-identical like the rest of the CPU path.
///
/// BUG-267 (slice D): only the layer's dirty window is cloned and blended —
/// outside it `src` is fully transparent, and `SourceOver` of a transparent
/// source leaves the backdrop bit-identical, so skipping those pixels changes
/// nothing. The integer-offset `draw_pixmap` with `Nearest` quality blends the
/// same source values onto the same destination pixels as the full-canvas pass.
fn composite_layer(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    alpha: f32,
    src_dirty: DrawBounds,
) {
    let Some((cx0, cy0, crop)) = crop_dirty_window(src, src_dirty) else {
        return;
    };
    let paint = tiny_skia::PixmapPaint {
        opacity: alpha.clamp(0.0, 1.0),
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    dst.draw_pixmap(
        cx0 as i32,
        cy0 as i32,
        crop.as_ref(),
        &paint,
        tiny_skia::Transform::identity(),
        None,
    );
}

/// Composite an off-screen group layer `src` onto `dst` per its `LayerComposite`
/// op. Used by `PopOpacity` / `PopTransform` (and the trailing balance loop).
///
/// BUG-267: an untouched group (`src.dirty == None`, fully transparent) is
/// skipped outright — every composite op leaves the backdrop bit-identical for
/// a transparent source (SourceOver adds nothing, all CSS blend modes reduce to
/// the backdrop, masks/clips only scale the source's zero alpha) — and the
/// closed group's dirty box is propagated into `dst` so nested filter groups
/// still see a correct ink superset.
fn close_layer(dst: &mut CpuLayer, src: &CpuLayer, op: &LayerComposite) {
    // An SVG filter can paint on an empty source (`feFlood`), so it alone runs on
    // an untouched layer.
    let has_svg = matches!(op, LayerComposite::Filter { filters, .. }
        if filters.iter().any(|f| matches!(f, FilterFn::Url(_))));
    let Some(src_dirty) = src.dirty.or(has_svg.then_some((0.0, 0.0, 0.0, 0.0))) else { return };
    match op {
        LayerComposite::Opacity(a) => {
            composite_layer(&mut dst.pm, &src.pm, *a, src_dirty);
            dst.mark(src_dirty);
        }
        // BUG-267: the transform composite is NOT cropped to the dirty window.
        // `draw_pixmap` under a non-trivial affine resamples with bilinear
        // filtering; baking the crop offset into the transform changes the f32
        // rounding of the composed matrix (and of its inverse used for source
        // sampling), so the crop output is not guaranteed bit-identical to the
        // full-canvas pass — unlike the integer-offset `Nearest` composites of
        // the other ops. Empty transform groups are still skipped above.
        LayerComposite::Transform(t) => {
            composite_transform_layer(&mut dst.pm, &src.pm, *t);
            dst.mark(transform_bounds(*t, src_dirty));
        }
        LayerComposite::Projective { h, clip } => {
            if let Some(written) =
                crate::cpu_projective::composite(&mut dst.pm, &src.pm, h, *clip, src_dirty)
            {
                dst.mark(written);
            }
        }
        LayerComposite::Blend(mode) => {
            composite_blend_layer(&mut dst.pm, &src.pm, *mode, src_dirty);
            dst.mark(src_dirty);
        }
        LayerComposite::Filter { filters, bounds } => {
            let written = if has_svg {
                composite_svg_filter_chain(&mut dst.pm, &src.pm, filters, *bounds, src_dirty)
            } else {
                composite_filter_layer(&mut dst.pm, &src.pm, filters, src_dirty)
            };
            if let Some(written) = written {
                dst.mark(written);
            }
        }
        LayerComposite::Mask(spec) => {
            composite_mask_layer(&mut dst.pm, &src.pm, spec, src_dirty);
            dst.mark(src_dirty);
        }
        // LIB-9: unlike every other arm, `dst` already holds the content to
        // keep (the masked shape's own paint) — `src` (the mask's rendered
        // content) is only read, never copied onto `dst`, and no new region
        // of `dst` becomes non-transparent, so `dst.mark` is not needed.
        LayerComposite::MaskLayer { rect, mode } => {
            composite_mask_layer_rendered(&mut dst.pm, &src.pm, *rect, *mode);
        }
        LayerComposite::ClipShape(shape) => {
            composite_clip_shape_layer(&mut dst.pm, &src.pm, shape, src_dirty);
            dst.mark(src_dirty);
        }
        LayerComposite::ClipRoundedRect { rect, radii } => {
            composite_clip_rounded_rect_layer(&mut dst.pm, &src.pm, rect, radii, src_dirty);
            dst.mark(src_dirty);
        }
    }
}

/// Map an axis-aligned bbox `(left, top, right, bottom)` through a 2D affine
/// and return the enclosing axis-aligned bbox, inflated by 1px for the bilinear
/// resampling bleed of [`composite_transform_layer`].
fn transform_bounds(t: tiny_skia::Transform, b: DrawBounds) -> DrawBounds {
    let (l, tp, r, btm) = b;
    let mut out = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for (x, y) in [(l, tp), (r, tp), (l, btm), (r, btm)] {
        let nx = t.sx * x + t.kx * y + t.tx;
        let ny = t.ky * x + t.sy * y + t.ty;
        out.0 = out.0.min(nx);
        out.1 = out.1.min(ny);
        out.2 = out.2.max(nx);
        out.3 = out.3.max(ny);
    }
    (out.0 - 1.0, out.1 - 1.0, out.2 + 1.0, out.3 + 1.0)
}

/// BUG-249: применяет скруглённый клип (`overflow:hidden` + `border-radius`) к
/// off-screen слою `src` и композитит результат на `dst` (`SourceOver`).
/// Зеркало [`composite_clip_shape_layer`], но покрытие — rounded-rect контур.
///
/// BUG-267 (срез B): клонируется, умножается на покрытие и композитится только
/// dirty-окно слоя — вне его `src` полностью прозрачен, и `SourceOver`
/// прозрачного источника оставляет фон бит-идентичным. Само покрытие
/// растеризуется по-прежнему во всё полотно: AA-заливка пути должна идти в
/// page-координатах — перенос пути на смещение кропа меняет f32-округление
/// позиций рёбер и не гарантирует бит-идентичное покрытие. Аллокация + локальная
/// заливка дёшевы против устранённых полнополотных clone/умножения/блендинга.
fn composite_clip_rounded_rect_layer(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    rect: &Rect,
    radii: &[f32; 4],
    src_dirty: DrawBounds,
) {
    let (w, h) = (src.width(), src.height());
    let Some((cx0, cy0, mut masked)) = crop_dirty_window(src, src_dirty) else {
        return;
    };
    let Some(coverage) = rasterize_rounded_rect_coverage(rect, radii, w, h) else {
        // Вырожденный rect (нулевая ширина/высота) клиппит всё — композит пуст.
        return;
    };
    multiply_alpha_by_mask(&mut masked, &coverage, cx0, cy0);
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    dst.draw_pixmap(
        cx0 as i32,
        cy0 as i32,
        masked.as_ref(),
        &paint,
        tiny_skia::Transform::identity(),
        None,
    );
}

/// Растеризует анти-алиасное покрытие скруглённого прямоугольника `rect` с
/// радиусами `[tl, tr, br, bl]` (px) в transparent-pixmap (альфа = покрытие).
/// Радиусы клампятся к половине меньшей стороны, чтобы соседние углы не
/// перекрывались. `None`, если `rect` вырожден.
fn rasterize_rounded_rect_coverage(
    rect: &Rect,
    radii: &[f32; 4],
    width: u32,
    height: u32,
) -> Option<tiny_skia::Pixmap> {
    const K: f32 = 0.5523;
    let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let max_r = (w.min(h) / 2.0).max(0.0);
    let tl = radii[0].clamp(0.0, max_r);
    let tr = radii[1].clamp(0.0, max_r);
    let br = radii[2].clamp(0.0, max_r);
    let bl = radii[3].clamp(0.0, max_r);
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x + tl, y);
    pb.line_to(x + w - tr, y);
    pb.cubic_to(x + w - tr + K * tr, y, x + w, y + tr - K * tr, x + w, y + tr);
    pb.line_to(x + w, y + h - br);
    pb.cubic_to(x + w, y + h - br + K * br, x + w - br + K * br, y + h, x + w - br, y + h);
    pb.line_to(x + bl, y + h);
    pb.cubic_to(x + bl - K * bl, y + h, x, y + h - bl + K * bl, x, y + h - bl);
    pb.line_to(x, y + tl);
    pb.cubic_to(x, y + tl - K * tl, x + tl - K * tl, y, x + tl, y);
    pb.close();
    let path = pb.finish()?;
    let mut coverage = tiny_skia::Pixmap::new(width, height)?;
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    coverage.fill_path(
        &path,
        &paint,
        tiny_skia::FillRule::Winding,
        tiny_skia::Transform::identity(),
        None,
    );
    Some(coverage)
}

/// BUG-140: применяет shape-клип (`clip-path` circle/ellipse/polygon) к
/// off-screen слою `src` и композитит результат на `dst` (`SourceOver`).
///
/// Покрытие формы растеризуется заливкой tiny-skia пути (анти-алиасинг
/// детерминирован — pure Rust), альфа покрытия умножается в слой как у
/// `composite_mask_layer`. Координаты формы — page px (до transform
/// элемента): команда эмитится внутри `PushTransform`, поэтому слой
/// transform-группы выше переносит уже обрезанный результат.
///
/// BUG-267 (срез B): clone/умножение/композит идут по dirty-окну слоя, покрытие
/// остаётся полнополотным — см. [`composite_clip_rounded_rect_layer`].
fn composite_clip_shape_layer(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    shape: &ResolvedClipShape,
    src_dirty: DrawBounds,
) {
    let (w, h) = (src.width(), src.height());
    let Some((cx0, cy0, mut masked)) = crop_dirty_window(src, src_dirty) else {
        return;
    };
    let Some(coverage) = rasterize_clip_shape_coverage(shape, w, h) else {
        // Вырожденная форма (нулевой радиус / <3 вершин полигона) клиппит всё.
        return;
    };
    multiply_alpha_by_mask(&mut masked, &coverage, cx0, cy0);
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    dst.draw_pixmap(
        cx0 as i32,
        cy0 as i32,
        masked.as_ref(),
        &paint,
        tiny_skia::Transform::identity(),
        None,
    );
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
///
/// BUG-267 (slice C): only the layer's dirty window is cloned, multiplied and
/// composited — outside it `src` is fully transparent, so those pixels never
/// change the backdrop. The mask gradient itself is still rasterised full-canvas
/// so the existing gradient routines run in page coordinates unchanged (their
/// output must stay bit-identical); that is one allocation plus a rect-local
/// fill, cheap next to the eliminated full-canvas clone/multiply/blend passes.
fn composite_mask_layer(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    spec: &MaskSpec,
    src_dirty: DrawBounds,
) {
    let (w, h) = (src.width(), src.height());
    let Some((cx0, cy0, mut masked)) = crop_dirty_window(src, src_dirty) else {
        return;
    };
    if let Some(mask) = render_mask(spec, w, h) {
        multiply_alpha_by_mask(&mut masked, &mask, cx0, cy0);
    }
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    dst.draw_pixmap(
        cx0 as i32,
        cy0 as i32,
        masked.as_ref(),
        &paint,
        tiny_skia::Transform::identity(),
        None,
    );
}

/// Rasterise a `MaskSpec` gradient into a fresh transparent pixmap whose alpha
/// channel is the per-pixel mask value. Returns `None` for `MaskSpec::None` (no
/// effective mask) — the caller then composites the layer unchanged.
fn render_mask(spec: &MaskSpec, width: u32, height: u32) -> Option<tiny_skia::Pixmap> {
    let mut mask = tiny_skia::Pixmap::new(width, height)?;
    match spec {
        MaskSpec::None => return None,
        MaskSpec::Raster(pm) => return Some(pm.clone()),
        MaskSpec::Linear { rect, angle_deg, stops, repeating } => {
            rasterize_linear_gradient(&mut mask, rect, *angle_deg, stops, *repeating, None).ok()?;
        }
        MaskSpec::Radial { rect, center_x_pct, center_y_pct, stops, repeating } => {
            // Mask radial gradients stay circular (farthest-corner) — the mask
            // command carries no ending-shape, matching the femtovg mask path.
            let dx = center_x_pct.max(1.0 - center_x_pct) * rect.width;
            let dy = center_y_pct.max(1.0 - center_y_pct) * rect.height;
            let r = dx.hypot(dy).max(1.0);
            rasterize_radial_gradient(
                &mut mask, rect, *center_x_pct, *center_y_pct, r, r, stops, *repeating, None,
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
///
/// BUG-267: `layer` is a dirty-window crop whose top-left sits at `(x0, y0)` of
/// the full-canvas `mask`, so each crop pixel is multiplied by exactly the same
/// mask alpha as in the pre-crop full-canvas pass. The crop must lie inside the
/// mask (guaranteed by [`crop_dirty_window`]'s canvas clamp).
/// LIB-9 — `PopMaskLayer`'s composite. Multiplies `dst`'s own premultiplied
/// RGBA in place by a per-pixel mask value read from `mask_src` (the
/// just-closed mask-content layer), within `rect` only (CSS Masking L1 §5 —
/// pixels outside `rect` are unaffected). Unlike [`composite_mask_layer`]
/// (which copies a rendered `src` crop *onto* `dst`), `dst` already holds the
/// masked shape's own paint (rendered before `PushMaskLayer`, into the same
/// layer via the emitter's isolating `PushOpacity`) — this only attenuates it
/// in place; `mask_src` itself is discarded afterwards.
///
/// `mode` mirrors the wgpu `MASK_LAYER_SHADER_SRC`'s `fs_alpha`/`fs_luma`:
/// `Alpha` reads `mask_src`'s alpha channel directly (same read
/// `multiply_alpha_by_mask` does for the image/gradient mask path);
/// `Luminance` computes `0.2126·R + 0.7152·G + 0.0722·B` on `mask_src`'s
/// **premultiplied** bytes — since premultiplication is a uniform per-channel
/// scalar, this linear combination already equals `luma(unpremultiplied)·alpha`
/// without unpremultiplying first (same trick `mask_stops_for_mode`,
/// `background_mask.rs`, uses on gradient stops).
fn composite_mask_layer_rendered(
    dst: &mut tiny_skia::Pixmap,
    mask_src: &tiny_skia::Pixmap,
    rect: Rect,
    mode: crate::display_list::MaskMode,
) {
    let (w, h) = (dst.width() as i32, dst.height() as i32);
    let x0 = (rect.x.floor() as i32).clamp(0, w);
    let y0 = (rect.y.floor() as i32).clamp(0, h);
    let x1 = ((rect.x + rect.width).ceil() as i32).clamp(0, w);
    let y1 = ((rect.y + rect.height).ceil() as i32).clamp(0, h);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let stride = w as usize * 4;
    let md = mask_src.data();
    let dd = dst.data_mut();
    for y in y0..y1 {
        let row = y as usize * stride;
        for x in x0..x1 {
            let i = row + x as usize * 4;
            let value: u32 = match mode {
                crate::display_list::MaskMode::Alpha => u32::from(md[i + 3]),
                crate::display_list::MaskMode::InvertedAlpha => 255 - u32::from(md[i + 3]),
                crate::display_list::MaskMode::Luminance => {
                    let r = f32::from(md[i]);
                    let g = f32::from(md[i + 1]);
                    let b = f32::from(md[i + 2]);
                    (0.2126 * r + 0.7152 * g + 0.0722 * b).round().clamp(0.0, 255.0) as u32
                }
            };
            for ch in &mut dd[i..i + 4] {
                *ch = ((u32::from(*ch) * value + 127) / 255) as u8;
            }
        }
    }
}

fn multiply_alpha_by_mask(
    layer: &mut tiny_skia::Pixmap,
    mask: &tiny_skia::Pixmap,
    x0: u32,
    y0: u32,
) {
    let mw = mask.width() as usize;
    let (w, h) = (layer.width() as usize, layer.height() as usize);
    let (x0, y0) = (x0 as usize, y0 as usize);
    let md = mask.data();
    let ld = layer.data_mut();
    for row in 0..h {
        let l = row * w * 4;
        let m = ((y0 + row) * mw + x0) * 4;
        for col in 0..w {
            let a = u32::from(md[m + col * 4 + 3]);
            for ch in &mut ld[l + col * 4..l + (col + 1) * 4] {
                *ch = ((u32::from(*ch) * a + 127) / 255) as u8;
            }
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
///
/// BUG-267: the chain runs on a crop of `src` — `src_dirty` (the ink superset
/// tracked by [`CpuLayer`]) inflated by the chain's spatial reach plus 2px of
/// anti-aliasing slack — instead of the whole canvas. Outside that window `src`
/// is fully transparent, the colour-matrix filters keep transparent pixels
/// transparent, and blur cannot carry ink further than its reach, so the crop
/// output is bit-identical to filtering the full canvas (the box-blur running
/// sums accumulate exact zeros until the first ink sample, which leaves the f32
/// accumulator unchanged). Returns the composited window `(left, top, right,
/// bottom)` for the caller's dirty propagation, `None` when the window misses
/// the canvas entirely.
fn composite_filter_layer(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    filters: &[FilterFn],
    src_dirty: DrawBounds,
) -> Option<DrawBounds> {
    let margin = filter_chain_reach(filters) + 2.0;
    let (cx0, cy0, cx1, cy1) = clamp_crop(src_dirty, margin, src.width(), src.height())?;
    let mut cur = crop_pixmap(src, cx0, cy0, cx1 - cx0, cy1 - cy0)?;
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
    dst.draw_pixmap(
        cx0 as i32,
        cy0 as i32,
        cur.as_ref(),
        &paint,
        tiny_skia::Transform::identity(),
        None,
    );
    Some((cx0 as f32, cy0 as f32, cx1 as f32, cy1 as f32))
}

/// [`composite_filter_layer`] for a chain containing `url(#id)` (SVG filter graph,
/// [`crate::svg_filter`]). The chain runs on a window `(pixmap, origin)` that starts
/// as the ink crop of `src` and is replaced by the filter-region window after each
/// SVG filter (a flood may paint beyond the ink). A dangling reference makes the
/// whole chain invalid (Filter Effects L1 §3): the layer is composited unfiltered.
fn composite_svg_filter_chain(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    filters: &[FilterFn],
    bounds: Rect,
    src_dirty: DrawBounds,
) -> Option<DrawBounds> {
    if filters.iter().any(|f| matches!(f, FilterFn::Url(u) if u.graph.is_none())) {
        return composite_filter_layer(dst, src, &[], src_dirty);
    }
    let canvas = (src.width(), src.height());
    let margin = filter_chain_reach(filters) + 2.0;
    let (mut cur, mut origin) = match clamp_crop(src_dirty, margin, canvas.0, canvas.1) {
        Some((cx0, cy0, cx1, cy1)) => (
            crop_pixmap(src, cx0, cy0, cx1 - cx0, cy1 - cy0)?,
            (cx0 as i32, cy0 as i32),
        ),
        None => (tiny_skia::Pixmap::new(1, 1)?, (0, 0)),
    };
    for f in filters {
        match f {
            FilterFn::Blur(sigma) => cur = gaussian_blur(&cur, *sigma),
            FilterFn::Url(u) => {
                if let Some(win) = crate::svg_filter::apply(&cur, origin, canvas, u, bounds) {
                    cur = win.pixmap;
                    origin = (win.x, win.y);
                }
            }
            other => apply_color_filter(&mut cur, other),
        }
    }
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    dst.draw_pixmap(origin.0, origin.1, cur.as_ref(), &paint, tiny_skia::Transform::identity(), None);
    Some((
        origin.0 as f32,
        origin.1 as f32,
        origin.0 as f32 + cur.width() as f32,
        origin.1 as f32 + cur.height() as f32,
    ))
}

/// Total spatial reach in px of a filter chain — how far the chain can carry
/// ink beyond its source. Only `blur()` is spatial: three box passes of radius
/// `r` (the same `r` [`gaussian_blur`] derives from σ — keep the formulas in
/// sync) reach `3r` per side; the colour-matrix filters are per-pixel. Chained
/// blurs add their reaches.
fn filter_chain_reach(filters: &[FilterFn]) -> f32 {
    filters
        .iter()
        .map(|f| match f {
            FilterFn::Blur(sigma) => {
                let radius = (((4.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0).round();
                3.0 * radius.max(0.0)
            }
            _ => 0.0,
        })
        .sum()
}

/// Integer crop window: `dirty` `(left, top, right, bottom)` inflated by
/// `margin` px on every side and clamped to the `width`×`height` canvas.
/// `None` when the window is empty (all ink off-canvas, or a degenerate dirty
/// box) — the caller then skips filtering and compositing entirely.
fn clamp_crop(
    dirty: DrawBounds,
    margin: f32,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let x0 = ((dirty.0 - margin).floor().max(0.0) as u32).min(width);
    let y0 = ((dirty.1 - margin).floor().max(0.0) as u32).min(height);
    let x1 = ((dirty.2 + margin).ceil().max(0.0) as u32).min(width);
    let y1 = ((dirty.3 + margin).ceil().max(0.0) as u32).min(height);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some((x0, y0, x1, y1))
}

/// Byte-exact copy of the `w`×`h` window at `(x0, y0)` of `src` into a fresh
/// pixmap. Plain row memcpy — no blending or resampling — so filtering the crop
/// is bit-identical to filtering the full canvas (see
/// [`composite_filter_layer`]). The window must lie inside `src`.
fn crop_pixmap(src: &tiny_skia::Pixmap, x0: u32, y0: u32, w: u32, h: u32) -> Option<tiny_skia::Pixmap> {
    let mut out = tiny_skia::Pixmap::new(w, h)?;
    let sw = src.width() as usize;
    let (x0, y0, w, h) = (x0 as usize, y0 as usize, w as usize, h as usize);
    let sd = src.data();
    let od = out.data_mut();
    for row in 0..h {
        let s = ((y0 + row) * sw + x0) * 4;
        let d = row * w * 4;
        od[d..d + w * 4].copy_from_slice(&sd[s..s + w * 4]);
    }
    Some(out)
}

/// BUG-267 (slices B–D): cut the dirty window out of group layer `src` before
/// compositing. The window is `dirty` inflated by 2px of anti-aliasing slack
/// (draws mark their geometric bounds; AA bleed stays within ~1px) and clamped
/// to the canvas; the returned pixmap is a byte-exact [`crop_pixmap`] copy with
/// its canvas origin `(x0, y0)`. `None` when the window misses the canvas —
/// there `src` holds no on-canvas ink, and a fully transparent source leaves
/// every composite op's backdrop bit-identical (see [`close_layer`]), so the
/// caller skips the composite outright.
fn crop_dirty_window(
    src: &tiny_skia::Pixmap,
    dirty: DrawBounds,
) -> Option<(u32, u32, tiny_skia::Pixmap)> {
    let (cx0, cy0, cx1, cy1) = clamp_crop(dirty, 2.0, src.width(), src.height())?;
    let crop = crop_pixmap(src, cx0, cy0, cx1 - cx0, cy1 - cy0)?;
    Some((cx0, cy0, crop))
}

/// Apply a `backdrop-filter` chain (CSS Filter Effects L1 §6.2) to the content
/// already painted in `target` within `bounds`, in place.
///
/// The filter operates on the *backdrop* — the content painted behind the
/// element so far, i.e. the current active layer. A window of the layer —
/// `bounds` inflated by the chain's blur reach, so Gaussian blur samples every
/// neighbouring backdrop pixel that can influence a pixel inside `bounds`
/// (BUG-267: previously the whole canvas was cloned and blurred) — is cropped
/// and filtered, then only the `bounds` border-box region of the filtered copy
/// is written back over `target` with `Source` blend (replace). The element's
/// own background/border paint on top afterwards via subsequent draws, so the
/// matching `PopBackdropFilter` is a no-op. Reuses the same integer-only
/// `gaussian_blur` and un-premultiplied `apply_color_filter` as
/// `composite_filter_layer`, so the result is cross-OS bit-identical.
fn apply_backdrop_filter(
    target: &mut tiny_skia::Pixmap,
    filters: &[FilterFn],
    bounds: &Rect,
    width: u32,
    height: u32,
) {
    let reach = filter_chain_reach(filters);
    let Some((cx0, cy0, cx1, cy1)) = clamp_crop(rect_bounds(bounds), reach, width, height) else {
        // `bounds` misses the canvas — the masked write-back would be empty.
        return;
    };
    let Some(mut filtered) = crop_pixmap(target, cx0, cy0, cx1 - cx0, cy1 - cy0) else {
        return;
    };
    for f in filters {
        match f {
            FilterFn::Blur(sigma) => filtered = gaussian_blur(&filtered, *sigma),
            other => apply_color_filter(&mut filtered, other),
        }
    }
    // Write back only the element's border-box region. A hard rect mask scopes
    // the replace to `bounds`; `Source` blend overwrites the backdrop there with
    // its filtered counterpart (outside the mask the original target is kept).
    // The crop window contains all of `bounds ∩ canvas`, so every masked pixel
    // is covered by the drawn pixmap.
    let mask = build_clip_mask(width, height, Some(*bounds));
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::Source,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    target.draw_pixmap(
        cx0 as i32,
        cy0 as i32,
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
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
fn gaussian_blur(src: &tiny_skia::Pixmap, sigma: f32) -> tiny_skia::Pixmap {
    let w = src.width() as usize;
    let h = src.height() as usize;
    let radius = (((4.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0).round() as i32;
    if radius <= 0 || w == 0 || h == 0 {
        return src.clone();
    }
    // BUG-850: `box_blur_h`/`box_blur_v`'s accumulator-priming loop
    // (`for i in -r..=r`) costs O(radius) per row/column regardless of the
    // layer's own size — an unclamped `backdrop-filter: blur(100000px)`
    // primed a ~100 000-sample window per row of a 1024px-wide layer, 16 s
    // for one screenshot. Once the radius reaches the layer's larger
    // dimension, both box-blur passes' `i.clamp(0, last)` has already pulled
    // in every real sample of the row/column at least once; growing it
    // further only adds more repeats of the same edge pixel, shifting the
    // mix by a fraction no rounded 8-bit channel can still distinguish —
    // clamping there bounds the cost at O(layer size) with no visible
    // difference for a radius already this saturated, far above anything a
    // real blur value on a real layer produces (the graphic-tests corpus
    // tops out at `blur(8px)`).
    let radius = radius.min(w.max(h) as i32);
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
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
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
            FilterFn::Blur(_) | FilterFn::Url(_) => {}
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
///
/// BUG-267 (slice D): only the layer's dirty window is cloned and blended.
/// Outside it `src` is fully transparent, and every CSS blend mode composited
/// per Porter-Duff reduces to the unchanged backdrop for a zero-alpha source
/// (the same property the empty-group skip in [`close_layer`] already relies
/// on), so skipping those pixels is bit-identical to the full-canvas pass.
fn composite_blend_layer(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    mode: tiny_skia::BlendMode,
    src_dirty: DrawBounds,
) {
    let Some((cx0, cy0, crop)) = crop_dirty_window(src, src_dirty) else {
        return;
    };
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: mode,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    dst.draw_pixmap(
        cx0 as i32,
        cy0 as i32,
        crop.as_ref(),
        &paint,
        tiny_skia::Transform::identity(),
        None,
    );
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

/// Результат [`render_offcanvas_transform_group`].
enum GroupLayer {
    /// Группа целиком в полотне в локальных координатах — рисуется обычным
    /// слоем размером с полотно.
    Legacy,
    /// Группа отрисована в собственный слой: пиксели (`None` — в видимой
    /// области ничего нет) и аффинное преобразование слоя на полотно.
    Rendered(Option<tiny_skia::Pixmap>, tiny_skia::Transform),
}

/// Индекс `PopTransform`, закрывающего `PushTransform` в `commands[start]`.
fn matching_pop_transform(commands: &[DisplayCommand], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, cmd) in commands.iter().enumerate().skip(start) {
        match cmd {
            DisplayCommand::PushTransform { .. } => depth += 1,
            DisplayCommand::PopTransform => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Индекс `PopScrollLayer`, закрывающего `PushScrollLayer` в `commands[start]`.
fn matching_pop_scroll(commands: &[DisplayCommand], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, cmd) in commands.iter().enumerate().skip(start) {
        match cmd {
            DisplayCommand::PushScrollLayer { .. } => depth += 1,
            DisplayCommand::PopScrollLayer => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Объединённый габарит рисуемой геометрии `body` в системе координат группы
/// (вложенные `PushTransform` проецируются своей матрицей).
fn group_ink_bbox(body: &[DisplayCommand]) -> Option<Rect> {
    fn union(acc: &mut Option<Rect>, r: Rect) {
        *acc = Some(match *acc {
            None => r,
            Some(a) => {
                let x0 = a.x.min(r.x);
                let y0 = a.y.min(r.y);
                let x1 = (a.x + a.width).max(r.x + r.width);
                let y1 = (a.y + a.height).max(r.y + r.height);
                Rect::new(x0, y0, x1 - x0, y1 - y0)
            }
        });
    }
    let mut acc = None;
    let mut i = 0;
    while i < body.len() {
        if let DisplayCommand::PushTransform { matrix } = &body[i] {
            let Some(end) = matching_pop_transform(body, i) else { break };
            if let Some(inner) = group_ink_bbox(&body[i + 1..end]) {
                union(&mut acc, transform_rect_bbox(inner, matrix));
            }
            i = end + 1;
            continue;
        }
        if let Some(r) = body[i].cull_rect() {
            union(&mut acc, r);
        }
        i += 1;
    }
    acc
}

/// BUG-1288: рисует группу `PushTransform` в слой по её габариту, а не по
/// размеру полотна. Слой размером с полотно держит только локальные
/// координаты `0..W × 0..H`: блок, лежащий до трансформации левее/выше нуля
/// или за краем полотна, отбрасывался, хотя после трансформации попадает в
/// кадр. Слой берётся как видимая часть полотна, пересчитанная обратной
/// матрицей и пересечённая с габаритом содержимого; команды сдвигаются на
/// целое смещение его начала (текст остаётся на целых пикселях), а на
/// композите это смещение возвращается в матрицу.
///
/// [`GroupLayer::Legacy`] — когда содержимое и так внутри полотна (картинка
/// остаётся побитово прежней), матрица необратима или слой получился бы
/// непомерным.
fn render_offcanvas_transform_group(
    width: u32,
    height: u32,
    matrix: &lumen_layout::Mat4,
    body: &[DisplayCommand],
    ambient_clip: Option<Rect>,
    image_map: &std::collections::HashMap<&str, &Image>,
    font_provider: Option<&dyn FontProvider>,
) -> Result<GroupLayer, Box<dyn std::error::Error>> {
    /// Потолок площади слоя группы (px²) — как у полотна 8192×8192.
    const MAX_GROUP_AREA: f64 = 64.0 * 1024.0 * 1024.0;
    let Some(ink) = group_ink_bbox(body) else { return Ok(GroupLayer::Legacy) };
    let (w, h) = (width as f32, height as f32);
    if ink.x >= 0.0 && ink.y >= 0.0 && ink.x + ink.width <= w && ink.y + ink.height <= h {
        return Ok(GroupLayer::Legacy);
    }
    let Some(inv) = matrix.invert_2d_affine() else { return Ok(GroupLayer::Legacy) };
    // Видимая часть полотна в локальных координатах (+2 px на билинейный
    // выход за край) ∩ габарит содержимого (+64 px на размытие/обводку).
    let visible = transform_rect_bbox(Rect::new(-2.0, -2.0, w + 4.0, h + 4.0), &inv);
    let x0 = visible.x.max(ink.x - 64.0).floor();
    let y0 = visible.y.max(ink.y - 64.0).floor();
    let x1 = (visible.x + visible.width).min(ink.x + ink.width + 64.0).ceil();
    let y1 = (visible.y + visible.height).min(ink.y + ink.height + 64.0).ceil();
    let (a, b, c, d, e, f) = {
        let [a, b, c, d, e, f] = mat4_to_2d_affine(matrix);
        (a, b, c, d, e, f)
    };
    let t = tiny_skia::Transform::from_row(a, b, c, d, e, f);
    if !(x1 > x0 && y1 > y0) {
        return Ok(GroupLayer::Rendered(None, t));
    }
    if f64::from(x1 - x0) * f64::from(y1 - y0) > MAX_GROUP_AREA {
        return Ok(GroupLayer::Legacy);
    }
    let (gw, gh) = ((x1 - x0) as u32, (y1 - y0) as u32);

    let mut cmds: Vec<DisplayCommand> = Vec::with_capacity(body.len() + 1);
    if let Some(clip) = ambient_clip {
        let local = transform_rect_bbox(clip, &inv);
        cmds.push(DisplayCommand::PushClipRect { rect: Rect::new(local.x - x0, local.y - y0, local.width, local.height) });
    }
    cmds.extend(body.iter().map(|cmd| cmd.translated(-x0, -y0)));
    let pm = rasterize_cpu_pixmap(gw, gh, &cmds, image_map, font_provider, None)?;
    Ok(GroupLayer::Rendered(Some(pm), t.pre_translate(x0, y0)))
}

/// Axis-aligned bounding box of `rect` mapped through the 2D affine `m`.
///
/// BUG-424: used to carry an ambient clip across a `PushTransform` boundary.
/// Inside a transform layer the CPU rasterizer draws in pre-transform
/// coordinates, so the document-space clip is mapped by the *inverse* matrix
/// before it gates those draws. For a translate/scale matrix — the SVG
/// icon-sprite case (`viewBox` → CSS px) — the image is exactly a rect; under
/// rotation/skew the bbox is a superset of the true parallelogram, i.e. it
/// clips conservatively (never removes ink the exact clip would keep). Same
/// policy as the wgpu backend's `apply_transform_to_clip` (BUG-140).
fn transform_rect_bbox(rect: Rect, m: &lumen_layout::Mat4) -> Rect {
    let (x0, y0) = (rect.x, rect.y);
    let (x1, y1) = (rect.x + rect.width, rect.y + rect.height);
    let corners = [
        m.transform_point_2d(x0, y0),
        m.transform_point_2d(x1, y0),
        m.transform_point_2d(x0, y1),
        m.transform_point_2d(x1, y1),
    ];
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for (sx, sy) in corners {
        min_x = min_x.min(sx);
        min_y = min_y.min(sy);
        max_x = max_x.max(sx);
        max_y = max_y.max(sy);
    }
    Rect::new(min_x, min_y, (max_x - min_x).max(0.0), (max_y - min_y).max(0.0))
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

/// Axis-aligned bounding box `(left, top, right, bottom)` of a set of contours
/// (for [`DisplayCommand::DrawSvgFill`]).
fn contours_bounds(contours: &[Vec<[f32; 2]>]) -> (f32, f32, f32, f32) {
    let mut l = f32::INFINITY;
    let mut t = f32::INFINITY;
    let mut r = f32::NEG_INFINITY;
    let mut b = f32::NEG_INFINITY;
    for contour in contours {
        for v in contour {
            l = l.min(v[0]);
            t = t.min(v[1]);
            r = r.max(v[0]);
            b = b.max(v[1]);
        }
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

    // BUG-1249: pixel snapping рёбер бокса (CSS Painting) — иначе дробная
    // граница двух соседей даёт AA-шов, через который виден фон родителя.
    // Рёбра округляются, а не ширина, поэтому соседи делят одно ребро.
    let (x0, y0) = (rect.x.round(), rect.y.round());
    let (x1, y1) = ((rect.x + rect.width).round(), (rect.y + rect.height).round());
    if x1 <= x0 || y1 <= y0 {
        // Бокс тоньше пикселя: рисуем исходный прямоугольник как есть.
        let skia_rect = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height)
            .ok_or("Invalid rect dimensions")?;
        pixmap.fill_rect(skia_rect, &paint, tiny_skia::Transform::identity(), clip);
        return Ok(());
    }
    let skia_rect = tiny_skia::Rect::from_ltrb(x0, y0, x1, y1).ok_or("Invalid rect dimensions")?;

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

    let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
    if w <= 0.0 || h <= 0.0 {
        return Ok(());
    }

    let paint = Paint {
        shader: tiny_skia::Shader::SolidColor(color_to_skia(*color)),
        anti_alias: true,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };

    // Radii must be clamped to the box (CSS Backgrounds L3 §5.5): a pill button
    // with `border-radius: 999px` on a 140×44 box otherwise sends the cubic
    // control points far outside the rect, exploding the fill into a giant
    // diagonal wedge (BUG-221, TEST-36). `clamped_to_box` caps each axis at half
    // the corresponding side; the shared kappa-Bézier outline (also used by the
    // rounded border ring) then yields the same corner geometry as the GPU path.
    let clamped = radii.clamped_to_box(w, h);
    let mut pb = tiny_skia::PathBuilder::new();
    push_rounded_rect_outline(&mut pb, x, y, w, h, &clamped);

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

/// Краи бокса рамки по целым пикселям, ничья — вверх (Chromium `PixelSnappedIntRect`):
/// отличается от «пиксель, чей центр внутри» только на ровно дробной `.5` границе.
fn snap_border_box(rect: Rect) -> Rect {
    let snap = |v: f32| (v + 0.5).floor();
    let (x0, y0) = (snap(rect.x), snap(rect.y));
    Rect::new(x0, y0, snap(rect.x + rect.width) - x0, snap(rect.y + rect.height) - y0)
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
    // Рамка рисуется без сглаживания: tiny-skia берёт пиксель, чей центр лежит в
    // `[край, край)`, и на ровно дробной `.5` границе (`top: 212.5px`) округляет вниз, а
    // Chromium (`PixelSnappedIntRect`) — вверх; полосы `groove`/`ridge` из `border_bevel`
    // уже округляют вверх, и одна и та же рамка расходилась со своим `solid`-соседом.
    let snapped = snap_border_box(*rect);
    let rect = &snapped;
    // groove/ridge/inset/outset — общая геометрия `border_bevel` (две полосы / один оттенок
    // на сторону, стыки по диагонали); остальные стороны идут обычным путём ниже.
    let styles = paint_bevel_sides(*rect, *widths, *colors, *styles, |piece, color| {
        if let Some(r) = tiny_skia::Rect::from_xywh(piece.x, piece.y, piece.width, piece.height) {
            pixmap.fill_rect(r, &border_paint(color), tiny_skia::Transform::identity(), clip);
        }
    });
    let [top_s, right_s, bottom_s, left_s] = &styles;

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
// Geometry + style + clip naturally take 8 positional params; bundling them
// into a struct would not improve clarity for this internal raster helper.
#[allow(clippy::too_many_arguments)]
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
// Geometry + style + clip naturally take 8 positional params; bundling them
// into a struct would not improve clarity for this internal raster helper.
#[allow(clippy::too_many_arguments)]
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

    // The outline's inner edge is `rect ± offset`. A tiny-skia stroke is centred on
    // its path, so the path sits half a stroke width further out (BUG-1279).
    let grow = offset + width / 2.0;
    let x = rect.x - grow;
    let y = rect.y - grow;
    let w = rect.width + 2.0 * grow;
    let h = rect.height + 2.0 * grow;

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
    // CSS Images L4 §3.1 — interpolate fades to/through transparency in
    // premultiplied space (BUG-190); tiny-skia samples these dense stops in
    // straight space, reproducing the premultiplied curve. Opaque segments are
    // untouched, so solid gradients stay byte-identical.
    let subdivided = crate::gradient_math::premultiplied_subdivide_stops(resolved);
    let resolved = &subdivided[..];
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

/// CSS Images L3 §3.3/§3.5 — `radial-gradient(...)` via tiny-skia `RadialGradient`.
///
/// The ending-shape radii `(radius_x, radius_y)` are resolved upstream
/// ([`lumen_layout::radial_gradient_radii`]). A **circle** (`radius_x ==
/// radius_y`) is drawn isotropically, matching the femtovg backend byte for byte
/// (BUG-221, TEST-39). An **ellipse** is drawn as a unit circle of radius
/// `radius_x` plus a vertical scale `radius_y/radius_x` about the centre, so the
/// rings stretch to the elliptical ending shape Edge produces (BUG-239).
#[allow(clippy::too_many_arguments)]
fn rasterize_radial_gradient(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    center_x_pct: f32,
    center_y_pct: f32,
    radius_x: f32,
    radius_y: f32,
    stops: &[GradientStop],
    repeating: bool,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::{Paint, Point, RadialGradient, SpreadMode, Transform};

    let rx = radius_x.max(1.0);
    let ry = radius_y.max(1.0);
    // Px stops resolve against the gradient ray; use the larger radius (matches
    // the femtovg backend's `line_len`).
    let resolved = resolve_stop_positions(stops, rx.max(ry));
    let Some((skia_stops, lo, hi)) = skia_gradient_stops(&resolved, repeating) else {
        return Ok(());
    };

    // Unit circle of radius `rx` centred at the gradient origin. For
    // non-repeating gradients the resolved stops already fill `[0,1]` so
    // `hi-lo == 1` and the radius is the full horizontal radius; for repeating
    // ones it is shortened to the `[lo,hi]` period so `SpreadMode::Repeat` tiles
    // the rings outward. A vertical scale turns the circle into the ellipse.
    let cx = rect.x + center_x_pct * rect.width;
    let cy = rect.y + center_y_pct * rect.height;
    let radius = (rx * (hi - lo)).max(1e-3);
    // A centre or radius that overflowed to ±inf/NaN (`at 9999…9%`) has no
    // finite rings: every pixel lies infinitely far out, so the layer is the
    // last stop's solid colour (CSS Images 3 §3.5.1 — degenerate sizing) rather
    // than an error that fails the whole screenshot (BUG-1436).
    if !(cx.is_finite() && cy.is_finite() && radius.is_finite()) {
        let last = resolved.last().map(|s| s.1).ok_or("empty gradient")?;
        let skia_rect = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height)
            .ok_or("Invalid rect dimensions")?;
        let mut paint = Paint::default();
        paint.set_color(color_to_skia(last));
        pixmap.fill_rect(skia_rect, &paint, Transform::identity(), clip);
        return Ok(());
    }
    let center = Point::from_xy(cx, cy);
    let mode = if repeating { SpreadMode::Repeat } else { SpreadMode::Pad };
    // Scale y about the centre by ry/rx (identity for a circle → byte-identical).
    let sy = (ry / rx).max(1e-6);
    let local = Transform::from_translate(cx, cy)
        .pre_scale(1.0, sy)
        .pre_translate(-cx, -cy);
    let Some(shader) = RadialGradient::new(
        center,
        center,
        radius,
        skia_stops,
        mode,
        local,
    ) else {
        // tiny-skia refuses the remaining degenerate cases: nothing to draw.
        return Ok(());
    };

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

/// `DrawSvgFill` fills the raw outline contours with the nonzero (`Winding`)
/// rule (BUG-247 / BUG-173). Unlike [`rasterize_svg_path`], which receives a
/// pre-tessellated triangle soup whose internal shared edges each pick up a
/// ~1px AA fringe, this builds the true shape outline so tiny_skia's analytic
/// anti-aliaser only feathers the real boundary — matching Edge.
fn rasterize_svg_fill(
    pixmap: &mut tiny_skia::Pixmap,
    contours: &[Vec<[f32; 2]>],
    color: &Color,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::Paint;

    let mut pb = tiny_skia::PathBuilder::new();
    for contour in contours {
        let mut pts = contour.iter();
        if let Some(&[x0, y0]) = pts.next() {
            pb.move_to(x0, y0);
            for &[x, y] in pts {
                pb.line_to(x, y);
            }
            pb.close();
        }
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

/// Draw a decoded `<img>` into its content box, honouring `object-fit` /
/// `object-position` (CSS Images L3 §5.5). Mirrors the femtovg backend's
/// `draw_image_in_rect` (BUG-221, TEST-18): the placement rect comes from the
/// shared [`crate::display_list::fit_image_rect`] (identical geometry to the GPU
/// path), the texture is mapped onto it with a bilinear pattern, and the draw is
/// clipped to the intersection of the placement rect and the content box — so
/// `object-fit: cover` overflow is trimmed, exactly like the GPU scissor.
fn rasterize_image(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    img: &Image,
    fit: ObjectFit,
    position: ObjectPosition,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    if rect.width <= 0.0 || rect.height <= 0.0 || img.width == 0 || img.height == 0 {
        return Ok(());
    }

    let placed = crate::display_list::fit_image_rect(*rect, (img.width, img.height), fit, position);
    if placed.width <= 0.0 || placed.height <= 0.0 {
        return Ok(());
    }

    // Visible region = placement ∩ content box. `object-fit: cover` makes the
    // placement larger than the box (clipped here); `contain` makes it smaller
    // (only the placement area is painted — no edge smear from `SpreadMode::Pad`).
    let vx0 = placed.x.max(rect.x);
    let vy0 = placed.y.max(rect.y);
    let vx1 = (placed.x + placed.width).min(rect.x + rect.width);
    let vy1 = (placed.y + placed.height).min(rect.y + rect.height);
    if vx1 <= vx0 || vy1 <= vy0 {
        return Ok(());
    }
    let visible = Rect::new(vx0, vy0, vx1 - vx0, vy1 - vy0);
    blit_image_into_rect(pixmap, placed, visible, img, 1.0, clip)
}

/// Paint `img` scaled to fill the `placed` target rect, drawing only the
/// `visible` sub-rect (the part inside the painting/content area), blended at
/// `alpha`. Shared by `<img>` object-fit, `background-image` tiles and
/// `cross-fade()` so all three sample identically.
///
/// Area-averages the texture down before the pattern draw when the target is
/// smaller than the source — mirrors the femtovg backend
/// (`resolve_image_for_rect`/`downscale_target`, BUG-077): plain bilinear
/// sampling of a full-res photo into a much smaller box undersamples and
/// produces high-frequency noise that diverges from Edge's high-quality
/// resampler (BUG-221, TEST-18). Upscale or exact-size targets keep the
/// original texture (bilinear is correct there).
fn blit_image_into_rect(
    pixmap: &mut tiny_skia::Pixmap,
    placed: Rect,
    visible: Rect,
    img: &Image,
    alpha: f32,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tiny_skia::{FilterQuality, Pattern, SpreadMode, Transform};

    if placed.width <= 0.0 || placed.height <= 0.0 || img.width == 0 || img.height == 0 {
        return Ok(());
    }
    if visible.width <= 0.0 || visible.height <= 0.0 {
        return Ok(());
    }

    let tw = placed.width.round().max(1.0) as u32;
    let th = placed.height.round().max(1.0) as u32;
    let downscaled;
    let texture: &Image = if tw < img.width || th < img.height {
        downscaled = lumen_image::resize_area_avg(img, tw, th);
        &downscaled
    } else {
        img
    };
    let src = image_to_pixmap(texture).ok_or("failed to build image pixmap")?;

    // Map (downscaled) source texels (0..w, 0..h) onto the placement rect.
    let sx = placed.width / texture.width as f32;
    let sy = placed.height / texture.height as f32;
    let ts = Transform::from_row(sx, 0.0, 0.0, sy, placed.x, placed.y);
    let shader = Pattern::new(
        src.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, alpha.clamp(0.0, 1.0), ts,
    );
    let paint = tiny_skia::Paint {
        shader,
        anti_alias: true,
        force_hq_pipeline: false,
        blend_mode: tiny_skia::BlendMode::SourceOver,
    };
    let vis = tiny_skia::Rect::from_xywh(visible.x, visible.y, visible.width, visible.height)
        .ok_or("invalid visible image rect")?;
    pixmap.fill_rect(vis, &paint, Transform::identity(), clip);
    Ok(())
}

/// Draw a `background-image` (`DrawBackgroundImage`) on the CPU path.
///
/// `rect` is the background painting area (`background-clip`); `origin_rect` is
/// the positioning area (`background-origin`). Tile size/start/repeat come from
/// the shared [`bg_tile_geometry`] (identical to the femtovg backend), so the
/// CPU snapshot matches the live window. Each tile is blitted via
/// [`blit_image_into_rect`] clipped to its intersection with the painting area,
/// so `no-repeat`/`cover` overflow is trimmed and `repeat` fills the box.
#[allow(clippy::too_many_arguments)]
fn rasterize_background_image(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    origin_rect: &Rect,
    img: &Image,
    size: BackgroundSize,
    position: &ObjectPosition,
    repeat: BackgroundRepeat,
    clip: Option<&tiny_skia::Mask>,
) -> Result<(), Box<dyn std::error::Error>> {
    if rect.width <= 0.0 || rect.height <= 0.0 || img.width == 0 || img.height == 0 {
        return Ok(());
    }

    let (tile_w, tile_h, tile_x_start, tile_y_start, repeat_x, repeat_y, step_x, step_y) = bg_tile_geometry(
        size,
        position,
        repeat,
        img.width as f32,
        img.height as f32,
        origin_rect.width,
        origin_rect.height,
        origin_rect.x,
        origin_rect.y,
    );
    if tile_w <= 0.0 || tile_h <= 0.0 {
        return Ok(());
    }
    // BUG-1303: the grid also covers the clip-area strip before the origin box.
    let tile_x_start = extend_start_to_paint_area(tile_x_start, step_x, repeat_x, rect.x);
    let tile_y_start = extend_start_to_paint_area(tile_y_start, step_y, repeat_y, rect.y);

    let x_end = rect.x + rect.width;
    let y_end = rect.y + rect.height;
    let mut vector_cache: Option<Image> = None;
    let mut ty = tile_y_start;
    loop {
        if ty >= y_end {
            break;
        }
        if ty + tile_h > rect.y {
            let mut tx = tile_x_start;
            loop {
                if tx >= x_end {
                    break;
                }
                if tx + tile_w > rect.x {
                    // visible = tile ∩ painting area (trims overflow per axis).
                    let vx0 = tx.max(rect.x);
                    let vy0 = ty.max(rect.y);
                    let vx1 = (tx + tile_w).min(x_end);
                    let vy1 = (ty + tile_h).min(y_end);
                    if vx1 > vx0 && vy1 > vy0 {
                        // Snap tile edges to device pixels (CSS Backgrounds §3.9):
                        // neighbours share the same rounded edge, so a fractional
                        // position (`center`) leaves no 1px AA seam between tiles.
                        let (sx0, sx1) = (tx.round(), (tx + tile_w).round().max(tx.round() + 1.0));
                        let (sy0, sy1) = (ty.round(), (ty + tile_h).round().max(ty.round() + 1.0));
                        let placed = Rect::new(sx0, sy0, sx1 - sx0, sy1 - sy0);
                        let (vx0, vx1) = (vx0.round().max(sx0), vx1.round().min(sx1));
                        let (vy0, vy1) = (vy0.round().max(sy0), vy1.round().min(sy1));
                        if vx1 > vx0 && vy1 > vy0 {
                            let visible = Rect::new(vx0, vy0, vx1 - vx0, vy1 - vy0);
                            // SVG: redraw at the tile's device size instead of
                            // stretching the intrinsic raster (BUG-1296).
                            let tile = vector_tile(img, placed.width, placed.height, &mut vector_cache);
                            blit_image_into_rect(pixmap, placed, visible, tile.unwrap_or(img), 1.0, clip)?;
                        }
                    }
                }
                if !repeat_x {
                    break;
                }
                tx += step_x;
            }
        }
        if !repeat_y {
            break;
        }
        ty += step_y;
    }
    Ok(())
}

/// Raster of `img`'s vector source ([`Image::vector_source`]) at exactly the
/// `w`×`h` device pixels a tile occupies, or `None` when `img` is not vector or
/// already has that size. Tiles of one background share a size, so the last
/// render is kept in `cache` and reused.
fn vector_tile<'a>(img: &Image, w: f32, h: f32, cache: &'a mut Option<Image>) -> Option<&'a Image> {
    let source = img.vector_source.as_ref()?;
    let (tw, th) = (w.round().max(1.0) as u32, h.round().max(1.0) as u32);
    if (tw, th) == (img.width, img.height) {
        return None;
    }
    if cache.as_ref().is_none_or(|c| (c.width, c.height) != (tw.min(4096), th.min(4096))) {
        *cache = lumen_image::rasterize_svg(source, tw, th).ok();
    }
    cache.as_ref()
}

/// Convert a decoded [`Image`] (straight RGBA8) into a premultiplied tiny-skia
/// [`tiny_skia::Pixmap`] for use as a pattern shader source.
fn image_to_pixmap(img: &Image) -> Option<tiny_skia::Pixmap> {
    let rgba = img.to_rgba8();
    let mut pm = tiny_skia::Pixmap::new(img.width, img.height)?;
    for (dst, chunk) in pm.pixels_mut().iter_mut().zip(rgba.chunks_exact(4)) {
        let a = chunk[3];
        *dst = tiny_skia::PremultipliedColorU8::from_rgba(
            premultiply_channel(chunk[0], a),
            premultiply_channel(chunk[1], a),
            premultiply_channel(chunk[2], a),
            a,
        )?;
    }
    Some(pm)
}

/// Premultiply one 8-bit colour channel by an 8-bit alpha (rounded).
#[inline]
fn premultiply_channel(c: u8, a: u8) -> u8 {
    ((u16::from(c) * u16::from(a) + 127) / 255) as u8
}

#[inline]
fn color_to_skia(color: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(color.r, color.g, color.b, color.a)
}

/// Parsed bundled face plus the tables `rasterize_text` needs. `None` if the
/// embedded font fails to parse (should never happen for committed Inter).
///
/// Shaping itself (cmap mapping + GSUB/GPOS or `rustybuzz`, LIB-1) does not
/// need a table held here — `lumen_font::active_text_shaper()` re-derives
/// them from the same bytes per call, same as `build_face` already re-parses
/// this struct once per `DrawText` run.
///
/// `ascent_override`/`descent_override`/`size_adjust` (FONTLOAD-18, CSS Fonts
/// L4 §14) are `None` unless [`resolve_face`] matched a registered
/// `@font-face` — a bare `build_face` never sets them, [`rasterize_text`]
/// copies them in from the resolved [`lumen_core::FaceRecord`] afterwards.
struct CpuFace<'a> {
    font: lumen_font::Font<'a>,
    units_per_em: u16,
    ascent: f32,
    descent: f32,
    ascent_override: Option<f32>,
    descent_override: Option<f32>,
    size_adjust: Option<f32>,
}

/// Parse a face from `bytes` once per `DrawText` run — `bytes` comes from
/// [`resolve_face`] (bundled Inter by default, a registered `@font-face`, or a
/// system face under `LUMEN_CPU_SYSTEM_FONTS`). `None` if the buffer isn't a
/// valid sfnt (should never happen for bundled Inter; a corrupt/unreadable
/// face is already filtered out by [`resolve_face`]'s own fallback).
fn build_face(bytes: &[u8]) -> Option<CpuFace<'_>> {
    let font = lumen_font::Font::parse(bytes).ok()?;
    let head = font.head().ok()?;
    let hhea = font.hhea().ok()?;
    Some(CpuFace {
        font,
        units_per_em: head.units_per_em,
        ascent: f32::from(hhea.ascent),
        descent: f32::from(hhea.descent),
        ascent_override: None,
        descent_override: None,
        size_adjust: None,
    })
}

/// One `@font-face` cascade candidate ready for itemizing/shaping (FONTLOAD-19):
/// a parsed [`CpuFace`] plus its declared `unicode-range` (CSS Fonts L4 §5.1,
/// empty = unrestricted) and a pre-parsed `cmap` for the per-codepoint
/// coverage check in [`itemize_by_cascade`] — parsed once per `DrawText` run,
/// not once per character.
struct FaceCandidate<'a> {
    /// Raw sfnt bytes — what [`lumen_font::TextShaper::shape`] needs (it
    /// re-derives its own `rustybuzz::Face` from bytes, same as everywhere
    /// else in this file).
    bytes: &'a [u8],
    face: CpuFace<'a>,
    /// `(start, end)` inclusive codepoint ranges; empty = unrestricted (see
    /// [`lumen_core::codepoint_in_face_ranges`]).
    ranges: Vec<(u32, u32)>,
    /// `None` when the candidate's `cmap` table itself fails to parse — such
    /// a candidate never wins the cascade (mirrors `pick_face_for_codepoint`'s
    /// `metrics.as_ref()?` requirement on the wgpu side).
    cmap: Option<lumen_font::Cmap<'a>>,
}

impl<'a> FaceCandidate<'a> {
    fn from_parts(bytes: &'a [u8], record: &Option<lumen_core::FaceRecord>) -> Option<Self> {
        let mut face = build_face(bytes)?;
        if let Some(r) = record {
            face.ascent_override = r.ascent_override;
            face.descent_override = r.descent_override;
            face.size_adjust = r.size_adjust;
        }
        let ranges = record.as_ref().map(|r| r.unicode_ranges.clone()).unwrap_or_default();
        let cmap = face.font.cmap().ok();
        Some(Self { bytes, face, ranges, cmap })
    }
}

/// Builds the ordered [`FaceCandidate`] cascade from
/// [`resolve_face_candidates`]'s list — `None` if candidate 0 (the primary
/// pick) itself fails to parse, matching the pre-FONTLOAD-19 hard failure on
/// a corrupt/undecodable primary face. A sibling that fails to parse is
/// dropped from the cascade instead of failing the whole draw — it just
/// never wins [`itemize_by_cascade`].
fn build_cascade(
    candidates: &[(Vec<u8>, Option<lumen_core::FaceRecord>)],
) -> Option<Vec<FaceCandidate<'_>>> {
    let primary = FaceCandidate::from_parts(&candidates[0].0, &candidates[0].1)?;
    let mut cascade = vec![primary];
    for (bytes, record) in &candidates[1..] {
        if let Some(candidate) = FaceCandidate::from_parts(bytes, record) {
            cascade.push(candidate);
        }
    }
    Some(cascade)
}

/// Splits `text` into maximal contiguous stretches assigned to one cascade
/// candidate each, by codepoint coverage: the candidate's declared
/// `unicode-range` must include the codepoint AND its own `cmap` must carry a
/// real (non-`.notdef`) glyph for it. Mirrors the wgpu renderer's
/// per-codepoint `pick_face_for_codepoint` (FONTLOAD-9/BUG-434), but resolved
/// **before** shaping rather than per already-shaped glyph: CPU rendering
/// runs a real `rustybuzz` shaping pass per contiguous run (unlike the wgpu
/// path's own per-char cmap-only loop with no GSUB/GPOS), so a cascade
/// boundary here is also a shaping boundary — a ligature cannot span two
/// `@font-face` subsets, matching real browsers (a `unicode-range` split
/// can't join a ligature across files either).
///
/// A codepoint no candidate declares falls back to candidate 0 (primary),
/// same fallback `pick_face_for_codepoint` uses (draws `.notdef` from the
/// primary face rather than silently picking an unrelated candidate).
///
/// **Fast path:** a single candidate (the overwhelming common case — no
/// `unicode-range` subsetting on the page) returns the whole `text` as one
/// run without inspecting a single codepoint, so a page with no BUG-434
/// siblings pays zero itemization cost and stays byte-identical to the
/// pre-FONTLOAD-19 single-shape path.
fn itemize_by_cascade<'t>(text: &'t str, cascade: &[FaceCandidate]) -> Vec<(usize, &'t str)> {
    if text.is_empty() || cascade.len() <= 1 {
        return vec![(0, text)];
    }
    let mut runs = Vec::new();
    let mut run_start = 0usize;
    let mut run_candidate = usize::MAX;
    for (byte_idx, ch) in text.char_indices() {
        let cp = ch as u32;
        let candidate = cascade
            .iter()
            .position(|c| {
                lumen_core::codepoint_in_face_ranges(cp, &c.ranges)
                    && c.cmap
                        .as_ref()
                        .and_then(|m| m.glyph_index(cp))
                        .is_some_and(|g| g != 0)
            })
            .unwrap_or(0);
        if run_candidate == usize::MAX {
            run_candidate = candidate;
        } else if candidate != run_candidate {
            runs.push((run_candidate, &text[run_start..byte_idx]));
            run_start = byte_idx;
            run_candidate = candidate;
        }
    }
    runs.push((run_candidate, &text[run_start..]));
    runs
}

/// Ph3 writing-mode vertical, Срез 1 — render `text` rotated 90° clockwise
/// (CSS Writing Modes L4 §4, `text-orientation: sideways`/`mixed`).
///
/// Renders the run horizontally into a full-canvas-sized local buffer via
/// [`rasterize_text`] with the origin at `(0, 0)` and no clip, then
/// composites that buffer onto `pixmap` rotated 90° CW around the local
/// origin and translated so the rotated run's top-left lands at `rect`'s
/// layout-assigned column position — `dest = (-y, x) + rect.xy`. This makes
/// the run flow top→bottom starting at `rect`, matching
/// `wrap_inline_run_vertical`'s column placement, without duplicating the
/// glyph-shaping/blit loop.
///
/// A run whose horizontal advance would exceed the canvas width is truncated
/// by the local render's own bounds check (same as the unrotated path) —
/// real pages wrap vertical runs to the viewport well within that bound.
#[allow(clippy::too_many_arguments)]
fn rasterize_text_rotated(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    text: &str,
    font_size: f32,
    color: &Color,
    tab_size: f32,
    clip: Option<&Rect>,
    font_weight: u16,
    font_style: lumen_layout::FontStyle,
    font_features: &[([u8; 4], u32)],
    font_family: &[String],
    font_provider: Option<&dyn FontProvider>,
) -> Result<Option<DrawBounds>, Box<dyn std::error::Error>> {
    let width = pixmap.width();
    let height = pixmap.height();
    let mut local = tiny_skia::Pixmap::new(width, height)
        .ok_or("Failed to create rotated-text layer")?;
    let local_rect = Rect { x: 0.0, y: 0.0, width: rect.width, height: rect.height };
    let Some((l, t, r, b)) = rasterize_text(
        &mut local, &local_rect, text, font_size, color, tab_size, None, font_weight,
        font_style, font_features, font_family, font_provider, None, (0.0, 0.0),
    )?
    else {
        return Ok(None);
    };

    let transform = tiny_skia::Transform::from_row(0.0, 1.0, -1.0, 0.0, rect.x + rect.width, rect.y);
    let clip_mask = build_clip_mask(width, height, clip.copied());
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    pixmap.draw_pixmap(0, 0, local.as_ref(), &paint, transform, clip_mask.as_ref());

    let rotate = |x: f32, y: f32| (-y + rect.x + rect.width, x + rect.y);
    let corners = [rotate(l, t), rotate(r, t), rotate(r, b), rotate(l, b)];
    let (mut dl, mut dt, mut dr, mut db) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for (x, y) in corners {
        dl = dl.min(x);
        dt = dt.min(y);
        dr = dr.max(x);
        db = db.max(y);
    }
    Ok(Some((dl, dt, dr, db)))
}

/// Measures the shaped horizontal advance of `text` without rendering —
/// mirrors the tab/itemize/shape/advance loop in [`rasterize_text`] (FONTLOAD-19:
/// same cascade, so a unicode-range-subsetted segment advances by the same
/// amount `rasterize_text` would draw it at). Needed by
/// [`rasterize_text_mixed`] because a whitespace-only `Other` segment
/// produces no ink (so [`rasterize_text`]'s returned bbox can't place it) but
/// still has to advance the column by its real width.
fn measure_run_advance(
    cascade: &[FaceCandidate],
    text: &str,
    font_size: f32,
    tab_size: f32,
    font_features: &[([u8; 4], u32)],
) -> f32 {
    let mut total = 0.0_f32;
    let mut first_segment = true;
    let segments: Vec<&str> = if tab_size > 0.0 { text.split('\t').collect() } else { vec![text] };
    for segment in segments {
        if !first_segment {
            total += tab_size;
        }
        first_segment = false;
        if segment.is_empty() {
            continue;
        }
        for (candidate_idx, sub_text) in itemize_by_cascade(segment, cascade) {
            if sub_text.is_empty() {
                continue;
            }
            let candidate = &cascade[candidate_idx];
            // FONTLOAD-18 (CSS Fonts L4 §14.4): `size-adjust` premultiplies
            // font-size before advances are computed — same formula
            // `rasterize_text` applies per candidate.
            let adjusted_font_size = font_size * candidate.face.size_adjust.unwrap_or(1.0);
            let advance_scale = adjusted_font_size / f32::from(candidate.face.units_per_em);
            let shaped = lumen_font::active_text_shaper().shape(
                candidate.bytes,
                sub_text,
                lumen_core::ext::ShapeDirection::LeftToRight,
                None,
                font_features,
                &[],
            );
            total += shaped.iter().map(|sg| sg.x_advance as f32 * advance_scale).sum::<f32>();
        }
    }
    total
}

/// Ph3 writing-mode vertical, Срез 3 — per-glyph split for `text-orientation:
/// mixed`: each CJK ideograph paints upright (no rotation — [`rasterize_text`]
/// runs directly against `pixmap` at `rect` shifted down by `y_cursor`, same
/// as the plain horizontal path); each run of non-CJK characters paints as
/// one block rotated 90° CW like [`rasterize_text_rotated`] — shaping the
/// whole run together keeps Latin kerning/ligatures intact, and needs a
/// full-canvas local buffer (via [`rasterize_text`] with `text_orientation:
/// None`) composited onto `pixmap` with the rotation transform. The upright
/// branch must NOT go through that local-buffer-at-origin-then-translate
/// route: a local buffer always anchors at y=0, so any glyph ink that
/// extends above the assumed ascent offset (common for CJK, whose visual
/// bbox often exceeds the Latin-derived ascent metric) gets clipped at the
/// canvas edge before the translate ever runs — rendering straight at the
/// real `rect.y + y_cursor` keeps the same headroom the plain horizontal
/// path has. [`measure_run_advance`] advances `y_cursor` by each segment's
/// real shaped width (ink bbox alone would misplace a whitespace-only
/// segment).
#[allow(clippy::too_many_arguments)]
fn rasterize_text_mixed(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    text: &str,
    font_size: f32,
    color: &Color,
    tab_size: f32,
    clip: Option<&Rect>,
    font_weight: u16,
    font_style: lumen_layout::FontStyle,
    font_features: &[([u8; 4], u32)],
    font_family: &[String],
    font_provider: Option<&dyn FontProvider>,
) -> Result<Option<DrawBounds>, Box<dyn std::error::Error>> {
    let candidates = resolve_face_candidates(font_provider, font_family, font_weight, font_style);
    let Some(cascade) = build_cascade(&candidates) else {
        return Ok(None);
    };
    let width = pixmap.width();
    let height = pixmap.height();
    let local_rect = Rect { x: 0.0, y: 0.0, width: rect.width, height: rect.height };
    let clip_mask = build_clip_mask(width, height, clip.copied());
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };

    let mut y_cursor = 0.0_f32;
    let mut ink: Option<DrawBounds> = None;
    let mark_ink = |b: DrawBounds, ink: &mut Option<DrawBounds>| {
        *ink = Some(match *ink {
            None => b,
            Some((l, t, r, btm)) => (l.min(b.0), t.min(b.1), r.max(b.2), btm.max(b.3)),
        });
    };

    for seg in crate::display_list::split_mixed_runs(text) {
        let (seg_text, upright) = match seg {
            crate::display_list::MixedSegment::Cjk(ch) => {
                let mut s = String::new();
                s.push(ch);
                (s, true)
            }
            crate::display_list::MixedSegment::Other(s) => (s, false),
        };
        let advance = measure_run_advance(&cascade, &seg_text, font_size, tab_size, font_features);
        if upright {
            let dest_rect =
                Rect { x: rect.x, y: rect.y + y_cursor, width: rect.width, height: rect.height };
            if let Some(b) = rasterize_text(
                pixmap, &dest_rect, &seg_text, font_size, color, tab_size, clip, font_weight,
                font_style, font_features, font_family, font_provider, None, (0.0, 0.0),
            )? {
                mark_ink(b, &mut ink);
            }
        } else {
            let mut local = tiny_skia::Pixmap::new(width, height)
                .ok_or("Failed to create mixed-orientation text layer")?;
            if let Some((l, t, r, b)) = rasterize_text(
                &mut local, &local_rect, &seg_text, font_size, color, tab_size, None, font_weight,
                font_style, font_features, font_family, font_provider, None, (0.0, 0.0),
            )? {
                let transform =
                    tiny_skia::Transform::from_row(0.0, 1.0, -1.0, 0.0, rect.x + rect.width, rect.y + y_cursor);
                pixmap.draw_pixmap(0, 0, local.as_ref(), &paint, transform, clip_mask.as_ref());
                let rotate = |x: f32, y: f32| (-y + rect.x + rect.width, x + rect.y + y_cursor);
                let corners = [rotate(l, t), rotate(r, t), rotate(r, b), rotate(l, b)];
                let (mut dl, mut dt, mut dr, mut db) =
                    (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
                for (x, y) in corners {
                    dl = dl.min(x);
                    dt = dt.min(y);
                    dr = dr.max(x);
                    db = db.max(y);
                }
                mark_ink((dl, dt, dr, db), &mut ink);
            }
        }
        y_cursor += advance;
    }
    Ok(ink)
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
///
/// Synthetic styles (RP-6): the bundled face is Regular-only, so
/// `font_weight >= 600` is emulated by double-blitting each glyph with a small
/// horizontal offset (fake bold) and a non-`Normal` `font_style` by shearing
/// the glyph outline (fake italic). Both leave advances untouched — layout
/// metrics must not change.
///
/// `text_orientation: Some(Sideways)` delegates to [`rasterize_text_rotated`]
/// (Ph3 writing-mode vertical, Срез 1); `Some(Mixed)` delegates to
/// [`rasterize_text_mixed`] (Срез 3 — per-glyph CJK-upright/Latin-rotated
/// split). `Upright` and `None` render horizontally as before.
#[allow(clippy::too_many_arguments)]
fn rasterize_text(
    pixmap: &mut tiny_skia::Pixmap,
    rect: &Rect,
    text: &str,
    font_size: f32,
    color: &Color,
    tab_size: f32,
    clip: Option<&Rect>,
    font_weight: u16,
    font_style: lumen_layout::FontStyle,
    font_features: &[([u8; 4], u32)],
    font_family: &[String],
    font_provider: Option<&dyn FontProvider>,
    text_orientation: Option<TextOrientation>,
    (letter_spacing, word_spacing): (f32, f32),
) -> Result<Option<DrawBounds>, Box<dyn std::error::Error>> {
    if text.is_empty() || font_size <= 0.0 || color.a == 0 {
        return Ok(None);
    }
    // Ph3 writing-mode vertical: `Sideways` rotates the whole run 90° CW
    // (Срез 1); `Mixed` splits per glyph — CJK upright, Latin rotated (Срез
    // 3). `Upright` and the horizontal (`None`) case fall through to the
    // existing unrotated path — `Upright`'s per-glyph vertical advance is a
    // separate follow-up.
    match text_orientation {
        Some(TextOrientation::Sideways) => {
            return rasterize_text_rotated(
                pixmap, rect, text, font_size, color, tab_size, clip, font_weight, font_style,
                font_features, font_family, font_provider,
            );
        }
        Some(TextOrientation::Mixed) => {
            return rasterize_text_mixed(
                pixmap, rect, text, font_size, color, tab_size, clip, font_weight, font_style,
                font_features, font_family, font_provider,
            );
        }
        _ => {}
    }
    // FONTLOAD-19 (CSS Fonts L4 §5.1 `unicode-range`, BUG-434): candidate 0
    // is exactly the old single-face `resolve_face` pick; further candidates
    // are its unicode-range siblings (subsets of the SAME declared family —
    // see `resolve_face_candidates`), letting `itemize_by_cascade` route each
    // sub-run of `text` to the subset that actually declares it, the same
    // cascade the wgpu renderer already does per-codepoint (FONTLOAD-9).
    let candidates = resolve_face_candidates(font_provider, font_family, font_weight, font_style);
    let Some(cascade) = build_cascade(&candidates) else {
        return Ok(None);
    };
    let primary = &cascade[0];
    // CSS Fonts L4 §14 (FONTLOAD-18): mirrors `push_text_glyphs`
    // (`renderer/glyph_raster.rs`) — `ascent-override`/`descent-override`
    // replace the real hhea metrics at font-unit level (byte-identical when
    // absent). Baseline position is computed from the PRIMARY candidate only
    // and stays fixed for the whole command — exactly like `push_text_glyphs`,
    // which also never lets a per-glyph fallback face move the baseline.
    let ascent_units = primary
        .face
        .ascent_override
        .map_or(primary.face.ascent, |pct| pct * f32::from(primary.face.units_per_em));
    let descent_units = primary
        .face
        .descent_override
        .map_or(primary.face.descent, |pct| -(pct * f32::from(primary.face.units_per_em)));
    let denom = ascent_units - descent_units;
    let ascent_ratio = if denom != 0.0 { ascent_units / denom } else { 0.8 };
    let primary_adjusted_font_size = font_size * primary.face.size_adjust.unwrap_or(1.0);
    let baseline_y = rect.y + primary_adjusted_font_size * ascent_ratio;
    let bold_offset = (font_size / 24.0).clamp(0.5, 2.0);
    let synth_bold = font_weight >= 600;
    let synth_italic = !matches!(font_style, lumen_layout::FontStyle::Normal);

    let width = pixmap.width();
    let height = pixmap.height();
    let mut mask = tiny_skia::Mask::new(width, height).ok_or("Failed to create glyph mask")?;
    // TEXTQ-2: gamma/contrast correction of glyph coverage by the text colour's
    // luminance — the same function the wgpu text shader evaluates.
    let coverage_lut = crate::text_gamma::coverage_lut(color.r, color.g, color.b);
    let mut any_coverage = false;
    // BUG-267: union bbox of every glyph bitmap actually blitted (page px),
    // returned to the caller for the layer's dirty tracking. Uses the unclipped
    // bitmap extent — a superset of the written pixels.
    let mut ink: Option<DrawBounds> = None;
    let mark_ink = |b: DrawBounds, ink: &mut Option<DrawBounds>| {
        *ink = Some(match *ink {
            None => b,
            Some((l, t, r, btm)) => (l.min(b.0), t.min(b.1), r.max(b.2), btm.max(b.3)),
        });
    };

    // Tab handling (CSS Text L3 §10.1): a tab advances by `tab_size` pixels
    // and draws nothing. Shaping operates per tab-delimited segment so a tab
    // never participates in a ligature or kern pair. When `tab_size <= 0` the
    // whole string is shaped as one run and tabs fall through as glyphs,
    // matching the previous behaviour.
    let mut cursor_x = rect.x;
    let mut first_segment = true;
    let segments: Vec<&str> = if tab_size > 0.0 {
        text.split('\t').collect()
    } else {
        vec![text]
    };
    for segment in segments {
        if !first_segment {
            cursor_x += tab_size;
        }
        first_segment = false;
        if segment.is_empty() {
            continue;
        }
        // FONTLOAD-19: route each sub-run to the cascade candidate that
        // actually declares it (see `itemize_by_cascade`) — a no-op split
        // (one sub-run == the whole segment) when there's no unicode-range
        // sibling, which is the common case.
        for (candidate_idx, sub_text) in itemize_by_cascade(segment, &cascade) {
            if sub_text.is_empty() {
                continue;
            }
            let candidate = &cascade[candidate_idx];
            // `size-adjust` (CSS Fonts L4 §14.4) premultiplies `font_size` for
            // THIS candidate before advances/rasterizer size are derived from
            // it — a fallback candidate can declare its own `size-adjust`
            // independent of the primary's (same principle FONTLOAD-17
            // already applies per-glyph on the wgpu side).
            let adjusted_font_size = font_size * candidate.face.size_adjust.unwrap_or(1.0);
            let advance_scale = adjusted_font_size / f32::from(candidate.face.units_per_em);
            let rasterizer =
                lumen_font::Rasterizer::new(adjusted_font_size, candidate.face.units_per_em);
            // Shape via `lumen_font::active_text_shaper()` (LIB-1/LIB-2/LIB-3):
            // `rustybuzz`, the sole shaper since LIB-3 deleted the own GSUB/GPOS
            // engine. No cross-family fallback WITHIN one sub-run: a missing
            // codepoint in `candidate.bytes` resolves to glyph 0 (.notdef) —
            // matching the GPU renderer's `(primary, 0)` result when it too has
            // no fallback configured. `candidate.bytes` is Inter by default, a
            // registered `@font-face` face or one of its unicode-range siblings
            // (FONTLOAD-18/19) or, failing that, the OS index under
            // `LUMEN_CPU_SYSTEM_FONTS` (`crate::cpu_font_resolve::resolve_face_candidates`).
            // TEXTQ-5: те же фичи, что у wgpu-пути и измерителя раскладки —
            // при ненулевом `letter-spacing` необязательные лигатуры выключены.
            let shaped = lumen_font::active_text_shaper().shape(
                candidate.bytes,
                sub_text,
                lumen_core::ext::ShapeDirection::LeftToRight,
                None,
                &crate::shaped_width::shaping_features(font_features, letter_spacing),
                &[],
            );
            for (glyph_idx, sg) in shaped.iter().enumerate() {
                let pen_x = cursor_x + sg.x_offset as f32 * advance_scale;
                let glyph_baseline = baseline_y - sg.y_offset as f32 * advance_scale;
                if let Ok(Some(mut glyph)) = candidate.face.font.glyph_resolved(sg.glyph_id) {
                    if synth_italic {
                        shear_glyph(&mut glyph);
                    }
                    if let Some(bitmap) = rasterizer.rasterize(&glyph) {
                        // Unclipped page-px extent of this glyph bitmap (mirrors the
                        // origin snap in `blit_glyph_coverage`).
                        let gx0 = (pen_x + bitmap.left).round();
                        let gy0 = (glyph_baseline - bitmap.top).round();
                        let gb = (gx0, gy0, gx0 + bitmap.width as f32, gy0 + bitmap.height as f32);
                        if blit_glyph_coverage(
                            &mut mask,
                            &bitmap,
                            pen_x,
                            glyph_baseline,
                            clip,
                            width,
                            height,
                            &coverage_lut,
                        ) {
                            any_coverage = true;
                            mark_ink(gb, &mut ink);
                        }
                        // Fake bold: second blit shifted right; the advance below
                        // stays the same so line metrics are unchanged.
                        if synth_bold
                            && blit_glyph_coverage(
                                &mut mask,
                                &bitmap,
                                pen_x + bold_offset,
                                glyph_baseline,
                                clip,
                                width,
                                height,
                                &coverage_lut,
                            )
                        {
                            any_coverage = true;
                            // The bold blit re-snaps its origin; ±1px covers the
                            // rounding drift of `bold_offset`.
                            mark_ink((gb.0 - 1.0, gb.1, gb.2 + bold_offset + 1.0, gb.3), &mut ink);
                        }
                    }
                }
                // CSS Text L3 §8.2: `letter-spacing` — на КЛАСТЕР, а не на глиф
                // (как у wgpu-пути): лигатура из двух символов получает его раз.
                let cluster_end = shaped.get(glyph_idx + 1).is_none_or(|n| n.cluster != sg.cluster);
                cursor_x += sg.x_advance as f32 * advance_scale;
                if cluster_end {
                    cursor_x += letter_spacing;
                }
                // CSS Text L3 §8.1 — `word-spacing` follows every word separator.
                if cluster_end
                    && word_spacing != 0.0
                    && sub_text.get(sg.cluster as usize..).is_some_and(|t| t.starts_with(' '))
                {
                    cursor_x += word_spacing;
                }
            }
        }
    }

    #[cfg(test)]
    TEST_PEN_END.with(|c| c.set(cursor_x));

    if !any_coverage {
        return Ok(None);
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
    Ok(ink)
}

#[cfg(test)]
thread_local! {
    /// Где остановилось перо после последнего [`rasterize_text`] — шов для
    /// теста «раскладка и CPU-путь дают одну ширину» (TEXTQ-5): ink-рамка
    /// слишком груба, чтобы сверять до сотых пикселя.
    static TEST_PEN_END: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
}

/// Horizontal shear factor for synthetic italic — tan 12°.
const ITALIC_SKEW: f32 = 0.2126;

/// Shear a glyph outline horizontally for synthetic italic:
/// `x' = x + y * ITALIC_SKEW` (font units, y-up), bbox expanded accordingly.
/// Composite outlines are left unchanged (the rasterizer skips them anyway).
fn shear_glyph(glyph: &mut lumen_font::Glyph) {
    match &mut glyph.outline {
        lumen_font::Outline::Simple(contours) => {
            for contour in contours.iter_mut() {
                for point in contour.points.iter_mut() {
                    point.x = (point.x as f32 + point.y as f32 * ITALIC_SKEW).round() as i16;
                }
            }
        }
        lumen_font::Outline::Composite(_) => return,
    }
    let s = ITALIC_SKEW;
    let candidates = [
        glyph.bbox.x_min as f32 + glyph.bbox.y_min as f32 * s,
        glyph.bbox.x_min as f32 + glyph.bbox.y_max as f32 * s,
        glyph.bbox.x_max as f32 + glyph.bbox.y_min as f32 * s,
        glyph.bbox.x_max as f32 + glyph.bbox.y_max as f32 * s,
    ];
    let mut min_val = candidates[0];
    let mut max_val = candidates[0];
    for &v in &candidates[1..] {
        min_val = min_val.min(v);
        max_val = max_val.max(v);
    }
    glyph.bbox.x_min = min_val.floor() as i16;
    glyph.bbox.x_max = max_val.ceil() as i16;
}

/// Composite one glyph's coverage bitmap into `mask`, returning whether any
/// pixel was written. The bitmap's top-left maps to page coordinates
/// `(pen_x + bitmap.left, baseline_y − bitmap.top)`, rounded to the nearest
/// pixel for a deterministic pen-to-pixel snap. Pixels outside the pixmap or
/// the rectangular `clip` are dropped; overlapping glyph pixels keep the max
/// coverage (glyphs in a run rarely overlap, but kerning-tight pairs can).
/// `lut` maps raw glyph coverage to gamma-corrected coverage ([`crate::text_gamma`]).
#[allow(clippy::too_many_arguments)]
fn blit_glyph_coverage(
    mask: &mut tiny_skia::Mask,
    bitmap: &lumen_font::Bitmap,
    pen_x: f32,
    baseline_y: f32,
    clip: Option<&Rect>,
    width: u32,
    height: u32,
    lut: &[u8; 256],
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
            let cov = lut[cov as usize];
            if cov > data[idx] {
                data[idx] = cov;
            }
            wrote = true;
        }
    }
    wrote
}

#[cfg(test)]
mod tests;

