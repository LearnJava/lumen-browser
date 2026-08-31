//! P1/SPLIT-DL12: background/gradient/mask эмиссия — tile-геометрия
//! градиентных фонов, `emit_background_layer`/`emit_background_image`,
//! CSS Masking (`emit_push_mask`/`emit_push_mask_layer`). Вынесено из
//! `display_list.rs` (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL,
//! батч DL-12).

use super::*;

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
    let tile_x0 = origin.x + off_x;
    let tile_y0 = origin.y + off_y;

    let (tile_x_start, step_x, repeat_x, tile_y_start, step_y, repeat_y) = match repeat {
        BackgroundRepeat::NoRepeat => (tile_x0, tile_w, false, tile_y0, tile_h, false),
        BackgroundRepeat::RepeatX => (
            tile_x0 - (off_x / tile_w).ceil() * tile_w,
            tile_w,
            true,
            tile_y0,
            tile_h,
            false,
        ),
        BackgroundRepeat::RepeatY => (
            tile_x0,
            tile_w,
            false,
            tile_y0 - (off_y / tile_h).ceil() * tile_h,
            tile_h,
            true,
        ),
        BackgroundRepeat::Repeat | BackgroundRepeat::Round => (
            tile_x0 - (off_x / tile_w).ceil() * tile_w,
            tile_w,
            true,
            tile_y0 - (off_y / tile_h).ceil() * tile_h,
            tile_h,
            true,
        ),
        BackgroundRepeat::Space => {
            let (sx, step_x, rx) = space_axis_geometry(origin.x, origin.width, tile_w, off_x);
            let (sy, step_y, ry) = space_axis_geometry(origin.y, origin.height, tile_h, off_y);
            (sx, step_x, rx, sy, step_y, ry)
        }
    };

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
fn gradient_paint_rects(layer: &BackgroundLayer, origin: Rect, clip: Rect) -> (Vec<Rect>, bool) {
    match layer.size {
        BackgroundSize::Length(w, h) => {
            // Gradients have no intrinsic size/ratio: an `auto` axis falls back
            // to the positioning-area extent; percent resolves against it.
            let tile_w = w.resolve(origin.width).unwrap_or(origin.width).max(1.0);
            let tile_h = h.resolve(origin.height).unwrap_or(origin.height).max(1.0);
            let tiles =
                gradient_tile_rects(tile_w, tile_h, layer.position, layer.repeat, origin, clip);
            (tiles, true)
        }
        _ => (vec![clip], false),
    }
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
) {
    let clip = background_clip_rect(b, layer.clip);
    if clip.width <= 0.0 || clip.height <= 0.0 {
        return;
    }
    // CSS Backgrounds L3 §3.5: positioning area (background-origin) is independent of
    // the painting/clip area (background-clip). size/position calculations use origin_rect.
    let origin = background_origin_rect(b, layer.origin);
    let use_blend = !suppress_blend && layer.blend_mode != LayoutBlendMode::Normal;
    if use_blend {
        out.push(DisplayCommand::PushBlendMode { mode: map_blend_mode(layer.blend_mode), bounds: clip });
    }
    match &layer.image {
        BackgroundImage::Url(src) if !src.is_empty() => {
            // CSS: image-set — resolve image-set() to the best URL for the
            // current device pixel ratio; plain urls pass through unchanged.
            // P4 wires parsing: keep the raw `image-set(…)` string in
            // BackgroundImage::Url so this resolution triggers (CSS Images L4 §5).
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
            let (rects, needs_clip) = gradient_paint_rects(layer, origin, clip);
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
            let (rects, needs_clip) = gradient_paint_rects(layer, origin, clip);
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
            let (rects, needs_clip) = gradient_paint_rects(layer, origin, clip);
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
            // Gradient sides are not composited via DrawCrossFade (Phase 0 scope).
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
        BackgroundImage::Paint(name) => {
            // CSS Paint API (Houdini) — paint(name) generates dynamic image via registered worklet.
            // Phase 0: render as grey placeholder `DrawImage`; Phase 1: invoke worklet paint() callback.
            // `// CSS: background: paint(name)`
            out.push(DisplayCommand::DrawBackgroundImage {
                rect: clip,
                origin_rect: origin,
                src: format!("paint:{}", name),  // Prefixed to distinguish from URL images.
                size: layer.size,
                position: layer.position,
                repeat: layer.repeat,
                image_rendering: b.style.image_rendering,
            });
        }
        _ => {}
    }
    if use_blend {
        out.push(DisplayCommand::PopBlendMode);
    }
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
    // Isolation is needed only if some non-bottom layer actually blends (i == 0 is
    // always suppressed, see `emit_background_layer`'s `suppress_blend`).
    let needs_isolation = b
        .style
        .background_layers
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
    // Рисуем в обратном порядке: последний слой = нижний (рисуется первым).
    for (i, layer) in b.style.background_layers.iter().rev().enumerate() {
        // i == 0 is the bottom-most layer; suppress its blend mode (identity effect).
        emit_background_layer(out, b, layer, dpr, i == 0, radii);
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
/// реально попадают в display list.
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
///   тот же fallback, а не рендерится по букве спеки.
///
/// Иначе — `// CSS: mask-composite` — рендерится только верхний слой (прежнее
/// поведение). `add`/`subtract`/`exclude` между слоями вложением не выражаются:
/// им нужна сборка маски в отдельный офскрин во всех трёх бэкендах (femtovg,
/// wgpu `renderer.rs`, `cpu_raster.rs`), что уже renderer-side задача, а не
/// стилевая.
pub(crate) fn rendered_mask_layers(b: &LayoutBox) -> &[MaskLayer] {
    let layers = &b.style.mask_layers;
    let Some((bottom, upper)) = layers.split_last() else {
        return &[];
    };
    let all_intersect = !upper.is_empty()
        && upper.iter().all(|l| l.composite == MaskComposite::Intersect)
        && bottom.composite != MaskComposite::Intersect
        && layers.iter().all(is_renderable_mask_source);
    if all_intersect { layers } else { &layers[..1] }
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

/// Эмитит mask-группы элемента. Возвращает число открытых групп — столько же
/// `PopMask` обязан выставить вызывающий.
pub(crate) fn emit_push_mask(out: &mut Vec<DisplayCommand>, b: &LayoutBox) -> usize {
    let mut opened = 0;
    // Верхний слой идёт наружу. Для `intersect` порядок безразличен, но так
    // display list читается в том же порядке, что и CSS-список слоёв.
    for layer in rendered_mask_layers(b) {
        if emit_push_mask_layer(out, b, layer) {
            opened += 1;
        }
    }
    opened
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
