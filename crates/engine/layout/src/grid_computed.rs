//! CSSOM serialisation of the grid longhands and the gaps for `getComputedStyle()` (BUG-1307).
//!
//! [`insert_grid_computed`] answers the *computed* value of every property from the [`ComputedStyle`]
//! alone: tracks with absolutised lengths, `repeat(auto-*, …)` kept as written, line names, areas,
//! the line-based placement. For a laid-out grid container the track lists are replaced by their
//! *resolved* value (Grid L1 §7.2.6: used track sizes, [`resolved_track_list`]) in
//! `resolved_geometry`, which is the one place that sees the layout.
//!
//! Known gaps (kept as before this slice): `repeat(<count>, …)` is expanded when the declaration is
//! parsed, so a non-grid element reports the expanded list; `grid-auto-columns`/`-rows` hold one
//! track size, not a list; the `grid` / `grid-template` shorthands are not serialised.

use std::collections::HashMap;

use lumen_core::geom::Size;

use crate::selector_query::{length_to_css, px_str};
use crate::style::{ComputedStyle, GridAutoFlow, GridLine, GridRepeat, GridTrackSize, Length, RepeatCount};

/// Inserts the grid longhands, `row-gap`/`column-gap`/`gap` (and the legacy `grid-*-gap` aliases) of
/// `style` into `m`.
pub(crate) fn insert_grid_computed(style: &ComputedStyle, m: &mut HashMap<String, String>) {
    let (em, z) = (style.font_size, if style.effective_zoom > 0.0 { style.effective_zoom } else { 1.0 });

    m.insert(
        "grid-template-columns".into(),
        template_list(&style.grid_template_columns, &style.grid_template_col_line_names, style.grid_template_col_auto_repeat.as_ref(), em, z),
    );
    m.insert(
        "grid-template-rows".into(),
        template_list(&style.grid_template_rows, &style.grid_template_row_line_names, style.grid_template_row_auto_repeat.as_ref(), em, z),
    );
    m.insert("grid-template-areas".into(), template_areas(&style.grid_template_areas));
    m.insert("grid-auto-columns".into(), auto_track_list(&style.grid_auto_columns, em, z));
    m.insert("grid-auto-rows".into(), auto_track_list(&style.grid_auto_rows, em, z));
    m.insert(
        "grid-auto-flow".into(),
        match style.grid_auto_flow {
            GridAutoFlow::Row => "row",
            GridAutoFlow::Column => "column",
            GridAutoFlow::RowDense => "dense",
            GridAutoFlow::ColumnDense => "column dense",
        }
        .into(),
    );

    let (cs, ce) = (&style.grid_column_start, &style.grid_column_end);
    let (rs, re) = (&style.grid_row_start, &style.grid_row_end);
    m.insert("grid-column-start".into(), grid_line(cs));
    m.insert("grid-column-end".into(), grid_line(ce));
    m.insert("grid-row-start".into(), grid_line(rs));
    m.insert("grid-row-end".into(), grid_line(re));
    m.insert("grid-column".into(), line_range(cs, ce));
    m.insert("grid-row".into(), line_range(rs, re));
    m.insert("grid-area".into(), grid_area(rs, cs, re, ce));

    let row_gap = if style.row_gap_normal { "normal".to_owned() } else { absolute_length(&style.row_gap, em, z) };
    let column_gap = if style.column_gap_normal { "normal".to_owned() } else { absolute_length(&style.column_gap, em, z) };
    let gap = if row_gap == column_gap { row_gap.clone() } else { format!("{row_gap} {column_gap}") };
    m.insert("gap".into(), gap.clone());
    m.insert("grid-gap".into(), gap);
    m.insert("grid-row-gap".into(), row_gap.clone());
    m.insert("grid-column-gap".into(), column_gap.clone());
    m.insert("row-gap".into(), row_gap);
    m.insert("column-gap".into(), column_gap);
}

