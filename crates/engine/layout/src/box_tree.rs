//! Box tree: block-флоу + inline-флоу.
//!
//! Каждый DOM-элемент даёт один LayoutBox. Блочные элементы стэкаются
//! вертикально. Текстовые узлы и inline-элементы (`<a>`, `<span>`, `<em>`,
//! `<strong>`, и т.д.) объединяются в `InlineRun` — анонимный бокс, в
//! котором слова переносятся как единый поток. Слова с одинаковым стилем
//! на одной строке объединяются в один фрагмент (→ один DrawText).
//!
//! Whitespace-only текст и комментарии пропускаются.

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use lumen_core::geom::{Rect, Size};
use lumen_core::ext::{HyphenationProvider, NullHyphenationProvider};
use lumen_css_parser::Stylesheet;
use lumen_dom::{build_flat_tree, Document, FlatTree, NodeData, NodeId};
use lumen_html_parser::{
    PictureParams, SizesViewport, pick_img_source, pick_picture_source,
};

use crate::style::{
    apply_container_rules, clear_cq_context, compute_pseudo_element_style, compute_style,
    set_cq_context, AlignValue,
    BackgroundImage, BorderCollapse, BoxSizing, ClearSide, ContainFlags, ContainerContext, ContainerType, Content,
    ContentItem, ComputedStyle, Direction, Display, FlexBasis, FlexDirection, FlexWrap, FloatSide,
    FontVariantCaps,
    GridAutoFlow, GridLine, GridTrackSize, Hyphens, Length, LengthOrAuto, LineBreak,
    ListStylePosition,
    ListStyleType, Overflow, OverflowWrap, Position, ScrollbarGutter, ScrollbarWidth,
    TextAlign, TextAlignLast, TextOverflow,
    TextWrapMode, TextWrapStyle,
    VerticalAlign, WordBreak,
};
use crate::counters::{precompute_counters, CounterMap, CounterStyleRegistry, QuoteSlot,
                      build_counter_style_registry, format_counter_with_registry,
                      build_list_marker_text};
use crate::subgrid::{SubgridContext, SubgridContextGuard, SUBGRID_COL_CTX, SUBGRID_ROW_CTX};
use crate::anchor::{collect_anchors, InsetAreaKeyword};
use crate::field_sizing::field_sizing_content_intrinsic;
use crate::style::FieldSizing;
use crate::TextMeasurer;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

mod container_anchor;
pub use container_anchor::apply_container_styles;
use container_anchor::apply_anchor_positions;

mod inline_wrap;
pub use inline_wrap::{measure_text_w, measure_text_w_families, measure_text_w_varied};
pub(crate) use inline_wrap::strip_soft_hyphens;
use inline_wrap::{
    align_lines, apply_inline_vertical_align, apply_line_clamp, apply_text_overflow_ellipsis, balance_wrap,
    one_line_fallback, pretty_wrap, step_line_height, wrap_inline_run,
};
// Used only by `mod tests` (super::super::X) — never called from this file's own non-test code.
#[cfg(test)]
use inline_wrap::{caps_synthesis, char_break_offset, try_hyp_break, SMALL_CAPS_SCALE};

mod grid;
pub use grid::resolve_auto_fill_fit_count;
use grid::lay_out_grid;

mod flex;
use flex::{lay_out_flex, UsedSizeOverride};

mod multicol_abspos;
use multicol_abspos::{lay_out_abs_children, lay_out_multicol_children};

mod table;
use table::{lay_out_table, lay_out_table_row, table_intrinsic_content_width};

mod diagnostics;
pub use diagnostics::{
    BoxBuildStats, BoxCopyStats, LayoutKeyCensus, LayoutResultCacheStats,
    incremental_box_build_enabled, layout_result_cache_enabled, set_box_build_diagnostics,
    set_box_time_diagnostics, set_incremental_box_build, set_layout_key_census,
    set_layout_result_cache, take_box_build_log, take_box_build_stats, take_box_build_time_log,
    take_box_copy_stats, take_box_probe_ns, take_layout_key_census, take_layout_result_cache_stats,
};
use diagnostics::{
    BOX_BUILD_STATS, BOX_BUILD_TIME_LOG, BOX_CLONE_BOXES, BOX_CLONE_NS, BOX_TIME_LOG_ON,
    CV_AUTO_TOUCHED, FlexProbeKey, FLEX_COLUMN_PROBE_HEIGHTS, INDEFINITE_HEIGHT_CONSULTED,
    LAYOUT_RESULT_CACHE, LAYOUT_RESULT_CACHE_STATS, LayoutPassGuard, LayoutResultEntry,
    LayoutResultKey, UsedSizeOverrideBits, add_box_build_stats, box_build_diagnostics_on,
    cacheable_for_layout_result_cache, count_boxes, note_box_built, note_display_probe,
    note_prev_index, note_style_miss, record_layout_key_occurrence, resolve_block_size,
};

