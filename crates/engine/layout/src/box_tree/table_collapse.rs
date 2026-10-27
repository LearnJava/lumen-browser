//! CSS 2.1 §17.6.2.1 — border conflict resolution of the collapsing border model.
//!
//! Layout pulls neighbouring cells together by the width of their shared grid line, and paint
//! lets every cell draw its own four borders. For the two to agree both cells meeting at an edge
//! must carry the *same* border: the winner of the conflict. [`resolve_collapsed_borders`]
//! computes that winner for every cell side and writes it into the cell styles before layout:
//!
//! 1. `border-style: hidden` beats every other border on the edge, and the edge draws nothing;
//! 2. otherwise the wider border wins (`none` is zero-wide, so it loses to anything visible);
//! 3. at equal width the style decides: `double > solid > dashed > dotted > ridge > outset >
//!    groove > inset`;
//! 4. a full tie goes to the border nearer the table's start (the cell above / to the left).
//!
//! Only cells and the table box take part. Borders of rows, row groups and columns are not
//! resolved (their sides are not painted in collapse mode either), and the table's own border
//! is still painted whole: a `hidden` cell does not cut it.

use super::*;
use crate::style::{BorderStyle, Color, CssColor, Direction};

/// One resolved side of a border.
#[derive(Clone, Copy)]
struct Edge {
    style: BorderStyle,
    width: f32,
    color: Color,
}

#[derive(Clone, Copy)]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

const SIDES: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

fn edge_of(s: &ComputedStyle, side: Side) -> Edge {
    let (style, width, color) = match side {
        Side::Top => (s.border_top_style, s.border_top_width, s.border_top_color),
        Side::Right => (s.border_right_style, s.border_right_width, s.border_right_color),
        Side::Bottom => (s.border_bottom_style, s.border_bottom_width, s.border_bottom_color),
        Side::Left => (s.border_left_style, s.border_left_width, s.border_left_color),
    };
    Edge { style, width, color: color.resolve(s.color) }
}

fn set_edge(s: &mut ComputedStyle, side: Side, e: Edge) {
    let color = CssColor::Rgba(e.color);
    match side {
        Side::Top => (s.border_top_style, s.border_top_width, s.border_top_color) = (e.style, e.width, color),
        Side::Right => (s.border_right_style, s.border_right_width, s.border_right_color) = (e.style, e.width, color),
        Side::Bottom => (s.border_bottom_style, s.border_bottom_width, s.border_bottom_color) = (e.style, e.width, color),
        Side::Left => (s.border_left_style, s.border_left_width, s.border_left_color) = (e.style, e.width, color),
    }
}

/// Style strength at equal width (CSS 2.1 §17.6.2.1); `none`/`hidden` rank lowest.
fn style_rank(s: BorderStyle) -> u8 {
    match s {
        BorderStyle::Double => 8,
        BorderStyle::Solid => 7,
        BorderStyle::Dashed => 6,
        BorderStyle::Dotted => 5,
        BorderStyle::Ridge => 4,
        BorderStyle::Outset => 3,
        BorderStyle::Groove => 2,
        BorderStyle::Inset => 1,
        BorderStyle::None | BorderStyle::Hidden => 0,
    }
}

/// True when `a` strictly beats `b` (a full tie is not a win).
fn beats(a: Edge, b: Edge) -> bool {
    let (a_hidden, b_hidden) = (a.style == BorderStyle::Hidden, b.style == BorderStyle::Hidden);
    if a_hidden || b_hidden {
        return a_hidden && !b_hidden;
    }
    if (a.width - b.width).abs() > 1e-4 {
        return a.width > b.width;
    }
    style_rank(a.style) > style_rank(b.style)
}

/// A cell's place in the table grid.
struct Placed {
    /// Index among the table's rows (row groups flattened).
    row: usize,
    /// Index in the row's `children`.
    child: usize,
    r0: usize,
    r1: usize,
    c0: usize,
    c1: usize,
}

fn collect_rows_mut<'a>(b: &'a mut LayoutBox, out: &mut Vec<&'a mut LayoutBox>) {
    for c in b.children.iter_mut() {
        if matches!(c.kind, BoxKind::TableRow) {
            out.push(c);
        } else if matches!(c.kind, BoxKind::TableRowGroup) {
            for r in c.children.iter_mut() {
                if matches!(r.kind, BoxKind::TableRow) {
                    out.push(r);
                }
            }
        }
    }
}

