//! Display list — линейный список графических команд, выработанных из
//! дерева layout. Растеризатору (renderer) уже не нужно понимать DOM/CSS:
//! он рендерит то, что ему говорят.
//!
//! Координаты — экранные пиксели от верхнего левого угла окна.
//!
//! **ADR-008 Invariant 3 note (paint-pure-audit 10D.2, 2026-05-27):**
//! All display list builder functions (`build_display_list`, `build_display_list_with_anim`,
//! `build_display_list_ordered`, `build_display_list_ordered_with_anim`) are pure functions:
//! they depend only on their function parameters (LayoutBox, optional compositor anim frame,
//! optional stacking tree) and do not depend on hidden global state, thread-locals, or
//! environment variables. No `static mut` / `lazy_static!` / `OnceCell` found in this module.
//! Renderer caching (glyph atlas, image cache, layer snapshots) lives in separate crates
//! (lumen-font, lumen-image) with explicit eviction APIs.

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use std::collections::HashMap;
use std::ops::Range;

use lumen_core::geom::{Rect, Size};
use lumen_dom::InputType;
use lumen_layout::{
    box_can_own_stacking_context, creates_stacking_context, forward_box_transform,
    transform_fns_to_matrix, BoxOrigin, BoxRole, PseudoKind, CompositorAnimFrame, CompositorOverride,
    Appearance, BackfaceVisibility,
    BackgroundClip, BackgroundImage, BackgroundLayer, BackgroundOrigin, BackgroundRepeat, BackgroundSize, BorderCollapse, BorderStyle, BoxKind, MaskClip, MaskComposite, MaskLayer,
    ClipPath, Color, ComputedStyle, ContainFlags, CssColor, Display, EmptyCells, FilterFn, FontOpticalSizing, FontStretch, FontStyle, FontWeight, ShapeValue,
    FillRule, FormControlKind, StrokeLinecap, StrokeLinejoin, SvgShapeKind, SvgTextAnchor, SvgDominantBaseline, SvgBaselineShift,
    SvgGradientDef, SvgGradientUnits, SvgPaint,
    GradientStop, ImageRendering, Isolation, Length, ListStyleType, ParsedGradient,
    InlineFrag, LayoutBox, MarginBox, Mat4, MixBlendMode as LayoutBlendMode, ObjectFit, ObjectPosition,
    OutlineColor, OutlineStyle, Overflow, Page, PaintOrder, PaintPhase, Position, PositionComponent, Resize,
    ScrollbarWidth, SelectionHighlight,
    StackingContextId, StackingTree, TextDecorationSkipInk, TextDecorationStyle, TextDecorationThickness,
    TextEmphasisShape, TextEmphasisStyle, TextOverflow, TextUnderlinePosition,
    TransformStyle,
    Visibility, style::TextOrientation,
    font_palette::{palette_selection, FontPaletteSelection},
};

use crate::gap_decorations::{emit_gap_rules, GapDecorationContext, GapSegment};

/// CSS Images L3 §4.3 — image-rendering filter mode (scaling algorithm).
/// Determines how textures are sampled when an image is scaled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterMode {
    /// `auto` (default), `smooth`, `high-quality` — high-quality scaling (bilinear).
    #[default]
    Linear,
    /// `crisp-edges`, `pixelated` — preserve sharp edges (nearest-neighbour).
    Nearest,
}

impl FilterMode {
    /// Преобразует `ImageRendering` в `FilterMode`.
    /// `auto`/`smooth`/`high-quality` → `Linear` (bilinear).
    /// `crisp-edges`/`pixelated` → `Nearest` (pixel-perfect).
    #[must_use]
    pub fn from_image_rendering(ir: ImageRendering) -> Self {
        match ir {
            ImageRendering::Auto | ImageRendering::Smooth | ImageRendering::HighQuality => Self::Linear,
            ImageRendering::CrispEdges | ImageRendering::Pixelated => Self::Nearest,
        }
    }
}

