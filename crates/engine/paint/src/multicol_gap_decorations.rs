//! CSS Gap Decorations L1 on a multicol container laid out in *rows* of columns
//! (CSS Multicol L2 §4.2 / §4.4: a non-`auto` `column-height` with `column-wrap: wrap`).
//!
//! Rows are `column-height` tall and `row-gap` apart, columns inside a row are `column-gap`
//! apart, so both axes have gaps:
//!
//! * a `column-rule` is drawn in each column gap of a row, only between two columns that both hold
//!   content (Multicol L1 §4); `column-rule-break: none` joins the pieces of all rows into one line
//!   through the row gaps, `normal` (= `intersection` for multicol columns) and `intersection` keep
//!   one piece per row;
//! * a `row-rule` is drawn in each row gap; `row-rule-break: intersection` cuts it at the column
//!   gaps, `none`/`normal` (= `none` for multicol rows) keep one line across the content box;
//!   `row-rule-visibility-items: between|around` (`normal`/`all` for rows paint everywhere) cut it
//!   into one piece per column and keep the pieces whose two cells both / either hold content;
//! * `*-rule-inset-cap-*` act at the container's edges, `*-rule-inset-junction-*` where a rule is
//!   cut by a visible crossing rule (`%` against the crossing gap, `overlap-join` = half the gap
//!   plus half the crossing rule width — the same rules as `flex_gap_decorations`);
//! * `rule-overlap` decides which axis is painted on top.
//!
//! A `column-span: all` child splits a row: the columns before it form a band of their own (balanced,
//! shorter than `column-height`), the spanner sits between two bands with no row gap and no rule,
//! and the band after it takes the rest of the row (Multicol L2 §4.4). Bands are read back from the
//! fragment geometry (`read_bands`).
//!
//! `direction: rtl` mirrors the inline axis: the first column gap and the first value of a rule list
//! are the rightmost ones, `row-rule-inset-start` insets the right end of a row rule.
//!
//! Limits: `writing-mode` does not map the axes; a spanner with `row-gap: 0`
//! cannot be told from a row boundary, so the caller falls back to the one-row painter.

use lumen_core::geom::{Rect, Size};
use lumen_layout::{BoxKind, ComputedStyle, Direction, LayoutBox, Position, RuleBreak, RuleInset, RuleOverlap, RuleVisibilityItems};

use crate::display_list::DisplayCommand;
use crate::gap_decorations::{inset_span, rule_line_commands};

/// The multicol geometry shared by the inline-axis and row painters, in px.
pub(crate) struct MulticolGeom {
    pub content_x: f32,
    pub content_y: f32,
    pub content_w: f32,
    pub col_w: f32,
    pub col_gap: f32,
    pub n_cols: u32,
    pub col_h: f32,
    pub row_gap: f32,
}

/// Number of columns from `column-count` / `column-width` (CSS Multicol L1 §3.4) — the same
/// formula `build_multicol_init` uses in `lumen-layout`.
pub(crate) fn multicol_column_count(s: &ComputedStyle, em: f32, content_w: f32, col_gap: f32, vp: Size) -> u32 {
    match (s.column_count, &s.column_width) {
        (Some(n), Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(content_w), vp)
                && w > 0.0
            {
                let n_from_w = ((content_w + col_gap) / (w + col_gap)).floor() as u32;
                n.min(n_from_w).max(1)
            } else {
                n.max(1)
            }
        }
        (Some(n), None) => n.max(1),
        (None, Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(content_w), vp)
                && w > 0.0
            {
                ((content_w + col_gap) / (w + col_gap)).floor() as u32
            } else {
                1
            }
        }
        (None, None) => 1,
    }
    .max(1)
}

/// One row of column boxes (or the part of a row between spanners), in px.
struct Band {
    top: f32,
    bottom: f32,
    /// Which columns hold a fragment.
    cells: Vec<bool>,
    /// A row gap (not a spanner) separates this band from the next one.
    gap_after: bool,
}

/// A laid-out fragment: `(top, bottom, column)`; `column == None` marks a full-width spanner.
type Frag = (f32, f32, Option<usize>);

