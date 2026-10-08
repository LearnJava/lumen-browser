//! Intrinsic (max-content/min-content/shrink-to-fit) width helpers for block
//! and flex layout — `preferred_inline_block_width`/`max_content_outer_width`/
//! `min_content_outer_width` and the flex-item-specific main-size probes
//! (`flex_item_max_main_outer`/`flex_auto_base_main_width`/
//! `flex_item_min_main_width`).
//!
//! Перенесено батчем SPLIT-BT12 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `fn contributes_to_intrinsic_width` до конца оставшегося региона,
//! перед `mod shapes_floats;`) без правок тел.

use super::*;
use super::grid::{resolve_grid_axis, GridAxis};

/// max-content advance of a text run — all segments on one line (no wrapping).
fn text_max_content(segments: &[InlineSegment], measurer: Option<&dyn TextMeasurer>) -> f32 {
    measurer.map_or(0.0, |m| {
        segments.iter().map(|seg| {
            let ls = seg.style.letter_spacing;
            let fams = &seg.style.font_family;
            let ts = super::inline_wrap::TabStops::of(&seg.style, m).unit;
            measure_text_w_families(&seg.text, seg.style.font_size, ls, ts, fams, m)
        }).sum()
    })
}

/// Writing Modes L3 §7.3.1 (FLEX-VWM-4/5): the inline-axis room (physical height)
/// of an orthogonal block `child` in a parent that gives it no definite height —
/// it shrinks to its content (bounded by the viewport) instead of filling the
/// initial containing block. `None` — `child` is not such a box (horizontal,
/// authored `height`, not a plain block, or a flex container, which sizes
/// itself from its items). `eff_w` is the base of the child's percentage margins.
pub(crate) fn orthogonal_fit_content_height(
    child: &LayoutBox,
    eff_w: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> Option<f32> {
    if matches!(child.style.writing_mode, crate::style::WritingMode::HorizontalTb)
        || child.style.height.is_some()
        || !matches!(child.kind, BoxKind::Block | BoxKind::FlowRoot)
        || matches!(child.style.display, Display::Flex | Display::InlineFlex)
    {
        return None;
    }
    // `max_content_outer_height` leaves out the box's own inline-axis margins,
    // which `build_vertical_init` takes off the room it is given.
    let cem = child.style.font_size;
    let m_v = child.style.margin_top.resolve_or_zero(cem, eff_w, viewport)
        + child.style.margin_bottom.resolve_or_zero(cem, eff_w, viewport);
    Some(viewport.height.max(0.0).min(max_content_outer_height(child, measurer, viewport) + m_v))
}

/// max-content border-box **height** of a box in a vertical writing mode — its
/// inline size (CSS Writing Modes L3 §3), the extent text advances along. The
/// mirror of [`max_content_outer_width`]: an explicit `height` wins, a text run
/// is its unwrapped advance, a block is its longest in-flow child. A child in
/// the orthogonal (horizontal) mode has no inline size along y that is known
/// without laying it out, so only its explicit `height` counts.
pub(crate) fn max_content_outer_height(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let pt = s.padding_top.resolve_or_zero(em, 0.0, viewport);
    let pb = s.padding_bottom.resolve_or_zero(em, 0.0, viewport);
    let frame = pt + pb + s.border_top_width + s.border_bottom_width;
    if let Some(h_len) = &s.height
        && !h_len.is_intrinsic()
        && let Some(h) = h_len.resolve(em, None, viewport)
    {
        return match s.box_sizing {
            BoxSizing::ContentBox => h + frame,
            BoxSizing::BorderBox => h.max(frame),
        }
        .max(0.0);
    }
    let child_outer_height = |c: &LayoutBox| {
        let cem = c.style.font_size;
        let mt = c.style.margin_top.resolve_or_zero(cem, 0.0, viewport);
        let mb = c.style.margin_bottom.resolve_or_zero(cem, 0.0, viewport);
        let ch = if matches!(c.style.writing_mode, crate::style::WritingMode::HorizontalTb)
            && !matches!(c.kind, BoxKind::InlineRun { .. })
        {
            c.style
                .height
                .as_ref()
                .and_then(|h| h.resolve(cem, None, viewport))
                .unwrap_or(0.0)
        } else {
            max_content_outer_height(c, measurer, viewport)
        };
        ch + mt + mb
    };
    let content = match &b.kind {
        BoxKind::InlineRun { segments, .. } => text_max_content(segments, measurer),
        // BUG-1263: a row of atomic inlines in a vertical mode is one unwrapped
        // column — its inline size is the **sum** of the participants.
        BoxKind::InlineBlockRow if is_vertical_mode(b) => b
            .children
            .iter()
            .filter(|c| contributes_to_intrinsic_width(c))
            .map(|c| match c.kind {
                BoxKind::InlineSpace => {
                    measurer.map_or(0.0, |m| m.char_width(' ', c.style.font_size))
                }
                _ => child_outer_height(c),
            })
            .sum(),
        // FLEX-VWM-5: a flex row in a vertical mode runs along y — its inline size is the
        // **sum** of the items' contributions plus the gaps (CSS Flexbox L1 §9.9.1).
        _ if is_vertical_mode(b)
            && matches!(b.style.display, Display::Flex | Display::InlineFlex)
            && super::flex::flex_axes(&b.style).main_vertical =>
        {
            let items: Vec<f32> = b
                .children
                .iter()
                .filter(|c| contributes_to_intrinsic_width(c))
                .map(child_outer_height)
                .collect();
            let gap = match s.flex_direction {
                FlexDirection::Column | FlexDirection::ColumnReverse => s.row_gap.resolve(em, None, viewport),
                _ => s.column_gap.resolve(em, None, viewport),
            }
            .unwrap_or(0.0)
            .max(0.0);
            items.iter().sum::<f32>() + gap * items.len().saturating_sub(1) as f32
        }
        _ => {
            let longest = || {
                b.children
                    .iter()
                    .filter(|c| contributes_to_intrinsic_width(c))
                    .map(child_outer_height)
                    .fold(0.0_f32, f32::max)
            };
            // GRID-VWM: столбцы вертикальной сетки идут по высоте — её inline-размер это
            // сумма столбцов с промежутками, а не самый длинный элемент.
            if is_vertical_mode(b) && is_grid_container(b) {
                grid_col_intrinsic_sum(b, viewport, true, &|c| max_content_outer_height(c, measurer, viewport))
                    .unwrap_or_else(longest)
            } else {
                longest()
            }
        }
    };
    let outer = content + frame;
    // `max-height`/`min-height` of a vertical box bound its inline size.
    let bound = |len: &Option<Length>| {
        len.as_ref().filter(|l| !l.is_intrinsic()).and_then(|l| l.resolve(em, None, viewport)).map(|h| match s.box_sizing {
            BoxSizing::ContentBox => h + frame,
            BoxSizing::BorderBox => h.max(frame),
        })
    };
    let outer = if is_vertical_mode(b) { outer.min(bound(&s.max_height).unwrap_or(f32::INFINITY)) } else { outer };
    let outer = if is_vertical_mode(b) { outer.max(bound(&s.min_height).unwrap_or(0.0)) } else { outer };
    outer.max(0.0)
}

/// A block container in a vertical `writing-mode` stacks its children along the
/// physical x axis (CSS Writing Modes L3 §3), so its intrinsic *width* — the
/// block size — is the **sum** of its children's, not the widest. A text run
/// contributes its line box (one column per line; intrinsic sizing assumes a
/// single line, so `used_line_height`).
fn vertical_block_extent(
    b: &LayoutBox,
    viewport: Size,
    child_outer_width: &dyn Fn(&LayoutBox) -> f32,
) -> f32 {
    b.children
        .iter()
        .filter(|c| contributes_to_intrinsic_width(c))
        .map(|c| {
            let cem = c.style.font_size;
            let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
            let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
            let w = match c.kind {
                BoxKind::InlineRun { .. } => c.used_line_height,
                _ => child_outer_width(c),
            };
            w + ml + mr
        })
        .sum()
}

fn is_vertical_mode(b: &LayoutBox) -> bool {
    !matches!(b.style.writing_mode, crate::style::WritingMode::HorizontalTb)
}

fn is_vertical_block(b: &LayoutBox) -> bool {
    !matches!(b.style.writing_mode, crate::style::WritingMode::HorizontalTb)
        && matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot)
}