/// CSS Compositing & Blending L1 §5 — blend mode. Phase 0 содержит только
/// `Normal` (no-op); остальные 16 mode-ов парсятся в CSS-каскаде, но
/// реальный composite-pipeline для них — задача P2 п.4 (mix-blend-mode).
/// `PlusLighter` — из CSS Compositing & Blending L2 §6, реализуется
/// как additive compositing с pre-multiplied alpha.
/// Хранится в `DisplayCommand::PushBlendMode` как stub-значение, чтобы
/// расширить enum без правки потребителей.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    PlusLighter,
}

impl BlendMode {
    /// Парсит CSS-keyword `mix-blend-mode` / `background-blend-mode` (CSS
    /// Compositing & Blending L1 §5). Case-insensitive — `MULTIPLY` и
    /// `multiply` оба возвращают `Multiply`. Возвращает `None` на
    /// нераспознанной строке; caller (CSS-каскад) трактует это как
    /// invalid declaration и применяет initial value (`Normal`).
    #[must_use]
    pub fn from_keyword(s: &str) -> Option<Self> {
        // ASCII case fold — keyword-ы CSS все ASCII, дешёвый match
        // через to_ascii_lowercase в стек-буфер не нужен (хватает
        // `eq_ignore_ascii_case`).
        for (kw, mode) in [
            ("normal", Self::Normal),
            ("multiply", Self::Multiply),
            ("screen", Self::Screen),
            ("overlay", Self::Overlay),
            ("darken", Self::Darken),
            ("lighten", Self::Lighten),
            ("color-dodge", Self::ColorDodge),
            ("color-burn", Self::ColorBurn),
            ("hard-light", Self::HardLight),
            ("soft-light", Self::SoftLight),
            ("difference", Self::Difference),
            ("exclusion", Self::Exclusion),
            ("hue", Self::Hue),
            ("saturation", Self::Saturation),
            ("color", Self::Color),
            ("luminosity", Self::Luminosity),
            ("plus-lighter", Self::PlusLighter),
        ] {
            if s.eq_ignore_ascii_case(kw) {
                return Some(mode);
            }
        }
        None
    }
}

/// CSS Masking L1 §6 — how to derive the mask value from rendered mask-layer pixels.
///
/// `Alpha` is the default for raster images (§6.2). `Luminance` converts the mask
/// layer's RGB colour to relative luminance per ITU-R BT.709, then multiplies by
/// the alpha channel — identical to SVG `mask-type: luminance` (§6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaskMode {
    /// Use the mask layer's alpha channel directly as the mask value (default).
    #[default]
    Alpha,
    /// Convert the mask layer's colour to luminance: `luma = 0.2126·R + 0.7152·G + 0.0722·B`,
    /// then multiply by alpha. White opaque → mask=1, black opaque → mask=0.
    Luminance,
}

/// Corner radii for CSS `border-radius`. Values are in CSS pixels, clamped to ≥ 0.
/// Each corner stores separate horizontal (x) and vertical (y) radii supporting
/// elliptical corners (`border-radius: 10px / 20px`). When x == y the corner is circular.
/// Order matches CSS shorthand resolution: top-left, top-right, bottom-right, bottom-left.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CornerRadii {
    /// Top-left horizontal (x) radius in CSS px.
    pub tl: f32,
    /// Top-left vertical (y) radius in CSS px.
    pub tl_y: f32,
    /// Top-right horizontal (x) radius in CSS px.
    pub tr: f32,
    /// Top-right vertical (y) radius in CSS px.
    pub tr_y: f32,
    /// Bottom-right horizontal (x) radius in CSS px.
    pub br: f32,
    /// Bottom-right vertical (y) radius in CSS px.
    pub br_y: f32,
    /// Bottom-left horizontal (x) radius in CSS px.
    pub bl: f32,
    /// Bottom-left vertical (y) radius in CSS px.
    pub bl_y: f32,
}

