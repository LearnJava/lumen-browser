//! P1/SPLIT-DL17: provenance/geometry-хелперы — `struct ProvenanceIndex`
//! … до `fn contains_backdrop_filter`. Вынесено из `display_list.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-17).

use super::*;

/// Provenance for a display list (ADR-025 §3): a side index, not a field on
/// `DisplayCommand`. Answers "which layout box produced this command" without
/// touching the ~40-variant enum rebuilt every frame.
#[derive(Debug, Clone, Default)]
pub struct ProvenanceIndex {
    pub(crate) spans: Vec<ProvenanceSpan>,
}

impl ProvenanceIndex {
    /// All spans, in emission order. Not sorted by range — a box's spans can
    /// be interleaved with unrelated boxes' spans (see `ProvenanceSpan` docs).
    pub fn spans(&self) -> &[ProvenanceSpan] {
        &self.spans
    }

    /// Spans produced by exactly this origin — the primitive `explain_element`
    /// (DEVX-10) answers "which commands did this node produce" with.
    pub fn spans_for(&self, origin: BoxOrigin) -> impl Iterator<Item = &ProvenanceSpan> {
        self.spans.iter().filter(move |s| s.origin == origin)
    }
}

/// One contiguous run of commands produced by a single layout box's own
/// paint (ADR-025 §3). A box with descendants owns *more than one* span in
/// general: its own background/border is emitted before its children and its
/// closing layer-ops after them, with the children's own spans sitting in
/// between — `range` is contiguous, but "all of this box's spans" is not.
/// This is the resolution of the `p1-introspection-track.md` DEVX-7 finding
/// that `Range<usize>` cannot describe a *box*, only one of its spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceSpan {
    /// Half-open range into the final command list.
    pub range: Range<usize>,
    pub origin: BoxOrigin,
    /// Fragment index within the box — line box / column / page break.
    /// MVP: always `0` (the engine does not fragment single spans yet beyond
    /// the stacking-context bucket phases already captured by having several
    /// `ProvenanceSpan`s per box).
    pub fragment: u32,
    /// Number of open rect/rounded-rect/path clips at this span's first
    /// command — pairs with `PushClipRect`/`PushClipRoundedRect`/
    /// `PushClipPath` vs. `PopClip`. Scroll layers (`PushScrollLayer`/
    /// `PopScrollLayer`) are a separate stack and not counted here.
    pub clip_depth: u16,
}

pub(crate) fn object_fit_name(f: ObjectFit) -> &'static str {
    match f {
        ObjectFit::Fill => "fill",
        ObjectFit::Contain => "contain",
        ObjectFit::Cover => "cover",
        ObjectFit::None => "none",
        ObjectFit::ScaleDown => "scale-down",
    }
}

pub(crate) fn position_component_name(p: PositionComponent) -> String {
    match p {
        PositionComponent::Px(px) => format!("{px:.2}px"),
        PositionComponent::Percent(pc) => format!("{:.2}%", pc * 100.0),
    }
}

/// CSS Images L3 §5.5 — `object-fit` placement: где располагается
/// «полное» изображение внутри коробки (intrinsic-картинка после scale,
/// без обрезки). Возвращённый прямоугольник может быть больше `box_rect`
/// (cover / none на крупной картинке) — обрезку по box делает
/// [`fit_image_quad`] на стадии вычисления GPU-quad-а.
///
/// `intrinsic_size = (w, h)` — натуральный пиксельный размер декодированного
/// изображения; нулевые / отрицательные стороны коробки → возврат самой
/// коробки без масштабирования (deg fallback, рисовать всё равно нечего).
#[must_use]
pub fn fit_image_rect(
    box_rect: Rect,
    intrinsic_size: (u32, u32),
    fit: ObjectFit,
    position: ObjectPosition,
) -> Rect {
    let (iw, ih) = intrinsic_size;
    if iw == 0 || ih == 0 || box_rect.width <= 0.0 || box_rect.height <= 0.0 {
        return box_rect;
    }
    let iw = iw as f32;
    let ih = ih as f32;
    let bw = box_rect.width;
    let bh = box_rect.height;

    let (cw, ch) = match fit {
        ObjectFit::Fill => (bw, bh),
        ObjectFit::None => (iw, ih),
        ObjectFit::Contain => fit_with_ratio(iw, ih, bw, bh, /*cover*/ false),
        ObjectFit::Cover => fit_with_ratio(iw, ih, bw, bh, /*cover*/ true),
        ObjectFit::ScaleDown => {
            // `min(none, contain)` — выбираем результат с меньшей площадью.
            let (nw, nh) = (iw, ih);
            let (kw, kh) = fit_with_ratio(iw, ih, bw, bh, false);
            if nw * nh <= kw * kh { (nw, nh) } else { (kw, kh) }
        }
    };

    let free_x = bw - cw;
    let free_y = bh - ch;
    let off_x = position.x.resolve(free_x);
    let off_y = position.y.resolve(free_y);
    Rect::new(box_rect.x + off_x, box_rect.y + off_y, cw, ch)
}