/// CSS Intrinsic Sizing L3 §4.1 / CSS 2.1 §10.3.7 — does `c` contribute to its
/// parent's intrinsic (max-content / min-content / shrink-to-fit) width?
///
/// Two kinds of children do not:
/// * `display: none` (`BoxKind::Skip`) — no box is generated at all, so not even
///   the element's own padding/border may be counted;
/// * out-of-flow boxes (`position: absolute`/`fixed`) — they are sized against a
///   containing block, not against their parent's content, and are laid out
///   after it. A nav item holding a hidden 1104px-wide mega-menu dropdown must
///   still be as wide as its label (BUG-738, `tbank.ru` top navigation).
fn contributes_to_intrinsic_width(c: &LayoutBox) -> bool {
    !matches!(c.kind, BoxKind::Skip)
        && !matches!(c.style.position, Position::Absolute | Position::Fixed)
}

/// Is `b` a **row-direction** flex container (`display: flex`/`inline-flex`
/// with `flex-direction: row`/`row-reverse`)?
///
/// Only the row axis matters for intrinsic *width*: a column flex container
/// stacks its items vertically, exactly like a block container, so the existing
/// "widest child" rule is already right for it.
fn is_row_flex_container(b: &LayoutBox) -> bool {
    matches!(b.style.display, Display::Flex | Display::InlineFlex)
        && !matches!(
            b.style.flex_direction,
            FlexDirection::Column | FlexDirection::ColumnReverse
        )
}

/// Is `b` a grid container (`display: grid`/`inline-grid`)?
fn is_grid_container(b: &LayoutBox) -> bool {
    matches!(b.style.display, Display::Grid | Display::InlineGrid)
}

/// Does an item's column placement stay inside the explicit columns of `axis`
/// (so it cannot create an implicit column)? Lines are resolved by the very
/// routine placement uses (`resolve_grid_axis`), so numbers, `span`, line names
/// and `grid-area` names all agree with what `lay_out_grid` will do.
fn column_placement_in_explicit_grid(s: &ComputedStyle, axis: &GridAxis) -> bool {
    let (start, end) = resolve_grid_axis(&s.grid_column_start, &s.grid_column_end, axis);
    if start == 0 {
        // Auto position: `end` carries the span (0 — one column).
        end <= axis.n_tracks
    } else {
        end <= axis.last_line()
    }
}