/// Reads the bands back from the laid-out children. `None` when they cannot be told apart
/// (a spanner with `row-gap: 0`).
fn read_bands(b: &LayoutBox, g: &MulticolGeom) -> Option<Vec<Band>> {
    let step = g.col_w + g.col_gap;
    let pitch = g.col_h + g.row_gap;
    let n = g.n_cols as usize;
    let mut frags: Vec<Frag> = Vec::new();
    for c in b
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip) && !matches!(c.style.position, Position::Absolute | Position::Fixed))
    {
        if c.rect.width <= 0.0 && c.rect.height <= 0.0 {
            continue;
        }
        let col = if c.rect.width > g.col_w + 1.0 && n > 1 {
            None
        } else {
            Some((((c.rect.x - g.content_x) / step).round().max(0.0) as usize).min(n - 1))
        };
        frags.push((c.rect.y, c.rect.y + c.rect.height, col));
    }
    if !frags.iter().any(|f| f.2.is_none()) {
        // No spanner: rows sit on the fixed `column-height + row-gap` pitch.
        let mut bands: Vec<Band> = Vec::new();
        for &(top, _, col) in &frags {
            let row = ((top - g.content_y + 0.01) / pitch).floor().max(0.0) as usize;
            while bands.len() <= row {
                let t = g.content_y + bands.len() as f32 * pitch;
                bands.push(Band { top: t, bottom: t + g.col_h, cells: vec![false; n], gap_after: true });
            }
            bands[row].cells[col?] = true;
        }
        if let Some(last) = bands.last_mut() {
            last.gap_after = false;
        }
        return Some(bands);
    }
    if g.row_gap <= 0.0 {
        return None;
    }
    frags.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Raw bands of column fragments between spanners.
    struct Raw {
        top: f32,
        bottom: f32,
        cells: Vec<bool>,
        preceded: bool,
        followed: bool,
        row_start: f32,
    }
    let mut raw: Vec<Raw> = Vec::new();
    let mut open = false; // the last raw band can still take fragments
    let mut after_span = true;
    let mut row_start = g.content_y;
    for &(top, bottom, col) in &frags {
        match col {
            None => {
                if open && let Some(last) = raw.last_mut() {
                    last.followed = true;
                }
                open = false;
                after_span = true;
            }
            Some(c) => {
                // A fragment at or past the end of the current row opens the next row.
                if open && top < row_start + g.col_h - 0.5 && let Some(l) = raw.last_mut() {
                    l.bottom = l.bottom.max(bottom);
                    l.cells[c] = true;
                } else {
                    // After a spanner the columns continue the row while `column-height` is left.
                    let same_row = after_span && !raw.is_empty() && top - row_start < g.col_h - 0.5;
                    if !same_row {
                        row_start = top;
                    }
                    let mut cells = vec![false; n];
                    cells[c] = true;
                    raw.push(Raw { top, bottom, cells, preceded: same_row, followed: false, row_start });
                    open = true;
                    after_span = false;
                }
            }
        }
    }
    let mut bands: Vec<Band> = Vec::with_capacity(raw.len());
    for r in raw {
        // A band closed by a spanner is balanced; a band that ends a row keeps `column-height`.
        let bottom = if r.followed { r.bottom } else { r.row_start + g.col_h };
        // The spanner between a band and the continuation of its row takes no row gap.
        if let Some(prev) = bands.last_mut() {
            prev.gap_after = !r.preceded;
        }
        bands.push(Band { top: r.top, bottom, cells: r.cells, gap_after: false });
    }
    Some(bands)
}

/// Whether a rule piece between two cells is painted under `column-rule-visibility-items` /
/// `row-rule-visibility-items` (CSS Gap Decorations L1 §3.4). `normal` is `between` for multicol
/// columns and `all` for multicol rows.
fn piece_shown(vis: RuleVisibilityItems, columns: bool, a: bool, b: bool) -> bool {
    match vis {
        RuleVisibilityItems::All => true,
        RuleVisibilityItems::Around => a || b,
        RuleVisibilityItems::Between => a && b,
        RuleVisibilityItems::Normal => !columns || (a && b),
    }
}

/// An end of a rule line: the crossing gap `(len, rule width)` when a visible crossing rule cuts
/// it there, `None` for the container's edge (a cap).
type Cross = Option<(f32, f32)>;

