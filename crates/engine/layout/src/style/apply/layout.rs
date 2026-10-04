//! Раскладка — ветки `match prop` функции `apply_declaration`.
//!
//! `display`, flex, grid, размеры и их логические формы, отступы,
//! многоколоночная вёрстка, разрывы, выравнивание, позиционирование и якоря,
//! `contain`/контейнерные запросы, таблицы, формы (`shape-*`).
//!
//! Перенесено батчем SPLIT-ST8 из `crates/engine/layout/src/style.rs`: тела
//! веток скопированы побайтово, изменены только пути импортов и форма выхода
//! (`return` → `return true`, см. шапку `style/apply.rs`). Метка ветки в
//! группу не входит по алфавиту, а по смыслу — порядок веток внутри `match`
//! семантики не несёт, потому что все метки уникальны.

use crate::style::{
    AlignValue, ContentSide,
    BlockStepAlign,
    BlockStepInsert,
    BlockStepRound,
    BorderCollapse,
    BorderStyle,
    BoxSizing,
    CaptionSide,
    ClearSide,
    ComputedStyle,
    ContainFlags,
    ContainerType,
    ContentVisibility,
    Display,
    EmptyCells,
    FieldSizing,
    FlexBasis,
    FlexDirection,
    FlexWrap,
    FloatSide,
    TableLayout,
    GridAutoFlow,
    GridLine,
    GridTrackSize,
    InterpolateSizeMode,
    Length,
    MasonryAutoFlow,
    ObjectFit,
    ObjectPosition,
    Position,
    Resize,
    RuleBreak,
    RuleList,
    RuleOverlap,
    RuleVisibilityItems,
    ScrollMarkerGroup,
    ScrollTargetGroup,
    ShapeOutside,
    parse_aspect_ratio_value,
    parse_auto_repeat, parse_track_line_names,
    parse_grid_template_areas,
    parse_length_q,
    parse_overflow_clip_margin,
    parse_overflow_kw,
    parse_sizing_length,
    resolve_block_step_size,
};
use crate::style::calc::looks_like_function_call;
use crate::style::parse::box_sides::{
    parse_anchor_size_func,
    parse_rule_style_opt,
    parse_break_value,
    parse_line_width,
    parse_inset_area_keyword,
    parse_margin_shorthand,
    parse_padding_shorthand,
    set_inset_side,
    set_margin_side,
    set_padding_side,
    split_box_tokens,
};
use crate::style::parse::color::parse_css_color_keep_srgb_form;
use crate::style::values::length::split_top_level_ws;
use crate::style::values::misc::RuleInsetProp;
use crate::style::shorthand::{
    apply_flex_flow_shorthand,
    apply_flex_shorthand,
    apply_grid_area_shorthand,
    apply_grid_line_shorthand,
    apply_grid_shorthand,
    apply_grid_template_shorthand,
    parse_contain_intrinsic_one,
    parse_contain_intrinsic_size,
};
use lumen_core::geom::Size;