/// CSS Grid L1 §11.5 — intrinsic width contribution of a grid container:
/// the sum of its columns' intrinsic widths plus `column-gap`, not the widest
/// child (BUG-740).
///
/// A fully spec-correct answer needs the real placement + track-sizing
/// algorithm (`build_grid_init`) run against an infinite available width, so
/// this covers only the case the bug's own "direction" section calls out as
/// safe: an explicit, non-auto-repeat column template, row-flow placement,
/// and no item overriding its own grid placement (which could send it to a
/// column other than the round-robin one assumed here, or make columns
/// overlap). `None` tells the caller to fall back to the pre-existing
/// "widest child" rule instead of reporting a confidently wrong number.
///
/// `vertical` — сетка в вертикальном `writing-mode` (GRID-VWM): столбцы идут по
/// физической высоте, поэтому поля элемента по inline-оси — верхнее и нижнее.
pub(super) fn grid_col_intrinsic_sum(
    b: &LayoutBox,
    viewport: Size,
    vertical: bool,
    per_item: &dyn Fn(&LayoutBox) -> f32,
) -> Option<f32> {
    let s = &b.style;
    if s.grid_template_col_auto_repeat.is_some() {
        return None;
    }
    // `grid-auto-flow: column` — items fill columns, not rows: the round-robin rule below
    // does not apply and only the placement pass (`grid_col_sum_by_tracks`) can size it.
    let column_flow = matches!(s.grid_auto_flow, GridAutoFlow::Column | GridAutoFlow::ColumnDense);
    if column_flow && vertical {
        return None;
    }
    let template = &s.grid_template_columns;
    let n_cols = template.len();
    // A container with no items (empty, or only out-of-flow children) is as
    // wide as its fixed-length columns plus the gaps between them (Grid L1
    // §7.1); any other track type has nothing to size it, so the caller's
    // fallback applies.
    let all_fixed = template.iter().all(|t| matches!(t, GridTrackSize::Length(_)));
    // `grid-template-areas` wider than the template adds auto columns that this
    // fixed-length sum knows nothing about.
    let areas_cols = s.grid_template_areas.first().map_or(0, Vec::len);
    let col_axis = GridAxis {
        n_tracks: n_cols as u32,
        names: &s.grid_template_col_line_names,
        areas: &s.grid_template_areas,
        is_col: true,
        clamp: false,
    };
    let items_in_grid = areas_cols <= n_cols
        && b.children
            .iter()
            .filter(|c| contributes_to_intrinsic_width(c))
            .all(|c| column_placement_in_explicit_grid(&c.style, &col_axis));
    // Fixed-length columns do not depend on their items at all, so the same sum
    // holds for a container whose items are placed explicitly, as long as no
    // item reaches past the explicit grid (that would add an implicit column).
    if n_cols >= 1 && all_fixed && items_in_grid {
        let gap = s.column_gap.resolve(s.font_size, Some(0.0), viewport).unwrap_or(0.0).max(0.0);
        let mut sum = gap * (n_cols - 1) as f32;
        for t in template {
            let GridTrackSize::Length(l) = t else { return None };
            sum += l.resolve(s.font_size, None, viewport)?.max(0.0);
        }
        return Some(sum);
    }
    if !vertical
        && let Some(sum) = grid_col_sum_by_tracks(b, viewport, &col_axis, per_item)
    {
        return Some(sum);
    }
    if column_flow {
        return None;
    }
    if n_cols >= 1 && !b.children.iter().any(contributes_to_intrinsic_width) {
        return None;
    }
    if n_cols <= 1 || matches!(template.first(), Some(GridTrackSize::Subgrid) | Some(GridTrackSize::Masonry)) {
        return None;
    }

    let mut items: Vec<&LayoutBox> = b.children.iter().filter(|c| contributes_to_intrinsic_width(c)).collect();
    if items.is_empty() {
        return None;
    }
    let auto_placed = |line: &GridLine| matches!(line, GridLine::Auto);
    if items.iter().any(|c| {
        !auto_placed(&c.style.grid_column_start)
            || !auto_placed(&c.style.grid_column_end)
            || !auto_placed(&c.style.grid_row_start)
            || !auto_placed(&c.style.grid_row_end)
    }) {
        return None;
    }

    // CSS Grid §6 — modified document order (source order reordered by `order`),
    // same as `build_grid_init`'s auto-placement pass.
    items.sort_by_key(|c| c.style.order);

    let gap = s.column_gap.resolve(s.font_size, Some(0.0), viewport).unwrap_or(0.0).max(0.0);

    let mut col_widths = vec![0.0_f32; n_cols];
    for (k, c) in items.iter().enumerate() {
        let col = k % n_cols;
        let cem = c.style.font_size;
        let (ml, mr) = if vertical {
            (c.style.margin_top.resolve_or_zero(cem, 0.0, viewport), c.style.margin_bottom.resolve_or_zero(cem, 0.0, viewport))
        } else {
            (c.style.margin_left.resolve_or_zero(cem, 0.0, viewport), c.style.margin_right.resolve_or_zero(cem, 0.0, viewport))
        };
        col_widths[col] = col_widths[col].max(per_item(c) + ml + mr);
    }

    Some(col_widths.iter().sum::<f32>() + gap * (n_cols - 1) as f32)
}