fn fit_with_ratio(iw: f32, ih: f32, bw: f32, bh: f32, cover: bool) -> (f32, f32) {
    // contain = min(scale_w, scale_h); cover = max(...).
    let sx = bw / iw;
    let sy = bh / ih;
    let s = if cover { sx.max(sy) } else { sx.min(sy) };
    (iw * s, ih * s)
}

/// One classified run of a `text-orientation: mixed` string (CSS Writing
/// Modes L4 §4): a CJK ideograph paints upright (no rotation, stacked below
/// the previous glyph); a run of consecutive non-CJK characters (Latin,
/// digits, punctuation, whitespace) paints as one rotated block so kerning
/// and ligatures inside a Latin word stay intact. Produced by
/// [`split_mixed_runs`]; consumed by the CPU rasterizer
/// (`cpu_raster::rasterize_text_mixed`), the wgpu renderer
/// (`renderer::push_text_glyphs_mixed`) and the femtovg backend
/// (`femtovg_backend::FemtovgBackend::draw_text_mixed`) — every backend
/// rotates glyphs, so the CJK/Latin split rule lives here once.
#[cfg(any(feature = "backend-wgpu", feature = "cpu-render", feature = "backend-femtovg"))]
pub(crate) enum MixedSegment {
    /// A single CJK ideograph, rendered upright.
    Cjk(char),
    /// A run of consecutive non-CJK characters, rendered as one rotated block.
    Other(String),
}

/// Splits `text` into [`MixedSegment`]s for `text-orientation: mixed` paint —
/// see that type's docs for the CJK/Latin split rule.
#[cfg(any(feature = "backend-wgpu", feature = "cpu-render", feature = "backend-femtovg"))]
pub(crate) fn split_mixed_runs(text: &str) -> Vec<MixedSegment> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in text.chars() {
        if lumen_layout::vertical::is_cjk(ch) {
            if !buf.is_empty() {
                out.push(MixedSegment::Other(std::mem::take(&mut buf)));
            }
            out.push(MixedSegment::Cjk(ch));
        } else {
            buf.push(ch);
        }
    }
    if !buf.is_empty() {
        out.push(MixedSegment::Other(buf));
    }
    out
}

/// Per-axis tiling geometry for `background-repeat: space` /
/// `mask-repeat: space` (CSS Backgrounds L3 §3.4, CSS Masking L1 §4.4).
///
/// Given the positioning-area leading edge `area_origin`, its extent `area`
/// along the axis, the tile size `tile`, and the `position` offset `pos_off`
/// (from the leading edge), returns `(start, step, repeat)`:
/// * `start` — absolute coordinate of the first tile's leading edge;
/// * `step` — distance between successive tile origins (tile size + gap);
/// * `repeat` — whether more than one tile is laid out along the axis.
///
/// When two or more whole tiles fit, the first and last are pinned to the two
/// edges and the leftover space is distributed evenly as equal gaps (the
/// `position` offset is ignored on that axis, per spec). When at most one whole
/// tile fits, a single tile is placed at the `position` offset and the axis does
/// not repeat (identical to `no-repeat`).
///
/// Shared by every tiling path (femtovg + CPU via [`bg_tile_geometry`], and the
/// GPU renderer's inline background/mask loops) so `space` places tiles
/// identically everywhere.
#[must_use]
pub(crate) fn space_axis_geometry(
    area_origin: f32,
    area: f32,
    tile: f32,
    pos_off: f32,
) -> (f32, f32, bool) {
    if tile > 0.0 {
        let n = (area / tile).floor();
        if n >= 2.0 {
            let gap = (area - n * tile) / (n - 1.0);
            return (area_origin, tile + gap, true);
        }
    }
    (area_origin + pos_off, tile, false)
}