/// Computed value of a `<length-percentage>`: absolute units become `px`, a percentage (or a `calc()` that
/// needs a basis) stays as written. `zoom` is un-applied from a baked-in `px` the way the other
/// box-model lengths are.
pub(crate) fn absolute_length(l: &Length, em: f32, zoom: f32) -> String {
    match l {
        Length::Px(v) => px_str(*v / zoom),
        Length::Em(_) | Length::Rem(_) => l.resolve(em, None, Size::ZERO).map_or_else(|| length_to_css(l), px_str),
        Length::Calc(node) => node
            .resolve(em, None, Size::ZERO)
            .map_or_else(|| length_to_css(l), |px| px_str(px.max(0.0))),
        _ => length_to_css(l),
    }
}

/// One track sizing function in its computed form (`minmax(30px, max-content)`, `fit-content(4%)`, `1fr`).
/// `grid-auto-columns` / `grid-auto-rows`: the `<track-size>+` list; empty is the initial `auto`.
fn auto_track_list(list: &[GridTrackSize], em: f32, zoom: f32) -> String {
    if list.is_empty() {
        return "auto".into();
    }
    list.iter().map(|t| track_size(t, em, zoom)).collect::<Vec<_>>().join(" ")
}

fn track_size(t: &GridTrackSize, em: f32, zoom: f32) -> String {
    match t {
        GridTrackSize::Auto => "auto".into(),
        GridTrackSize::Length(l) => absolute_length(l, em, zoom),
        GridTrackSize::Fr(v) => format!("{v}fr"),
        GridTrackSize::MinContent => "min-content".into(),
        GridTrackSize::MaxContent => "max-content".into(),
        GridTrackSize::Minmax(min, max) => format!("minmax({}, {})", track_size(min, em, zoom), track_size(max, em, zoom)),
        GridTrackSize::FitContent(limit) => format!("fit-content({})", track_size(limit, em, zoom)),
        GridTrackSize::Subgrid => "subgrid".into(),
        GridTrackSize::Masonry => "masonry".into(),
    }
}

/// `[a b]` — one line-name group, `None` when the line has no names.
fn names_group(names: Option<&Vec<String>>) -> Option<String> {
    names.filter(|n| !n.is_empty()).map(|n| format!("[{}]", n.join(" ")))
}

/// `tracks` interleaved with the names of the lines around them (`names[i]` is the line before track `i`).
fn tracks_with_names(tracks: &[String], names: &[Vec<String>], out: &mut Vec<String>) {
    for (i, t) in tracks.iter().enumerate() {
        out.extend(names_group(names.get(i)));
        out.push(t.clone());
    }
    out.extend(names_group(names.get(tracks.len())));
}

/// Computed `grid-template-columns` / `-rows` of an element that is not laid out as a grid: the
/// specified track list, lengths absolutised, `none` when empty.
fn template_list(tracks: &[GridTrackSize], names: &[Vec<String>], repeat: Option<&GridRepeat>, em: f32, zoom: f32) -> String {
    let sizes = |ts: &[GridTrackSize]| ts.iter().map(|t| track_size(t, em, zoom)).collect::<Vec<_>>();
    if matches!(tracks, [GridTrackSize::Subgrid]) {
        return subgrid_list(names.iter().map(Vec::as_slice));
    }
    let mut out = Vec::new();
    if let Some(rep) = repeat {
        let kind = if rep.count == RepeatCount::AutoFit { "auto-fit" } else { "auto-fill" };
        let (before, inner, after) = (sizes(&rep.before), sizes(&rep.tracks), sizes(&rep.after));
        tracks_with_names(&before, &rep.names.before, &mut out);
        let mut body = Vec::new();
        tracks_with_names(&inner, &rep.names.rep, &mut body);
        out.push(format!("repeat({kind}, {})", body.join(" ")));
        let mut tail = Vec::new();
        tracks_with_names(&after, &rep.names.after, &mut tail);
        out.extend(tail);
    } else {
        tracks_with_names(&sizes(tracks), names, &mut out);
    }
    if out.is_empty() { "none".into() } else { out.join(" ") }
}

/// Resolved value of `grid-template-columns` / `-rows` of a grid container (Grid L1 §7.2.6): the used
/// size of every track in px, the names of the lines between them.
pub(crate) fn resolved_track_list(sizes: &[f32], names: &[Vec<String>], zoom: f32) -> String {
    if sizes.is_empty() {
        return "none".into();
    }
    let px: Vec<String> = sizes.iter().map(|s| px_str(*s / zoom)).collect();
    let mut out = Vec::new();
    tracks_with_names(&px, names, &mut out);
    out.join(" ")
}