impl CornerRadii {
    /// Returns `true` if all eight radii are zero (no rounding needed).
    #[must_use]
    pub fn all_zero(&self) -> bool {
        self.tl == 0.0 && self.tr == 0.0 && self.br == 0.0 && self.bl == 0.0
            && self.tl_y == 0.0 && self.tr_y == 0.0 && self.br_y == 0.0 && self.bl_y == 0.0
    }

    fn resolve_radius(len: &Length, basis: f32) -> f32 {
        match len {
            Length::Px(v) => *v,
            Length::Percent(p) => p / 100.0 * basis,
            _ => 0.0,
        }
    }

    /// Builds `CornerRadii` from a `ComputedStyle` and the element's border-box dimensions.
    /// `border_w` / `border_h` resolve `border-radius: N%` per CSS Backgrounds L3 §5.5:
    /// H radii use width as basis, V radii use height.
    pub fn from_style_and_box(s: &ComputedStyle, border_w: f32, border_h: f32) -> Self {
        Self {
            tl:   Self::resolve_radius(&s.border_top_left_radius,     border_w),
            tl_y: Self::resolve_radius(&s.border_top_left_radius_y,   border_h),
            tr:   Self::resolve_radius(&s.border_top_right_radius,    border_w),
            tr_y: Self::resolve_radius(&s.border_top_right_radius_y,  border_h),
            br:   Self::resolve_radius(&s.border_bottom_right_radius,   border_w),
            br_y: Self::resolve_radius(&s.border_bottom_right_radius_y, border_h),
            bl:   Self::resolve_radius(&s.border_bottom_left_radius,   border_w),
            bl_y: Self::resolve_radius(&s.border_bottom_left_radius_y, border_h),
        }
    }

    /// Builds `CornerRadii` from a `ComputedStyle`. `border-radius: N%` values are
    /// resolved as 0 because box dimensions are unavailable here. Prefer
    /// `from_style_and_box` when the border-box rect is known.
    pub fn from_style(s: &ComputedStyle) -> Self {
        Self::from_style_and_box(s, 0.0, 0.0)
    }

    /// Clamps every radius via the CSS Backgrounds L3 §5.5 corner-overlap rule.
    /// `w`/`h` are the box dimensions the radii apply to. A single scale factor
    /// `f ≤ 1` is chosen so the sum of the two radii along every edge fits that
    /// edge's length, then **all** radii are multiplied by `f`. Computed from the
    /// specified radii per-axis (x-radii against `w`, y-radii against `h`), so a
    /// wide-but-short elliptical corner (`rx ≠ ry`, e.g. an SVG `<ellipse>` mapped
    /// to a 240×90 box) keeps its aspect. The earlier naive `min(w/2, h/2)` cap
    /// collapsed such corners into circles (BUG-198), turning ellipses into
    /// stadiums in the femtovg/border paths. For uniform radii the result is
    /// unchanged from that cap.
    #[must_use]
    pub fn clamped_to_box(&self, w: f32, h: f32) -> Self {
        if w <= 0.0 || h <= 0.0 {
            return Self::default();
        }
        // Per-edge ratio: edge length / sum of the two radii along it (capped at 1).
        let ratio = |len: f32, sum: f32| if sum > len { len / sum } else { 1.0 };
        let f = ratio(w, self.tl + self.tr)        // top edge (x-radii)
            .min(ratio(h, self.tr_y + self.br_y))  // right edge (y-radii)
            .min(ratio(w, self.br + self.bl))      // bottom edge (x-radii)
            .min(ratio(h, self.bl_y + self.tl_y))  // left edge (y-radii)
            .clamp(0.0, 1.0);
        let s = |r: f32| (r * f).max(0.0);
        Self {
            tl: s(self.tl),   tl_y: s(self.tl_y),
            tr: s(self.tr),   tr_y: s(self.tr_y),
            br: s(self.br),   br_y: s(self.br_y),
            bl: s(self.bl),   bl_y: s(self.bl_y),
        }
    }