mod predicates;
use predicates::{
    is_audio_element, is_canvas_element, is_iframe_element, is_image_element, is_picture_element,
    is_video_element, scrollbar_gutter_block, scrollbar_gutter_inline,
};

// EE-3: when true, `lay_out` checks `b.dirty.is_clean()` and skips clean subtrees.
thread_local! {
    static INCREMENTAL_LAYOUT_MODE: Cell<bool> = const { Cell::new(false) };
}

thread_local! {
    /// BUG-341 S4 — master on/off switch for incremental box-build
    /// (`build_box_or_reuse`'s whole-subtree clone path), mirroring
    /// `counters::INCREMENTAL_RESTYLE`'s pattern. Off by default; S15 turns it
    /// on around `layout_mutation_incremental_restyle` at the pipeline call
    /// sites, alongside `counters::set_incremental_restyle`.
    static INCREMENTAL_BOX_BUILD: Cell<bool> = const { Cell::new(false) };
}

mod svg;
pub use svg::{
    FormControlKind, PreserveAspectRatio, SvgAlignX, SvgAlignY, SvgBaselineShift,
    SvgDominantBaseline, SvgMeetOrSlice, SvgShapeKind, SvgTextAnchor, SvgTransform, ViewBox,
    collect_selectlist_label, is_open_details, is_selectlist,
};
use svg::{
    build_svg_children, collect_select_label, collect_textarea_content, is_closed_popover,
    is_details_element, is_form_control_element, is_summary_element, is_svg_defs, is_svg_root,
    lay_out_svg_root, parse_preserve_aspect_ratio, parse_view_box, svg_root_own_size, ImageSource,
};
#[cfg(test)]
use svg::{
    compute_preserve_aspect_ratio_transform, parse_svg_points, parse_svg_transform,
    points_to_path_d, svg_intrinsic_ratio,
};

mod image_requests;
pub use image_requests::{
    apply_intrinsic_size, collect_background_image_requests, collect_image_requests, ImageRequest,
};
use image_requests::resolve_image_source;

mod types;
pub use types::{BoxKind, BoxOrigin, BoxRole, InlineFrag, InlineSegment, LayoutBox, PseudoKind, SvgMaskContent};

mod pseudo_text;
use pseudo_text::{
    apply_first_letter_style, apply_first_line_pseudo_styles, extract_first_letter_float,
    extract_initial_letter, first_letter_text_len, split_first_line_boxes,
    split_segments_at_first_line,
};
#[cfg(test)]
use pseudo_text::is_first_letter_box;

mod entry;
use entry::{is_invisible_control, strip_invisible_controls};
#[cfg(test)]
use entry::{apply_font_size_adjust, font_size_adjust_used};
pub use entry::{
    build_iframe_document, canvas_background_color, lay_out_incremental, layout, layout_measured,
    layout_measured_hyp, layout_measured_hyp_with_counters, layout_mutation_incremental,
    layout_mutation_incremental_restyle, layout_streaming_incremental,
};

mod inline_build;
use inline_build::{
    anon_inline_block_row, anon_inline_run, anon_style, breaks_inline_row, build_anon_text_item,
    collect_inline_segments, control_value_segments, flatten_contents, inject_marker, inject_pseudo,
    inline_baseline, inline_run_advance, inline_run_lead_space, inline_v_align, is_atomic_inline_level,
    is_inline_content, li_ordinal, probe_display, split_inline_pieces, assign_first_line_style,
    InlineEscape,
};

mod build;
use build::{build_box, build_box_or_reuse};
pub use build::incremental_build_box;

mod intrinsic;
use intrinsic::{
    flex_auto_base_main_width, flex_item_max_main_outer, flex_item_min_main_width,
    max_content_outer_width, min_content_outer_width, preferred_inline_block_width,
};

mod shapes_floats;
use shapes_floats::{
    parse_circle_px, parse_shape_ellipse_px, parse_shape_inset_px, parse_shape_path_px,
    parse_shape_polygon_px, shift_tree, shift_y_box, FloatContext, ShapeEllipse, ShapeInset,
    ShapePolygon,
};
// Used only by `mod tests` (super::super::X) — never called from this file's
// own non-test code.
#[cfg(test)]
use shapes_floats::{inset_corner_inward, polygon_left_edge_at_y, polygon_right_edge_at_y};

mod bfc;
mod layout_dispatch;

pub(crate) use bfc::lay_out_for_vertical;
use bfc::{
    collapsed_bottom_margin, collapsed_top_margin, contained_content_height, establishes_bfc,
    has_in_flow_content, last_collapsible_child,
};
use layout_dispatch::{lay_out, lay_out_with_used_size};

#[cfg(test)]
mod tests;

