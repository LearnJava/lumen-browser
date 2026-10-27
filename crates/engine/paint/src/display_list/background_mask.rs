//! P1/SPLIT-DL12: background/gradient/mask эмиссия — tile-геометрия
//! градиентных фонов, `emit_background_layer`/`emit_background_image`,
//! CSS Masking (`emit_push_mask`/`emit_push_mask_layer`). Вынесено из
//! `display_list.rs` (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL,
//! батч DL-12).

use super::*;
use super::inline_frag::mask_clip_layer_rect;
use super::paint_worklet::{paint_image_key, paint_tile_size};
use lumen_layout::BgSizeAxis;

thread_local! {
    /// CSS Backgrounds L3 §3.6 — the viewport rect (scroll 0, page space) that
    /// `background-attachment: fixed` layers of the display list being built
    /// are positioned against. `None` outside a build.
    static FIXED_BG_VIEWPORT: std::cell::Cell<Option<Rect>> = const { std::cell::Cell::new(None) };
}

/// Runs `build` — one or more display-list builds of a document laid out
/// against `viewport` — with that viewport installed as the positioning area of
/// its `background-attachment: fixed` layers (CSS Backgrounds L3 §3.6).
///
/// The layout tree does not carry the viewport: the root box is
/// `max(viewport, document)` tall, so without this scope a fixed background of
/// a page taller than the window would be positioned against the whole
/// document. Builders called outside it fall back to the root box's rect —
/// exact for a document no taller than its viewport.
pub fn with_fixed_background_viewport<R>(viewport: Size, build: impl FnOnce() -> R) -> R {
    struct Restore(Option<Rect>);
    impl Drop for Restore {
        fn drop(&mut self) {
            FIXED_BG_VIEWPORT.with(|c| c.set(self.0));
        }
    }
    let rect = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    let _restore = Restore(FIXED_BG_VIEWPORT.with(|c| c.replace(Some(rect))));
    build()
}

/// Installs the fixed-background viewport for one display-list build: keeps
/// the one an enclosing [`with_fixed_background_viewport`] set, otherwise
/// falls back to `root`'s rect. Restores the previous value on drop.
pub(crate) struct FixedBgViewportGuard(Option<Rect>);

impl FixedBgViewportGuard {
    pub(crate) fn install(root: &LayoutBox) -> Self {
        let prev = FIXED_BG_VIEWPORT.with(std::cell::Cell::get);
        if prev.is_none() {
            FIXED_BG_VIEWPORT.with(|c| c.set(Some(root.rect)));
        }
        Self(prev)
    }
}

impl Drop for FixedBgViewportGuard {
    fn drop(&mut self) {
        FIXED_BG_VIEWPORT.with(|c| c.set(self.0));
    }
}

/// Positioning area of a `background-attachment: fixed` layer — the viewport,
/// when a build installed one and it is non-degenerate.
pub(super) fn fixed_bg_viewport() -> Option<Rect> {
    FIXED_BG_VIEWPORT
        .with(std::cell::Cell::get)
        .filter(|r| r.width > 0.0 && r.height > 0.0)
}