    /// Computes the inner-edge corner radii for a border of per-side widths
    /// `[top, right, bottom, left]` (CSS px). Each inner radius is the outer
    /// radius minus the adjacent border width, floored at 0 — the standard CSS
    /// border inner-radius rule (CSS Backgrounds L3 §5.5). A corner's horizontal
    /// radius is reduced by the adjacent vertical border (left/right), its
    /// vertical radius by the adjacent horizontal border (top/bottom).
    #[must_use]
    pub fn inner_for_border(&self, widths: [f32; 4]) -> Self {
        let [top, right, bottom, left] = widths;
        Self {
            tl:   (self.tl   - left).max(0.0),
            tl_y: (self.tl_y - top).max(0.0),
            tr:   (self.tr   - right).max(0.0),
            tr_y: (self.tr_y - top).max(0.0),
            br:   (self.br   - right).max(0.0),
            br_y: (self.br_y - bottom).max(0.0),
            bl:   (self.bl   - left).max(0.0),
            bl_y: (self.bl_y - bottom).max(0.0),
        }
    }
}

/// BUG-140: `clip-path` basic-shape, разрешённая эмиттером в page-координаты
/// (px) относительно border-box элемента. Координаты — в пространстве ДО
/// transform элемента: команда `PushClipPath` эмитится внутри
/// `PushTransform`, бэкенд переносит форму активной матрицей канвы.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedClipShape {
    /// `circle(r at cx cy)`: центр и радиус в page px.
    Circle {
        /// Центр X (page px).
        cx: f32,
        /// Центр Y (page px).
        cy: f32,
        /// Радиус (px).
        r: f32,
    },
    /// `ellipse(rx ry at cx cy)`: центр и полуоси в page px.
    Ellipse {
        /// Центр X (page px).
        cx: f32,
        /// Центр Y (page px).
        cy: f32,
        /// Горизонтальная полуось (px).
        rx: f32,
        /// Вертикальная полуось (px).
        ry: f32,
    },
    /// `polygon(...)` / `path(...)`: вершины в page px. `even_odd` выбирает
    /// правило заливки самопересекающихся контуров (CSS Shapes L1 §3/§4):
    /// `true` → even-odd (дырки в перекрытиях), `false` → nonzero (default).
    Polygon {
        /// Вершины формы в page px (до transform элемента).
        verts: Vec<(f32, f32)>,
        /// `true` = even-odd fill rule, `false` = nonzero.
        even_odd: bool,
    },
}

impl ResolvedClipShape {
    /// Axis-aligned bounding box формы (page px, до transform). Используется
    /// fallback-путями, не умеющими клиппить произвольную форму (wgpu
    /// scissor, hit-test).
    pub fn bounding_rect(&self) -> Rect {
        match self {
            Self::Circle { cx, cy, r } => {
                Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r)
            }
            Self::Ellipse { cx, cy, rx, ry } => {
                Rect::new(cx - rx, cy - ry, 2.0 * rx, 2.0 * ry)
            }
            Self::Polygon { verts, .. } => {
                if verts.is_empty() {
                    return Rect::new(0.0, 0.0, 0.0, 0.0);
                }
                let mut mn_x = f32::MAX;
                let mut mn_y = f32::MAX;
                let mut mx_x = f32::MIN;
                let mut mx_y = f32::MIN;
                for (x, y) in verts {
                    mn_x = mn_x.min(*x);
                    mn_y = mn_y.min(*y);
                    mx_x = mx_x.max(*x);
                    mx_y = mx_y.max(*y);
                }
                Rect::new(mn_x, mn_y, (mx_x - mn_x).max(0.0), (mx_y - mn_y).max(0.0))
            }
        }
    }
}

mod commands;
pub use commands::{DisplayCommand, DisplayList};

