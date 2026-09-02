//! Главный проход каскада: `compute_style` — построение `ComputedStyle` узла
//! из UA-таблицы, презентационных атрибутов, author-правил и инлайнового
//! `style`, — плюс CSS Viewport L1 §5 `zoom` и счётчик полных проходов
//! (BUG-341 S18).
//!
//! Перенесено батчем SPLIT-ST14 из `crates/engine/layout/src/style.rs`
//! (анкер `static COMPUTE_STYLE_CALLS`) без правок тел.

use std::collections::HashMap;

use lumen_core::geom::Size;
use lumen_css_parser::{
    parse_inline_style, Declaration, PropertyRule, Specificity, Stylesheet,
};
use lumen_dom::{Document, DocumentMode, NodeData, NodeId};

use crate::font_palette::resolve_font_palette_overrides;
use crate::scroll_timeline::ScrollAxis;
use crate::style::{
    apply_align_presentational_hint, apply_background_image_presentational_hint,
    apply_bgcolor_presentational_hint, apply_bordercolor_presentational_hint,
    apply_cellspacing_presentational_hint, apply_declaration,
    apply_font_element_presentational_hints, apply_font_size, apply_forced_colors_mode,
    apply_image_presentational_hints, apply_property_initial_values, apply_quirks_html_height,
    apply_quirks_line_height, apply_quirks_table_reset, apply_svg_presentational_hints,
    apply_table_cell_width_hint, apply_text_color_presentational_hint, apply_ua_body_margin,
    apply_ua_dialog_display, apply_ua_form_controls, apply_ua_form_controls_field_sizing_clear,
    apply_ua_heading_style, apply_ua_hr_style, apply_ua_inert, apply_ua_table_cell_padding,
    apply_ua_text_decoration, apply_webkit_scrollbar_pseudos, coerce_overflow_axes,
    complex_has_host, default_display, ensure_cascade_index, expand_attr_val,
    expand_custom_functions, expand_vars, forced_colors_active, matches_complex,
    matches_slotted_complex, node_in_scope, resolve_logical_properties,
    resolve_system_colors_in_style, strip_ua_appearance_box_styling, ua_font_family,
    ua_font_size_factor, ua_font_style, ua_font_weight, ua_link_color, ua_vertical_align,
    ua_white_space, validate_against_syntax, with_front_cascade_index, AlignValue, Appearance,
    BackfaceVisibility, BorderStyle, BoxSizing, BreakValue, ClearSide,
    ComputedStyle, ContainFlags, ContainerType, Content, ContentVisibility, CssColor,
    FieldSizing, FlexBasis, FlexDirection, FlexWrap, FloatSide,
    FontPalette, FontSizeBasis, FontWeight, GridAutoFlow, GridLine, GridTrackSize, Isolation,
    Length, LengthOrAuto, MasonryAutoFlow, MixBlendMode, ObjectFit, ObjectPosition, OffsetRotate,
    OutlineColor, OutlineStyle, Overflow, OverscrollBehavior, PointerEvents, Position,
    PositionComponent, PrintColorAdjust, Resize, ScrollSnapAlign, ScrollSnapStop, ScrollSnapType,
    ScrollbarGutter, ShapeOutside, TextAlignLast, TextOverflow, TouchAction, TransformStyle,
    UnicodeBidi, VerticalAlign, WhiteSpace, SHADOW_HOST_SCOPE, SHADOW_SHEETS,
};

/// BUG-341 S18 — process-wide tally of full [`compute_style`] runs.
///
/// The cascade stage's own [`crate::counters::CascadeStats`] counts only the
/// calls `counters::walk` makes. It cannot see the ones the box-build stage
/// makes behind its back: `is_inline_content` / `is_inline_block` probe every
/// child of every rebuilt container with a fresh `compute_style` instead of the
/// `CounterMap` cache `build_box` itself uses, and non-element nodes have no
/// cache entry at all. Process-wide (an atomic, not a thread-local) because
/// `build_box` fans out over rayon workers — the S15 trap.
static COMPUTE_STYLE_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Returns the number of [`compute_style`] runs since the last drain, and
/// resets the tally (see [`COMPUTE_STYLE_CALLS`]).
pub fn take_compute_style_calls() -> u64 {
    COMPUTE_STYLE_CALLS.swap(0, std::sync::atomic::Ordering::Relaxed)
}