/// CSS Grid L1 §11.5 / L2 §9 — the column sum of a grid whose items are placed
/// explicitly or through a subgrid (BUG-1318): the placement and track-sizing
/// passes `build_grid_init` runs, against an infinite width. `None` when the
/// grid is not of that kind (every item auto-placed and none a subgrid — the
/// round-robin rule in `grid_col_intrinsic_sum` covers it), when a track is not
/// `auto` or a definite length, or when the axis is not plain row flow.
fn grid_col_sum_by_tracks(
    b: &LayoutBox,
    viewport: Size,
    col_axis: &GridAxis,
    per_item: &dyn Fn(&LayoutBox) -> f32,
) -> Option<f32> {
    use super::grid::{grid_item_indices, place_grid_items};
    use super::grid_auto_cols as gac;
    let s = &b.style;
    let template = &s.grid_template_columns;
    if matches!(template.first(), Some(GridTrackSize::Subgrid) | Some(GridTrackSize::Masonry)) {
        return None;
    }
    let item_idxs = grid_item_indices(&b.children);
    let column_flow = matches!(s.grid_auto_flow, GridAutoFlow::Column | GridAutoFlow::ColumnDense);
    // `minmax(auto, <length>)` (BUG-1313): the track is as wide as its items' minimum
    // contribution when that exceeds the length, which only the track pass knows.
    let has_bounded_track = template.iter().any(|t| {
        matches!(t, GridTrackSize::Minmax(min, max)
            if matches!(**min, GridTrackSize::Auto) && matches!(**max, GridTrackSize::Length(_)))
    });
    let needs_tracks = column_flow
        || has_bounded_track
        || item_idxs.iter().any(|&i| {
            let st = &b.children[i].style;
            gac::is_col_subgrid(&b.children[i])
                || !matches!(st.grid_column_start, GridLine::Auto)
                || !matches!(st.grid_column_end, GridLine::Auto)
        });
    if !needs_tracks {
        return None;
    }
    let row_axis = GridAxis {
        n_tracks: s.grid_template_rows.len().max(s.grid_template_areas.len()) as u32,
        names: &s.grid_template_row_line_names,
        areas: &s.grid_template_areas,
        is_col: false,
        clamp: false,
    };
    let n_explicit = template.len().max(1);
    let placements = place_grid_items(
        &b.children,
        &item_idxs,
        s,
        n_explicit,
        s.grid_template_rows.len(),
        col_axis,
        &row_axis,
    );
    let n_cols = placements
        .iter()
        .map(|&(_, ce, _, _)| ce.saturating_sub(1) as usize)
        .max()
        .unwrap_or(1)
        .max(n_explicit);
    let kinds = gac::classify_col_tracks(template, &s.grid_auto_columns, n_cols, &|l| {
        l.resolve(s.font_size, None, viewport)
    })?;
    let gap = s.column_gap.resolve(s.font_size, Some(0.0), viewport).unwrap_or(0.0).max(0.0);
    let mut contribs = Vec::new();
    let measure = |c: &LayoutBox| {
        let w = per_item(c);
        (w, w)
    };
    gac::collect_col_contributions(
        &b.children, &item_idxs, &placements, &s.grid_template_col_line_names, n_cols, 0, 0.0, 0.0, viewport,
        &measure, &mut contribs,
    );
    let (_, limit) = gac::base_and_limit(&kinds, &contribs, gap);
    Some(limit.iter().sum::<f32>() + gap * (n_cols - 1) as f32)
}

/// CSS Flexbox L1 §9.9 — intrinsic width contribution of a **row-direction**
/// flex container: its items sit side by side on the main axis, so the
/// container's intrinsic width is the *sum* of the items' outer (margin-box)
/// intrinsic widths plus the `column-gap` between them — not the maximum, which
/// is what the block-container rule (children stack vertically) yields.
///
/// `per_item` supplies the caller's own notion of an item's border-box
/// intrinsic width (max-content, min-content or shrink-to-fit preferred);
/// margins and gaps are added here so every caller agrees on them.
///
/// Same class of defect as [BUG-178] for floats: a formatting context whose
/// children are laid out horizontally was being measured with the vertical rule.
///
/// Item selection mirrors `lay_out_flex`: `Skip` boxes and absolutely-positioned
/// children are not flex items (§4.1) and contribute nothing.
///
/// Percentage `column-gap` resolves against the container's own content box,
/// which is exactly what intrinsic sizing does not know yet — it resolves to
/// zero here, consistent with every other percentage in these functions.
fn flex_row_intrinsic_sum(
    b: &LayoutBox,
    viewport: Size,
    per_item: &dyn Fn(&LayoutBox) -> f32,
) -> f32 {
    let gap = b
        .style
        .column_gap
        .resolve(b.style.font_size, Some(0.0), viewport)
        .unwrap_or(0.0)
        .max(0.0);
    let mut sum = 0.0_f32;
    let mut n_items = 0_usize;
    for c in &b.children {
        if !contributes_to_intrinsic_width(c) {
            continue;
        }
        let cem = c.style.font_size;
        let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
        let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
        sum += per_item(c) + ml + mr;
        n_items += 1;
    }
    sum + gap * n_items.saturating_sub(1) as f32
}