/// Px offset of one end of a rule line (positive shortens the line), CSS Gap Decorations L1 §3.3.
fn end_inset(inset: &RuleInset, cross: Cross, em: f32, vp: Size) -> f32 {
    match (inset, cross) {
        (RuleInset::Length(l), Some((len, _))) => l.resolve_or_zero(em, len, vp),
        (RuleInset::Length(l), None) => l.resolve_or_zero(em, 0.0, vp),
        (RuleInset::OverlapJoin, Some((len, w))) => -(len * 0.5 + w * 0.5),
        (RuleInset::OverlapJoin, None) => 0.0,
    }
}

/// Paints each point of one *row* rule line once. Pieces of a line are extended past their cut
/// points (`overlap-join`, a negative inset), so neighbouring pieces overlap at a column gap, and
/// a translucent colour would be blended twice there (Chromium blends it once).
/// `spans` are `(start, len)` along the line in ascending order; each span loses the part already covered by the spans before it.
fn without_overlap(spans: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
    let mut end = f32::NEG_INFINITY;
    let mut out = Vec::with_capacity(spans.len());
    for (start, len) in spans {
        let from = start.max(end);
        let to = start + len;
        if to > from {
            out.push((from, to - from));
        }
        end = end.max(to);
    }
    out
}

/// The two end insets `(start, end)` of a line whose ends meet `lo`/`hi` crossings.
fn line_insets(insets: &lumen_layout::RuleInsets, lo: Cross, hi: Cross, em: f32, vp: Size) -> (f32, f32) {
    let start = if lo.is_some() { &insets.junction_start } else { &insets.cap_start };
    let end = if hi.is_some() { &insets.junction_end } else { &insets.cap_end };
    (end_inset(start, lo, em, vp), end_inset(end, hi, em, vp))
}

/// Paints the column and row rules of a multicol container laid out in rows. Returns `false`
/// when the geometry cannot be read back (see [`occupied_cells`]) and the caller should paint
/// the single-row rules instead.
pub(crate) fn emit_multicol_row_rules(b: &LayoutBox, g: &MulticolGeom, content_h: f32, out: &mut Vec<DisplayCommand>) -> bool {
    let Some(bands) = read_bands(b, g) else {
        return false;
    };
    emit_bands(b, g, &bands, content_h, false, out);
    true
}

/// Reads the bands of a multicol container that has `column-span: all` children but no rows of
/// columns (no `column-height`): the columns between two spanners form one band each. `None` when
/// the container has no spanner.
fn read_spanner_bands(b: &LayoutBox, g: &MulticolGeom) -> Option<Vec<Band>> {
    let step = g.col_w + g.col_gap;
    let n = g.n_cols as usize;
    if n <= 1 {
        return None;
    }
    let mut frags: Vec<Frag> = Vec::new();
    for c in b
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip) && !matches!(c.style.position, Position::Absolute | Position::Fixed))
    {
        if c.rect.width <= 0.0 && c.rect.height <= 0.0 {
            continue;
        }
        let col = if c.rect.width > g.col_w + 1.0 {
            None
        } else {
            Some((((c.rect.x - g.content_x) / step).round().max(0.0) as usize).min(n - 1))
        };
        frags.push((c.rect.y, c.rect.y + c.rect.height, col));
    }
    if !frags.iter().any(|f| f.2.is_none()) {
        return None;
    }
    frags.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut bands: Vec<Band> = Vec::new();
    let mut open = false;
    for &(top, bottom, col) in &frags {
        match col {
            None => open = false,
            Some(c) if open => {
                if let Some(last) = bands.last_mut() {
                    last.bottom = last.bottom.max(bottom);
                    last.cells[c] = true;
                }
            }
            Some(c) => {
                let mut cells = vec![false; n];
                cells[c] = true;
                // The first band starts at the content box, the others where the spanner ends.
                let top = if bands.is_empty() { g.content_y } else { top };
                bands.push(Band { top, bottom, cells, gap_after: false });
                open = true;
            }
        }
    }
    // A rule never leaves the content box, even when overflowing columns run past it.
    let limit = g.content_y + g.col_h;
    for band in &mut bands {
        band.top = band.top.min(limit);
        band.bottom = band.bottom.min(limit);
    }
    Some(bands)
}

/// Paints the column rules of a multicol container whose `column-span: all` children cut the
/// columns into bands (CSS Multicol L1 §6.1: a rule does not run through a spanner; with
/// `column-rule-break: none` it does). Returns `false` when the container has no spanner.
pub(crate) fn emit_multicol_spanner_rules(b: &LayoutBox, g: &MulticolGeom, content_h: f32, out: &mut Vec<DisplayCommand>) -> bool {
    let Some(bands) = read_spanner_bands(b, g) else {
        return false;
    };
    emit_bands(b, g, &bands, content_h, true, out);
    true
}