mod geometry;
pub use geometry::{contains_backdrop_filter, cull_display_list, fit_image_quad, fit_image_rect, ProvenanceIndex, ProvenanceSpan};
#[cfg(any(feature = "backend-wgpu", feature = "cpu-render", feature = "backend-femtovg"))]
pub(crate) use geometry::{split_mixed_runs, MixedSegment};
pub(crate) use geometry::space_axis_geometry;
#[cfg(any(feature = "backend-femtovg", feature = "cpu-render"))]
pub(crate) use geometry::bg_tile_geometry;
// Used only by `display_list/serialize.rs` (via `super::*`).
use geometry::{border_style_short, object_fit_name, position_component_name};

mod fingerprint;
pub(crate) use fingerprint::{hash_command_into, hash_one_command};
// Used only by `display_list/tests/svg_table_and_hash.rs` (via `super::*`).
#[cfg(test)]
pub(crate) use fingerprint::{h_color, h_f32, h_rect, h_str, HashFmt};
// Needed by `display_list/geometry.rs::cull_display_list` (via `super::*`).
use fingerprint::get_command_rect;
pub use fingerprint::{
    anim_split_compose_plan, diff_display_lists, fold_content_dual, fold_overlay,
    fold_overlay_with_reuse, hash_content, hash_display_list, hash_display_list_dual,
    hash_display_list_dual_memo, hash_display_list_dual_memo_with_overlay_digests,
    hash_display_list_skipping, DiffResult, FrameDelta, FrameFingerprint,
};

mod serialize;
pub use serialize::serialize_display_list;

mod builder;
pub use builder::{
    build_display_list, build_display_list_ordered, build_display_list_ordered_dpr,
    build_display_list_ordered_with_anim, build_display_list_ordered_with_anim_dpr,
    build_display_list_ordered_with_anim_split, build_display_list_with_anim,
    build_display_list_with_selection,
};
// Used only by `display_list/box_layer.rs` (via `super::*`).
use builder::SplitTracker;

mod print;
pub use print::{build_print_display_list, split_at_page_breaks, strip_background_graphics};
// Used by `display_list/{box_layer,walk}.rs` and `display_list/tests/anim_and_chrome.rs`
// (via `super::*` / explicit `use super::clip_path_to_rect`).
use print::clip_path_to_rect;
// Used only by `display_list/text_run.rs` (via `super::*`).
use print::ELLIPSIS_EM;
// Used by `display_list/{box_layer,walk,background_mask}.rs` (via `super::*`).
use print::map_blend_mode;
// Used by `display_list/{box_layer,scrollbars,text_run,walk}.rs` (via `super::*`).
use print::overflow_clips;
// Used only by `display_list/box_layer.rs` (via `super::*`).
use print::{record_span, BucketField, RawSpan, ScBucket};
// Used by `clip_path_to_shape`, still in this file just below the DL-15 region.
use print::resolve_shape_center;