/// Tile geometry for a background image from `background-size` /
/// `background-position` / `background-repeat` (CSS Backgrounds L3 §3.3–3.5).
///
/// Pure (GL-free) so both the femtovg backend and the deterministic CPU
/// rasterizer derive identical placement. `img_w`/`img_h` — intrinsic image
/// size; `oarea_*` — the `background-origin` positioning area (x/y/width/height).
///
/// Returns `(tile_w, tile_h, tile_x_start, tile_y_start, repeat_x, repeat_y,
/// step_x, step_y)`: one tile's size, the top-left corner of the first tile, the
/// per-axis repeat flags, and the per-axis step between successive tile origins.
/// The caller tiles from `(tile_x_start, tile_y_start)` across the painting area,
/// stepping by `(step_x, step_y)` while the corresponding repeat flag is set,
/// clipping to the painting rect. `step_*` equals `tile_*` for every repeat mode
/// except `space`, where it includes the inter-tile gap (CSS Backgrounds L3 §3.4).
// BUG-235: only the femtovg window and the tiny-skia CPU snapshot tile
// backgrounds via this helper; the wgpu renderer tiles on the GPU. Gate it to
// its consumers so a wgpu-only build (e.g. lumen-driver default features) does
// not flag it as dead code under `-D warnings`.
#[cfg(any(feature = "backend-femtovg", feature = "cpu-render"))]
#[allow(clippy::too_many_arguments)]
#[must_use]
pub(crate) fn bg_tile_geometry(
    size: BackgroundSize,
    position: &ObjectPosition,
    repeat: BackgroundRepeat,
    img_w: f32,
    img_h: f32,
    oarea_w: f32,
    oarea_h: f32,
    oarea_x: f32,
    oarea_y: f32,
) -> (f32, f32, f32, f32, bool, bool, f32, f32) {
    let (tile_w, tile_h) = match size {
        BackgroundSize::Auto => (img_w, img_h),
        BackgroundSize::Cover => {
            let s = (oarea_w / img_w).max(oarea_h / img_h);
            (img_w * s, img_h * s)
        }
        BackgroundSize::Contain => {
            let s = (oarea_w / img_w).min(oarea_h / img_h);
            (img_w * s, img_h * s)
        }
        BackgroundSize::Length(w, h) => {
            // CSS Backgrounds L3 §3.5: percent axes resolve against the
            // positioning area; an `auto` axis derives from the other via the
            // image's intrinsic aspect ratio.
            match (w.resolve(oarea_w), h.resolve(oarea_h)) {
                (Some(tw), Some(th)) => (tw.max(1.0), th.max(1.0)),
                (Some(tw), None) => {
                    let tw = tw.max(1.0);
                    (tw, (img_h * (tw / img_w)).max(1.0))
                }
                (None, Some(th)) => {
                    let th = th.max(1.0);
                    ((img_w * (th / img_h)).max(1.0), th)
                }
                (None, None) => (img_w, img_h),
            }
        }
    };

    // CSS Backgrounds L3 §3.4 — `round`: rescale the tile so a whole number of
    // copies exactly fills the positioning area along each axis (no clipped
    // partial tiles at the far edge). `n = max(1, round(area / tile))`, then the
    // tile is stretched to `area / n`. Both axes are rounded independently, which
    // may distort the aspect ratio — matching the reference rendering (the spec's
    // "note" explicitly permits distortion when only one axis, or a size-auto
    // axis, is involved). Applied before offset resolution so percentage
    // positions resolve against the rounded tile size.
    let (tile_w, tile_h) = if repeat == BackgroundRepeat::Round {
        let round_axis = |area: f32, tile: f32| -> f32 {
            if tile > 0.0 && area > 0.0 {
                let n = (area / tile).round().max(1.0);
                area / n
            } else {
                tile
            }
        };
        (round_axis(oarea_w, tile_w), round_axis(oarea_h, tile_h))
    } else {
        (tile_w, tile_h)
    };

    let off_x = match position.x {
        PositionComponent::Px(px) => px,
        PositionComponent::Percent(p) => (oarea_w - tile_w) * p,
    };
    let off_y = match position.y {
        PositionComponent::Px(py) => py,
        PositionComponent::Percent(p) => (oarea_h - tile_h) * p,
    };
    let tile_x0 = oarea_x + off_x;
    let tile_y0 = oarea_y + off_y;

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
            let (sx, step_x, rx) = space_axis_geometry(oarea_x, oarea_w, tile_w, off_x);
            let (sy, step_y, ry) = space_axis_geometry(oarea_y, oarea_h, tile_h, off_y);
            (sx, step_x, rx, sy, step_y, ry)
        }
    };

    (tile_w, tile_h, tile_x_start, tile_y_start, repeat_x, repeat_y, step_x, step_y)
}