/// Bumps the [`COMPUTE_STYLE_CALLS`] tally.
fn note_compute_style() {
    COMPUTE_STYLE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// CSS Viewport L1 §5 — parse the specified value of `zoom`.
///
/// Accepted: a non-negative `<number>` (`0.8`, `.8`, `1`), a `<percentage>`
/// (`80%`), and the keywords `normal` / `reset`, both of which mean "no scaling
/// of my own" and so yield `1.0`. (`reset`'s real WebKit semantics — ignore the
/// ancestors' zoom rather than merely contributing 1.0 — are not modelled;
/// nothing in the wild depends on it and it would need a separate flag.)
///
/// Returns `None` when the value does not parse, in which case the caller must
/// leave the previous value alone — an invalid declaration is ignored, per
/// CSS Syntax, not treated as `1.0`.
pub(in crate::style) fn parse_zoom(value: &str) -> Option<f32> {
    let v = value.trim();
    if v.eq_ignore_ascii_case("normal") || v.eq_ignore_ascii_case("reset") {
        return Some(1.0);
    }
    let factor = if let Some(pct) = v.strip_suffix('%') {
        pct.trim().parse::<f32>().ok()? / 100.0
    } else {
        v.parse::<f32>().ok()?
    };
    // A negative or non-finite zoom is invalid; a zero one would collapse the
    // subtree to nothing, which no page means and which would divide by zero
    // when un-zooming. Both are rejected so the declaration is simply dropped.
    if !factor.is_finite() || factor <= 0.0 {
        return None;
    }
    Some(factor)
}

/// Scale one already-computed absolute length by `z`. Only `Px` is touched:
/// every other unit resolves later against a basis (`font_size`, the containing
/// block, the viewport) that is itself already zoomed, so scaling here too
/// would apply the factor twice.
fn zoom_length(len: &mut Length, z: f32) {
    if let Length::Px(v) = len {
        *v *= z;
    }
}

/// Same for a `<length> | auto` field — `auto` carries no length to scale.
fn zoom_length_or_auto(len: &mut LengthOrAuto, z: f32) {
    if let LengthOrAuto::Length(l) = len {
        zoom_length(l, z);
    }
}

/// CSS Viewport L1 §5 — fold the element's effective `zoom` into its computed
/// box-model lengths.
///
/// Runs after the main cascade pass, so it sees the winning declarations. Every
/// property scaled here is **non-inherited**, which is what makes a blanket
/// multiply correct: the value is either specified on this element (and so has
/// not been scaled by anyone) or is the initial `0`/`auto`/`none` (where
/// scaling is a no-op). Inherited length properties are deliberately absent —
/// they arrive already carrying the ancestors' zoom, so touching them would
/// double-apply it.
///
/// `font_size` is handled by the caller rather than here, because it is the one
/// value whose correct factor depends on whether the element specified it (see
/// the call site).
fn apply_zoom_to_lengths(style: &mut ComputedStyle, z: f32) {
    if (z - 1.0).abs() < f32::EPSILON {
        return;
    }
    for len in [
        &mut style.width,
        &mut style.height,
        &mut style.min_width,
        &mut style.max_width,
        &mut style.min_height,
        &mut style.max_height,
    ] {
        if let Some(l) = len.as_mut() {
            zoom_length(l, z);
        }
    }
    for len in [
        &mut style.margin_top,
        &mut style.margin_right,
        &mut style.margin_bottom,
        &mut style.margin_left,
        &mut style.top,
        &mut style.right,
        &mut style.bottom,
        &mut style.left,
    ] {
        zoom_length_or_auto(len, z);
    }
    for len in [
        &mut style.padding_top,
        &mut style.padding_right,
        &mut style.padding_bottom,
        &mut style.padding_left,
        &mut style.row_gap,
        &mut style.column_gap,
    ] {
        zoom_length(len, z);
    }
    // Border widths are already resolved to px by the cascade.
    style.border_top_width *= z;
    style.border_right_width *= z;
    style.border_bottom_width *= z;
    style.border_left_width *= z;
}

/// Computes the `ComputedStyle` for `node` by running the CSS cascade.
///
/// `dark_mode` is forwarded to `@media (prefers-color-scheme: dark)` matching.
pub fn compute_style(
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) -> ComputedStyle {
    // BUG-341 S10: permanent per-phase instrumentation. Same-named sibling
    // scopes are merged by `lumen_core::profile`, so a `LUMEN_PROFILE_TREE=1`
    // run prints one aggregated line per phase with a `×N` call count instead
    // of one line per node. Costs a cached bool check per phase when disabled.
    let _prof = lumen_core::profile::scope_detail("compute_style");
    note_compute_style();
    let prof_init = lumen_core::profile::scope_detail("cs_init");
    let mut style = ComputedStyle {
        display: default_display(doc, node),
        // Наследуемые свойства (CSS inherited properties).
        color: inherited.color,
        color_space: inherited.color_space,
        text_align: inherited.text_align,
        direction: inherited.direction,
        // `unicode-bidi` не наследуется (CSS Writing Modes L4 §2.2).
        unicode_bidi: UnicodeBidi::Normal,
        font_size: inherited.font_size,
        // Seeded from the parent so the value compounds; the element's own
        // `zoom` declaration is folded in by the pre-pass below.
        effective_zoom: inherited.effective_zoom,
        line_height: inherited.line_height,
        line_height_is_relative: inherited.line_height_is_relative,
        line_height_step: inherited.line_height_step,
        font_style: inherited.font_style,
        font_weight: inherited.font_weight,
        font_variant_caps: inherited.font_variant_caps,
        font_variant_emoji: inherited.font_variant_emoji,
        font_stretch: inherited.font_stretch,
        font_family: inherited.font_family.clone(),
        font_variation_settings: inherited.font_variation_settings.clone(),
        font_feature_settings: inherited.font_feature_settings.clone(),
        font_palette: inherited.font_palette.clone(),
        font_palette_resolved: inherited.font_palette_resolved.clone(),
        font_optical_sizing: inherited.font_optical_sizing,
        text_transform: inherited.text_transform,
        white_space: ua_white_space(doc, node).unwrap_or(inherited.white_space),
        white_space_collapse: ua_white_space(doc, node)
            .map(WhiteSpace::collapse_component)
            .unwrap_or(inherited.white_space_collapse),
        text_indent: inherited.text_indent.clone(),
        letter_spacing: inherited.letter_spacing,
        word_spacing: inherited.word_spacing,
        text_decoration_line: inherited.text_decoration_line,
        text_decoration_color: inherited.text_decoration_color,
        text_decoration_style: inherited.text_decoration_style,
        text_decoration_thickness: inherited.text_decoration_thickness,
        text_emphasis_style: inherited.text_emphasis_style.clone(),
        text_emphasis_color: inherited.text_emphasis_color,
        text_emphasis_position: inherited.text_emphasis_position,
        text_underline_position: inherited.text_underline_position,
        text_underline_offset: inherited.text_underline_offset,
        text_decoration_skip_ink: inherited.text_decoration_skip_ink,
        accent_color: inherited.accent_color,
        color_scheme: inherited.color_scheme,
        // CSS Color Adjustment L1 §4: forced-color-adjust IS inherited.
        forced_color_adjust: inherited.forced_color_adjust,
        // CSS Variables L1: все custom properties inherited.
        custom_props: inherited.custom_props.clone(),
        // Ненаследуемые — сброс.
        background_color: None,
        width: None,
        height: None,
        min_width: None,
        max_width: None,
        min_height: None,
        max_height: None,
        margin_top: LengthOrAuto::ZERO,
        margin_right: LengthOrAuto::ZERO,
        margin_bottom: LengthOrAuto::ZERO,
        margin_left: LengthOrAuto::ZERO,
        padding_top: Length::Px(0.0),
        padding_right: Length::Px(0.0),
        padding_bottom: Length::Px(0.0),
        padding_left: Length::Px(0.0),
        border_top_width: 0.0,
        border_right_width: 0.0,
        border_bottom_width: 0.0,
        border_left_width: 0.0,
        border_top_style: BorderStyle::None,
        border_right_style: BorderStyle::None,
        border_bottom_style: BorderStyle::None,
        border_left_style: BorderStyle::None,
        border_top_color: CssColor::CurrentColor,
        border_right_color: CssColor::CurrentColor,
        border_bottom_color: CssColor::CurrentColor,
        border_left_color: CssColor::CurrentColor,
        box_sizing: BoxSizing::ContentBox,
        // CSS Positioned Layout L3 §3 / Compositing L1 — не наследуются.
        position: Position::Static,
        top: LengthOrAuto::Auto,
        right: LengthOrAuto::Auto,
        bottom: LengthOrAuto::Auto,
        left: LengthOrAuto::Auto,
        z_index: None,
        float_side: FloatSide::None,
        clear: ClearSide::None,
        initial_letter_size: 1.0,
        initial_letter_sink: 0,
        isolation: Isolation::Auto,
        mix_blend_mode: MixBlendMode::Normal,
        // border-radius не наследуется.
        border_top_left_radius: Length::Px(0.0),
        border_top_right_radius: Length::Px(0.0),
        border_bottom_right_radius: Length::Px(0.0),
        border_bottom_left_radius: Length::Px(0.0),
        border_top_left_radius_y: Length::Px(0.0),
        border_top_right_radius_y: Length::Px(0.0),
        border_bottom_right_radius_y: Length::Px(0.0),
        border_bottom_left_radius_y: Length::Px(0.0),
        // Inherited (CSS Display L3 §4).
        visibility: inherited.visibility,
        // Inherited (CSS UI L4 §8.1).
        cursor: inherited.cursor,
        // text-shadow inherited (CSS Text Decoration L3 §4).
        text_shadow: inherited.text_shadow.clone(),
        // Не наследуется.
        box_shadow: Vec::new(),
        overflow_x: Overflow::Visible,
        overflow_y: Overflow::Visible,
        overflow_clip_margin: None,
        text_overflow: TextOverflow::Clip,
        opacity: 1.0,
        outline_width: 3.0,
        outline_style: OutlineStyle::None,
        outline_color: OutlineColor::Auto,
        outline_offset: Length::Px(0.0),
        // CSS Lists L3 §3 — не наследуются.
        counter_reset: Vec::new(),
        counter_increment: Vec::new(),
        counter_set: Vec::new(),
        // CSS Masking / Transforms / Filter — не наследуются.
        clip_path: None,
        transform: Vec::new(),
        translate: None,
        rotate: None,
        scale: None,
        filter: Vec::new(),
        // Box Alignment gap / Sizing aspect-ratio — не наследуются.
        row_gap: Length::Px(0.0),
        column_gap: Length::Px(0.0),
        // CSS Multi-column — не наследуются.
        column_count: None,
        column_width: None,
        column_rule_width: 0.0,
        column_rule_style: BorderStyle::None,
        column_rule_color: CssColor::CurrentColor,
        gap_rule_width: 0.0,
        gap_rule_style: BorderStyle::None,
        gap_rule_color: CssColor::CurrentColor,
        column_span_all: false,
        column_fill_balance: true,
        break_before: BreakValue::Auto,
        break_after: BreakValue::Auto,
        break_inside: BreakValue::Auto,
        aspect_ratio: None,
        // Box Alignment — все не наследуются, default = Auto.
        align_items: AlignValue::Auto,
        align_self: AlignValue::Auto,
        align_content: AlignValue::Auto,
        justify_items: AlignValue::Auto,
        justify_self: AlignValue::Auto,
        justify_content: AlignValue::Auto,
        // Backgrounds — не наследуются, defaults.
        background_layers: Vec::new(),
        // Will Change / Pointer Events — не наследуются.
        will_change: Vec::new(),
        pointer_events: PointerEvents::Auto,
        touch_action: TouchAction::Auto,
        appearance: Appearance::Auto,
        field_sizing: FieldSizing::Fixed,
        text_align_last: TextAlignLast::Auto,
        // User Select / Scroll Behavior — наследуются.
        user_select: inherited.user_select,
        resize: Resize::None,
        scroll_behavior: inherited.scroll_behavior,
        // Scroll Snap / Overscroll — не наследуются, defaults.
        scroll_snap_type: ScrollSnapType::default(),
        scroll_snap_align: ScrollSnapAlign::default(),
        scroll_snap_stop: ScrollSnapStop::default(),
        scroll_margin_top: 0.0,
        scroll_margin_right: 0.0,
        scroll_margin_bottom: 0.0,
        scroll_margin_left: 0.0,
        scroll_padding_top: 0.0,
        scroll_padding_right: 0.0,
        scroll_padding_bottom: 0.0,
        scroll_padding_left: 0.0,
        overscroll_behavior_x: OverscrollBehavior::Auto,
        overscroll_behavior_y: OverscrollBehavior::Auto,
        // CSS Table — border-collapse and border-spacing are inherited (CSS Tables L2 §17.6).
        border_collapse: inherited.border_collapse,
        empty_cells: inherited.empty_cells,
        border_spacing_h: inherited.border_spacing_h,
        border_spacing_v: inherited.border_spacing_v,
        // CSS Text typography — все inherited.
        tab_size: inherited.tab_size,
        caret_color: inherited.caret_color,
        overflow_wrap: inherited.overflow_wrap,
        word_break: inherited.word_break,
        line_break: inherited.line_break,
        hyphens: inherited.hyphens,
        // CSS Transforms — не наследуются.
        transform_origin: (PositionComponent::Percent(0.5), PositionComponent::Percent(0.5), 0.0),
        perspective: None,
        perspective_origin: (PositionComponent::Percent(0.5), PositionComponent::Percent(0.5)),
        transform_style: TransformStyle::Flat,
        backface_visibility: BackfaceVisibility::Visible,
        // CSS Lists — list-style-* наследуются.
        list_style_type: inherited.list_style_type.clone(),
        list_style_position: inherited.list_style_position,
        list_style_image: inherited.list_style_image.clone(),
        // CSS Transitions / Animations — не наследуются. Initial = empty list.
        transition_properties: Vec::new(),
        transition_durations: Vec::new(),
        transition_delays: Vec::new(),
        transition_timing_functions: Vec::new(),
        transition_fill_modes: Vec::new(),
        animation_names: Vec::new(),
        animation_durations: Vec::new(),
        animation_timing_functions: Vec::new(),
        animation_delays: Vec::new(),
        animation_iteration_counts: Vec::new(),
        animation_directions: Vec::new(),
        animation_fill_modes: Vec::new(),
        animation_play_states: Vec::new(),
        animation_timelines: Vec::new(),
        scroll_timeline_name: None,
        scroll_timeline_axis: ScrollAxis::Block,
        view_timeline_name: None,
        view_timeline_axis: ScrollAxis::Block,
        // CSS Masking — не наследуется.
        mask_layers: Vec::new(),
        // CSS Scrollbars — scrollbar-width/-color inherited;
        // scrollbar-gutter не наследуется.
        scrollbar_width: inherited.scrollbar_width,
        scrollbar_color: inherited.scrollbar_color,
        scrollbar_gutter: ScrollbarGutter::Auto,
        content: Content::Normal,
        // CSS Images L3 §5.5 — object-fit / object-position не наследуются.
        object_fit: ObjectFit::Fill,
        object_position: ObjectPosition::default(),
        // CSS 2.1 §10.8.1 — vertical-align не наследуется. Initial = baseline.
        vertical_align: VerticalAlign::Baseline,
        // CSS Images L3 §6.1 — image-rendering inherited.
        image_rendering: inherited.image_rendering,
        // CSS Flexbox L1 §5 — flex-direction / flex-wrap не наследуются.
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Nowrap,
        // CSS Flexbox L1 §7 — flex-grow / flex-shrink / flex-basis не наследуются.
        flex_grow: 0.0,
        flex_shrink: 1.0,
        flex_basis: FlexBasis::Auto,
        order: 0,
        // CSS Grid Layout L1 — grid properties не наследуются.
        grid_template_columns: Vec::new(),
        grid_template_rows: Vec::new(),
        grid_template_col_auto_repeat: None,
        grid_template_row_auto_repeat: None,
        grid_template_areas: Vec::new(),
        grid_auto_flow: GridAutoFlow::Row,
        masonry_auto_flow: MasonryAutoFlow::DefiniteFirst,
        grid_auto_columns: GridTrackSize::Auto,
        grid_auto_rows: GridTrackSize::Auto,
        grid_column_start: GridLine::Auto,
        grid_column_end: GridLine::Auto,
        grid_row_start: GridLine::Auto,
        grid_row_end: GridLine::Auto,
        // CSS Text Module Level 4 §6.4 — text-wrap-mode / text-wrap-style inherited.
        text_wrap_mode: inherited.text_wrap_mode,
        text_wrap_style: inherited.text_wrap_style,
        // CSS Overflow L4 — line-clamp не наследуется. Initial = none.
        line_clamp: None,
        // CSS Fragmentation L3 §3.3 — orphans / widows наследуются. Initial = 2.
        orphans: inherited.orphans,
        widows: inherited.widows,
        // CSS Containment L3 — не наследуются. Initial values.
        contain: ContainFlags::NONE,
        content_visibility: ContentVisibility::Visible,
        // CSS Box Sizing L4 §5 — contain-intrinsic-* are NOT inherited.
        contain_intrinsic_width: None,
        contain_intrinsic_width_auto: false,
        contain_intrinsic_height: None,
        contain_intrinsic_height_auto: false,
        // CSS Sizing L4 §4.5 — interpolate-size is inherited.
        interpolate_size: inherited.interpolate_size,
        container_type: ContainerType::Normal,
        container_name: Vec::new(),
        // CSS Filter Effects L2 — backdrop-filter не наследуется.
        backdrop_filter: Vec::new(),
        // CSS Color Adjustment L1 §5 — print-color-adjust не наследуется.
        print_color_adjust: PrintColorAdjust::Economy,
        // CSS Fonts L5 §4 — font-size-adjust inherited.
        font_size_adjust: inherited.font_size_adjust,
        // CSS Writing Modes L3 — оба inherited.
        writing_mode: inherited.writing_mode,
        text_orientation: inherited.text_orientation,
        // CSS Ruby L1 §4 — все три inherited.
        ruby_position: inherited.ruby_position,
        ruby_align: inherited.ruby_align,
        ruby_merge: inherited.ruby_merge,
        // MathML Core §2.1 — оба inherited (math-depth уже как computed integer).
        math_style: inherited.math_style,
        math_depth: inherited.math_depth,
        // CSS Shapes L1 / Motion Path — не наследуются. Initial values.
        shape_outside: ShapeOutside::None,
        shape_margin: Length::Px(0.0),
        shape_image_threshold: 0.0,
        offset_path: None,
        offset_distance: Length::Px(0.0),
        offset_rotate: OffsetRotate::Auto,
        offset_anchor: None,
        // SVG presentation attributes — all inherited per SVG spec §11.
        svg_fill: inherited.svg_fill.clone(),
        svg_fill_opacity: inherited.svg_fill_opacity,
        svg_stroke: inherited.svg_stroke.clone(),
        svg_stroke_opacity: inherited.svg_stroke_opacity,
        svg_stroke_width: inherited.svg_stroke_width,
        svg_fill_rule: inherited.svg_fill_rule,
        svg_clip_rule: inherited.svg_clip_rule,
        svg_stroke_linecap: inherited.svg_stroke_linecap,
        svg_stroke_linejoin: inherited.svg_stroke_linejoin,
        svg_stroke_miterlimit: inherited.svg_stroke_miterlimit,
        svg_stroke_dasharray: inherited.svg_stroke_dasharray.clone(),
        svg_stroke_dashoffset: inherited.svg_stroke_dashoffset,
        paint_order: inherited.paint_order,
        text_anchor: inherited.text_anchor,
        dominant_baseline: inherited.dominant_baseline,
        // SVG baseline-shift is NOT inherited — reset to initial each element.
        baseline_shift: crate::box_tree::SvgBaselineShift::Baseline,
        // CSS Logical Properties L1 — not inherited. Initial values.
        inline_size: None,
        block_size: None,
        inset_inline_start: LengthOrAuto::Auto,
        inset_inline_end: LengthOrAuto::Auto,
        inset_block_start: LengthOrAuto::Auto,
        inset_block_end: LengthOrAuto::Auto,
        margin_inline_start: LengthOrAuto::ZERO,
        margin_inline_end: LengthOrAuto::ZERO,
        margin_block_start: LengthOrAuto::ZERO,
        margin_block_end: LengthOrAuto::ZERO,
        padding_inline_start: Length::Px(0.0),
        padding_inline_end: Length::Px(0.0),
        padding_block_start: Length::Px(0.0),
        padding_block_end: Length::Px(0.0),
        border_inline_start_width: 0.0,
        border_inline_end_width: 0.0,
        border_block_start_width: 0.0,
        border_block_end_width: 0.0,
        anchor_name: None,
        position_anchor: None,
        inset_area_row: crate::anchor::InsetAreaKeyword::None,
        inset_area_col: crate::anchor::InsetAreaKeyword::None,
        anchor_scope: crate::anchor::AnchorScope::None,
        anchor_size_w: None,
        anchor_size_h: None,
        anchor_top: None,
        anchor_right: None,
        anchor_bottom: None,
        anchor_left: None,
        view_transition_name: None,
        // CSS Generated Content L3 §3.2 — quotes inherited.
        quotes: inherited.quotes.clone(),
    };

    // CSS Properties and Values L1 §1.1 — registry зарегистрированных
    // custom-properties. Карта строится локально для каждого узла:
    // на типичной странице 0..5 @property-правил, накладные расходы мизерны
    // в сравнении со стоимостью каскада. При повторе имени (см. spec —
    // last wins) `insert` корректно сохраняет последнее объявление.
    let registry: HashMap<&str, &PropertyRule> = sheet
        .properties
        .iter()
        .map(|p| (p.name.as_str(), p))
        .collect();

    // Откатываем у себя унаследованные значения тех зарегистрированных
    // custom-properties, у которых `inherits: false` — для них потомок
    // должен видеть либо локальную декларацию, либо initial-value, а не
    // родительское значение.
    //
    // BUG-341 S9: `retain` needs a `make_mut`, which copies the inherited map —
    // so first check whether any key would actually be dropped. Pages that
    // register no `inherits: false` property (or declare none of the ones they
    // do register) keep sharing the parent's allocation.
    if !registry.is_empty()
        && style
            .custom_props
            .keys()
            .any(|key| registry.get(key.as_str()).is_some_and(|p| !p.inherits))
    {
        style.custom_props.make_mut().retain(|key, _| {
            registry.get(key.as_str()).is_none_or(|p| p.inherits)
        });
    }

    if !matches!(doc.get(node).data, NodeData::Element { .. }) {
        // Для не-элементов (Document, Text внутри anonymous-wrapping) тоже
        // применяем initial-value: var(--registered) в наследуемом стиле
        // должен резолвиться через initial-value, если декларации нет.
        apply_property_initial_values(&mut style.custom_props, &registry);
        return style;
    }
    drop(prof_init);
    let prof_ua = lumen_core::profile::scope_detail("cs_ua_hints");

    // UA stylesheet: семантические элементы получают italic / bold по
    // умолчанию, CSS-декларации ниже могут это переопределить.
    if let Some(fs) = ua_font_style(doc, node) {
        style.font_style = fs;
    }
    if let Some(fw) = ua_font_weight(doc, node) {
        style.font_weight = fw;
    }
    // UA stylesheet: <pre>/<code>/<kbd>/<samp>/<tt> → font-family: monospace.
    if let Some(fam) = ua_font_family(doc, node) {
        style.font_family = fam;
    }
    // UA stylesheet: text-decoration для <del>/<s> (line-through),
    // <ins>/<u>/<a href> (underline). HTML5 §15.3.7.
    apply_ua_text_decoration(doc, node, &mut style);
    // UA stylesheet: <a href> → color: #0000ee. HTML5 §15.3.3.
    if let Some(c) = ua_link_color(doc, node) {
        style.color = c;
    }
    // UA stylesheet: <small>/<sub>/<sup> → font-size: 0.83× parent.
    // HTML5 §15.3.3. Author font-size перекроет через pre-pass.
    if let Some(factor) = ua_font_size_factor(doc, node) {
        style.font_size = inherited.font_size * factor;
    }
    // UA stylesheet: <sub>/<sup> → vertical-align. HTML5 §15.3.3.
    if let Some(va) = ua_vertical_align(doc, node) {
        style.vertical_align = va;
    }
    // UA stylesheet: <h1>–<h6> → font-size + vertical margins. HTML Rendering §15.3.3.
    // Set font-size here (before the author font-size pre-pass) so author CSS overrides it.
    apply_ua_heading_style(doc, node, inherited, &mut style);
    apply_ua_hr_style(doc, node, &mut style);
    // UA stylesheet: <body> → margin: 8px. HTML Rendering §14.3.3. Author CSS перекроет.
    apply_ua_body_margin(doc, node, &mut style);
    // UA stylesheet: form controls — display, intrinsic dimensions, border,
    // background, and foreground color. HTML5 §15.5. Author CSS поверх перекроет.
    //
    // CSS Color Adjustment L1 §2.3: тема UA-виджета определяется «used color
    // scheme» элемента, а не сырым предпочтением ОС. `color-scheme` наследуется,
    // поэтому на этапе UA-фазы (до author-каскада) берём inherited-значение —
    // оно покрывает типовой паттерн `:root { color-scheme: dark }`, спускающийся
    // к контролам. Так `color-scheme: light` форсирует светлый виджет даже в
    // OS-dark, а `dark` — тёмный в OS-light.
    //
    // CSS: system-color — P4 wires `system_color()` into the color cascade
    // (a `CssColor::System(name)` variant resolved at used-value time against
    // the element's used color scheme) for `Canvas`/`CanvasText`/`ButtonFace`/…
    // keyword support. The resolution table already lives in `system_color()`.
    let widget_dark = inherited.color_scheme.used_dark(dark_mode);
    apply_ua_form_controls(doc, node, &mut style, widget_dark);
    // UA stylesheet: <dialog> without `open` → display:none. HTML5 §15.3.9.
    apply_ua_dialog_display(doc, node, &mut style);
    // UA stylesheet: <td>/<th> → padding: 1px (HTML Rendering §15.3.8); the
    // ancestor <table cellpadding=N> overrides it. Author `padding` wins.
    apply_ua_table_cell_padding(doc, node, &mut style);
    // UA stylesheet (HTML Rendering §15.4.2): `[inert] { pointer-events: none; }`.
    // Applied during the pre-cascade UA phase so author `pointer-events` wins.
    apply_ua_inert(doc, node, &mut style);

    // CSS Quirks Mode — Quirks-only UA-rule для `<table>`: сбрасывает
    // font / color / text-align / white-space к initial-values, чтобы
    // legacy table-layout страницы (где CSS на `<body>` задавал шрифт /
    // цвет) рендерились с дефолтным шрифтом таблицы, как в IE/Netscape.
    // В Standards / LimitedQuirks не применяется.
    apply_quirks_table_reset(doc, node, &mut style);
    // CSS Quirks Mode §3.2: replaced-элементы получают line-height: 1 как UA-правило.
    apply_quirks_line_height(doc, node, &mut style);
    // CSS Quirks Mode §3.5: <html> получает height: 100vh как UA-правило,
    // чтобы body { height: 100% } резолвилось против viewport.
    apply_quirks_html_height(doc, node, &mut style);

    // HTML presentational hints (HTML5 §10): для `<img>` атрибуты
    // `width`/`height` задают начальные значения соответствующих CSS-свойств.
    // Применяются ДО CSS-каскада, поэтому любое author-CSS правило
    // перекроет атрибут даже с specificity (0,0,1). Парсятся как unitless
    // целые пиксели — это HTML5 правило для `<img>`, единицы и проценты
    // в этих атрибутах игнорируются.
    apply_image_presentational_hints(doc, node, &mut style);

    // HTML5 §15 «Rendering»: `bgcolor` на `<body>` / `<table>` / `<thead>` /
    // `<tbody>` / `<tfoot>` / `<tr>` / `<td>` / `<th>` мапается на
    // `background-color` (presentational hint). Парсится по HTML5 §2.4.6
    // «rules for parsing a legacy color value» — более лояльный алгоритм,
    // чем CSS quirks hashless hex: принимает named colors, `#rgb` / `#rrggbb`,
    // hashless hex произвольной длины и любую строку, в которой можно
    // найти хотя бы какие-то hex-digits после padding-procedure.
    apply_bgcolor_presentational_hint(doc, node, &mut style);

    // HTML LS §15.3.8 «Tables»: `background`/`bordercolor`/`cellspacing`
    // presentational hints (BUG-603 point 2) — siblings of `bgcolor` above,
    // narrower in scope (table-tree elements only, `cellspacing` table-only).
    apply_background_image_presentational_hint(doc, node, &mut style);
    apply_bordercolor_presentational_hint(doc, node, &mut style);
    apply_cellspacing_presentational_hint(doc, node, &mut style);

    // HTML5 §15.3.6 «The page»: `text` атрибут на `<body>` и `<font color>`
    // на любом элементе мапаются на CSS `color` (presentational hint).
    // Парсятся тем же legacy-парсером, что и `bgcolor`. Author CSS поверх —
    // выигрывает. `<body link/vlink/alink>` отложены: `:link` единственный
    // матчится в Phase 0, `:visited`/`:active` без runtime — no-op.
    apply_text_color_presentational_hint(doc, node, &mut style);

    // HTML5 §15.3.2: `<font size>` → font-size; `<font face>` → font-family.
    apply_font_element_presentational_hints(doc, node, &mut style);

    // HTML5 §15.3.3: `align` на блочных элементах → text-align.
    apply_align_presentational_hint(doc, node, &mut style);

    // CSS Quirks Mode §4.1 + HTML5 §14.3.9: `width`/`height` attr на
    // `<td>`/`<th>`/`<table>`. В quirks-mode width ячейки → min-width.
    apply_table_cell_width_hint(doc, node, &mut style);

    // CSS Cascade L4 §6.4.3 — inline style: парсим HTML-атрибут `style=""`
    // и кладём его декларации в отдельный буфер. Они подключаются к каскаду
    // через дополнительный sort-bit `is_inline` (ниже): внутри одного origin
    // (нормального или !important) inline всегда побеждает любой селектор —
    // это «Element-Attached Styles» тир в Cascade L4 §8.1, идущий после
    // Layer/Specificity/Order, но до Importance-инверсии.
    drop(prof_ua);
    let prof_match = lumen_core::profile::scope_detail("cs_match");
    let inline_decls: Vec<Declaration> = doc
        .get(node)
        .get_attr("style")
        .filter(|s| !s.is_empty())
        .map(parse_inline_style)
        .unwrap_or_default();

    // Собираем все matched declarations с их sort key:
    // (important, is_inline, layer_priority, specificity, rule_order, decl_index).
    //
    // `important` идёт первым: !important побеждает normal (CSS Cascade L4 §8.1).
    // `is_inline` — вторым: inline-style атрибут побеждает стилевой лист
    // (CSS Cascade L4 §6.4.3).
    // `layer_priority` — CSS Cascade L5 §6.4.5 @layer ordering:
    //   - normal: unlayered = N (highest), layer[i] = i (earlier layer = lower priority)
    //   - !important: unlayered = -N (lowest), layer[i] = -i (earlier layer = highest)
    //   Ascending sort, last applied wins → correct per spec.
    // `specificity`, `rule_idx`, `decl_idx` — обычный каскад внутри одного layer.
    let layer_n = sheet.layer_order.len() as i32;
    // Compute layer priority sign correctly for normal vs !important declarations.
    // For normal (imp=false): higher = wins → unlayered = N > layer[N-1] > ... > layer[0]
    // For !important (imp=true): lower layer_idx wins → layer[0] = 0 > layer[1] = -1 > ... > unlayered = -N
    let layer_pri = |imp: bool, layer_idx: i32| -> i32 {
        if imp { -layer_idx } else { layer_idx }
    };
    let mut matched: Vec<(bool, bool, i32, Specificity, usize, usize, &Declaration)> = Vec::new();


    // Build or reuse a per-stylesheet rule index (thread-local, keyed by
    // pointer+length). Amortised O(1): rebuilt only when the sheet changes.
    let node_data = doc.get(node);
    let node_tag = node_data.element_name().map_or("", |q| q.local.as_str());
    let node_id = node_data.get_attr("id");
    let class_attr = node_data.get_attr("class").unwrap_or("");
    let node_classes: Vec<&str> = class_attr.split_whitespace().collect();

    ensure_cascade_index(sheet, viewport, dark_mode);
    let cands = with_front_cascade_index(|idx| {
        idx.rules.candidates(node_tag, node_id, &node_classes)
    });

    for &rule_idx in &cands {
        let rule = &sheet.rules[rule_idx];
        let mut best: Option<Specificity> = None;
        for complex in &rule.selectors {
            if matches_complex(complex, doc, node) {
                let spec = complex.specificity();
                best = Some(match best {
                    Some(prev) if prev >= spec => prev,
                    _ => spec,
                });
            }
        }
        if let Some(spec) = best {
            for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                let lp = layer_pri(decl.important, layer_n);
                matched.push((decl.important, false, lp, spec, rule_idx, decl_idx, decl));
            }
        }
    }

    // CSS Cascade L5 §6.4.5 — @layer rules: каждый LayerRule добавляет
    // свои декларации в каскад с layer_priority < unlayered. Layer с меньшим
    // индексом в `layer_order` имеет меньший приоритет для normal (earlier
    // declared → overridden by later), и больший для !important (CSS Cascade
    // L5 §6.4.5 inversion: earlier layer !important wins).
    let layer_rule_base = sheet.rules.len()
        + sheet.media_rules.iter().map(|m| m.rules.len()).sum::<usize>();
    let mut layer_rule_offset = 0usize;
    for (layer_i, layer_rule) in sheet.layers.iter().enumerate() {
        let layer_idx = sheet.layer_order.iter()
            .position(|n| n == &layer_rule.name)
            .unwrap_or(0) as i32;
        // BUG-284: candidate pre-filter (was a brute-force scan of every rule
        // in the layer for every node — dominant cascade cost on stylesheets
        // that put most rules inside layers/media/supports blocks).
        let layer_cands = with_front_cascade_index(|idx| {
            idx.layers[layer_i].candidates(node_tag, node_id, &node_classes)
        });
        for rule_idx in layer_cands {
            let rule = &layer_rule.rules[rule_idx];
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let global_rule_idx = layer_rule_base + layer_rule_offset + rule_idx;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_idx);
                    matched.push((decl.important, false, lp, spec, global_rule_idx, decl_idx, decl));
                }
            }
        }
        layer_rule_offset += layer_rule.rules.len();
    }

    // CSS Media Queries L4: rules внутри `@media`-блока, чей query
    // совпадает с текущим MediaContext, добавляются в каскад. В Phase 0
    // упрощённый MediaContext: media_type="screen", width/height из
    // viewport. Source-order между обычными и
    // @media-rules не сохраняется идеально (все @media идут после
    // обычных) — это известное ограничение.
    //
    // Perf: "active" per block precomputed once per (sheet, viewport,
    // dark_mode) in `CascadeIndex::active_media` — see its doc comment.
    // `media.query.matches(..)` used to run here on every node. Fetched once
    // per node (not once per block) to avoid N thread-local accesses when
    // the stylesheet has many `@media` blocks.
    let active_media = with_front_cascade_index(|idx| idx.active_media.clone());
    let mut next_rule_idx = sheet.rules.len();
    for (media_i, media) in sheet.media_rules.iter().enumerate() {
        if !active_media[media_i] {
            next_rule_idx += media.rules.len();
            continue;
        }
        // BUG-284: candidate pre-filter (see @layer above) — real-world
        // stylesheets often put the bulk of their rules inside @media blocks.
        let media_cands = with_front_cascade_index(|idx| {
            idx.media[media_i].candidates(node_tag, node_id, &node_classes)
        });
        for rule_idx in media_cands {
            let rule = &media.rules[rule_idx];
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let global_rule_idx = next_rule_idx + rule_idx;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, global_rule_idx, decl_idx, decl));
                }
            }
        }
        next_rule_idx += media.rules.len();
    }
    // CSS Conditional Rules L3 §2 — `@supports`: evaluate condition against
    // Lumen's supported-properties list; include contained rules only when
    // condition is true (same ordering semantics as @media).
    //
    // Perf: "active" precomputed once per sheet in `CascadeIndex::active_supports`
    // (see doc comment) — `supports.condition.evaluate(..)` used to run per node.
    let active_supports = with_front_cascade_index(|idx| idx.active_supports.clone());
    for (supports_i, supports) in sheet.supports_rules.iter().enumerate() {
        if !active_supports[supports_i] {
            next_rule_idx += supports.rules.len();
            continue;
        }
        let supports_cands = with_front_cascade_index(|idx| {
            idx.supports[supports_i].candidates(node_tag, node_id, &node_classes)
        });
        for rule_idx in supports_cands {
            let rule = &supports.rules[rule_idx];
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let global_rule_idx = next_rule_idx + rule_idx;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, global_rule_idx, decl_idx, decl));
                }
            }
        }
        next_rule_idx += supports.rules.len();
    }
    // CSS Cascade L6 §5 — @scope rules: apply only when node is in scope.
    for scope_rule in &sheet.scope_rules {
        // Donut scoping (§3): `node` is in scope when it is an inclusive
        // descendant of the scope root but *not* of a scope limit that lies
        // within that same root subtree. `node_in_scope` resolves root and
        // limit together (nearest boundary wins) so a limit-matching element
        // *above* the root no longer removes the node from scope.
        if !node_in_scope(doc, node, &scope_rule.root, scope_rule.limit.as_deref()) {
            next_rule_idx += scope_rule.rules.len();
            continue;
        }
        for rule in &scope_rule.rules {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, next_rule_idx, decl_idx, decl));
                }
            }
            next_rule_idx += 1;
        }
    }
    // CSS Scoping L1 §6.1-6.2 — shadow-tree-scoped style rules. `:host`/`:host()`
    // and `::slotted()` only have effect when written *inside* a shadow tree's own
    // stylesheet (collected per-host in `SHADOW_SHEETS`); the same selectors in the
    // page's document `<style>` are no-ops. Two scopes touch this node:
    //   (a) the node is itself a shadow host → its OWN shadow sheet's `:host` rules
    //       cascade onto it (the host lives in the light tree, so only `:host`-bearing
    //       rules from its shadow reach it);
    //   (b) the node is a slotted light child → its host's shadow sheet's `::slotted()`
    //       rules cascade onto it.
    // These declarations join the same cascade as document author rules; we give them
    // `rule_idx` values past the document range so source order stays stable (shadow
    // markup follows the head `<style>` in document order).
    // Clone the relevant shadow sheets out of the thread-local into locals that
    // live for the rest of this function, so the `&Declaration` references pushed
    // into `matched` outlive the (closure-scoped) thread-local borrow.
    let any_shadow = SHADOW_SHEETS.with(|c| !c.borrow().is_empty());
    let own_shadow: Option<Stylesheet> = if any_shadow && doc.is_shadow_host(node) {
        SHADOW_SHEETS.with(|c| c.borrow().get(&node).cloned())
    } else {
        None
    };
    let host_shadow: Option<Stylesheet> = if any_shadow {
        doc.get(node)
            .parent
            .filter(|&p| doc.is_shadow_host(p))
            .and_then(|host| SHADOW_SHEETS.with(|c| c.borrow().get(&host).cloned()))
    } else {
        None
    };
    // (a) `:host` / `:host(sel)` from the node's own shadow tree apply to the host.
    if let Some(ref shadow) = own_shadow {
        SHADOW_HOST_SCOPE.with(|c| c.set(node.index() as u32));
        for (i, rule) in shadow.rules.iter().enumerate() {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if complex_has_host(complex) && matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let gidx = next_rule_idx + i;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, gidx, decl_idx, decl));
                }
            }
        }
        SHADOW_HOST_SCOPE.with(|c| c.set(u32::MAX));
    }
    // (b) `::slotted(sel)` from this node's host's shadow tree apply to the slotted child.
    if let Some(ref shadow) = host_shadow {
        let base = next_rule_idx + shadow.rules.len();
        for (i, rule) in shadow.rules.iter().enumerate() {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if let Some(spec) = matches_slotted_complex(complex, doc, node) {
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if let Some(spec) = best {
                let gidx = base + i;
                for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                    let lp = layer_pri(decl.important, layer_n);
                    matched.push((decl.important, false, lp, spec, gidx, decl_idx, decl));
                }
            }
        }
    }

    // Inline-style declarations подключаются с `is_inline = true` и
    // synthetic specificity = default (Cascade L4 §6.4.3 — реальная
    // specificity inline-стиля игнорируется в сортировке: за порядок
    // отвечает is_inline-бит, а внутри inline — источниковый порядок
    // декларации в атрибуте). Inline-стиль всегда unlayered.
    for (decl_idx, decl) in inline_decls.iter().enumerate() {
        matched.push((
            decl.important,
            true,
            layer_pri(decl.important, layer_n),
            Specificity::default(),
            next_rule_idx,
            decl_idx,
            decl,
        ));
    }
    matched.sort_by_key(|&(imp, inline, lp, spec, rule_idx, decl_idx, _)| {
        (imp, inline, lp, spec, rule_idx, decl_idx)
    });
    drop(prof_match);
    let prof_revert = lumen_core::profile::scope_detail("cs_revert_prepass");

    // CSS Cascade L5 §6.4.6 — `revert-layer`: a declaration whose value is
    // `revert-layer` rolls the cascaded value back to what it would be if all
    // declarations of that property in the *current* cascade layer (same
    // importance) were removed. CSS Cascade L5 §revert-rule-keyword (BUG-487)
    // — `revert-rule` rolls the value back to what it would be if the one
    // style rule (or the inline `style` attribute, which shares a single
    // synthetic rule index for all its declarations) that contributed the
    // winning declaration didn't exist, regardless of layer/origin/importance.
    // Both are resolved as a pre-pass over the already cascade-sorted
    // `matched` set: for every property whose winning declaration (the last
    // occurrence in sort order) is `revert-layer`/`revert-rule`, drop every
    // declaration of that property belonging to the winning layer/rule
    // respectively, then repeat. Repetition matters for two reasons: a lower
    // layer may itself contain `revert-layer`, and resolving one keyword can
    // reveal the *other* as the new winner (`revert-rule-revert-layer.html`
    // chains both), so every round rechecks for both. The normal last-wins
    // apply loop below then yields the reverted value automatically; when
    // nothing remains the property keeps its inherited/initial value.
    //
    // Neither `revert-layer` nor `revert-rule` is a `CssWideKeyword`: one
    // depends on the declaration's own layer, the other on its own rule, so
    // neither can be applied per-declaration like `inherit`/`initial`.
    // Shorthand↔longhand reverts across layers/rules are a known limitation
    // (grouping is by exact property name).
    //
    // BUG-341 S10: the loop below allocates a lowercased `String` key per
    // matched declaration plus a `HashMap` just to discover, on essentially
    // every element of every real page, that nothing declares `revert-layer`/
    // `revert-rule`. One allocation-free scan first (measured: 1.4 ms per
    // chrome layout pass, ~7% of the cascade stage).
    while matched.iter().any(|&(_, _, _, _, _, _, decl)| {
        let v = decl.value.trim();
        v.eq_ignore_ascii_case("revert-layer") || v.eq_ignore_ascii_case("revert-rule")
    }) {
        use std::collections::HashMap;
        // Winner per property = last occurrence in the cascade-sorted vec.
        // (lp, important, rule_idx, is_revert_layer, is_revert_rule)
        let mut winners: HashMap<String, (i32, bool, usize, bool, bool)> = HashMap::new();
        for &(imp, _inline, lp, _, rule_idx, _, decl) in &matched {
            let key = decl.property.to_ascii_lowercase();
            let v = decl.value.trim();
            let is_revert_layer = v.eq_ignore_ascii_case("revert-layer");
            let is_revert_rule = v.eq_ignore_ascii_case("revert-rule");
            winners.insert(key, (lp, imp, rule_idx, is_revert_layer, is_revert_rule));
        }
        let layer_targets: Vec<(String, i32, bool)> = winners
            .iter()
            .filter(|&(_, &(_, _, _, is_revert_layer, _))| is_revert_layer)
            .map(|(k, &(lp, imp, _, _, _))| (k.clone(), lp, imp))
            .collect();
        let rule_targets: Vec<(String, usize)> = winners
            .iter()
            .filter(|&(_, &(_, _, _, _, is_revert_rule))| is_revert_rule)
            .map(|(k, &(_, _, rule_idx, _, _))| (k.clone(), rule_idx))
            .collect();
        if layer_targets.is_empty() && rule_targets.is_empty() {
            break;
        }
        matched.retain(|&(imp, _inline, lp, _, rule_idx, _, decl)| {
            let key = decl.property.to_ascii_lowercase();
            let hit_layer = layer_targets
                .iter()
                .any(|(tk, tlp, timp)| *tk == key && *tlp == lp && *timp == imp);
            let hit_rule =
                rule_targets.iter().any(|(tk, tridx)| *tk == key && *tridx == rule_idx);
            !(hit_layer || hit_rule)
        });
    }

    // CSS Cascade L4 §7.4 — `revert` откатывается к значению «как если бы
    // author/user-правил не было». `style` прямо здесь уже содержит ровно
    // это: наследуемые поля скопированы из `inherited`, а все `ua_*`/
    // `apply_ua_*`/presentational-hint пассы выше (§ «UA stylesheet» /
    // «HTML presentational hints») отработали, но ни одна matched-декларация
    // ещё не применена. Снэпшот уходит в `apply_declaration` → `apply_css_wide_keyword`.
    //
    // Perf (docs/tasks/p3-cascade-perf.md Задача 1): безусловный
    // `ComputedStyle::clone()` здесь был вторым по весу вкладом в build_box
    // на тяжёлых страницах — на каждый узел клонируются десятки Vec/String/
    // HashMap-полей ради свойства, которое почти никогда не встречается в
    // реальном CSS. Клонируем, только если среди matched-деклараций реально
    // есть `revert` — прямой (`prop: revert`) или через цепочку custom
    // properties (`--x: revert; prop: var(--x);`, в т.ч. унаследованную от
    // предка — все такие декларации остаются raw-строками в
    // `custom_props`/`inherited.custom_props`, поэтому проверка ловит любую
    // глубину вложенности). Когда клон не нужен, `ua_baseline_ref` указывает
    // на `inherited` как безопасную заглушку: `apply_declaration` читает этот
    // параметр только внутри ветки `kw == Revert`, которая в этом случае
    // гарантированно не сработает ни для одной декларации.
    let ua_baseline_font_size = style.font_size;
    let needs_ua_baseline = matched.iter().any(|&(_, _, _, _, _, _, decl)| {
        decl.value.trim().eq_ignore_ascii_case("revert")
    }) || (
        matched.iter().any(|&(_, _, _, _, _, _, decl)| decl.value.contains("var("))
            && inherited.custom_props.values().any(|v| v.trim().eq_ignore_ascii_case("revert"))
    );
    let ua_baseline_storage: Option<ComputedStyle> = needs_ua_baseline.then(|| style.clone());
    let ua_baseline_ref: &ComputedStyle = ua_baseline_storage.as_ref().unwrap_or(inherited);
    drop(prof_revert);
    let prof_apply = lumen_core::profile::scope_detail("cs_apply");

    // Custom-properties pass: все `--name: value` декларации применяются
    // отдельно и ДО остальных пассов, чтобы любая обычная декларация могла
    // видеть финальное значение custom property независимо от порядка
    // объявления в source. Каскад уже соблюдён через sort `matched`:
    // последующая запись с тем же ключом перебивает раннюю.
    //
    // BUG-731: пасс стоит ПЕРЕД font-size-pre-pass, а не после него. Иначе
    // `font-size: var(--x)` / `font: var(--x)` видели бы только унаследованную
    // карту, а собственное объявление элемента (`.card { --fs: 20px;
    // font-size: var(--fs) }`) — нет. Пасс ни от чего в pre-pass-ах не зависит:
    // он читает только `matched` + `registry`, а `validate_against_syntax`
    // работает по тексту значения, не по computed font-size.
    //
    // CSS Properties and Values L1 §1.1 «invalid at computed value time»:
    // для зарегистрированных custom properties value валидируется против
    // `syntax`-дескриптора. Невалидное значение игнорируется — старое
    // значение (родительское inherited или initial-value) остаётся.
    // value, содержащее `var(`, пропускается без валидации — резолв
    // происходит позже, и итоговая строка может быть валидной.
    for (_, _, _, _, _, _, decl) in &matched {
        if let Some(name) = decl.property.strip_prefix("--") {
            let key = format!("--{name}");
            if let Some(prop_rule) = registry.get(key.as_str())
                && !decl.value.contains("var(")
                && !validate_against_syntax(&decl.value, &prop_rule.syntax)
            {
                // Invalid at computed value time — skip declaration.
                continue;
            }
            style.custom_props.make_mut().insert(key, decl.value.clone());
        }
    }

    // CSS Properties and Values L1 §1.1: для каждого зарегистрированного
    // имени, у которого после custom-pass нет значения (ни унаследованного,
    // ни локально объявленного), подставить `initial-value`. Делается до
    // остальных пассов, чтобы `var(--registered)` в обычных декларациях
    // видел initial-value-fallback.
    apply_property_initial_values(&mut style.custom_props, &registry);

    // Pre-pass: применяем font-size раньше, потому что em/% других свойств
    // считаются относительно computed font-size этого же элемента, а em для
    // самого font-size — относительно inherited (родительского) font-size.
    // Pre-pass: `zoom` (CSS Viewport L1 §5) must be known before font-size and
    // before any other length is resolved, because it multiplies all of them.
    // `matched` is cascade-sorted, so the last parseable declaration wins.
    let mut own_zoom = 1.0f32;
    for (_, _, _, _, _, _, decl) in &matched {
        if decl.property.eq_ignore_ascii_case("zoom")
            && let Some(z) = parse_zoom(&decl.value)
        {
            own_zoom = z;
        }
    }
    style.effective_zoom = inherited.effective_zoom * own_zoom;

    let parent_fs = inherited.font_size;
    let is_quirks = doc.mode() == DocumentMode::Quirks;
    // Which basis the winning font-size resolved against decides the zoom factor
    // below. No declaration applies → the value is the inherited (or UA-hinted
    // `em`) one, i.e. parent-relative.
    let mut fs_basis = FontSizeBasis::ParentRelative;
    for (_, _, _, _, _, _, decl) in &matched {
        if let Some(basis) =
            apply_font_size(&mut style, decl, parent_fs, ua_baseline_font_size, viewport, is_quirks)
        {
            fs_basis = basis;
        }
    }

    // A font-size resolved from a zoom-independent basis (`16px`, `rem`, …) has
    // not been scaled by anyone, so it takes the full compounded factor. One
    // resolved against the parent's size (`em`, `%`, or plain inheritance)
    // already carries every ancestor's zoom and needs only this element's own
    // contribution — applying `effective_zoom` to it would re-apply the
    // ancestors', once per level of nesting.
    style.font_size *= match fs_basis {
        FontSizeBasis::Absolute => style.effective_zoom,
        FontSizeBasis::ParentRelative => own_zoom,
    };

    // Pre-pass: применяем color-scheme раньше main-pass, чтобы системные
    // цвета (Canvas, ButtonFace, …) резолвились против правильной темы
    // ещё в ходе main-pass (для поля `color: Color`; CssColor-поля
    // резолвятся отдельным post-pass в конце compute_style).
    for (_, _, _, _, _, _, decl) in &matched {
        if decl.property.eq_ignore_ascii_case("color-scheme") {
            apply_declaration(&mut style, decl, parent_fs, viewport, FontWeight::NORMAL, inherited, ua_baseline_ref, is_quirks, dark_mode);
        }
    }

    // Main-pass: остальные декларации; em-basis теперь = current font_size.
    // Inherited font_weight нужен для разрешения `lighter`/`bolder`;
    // `inherited` целиком — для CSS-wide keywords (CSS Cascade L4 §7).
    let em_basis = style.font_size;
    let parent_weight = inherited.font_weight;

    // SVG 2 §6.4: presentation attributes act as author rules of the lowest
    // priority. Apply them before the matched-declaration loop so any CSS rule
    // (stylesheet or inline) overrides them.
    apply_svg_presentational_hints(
        doc, node, &mut style, em_basis, viewport, parent_weight, inherited, is_quirks,
    );

    // CSS Basic UI L4 §5 — pre-scan the cascade-winning `appearance` value
    // (matched is cascade-sorted; later = higher priority, inline included) so
    // that `appearance: none` strips UA-default border/background/padding
    // *before* the author cascade. Stripping after the cascade clobbered
    // author-specified border/background/padding (BUG-211).
    let mut appearance_none = false;
    for (_, _, _, _, _, _, decl) in &matched {
        match decl.property.as_str() {
            "appearance" | "-webkit-appearance" | "-moz-appearance" => {
                appearance_none = decl.value.trim().eq_ignore_ascii_case("none");
            }
            _ => {}
        }
    }
    if appearance_none {
        strip_ua_appearance_box_styling(doc, node, &mut style);
    }

    for (_, _, _, _, _, _, decl) in &matched {
        // CSS Cascade L5 §6.4.6 / §revert-rule-keyword: a `revert-layer`/
        // `revert-rule` declaration that survived the pre-pass was overridden
        // by a higher layer/rule for the same property, so it has no effect —
        // skip it instead of letting it fail property parsing.
        let dv = decl.value.trim();
        if dv.eq_ignore_ascii_case("revert-layer") || dv.eq_ignore_ascii_case("revert-rule") {
            continue;
        }
        // CSS Values L4 §7.7: expand attr() typed references before applying.
        let attr_buf;
        let effective_decl: &Declaration = if decl.value.contains("attr(") {
            let Some(v) = expand_attr_val(&decl.value, doc, node) else { continue };
            attr_buf = Declaration { property: decl.property.clone(), value: v, important: decl.important };
            &attr_buf
        } else {
            decl
        };
        // CSS Functions and Mixins L1: expand `--name(<args>)` custom function
        // calls before applying. `var(` is resolved first (against the same
        // `style.custom_props` `apply_declaration` would use) so a call reached
        // indirectly through a custom property (`--gap: --double(5px); width:
        // var(--gap);`) is visible to the call-site scanner, not just direct
        // calls (`width: --double(5px);`). Gated on `function_rules` being
        // non-empty — pages without `@function` pay nothing extra here, and
        // `apply_declaration`'s own `var()` pass below is then a no-op.
        let func_buf;
        let effective_decl: &Declaration = if !sheet.function_rules.is_empty()
            && effective_decl.value.contains("--")
        {
            let pre = if effective_decl.value.contains("var(") {
                match expand_vars(&effective_decl.value, &style.custom_props, 0) {
                    Some(v) => v,
                    None => continue,
                }
            } else {
                effective_decl.value.clone()
            };
            match expand_custom_functions(&pre, &sheet.function_rules, &style.custom_props, 0) {
                Some(v) => {
                    func_buf = Declaration {
                        property: effective_decl.property.clone(),
                        value: v,
                        important: effective_decl.important,
                    };
                    &func_buf
                }
                None => continue,
            }
        } else {
            effective_decl
        };
        apply_declaration(&mut style, effective_decl, em_basis, viewport, parent_weight, inherited, ua_baseline_ref, is_quirks, dark_mode);
    }

    // CSS Color 4 §6.2 — post-pass: resolve any CssColor::System variants in
    // CssColor-typed fields (border-color, background-color, etc.) now that
    // style.color_scheme is final. The `color` field (Color, not CssColor) was
    // already resolved inline in the `"color"` branch of apply_declaration.
    drop(prof_apply);
    let _prof_post = lumen_core::profile::scope_detail("cs_post");
    resolve_system_colors_in_style(&mut style, dark_mode);

    // CSS Color Adjustment L1 §3 — Forced Colors Mode: when the user preference
    // is active, override author colors with the forced system palette
    // (respecting `forced-color-adjust`). Runs after system-color resolution so
    // it sees final Rgba values and after the full cascade so it sees the final
    // `forced-color-adjust` value.
    if forced_colors_active() {
        apply_forced_colors_mode(doc, node, &mut style, dark_mode);
    }

    // CSS Overflow L3 §2.1: if one axis is `visible` and the other is not,
    // the `visible` axis becomes `auto` (both axes must agree on visibility).
    (style.overflow_x, style.overflow_y) = coerce_overflow_axes(style.overflow_x, style.overflow_y);

    // CSS Logical Properties L1 — resolve logical properties to physical.
    resolve_logical_properties(&mut style);

    // CSS Basic UI L4 §4.4 — field-sizing: content post-pass.
    // apply_ua_form_controls ran before the cascade and may have set explicit UA
    // dimensions. Now that field_sizing is final, clear width/height for text-entry
    // controls so lay_out picks up field_sizing_content_intrinsic dimensions instead.
    if style.field_sizing == FieldSizing::Content {
        apply_ua_form_controls_field_sizing_clear(doc, node, &mut style);
    }

    // CSS Fonts L4 §13 — resolve `font-palette: <dashed-ident>` against the
    // stylesheet's `@font-palette-values` rules now: paint builds the display
    // list from ComputedStyle alone and has no stylesheet access. Runs after
    // the full cascade so it sees the final `font-palette` and `font-family`.
    style.font_palette_resolved = match &style.font_palette {
        FontPalette::Custom(name) => resolve_font_palette_overrides(
            &sheet.font_palette_values,
            name,
            style.font_family.first().map(String::as_str).unwrap_or(""),
        ),
        _ => None,
    };

    apply_webkit_scrollbar_pseudos(doc, node, sheet, &mut style, viewport, dark_mode);

    // Last, so every earlier pass has already written its box-model lengths and
    // each is scaled exactly once. `font_size` was handled next to the cascade's
    // font-size pre-pass and is deliberately not re-scaled here.
    let z = style.effective_zoom;
    apply_zoom_to_lengths(&mut style, z);

    style
}