/// Phase 0 shrink-to-fit: возвращает «предпочтительную» ширину inline-block-бокса
/// (включая padding+border самого бокса). Алгоритм: если у бокса явная CSS `width` —
/// берём её; иначе рекурсивно ищем максимальную preferred_width среди потомков
/// и добавляем padding+border текущего бокса. Возвращает `None` если явных размеров
/// нет ни у бокса, ни у его потомков.
///
/// Для typed-Length полей используем em = font_size, cb_width = 0 как
/// аппроксимацию (shrink-to-fit не знает cb_width заранее).
pub(crate) fn preferred_inline_block_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> Option<f32> {
    let s = &b.style;
    let em = s.font_size;
    // % ширины на этом этапе не разрешима — трактуем как отсутствие.
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    // CSS Sizing L3 §5.2.1 (BUG-742): процентная `width` в intrinsic-контексте
    // неразрешима и ведёт себя как `auto` — вклад считается по содержимому.
    // `percent_basis: None` (а не `Some(0.0)`) — единственное отличие от
    // остальных длин: иначе `width: 100%` давала бы 0 и целиком стирала вклад
    // поддерева, оставляя от бокса только его собственные padding + border.
    if let Some(w_len) = &s.width
        && let Some(w) = w_len.resolve(em, None, viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr
                + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return Some(outer.max(0.0));
    }
    // InlineRun — чисто-текстовый анонимный run: preferred = max-content ширина
    // текста (все сегменты на одной строке, без переноса). Без этой ветки
    // text-only inline-block (`<span style="display:inline-block">текст</span>`)
    // получал content_w = 0 (текст лежит в `segments`, а не в `children`) → None
    // → shrink-to-fit не применялся → бокс растягивался на всю доступную ширину
    // вместо обтягивания текста (BUG-202).
    if let BoxKind::InlineRun { segments, .. } = &b.kind {
        let text_w = measurer.map_or(0.0, |m| {
            segments
                .iter()
                .map(|seg| {
                    let ls = seg.style.letter_spacing;
                    let fams = &seg.style.font_family;
                    let ts = super::inline_wrap::TabStops::of(&seg.style, m).unit;
                    measure_text_w_families(&seg.text, seg.style.font_size, ls, ts, fams, m)
                })
                .sum()
        });
        return if text_w > 0.0 { Some(text_w) } else { None };
    }
    // InlineBlockRow — горизонтальный поток: суммируем ширины детей + их margins.
    // InlineSpace — collapsed whitespace gap; его ширина = char_width(' ').
    // Остальные боксы (Block, Image и т.д.) — вертикальный поток: берём max.
    let block_flow = || {
        // Vertical (block) flow: in-flow children stack, so the container is as
        // wide as its widest child. Floated children, however, are placed side
        // by side on the same line (CSS 2.1 §9.5.1) — their margin-box widths
        // sum. The shrink-to-fit width is the larger of the two contributions.
        let mut inflow_max = 0.0_f32;
        let mut float_sum = 0.0_f32;
        for c in &b.children {
            if !contributes_to_intrinsic_width(c) {
                continue;
            }
            let Some(cw) = preferred_inline_block_width(c, measurer, viewport) else {
                continue;
            };
            if c.style.float_side != FloatSide::None {
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                float_sum += cw + ml.max(0.0) + mr.max(0.0);
            } else {
                inflow_max = inflow_max.max(cw);
            }
        }
        inflow_max.max(float_sum)
    };
    let content_w = if is_row_flex_container(b) {
        // Row flex container: items are laid side by side (see
        // `flex_row_intrinsic_sum`). A child with no preference of its own
        // contributes 0, matching the `unwrap_or(0.0)` used for the other
        // horizontal flow below.
        flex_row_intrinsic_sum(b, viewport, &|c| {
            preferred_inline_block_width(c, measurer, viewport).unwrap_or(0.0)
        })
    } else if matches!(b.kind, BoxKind::InlineBlockRow) && is_vertical_mode(b) {
        // BUG-1263: в вертикальном режиме ряд течёт вниз по inline-оси, так что
        // по ширине (block-оси) он — одна колонка: самый широкий участник.
        b.children
            .iter()
            .filter(|c| contributes_to_intrinsic_width(c) && !matches!(c.kind, BoxKind::InlineSpace))
            .map(|c| {
                let cw = match c.kind {
                    BoxKind::InlineRun { .. } => c.used_line_height,
                    _ => preferred_inline_block_width(c, measurer, viewport).unwrap_or(0.0),
                };
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                cw + ml + mr
            })
            .fold(0.0_f32, f32::max)
    } else if matches!(b.kind, BoxKind::InlineBlockRow) {
        let sum: f32 = b.children.iter().filter(|c| contributes_to_intrinsic_width(c)).map(|c| {
            if matches!(c.kind, BoxKind::InlineSpace) {
                // Учитываем ширину collapsed space, чтобы при shrink-to-fit
                // не занижать ширину контейнера и не вызывать перенос соседних
                // inline-block элементов на следующую строку.
                return measurer.map_or(0.0, |m| m.char_width(' ', c.style.font_size));
            }
            let cw = preferred_inline_block_width(c, measurer, viewport).unwrap_or(0.0);
            let cem = c.style.font_size;
            let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
            let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
            cw + ml + mr
        }).sum();
        sum
    } else if is_grid_container(b) {
        grid_col_intrinsic_sum(b, viewport, false, &|c| {
            preferred_inline_block_width(c, measurer, viewport).unwrap_or(0.0)
        })
        .unwrap_or_else(block_flow)
    } else {
        block_flow()
    };
    if content_w > 0.0 {
        Some(
            (content_w + pl + pr
                + s.border_left_width + s.border_right_width)
                .max(0.0),
        )
    } else {
        None
    }
}

/// CSS Intrinsic Sizing L3 §4 — max-content border-box width of `b`.
///
/// The max-content width is the width a box would use if line breaking were
/// suppressed: all content on one line. For block containers this is the
/// maximum over children's max-content widths. For `InlineRun` boxes it is
/// the sum of all segment text widths (no wrapping). Includes the box's own
/// padding + border in the returned value (border-box width).
///
/// Phase-0 approximation: only `char_width` per-character measurement is
/// available; inter-word spacing is included, but features like ligatures or
/// kerning are not. Word-break is not applied — text is treated as one run.
pub(crate) fn max_content_outer_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    // Explicit non-intrinsic CSS width takes precedence (same logic as
    // preferred_inline_block_width). A percentage width is *not* explicit here:
    // it is unresolvable in an intrinsic context and behaves as `auto`
    // (CSS Sizing L3 §5.2.1, BUG-742) — hence `percent_basis: None`.
    if let Some(w_len) = &s.width
        && !w_len.is_intrinsic()
        && let Some(w) = w_len.resolve(em, None, viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return outer.max(0.0);
    }
    let block_flow = || {
        // Block container: in-flow children stack vertically → take the
        // widest. Floated children are laid side by side on one line
        // (CSS 2.1 §9.5.1), so their margin-box widths sum. The max-content
        // width is the larger of the in-flow maximum and the float run sum.
        let mut inflow_max = 0.0_f32;
        let mut float_sum = 0.0_f32;
        for c in &b.children {
            if !contributes_to_intrinsic_width(c) {
                continue;
            }
            let cw = max_content_outer_width(c, measurer, viewport);
            if c.style.float_side != FloatSide::None {
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                float_sum += cw + ml.max(0.0) + mr.max(0.0);
            } else {
                inflow_max = inflow_max.max(cw);
            }
        }
        inflow_max.max(float_sum)
    };
    let content_w = match &b.kind {
        BoxKind::InlineRun { segments, .. } => text_max_content(segments, measurer),
        BoxKind::InlineBlockRow => {
            b.children.iter().filter(|c| contributes_to_intrinsic_width(c)).map(|c| {
                if matches!(c.kind, BoxKind::InlineSpace) {
                    return measurer.map_or(0.0, |m| m.char_width(' ', c.style.font_size));
                }
                let cw = max_content_outer_width(c, measurer, viewport);
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                cw + ml + mr
            }).sum()
        }
        // Row flex container: items sit side by side, so max-content is their
        // sum + gaps (CSS Flexbox §9.9). This holds for `flex-wrap: wrap` too —
        // max-content suppresses line breaking, so every item stays on one line.
        _ if is_row_flex_container(b) => {
            flex_row_intrinsic_sum(b, viewport, &|c| {
                max_content_outer_width(c, measurer, viewport)
            })
        }
        // Grid container: max-content is the sum of column max-content widths
        // + gaps (CSS Grid L1 §11.5), not the widest item — see BUG-740.
        _ if is_grid_container(b) => {
            grid_col_intrinsic_sum(b, viewport, false, &|c| max_content_outer_width(c, measurer, viewport))
                .unwrap_or_else(block_flow)
        }
        _ if is_vertical_block(b) => {
            vertical_block_extent(b, viewport, &|c| max_content_outer_width(c, measurer, viewport))
        }
        _ => block_flow(),
    };
    (content_w + pl + pr + s.border_left_width + s.border_right_width).max(0.0)
}