/// Применить одну декларацию из группы «раскладка».
///
/// Возвращает `true`, если свойство принадлежит этой группе и было обработано;
/// `false` — если метка не наша и декларацию нужно предложить следующему
/// помощнику в цепочке `apply_declaration`.
#[allow(clippy::too_many_arguments)]
pub(in crate::style) fn apply_decl_layout(
    style: &mut ComputedStyle,
    prop: &str,
    val: &str,
    em_basis: f32,
    viewport: Size,
    is_quirks: bool,
) -> bool {
    match prop {
        "display" => {
            style.display = match val {
                "block" => Display::Block,
                "inline" => Display::Inline,
                "none" => Display::None,
                "flex" => Display::Flex,
                "inline-flex" => Display::InlineFlex,
                "grid" => Display::Grid,
                "inline-grid" => Display::InlineGrid,
                "inline-block" => Display::InlineBlock,
                "flow-root" => Display::FlowRoot,
                "contents" => Display::Contents,
                "table" => Display::Table,
                "inline-table" => Display::InlineTable,
                "table-row-group" => Display::TableRowGroup,
                "table-header-group" => Display::TableHeaderGroup,
                "table-footer-group" => Display::TableFooterGroup,
                "table-row" => Display::TableRow,
                "table-column-group" => Display::TableColumnGroup,
                "table-column" => Display::TableColumn,
                "table-cell" => Display::TableCell,
                "table-caption" => Display::TableCaption,
                "list-item" => Display::ListItem,
                // WHATWG Compat §2.1 — legacy `-webkit-box`/`-webkit-inline-box`
                // keep their own computed-value identity (BUG-505 срез 5,
                // `webkit_box_computed_display`, `selector_query.rs`);
                // `-webkit-flex`/`-webkit-inline-flex` are plain compat
                // aliases for the standard `flex`/`inline-flex` and get no
                // such quirk (confirmed by `webkit-box-computed.html`'s own
                // `-webkit-flex`/`-webkit-inline-flex` cases).
                "-webkit-box" => Display::WebkitBox,
                "-webkit-inline-box" => Display::WebkitInlineBox,
                "-webkit-flex" => Display::Flex,
                "-webkit-inline-flex" => Display::InlineFlex,
                _ => style.display,
            };
        }
        "object-fit" => {
            if let Some(of) = ObjectFit::parse(val) {
                style.object_fit = of;
            }
        }
        "object-position" => {
            if let Some(op) = ObjectPosition::parse(val, em_basis, viewport) {
                style.object_position = op;
            }
        }
        "flex-direction" => {
            // CSS Flexbox L1 §5.1: row | row-reverse | column | column-reverse.
            if let Some(v) = FlexDirection::parse(val) {
                style.flex_direction = v;
            }
        }
        "flex-wrap" => {
            // CSS Flexbox L1 §5.2: nowrap | wrap | wrap-reverse.
            if let Some(v) = FlexWrap::parse(val) {
                style.flex_wrap = v;
            }
        }
        "flex-flow" => {
            // CSS Flexbox L1 §5.3: shorthand flex-direction || flex-wrap.
            apply_flex_flow_shorthand(style, val);
        }
        "flex-grow" => {
            // CSS Flexbox L1 §7.1: <number> ≥ 0. Отрицательные — invalid.
            if let Ok(n) = val.trim().parse::<f32>()
                && n >= 0.0
            {
                style.flex_grow = n;
            }
        }
        "flex-shrink" => {
            // CSS Flexbox L1 §7.2: <number> ≥ 0. Отрицательные — invalid.
            if let Ok(n) = val.trim().parse::<f32>()
                && n >= 0.0
            {
                style.flex_shrink = n;
            }
        }
        "flex-basis" => {
            // CSS Flexbox L1 §7.3: auto | content | <length>.
            if let Some(v) = FlexBasis::parse(val, is_quirks) {
                style.flex_basis = v;
            }
        }
        "order" => {
            // CSS Flexbox L1 §4: <integer>. Non-inherited. Initial: 0.
            if let Ok(n) = val.trim().parse::<i32>() {
                style.order = n;
            }
        }
        "flex" => {
            // CSS Flexbox L1 §7: shorthand flex-grow flex-shrink flex-basis.
            apply_flex_shorthand(style, val, is_quirks);
        }
        // CSS Grid Layout L1 — container properties.
        "grid-template-columns" => {
            if !val.trim().eq_ignore_ascii_case("none") {
                style.grid_template_columns = GridTrackSize::parse_track_list(val, is_quirks);
                // Phase 2: capture auto-fill/auto-fit repeat metadata for layout-time expansion.
                style.grid_template_col_auto_repeat = parse_auto_repeat(val.trim());
                style.grid_template_col_line_names = parse_track_line_names(val, is_quirks);
            } else {
                style.grid_template_columns = Vec::new();
                style.grid_template_col_auto_repeat = None;
                style.grid_template_col_line_names = Vec::new();
            }
        }
        "grid-template-rows" => {
            if !val.trim().eq_ignore_ascii_case("none") {
                style.grid_template_rows = GridTrackSize::parse_track_list(val, is_quirks);
                // Phase 2: capture auto-fill/auto-fit repeat metadata for layout-time expansion.
                style.grid_template_row_auto_repeat = parse_auto_repeat(val.trim());
                style.grid_template_row_line_names = parse_track_line_names(val, is_quirks);
            } else {
                style.grid_template_rows = Vec::new();
                style.grid_template_row_auto_repeat = None;
                style.grid_template_row_line_names = Vec::new();
            }
        }
        "grid-template-areas" => {
            if val.trim().eq_ignore_ascii_case("none") {
                style.grid_template_areas = Vec::new();
            } else {
                style.grid_template_areas = parse_grid_template_areas(val);
            }
        }
        "grid-auto-columns" => {
            if let Some(ts) = GridTrackSize::parse_single(val, is_quirks) {
                style.grid_auto_columns = ts;
            }
        }
        "grid-auto-rows" => {
            if let Some(ts) = GridTrackSize::parse_single(val, is_quirks) {
                style.grid_auto_rows = ts;
            }
        }
        "grid-auto-flow" => {
            if let Some(v) = GridAutoFlow::parse(val) {
                style.grid_auto_flow = v;
            }
        }
        "masonry-auto-flow" => {
            if let Some(v) = MasonryAutoFlow::parse(val) {
                style.masonry_auto_flow = v;
            }
        }
        "grid-template" => {
            // CSS Grid L1 §7.4: невалидное значение — декларация игнорируется.
            apply_grid_template_shorthand(val, style, is_quirks);
        }
        "grid" => {
            // CSS Grid L1 §8.2: сбрасывает явную и неявную сетку целиком.
            apply_grid_shorthand(val, style, is_quirks);
        }
        // CSS Grid Layout L1 — item placement properties.
        "grid-column-start" => {
            if let Some(v) = GridLine::parse(val) {
                style.grid_column_start = v;
            }
        }
        "grid-column-end" => {
            if let Some(v) = GridLine::parse(val) {
                style.grid_column_end = v;
            }
        }
        "grid-row-start" => {
            if let Some(v) = GridLine::parse(val) {
                style.grid_row_start = v;
            }
        }
        "grid-row-end" => {
            if let Some(v) = GridLine::parse(val) {
                style.grid_row_end = v;
            }
        }
        "grid-column" => {
            // `grid-column-start / grid-column-end`
            apply_grid_line_shorthand(val, &mut style.grid_column_start, &mut style.grid_column_end);
        }
        "grid-row" => {
            // `grid-row-start / grid-row-end`
            apply_grid_line_shorthand(val, &mut style.grid_row_start, &mut style.grid_row_end);
        }
        "grid-area" => {
            // `row-start / col-start / row-end / col-end`
            apply_grid_area_shorthand(val, style);
        }
        "width" => {
            // CSS Anchor Positioning L1 §4 — intercept `anchor-size()` before normal sizing.
            if let Some(func) = parse_anchor_size_func(val, is_quirks) {
                style.anchor_size_w = Some(func);
            } else {
                // `auto` = None; intrinsic keywords = MinContent/MaxContent/FitContent.
                style.width = parse_sizing_length(val, is_quirks);
                style.anchor_size_w = None;
                // BUG-736: an authored `width` declaration always wins over
                // (and cancels) the presentational-hint marker a `width`
                // content attribute or decoded intrinsic size may have set —
                // this value is genuinely specified, not a replaced
                // element's intrinsic-size fallback.
                style.width_is_intrinsic_hint = false;
            }
        }
        "height" => {
            if let Some(func) = parse_anchor_size_func(val, is_quirks) {
                style.anchor_size_h = Some(func);
            } else {
                style.height = parse_sizing_length(val, is_quirks);
                style.anchor_size_h = None;
                // BUG-736: see the `"width"` arm above.
                style.height_is_intrinsic_hint = false;
            }
        }
        // CSS Logical Properties L1 §2.1 — inline-size / block-size.
        "inline-size" => {
            style.inline_size = parse_sizing_length(val, is_quirks);
        }
        "block-size" => {
            style.block_size = parse_sizing_length(val, is_quirks);
        }
        // CSS 2.1 §10.4: min-/max- ширина и высота. Отрицательные `<length>`
        // запрещены — сохраняем typed, фильтрация при resolve в box_tree.
        // `auto` для min-* = None (Phase 0: эквивалентно 0); `none` для
        // max-* = None (без ограничения). `%` теперь сохраняется, резолв
        // при layout с known cb_width. Intrinsic keywords accepted here too.
        "min-width" => {
            if val.trim() == "auto" {
                style.min_width = None;
            } else if let Some(len) = parse_sizing_length(val, is_quirks)
                && !matches!(&len, Length::Px(v) if *v < 0.0)
            {
                style.min_width = Some(len);
            }
        }
        "max-width" => {
            if val.trim() == "none" {
                style.max_width = None;
            } else if let Some(len) = parse_sizing_length(val, is_quirks)
                && !matches!(&len, Length::Px(v) if *v < 0.0)
            {
                style.max_width = Some(len);
            }
        }
        "min-height" => {
            if val.trim() == "auto" {
                style.min_height = None;
            } else if let Some(len) = parse_sizing_length(val, is_quirks)
                && !matches!(&len, Length::Px(v) if *v < 0.0)
            {
                style.min_height = Some(len);
            }
        }
        "max-height" => {
            if val.trim() == "none" {
                style.max_height = None;
            } else if let Some(len) = parse_sizing_length(val, is_quirks)
                && !matches!(&len, Length::Px(v) if *v < 0.0)
            {
                style.max_height = Some(len);
            }
        }
        // CSS Logical Properties L1 §2 — min/max inline-size / block-size.
        // Stored logically; `resolve_logical_properties` maps them onto
        // min/max-width/height by `writing-mode` after the cascade.
        "min-inline-size" | "min-block-size" => {
            let parsed = if val.trim() == "auto" {
                Some(None)
            } else if let Some(len) = parse_sizing_length(val, is_quirks)
                && !matches!(&len, Length::Px(v) if *v < 0.0)
            {
                Some(Some(len))
            } else {
                None
            };
            if let Some(v) = parsed {
                let sizes = style.logical_min_max_sizes.get_or_insert_with(Default::default);
                if prop == "min-inline-size" {
                    sizes.min_inline = v;
                } else {
                    sizes.min_block = v;
                }
            }
        }
        "max-inline-size" | "max-block-size" => {
            let parsed = if val.trim() == "none" {
                Some(None)
            } else if let Some(len) = parse_sizing_length(val, is_quirks)
                && !matches!(&len, Length::Px(v) if *v < 0.0)
            {
                Some(Some(len))
            } else {
                None
            };
            if let Some(v) = parsed {
                let sizes = style.logical_min_max_sizes.get_or_insert_with(Default::default);
                if prop == "max-inline-size" {
                    sizes.max_inline = v;
                } else {
                    sizes.max_block = v;
                }
            }
        }
        "overflow" => {
            // CSS Overflow L3: shorthand. Один токен — оба axis; два — x y.
            let toks: Vec<&str> = val.split_whitespace().collect();
            match toks.as_slice() {
                [a] => {
                    if let Some(o) = parse_overflow_kw(a) {
                        style.overflow_x = o;
                        style.overflow_y = o;
                    }
                }
                [a, b] => {
                    if let Some(o) = parse_overflow_kw(a) { style.overflow_x = o; }
                    if let Some(o) = parse_overflow_kw(b) { style.overflow_y = o; }
                }
                _ => {}
            }
        }
        "overflow-x" => {
            if let Some(o) = parse_overflow_kw(val.trim()) {
                style.overflow_x = o;
            }
        }
        "overflow-y" => {
            if let Some(o) = parse_overflow_kw(val.trim()) {
                style.overflow_y = o;
            }
        }
        "overflow-block" => {
            // CSS Overflow L3 §logical / BUG-505: flow-relative axis, mapped
            // to `overflow_x`/`overflow_y` post-cascade by
            // `resolve_overflow_logical_properties` (`style/logical.rs`),
            // once the final `writing-mode` is known.
            if let Some(o) = parse_overflow_kw(val.trim()) {
                style.overflow_block = o;
            }
        }
        "overflow-inline" => {
            if let Some(o) = parse_overflow_kw(val.trim()) {
                style.overflow_inline = o;
            }
        }
        "overflow-clip-margin" => {
            // CSS Overflow L3 §overflow-clip-margin (BUG-505 срез 4): full
            // `[<visual-box> || <length [0,∞]>]` grammar — was bare
            // `<length>` only (Phase 0 leftover).
            if let Some(parsed) = parse_overflow_clip_margin(val) {
                style.overflow_clip_margin = Some(parsed);
            }
        }
        "scroll-marker-group" => {
            // CSS Overflow L5 §scroll-marker-group-property (BUG-505 срез 6):
            // `none | [before|after] [tabs|links]?`.
            if let Some(parsed) = ScrollMarkerGroup::parse(val) {
                style.scroll_marker_group = parsed;
            }
        }
        "scroll-target-group" => {
            // CSS Overflow L5 §scroll-target-group: `none | auto`.
            if let Some(v) = ScrollTargetGroup::parse(val) {
                style.scroll_target_group = v;
            }
        }
        "shape-outside" => {
            style.shape_outside = match val.trim() {
                "none" => ShapeOutside::None,
                v => ShapeOutside::Value(v.to_string()),
            };
        }
        "shape-margin" => {
            if let Some(len) = parse_length_q(val, is_quirks)
                && !matches!(&len, Length::Px(v) if *v < 0.0)
            {
                style.shape_margin = len;
            }
        }
        "shape-image-threshold" => {
            if let Ok(n) = val.trim().parse::<f32>() {
                style.shape_image_threshold = n.clamp(0.0, 1.0);
            }
        }
        "row-gap" => {
            // Typed Length — % = % cb_height, резолвится при layout.
            // Отрицательные значения запрещены (CSS Multi-column §3.4).
            if let Some(len) = parse_length_q(val, is_quirks) {
                style.row_gap = if matches!(&len, Length::Px(v) if *v < 0.0) {
                    Length::Px(0.0)
                } else {
                    len
                };
            }
        }
        "column-gap" => {
            if let Some(len) = parse_length_q(val, is_quirks) {
                style.column_gap = if matches!(&len, Length::Px(v) if *v < 0.0) {
                    Length::Px(0.0)
                } else {
                    len
                };
            }
        }
        "grid-column-gap" => {
            // CSS Grid L1 §7.3: legacy alias for column-gap (deprecated).
            if let Some(len) = parse_length_q(val, is_quirks) {
                style.column_gap = if matches!(&len, Length::Px(v) if *v < 0.0) {
                    Length::Px(0.0)
                } else {
                    len
                };
            }
        }
        "grid-row-gap" => {
            // CSS Grid L1 §7.3: legacy alias for row-gap (deprecated).
            if let Some(len) = parse_length_q(val, is_quirks) {
                style.row_gap = if matches!(&len, Length::Px(v) if *v < 0.0) {
                    Length::Px(0.0)
                } else {
                    len
                };
            }
        }
        "gap" => {
            // Shorthand: `<row-gap> <column-gap>?` (если column отсутствует,
            // = row).
            let clamp_gap = |len: Length| -> Length {
                if matches!(&len, Length::Px(v) if *v < 0.0) {
                    Length::Px(0.0)
                } else {
                    len
                }
            };
            let parts: Vec<&str> = val.split_whitespace().collect();
            if !parts.is_empty()
                && let Some(row) = parse_length_q(parts[0], is_quirks)
            {
                let col = if parts.len() >= 2 {
                    parse_length_q(parts[1], is_quirks)
                } else {
                    Some(row.clone())
                };
                if let Some(c) = col {
                    style.row_gap = clamp_gap(row);
                    style.column_gap = clamp_gap(c);
                }
            }
        }
        "border-collapse" => {
            // CSS Tables L2 §17.6 — `border-collapse: separate | collapse`.
            if let Some(v) = BorderCollapse::parse(val.trim()) {
                style.border_collapse = v;
            }
        }
        "caption-side" => {
            // CSS Tables L2 §17.4.1 — `caption-side: top | bottom`.
            if let Some(v) = CaptionSide::parse(val.trim()) {
                style.caption_side = v;
            }
        }
        "table-layout" => {
            // CSS Tables L2 §17.5.2 — `table-layout: auto | fixed`.
            if let Some(v) = TableLayout::parse(val.trim()) {
                style.table_layout = v;
            }
        }
        "empty-cells" => {
            // CSS Tables L2 §17.6.1.1 — `empty-cells: show | hide`.
            if let Some(v) = EmptyCells::parse(val.trim()) {
                style.empty_cells = v;
            }
        }
        "border-spacing" => {
            // CSS 2.1 §17.6 — `border-spacing: <length> [<length>]?`.
            // One value: both h and v. Two values: h then v. Negatives invalid — skip.
            let parts: Vec<&str> = val.split_whitespace().collect();
            if let Some(first) = parts.first()
                && let Some(h) = parse_length_q(first, is_quirks)
                && let Length::Px(hpx) = h
                && hpx >= 0.0
            {
                let vpx = if parts.len() >= 2 {
                    parse_length_q(parts[1], is_quirks)
                        .and_then(|v| if let Length::Px(px) = v { Some(px) } else { None })
                        .filter(|px| *px >= 0.0)
                        .unwrap_or(hpx)
                } else {
                    hpx
                };
                style.border_spacing_h = hpx;
                style.border_spacing_v = vpx;
            }
        }
        "column-count" => {
            // CSS Multi-column L1 §3.2: <integer> | auto.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.column_count = None;
            } else if let Ok(n) = trimmed.parse::<u32>()
                && n > 0
            {
                style.column_count = Some(n);
            }
        }
        "column-width" => {
            // CSS Multi-column L1 §3.3: <length> | auto. Typed.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.column_width = None;
            } else if let Some(len) = parse_length_q(trimmed, is_quirks) {
                style.column_width = Some(len);
            }
        }
        "column-height" => {
            // CSS Multi-column L2 §4.2: auto | <length [0,∞]>.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.column_height = None;
            } else if let Some(len) = parse_length_q(trimmed, is_quirks)
                && !matches!(&len, Length::Px(v) if *v < 0.0)
            {
                style.column_height = Some(len);
            }
        }
        "column-wrap" => {
            // CSS Multi-column L2 §4.4: auto | nowrap | wrap.
            match val.trim().to_ascii_lowercase().as_str() {
                "auto" | "wrap" => style.column_wrap_nowrap = false,
                "nowrap" => style.column_wrap_nowrap = true,
                _ => {}
            }
        }
        "columns" => {
            // CSS Multi-column L1 §3.4 shorthand: <column-width> || <column-count>.
            // Любой токен может быть `auto`. Length → width, integer → count.
            // CSS Multi-column L2 §4.5: `[ <column-width> || <column-count> ] [ / <column-height> ]?`.
            let (head, height) = match val.split_once('/') {
                Some((h, t)) => (h, Some(t.trim())),
                None => (val, None),
            };
            let parts: Vec<&str> = head.split_whitespace().collect();
            let mut count: Option<u32> = None;
            let mut width: Option<Length> = None;
            let mut had_width = false;
            let mut had_count = false;
            for p in &parts {
                if p.eq_ignore_ascii_case("auto") {
                    continue;
                }
                if let Ok(n) = p.parse::<u32>()
                    && n > 0
                    && !had_count
                {
                    count = Some(n);
                    had_count = true;
                    continue;
                }
                if !had_width
                    && let Some(len) = parse_length_q(p, is_quirks)
                {
                    width = Some(len);
                    had_width = true;
                }
            }
            let height_len = match height {
                None => Some(None),
                Some(h) if h.eq_ignore_ascii_case("auto") => Some(None),
                Some(h) => parse_length_q(h, is_quirks)
                    .filter(|l| !matches!(l, Length::Px(v) if *v < 0.0))
                    .map(Some),
            };
            if (had_width || had_count) && let Some(h) = height_len {
                style.column_width = width;
                style.column_count = count;
                style.column_height = h;
            }
        }
        // CSS Multi-column L1 §4 + CSS Gap Decorations L1 §3: `column-rule*` / `row-rule*`
        // / `rule*`. `column_rule_*` — одни и те же поля для multicol и для вертикальных
        // линий flex/grid; `row_rule_*` — горизонтальные линии между строками.
        "column-rule" | "column-rule-width" | "column-rule-style" | "column-rule-color"
        | "row-rule" | "row-rule-width" | "row-rule-style" | "row-rule-color" | "rule"
        | "rule-width" | "rule-style" | "rule-color" => {
            apply_gap_rule_declaration(style, prop, val, em_basis, viewport, is_quirks);
        }
        // CSS Gap Decorations L1 §3.2 / §3.4 / §3.5: `*-rule-break`, `*-rule-visibility-items`
        // (one keyword; `rule-*` shorthand sets both axes) and `rule-overlap`.
        "column-rule-break" | "row-rule-break" | "rule-break" => {
            if let Some(v) = RuleBreak::parse(val) {
                if prop != "row-rule-break" {
                    style.column_rule_break = v;
                }
                if prop != "column-rule-break" {
                    style.row_rule_break = v;
                }
            }
        }
        "column-rule-visibility-items" | "row-rule-visibility-items" | "rule-visibility-items" => {
            if let Some(v) = RuleVisibilityItems::parse(val) {
                if prop != "row-rule-visibility-items" {
                    style.column_rule_visibility_items = v;
                }
                if prop != "column-rule-visibility-items" {
                    style.row_rule_visibility_items = v;
                }
            }
        }
        "rule-overlap" => {
            if let Some(v) = RuleOverlap::parse(val) {
                style.rule_overlap = v;
            }
        }
        // CSS Gap Decorations L1 §3.3: `*-rule-inset*` (восемь longhand-ов + шортхенды).
        p if RuleInsetProp::of(p).is_some() => {
            if let Some(rp) = RuleInsetProp::of(p)
                && let Some(values) = rp.parse(val, is_quirks)
            {
                for (slot, v) in values {
                    if rp.cols {
                        *style.column_rule_inset.slot_mut(slot) = v.clone();
                    }
                    if rp.rows {
                        *style.row_rule_inset.slot_mut(slot) = v;
                    }
                }
            }
        }
        "column-span" => {
            match val.trim().to_ascii_lowercase().as_str() {
                "all" => style.column_span_all = true,
                "none" => style.column_span_all = false,
                _ => {}
            }
        }
        "column-fill" => {
            match val.trim().to_ascii_lowercase().as_str() {
                "balance" => style.column_fill_balance = true,
                "auto" => style.column_fill_balance = false,
                _ => {}
            }
        }
        "break-before" => {
            if let Some(v) = parse_break_value(val.trim()) {
                style.break_before = v;
            }
        }
        "break-after" => {
            if let Some(v) = parse_break_value(val.trim()) {
                style.break_after = v;
            }
        }
        "break-inside" => {
            if let Some(v) = parse_break_value(val.trim()) {
                style.break_inside = v;
            }
        }
        "orphans" => {
            // CSS Fragmentation L3 §3.3: <integer> >= 1. Inherited.
            if let Ok(n) = val.trim().parse::<u32>()
                && n >= 1
            {
                style.orphans = n;
            }
        }
        "widows" => {
            // CSS Fragmentation L3 §3.3: <integer> >= 1. Inherited.
            if let Ok(n) = val.trim().parse::<u32>()
                && n >= 1
            {
                style.widows = n;
            }
        }
        "aspect-ratio" => {
            // CSS Sizing L4 §6.1: `auto | <ratio>`. <ratio> = number или
            // `W / H`. Phase 0 игнорирует `auto <ratio>` форму
            // (intrinsic + override).
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.aspect_ratio = None;
            } else if let Some(r) = parse_aspect_ratio_value(trimmed) {
                style.aspect_ratio = Some(r);
            }
        }
        // CSS Box Alignment L3 — alignment свойства. Парсятся как одно
        // значение (полная грамматика с baseline-fallback и safe/unsafe —
        // отложена).
        "align-items" => {
            if let Some((v, safe, wm)) = AlignValue::parse_with_overflow(val) {
                style.align_items = v;
                style.content_align_extra.items_safe = safe;
                style.content_align_extra.items_wm = wm;
            }
        }
        "align-self" => {
            if let Some((v, safe, wm)) = AlignValue::parse_with_overflow(val) {
                style.align_self = v;
                style.content_align_extra.self_safe = safe;
                style.content_align_extra.self_wm = wm;
            }
        }
        "align-content" => {
            if let Some((v, safe, wm)) = AlignValue::parse_with_overflow(val) {
                style.align_content = v;
                style.content_align_extra.align_safe = safe;
                style.content_align_extra.align_wm = wm;
            }
        }
        "justify-items" => {
            if let Some((v, safe, _)) = AlignValue::parse_with_overflow(val) {
                style.justify_items = v;
                style.content_align_extra.justify_items_safe = safe;
            }
        }
        "justify-self" => {
            if let Some((v, safe, _)) = AlignValue::parse_with_overflow(val) {
                style.justify_self = v;
                style.content_align_extra.justify_self_safe = safe;
            }
        }
        "justify-content" => {
            if let Some((v, safe, wm)) = AlignValue::parse_with_overflow(val) {
                style.justify_content = v;
                style.content_align_extra.justify_safe = safe;
                style.content_align_extra.justify_wm = wm;
                style.content_align_extra.justify_side = None;
            } else if let Some(side) = AlignValue::parse_content_side(val) {
                // `left`/`right`: the flex container resolves the side against
                // its axes; until then it reads as `start`/`end`.
                style.justify_content = match side {
                    ContentSide::Left => AlignValue::Start,
                    ContentSide::Right => AlignValue::End,
                };
                style.content_align_extra.justify_safe = false;
                style.content_align_extra.justify_wm = false;
                style.content_align_extra.justify_side = Some(side);
            }
        }
        // Shorthand: `place-items: <align-items> [<justify-items>]?`
        "place-items" => {
            let parts = place_tokens(val);
            if let Some(a) = parts.first().and_then(|s| AlignValue::parse(s)) {
                style.align_items = a;
                style.justify_items = parts
                    .get(1)
                    .and_then(|s| AlignValue::parse(s))
                    .unwrap_or(a);
            }
        }
        "place-self" => {
            let parts = place_tokens(val);
            if let Some(a) = parts.first().and_then(|s| AlignValue::parse(s)) {
                style.align_self = a;
                style.justify_self = parts
                    .get(1)
                    .and_then(|s| AlignValue::parse(s))
                    .unwrap_or(a);
            }
        }
        "position" => {
            if let Some(v) = Position::parse(val) {
                style.position = v;
            }
        }
        // CSS Anchor Positioning L1 §2 — anchor-name: <custom-ident> | none.
        "anchor-name" => {
            let v = val.trim();
            if v.eq_ignore_ascii_case("none") {
                style.anchor_name = None;
            } else if v.starts_with("--") {
                style.anchor_name = Some(v.into());
            }
        }
        // CSS Anchor Positioning L1 §3 — position-anchor: <custom-ident> | auto.
        "position-anchor" => {
            let v = val.trim();
            if v.eq_ignore_ascii_case("auto") || v.eq_ignore_ascii_case("none") {
                style.position_anchor = None;
            } else if v.starts_with("--") {
                style.position_anchor = Some(v.into());
            }
        }
        // CSS Anchor Positioning L1 §5 — inset-area: [<inset-area-kw>]{1,2}.
        // `position-area` is the updated spec name; both are identical.
        "inset-area" | "position-area" => {
            let toks: Vec<&str> = val.split_whitespace().collect();
            let (row_tok, col_tok) = match toks.as_slice() {
                [a] => (*a, *a),
                [a, b] => (*a, *b),
                _ => return true,
            };
            if let Some(r) = parse_inset_area_keyword(row_tok) {
                style.inset_area_row = r;
            }
            if let Some(c) = parse_inset_area_keyword(col_tok) {
                style.inset_area_col = c;
            }
        }
        // CSS Anchor Positioning L1 §2.1 — anchor-scope: none | all | <custom-ident>.
        "anchor-scope" => {
            use crate::anchor::AnchorScope;
            let v = val.trim();
            style.anchor_scope = if v.eq_ignore_ascii_case("none") {
                AnchorScope::None
            } else if v.eq_ignore_ascii_case("all") {
                AnchorScope::All
            } else if v.starts_with("--") {
                AnchorScope::Named(v.into())
            } else {
                AnchorScope::None
            };
        }
        // CSS Anchor Positioning L1 §3.1 — intercept `anchor()` before normal inset parsing.
        "top" => set_inset_side(&mut style.top, &mut style.anchor_top, val, is_quirks),
        "right" => set_inset_side(&mut style.right, &mut style.anchor_right, val, is_quirks),
        "bottom" => set_inset_side(&mut style.bottom, &mut style.anchor_bottom, val, is_quirks),
        "left" => set_inset_side(&mut style.left, &mut style.anchor_left, val, is_quirks),
        "inset" => {
            // CSS Logical Properties L1 §8.2.1 — inset shorthand (1-4 values).
            let tokens = split_box_tokens(val);
            let (t, r, b, l) = match tokens.as_slice() {
                [a] => (a, a, a, a),
                [a, b] => (a, b, b, a),
                [a, b, c] => (a, b, c, b),
                [a, b, c, d] => (a, b, c, d),
                _ => return true,
            };
            set_inset_side(&mut style.top, &mut style.anchor_top, t, is_quirks);
            set_inset_side(&mut style.right, &mut style.anchor_right, r, is_quirks);
            set_inset_side(&mut style.bottom, &mut style.anchor_bottom, b, is_quirks);
            set_inset_side(&mut style.left, &mut style.anchor_left, l, is_quirks);
        }
        // CSS Logical Properties L1 §8.2 — inset-inline-* / inset-block-*.
        // Stored in logical fields; resolved to physical (top/right/bottom/left) in resolve_logical_properties().
        "inset-inline-start" => set_margin_side(&mut style.inset_inline_start, val, is_quirks),
        "inset-inline-end"   => set_margin_side(&mut style.inset_inline_end, val, is_quirks),
        "inset-block-start"  => set_margin_side(&mut style.inset_block_start, val, is_quirks),
        "inset-block-end"    => set_margin_side(&mut style.inset_block_end, val, is_quirks),
        "inset-inline" => {
            let toks = split_box_tokens(val);
            let (s, e) = match toks.as_slice() { [a] => (a, a), [a, b] => (a, b), _ => return true };
            set_margin_side(&mut style.inset_inline_start, s, is_quirks);
            set_margin_side(&mut style.inset_inline_end, e, is_quirks);
        }
        "inset-block" => {
            let toks = split_box_tokens(val);
            let (s, e) = match toks.as_slice() { [a] => (a, a), [a, b] => (a, b), _ => return true };
            set_margin_side(&mut style.inset_block_start, s, is_quirks);
            set_margin_side(&mut style.inset_block_end, e, is_quirks);
        }
        "z-index" => {
            // CSS Positioned Layout L3 §9.3 — `auto | <integer>`.
            // `auto` → None (stacking context зависит от других триггеров);
            // целое → Some(n).
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("auto") {
                style.z_index = None;
            } else if let Ok(n) = trimmed.parse::<i32>() {
                style.z_index = Some(n);
            }
        }
        "float" => {
            if let Some(v) = FloatSide::parse(val) {
                style.float_side = v;
            }
        }
        "clear" => {
            if let Some(v) = ClearSide::parse(val) {
                style.clear = v;
            }
        }
        "field-sizing" => {
            style.field_sizing = match val.trim() {
                "content" => FieldSizing::Content,
                _ => FieldSizing::Fixed,
            };
        }
        "contain" => {
            let v = val.trim();
            style.contain = if v.eq_ignore_ascii_case("none") {
                ContainFlags::NONE
            } else if v.eq_ignore_ascii_case("strict") {
                ContainFlags::STRICT
            } else if v.eq_ignore_ascii_case("content") {
                ContainFlags::CONTENT
            } else {
                let mut f = ContainFlags::NONE;
                for kw in v.split_whitespace() {
                    match kw {
                        "size" => f.0 |= ContainFlags::SIZE.0,
                        "inline-size" => f.0 |= ContainFlags::INLINE_SIZE.0,
                        "layout" => f.0 |= ContainFlags::LAYOUT.0,
                        "style" => f.0 |= ContainFlags::STYLE.0,
                        "paint" => f.0 |= ContainFlags::PAINT.0,
                        _ => {}
                    }
                }
                f
            };
        }
        "content-visibility" => {
            style.content_visibility = match val.trim() {
                "visible" => ContentVisibility::Visible,
                "auto" => ContentVisibility::Auto,
                "hidden" => ContentVisibility::Hidden,
                _ => style.content_visibility,
            };
        }
        // CSS Box Sizing L4 §5 — contain-intrinsic-size and its longhands.
        // Each value is `auto? [ none | <length> ]`. `none` → field stays/becomes
        // `None`; the optional `auto` (last-remembered size) is accepted and
        // ignored *by layout* (we always use the length), but recorded so the
        // computed value round-trips through `getComputedStyle` (BUG-852).
        // Logical `*-block-size` / `*-inline-size` map to height / width under
        // horizontal-tb writing modes.
        "contain-intrinsic-width" | "contain-intrinsic-inline-size" => {
            if let Some((auto, v)) = parse_contain_intrinsic_one(val) {
                style.contain_intrinsic_width = v;
                style.contain_intrinsic_width_auto = auto;
            }
        }
        "contain-intrinsic-height" | "contain-intrinsic-block-size" => {
            if let Some((auto, v)) = parse_contain_intrinsic_one(val) {
                style.contain_intrinsic_height = v;
                style.contain_intrinsic_height_auto = auto;
            }
        }
        "contain-intrinsic-size" => {
            if let Some(((wa, w), (ha, h))) = parse_contain_intrinsic_size(val) {
                style.contain_intrinsic_width = w;
                style.contain_intrinsic_width_auto = wa;
                style.contain_intrinsic_height = h;
                style.contain_intrinsic_height_auto = ha;
            }
        }
        "interpolate-size" => {
            // CSS Sizing L4 §4.5 — gates keyword-size interpolation in transitions.
            style.interpolate_size = match val.trim() {
                "numeric-only" => InterpolateSizeMode::NumericOnly,
                "allow-keywords" => InterpolateSizeMode::AllowKeywords,
                _ => style.interpolate_size,
            };
        }
        "container-type" => {
            style.container_type = match val.trim() {
                "normal" => ContainerType::Normal,
                "size" => ContainerType::Size,
                "inline-size" => ContainerType::InlineSize,
                _ => style.container_type,
            };
        }
        "container-name" => {
            let v = val.trim();
            if v.eq_ignore_ascii_case("none") {
                style.container_name = Vec::new();
            } else {
                style.container_name = v
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
            }
        }
        "container" => {
            // Shorthand: <name> [ / <type> ]?
            let mut parts = val.trim().splitn(2, '/');
            let name_part = parts.next().unwrap_or("").trim();
            let type_part = parts.next().unwrap_or("normal").trim();
            if name_part.eq_ignore_ascii_case("none") {
                style.container_name = Vec::new();
            } else {
                style.container_name = name_part
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
            }
            style.container_type = match type_part {
                "size" => ContainerType::Size,
                "inline-size" => ContainerType::InlineSize,
                _ => ContainerType::Normal,
            };
        }
        "resize" => {
            style.resize = match val.trim() {
                "none" => Resize::None,
                "both" => Resize::Both,
                "horizontal" => Resize::Horizontal,
                "vertical" => Resize::Vertical,
                "block" => Resize::Block,
                "inline" => Resize::Inline,
                _ => style.resize,
            };
        }
        "place-content" => {
            let parts: Vec<&str> = val.split_whitespace().collect();
            if let Some(a) = parts.first().and_then(|s| AlignValue::parse(s)) {
                style.align_content = a;
                style.justify_content = parts
                    .get(1)
                    .and_then(|s| AlignValue::parse(s))
                    .unwrap_or(a);
            }
        }
        "margin" => {
            if let Some((t, r, b, l)) = parse_margin_shorthand(val, is_quirks) {
                style.margin_top = t;
                style.margin_right = r;
                style.margin_bottom = b;
                style.margin_left = l;
            }
        }
        "margin-top" => set_margin_side(&mut style.margin_top, val, is_quirks),
        "margin-right" => set_margin_side(&mut style.margin_right, val, is_quirks),
        "margin-bottom" => set_margin_side(&mut style.margin_bottom, val, is_quirks),
        "margin-left" => set_margin_side(&mut style.margin_left, val, is_quirks),
        // CSS Logical Properties L1 §6.1 — margin-inline-* / margin-block-*.
        // Phase 0: LTR horizontal → inline-start=left, inline-end=right,
        //          block-start=top, block-end=bottom.
        // Stored in logical fields; resolved to physical (left/right/top/bottom) in resolve_logical_properties().
        "margin-inline-start" => set_margin_side(&mut style.margin_inline_start, val, is_quirks),
        "margin-inline-end"   => set_margin_side(&mut style.margin_inline_end, val, is_quirks),
        "margin-block-start"  => set_margin_side(&mut style.margin_block_start, val, is_quirks),
        "margin-block-end"    => set_margin_side(&mut style.margin_block_end, val, is_quirks),
        "margin-inline" => {
            let toks = split_box_tokens(val);
            let (s, e) = match toks.as_slice() { [a] => (a, a), [a, b] => (a, b), _ => return true };
            set_margin_side(&mut style.margin_inline_start, s, is_quirks);
            set_margin_side(&mut style.margin_inline_end, e, is_quirks);
        }
        "margin-block" => {
            let toks = split_box_tokens(val);
            let (s, e) = match toks.as_slice() { [a] => (a, a), [a, b] => (a, b), _ => return true };
            set_margin_side(&mut style.margin_block_start, s, is_quirks);
            set_margin_side(&mut style.margin_block_end, e, is_quirks);
        }
        "padding" => {
            if let Some((t, r, b, l)) = parse_padding_shorthand(val, is_quirks) {
                style.padding_top = t;
                style.padding_right = r;
                style.padding_bottom = b;
                style.padding_left = l;
            }
        }
        "padding-top" => set_padding_side(&mut style.padding_top, val, is_quirks),
        "padding-right" => set_padding_side(&mut style.padding_right, val, is_quirks),
        "padding-bottom" => set_padding_side(&mut style.padding_bottom, val, is_quirks),
        "padding-left" => set_padding_side(&mut style.padding_left, val, is_quirks),
        // CSS Logical Properties L1 §6.2 — padding-inline-* / padding-block-*.
        // Stored in logical fields; resolved to physical (left/right/top/bottom) in resolve_logical_properties().
        "padding-inline-start" => set_padding_side(&mut style.padding_inline_start, val, is_quirks),
        "padding-inline-end"   => set_padding_side(&mut style.padding_inline_end, val, is_quirks),
        "padding-block-start"  => set_padding_side(&mut style.padding_block_start, val, is_quirks),
        "padding-block-end"    => set_padding_side(&mut style.padding_block_end, val, is_quirks),
        "padding-inline" => {
            let toks = split_box_tokens(val);
            let (s, e) = match toks.as_slice() { [a] => (a, a), [a, b] => (a, b), _ => return true };
            set_padding_side(&mut style.padding_inline_start, s, is_quirks);
            set_padding_side(&mut style.padding_inline_end, e, is_quirks);
        }
        "padding-block" => {
            let toks = split_box_tokens(val);
            let (s, e) = match toks.as_slice() { [a] => (a, a), [a, b] => (a, b), _ => return true };
            set_padding_side(&mut style.padding_block_start, s, is_quirks);
            set_padding_side(&mut style.padding_block_end, e, is_quirks);
        }
        "box-sizing" => {
            style.box_sizing = match val.trim().to_ascii_lowercase().as_str() {
                "border-box" => BoxSizing::BorderBox,
                "content-box" => BoxSizing::ContentBox,
                _ => style.box_sizing,
            };
        }
        // CSS Rhythmic Sizing L1 §3 (BUG-517): `block-step-size`/`-insert`/
        // `-align`/`-round` longhands plus the `block-step` shorthand.
        // Phase 0: parse + store only, same scope as `line-height-step`
        // (`style/apply/text.rs`) — no layout algorithm reads these fields
        // yet, the module has no evergreen browser implementation to match
        // against.
        "block-step-size" => {
            if let Some(px) = resolve_block_step_size(val, style.font_size, em_basis, viewport) {
                style.block_step_size = px;
            }
        }
        "block-step-insert" => {
            if let Some(v) = BlockStepInsert::parse(val) {
                style.block_step_insert = v;
            }
        }
        "block-step-align" => {
            if let Some(v) = BlockStepAlign::parse(val) {
                style.block_step_align = v;
            }
        }
        "block-step-round" => {
            if let Some(v) = BlockStepRound::parse(val) {
                style.block_step_round = v;
            }
        }
        "block-step" => {
            apply_block_step_shorthand(style, val, em_basis, viewport);
        }
        _ => return false,
    }
    true
}

