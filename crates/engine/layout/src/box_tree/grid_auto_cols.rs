//! CSS Grid L1 §11 — content-based sizing of `auto` columns, with CSS Grid L2 §9
//! subgrid contributions (BUG-1318).
//!
//! Until this module an `auto` column was just an equal share of the free space,
//! so a subgrid's items (whose tracks are the parent's) could not widen the
//! parent's tracks. A grid whose column tracks are all `auto` or fixed lengths
//! is sized here instead: every item (a subgrid item is replaced by its own
//! items, placed in the parent's coordinates) reports a min-content and a
//! max-content contribution; single-track items set the base size / growth
//! limit, items spanning several tracks add what the spanned tracks lack
//! (smallest span first), then the free space grows the tracks up to their
//! limits and the rest stretches the `auto` ones equally. `minmax(auto, <length>)` is
//! sized here too (BUG-1313): its base is the items' minimum contribution, its growth
//! limit the length — raised to the base when the length is smaller. Templates with `fr`,
//! other `minmax()`, `min-content`, … keep the older path in `grid.rs`.

use super::grid::{grid_item_indices, grid_track, place_grid_items, GridAxis};
use super::intrinsic::{max_content_outer_width, min_content_outer_width};
use super::*;

/// One track sizing function (min or max side) on this path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Sizing {
    /// A fixed length, already resolved to px.
    Fixed(f32),
    /// `auto`: the items' minimum / maximum contribution (a max side is also stretched).
    Auto,
    /// `min-content`.
    MinContent,
    /// `max-content`.
    MaxContent,
}

/// Sizing function of one column track on this path: `min` gives the base size, `max` the
/// growth limit (never below the base, Grid L1 §11.5 step 5). Only an `auto` max side
/// stretches; `minmax(auto, <length>)` grows towards the length but not past it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TrackKind {
    pub(super) min: Sizing,
    pub(super) max: Sizing,
}

impl TrackKind {
    pub(super) const AUTO: Self = Self { min: Sizing::Auto, max: Sizing::Auto };

    fn fixed(l: f32) -> Self {
        Self { min: Sizing::Fixed(l), max: Sizing::Fixed(l) }
    }

    fn is_fixed(&self) -> bool {
        matches!((self.min, self.max), (Sizing::Fixed(_), Sizing::Fixed(_)))
    }

    /// The base size is set by the items.
    fn grows_base(&self) -> bool {
        !matches!(self.min, Sizing::Fixed(_))
    }

    /// The growth limit is set by the items.
    fn grows_limit(&self) -> bool {
        !matches!(self.max, Sizing::Fixed(_))
    }
}

/// What one item (outer margin box, plus the subgrid edge it sits on) asks of
/// the tracks it spans.
#[derive(Clone, Copy, Debug)]
pub(super) struct ColContribution {
    /// 0-based first track in the sized grid.
    pub(super) start: usize,
    pub(super) span: usize,
    pub(super) min: f32,
    pub(super) max: f32,
}

/// One side of a track sizing function: `auto`, a length `resolve` can turn into px, `min-content`
/// or `max-content`; `None` for `fr`, `fit-content()`, nested `minmax()`, …
fn sizing_of(t: &GridTrackSize, resolve: &dyn Fn(&Length) -> Option<f32>) -> Option<Sizing> {
    Some(match t {
        GridTrackSize::Auto => Sizing::Auto,
        GridTrackSize::Length(l) => Sizing::Fixed(resolve(l)?.max(0.0)),
        GridTrackSize::MinContent => Sizing::MinContent,
        GridTrackSize::MaxContent => Sizing::MaxContent,
        _ => return None,
    })
}

/// `Some(kinds)` when every one of the `n_cols` tracks is a keyword (`auto`, `min-content`,
/// `max-content`), a length that `resolve` can turn into px or a `minmax()` of those, and at
/// least one is not a plain length; `None` otherwise (the caller keeps its older path).
pub(super) fn classify_col_tracks(
    template: &[GridTrackSize],
    auto_track: &GridTrackSize,
    n_cols: usize,
    resolve: &dyn Fn(&Length) -> Option<f32>,
) -> Option<Vec<TrackKind>> {
    classify_all_col_tracks(template, auto_track, n_cols, resolve).filter(|k| k.iter().any(|k| !k.is_fixed()))
}