/// CSS Intrinsic Sizing L3 §4 — min-content border-box width of `b`.
///
/// The min-content width is the narrowest a box can be without overflowing:
/// the width of the longest unbreakable content unit (word, image, etc.).
///
/// Phase-0 approximation: computes the max word width per `InlineRun` by
/// splitting on ASCII whitespace. This gives correct results for Latin text
/// but may overestimate for languages without whitespace-based word breaks.
pub(crate) fn min_content_outer_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    // Percentage width behaves as `auto` here — see [`max_content_outer_width`]
    // (CSS Sizing L3 §5.2.1, BUG-742).
    if let Some(w_len) = &s.width
        && !w_len.is_intrinsic()
        && let Some(w) = w_len.resolve(em, None, viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return outer.max(0.0);
    }
    min_content_outer_width_of_contents(b, measurer, viewport)
}

/// Same as [`min_content_outer_width`] but ignoring `b`'s own definite `width`:
/// the min-content width the box would have if it were sized by its contents.
///
/// This is the CSS Flexbox §4.5 *content size suggestion*, which is deliberately
/// intrinsic — a flex item with `width: 300px` whose contents can collapse to
/// nothing still has a content size suggestion of 0, and so may be shrunk below
/// its preferred width. Descendants keep their own explicit widths; only the
/// box's own preferred size is bypassed.
pub(crate) fn min_content_outer_width_of_contents(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    let content_w = match &b.kind {
        BoxKind::InlineRun { segments, .. } => {
            // min-content = widest unbreakable stretch of text.
            //
            // A space is a soft-wrap opportunity only where the segment's own
            // `white-space`/`text-wrap-mode` permits wrapping. Under `nowrap`
            // (and `pre`) there are none, so the stretch runs to the end of the
            // segment — and on to the next segment, since nothing between two
            // adjacent non-wrapping segments can break either. Splitting such
            // text on spaces anyway reported the widest *word* as the whole
            // run's minimum, which is what let a row of `white-space: nowrap`
            // flex items shrink far below their text and paint over each other
            // (BUG-427, dzen.ru topic tabs: "Москва — город будущего" claimed
            // the width of "будущего").
            //
            // `pre` still breaks at preserved newlines, so its stretches are the
            // segment's `\n`-separated lines rather than the whole segment.
            measurer.map_or(0.0, |m| {
                let mut best = 0.0_f32;
                let mut run = 0.0_f32;
                for seg in segments {
                    let ls = seg.style.letter_spacing;
                    let fams = &seg.style.font_family;
                    let fs = seg.style.font_size;
                    let ts = super::inline_wrap::TabStops::of(&seg.style, m).unit;
                    let piece =
                        |t: &str| measure_text_w_families(t, fs, ls, ts, fams, m);
                    let no_wrap = seg.style.white_space.is_nowrap()
                        || seg.style.text_wrap_mode == TextWrapMode::Nowrap;
                    if no_wrap {
                        let mut lines = seg.text.split('\n');
                        // The first line continues the stretch built so far.
                        if let Some(first) = lines.next() {
                            run += piece(first);
                            best = best.max(run);
                        }
                        for line in lines {
                            run = piece(line);
                            best = best.max(run);
                        }
                    } else {
                        // Wrappable: every space is a break opportunity, so the
                        // longest word bounds the minimum (a leading word could
                        // extend the previous stretch — deliberately not modelled,
                        // as before).
                        run = 0.0;
                        for word in split_css_whitespace(&seg.text) {
                            best = best.max(piece(word));
                        }
                    }
                }
                best
            })
        }
        BoxKind::InlineBlockRow => {
            // For inline-block row, min-content is the max over children.
            b.children.iter().filter(|c| contributes_to_intrinsic_width(c)).map(|c| {
                if matches!(c.kind, BoxKind::InlineSpace) {
                    return 0.0; // spaces are breakable
                }
                let cw = min_content_outer_width(c, measurer, viewport);
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                cw + ml + mr
            }).fold(0.0_f32, f32::max)
        }
        // Row flex container with `flex-wrap: nowrap`: the items cannot be
        // pushed onto separate lines, so the narrowest the container can get is
        // the sum of its items' min-content widths + gaps (CSS Flexbox §9.9).
        // With `wrap` the items *can* break onto their own lines, so the
        // min-content width is the widest single item — the block rule below.
        _ if is_row_flex_container(b) && matches!(b.style.flex_wrap, FlexWrap::Nowrap) => {
            flex_row_intrinsic_sum(b, viewport, &|c| {
                min_content_outer_width(c, measurer, viewport)
            })
        }
        // Grid container: columns can't share space, so min-content is the sum
        // of column min-content widths + gaps, not the widest item (BUG-740).
        _ if is_grid_container(b) => {
            grid_col_intrinsic_sum(b, viewport, false, &|c| min_content_outer_width(c, measurer, viewport))
                .unwrap_or_else(|| {
                    b.children.iter()
                        .filter(|c| contributes_to_intrinsic_width(c))
                        .map(|c| min_content_outer_width(c, measurer, viewport))
                        .fold(0.0_f32, f32::max)
                })
        }
        _ if is_vertical_block(b) => {
            vertical_block_extent(b, viewport, &|c| min_content_outer_width(c, measurer, viewport))
        }
        _ => {
            b.children.iter()
                .filter(|c| contributes_to_intrinsic_width(c))
                .map(|c| min_content_outer_width(c, measurer, viewport))
                .fold(0.0_f32, f32::max)
        }
    };
    (content_w + pl + pr + s.border_left_width + s.border_right_width).max(0.0)
}