/// BUG-140: резолвит `clip-path` в точную форму в page-координатах
/// (пространство до transform элемента) относительно border-box `r`.
/// `None` для `inset(...)` — он точно представим прямоугольником и эмитится
/// как `PushClipRect` (см. `clip_path_to_rect`). Базисы процентов —
/// CSS Shapes L1 §5: x/width, y/height, радиус circle — `sqrt(w²+h²)/√2`.
fn clip_path_to_shape(clip: &ClipPath, r: Rect) -> Option<ResolvedClipShape> {
    match clip {
        ClipPath::Inset(_) => None,
        ClipPath::Circle { radius, center } => {
            let (cx, cy) = resolve_shape_center(*center, r);
            let diag = ((r.width * r.width + r.height * r.height) * 0.5).sqrt();
            Some(ResolvedClipShape::Circle { cx, cy, r: radius.resolve(diag) })
        }
        ClipPath::Ellipse { rx, ry, center } => {
            let (cx, cy) = resolve_shape_center(*center, r);
            Some(ResolvedClipShape::Ellipse {
                cx,
                cy,
                rx: rx.resolve(r.width),
                ry: ry.resolve(r.height),
            })
        }
        ClipPath::Polygon(vertices, fill_rule) => {
            if vertices.is_empty() {
                return None;
            }
            Some(ResolvedClipShape::Polygon {
                verts: vertices
                    .iter()
                    .map(|(x, y)| (r.x + x.resolve(r.width), r.y + y.resolve(r.height)))
                    .collect(),
                even_odd: matches!(fill_rule, FillRule::EvenOdd),
            })
        }
        // CSS Shapes L1 §4 — `path()`: точки уже флэттены в px системы пути
        // (origin = верхний левый угол reference box). Смещаем на позицию box.
        ClipPath::Path(points, fill_rule) => {
            if points.len() < 3 {
                return None;
            }
            Some(ResolvedClipShape::Polygon {
                verts: points.iter().map(|(x, y)| (r.x + x, r.y + y)).collect(),
                even_odd: matches!(fill_rule, FillRule::EvenOdd),
            })
        }
    }
}

/// Union of a line's visible fragments, as a painting rect for
/// [`text_run::emit_first_line_background`]. `None` when the line contributes no extent.
///
/// Fragment padding/border are included only for real inline element boxes —
/// for anonymous text fragments those style fields belong to the enclosing
/// element and would widen the union by space the text does not occupy.
fn first_line_content_rect(b: &LayoutBox, line: &[InlineFrag], line_h: f32) -> Option<Rect> {
    let mut left = f32::MAX;
    let mut right = f32::MIN;
    for frag in line.iter() {
        if !matches!(frag.style.visibility, Visibility::Visible) {
            continue;
        }
        let (pad_l, pad_r, bl, br) = if frag.is_element_box {
            (
                frag.padding_left,
                frag.padding_right,
                frag.style.border_left_width,
                frag.style.border_right_width,
            )
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };
        let l = b.rect.x + frag.x - pad_l - bl;
        let r = b.rect.x + frag.x + frag.width + pad_r + br;
        if r <= l {
            continue;
        }
        left = left.min(l);
        right = right.max(r);
    }
    if right <= left {
        return None;
    }
    // Same integer-pixel snapping `emit_inline_frag_box` applies, so an inline
    // background and a ::first-line background under it share their edges.
    Some(Rect::new(left.round(), b.rect.y.round(), (right - left).round(), line_h.round()))
}

/// CSS Images L4 §5 — selects the best `image-set()` candidate URL for `dpr`.
///
/// Parses an `image-set( <option># )` expression where each option is
/// `<url-or-string> [<resolution>]`. Resolution defaults to `1x`. Supported
/// resolution units: `x` / `dppx` (device pixel ratio), `dpi`, `dpcm`.
///
/// Returns the URL (with `url(…)` wrapper and surrounding quotes stripped)
/// whose resolution is closest to `dpr`; ties prefer the higher resolution
/// (sharper asset). If `value` is not an `image-set()` expression the whole
/// trimmed value is treated as a single 1× option, so plain URLs pass through
/// unchanged. Returns `""` when no candidate can be parsed.
///
/// The result is a subslice of `value` — no allocation.
#[must_use]
pub fn select_image_set_url(value: &str, dpr: f32) -> &str {
    let trimmed = value.trim();
    let inner = strip_image_set_wrapper(trimmed).unwrap_or(trimmed);

    let mut best: Option<(&str, f32)> = None;
    for opt in split_top_level_commas(inner) {
        let opt = opt.trim();
        if opt.is_empty() {
            continue;
        }
        let (url, res) = parse_image_set_option(opt);
        if url.is_empty() {
            continue;
        }
        let better = match best {
            None => true,
            Some((_, bres)) => {
                let d = (res - dpr).abs();
                let bd = (bres - dpr).abs();
                d < bd || (d == bd && res > bres)
            }
        };
        if better {
            best = Some((url, res));
        }
    }
    best.map_or("", |(u, _)| u)
}