/// [`classify_col_tracks`] без отбраковки сетки, где все дорожки фиксированы: `grid-lanes` без шаблона
/// (одни `grid-auto-columns: 80px`) всё равно считается по дорожкам — число полос задают элементы.
pub(super) fn classify_all_col_tracks(
    template: &[GridTrackSize],
    auto_track: &GridTrackSize,
    n_cols: usize,
    resolve: &dyn Fn(&Length) -> Option<f32>,
) -> Option<Vec<TrackKind>> {
    let mut kinds = Vec::with_capacity(n_cols);
    for c in 0..n_cols {
        kinds.push(match grid_track(c as u32, template, auto_track) {
            GridTrackSize::Minmax(min, max) => {
                TrackKind { min: sizing_of(min, resolve)?, max: sizing_of(max, resolve)? }
            }
            GridTrackSize::Length(l) => TrackKind::fixed(resolve(l)?.max(0.0)),
            t => {
                let z = sizing_of(t, resolve)?;
                TrackKind { min: z, max: z }
            }
        });
    }
    Some(kinds)
}

/// Is `b` a grid container whose column axis is `subgrid`?
pub(super) fn is_col_subgrid(b: &LayoutBox) -> bool {
    matches!(b.style.display, Display::Grid | Display::InlineGrid)
        && b.style.grid_template_columns.first() == Some(&GridTrackSize::Subgrid)
}

/// Margin + border + padding of `b` on its (left, right) edges.
fn inline_edges(b: &LayoutBox, viewport: Size) -> (f32, f32) {
    let s = &b.style;
    let em = s.font_size;
    (
        s.margin_left.resolve_or_zero(em, 0.0, viewport)
            + s.border_left_width
            + s.padding_left.resolve_or_zero(em, 0.0, viewport),
        s.margin_right.resolve_or_zero(em, 0.0, viewport)
            + s.border_right_width
            + s.padding_right.resolve_or_zero(em, 0.0, viewport),
    )
}

/// Collect the column contributions of the placed items `item_idxs` of a grid
/// (or subgrid) whose first track is track `base_col` of the sized grid and
/// which has `n_cols` tracks. `edge_start` / `edge_end` is the extra
/// margin/border/padding of the enclosing subgrids that an item on the first /
/// last track must carry (Grid L2 §9, "extra margin"). `col_names` — the names of the
/// `n_cols + 1` column lines of this (sub)grid; a subgrid item adds the parent lines it spans
/// to its own names (Grid L2 §9), which its items may be placed by.
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_col_contributions(
    children: &[LayoutBox],
    item_idxs: &[usize],
    placements: &[(u32, u32, u32, u32)],
    col_names: &[Vec<String>],
    n_cols: usize,
    base_col: usize,
    edge_start: f32,
    edge_end: f32,
    viewport: Size,
    contribution: &dyn Fn(&LayoutBox) -> (f32, f32),
    out: &mut Vec<ColContribution>,
) {
    for (k, &i) in item_idxs.iter().enumerate() {
        let item = &children[i];
        let (cs, ce, _, _) = placements[k];
        if cs == 0 {
            continue;
        }
        let local_start = cs as usize - 1;
        let span = (ce.saturating_sub(cs) as usize).max(1);
        let local_end = local_start + span;
        let es = if local_start == 0 { edge_start } else { 0.0 };
        let ee = if local_end >= n_cols { edge_end } else { 0.0 };
        if is_col_subgrid(item) {
            let sub_idxs = grid_item_indices(&item.children, super::fieldset::rendered_legend_idx_of(item));
            let st = &item.style;
            let rows_len = st.grid_template_rows.len();
            let sub_names = super::grid::subgrid_line_names(
                &st.grid_template_col_line_names,
                st.grid_template_col_subgrid_fill,
                &crate::subgrid::line_names_between(col_names, local_start, local_end),
                span,
            );
            let col_axis = GridAxis {
                n_tracks: span as u32,
                names: &sub_names,
                areas: &st.grid_template_areas,
                is_col: true,
                clamp: true,
            };
            let row_axis = GridAxis {
                n_tracks: rows_len.max(st.grid_template_areas.len()) as u32,
                names: &st.grid_template_row_line_names,
                areas: &st.grid_template_areas,
                is_col: false,
                clamp: false,
            };
            let sub_placements =
                place_grid_items(&item.children, &sub_idxs, st, span, rows_len, &col_axis, &row_axis);
            let (own_start, own_end) = inline_edges(item, viewport);
            collect_col_contributions(
                &item.children,
                &sub_idxs,
                &sub_placements,
                &sub_names,
                span,
                base_col + local_start,
                es + own_start,
                ee + own_end,
                viewport,
                contribution,
                out,
            );
        } else {
            let em = item.style.font_size;
            let ml = item.style.margin_left.resolve_or_zero(em, 0.0, viewport);
            let mr = item.style.margin_right.resolve_or_zero(em, 0.0, viewport);
            let (mn, mx) = contribution(item);
            let extra = ml + mr + es + ee;
            out.push(ColContribution {
                start: base_col + local_start,
                span,
                min: mn + extra,
                max: mx + extra,
            });
        }
    }
}