/// Финальный GPU-quad для `<img>`: пересечение «полного» placement-rect
/// (см. [`fit_image_rect`]) с `box_rect` плюс соответствующие UV-bounds
/// исходной текстуры. Спецификация CSS Images L3 §5.5 требует «clipped to
/// the content box» — для cover / none, когда картинка выходит за коробку,
/// мы делаем clip через UV (рисуем меньший quad с поджатыми UV), без
/// scissor-state в GPU pipeline.
///
/// Возвращает `None`, если intrinsic-размер нулевой, коробка пуста или
/// пересечение placement и box пусто (placement полностью снаружи box —
/// в норме не случается, но возможны deg-edge с отрицательным
/// `object-position`).
#[must_use]
pub fn fit_image_quad(
    box_rect: Rect,
    intrinsic_size: (u32, u32),
    fit: ObjectFit,
    position: ObjectPosition,
) -> Option<(Rect, [f32; 2], [f32; 2])> {
    let (iw, ih) = intrinsic_size;
    if iw == 0 || ih == 0 || box_rect.width <= 0.0 || box_rect.height <= 0.0 {
        return None;
    }
    let placed = fit_image_rect(box_rect, intrinsic_size, fit, position);
    if placed.width <= 0.0 || placed.height <= 0.0 {
        return None;
    }
    let bx0 = box_rect.x;
    let by0 = box_rect.y;
    let bx1 = box_rect.x + box_rect.width;
    let by1 = box_rect.y + box_rect.height;
    let px0 = placed.x;
    let py0 = placed.y;
    let px1 = placed.x + placed.width;
    let py1 = placed.y + placed.height;
    let vx0 = px0.max(bx0);
    let vy0 = py0.max(by0);
    let vx1 = px1.min(bx1);
    let vy1 = py1.min(by1);
    if vx1 <= vx0 || vy1 <= vy0 {
        return None;
    }
    let visible = Rect::new(vx0, vy0, vx1 - vx0, vy1 - vy0);
    let u0 = (vx0 - px0) / placed.width;
    let v0 = (vy0 - py0) / placed.height;
    let u1 = (vx1 - px0) / placed.width;
    let v1 = (vy1 - py0) / placed.height;
    Some((visible, [u0, v0], [u1, v1]))
}

/// Сериализует display list в детерминированный текст для snapshot-тестов.
///
/// Формат (одна команда — одна строка):
/// - `FillRect (x.xx, y.xx, w.xx, h.xx) #rrggbbaa`
/// - `DrawBorder (x.xx, y.xx, w.xx, h.xx) w=[t,r,b,l] c=[#top,#right,#bottom,#left]`
///   плюс `s=[t,r,b,l]` если хоть один стиль ≠ Solid (bw-compat: чистый
///   Solid-border печатается как раньше, snapshot-ы не ломаются).
/// - `DrawText (x.xx, y.xx, w.xx, h.xx) "text" fs.xx #rrggbbaa`
///
/// Сокращённый префикс `BorderStyle` для snapshot-сериализатора.
/// None уже фильтруется emit-side, но обрабатываем для устойчивости.
pub(crate) fn border_style_short(s: BorderStyle) -> &'static str {
    match s {
        BorderStyle::None => "n",
        BorderStyle::Solid => "s",
        BorderStyle::Dashed => "da",
        BorderStyle::Dotted => "do",
        BorderStyle::Double => "db",
    }
}

/// Returns `true` if the display list contains any `backdrop-filter` element.
///
/// Cull a display list to only commands that intersect the given tile region.
///
/// `tile_x` and `tile_y` are tile-space coordinates; the tile covers CSS pixels
/// `[tile_x*tile_size, (tile_x+1)*tile_size) × [tile_y*tile_size, (tile_y+1)*tile_size)`.
///
/// Commands that carry a bounding rect are included only when their rect
/// overlaps the tile (AABB test). State commands (`PushClipRect`, `PopClipRect`,
/// `PushScrollLayer`, `PopScrollLayer`, `PushOpacity`, `PopOpacity`,
/// `PushTransform`, `PopTransform`, `PushBlendMode`, `PopBlendMode`, etc.)
/// always pass through unchanged so that the GPU state machine remains correct.
///
/// Returns owned clones of the matching commands, ready to pass to the renderer.
#[must_use]
pub fn cull_display_list(
    dl: &[DisplayCommand],
    tile_x: i32,
    tile_y: i32,
    tile_size: f32,
) -> Vec<DisplayCommand> {
    let tx = tile_x as f32 * tile_size;
    let ty = tile_y as f32 * tile_size;

    let mut out = Vec::new();
    for cmd in dl {
        match get_command_rect(cmd) {
            Some(r) => {
                // AABB intersection: both axes must overlap.
                let overlaps_x = r.x < tx + tile_size && r.x + r.width > tx;
                let overlaps_y = r.y < ty + tile_size && r.y + r.height > ty;
                if overlaps_x && overlaps_y {
                    out.push(cmd.clone());
                }
            }
            // State / stack commands always pass through.
            None => out.push(cmd.clone()),
        }
    }
    out
}

/// Cheap pre-check the renderer uses to decide whether computing a frame
/// content hash for [`hash_display_list`] is worthwhile — pages without a
/// backdrop-filter pay zero hashing cost.
#[must_use]
pub fn contains_backdrop_filter(content: &[DisplayCommand], overlay: &[DisplayCommand]) -> bool {
    content
        .iter()
        .chain(overlay.iter())
        .any(|c| matches!(c, DisplayCommand::PushBackdropFilter { .. }))
}