/// Ось, к которой относится декларация `*-rule*`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RuleAxis {
    Column,
    Row,
}

/// Одна `<line-width> || <line-style> || <color>`-тройка шортхенда `*-rule`.
/// `None` у компонента — он не задан и сбрасывается в initial.
#[derive(Clone)]
struct RuleTriplet {
    width: Option<f32>,
    style: Option<BorderStyle>,
    color: Option<crate::style::CssColor>,
}

/// Токены значения `place-items`/`place-self`: слова через пробел, но
/// `first baseline` / `last baseline` — один токен (CSS Box Alignment L3 §6.1).
fn place_tokens(val: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = val.split_whitespace().peekable();
    while let Some(t) = it.next() {
        let is_baseline_prefix = t.eq_ignore_ascii_case("first") || t.eq_ignore_ascii_case("last");
        if is_baseline_prefix && it.peek().is_some_and(|n| n.eq_ignore_ascii_case("baseline")) {
            it.next();
            out.push(format!("{t} baseline"));
        } else {
            out.push(t.to_string());
        }
    }
    out
}

/// Разбор значения шортхенда `column-rule` / `row-rule` / `rule`:
/// `<line-width> || <line-style> || <color>` в любом порядке. Повтор
/// компонента или нераспознанный токен делает всю декларацию невалидной
/// (CSS Cascade L4 §7) — тогда `None`, ничего не применяется.
fn parse_rule_triplet(
    val: &str,
    em_basis: f32,
    viewport: Size,
    is_quirks: bool,
) -> Option<RuleTriplet> {
    let mut t = RuleTriplet { width: None, style: None, color: None };
    let tokens = split_top_level_ws(val);
    if tokens.is_empty() {
        return None;
    }
    for tok in tokens {
        if let Some(s) = parse_rule_style_opt(tok) {
            if t.style.replace(s).is_some() {
                return None;
            }
        } else if let Some(px) = parse_line_width(tok, em_basis, viewport, is_quirks)
            && px >= 0.0
        {
            if t.width.replace(px).is_some() {
                return None;
            }
        } else {
            let c = parse_css_color_keep_srgb_form(tok, is_quirks)?;
            if t.color.replace(c).is_some() {
                return None;
            }
        }
    }
    Some(t)
}