/// CSS Grid L1 §11.5 — base sizes and growth limits of the tracks. `gap` is the
/// gutter between adjacent tracks (a spanning item covers the gutters inside its
/// span, so they do not count against what the tracks lack).
pub(super) fn base_and_limit(
    kinds: &[TrackKind],
    contribs: &[ColContribution],
    gap: f32,
) -> (Vec<f32>, Vec<f32>) {
    let fixed = |z: Sizing| if let Sizing::Fixed(l) = z { l } else { 0.0 };
    let mut base: Vec<f32> = kinds.iter().map(|k| fixed(k.min)).collect();
    let mut limit: Vec<f32> = kinds.iter().map(|k| fixed(k.max)).collect();
    // What an item asks of one side of a track: `min-content` its minimum contribution,
    // `auto` its minimum (base) or maximum (limit), `max-content` its maximum.
    let ask = |z: Sizing, is_base: bool, c: &ColContribution| match z {
        Sizing::Fixed(_) => None,
        Sizing::MinContent => Some(c.min),
        Sizing::MaxContent => Some(c.max),
        Sizing::Auto => Some(if is_base { c.min } else { c.max }),
    };
    for c in contribs.iter().filter(|c| c.span == 1 && c.start < kinds.len()) {
        let k = kinds[c.start];
        if let Some(v) = ask(k.min, true, c) {
            base[c.start] = base[c.start].max(v);
        }
        if let Some(v) = ask(k.max, false, c) {
            limit[c.start] = limit[c.start].max(v);
        }
    }
    let mut spanning: Vec<&ColContribution> =
        contribs.iter().filter(|c| c.span > 1 && c.start < kinds.len()).collect();
    spanning.sort_by_key(|c| c.span);
    for c in spanning {
        let end = (c.start + c.span).min(kinds.len());
        let gaps = gap * (end - c.start - 1) as f32;
        // The base grows on the tracks with an intrinsic min side, the limit on those with an
        // intrinsic max side; each takes an equal share of what the span still lacks.
        for is_base in [true, false] {
            let targets: Vec<usize> = (c.start..end)
                .filter(|&t| if is_base { kinds[t].grows_base() } else { kinds[t].grows_limit() })
                .collect();
            let Some(want) = targets
                .iter()
                .filter_map(|&t| ask(if is_base { kinds[t].min } else { kinds[t].max }, is_base, c))
                .reduce(f32::min)
            else {
                continue;
            };
            let sizes = if is_base { &mut base } else { &mut limit };
            let have: f32 = sizes[c.start..end].iter().sum::<f32>() + gaps;
            let extra = want - have;
            if extra > 0.0 {
                let share = extra / targets.len() as f32;
                for &t in &targets {
                    sizes[t] += share;
                }
            }
        }
    }
    for (l, b) in limit.iter_mut().zip(&base) {
        *l = l.max(*b);
    }
    (base, limit)
}