/// Maps every cell to its rows and columns, honouring `colspan`/`rowspan` the way
/// `build_row_init` does.
fn place_cells(rows: &[&mut LayoutBox]) -> (Vec<Placed>, usize) {
    let n_rows = rows.len();
    let mut placed = Vec::new();
    let mut occupied: Vec<u32> = Vec::new();
    let mut n_cols = 0;
    for (r, row) in rows.iter().enumerate() {
        let mut col = 0;
        for (child, cell) in row.children.iter().enumerate() {
            if matches!(cell.kind, BoxKind::Skip) {
                continue;
            }
            while col < occupied.len() && occupied[col] > 0 {
                col += 1;
            }
            let span = cell.col_span.max(1) as usize;
            let rs = cell.row_span.max(1);
            if rs > 1 {
                if col + span > occupied.len() {
                    occupied.resize(col + span, 0);
                }
                for v in &mut occupied[col..col + span] {
                    *v = (*v).max(rs);
                }
            }
            placed.push(Placed { row: r, child, r0: r, r1: (r + rs as usize).min(n_rows), c0: col, c1: col + span });
            n_cols = n_cols.max(col + span);
            col += span;
        }
        for v in &mut occupied {
            *v = v.saturating_sub(1);
        }
    }
    (placed, n_cols)
}

/// Rewrites the border of every cell side of a `border-collapse: collapse` table to the winner of
/// the conflict on that side's grid line (see the module doc).
pub(super) fn resolve_collapsed_borders(table: &mut LayoutBox) {
    let table_style = Arc::clone(&table.style);
    if table_style.direction == Direction::Rtl {
        return; // columns are mirrored after layout, so physical left/right are not grid order
    }
    let mut rows = Vec::new();
    collect_rows_mut(table, &mut rows);
    let (placed, n_cols) = place_cells(&rows);
    if placed.is_empty() {
        return;
    }
    let n_rows = rows.len();
    let mut grid = vec![vec![usize::MAX; n_cols]; n_rows];
    for (i, p) in placed.iter().enumerate() {
        for row in &mut grid[p.r0..p.r1] {
            for slot in &mut row[p.c0..p.c1] {
                *slot = i;
            }
        }
    }
    let own: Vec<[Edge; 4]> = placed
        .iter()
        .map(|p| SIDES.map(|side| edge_of(&rows[p.row].children[p.child].style, side)))
        .collect();

    let mut resolved = own.clone();
    for (i, p) in placed.iter().enumerate() {
        for (k, side) in SIDES.into_iter().enumerate() {
            let me = own[i][k];
            // (neighbour across the edge, table edge when the cell sits on the table's outer edge,
            //  index in the neighbour's `own` of the side facing back, true when the neighbour comes first)
            let (neighbour, table_side, back, neighbour_first) = match side {
                Side::Top if p.r0 > 0 => (grid[p.r0 - 1][p.c0], Side::Top, 2, true),
                Side::Bottom if p.r1 < n_rows => (grid[p.r1][p.c0], Side::Bottom, 0, false),
                Side::Left if p.c0 > 0 => (grid[p.r0][p.c0 - 1], Side::Left, 1, true),
                Side::Right if p.c1 < n_cols => (grid[p.r0][p.c1], Side::Right, 3, false),
                _ => (usize::MAX, side, 0, false),
            };
            if neighbour == usize::MAX && !is_outer(side, p, n_rows, n_cols) {
                continue; // a hole in the grid: nothing to resolve against
            }
            if neighbour == usize::MAX {
                // Outer edge: the table border competes. When it strictly wins the cell stops
                // drawing the side and the table's own border shows.
                if beats(edge_of(&table_style, table_side), me) {
                    resolved[i][k] = Edge { style: BorderStyle::None, width: 0.0, color: me.color };
                }
                continue;
            }
            let other = own[neighbour][back];
            let (first, second) = if neighbour_first { (other, me) } else { (me, other) };
            resolved[i][k] = if beats(second, first) { second } else { first };
        }
    }

    for (i, p) in placed.iter().enumerate() {
        let mut changed = false;
        for k in 0..4 {
            let (a, b) = (own[i][k], resolved[i][k]);
            changed |= a.style != b.style || (a.width - b.width).abs() > 1e-4 || a.color != b.color;
        }
        if !changed {
            continue;
        }
        let style = Arc::make_mut(&mut rows[p.row].children[p.child].style);
        for (k, side) in SIDES.into_iter().enumerate() {
            set_edge(style, side, resolved[i][k]);
        }
    }
}

fn is_outer(side: Side, p: &Placed, n_rows: usize, n_cols: usize) -> bool {
    match side {
        Side::Top => p.r0 == 0,
        Side::Bottom => p.r1 >= n_rows,
        Side::Left => p.c0 == 0,
        Side::Right => p.c1 >= n_cols,
    }
}