/// `column-rule*` / `row-rule*` / `rule*` (CSS Multi-column L1 §4, CSS Gap
/// Decorations L1 §3). `rule*` задаёт обе оси сразу. Поля `column_rule_*`
/// общие для multicol-контейнера и вертикальных линий flex/grid.
fn apply_gap_rule_declaration(
    style: &mut ComputedStyle,
    prop: &str,
    val: &str,
    em_basis: f32,
    viewport: Size,
    is_quirks: bool,
) {
    let (axes, part): (&[RuleAxis], &str) = match prop {
        "rule" => (&[RuleAxis::Column, RuleAxis::Row], ""),
        "rule-width" => (&[RuleAxis::Column, RuleAxis::Row], "width"),
        "rule-style" => (&[RuleAxis::Column, RuleAxis::Row], "style"),
        "rule-color" => (&[RuleAxis::Column, RuleAxis::Row], "color"),
        "column-rule" => (&[RuleAxis::Column], ""),
        "column-rule-width" => (&[RuleAxis::Column], "width"),
        "column-rule-style" => (&[RuleAxis::Column], "style"),
        "column-rule-color" => (&[RuleAxis::Column], "color"),
        "row-rule" => (&[RuleAxis::Row], ""),
        "row-rule-width" => (&[RuleAxis::Row], "width"),
        "row-rule-style" => (&[RuleAxis::Row], "style"),
        "row-rule-color" => (&[RuleAxis::Row], "color"),
        _ => return,
    };
    let v = val.trim();
    // `<line-width>` не бывает отрицательной, но результат `calc()`/`min()`/… зажимается
    // в 0 (CSS Values L4 §10.1), а не отбрасывает значение: `calc(10px - 0.5em)` → `0px`.
    let width_item = |t: &str| {
        let px = parse_line_width(t, em_basis, viewport, is_quirks)?;
        (px >= 0.0 || looks_like_function_call(t.trim())).then(|| px.max(0.0))
    };
    let style_item = parse_rule_style_opt;
    let color_item = |t: &str| parse_css_color_keep_srgb_form(t.trim(), is_quirks);
    // CSS Gap Decorations L1 §4.4: шортхенд — список `<gap-rule>` (с `repeat()`); каждый
    // элемент раскладывается в три списка одинаковой формы, пропущенное — initial.
    let mut width_list = None;
    let mut style_list = None;
    let mut color_list = None;
    match part {
        "" => {
            let item = |t: &str| parse_rule_triplet(t, em_basis, viewport, is_quirks);
            let Some(list) = RuleList::parse(v, &item) else { return };
            width_list = Some(list.map(|t| t.width.unwrap_or(3.0)));
            style_list = Some(list.map(|t| t.style.unwrap_or(BorderStyle::None)));
            color_list = Some(list.map(|t| t.color.unwrap_or(crate::style::CssColor::CurrentColor)));
        }
        "width" => match RuleList::parse(v, &width_item) {
            Some(l) => width_list = Some(l),
            None => return,
        },
        "style" => match RuleList::parse(v, &style_item) {
            Some(l) => style_list = Some(l),
            None => return,
        },
        _ => match RuleList::parse(v, &color_item) {
            Some(l) => color_list = Some(l),
            None => return,
        },
    }
    for axis in axes {
        let (w, st, c) = match axis {
            RuleAxis::Column => (
                &mut style.column_rule_width,
                &mut style.column_rule_style,
                &mut style.column_rule_color,
            ),
            RuleAxis::Row => (
                &mut style.row_rule_width,
                &mut style.row_rule_style,
                &mut style.row_rule_color,
            ),
        };
        if let Some(l) = &width_list {
            *w = l.clone();
        }
        if let Some(l) = &style_list {
            *st = l.clone();
        }
        if let Some(l) = &color_list {
            *c = l.clone();
        }
    }
}


