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
//! Limits: `direction: rtl`/`writing-mode` do not mirror the axes; a `column-span: all` child is not
//! understood (the caller falls back to the one-row painter when it sees a full-width fragment).

use lumen_core::geom::{Rect, Size};
use lumen_layout::{BoxKind, ComputedStyle, LayoutBox, Position, RuleBreak, RuleInset, RuleOverlap, RuleVisibilityItems};

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

/// Which `(row, column)` cells of the container hold a fragment, read back from the laid-out
/// children. `None` when a full-width fragment (a `column-span: all` box) is present.
fn occupied_cells(b: &LayoutBox, g: &MulticolGeom) -> Option<Vec<Vec<bool>>> {
    let step = g.col_w + g.col_gap;
    let pitch = g.col_h + g.row_gap;
    let mut cells: Vec<Vec<bool>> = Vec::new();
    for c in b
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip) && !matches!(c.style.position, Position::Absolute | Position::Fixed))
    {
        if c.rect.width > g.col_w + 1.0 && g.n_cols > 1 {
            return None;
        }
        if c.rect.width <= 0.0 && c.rect.height <= 0.0 {
            continue;
        }
        let col = (((c.rect.x - g.content_x) / step).round().max(0.0) as usize).min(g.n_cols as usize - 1);
        let row = ((c.rect.y - g.content_y + 0.01) / pitch).floor().max(0.0) as usize;
        while cells.len() <= row {
            cells.push(vec![false; g.n_cols as usize]);
        }
        cells[row][col] = true;
    }
    Some(cells)
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
    let s = &b.style;
    let Some(cells) = occupied_cells(b, g) else {
        return false;
    };
    let rows = cells.len();
    if rows == 0 {
        return true;
    }
    let em = s.font_size;
    let vp = Size::new(g.content_w, content_h);
    let n = g.n_cols as usize;
    let (col_total, row_total) = (n.saturating_sub(1), rows - 1);
    let row_top = |r: usize| g.content_y + r as f32 * (g.col_h + g.row_gap);

    let col_style = |i: usize| {
        (
            *s.column_rule_width.value_for_gap(i, col_total),
            *s.column_rule_style.value_for_gap(i, col_total),
            s.column_rule_color.value_for_gap(i, col_total).resolve(s.color),
        )
    };
    let row_style = |r: usize| {
        (
            *s.row_rule_width.value_for_gap(r, row_total),
            *s.row_rule_style.value_for_gap(r, row_total),
            s.row_rule_color.value_for_gap(r, row_total).resolve(s.color),
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

    // Is the column-gap-`i` piece of row `r` / the row-gap-`r` piece of column `c` painted?
    let col_piece = |i: usize, r: usize| {
        col_visible(i) && piece_shown(s.column_rule_visibility_items, true, cells[r][i], cells[r][i + 1])
    };
    let row_piece = |r: usize, c: usize| {
        row_visible(r)
            && piece_shown(s.row_rule_visibility_items, false, cells[r][c], cells[r + 1][c])
            && (cells[r][c] || cells[r + 1][c] || s.row_rule_visibility_items == RuleVisibilityItems::All)
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
        if col_joined {
            let (Some(first), Some(last)) = ((0..rows).find(|&r| both(r)), (0..rows).rfind(|&r| both(r))) else {
                continue;
            };
            let (a, bm) = line_insets(&s.column_rule_inset, None, None, em, vp);
            if let Some((y, h)) = inset_span(row_top(first), row_top(last) + g.col_h - row_top(first), a, bm, false) {
                col_cmds.extend(rule_line_commands(Rect::new(sep_x, y, w, h), false, st, color));
            }
            continue;
        }
        for r in (0..rows).filter(|&r| both(r)) {
            // A row gap above/below is a junction only while a visible row rule runs through it.
            // The row rule meets the column rule only where one of its pieces touches this gap.
            let cross = |gap_idx: usize| {
                let touches = if !row_cut { row_visible(gap_idx) } else { row_piece(gap_idx, i) || row_piece(gap_idx, i + 1) };
                touches.then(|| (g.row_gap, row_style(gap_idx).0))
            };
            let lo = if r > 0 { cross(r - 1) } else { None };
            let hi = if r + 1 < rows { cross(r) } else { None };
            let (a, bm) = line_insets(&s.column_rule_inset, lo, hi, em, vp);
            if let Some((y, h)) = inset_span(row_top(r), g.col_h, a, bm, false) {
                col_cmds.extend(rule_line_commands(Rect::new(sep_x, y, w, h), false, st, color));
            }
        }
    }

    // ── row rules (horizontal lines in the row gaps) ─────────────────────────────────────────
    let mut row_cmds: Vec<DisplayCommand> = Vec::new();
    for r in 0..row_total {
        if !row_visible(r) {
            continue;
        }
        let (w, st, color) = row_style(r);
        let gap_top = row_top(r) + g.col_h;
        let sep_y = gap_top + (g.row_gap - w) * 0.5;
        if !row_cut {
            let (a, bm) = line_insets(&s.row_rule_inset, None, None, em, vp);
            if let Some((x, len)) = inset_span(g.content_x, g.content_w, a, bm, false) {
                row_cmds.extend(rule_line_commands(Rect::new(x, sep_y, len, w), true, st, color));
            }
            continue;
        }
        // Cut at the column gaps: one piece per column that holds content in either row.
        for c in (0..n).filter(|&c| row_piece(r, c)) {
            let cross = |gap_idx: usize| {
                let touches = col_joined && col_visible(gap_idx) || col_piece(gap_idx, r) || col_piece(gap_idx, r + 1);
                touches.then(|| (g.col_gap, col_style(gap_idx).0))
            };
            let lo = if c > 0 { cross(c - 1) } else { None };
            let hi = if c + 1 < n { cross(c) } else { None };
            let (a, bm) = line_insets(&s.row_rule_inset, lo, hi, em, vp);
            let col_left = g.content_x + c as f32 * (g.col_w + g.col_gap);
            if let Some((x, len)) = inset_span(col_left, g.col_w, a, bm, false) {
                row_cmds.extend(rule_line_commands(Rect::new(x, sep_y, len, w), true, st, color));
            }
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
    true
}