mod text_run;
use text_run::emit_inline_run;

mod box_layer;
use box_layer::fill_buckets;

mod inline_frag;
use inline_frag::{
    emit_inline_frag_box, emit_text_shadows, mask_clip_paint_rect, parse_image_set_option,
    split_top_level_commas, strip_image_set_wrapper,
};
pub use inline_frag::is_image_set;
pub(crate) use inline_frag::{background_clip_rect, background_color_clip};
// Used only by `display_list/background_mask.rs` (via `super::*`).
use inline_frag::background_origin_rect;
// Used only by `display_list/walk.rs` (via `super::*`).
use inline_frag::content_box_rect;

mod background_mask;
use background_mask::{emit_background_image, emit_push_mask, rendered_mask_layers};
// Used only by `display_list/tests/background_and_layers.rs` (via `super::*`).
#[cfg(test)]
use background_mask::gradient_tile_rects;
// Used only by `display_list/tests/anim_and_chrome.rs` (via `super::*`).
#[cfg(test)]
use background_mask::mask_stops_for_mode;

mod box_shadow;
use box_shadow::{emit_box_shadows, emit_inset_box_shadows};

mod scrollbars;
use scrollbars::emit_scrollbars;
pub use scrollbars::patch_scroll_layer;
// Used only by `display_list/tests/anim_and_chrome.rs` (via `super::*`).
#[cfg(test)]
use scrollbars::{SCROLLBAR_THUMB_COLOR, SCROLLBAR_TRACK_COLOR, SCROLLBAR_WIDTH_THIN};

mod outline_misc;
use outline_misc::{emit_column_rules, emit_outline, emit_resize_grip};
pub use outline_misc::point_on_resize_grip;
pub(crate) use outline_misc::{is_opacity_subtree_painted, is_paint_visible};

mod form_controls;
use form_controls::{emit_form_control_indicator, emit_list_marker, push_thick_segment};
pub(crate) use form_controls::is_hidden_empty_cell;
// Used only by `display_list/tests/svg_table_and_hash.rs` (via `super::meter_gauge_color`).
#[cfg(test)]
use form_controls::meter_gauge_color;

mod svg_text_decoration;
use svg_text_decoration::{emit_svg_shape, emit_svg_shape_masked, emit_svg_text, push_text_decoration, walk_with_anim};

#[cfg(test)]
#[path = "display_list/tests/text_and_images.rs"]
mod text_and_images;

mod text_highlight;
pub use text_highlight::emit_text_with_highlights;

mod table;
use table::{collect_table_cells, emit_table_box, emit_table_cell_border};

mod walk;
use walk::{
    depth_sorted_child_order, emit_box_self, establishes_3d_rendering_context, is_backface_hidden,
    walk,
};
// Used only by `display_list/tests/shadows_and_transforms.rs` (via `use super::*`).
#[cfg(test)]
use walk::depth_order_by_z;

#[cfg(test)]
#[path = "display_list/tests/highlight.rs"]
mod highlight;

#[cfg(test)]
#[path = "display_list/tests/svg_table_and_hash.rs"]
mod svg_table_and_hash;

#[cfg(test)]
#[path = "display_list/tests/anim_and_chrome.rs"]
mod anim_and_chrome;

#[cfg(test)]
#[path = "display_list/tests/shadows_and_transforms.rs"]
mod shadows_and_transforms;

#[cfg(test)]
#[path = "display_list/tests/background_and_layers.rs"]
mod background_and_layers;

#[cfg(test)]
#[path = "display_list/tests/ordered_build_scroll.rs"]
mod ordered_build_scroll;

#[cfg(test)]
#[path = "display_list/tests/form_controls_caret.rs"]
mod form_controls_caret;