/// `names` (the line names a grid axis was laid out with) less the implicit `<area>-start` /
/// `<area>-end` names the container's own `grid-template-areas` added to them: CSS Grid L1 §7.3.2 —
/// implicitly-assigned line names do not appear in the value of `grid-template-rows`/`-columns`,
/// the resolved value (§7.2.6) included. A name also written explicitly keeps its written copy.
pub(crate) fn without_area_line_names(names: &[Vec<String>], areas: &[Vec<String>], col: bool) -> Vec<Vec<String>> {
    let mut implicit = Vec::new();
    crate::box_tree::add_area_line_names(&mut implicit, areas, col);
    let mut out = names.to_vec();
    for (line, added) in out.iter_mut().zip(&implicit) {
        for name in added {
            if let Some(i) = line.iter().rposition(|n| n == name) {
                line.remove(i);
            }
        }
    }
    out
}

/// `subgrid [a] [] [b c]` — every line is listed, an unnamed one as `[]`.
fn subgrid_list<'a>(lines: impl Iterator<Item = &'a [String]>) -> String {
    std::iter::once("subgrid".to_owned()).chain(lines.map(|n| format!("[{}]", n.join(" ")))).collect::<Vec<_>>().join(" ")
}

/// Resolved value of a `subgrid` axis of `tracks` tracks (Grid L2 §9): the names of its `tracks + 1` lines.
pub(crate) fn resolved_subgrid_list(tracks: usize, names: &[Vec<String>]) -> String {
    subgrid_list((0..=tracks).map(|i| names.get(i).map_or(&[][..], Vec::as_slice)))
}

/// `grid-template-areas`: `none`, or one quoted string per row.
fn template_areas(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return "none".into();
    }
    rows.iter().map(|r| format!("\"{}\"", r.join(" "))).collect::<Vec<_>>().join(" ")
}

/// A `<grid-line>` (Grid L1 §8.3): `span 1 i` is `span i`, any other count is kept.
fn grid_line(l: &GridLine) -> String {
    match l {
        GridLine::Auto => "auto".into(),
        GridLine::Line(n) => n.to_string(),
        GridLine::Span(n) => format!("span {n}"),
        GridLine::Named(name) => name.clone(),
        GridLine::NamedLine(name, n) => format!("{n} {name}"),
        GridLine::SpanNamed(name, 1) => format!("span {name}"),
        GridLine::SpanNamed(name, n) => format!("span {n} {name}"),
    }
}

/// An omitted `grid-row-end` / … copies a custom-ident start and is `auto` otherwise (Grid L1 §8.4):
/// whether `line` is exactly what the shorthand would fill in for `start`.
fn is_implied_by(line: &GridLine, start: &GridLine) -> bool {
    match start {
        GridLine::Named(_) => line == start,
        _ => *line == GridLine::Auto,
    }
}

/// `grid-column` / `grid-row` in the shortest form: the end line is dropped when it is implied by the start.
fn line_range(start: &GridLine, end: &GridLine) -> String {
    if is_implied_by(end, start) {
        grid_line(start)
    } else {
        format!("{} / {}", grid_line(start), grid_line(end))
    }
}

/// `grid-area` (`row-start / column-start / row-end / column-end`) in the shortest form: trailing lines
/// the shorthand fills in (`column-end` from `column-start`, `row-end` from `row-start`, `column-start`
/// from `row-start`) are dropped.
fn grid_area(row_start: &GridLine, col_start: &GridLine, row_end: &GridLine, col_end: &GridLine) -> String {
    let mut parts = vec![row_start, col_start, row_end, col_end];
    if is_implied_by(col_end, col_start) {
        parts.pop();
        if is_implied_by(row_end, row_start) {
            parts.pop();
            if is_implied_by(col_start, row_start) {
                parts.pop();
            }
        }
    }
    parts.into_iter().map(grid_line).collect::<Vec<_>>().join(" / ")
}