/// CSS Flexbox L1 §9.2/§9.7 — flex base size (main-axis, **border-box**) of a
/// row-direction flex item whose `flex-basis` is `auto`/`content` and which has
/// no explicit `width`. This is the item's max-content width clamped by its own
/// `min-width` / `max-width`. Margins are excluded (the caller adds them).
/// `cb` is the flex container's inner main size, used to resolve percentage
/// min/max-width. Replaces the old approximation that fell back to the
/// preliminary-pass stretched `item.rect.width` for text-only items (BUG-179).
/// Потолок главной оси флекс-элемента во ВНЕШНИХ величинах (граничная рамка
/// плюс поля) — `f32::INFINITY`, если максимум не задан или не разрешается в
/// длину.
///
/// Нужен шагу «fix min/max violations» (CSS Flexbox §9.7 шаг 4): растущий
/// элемент обязан замереть на своём `max-width`/`max-height`, а не забирать
/// всё свободное место строки. Величина внешняя, потому что гипотетические
/// главные размеры в `lay_out_flex` тоже внешние.
pub(crate) fn flex_item_max_main_outer(
    item: &LayoutBox,
    cb: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    is_column: bool,
) -> f32 {
    let s = &item.style;
    let em = s.font_size;
    let max_len = if is_column { s.max_height.as_ref() } else { s.max_width.as_ref() };
    let Some(max_len) = max_len else {
        return f32::INFINITY;
    };
    // Внутренние ключевые слова: по главной оси-ширине `min-content`/`max-content`
    // измеряются по содержимому (граничная рамка — вместе с padding и border);
    // остальные (`fit-content` и родня), как и высота, здесь не ограничивают —
    // промах в бо́льшую сторону безопаснее, чем ложная заморозка элемента.
    if max_len.is_intrinsic() {
        if !is_column && matches!(max_len, Length::MinContent | Length::MaxContent) {
            let border_box = if matches!(max_len, Length::MinContent) {
                min_content_outer_width(item, measurer, viewport)
            } else {
                max_content_outer_width(item, measurer, viewport)
            };
            let m_l = s.margin_left.resolve_or_zero(em, cb, viewport);
            let m_r = s.margin_right.resolve_or_zero(em, cb, viewport);
            return (border_box + m_l + m_r).max(0.0);
        }
        return f32::INFINITY;
    }
    let Some(v) = max_len.resolve(em, Some(cb), viewport) else {
        return f32::INFINITY;
    };
    let (p_start, p_end, b_start, b_end) = if is_column {
        (
            s.padding_top.resolve_or_zero(em, cb, viewport),
            s.padding_bottom.resolve_or_zero(em, cb, viewport),
            s.border_top_width,
            s.border_bottom_width,
        )
    } else {
        (
            s.padding_left.resolve_or_zero(em, cb, viewport),
            s.padding_right.resolve_or_zero(em, cb, viewport),
            s.border_left_width,
            s.border_right_width,
        )
    };
    let border_box = match s.box_sizing {
        BoxSizing::ContentBox => v + p_start + p_end + b_start + b_end,
        BoxSizing::BorderBox => v,
    };
    let (m_start, m_end) = if is_column {
        (
            s.margin_top.resolve_or_zero(em, cb, viewport),
            s.margin_bottom.resolve_or_zero(em, cb, viewport),
        )
    } else {
        (
            s.margin_left.resolve_or_zero(em, cb, viewport),
            s.margin_right.resolve_or_zero(em, cb, viewport),
        )
    };
    (border_box + m_start + m_end).max(0.0)
}

pub(crate) fn flex_auto_base_main_width(
    item: &LayoutBox,
    cb: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    flex_auto_base_main_width_from(item, max_content_outer_width(item, measurer, viewport), cb, measurer, viewport)
}

/// [`flex_auto_base_main_width`] with the content size supplied by the caller
/// instead of read off the item's max-content width — for an item whose width
/// only a layout can tell (BUG-1264: columns of vertical text).
pub(crate) fn flex_auto_base_main_width_from(
    item: &LayoutBox,
    content: f32,
    cb: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &item.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, cb, viewport);
    let pr = s.padding_right.resolve_or_zero(em, cb, viewport);
    // content-box → border-box conversion for a resolved min/max length.
    let outer_horiz = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => v + pl + pr + s.border_left_width + s.border_right_width,
        BoxSizing::BorderBox => v,
    };
    let mut base = content;
    if let Some(max_len) = &s.max_width {
        let max_bb = if max_len.is_intrinsic() {
            Some(max_content_outer_width(item, measurer, viewport))
        } else {
            max_len
                .resolve(em, Some(cb), viewport)
                .map(|v| outer_horiz(v).max(0.0))
        };
        if let Some(m) = max_bb {
            base = base.min(m);
        }
    }
    if let Some(min_len) = &s.min_width {
        let min_bb = if min_len.is_intrinsic() {
            Some(min_content_outer_width(item, measurer, viewport))
        } else {
            min_len
                .resolve(em, Some(cb), viewport)
                .map(|v| outer_horiz(v.max(0.0)))
        };
        if let Some(m) = min_bb {
            base = base.max(m);
        }
    }
    base.max(0.0)
}

