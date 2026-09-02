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

mod paint_types;
pub use paint_types::{BlendMode, CornerRadii, FilterMode, MaskMode, ResolvedClipShape};

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