/// CSS Rhythmic Sizing L1 §3.1 (BUG-517): `block-step` shorthand — `[
/// <'block-step-size'> || <'block-step-insert'> || <'block-step-align'> ||
/// <'block-step-round'> ]`. The four longhand grammars share no token
/// (`none`/`<length>` vs `margin-box`/`padding-box`/`content-box` vs
/// `auto`/`center`/`start`/`end` vs `up`/`down`/`nearest`), so each
/// whitespace-separated token unambiguously belongs to exactly one slot —
/// a duplicate slot or an unrecognized token invalidates the whole
/// declaration (CSS Cascade L4 §7: an invalid shorthand value sets nothing).
fn apply_block_step_shorthand(style: &mut ComputedStyle, val: &str, em_basis: f32, viewport: Size) {
    let tokens = split_box_tokens(val);
    if tokens.is_empty() {
        return;
    }
    let mut size: Option<Option<f32>> = None;
    let mut insert: Option<BlockStepInsert> = None;
    let mut align: Option<BlockStepAlign> = None;
    let mut round: Option<BlockStepRound> = None;
    for tok in &tokens {
        if let Some(v) = BlockStepInsert::parse(tok) {
            if insert.replace(v).is_some() {
                return;
            }
        } else if let Some(v) = BlockStepAlign::parse(tok) {
            if align.replace(v).is_some() {
                return;
            }
        } else if let Some(v) = BlockStepRound::parse(tok) {
            if round.replace(v).is_some() {
                return;
            }
        } else if let Some(px) = resolve_block_step_size(tok, style.font_size, em_basis, viewport) {
            if size.replace(px).is_some() {
                return;
            }
        } else {
            return;
        }
    }
    style.block_step_size = size.unwrap_or(None);
    style.block_step_insert = insert.unwrap_or_default();
    style.block_step_align = align.unwrap_or_default();
    style.block_step_round = round.unwrap_or_default();
}