/// CSS Backgrounds L3 §3.3–3.5 — прямоугольники-плитки для градиентного слоя с
/// явным `background-size`.
///
/// У градиента нет ни внутреннего размера, ни соотношения сторон (CSS Images),
/// поэтому при `background-size: <length>` он рисуется плитками этого размера,
/// размещёнными по `background-position` и повторёнными по `background-repeat`;
/// `auto`-ось разрешается в размер positioning area по этой оси (не
/// пропорциональное масштабирование — соотношения нет). Возвращает по одному
/// rect на плитку: каждый отображает цветовую линию/окружность градиента в свою
/// плитку. Геометрия зеркалит [`super::backends`] image-tiling
/// (`bg_tile_geometry` + loop), чтобы градиенты и картинки плитковались
/// одинаково. Вызывается только для `BackgroundSize::Length`;
/// auto/cover/contain заливают всю area одной командой.
pub(crate) fn gradient_tile_rects(
    tile_w: f32,
    tile_h: f32,
    position: ObjectPosition,
    repeat: BackgroundRepeat,
    origin: Rect,
    clip: Rect,
) -> Vec<Rect> {
    if tile_w <= 0.0 || tile_h <= 0.0 {
        return Vec::new();
    }
    let off_x = position.x.resolve(origin.width - tile_w);
    let off_y = position.y.resolve(origin.height - tile_h);

    let (tile_x_start, step_x, repeat_x) = repeat_axis_geometry(repeat.x, origin.x, origin.width, tile_w, off_x);
    let (tile_y_start, step_y, repeat_y) = repeat_axis_geometry(repeat.y, origin.y, origin.height, tile_h, off_y);

    // Cap, чтобы крошечная плитка с repeat не породила взрывное число команд.
    const MAX_TILES: usize = 4096;
    let mut rects = Vec::new();
    let x_end = clip.x + clip.width;
    let y_end = clip.y + clip.height;
    let mut ty = tile_y_start;
    loop {
        if ty >= y_end || rects.len() >= MAX_TILES {
            break;
        }
        if ty + tile_h > clip.y {
            let mut tx = tile_x_start;
            loop {
                if tx >= x_end || rects.len() >= MAX_TILES {
                    break;
                }
                if tx + tile_w > clip.x {
                    rects.push(Rect::new(tx, ty, tile_w, tile_h));
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
    rects
}

/// CSS Backgrounds L3 §3.3–3.5 — список rect-ов, в которые рисуется градиентный
/// слой, и нужно ли клипировать их по painting area.
///
/// `BackgroundSize::Length` → плитки через [`gradient_tile_rects`] (требуют клипа
/// по `clip`, т.к. плитка может выходить за painting area). Auto/Cover/Contain
/// (у градиента нет внутреннего размера/ratio) → одна команда на всю painting
/// area (`clip`) — историческое поведение, клип не нужен.
///
/// `fixed` — `background-attachment: fixed`: `origin` is the viewport and the
/// renderer moves the gradient with the scroll offset, so tiles are generated
/// over the whole viewport (not just the ones meeting `clip` at scroll 0) and
/// an auto-sized gradient spans the viewport rather than the painting area —
/// both then need the clip.
fn gradient_paint_rects(
    layer: &BackgroundLayer,
    origin: Rect,
    clip: Rect,
    fixed: bool,
) -> (Vec<Rect>, bool) {
    let tile_bounds = if fixed { origin } else { clip };
    match layer.size {
        BackgroundSize::Length(w, h) => {
            // Gradients have no intrinsic size/ratio: an `auto` axis falls back
            // to the positioning-area extent; percent resolves against it.
            let tile_w = w.resolve(origin.width).unwrap_or(origin.width).max(1.0);
            let tile_h = h.resolve(origin.height).unwrap_or(origin.height).max(1.0);
            let tiles = gradient_tile_rects(
                tile_w,
                tile_h,
                layer.position,
                layer.repeat,
                origin,
                tile_bounds,
            );
            (tiles, true)
        }
        _ if fixed => (vec![origin], true),
        _ => (vec![clip], false),
    }
}

/// Область, относительно которой [`emit_background_layer`] позиционирует и
/// обрезает слой.
#[derive(Clone, Copy)]
enum LayerArea {
    /// Сам бокс: origin/clip берутся по `background-origin`/`-clip` от `b.rect`.
    Own,
    /// CSS Backgrounds L3 §3.6 `background-attachment: local`: border-box-аналог
    /// прокручиваемой области (см. [`local_scroll_area`]) вместо `b.rect`.
    Local(Rect),
    /// `mask-clip: no-clip` (CSS Masking L1 §4.6): область рисования — этот
    /// прямоугольник, а не бокс `background-clip`; плитки слоя продолжаются по
    /// нему, и маска накрывает то, что выступает из элемента. Позиционирование — по
    /// `origin` самого бокса.
    Unclipped(Rect),
}

/// Эмитит одну background-layer команду.
///
/// CSS Compositing L1 §8.3: если `layer.blend_mode != Normal`, оборачивает
/// draw-команду в PushBlendMode/PopBlendMode. Слои рисуются снизу вверх,
/// каждый с указанным blend mode относительно уже нарисованных слоёв ниже.
///
/// `dpr` — device pixel ratio, передаётся в [`select_image_set_url`] для
/// выбора варианта `image-set()` (CSS Images L4 §5).
fn emit_background_layer(
    out: &mut Vec<DisplayCommand>,
    b: &LayoutBox,
    layer: &BackgroundLayer,
    dpr: f32,
    // CSS Compositing L1 §8.3: the bottom-most background layer blends with transparent
    // background-color. For premultiplied alpha, multiply(src, 0) = src (identity), so
    // blend mode has no visible effect — skip PushBlendMode to avoid blending against the
    // stacking context instead of an isolated background canvas.
    suppress_blend: bool,
    // CSS Backgrounds L3 §4.3: a gradient background is clipped to the same
    // rounded painting-area box as a solid `background-color` (BUG-631) — the
    // border-box radii, reused as-is for any `background-clip` box (same
    // simplification the solid-color path already makes at the `FillRoundedRect`
    // call site above).
    radii: CornerRadii,
    area: LayerArea,
) {
    // CSS Backgrounds L4 §3.8 `background-clip: text` — paint the layer as for
    // `border-box`, then keep only what lies under the glyphs.
    if layer.clip == BackgroundClip::Text {
        let mut plain = layer.clone();
        plain.clip = BackgroundClip::BorderBox;
        emit_text_clipped(out, b, dpr, |inner| {
            emit_background_layer(inner, b, &plain, dpr, suppress_blend, radii, area);
        });
        return;
    }
    // CSS Backgrounds L4 §3.8 `background-clip: border-area` — paint the layer
    // as for `border-box`, then keep only what lies under the drawn border.
    if layer.clip == BackgroundClip::BorderArea {
        let mut plain = layer.clone();
        plain.clip = BackgroundClip::BorderBox;
        emit_border_area_clipped(out, b, |inner| {
            emit_background_layer(inner, b, &plain, dpr, suppress_blend, radii, area);
        });
        return;
    }
    let clip = match area {
        LayerArea::Unclipped(r) => r,
        LayerArea::Local(r) => background_clip_rect_in(b, r, layer.clip),
        LayerArea::Own => background_clip_rect(b, layer.clip),
    };
    if clip.width <= 0.0 || clip.height <= 0.0 {
        return;
    }
    // CSS Images L4 §4 — `cross-fade()` с хотя бы одной не-URL стороной
    // (градиент, вложенный `cross-fade()`): `DrawCrossFade` умеет смешивать
    // только две растровые текстуры, поэтому сторона `a` рисуется как обычный
    // слой, а сторона `b` — поверх, внутри opacity-группы с альфой `t`. Для
    // непрозрачных картинок это тот же результат, что даёт `DrawCrossFade`
    // (`a` целиком, `b` с прогрессом `t`). Blend-режим слоя оборачивает пару
    // целиком; сами стороны рисуются с `Normal`.
    if let BackgroundImage::CrossFade { a, b: side_b, t } = &layer.image
        && !matches!((a.as_ref(), side_b.as_ref()), (BackgroundImage::Url(_), BackgroundImage::Url(_)))
    {
        let use_blend = !suppress_blend && layer.blend_mode != LayoutBlendMode::Normal;
        if use_blend {
            out.push(DisplayCommand::PushBlendMode { mode: map_blend_mode(layer.blend_mode), bounds: clip });
        }
        let mut side = layer.clone();
        side.blend_mode = LayoutBlendMode::Normal;
        side.image = (**a).clone();
        emit_background_layer(out, b, &side, dpr, true, radii, area);
        side.image = (**side_b).clone();
        out.push(DisplayCommand::PushOpacity { alpha: t.clamp(0.0, 1.0), bounds: Some(clip) });
        emit_background_layer(out, b, &side, dpr, true, radii, area);
        out.push(DisplayCommand::PopOpacity);
        if use_blend {
            out.push(DisplayCommand::PopBlendMode);
        }
        return;
    }
    // CSS Backgrounds L3 §3.5: positioning area (background-origin) is independent of
    // the painting/clip area (background-clip). size/position calculations use origin_rect.
    //
    // CSS Backgrounds L3 §3.6 — `background-attachment: fixed`: the positioning
    // area is the viewport (initial containing block) instead of
    // `background-origin`'s box, and the picture must not move when the page
    // scrolls. The list is scroll-independent, so the layer's draw commands are
    // bracketed by `BeginFixedBackground`/`EndFixedBackground` and the renderer
    // cancels the page scroll for their positioning geometry (not for the
    // painting-area clip). `local` layers of a scroll container are emitted by
    // `emit_local_background` inside the container's scroll layer, with `area`.
    let fixed_vp = if layer.attachment == BackgroundAttachment::Fixed {
        fixed_bg_viewport()
    } else {
        None
    };
    let fixed = fixed_vp.is_some();
    let origin = fixed_vp.unwrap_or_else(|| match area {
        LayerArea::Local(r) => background_origin_rect_in(b, r, layer.origin),
        LayerArea::Own | LayerArea::Unclipped(_) => background_origin_rect(b, layer.origin),
    });
    let use_blend = !suppress_blend && layer.blend_mode != LayoutBlendMode::Normal;
    if use_blend {
        out.push(DisplayCommand::PushBlendMode { mode: map_blend_mode(layer.blend_mode), bounds: clip });
    }
    if fixed {
        out.push(DisplayCommand::BeginFixedBackground);
    }
    match &layer.image {
        BackgroundImage::Url(src) if !src.is_empty() => {
            // image-set: resolve image-set() to the best URL for the
            // current device pixel ratio; plain urls pass through unchanged.
            // Parsing keeps the raw `image-set(…)` string in BackgroundImage::Url
            // (`style/parse/image.rs`, `style/apply/paint.rs`), so this resolution
            // triggers (CSS Images L4 §5).
            let resolved = if is_image_set(src) {
                select_image_set_url(src, dpr)
            } else {
                src.as_str()
            };
            if !resolved.is_empty() {
                out.push(DisplayCommand::DrawBackgroundImage {
                    rect: clip,
                    origin_rect: origin,
                    src: resolved.to_string(),
                    size: layer.size,
                    position: layer.position,
                    repeat: layer.repeat,
                    image_rendering: b.style.image_rendering,
                });
            }
        }
        BackgroundImage::Gradient(ParsedGradient::Linear { angle_deg, corner, stops, repeating }) => {
            let (rects, needs_clip) = gradient_paint_rects(layer, origin, clip, fixed);
            // BUG-631: a rounded box needs its gradient clipped to the rounded
            // painting area even when `needs_clip` is false (single full-`clip`
            // rect, otherwise unclipped) — square corners must not leak through.
            let has_radii = !radii.all_zero();
            if (needs_clip || has_radii) && !rects.is_empty() {
                if has_radii {
                    out.push(DisplayCommand::PushClipRoundedRect {
                        rect: clip,
                        radii: [radii.tl, radii.tr, radii.br, radii.bl],
                    });
                } else {
                    out.push(DisplayCommand::PushClipRect { rect: clip });
                }
            }
            for r in &rects {
                // CSS Images L3 §3.1 — a `to <corner>` keyword's true angle
                // depends on this paint rect's aspect ratio; an explicit
                // `<angle>` is box-independent and passes through unchanged.
                let resolved_angle = corner.map_or(*angle_deg, |c| c.angle_deg(r.width, r.height));
                out.push(DisplayCommand::DrawLinearGradient {
                    rect: *r,
                    angle_deg: resolved_angle,
                    stops: stops.clone(),
                    repeating: *repeating,
                });
            }
            if (needs_clip || has_radii) && !rects.is_empty() {
                out.push(DisplayCommand::PopClip);
            }
        }
        BackgroundImage::Gradient(ParsedGradient::Radial {
            center_x_pct, center_y_pct, shape, size, stops, repeating,
        }) => {
            let (rects, needs_clip) = gradient_paint_rects(layer, origin, clip, fixed);
            let has_radii = !radii.all_zero();
            if (needs_clip || has_radii) && !rects.is_empty() {
                if has_radii {
                    out.push(DisplayCommand::PushClipRoundedRect {
                        rect: clip,
                        radii: [radii.tl, radii.tr, radii.br, radii.bl],
                    });
                } else {
                    out.push(DisplayCommand::PushClipRect { rect: clip });
                }
            }
            for r in &rects {
                // Resolve the CSS ending-shape/size to concrete px radii against
                // this paint rect (CSS Images L3 §3.5.1) — circle keeps rx == ry,
                // ellipse gets independent radii (BUG-239).
                let (radius_x, radius_y) = lumen_layout::radial_gradient_radii(
                    *shape, *size, *center_x_pct, *center_y_pct, r.width, r.height,
                );
                out.push(DisplayCommand::DrawRadialGradient {
                    rect: *r,
                    center_x_pct: *center_x_pct,
                    center_y_pct: *center_y_pct,
                    radius_x,
                    radius_y,
                    stops: stops.clone(),
                    repeating: *repeating,
                });
            }
            if (needs_clip || has_radii) && !rects.is_empty() {
                out.push(DisplayCommand::PopClip);
            }
        }
        BackgroundImage::Gradient(ParsedGradient::Conic {
            center_x_pct, center_y_pct, from_angle_deg, stops, repeating
        }) => {
            let (rects, needs_clip) = gradient_paint_rects(layer, origin, clip, fixed);
            let has_radii = !radii.all_zero();
            if (needs_clip || has_radii) && !rects.is_empty() {
                if has_radii {
                    out.push(DisplayCommand::PushClipRoundedRect {
                        rect: clip,
                        radii: [radii.tl, radii.tr, radii.br, radii.bl],
                    });
                } else {
                    out.push(DisplayCommand::PushClipRect { rect: clip });
                }
            }
            for r in &rects {
                out.push(DisplayCommand::DrawConicGradient {
                    rect: *r,
                    center_x_pct: *center_x_pct,
                    center_y_pct: *center_y_pct,
                    from_angle_deg: *from_angle_deg,
                    stops: stops.clone(),
                    repeating: *repeating,
                });
            }
            if (needs_clip || has_radii) && !rects.is_empty() {
                out.push(DisplayCommand::PopClip);
            }
        }
        BackgroundImage::CrossFade { a, b, t } => {
            // CSS Images L4 §4 — emit DrawCrossFade for two-URL cross-fade.
            // Не-URL стороны (градиенты, вложенный cross-fade) обработаны выше, в начале функции.
            if let (BackgroundImage::Url(url_a), BackgroundImage::Url(url_b)) =
                (a.as_ref(), b.as_ref())
            {
                let src_a = if is_image_set(url_a) {
                    select_image_set_url(url_a, dpr).to_string()
                } else {
                    url_a.clone()
                };
                let src_b = if is_image_set(url_b) {
                    select_image_set_url(url_b, dpr).to_string()
                } else {
                    url_b.clone()
                };
                if !src_a.is_empty() && !src_b.is_empty() {
                    out.push(DisplayCommand::DrawCrossFade {
                        dest: clip,
                        src_a,
                        src_b,
                        progress: *t,
                    });
                }
            }
        }
        BackgroundImage::Paint { name, args } => {
            // CSS Paint API (Houdini): the worklet host draws the bitmap under `src`
            // (see `paint_worklet`); an image without intrinsic size gets the
            // resolved tile size explicitly, so the renderer scales nothing.
            let (w, h) = paint_tile_size(layer.size, origin);
            if w > 0.0 && h > 0.0 {
                out.push(DisplayCommand::DrawBackgroundImage {
                    rect: clip,
                    origin_rect: origin,
                    src: paint_image_key(b.node.index(), name, args, w, h),
                    size: BackgroundSize::Length(BgSizeAxis::Px(w), BgSizeAxis::Px(h)),
                    position: layer.position,
                    repeat: layer.repeat,
                    image_rendering: b.style.image_rendering,
                });
            }
        }
        _ => {}
    }
    if fixed {
        out.push(DisplayCommand::EndFixedBackground);
    }
    if use_blend {
        out.push(DisplayCommand::PopBlendMode);
    }
}

/// Glyph runs of `b` and its in-flow descendants for a `background-clip: text`
/// mask, as opaque-black `DrawText` commands (only their coverage matters to
/// the alpha mask).
///
/// CSS Backgrounds L4 §3.8: the mask is "the geometry of the text in the
/// element and its in-flow and floated descendants" — so own absolutely
/// positioned / fixed descendants are left out (they are out of flow), while a
/// transformed in-flow descendant stays in the mask **at its transformed
/// position**: its glyphs are wrapped in the descendant's own
/// `PushTransform`/`PopTransform`, the same matrix `box_layer` puts around the
/// box's normal paint. The root's own transform is not repeated here — the
/// whole background group is already emitted inside it.
fn collect_text_mask(b: &LayoutBox, dpr: f32, is_root: bool, out: &mut Vec<DisplayCommand>) {
    if matches!(b.kind, BoxKind::Skip) {
        return;
    }
    if !is_root && matches!(b.style.position, Position::Absolute | Position::Fixed) {
        return;
    }
    // Only boxes that own a stacking context carry a transform of their own:
    // anonymous `InlineRun` wrappers hold a clone of the parent's style (and
    // so its `transform`), but `box_layer` never re-applies it to them.
    let transform = if is_root || !box_can_own_stacking_context(b) {
        None
    } else {
        forward_box_transform(b)
    };
    if let Some(matrix) = transform {
        out.push(DisplayCommand::PushTransform { matrix });
    }
    if let BoxKind::InlineRun { lines, .. } = &b.kind {
        let mut tmp = Vec::new();
        super::inline_frag::TEXT_MASK_BUILD.with(|f| f.set(true));
        emit_inline_run(b, lines, dpr, &mut tmp);
        super::inline_frag::TEXT_MASK_BUILD.with(|f| f.set(false));
        for mut c in tmp {
            if let DisplayCommand::DrawText { color, .. } = &mut c {
                *color = Color { r: 0, g: 0, b: 0, a: 255 };
                out.push(c);
            }
        }
    }
    for child in &b.children {
        collect_text_mask(child, dpr, false, out);
    }
    if transform.is_some() {
        out.push(DisplayCommand::PopTransform);
    }
}

/// CSS Backgrounds L4 §3.8 — runs `paint` (which emits the background content
/// as for `border-box`) and clips the result to the element's glyphs.
///
/// Emitted as `PushOpacity(1)` · content · `PushMaskLayer(alpha)` · glyphs ·
/// `PopMaskLayer` · `PopOpacity` — the same order `emit_svg_shape_masked` uses:
/// the content goes to the level below the mask layer, `PopMaskLayer`
/// multiplies it by the glyph alpha. With no text under the box nothing is
/// painted at all (an empty mask hides the whole background).
fn emit_text_clipped(
    out: &mut Vec<DisplayCommand>,
    b: &LayoutBox,
    dpr: f32,
    paint: impl FnOnce(&mut Vec<DisplayCommand>),
) {
    let mut glyphs = Vec::new();
    collect_text_mask(b, dpr, true, &mut glyphs);
    // A transformed descendant leaves Push/PopTransform behind even when it
    // holds no text, so test for actual glyph runs, not for an empty list.
    if !glyphs.iter().any(|c| matches!(c, DisplayCommand::DrawText { .. })) {
        return;
    }
    let mut content = Vec::new();
    paint(&mut content);
    if content.is_empty() {
        return;
    }
    out.push(DisplayCommand::PushOpacity { alpha: 1.0, bounds: Some(b.rect) });
    out.extend(content);
    out.push(DisplayCommand::PushMaskLayer { rect: b.rect, mode: MaskMode::Alpha });
    out.extend(glyphs);
    out.push(DisplayCommand::PopMaskLayer);
    out.push(DisplayCommand::PopOpacity);
}

/// CSS Backgrounds L4 §3.8 — runs `paint` (which emits the background content
/// as for `border-box`) and clips the result to the border area: the area the
/// element's own `DrawBorder` covers, so rounded corners, per-side widths and
/// `border-style` gaps (dashed, dotted, double) shape the visible background.
///
/// Emitted like [`emit_text_clipped`]: `PushOpacity(1)` · content ·
/// `PushMaskLayer(alpha)` · opaque-black border · `PopMaskLayer` · `PopOpacity`.
/// With no visible border nothing is painted (the border area is empty).
fn emit_border_area_clipped(
    out: &mut Vec<DisplayCommand>,
    b: &LayoutBox,
    paint: impl FnOnce(&mut Vec<DisplayCommand>),
) {
    let s = &b.style;
    let styles = [
        s.border_top_style, s.border_right_style, s.border_bottom_style, s.border_left_style,
    ];
    if !styles.iter().any(|st| st.is_visible()) {
        return;
    }
    let mut content = Vec::new();
    paint(&mut content);
    if content.is_empty() {
        return;
    }
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    out.push(DisplayCommand::PushOpacity { alpha: 1.0, bounds: Some(b.rect) });
    out.extend(content);
    out.push(DisplayCommand::PushMaskLayer { rect: b.rect, mode: MaskMode::Alpha });
    out.push(DisplayCommand::DrawBorder {
        rect: b.rect,
        widths: [
            s.border_top_width, s.border_right_width, s.border_bottom_width, s.border_left_width,
        ],
        colors: [black; 4],
        styles,
        radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
    });
    out.push(DisplayCommand::PopMaskLayer);
    out.push(DisplayCommand::PopOpacity);
}

/// CSS Backgrounds L3 §3.10 — эмитит все фоновые слои элемента.
///
/// CSS Backgrounds L3 §3: слои рисуются снизу вверх — последний в списке (Vec)
/// рисуется первым (самый нижний), первый в списке — последним (самый верхний).
/// Пустых layers → no-op.
///
/// CSS Compositing L1 §8.3: background creates an isolated compositing group.
/// The bottom-most layer blends against transparent background-color; for common
/// blend modes (multiply, screen etc.) this is identity for premultiplied alpha,
/// so we suppress PushBlendMode for that layer.
///
/// BUG-277 slice 2: when a non-bottom layer actually blends, the layer stack is
/// wrapped in its own `PushOpacity{alpha:1.0}`/`PopOpacity` isolation group. Without
/// it, the wgpu renderer's level-based compositor has no readable "parent" texture
/// for a top-level (non-nested) box — `PopBlendMode`'s composite silently falls back
/// to plain alpha-over, dropping the blend effect entirely (renderer.rs `Composite`
/// requires `from_level > 1` to read a parent layer; a box with no ancestor
/// stacking context sits at `from_level == 1`, whose "parent" is the real
/// swapchain surface, which has no `TEXTURE_BINDING` usage and can't be sampled).
/// Forcing an isolate group gives the blend pair its own two-level offscreen stack
/// (bottom layer at the isolate's level, top layer nested one level above it)
/// regardless of ancestor nesting, matching cpu_raster/femtovg (whose immediate-mode
/// canvas already contains only this box's own painted content at this point) and
/// the CSS spec's "background forms an isolated group" semantics.
pub(crate) fn emit_background_image(out: &mut Vec<DisplayCommand>, b: &LayoutBox, dpr: f32) {
    let scroller = local_bg_scroller(b);
    let layers: Vec<&BackgroundLayer> = b
        .style
        .background_layers
        .iter()
        .filter(|l| !(scroller && is_local_layer(l)))
        .collect();
    // Isolation is needed only if some non-bottom layer actually blends (i == 0 is
    // always suppressed, see `emit_background_layer`'s `suppress_blend`).
    let needs_isolation = layers
        .iter()
        .rev()
        .enumerate()
        .any(|(i, layer)| i > 0 && layer.blend_mode != LayoutBlendMode::Normal);
    if needs_isolation {
        out.push(DisplayCommand::PushOpacity { alpha: 1.0, bounds: Some(b.rect) });
    }
    // CSS Backgrounds L3 §4.3 — same border-box radii the solid `background-color`
    // path uses (BUG-631: a gradient background must be clipped to the same
    // rounded box, not a square `PushClipRect`).
    let radii = CornerRadii::from_style_and_box(&b.style, b.rect.width, b.rect.height);
    // CSS Backgrounds L4 §3.8: `background-color` follows the last layer's clip;
    // with `text` the call sites skipped the box fill (empty `background_clip_rect`),
    // so the colour is painted here, glyph-masked, beneath every image layer.
    if background_color_clip(b) == BackgroundClip::Text
        && let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
        && bg.a > 0
    {
        emit_text_clipped(out, b, dpr, |inner| {
            inner.push(DisplayCommand::FillRect { rect: b.rect, color: bg });
        });
    }
    // `border-area` likewise: the call sites skipped the box fill, the colour is
    // painted here through the border-shaped mask.
    if background_color_clip(b) == BackgroundClip::BorderArea
        && let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
        && bg.a > 0
    {
        emit_border_area_clipped(out, b, |inner| {
            let radii = CornerRadii::from_style_and_box(&b.style, b.rect.width, b.rect.height);
            if radii.all_zero() {
                inner.push(DisplayCommand::FillRect { rect: b.rect, color: bg });
            } else {
                inner.push(DisplayCommand::FillRoundedRect { rect: b.rect, color: bg, radii });
            }
        });
    }
    // Рисуем в обратном порядке: последний слой = нижний (рисуется первым).
    for (i, layer) in layers.iter().rev().enumerate() {
        // i == 0 is the bottom-most layer; suppress its blend mode (identity effect).
        emit_background_layer(out, b, layer, dpr, i == 0, radii, LayerArea::Own);
    }
    if needs_isolation {
        out.push(DisplayCommand::PopOpacity);
    }
}

/// A layer that scrolls with its box's contents (CSS Backgrounds L3 §3.6). `background-clip: text`
/// stays with the box: its glyph mask is laid out against the unscrolled content; so does
/// `border-area`, whose border-shaped mask does not scroll.
fn is_local_layer(l: &BackgroundLayer) -> bool {
    l.attachment == BackgroundAttachment::Local
        && !matches!(l.clip, BackgroundClip::Text | BackgroundClip::BorderArea)
}

/// The box scrolls its own `background-attachment: local` layers: a block-level scroll container
/// (`overflow: hidden|scroll|auto` in some axis — `hidden` is a scroll container too) with a
/// square border box and a `local` layer or a `background-color` under a `local` bottom layer.
/// Anything else paints `local` like `scroll`: with no scrolling mechanism the spec says so.
/// A rounded box clips the moving layers with a stationary rounded padding box
/// ([`local_round_clip`]).
pub(crate) fn local_bg_scroller(b: &LayoutBox) -> bool {
    let s = &b.style;
    let scrolls = |o: Overflow| matches!(o, Overflow::Hidden | Overflow::Scroll | Overflow::Auto);
    matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot)
        && (scrolls(s.overflow_x) || scrolls(s.overflow_y))
        && s.background_layers.iter().any(is_local_layer)
}

/// Rounded padding-box clip of a `local`-background scroller with `border-radius`: it stays put
/// while the layers move, so the corners cut the scrolled background (§5.3 inner radii). The
/// caller pushes it before the scroll layer / clip and pops it after the matching pop.
pub(crate) fn local_round_clip(b: &LayoutBox) -> Option<DisplayCommand> {
    if !local_bg_scroller(b) {
        return None;
    }
    let s = &b.style;
    let outer = CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height);
    if outer.all_zero() {
        return None;
    }
    let r = outer.inner_for_border([
        s.border_top_width,
        s.border_right_width,
        s.border_bottom_width,
        s.border_left_width,
    ]);
    Some(DisplayCommand::PushClipRoundedRect {
        rect: Rect::new(
            b.rect.x + s.border_left_width,
            b.rect.y + s.border_top_width,
            (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
            (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0),
        ),
        radii: [r.tl, r.tr, r.br, r.bl],
    })
}

/// The solid `background-color` scrolls too when the bottom layer (the one the colour follows,
/// §3.10) is `local` — it then fills the scrollable area, not the box.
pub(crate) fn local_bg_color(b: &LayoutBox) -> bool {
    local_bg_scroller(b) && b.style.background_layers.last().is_some_and(is_local_layer)
}

/// Border box whose padding box is the scrollable area of `b` (the padding box grown to every
/// child's rect, also towards the start edges: `rtl` and `*-reverse` content overflows to the
/// left/top) — what a `local` layer is positioned and painted against. Borders and padding keep
/// their widths, so `background-origin`/`-clip` resolve exactly as for the box itself.
fn local_scroll_area(b: &LayoutBox) -> Rect {
    let s = &b.style;
    let px = b.rect.x + s.border_left_width;
    let py = b.rect.y + s.border_top_width;
    let pw = (b.rect.width - s.border_left_width - s.border_right_width).max(0.0);
    let ph = (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0);
    let (mut x0, mut y0, mut x1, mut y1) = (px, py, px + pw, py + ph);
    for c in &b.children {
        x0 = x0.min(c.rect.x);
        y0 = y0.min(c.rect.y);
        x1 = x1.max(c.rect.x + c.rect.width);
        y1 = y1.max(c.rect.y + c.rect.height);
    }
    Rect::new(
        b.rect.x - (px - x0),
        b.rect.y - (py - y0),
        b.rect.width + (x1 - x0) - pw,
        b.rect.height + (y1 - y0) - ph,
    )
}

/// CSS Backgrounds L3 §3.6 `background-attachment: local` — the layers (and the colour) of a
/// scroll container that move with its content. The caller emits them right after opening the
/// container's clip / `PushScrollLayer`, so the scroll offset translates them and the padding box
/// clips them: the border area stays unpainted.
pub(crate) fn emit_local_background(out: &mut Vec<DisplayCommand>, b: &LayoutBox, dpr: f32) {
    if !local_bg_scroller(b) || !is_paint_visible(b) || is_hidden_empty_cell(b) {
        return;
    }
    let area = local_scroll_area(b);
    if local_bg_color(b)
        && let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
        && bg.a > 0
    {
        let clip = background_clip_rect_in(b, area, background_color_clip(b));
        if clip.width > 0.0 && clip.height > 0.0 {
            out.push(DisplayCommand::FillRect { rect: clip, color: bg });
        }
    }
    // Bottom-most first, like `emit_background_image`; a blend mode is a no-op on the very
    // bottom layer, and a blending local layer gets its own isolated group.
    let all = &b.style.background_layers;
    let needs_isolation = all
        .iter()
        .rev()
        .enumerate()
        .any(|(i, l)| i > 0 && is_local_layer(l) && l.blend_mode != LayoutBlendMode::Normal);
    if needs_isolation {
        out.push(DisplayCommand::PushOpacity { alpha: 1.0, bounds: Some(area) });
    }
    let radii = CornerRadii::default();
    for (i, layer) in all.iter().rev().enumerate() {
        if is_local_layer(layer) {
            emit_background_layer(out, b, layer, dpr, i == 0, radii, LayerArea::Local(area));
        }
    }
    if needs_isolation {
        out.push(DisplayCommand::PopOpacity);
    }
}

/// CSS Masking L1 §4 — эмитит PushMask* перед элементом + его детьми.
/// Возвращает `true` если команда была эмитирована (нужен парный PopMask).
/// `rect` = border-box элемента (mask painting area).
/// CSS Masking L1 §6.4 `mask-mode: luminance` — rewrites each gradient stop so
/// its alpha channel encodes `luminance(rgb)·alpha`. The mask backends
/// (`composite_mask_layer` in femtovg, `render_mask` in cpu_raster) read only
/// the rendered gradient's **alpha** under a `DestinationIn` composite, so
/// baking luminance into the alpha here makes a dark mask pixel hide the element
/// even when it is fully opaque — without threading the mode into the backends.
/// For `mask-mode: alpha` (default) the stops are returned unchanged.
///
/// Luminance is exact across a linear gradient: `luma` is a linear combination
/// of R, G, B, so `luma(lerp(c0, c1, t)) == lerp(luma(c0), luma(c1), t)`.
pub(crate) fn mask_stops_for_mode(stops: &[GradientStop], mode: lumen_layout::MaskMode) -> Vec<GradientStop> {
    match mode {
        lumen_layout::MaskMode::Alpha => stops.to_vec(),
        lumen_layout::MaskMode::Luminance => stops
            .iter()
            .map(|s| {
                let c = s.color;
                let luma = 0.2126 * f32::from(c.r)
                    + 0.7152 * f32::from(c.g)
                    + 0.0722 * f32::from(c.b);
                let a = (luma / 255.0 * f32::from(c.a)).round().clamp(0.0, 255.0) as u8;
                GradientStop {
                    color: Color { a, ..c },
                    color_space: s.color_space,
                    position: s.position.clone(),
                }
            })
            .collect(),
    }
}

/// CSS Masking L1 §4.7/§4.9 — какие слои из [`ComputedStyle::mask_layers`]
/// попадают в display list как вложенные `PushMask*` (быстрый путь).
///
/// Один `PushMask*` несёт ровно один mask-канал, но группы **вкладываются**, а
/// вложение перемножает альфы: содержимое под `PushMask(A) PushMask(B) … PopMask
/// PopMask` получает `alpha · b · a`. Умножение — это ровно Porter-Duff
/// source-in, то есть `mask-composite: intersect`. Поэтому цепочку, где каждый
/// слой поверх нижнего складывается через `intersect`, можно отрендерить точно,
/// не собирая маску в отдельный офскрин и не трогая бэкенды. Порядок вложения
/// не важен — умножение коммутативно.
///
/// Условия, при которых эмитятся все слои:
/// * у каждого слоя есть рисуемый источник (`url(...)` или градиент) — слой
///   `none` даёт прозрачную маску и в `intersect` обнулил бы результат;
/// * у всех слоёв, кроме нижнего, `composite: intersect`;
/// * у нижнего слоя `composite` **не** `intersect` — его оператор применяется к
///   прозрачному фону, где `add`/`subtract`/`exclude` дают сам слой, а
///   `intersect` (source-in с прозрачным) вычистил бы маску целиком. Реализация
///   этого вырожденного случая расходится между браузерами, поэтому он уходит в
///   общий путь [`mask_needs_composite`].
///
/// Всё остальное — `add`/`subtract`/`exclude` между слоями и градиентные слои с
/// `mask-size` — собирается в одну маску отдельным офскрином
/// ([`composite_mask_close`]); тогда эта функция возвращает пустой список.
pub(crate) fn rendered_mask_layers(b: &LayoutBox) -> &[MaskLayer] {
    if mask_needs_composite(b) {
        return &[];
    }
    let layers = &b.style.mask_layers;
    if is_intersect_chain(layers) { layers } else { &layers[..layers.len().min(1)] }
}

/// Цепочка слоёв, которую точно выражает вложение `PushMask*` (см.
/// [`rendered_mask_layers`]).
fn is_intersect_chain(layers: &[MaskLayer]) -> bool {
    let Some((bottom, upper)) = layers.split_last() else {
        return false;
    };
    !upper.is_empty()
        && upper.iter().all(|l| l.composite == MaskComposite::Intersect)
        && bottom.composite != MaskComposite::Intersect
        && layers.iter().all(is_renderable_mask_source)
}

/// Нужна ли слоям маски элемента сборка в одну маску через офскрин
/// ([`composite_mask_close`]) вместо вложенных `PushMask*`.
///
/// `PushMask*` несёт только `rect` и источник: размера плитки `mask-size` у
/// градиента в команде нет (для картинки он есть, плитку считает бэкенд), а
/// `add`/`subtract`/`exclude` между слоями вложением не выражаются.
fn mask_needs_composite(b: &LayoutBox) -> bool {
    let layers = &b.style.mask_layers;
    layers.iter().any(|l| {
        matches!(l.image, BackgroundImage::Gradient(_)) && matches!(l.size, BackgroundSize::Length(..))
    }) || (layers.len() > 1 && !is_intersect_chain(layers))
        // `mask-clip: no-clip`: `PushMask*` несёт один `rect` — и позиционирование,
        // и область рисования, — поэтому слой, плитки которого выходят за
        // элемент, собирается через офскрин.
        || layers.iter().any(|l| l.clip == MaskClip::NoClip && is_renderable_mask_source(l))
}

/// Объединение рамок `b` и всех его потомков — область, которую `mask-clip:
/// no-clip` обязан накрыть маской. Обход итеративный (глубина дерева не
/// ограничена).
fn subtree_rect(b: &LayoutBox) -> Rect {
    let mut r = b.rect;
    let mut stack = vec![b];
    while let Some(n) = stack.pop() {
        let (x0, y0) = (r.x.min(n.rect.x), r.y.min(n.rect.y));
        let (x1, y1) = ((r.x + r.width).max(n.rect.x + n.rect.width), (r.y + r.height).max(n.rect.y + n.rect.height));
        r = Rect::new(x0, y0, x1 - x0, y1 - y0);
        stack.extend(n.children.iter());
    }
    r
}

/// Есть ли у слоя источник, который [`emit_push_mask`] умеет превратить в
/// `PushMask*`. `mask-image: none` и пустой `url()` — нет.
fn is_renderable_mask_source(l: &MaskLayer) -> bool {
    match &l.image {
        BackgroundImage::Url(src) => !src.is_empty(),
        BackgroundImage::Gradient(_) => true,
        _ => false,
    }
}

/// Эмитит открытие mask-групп элемента и возвращает команды их закрытия — их
/// вызывающий выставляет после содержимого элемента (и после своих внутренних
/// `PopClip`). Пустой список — маски нет.
pub(crate) fn emit_push_mask(out: &mut Vec<DisplayCommand>, b: &LayoutBox, dpr: f32) -> Vec<DisplayCommand> {
    if mask_needs_composite(b) {
        // Содержимое элемента — в изолированную opacity-группу: `PopMaskLayer`
        // умножает слой под собой на маску, а без изоляции этим слоем была бы
        // вся страница под элементом.
        out.push(DisplayCommand::PushOpacity { alpha: 1.0, bounds: Some(b.rect) });
        return composite_mask_close(b, dpr);
    }
    let mut opened = 0;
    // Верхний слой идёт наружу. Для `intersect` порядок безразличен, но так
    // display list читается в том же порядке, что и CSS-список слоёв.
    for layer in rendered_mask_layers(b) {
        if emit_push_mask_layer(out, b, layer) {
            opened += 1;
        }
    }
    vec![DisplayCommand::PopMask; opened]
}

/// Запас вокруг `b.rect` для прямоугольника `PushMaskLayer`: композит маски
/// действует только внутри него, а содержимое элемента (тень, переполнение)
/// выходит за border-box — его маска тоже обязана скрыть.
const MASK_LAYER_MARGIN: f32 = 16384.0;

/// Закрытие группы, открытой `PushOpacity` в [`emit_push_mask`] для составной
/// маски: `PushMaskLayer`, собранная из слоёв маска, `PopMaskLayer`,
/// `PopOpacity`.
fn composite_mask_close(b: &LayoutBox, dpr: f32) -> Vec<DisplayCommand> {
    let layers: Vec<&MaskLayer> = b.style.mask_layers.iter().rev().collect();
    let mut close = vec![DisplayCommand::PushMaskLayer {
        rect: mask_layer_rect(b),
        mode: MaskMode::Alpha,
    }];
    close.extend(composite_mask_stack(b, dpr, &layers));
    close.push(DisplayCommand::PopMaskLayer);
    close.push(DisplayCommand::PopOpacity);
    close
}

fn mask_layer_rect(b: &LayoutBox) -> Rect {
    Rect::new(
        b.rect.x - MASK_LAYER_MARGIN,
        b.rect.y - MASK_LAYER_MARGIN,
        b.rect.width + 2.0 * MASK_LAYER_MARGIN,
        b.rect.height + 2.0 * MASK_LAYER_MARGIN,
    )
}

/// Команды, рисующие в текущий слой альфу-маску `layers[..]` (снизу вверх;
/// последний — верхний слой), собранную операторами `mask-composite` (CSS
/// Masking L1 §4.7): оператор слоя применяется к маске слоёв под ним, оператор
/// самого нижнего слоя применяется к прозрачному фону.
///
/// * `add` — слой поверх накопленного (source-over);
/// * `intersect` — накопленное, умноженное на слой: `PushMaskLayer` с альфой
///   слоя поверх группы с накопленным;
/// * `subtract` — слой, умноженный на `1 − накопленное` (`InvertedAlpha`);
/// * `exclude` — `subtract` в обе стороны. Две половины складываются
///   source-over, что для непересекающихся (`a·(1−b)` и `b·(1−a)` не
///   пересекаются) точно, а для частичных альф отличается на произведение.
///
/// `exclude` дублирует накопленное, поэтому размер растёт экспоненциально по
/// числу подряд идущих `exclude`; за [`MAX_COMPOSITE_COMMANDS`] вторая половина
/// отбрасывается.
fn composite_mask_stack(b: &LayoutBox, dpr: f32, layers: &[&MaskLayer]) -> Vec<DisplayCommand> {
    const MAX_COMPOSITE_COMMANDS: usize = 8192;
    let Some((top, below)) = layers.split_last() else {
        return Vec::new();
    };
    let own = composite_mask_layer_content(b, dpr, top);
    if below.is_empty() {
        return own;
    }
    let lower = composite_mask_stack(b, dpr, below);
    let group = |content: &[DisplayCommand], mode: MaskMode, mask: &[DisplayCommand]| {
        let mut g = vec![DisplayCommand::PushOpacity { alpha: 1.0, bounds: Some(b.rect) }];
        g.extend_from_slice(content);
        g.push(DisplayCommand::PushMaskLayer { rect: mask_layer_rect(b), mode });
        g.extend_from_slice(mask);
        g.push(DisplayCommand::PopMaskLayer);
        g.push(DisplayCommand::PopOpacity);
        g
    };
    match top.composite {
        MaskComposite::Add => {
            let mut out = lower;
            out.extend(own);
            out
        }
        MaskComposite::Intersect => group(&lower, MaskMode::Alpha, &own),
        MaskComposite::Subtract => group(&own, MaskMode::InvertedAlpha, &lower),
        MaskComposite::Exclude => {
            let mut out = group(&own, MaskMode::InvertedAlpha, &lower);
            if own.len() + lower.len() <= MAX_COMPOSITE_COMMANDS {
                out.extend(group(&lower, MaskMode::InvertedAlpha, &own));
            }
            out
        }
    }
}

/// Команды, рисующие один слой маски (плитки `mask-size`/`-position`/`-repeat`
/// внутри `mask-origin`, обрезанные по `mask-clip`) в текущий слой. Слой
/// `none` и нерисуемые источники дают пустой список — прозрачную маску.
///
/// Градиентные стопы пересчитываются под `mask-mode` слоя ([`mask_stops_for_mode`]);
/// у картинки берётся её альфа, `mask-mode: luminance` для `url()` в составной
/// маске не учитывается.
fn composite_mask_layer_content(b: &LayoutBox, dpr: f32, layer: &MaskLayer) -> Vec<DisplayCommand> {
    let image = match &layer.image {
        BackgroundImage::Gradient(g) => BackgroundImage::Gradient(gradient_for_mask_mode(g, layer.mode)),
        BackgroundImage::Url(src) if !src.is_empty() => layer.image.clone(),
        _ => return Vec::new(),
    };
    // У градиента нет собственного размера и пропорций: `auto`/`cover`/`contain`
    // дают плитку во всю область позиционирования (CSS Images 3 §5.3). Без явной
    // плитки `emit_background_layer` залил бы одной командой всю область
    // рисования и проигнорировал `mask-repeat`/`mask-position`.
    let size = match (&image, layer.size) {
        (BackgroundImage::Gradient(_), BackgroundSize::Length(..)) | (BackgroundImage::Url(_), _) => layer.size,
        (_, _) => BackgroundSize::Length(lumen_layout::BgSizeAxis::Percent(1.0), lumen_layout::BgSizeAxis::Percent(1.0)),
    };
    let bg = BackgroundLayer {
        image,
        repeat: layer.repeat,
        size,
        position: layer.position,
        origin: layer.origin,
        ..BackgroundLayer::default()
    };
    let mut out = Vec::new();
    if layer.clip == MaskClip::NoClip {
        // `no-clip` (§4.6): плитки слоя продолжаются за границы элемента — маска
        // накрывает и то, что выступает из него (потомки, `overflow: visible`).
        let area = subtree_rect(b);
        emit_background_layer(&mut out, b, &bg, dpr, true, CornerRadii::default(), LayerArea::Unclipped(area));
        return out;
    }
    // `mask-clip` ограничивает рисуемую область слоя (§4.6); `border-box`
    // оставляет плитки в границах элемента.
    let clip_rect = mask_clip_layer_rect(b, layer.clip).unwrap_or(b.rect);
    out.push(DisplayCommand::PushClipRect { rect: clip_rect });
    emit_background_layer(&mut out, b, &bg, dpr, true, CornerRadii::default(), LayerArea::Own);
    out.push(DisplayCommand::PopClip);
    out
}

/// Градиент с альфой стопов, пересчитанной под `mask-mode` ([`mask_stops_for_mode`]).
fn gradient_for_mask_mode(g: &ParsedGradient, mode: lumen_layout::MaskMode) -> ParsedGradient {
    let mut g = g.clone();
    match &mut g {
        ParsedGradient::Linear { stops, .. }
        | ParsedGradient::Radial { stops, .. }
        | ParsedGradient::Conic { stops, .. } => *stops = mask_stops_for_mode(stops, mode),
        ParsedGradient::Unknown(_) => {}
    }
    g
}

/// Эмитит `PushMask*` одного слоя. `false` — источник не рисуемый, группа не
/// открыта (парный `PopMask` не нужен).
fn emit_push_mask_layer(out: &mut Vec<DisplayCommand>, b: &LayoutBox, layer: &MaskLayer) -> bool {
    // CSS Masking L1 §4.5 — `mask-origin` sets the mask **positioning area**
    // (border/padding/content box). Reuses the background-origin geometry; for
    // the default `border-box` this equals `b.rect`, so existing behaviour is
    // unchanged.
    let rect = background_origin_rect(b, layer.origin);
    let mode = layer.mode;
    // CSS Masking L1 §4.6 — `mask-clip` restricts the masked element's painting
    // area. It is wired at the call sites by wrapping the whole mask group in a
    // `PushClipRect` / `PopClip` pair (see `mask_clip_paint_rect`), reusing the
    // existing scissor path instead of threading a clip rect through the mask
    // commands + every backend.
    match &layer.image {
        BackgroundImage::Url(src) if !src.is_empty() => {
            out.push(DisplayCommand::PushMaskImage {
                rect,
                src: src.clone(),
                size: layer.size,
                // CSS Masking L1 §4.4 — `mask-position` (same syntax as
                // background-position). Applies to image masks; gradient masks
                // derive their geometry from `rect` above.
                position: layer.position,
                repeat: layer.repeat,
                image_rendering: b.style.image_rendering,
            });
            true
        }
        BackgroundImage::Gradient(ParsedGradient::Linear { angle_deg, corner, stops, repeating }) => {
            let resolved_angle = corner.map_or(*angle_deg, |c| c.angle_deg(rect.width, rect.height));
            out.push(DisplayCommand::PushMaskLinearGradient {
                rect,
                angle_deg: resolved_angle,
                stops: mask_stops_for_mode(stops, mode),
                repeating: *repeating,
            });
            true
        }
        BackgroundImage::Gradient(ParsedGradient::Radial {
            center_x_pct, center_y_pct, stops, repeating, ..
        }) => {
            out.push(DisplayCommand::PushMaskRadialGradient {
                rect,
                center_x_pct: *center_x_pct,
                center_y_pct: *center_y_pct,
                stops: mask_stops_for_mode(stops, mode),
                repeating: *repeating,
            });
            true
        }
        BackgroundImage::Gradient(ParsedGradient::Conic {
            center_x_pct, center_y_pct, from_angle_deg, stops, repeating
        }) => {
            out.push(DisplayCommand::PushMaskConicGradient {
                rect,
                center_x_pct: *center_x_pct,
                center_y_pct: *center_y_pct,
                from_angle_deg: *from_angle_deg,
                stops: mask_stops_for_mode(stops, mode),
                repeating: *repeating,
            });
            true
        }
        _ => false,
    }
}