/// CSS Grid L1 §11.6–11.8 — grow the tracks from their base sizes towards their
/// growth limits with the free space of `available` (equally, a saturated track
/// drops out), then stretch the `auto` tracks with whatever is left. Without
/// enough room the tracks stay at their base sizes (the grid overflows).
pub(super) fn distribute_free_space(
    kinds: &[TrackKind],
    base: &[f32],
    limit: &[f32],
    gap: f32,
    available: f32,
) -> Vec<f32> {
    const EPS: f32 = 1e-3;
    let mut w = base.to_vec();
    let gaps = gap * kinds.len().saturating_sub(1) as f32;
    let mut free = available - w.iter().sum::<f32>() - gaps;
    while free > EPS {
        let growable: Vec<usize> = (0..w.len()).filter(|&t| limit[t] - w[t] > EPS).collect();
        if growable.is_empty() {
            break;
        }
        let n = growable.len() as f32;
        let headroom = growable.iter().map(|&t| limit[t] - w[t]).fold(f32::INFINITY, f32::min);
        let step = (free / n).min(headroom);
        for &t in &growable {
            w[t] += step;
        }
        free -= step * n;
    }
    if free > EPS {
        let autos: Vec<usize> = (0..w.len()).filter(|&t| kinds[t].max == Sizing::Auto).collect();
        if !autos.is_empty() {
            let share = free / autos.len() as f32;
            for t in autos {
                w[t] += share;
            }
        }
    }
    w
}

/// Border-box width of a definite `min-width` of `b` (`None` for `auto`, a percentage or an
/// intrinsic keyword, none of which has a floor to offer before layout).
fn min_width_floor(b: &LayoutBox, viewport: Size) -> Option<f32> {
    let s = &b.style;
    let em = s.font_size;
    let len = s.min_width.as_ref().filter(|l| !l.is_intrinsic())?;
    let v = len.resolve(em, None, viewport)?.max(0.0);
    Some(match s.box_sizing {
        BoxSizing::ContentBox => {
            v + s.padding_left.resolve_or_zero(em, 0.0, viewport)
                + s.padding_right.resolve_or_zero(em, 0.0, viewport)
                + s.border_left_width
                + s.border_right_width
        }
        BoxSizing::BorderBox => v,
    })
}

/// Min-/max-content contribution of a grid item's border box: the ordinary
/// intrinsic widths, never below a definite `min-width` (Grid L1 §11.5: the minimum
/// contribution of an `auto`-sized item is its used minimum size).
pub(super) fn item_contribution<'a>(
    measurer: Option<&'a dyn TextMeasurer>,
    viewport: Size,
) -> impl Fn(&LayoutBox) -> (f32, f32) + 'a {
    move |c| {
        let floor = min_width_floor(c, viewport).unwrap_or(0.0);
        (
            min_content_outer_width(c, measurer, viewport).max(floor),
            max_content_outer_width(c, measurer, viewport).max(floor),
        )
    }
}

/// Final column widths of a grid whose tracks are all `auto` / `minmax(auto, <length>)` /
/// fixed lengths and that has at least two columns or a `minmax(auto, <length>)` one (a
/// lone `auto` column is the whole free space either way, so it is not worth measuring
/// every item); `None` when the older path must size it. `content_width` is the definite inline size to fill.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn content_sized_col_widths(
    children: &[LayoutBox],
    item_idxs: &[usize],
    placements: &[(u32, u32, u32, u32)],
    col_names: &[Vec<String>],
    s: &ComputedStyle,
    eff_col_template: &[GridTrackSize],
    n_cols: usize,
    col_gap: f32,
    content_width: f32,
    em: f32,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
) -> Option<Vec<f32>> {
    let kinds = classify_col_tracks(eff_col_template, &s.grid_auto_columns, n_cols, &|l| {
        l.resolve(em, Some(content_width), viewport)
    })?;
    if n_cols < 2 && kinds.iter().all(|k| k.is_fixed() || *k == TrackKind::AUTO) {
        return None;
    }
    let mut contribs = Vec::new();
    let measure = item_contribution(measurer, viewport);
    collect_col_contributions(
        children, item_idxs, placements, col_names, n_cols, 0, 0.0, 0.0, viewport, &measure, &mut contribs,
    );
    let (base, limit) = base_and_limit(&kinds, &contribs, col_gap);
    Some(distribute_free_space(&kinds, &base, &limit, col_gap, content_width))
}