/// `columns_all`: `column-rule-visibility-items: normal` paints every column gap (the reference of
/// multicol-gap-decorations-020 draws full-height rules between columns that hold no content in a
/// band); the explicit `between`/`around` values still hide pieces.
fn emit_bands(b: &LayoutBox, g: &MulticolGeom, bands: &[Band], content_h: f32, columns_all: bool, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    let rows = bands.len();
    if rows == 0 {
        return;
    }
    let em = s.font_size;
    let vp = Size::new(g.content_w, content_h);
    let n = g.n_cols as usize;
    let col_total = n.saturating_sub(1);
    // Row gaps are numbered in order, a spanner between two bands is not one (§4.6).
    let gap_ix: Vec<usize> = bands
        .iter()
        .scan(0usize, |k, band| {
            let ix = *k;
            *k += usize::from(band.gap_after);
            Some(ix)
        })
        .collect();
    let row_last = bands.iter().filter(|band| band.gap_after).count().saturating_sub(1);
    let cells = |r: usize, c: usize| bands[r].cells[c];
    let gap_below = |r: usize| r + 1 < rows && bands[r].gap_after;

    // `direction: rtl` runs the columns right to left: the first column gap (and the first value of
    // a rule list) is the rightmost one, `i` below counts gaps from the left edge.
    let rtl = s.direction == Direction::Rtl;
    let col_style = |i: usize| {
        let k = if rtl { col_total - 1 - i } else { i };
        (
            *s.column_rule_width.value_for_gap(k, col_total),
            *s.column_rule_style.value_for_gap(k, col_total),
            s.column_rule_color.value_for_gap(k, col_total).resolve(s.color),
        )
    };
    let row_style = |r: usize| {
        let ix = gap_ix[r];
        (
            *s.row_rule_width.value_for_gap(ix, row_last),
            *s.row_rule_style.value_for_gap(ix, row_last),
            s.row_rule_color.value_for_gap(ix, row_last).resolve(s.color),
        )
    };
    let col_visible = |i: usize| {
        let (w, st, _) = col_style(i);
        st.is_visible() && w > 0.0
    };
    let row_visible = |r: usize| {
        let (w, st, _) = row_style(r);
        st.is_visible() && w > 0.0
    };

    let col_vis = match s.column_rule_visibility_items {
        RuleVisibilityItems::Normal if columns_all => RuleVisibilityItems::All,
        v => v,
    };
    // Is the column-gap-`i` piece of row `r` / the row-gap-`r` piece of column `c` painted?
    let col_piece = |i: usize, r: usize| {
        col_visible(i) && piece_shown(col_vis, true, cells(r, i), cells(r, i + 1))
    };
    let row_piece = |r: usize, c: usize| {
        gap_below(r)
            && row_visible(r)
            && piece_shown(s.row_rule_visibility_items, false, cells(r, c), cells(r + 1, c))
            && (cells(r, c) || cells(r + 1, c) || s.row_rule_visibility_items == RuleVisibilityItems::All)
    };

    let vis = s.row_rule_visibility_items;
    // `row-rule-break: normal` is `none` for multicol rows; hiding pieces needs them cut anyway.
    let row_cut = s.row_rule_break == RuleBreak::Intersection
        || matches!(vis, RuleVisibilityItems::Between | RuleVisibilityItems::Around);
    // `normal` is `intersection` for multicol columns, so only an explicit `none` joins rows.
    let col_joined = s.column_rule_break == RuleBreak::None;

    // ── column rules (vertical lines in the column gaps) ────────────────────────────────────
    let mut col_cmds: Vec<DisplayCommand> = Vec::new();
    for i in 0..col_total {
        if !col_visible(i) {
            continue;
        }
        let (w, st, color) = col_style(i);
        let gap_left = g.content_x + (i + 1) as f32 * g.col_w + i as f32 * g.col_gap;
        let sep_x = gap_left + (g.col_gap - w) * 0.5;
        let both = |r: usize| col_piece(i, r);
        // A spanner is as wide as the content box, so it does not cut the rule of an overflow
        // column gap past that box's inline end (multicol-gap-decorations-027).
        let past_content = gap_left >= g.content_x + g.content_w - 0.01;
        if col_joined || past_content {
            // One line through the row gaps and under the spanners.
            if let (Some(first), Some(last)) = ((0..rows).find(|&r| both(r)), (0..rows).rfind(|&r| both(r))) {
                let (a, bm) = line_insets(&s.column_rule_inset, None, None, em, vp);
                // The joined line starts at the spanner above its first visible piece (the reference of
                // multicol-gap-decorations-029 paints it under that spanner).
                let top = if first > 0 && !bands[first - 1].gap_after { bands[first - 1].bottom } else { bands[first].top };
                let bottom = bands[last].bottom;
                if let Some((y, h)) = inset_span(top, bottom - top, a, bm, false) {
                    col_cmds.extend(rule_line_commands(Rect::new(sep_x, y, w, h), false, st, color));
                }
            }
            continue;
        }
        for r in (0..rows).filter(|&r| both(r)) {
            // A row gap above/below is a junction only while a visible row rule runs through it.
            // The row rule meets the column rule only where one of its pieces touches this gap.
            let cross = |gap_row: usize| {
                let touches = if !row_cut {
                    gap_below(gap_row) && row_visible(gap_row)
                } else {
                    row_piece(gap_row, i) || row_piece(gap_row, i + 1)
                };
                touches.then(|| (g.row_gap, row_style(gap_row).0))
            };
            let lo = if r > 0 { cross(r - 1) } else { None };
            let hi = if r + 1 < rows { cross(r) } else { None };
            let (a, bm) = line_insets(&s.column_rule_inset, lo, hi, em, vp);
            // Column pieces keep their `overlap-join` overlap (the reference of
            // multicol-gap-decorations-033 blends the doubled strip twice); row pieces do not.
            if let Some((y, h)) = inset_span(bands[r].top, bands[r].bottom - bands[r].top, a, bm, false) {
                col_cmds.extend(rule_line_commands(Rect::new(sep_x, y, w, h), false, st, color));
            }
        }
    }

    // ── row rules (horizontal lines in the row gaps) ─────────────────────────────────────────
    let mut row_cmds: Vec<DisplayCommand> = Vec::new();
    for r in (0..rows).filter(|&r| gap_below(r)) {
        if !row_visible(r) {
            continue;
        }
        let (w, st, color) = row_style(r);
        let gap_top = bands[r].bottom;
        let sep_y = gap_top + (g.row_gap - w) * 0.5;
        if !row_cut {
            let (a, bm) = line_insets(&s.row_rule_inset, None, None, em, vp);
            if let Some((x, len)) = inset_span(g.content_x, g.content_w, a, bm, rtl) {
                row_cmds.extend(rule_line_commands(Rect::new(x, sep_y, len, w), true, st, color));
            }
            continue;
        }
        // Cut at the column gaps: one piece per column that holds content in either row.
        let mut spans: Vec<(f32, f32)> = Vec::new();
        for c in (0..n).filter(|&c| row_piece(r, c)) {
            let cross = |gap_idx: usize| {
                let touches = col_joined && col_visible(gap_idx) || col_piece(gap_idx, r) || col_piece(gap_idx, r + 1);
                touches.then(|| (g.col_gap, col_style(gap_idx).0))
            };
            let lo = if c > 0 { cross(c - 1) } else { None };
            let hi = if c + 1 < n { cross(c) } else { None };
            // `inset-start` is the inline-start end of the line: the right one under `rtl`.
            let (a, bm) = if rtl { line_insets(&s.row_rule_inset, hi, lo, em, vp) } else { line_insets(&s.row_rule_inset, lo, hi, em, vp) };
            let col_left = g.content_x + c as f32 * (g.col_w + g.col_gap);
            spans.extend(inset_span(col_left, g.col_w, a, bm, rtl));
        }
        for (x, len) in without_overlap(spans) {
            row_cmds.extend(rule_line_commands(Rect::new(x, sep_y, len, w), true, st, color));
        }
    }

    // CSS Gap Decorations L1 §3.5 `rule-overlap`: the axis painted last lies on top.
    match s.rule_overlap {
        RuleOverlap::RowOverColumn => {
            out.extend(col_cmds);
            out.extend(row_cmds);
        }
        RuleOverlap::ColumnOverRow => {
            out.extend(row_cmds);
            out.extend(col_cmds);
        }
    }
}