/// CSS Flexbox L1 §4.5 — automatic minimum size (main axis, **border-box**) of a
/// row-direction flex item. This is the floor below which the item may not be
/// shrunk by `flex-shrink` (§9.7 step 4). Margins are excluded (the caller adds
/// them).
///
/// * An explicit `min-width` always wins — it is simply resolved (an intrinsic
///   keyword resolves against the item's own min-content width).
/// * `min-width: auto` (the initial value, stored as `None`) means the
///   *content-based minimum size*: the smaller of the item's *content size
///   suggestion* (the min-content width of its **contents** — see
///   [`min_content_outer_width_of_contents`]) and its *specified size
///   suggestion* (its own definite `width`, when it has one), capped by a
///   definite `max-width`. Taking the smaller of the two is what keeps an item
///   whose contents can collapse — e.g. one holding only a `width: 100%` child —
///   shrinkable below its own preferred width.
///   It applies only while the main-axis overflow is `visible`; a scroll
///   container has no content-based minimum and may shrink to zero.
///
/// `cb` is the flex container's inner main size, used to resolve percentages.
pub(crate) fn flex_item_min_main_width(
    item: &LayoutBox,
    cb: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &item.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, cb, viewport);
    let pr = s.padding_right.resolve_or_zero(em, cb, viewport);
    // content-box → border-box conversion for a resolved min/max length.
    let outer_horiz = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => v + pl + pr + s.border_left_width + s.border_right_width,
        BoxSizing::BorderBox => v,
    };
    if let Some(min_len) = &s.min_width {
        let v = if min_len.is_intrinsic() {
            min_content_outer_width(item, measurer, viewport)
        } else {
            min_len
                .resolve(em, Some(cb), viewport)
                .map_or(0.0, |v| outer_horiz(v.max(0.0)))
        };
        // CSS Tables L3 §"used min width of table": a table is never narrower than
        // its min-content width, whatever `min-width` says (FLEX-VWM-4).
        if matches!(s.display, Display::Table | Display::InlineTable) {
            return v.max(min_content_outer_width_of_contents(item, measurer, viewport)).max(0.0);
        }
        return v.max(0.0);
    }
    if s.overflow_x != Overflow::Visible {
        return 0.0;
    }
    let mut floor = min_content_outer_width_of_contents(item, measurer, viewport);
    // Specified size suggestion — the item's own definite preferred main size.
    if let Some(w_len) = &s.width
        && !w_len.is_intrinsic()
        && let Some(w) = w_len.resolve(em, Some(cb), viewport)
    {
        floor = floor.min(outer_horiz(w).max(0.0));
    }
    if let Some(max_len) = &s.max_width
        && !max_len.is_intrinsic()
        && let Some(v) = max_len.resolve(em, Some(cb), viewport)
    {
        floor = floor.min(outer_horiz(v).max(0.0));
    }
    floor.max(0.0)
}

/// BUG-926 — max-content border-box width of a form control whose used width
/// comes from the content it renders itself (`<button>`, `<select>`,
/// `<selectlist>`). `None` = "this control has no content-derived width", i.e.
/// keep the replaced-element path.
///
/// `BoxKind::FormControl` is sized as a replaced element (CSS 2.1 §10.3.2 —
/// `width: auto` resolves to the intrinsic size, not to the containing block).
/// A form control, however, has no decoded pixels to take that intrinsic size
/// from, so the replaced branch yielded 0 and the shrink-to-fit `min` below it
/// could only preserve that 0 — a `<button>` collapsed into a pair of borders.
/// HTML rendering §15.5.1 sizes these controls from their rendered content:
///
/// * `<button>` renders child boxes, so its content width is the ordinary
///   shrink-to-fit width of the subtree ([`preferred_inline_block_width`]).
///   An empty button is still as wide as its own padding + border.
/// * `<select>`/`<selectlist>` render `FormControlKind::Select::selected_text`
///   plus the native dropdown arrow. Neither is a box — UA style gives
///   `<option>` `display: none` — so the label is measured here, with the same
///   font size and inner padding the widget paints with
///   ([`select_widget_font_size`], [`SELECT_WIDGET_PAD_PX`]); `appearance: none`
///   drops the arrow column in paint, so it is not reserved here either.
///
/// Every other `FormControlKind` keeps the old path: checkbox, radio, range,
/// progress and meter have a real widget intrinsic size and no rendered label,
/// and the text-entry controls receive an explicit UA `width` (or a
/// `field-sizing: content` size) before this is ever consulted.
pub(crate) fn form_control_fit_content_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> Option<f32> {
    let BoxKind::FormControl { kind } = &b.kind else {
        return None;
    };
    let s = &b.style;
    let em = s.font_size;
    // % padding is unresolvable in an intrinsic context — same `0.0` basis as
    // the rest of this module.
    let frame = s.padding_left.resolve_or_zero(em, 0.0, viewport)
        + s.padding_right.resolve_or_zero(em, 0.0, viewport)
        + s.border_left_width
        + s.border_right_width;
    match kind {
        FormControlKind::Button => {
            Some(preferred_inline_block_width(b, measurer, viewport).unwrap_or(frame).max(frame))
        }
        FormControlKind::Select { selected_text } => {
            let widget_fs = select_widget_font_size(em);
            let label_w = measurer.map_or(0.0, |m| {
                let tab = super::inline_wrap::TabStops::of(s, m).unit;
                measure_text_w_families(
                    selected_text,
                    widget_fs,
                    s.letter_spacing,
                    tab,
                    &s.font_family,
                    m,
                )
            });
            let arrow_w = if s.appearance == crate::style::Appearance::None {
                0.0
            } else {
                select_widget_arrow_width(em)
            };
            Some(frame + SELECT_WIDGET_PAD_PX * 2.0 + label_w + arrow_w)
        }
        _ => None,
    }
}
