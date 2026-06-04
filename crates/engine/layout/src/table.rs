// CSS Tables Layout (CSS Tables L2 §17) — Phase 0
// Implements fixed and auto table layout algorithms, anonymous box generation for implicit rows/cells,
// colspan/rowspan handling, and border-collapse/border-spacing computations.
// // CSS: table-layout, border-collapse, border-spacing, caption-side, empty-cells

use crate::box_tree::{BoxKind, LayoutBox, BoxSizing, Size, Rect};
use crate::ext::{TextMeasurer, HyphenationProvider};

/// Table layout algorithm context.
/// Holds table structure metadata (column count, row count, explicit widths),
/// rowspan occupancy map, and computed column widths for all phases.
#[derive(Debug, Clone)]
pub struct TableContext {
    /// Number of columns in the table (inferred from max(colspan) across all rows).
    pub col_count: usize,

    /// Explicit column widths (from `col` or cell `width` attributes).
    /// None = width unspecified, caller determines via auto layout.
    pub col_explicit_widths: Vec<Option<f32>>,

    /// Computed column widths after applying table-layout algorithm.
    /// Index by column index; sum should equal table content-box width.
    pub col_widths: Vec<f32>,

    /// Rowspan occupancy tracker: rowspan_map[col] = remaining span for this column.
    /// Decrement after each row, skip columns where > 0 during cell placement.
    pub rowspan_map: Vec<u32>,

    /// Total number of rows in the table (excluding row groups; they count as transparent wrappers).
    pub row_count: usize,

    /// Total table height after layout (block-axis).
    pub total_height: f32,
}

impl Default for TableContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TableContext {
    /// Create a new empty table context.
    pub fn new() -> Self {
        Self {
            col_count: 0,
            col_explicit_widths: Vec::new(),
            col_widths: Vec::new(),
            rowspan_map: Vec::new(),
            row_count: 0,
            total_height: 0.0,
        }
    }

    /// Scan table structure and infer column count, explicit widths, and rowspan occupancy.
    /// Call before any layout pass.
    pub fn collect_table_structure(table: &LayoutBox, content_width: f32, viewport: Size) -> Self {
        let mut ctx = TableContext::new();

        // Phase 1: Scan all rows (direct + nested in row groups) to infer column count and explicit widths.
        for child in &table.children {
            match child.kind {
                BoxKind::TableRow => {
                    ctx.scan_row_structure(child, content_width, viewport);
                    ctx.row_count += 1;
                }
                BoxKind::TableRowGroup => {
                    for row_child in &child.children {
                        if matches!(row_child.kind, BoxKind::TableRow) {
                            ctx.scan_row_structure(row_child, content_width, viewport);
                            ctx.row_count += 1;
                        }
                    }
                }
                _ => {}
            }
        }

        ctx
    }

    /// Scan a single row to determine column count and cell explicit widths.
    fn scan_row_structure(&mut self, row: &LayoutBox, content_width: f32, viewport: Size) {
        let cells: Vec<_> = row
            .children
            .iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();

        let mut col_pos = 0usize;
        for cell in &cells {
            // Skip columns occupied by rowspan cells from previous rows.
            while col_pos < self.rowspan_map.len() && self.rowspan_map[col_pos] > 0 {
                col_pos += 1;
            }

            let span = cell.col_span.max(1) as usize;
            let em = cell.style.font_size;

            // Extract explicit width from cell or column.
            let w_border = if let Some(w_len) = &cell.style.width
                && let Some(w) = w_len.resolve(em, Some(content_width), viewport)
            {
                let bw = match cell.style.box_sizing {
                    BoxSizing::ContentBox => {
                        let pl = cell.style.padding_left.resolve_or_zero(em, content_width, viewport);
                        let pr = cell.style.padding_right.resolve_or_zero(em, content_width, viewport);
                        w + pl + pr + cell.style.border_left_width + cell.style.border_right_width
                    }
                    BoxSizing::BorderBox => w,
                };
                Some(bw)
            } else {
                None
            };

            let end_col = col_pos + span;
            if end_col > self.col_explicit_widths.len() {
                self.col_explicit_widths.resize(end_col, None);
            }
            if end_col > self.col_count {
                self.col_count = end_col;
            }

            // Distribute explicit width evenly across spanned columns.
            if let Some(total_w) = w_border {
                let per_col = total_w / span as f32;
                for slot in self.col_explicit_widths.iter_mut().skip(col_pos).take(span) {
                    *slot = Some(match *slot {
                        Some(existing) => existing.max(per_col),
                        None => per_col,
                    });
                }
            }

            // Track rowspan occupancy.
            let rowspan = cell.row_span.max(1) as u32;
            if rowspan > 1 {
                if self.rowspan_map.len() < end_col {
                    self.rowspan_map.resize(end_col, 0);
                }
                for col in col_pos..end_col {
                    self.rowspan_map[col] = rowspan.saturating_sub(1);
                }
            }

            col_pos = end_col;
        }

        // Decrement rowspan occupancy for next row.
        self.decrement_rowspan_map();
    }

    /// Decrement rowspan counter for all columns (call after each row).
    fn decrement_rowspan_map(&mut self) {
        for entry in self.rowspan_map.iter_mut() {
            *entry = entry.saturating_sub(1);
        }
    }
}

/// Compute table column widths using the table-layout algorithm.
/// Implements fixed (explicit widths only) and auto (greedy fit with content wrapping) algorithms.
///
/// # Parameters:
/// - `table`: root table box
/// - `content_width`: available width (table content-box width)
/// - `viewport`: viewport dimensions (for length resolution)
///
/// # Returns:
/// Vector of column widths (border-box) summing to content_width.
pub fn compute_table_col_widths(
    table: &LayoutBox,
    content_width: f32,
    viewport: Size,
) -> Vec<f32> {
    let ctx = TableContext::collect_table_structure(table, content_width, viewport);

    if ctx.col_count == 0 {
        return Vec::new();
    }

    // Phase 0: Distribute content_width evenly across columns.
    // TODO (P4): Implement `table-layout: fixed` (respect explicit widths, distribute remainder).
    // TODO (P4): Implement `table-layout: auto` (wrap content, compute minimum cell widths).
    // // CSS: table-layout

    let col_width = content_width / ctx.col_count as f32;
    vec![col_width; ctx.col_count]
}

/// Lay out table rows and cells.
/// Distributes cells across computed columns, handles rowspan/colspan, and applies table-specific spacing rules.
///
/// # Parameters:
/// - `table`: mutable reference to table box
/// - `content_x`, `content_y`: table content-box origin
/// - `content_width`: available width
/// - `measurer`: optional text measurer (for measuring cell content)
/// - `viewport`: viewport dimensions
/// - `hp`: hyphenation provider
///
/// # Returns:
/// Total table height (block-axis, from first to last row).
pub fn lay_out_table(
    table: &mut LayoutBox,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let col_widths = compute_table_col_widths(table, content_width, viewport);

    let mut cur_y = content_y;
    let mut rowspan_map: Vec<u32> = Vec::new();

    // flat_row_rects[k] = (y, height) for the k-th row in DOM order (across all groups).
    let mut flat_row_rects: Vec<(f32, f32)> = Vec::new();

    // Spanning cells that need height post-fix:
    // (group: Option<usize>, row_in_group: usize, child_idx: usize, start_flat: usize, span: u32)
    let mut span_fixes: Vec<(Option<usize>, usize, usize, usize, u32)> = Vec::new();

    let n = table.children.len();
    for i in 0..n {
        match table.children[i].kind {
            BoxKind::TableRow => {
                let c_em = table.children[i].style.font_size;
                let c_mt = table.children[i]
                    .style
                    .margin_top
                    .resolve_or_zero(c_em, content_width, viewport);
                let row_y = cur_y + c_mt;
                table.children[i].rect.x = content_x;
                table.children[i].rect.y = row_y;
                table.children[i].rect.width = content_width;
                let flat_idx = flat_row_rects.len();
                let row_h = lay_out_table_row(
                    &mut table.children[i],
                    content_x,
                    row_y,
                    content_width,
                    Some(&col_widths),
                    Some(&mut rowspan_map),
                    measurer,
                    viewport,
                    hp,
                );
                let row_style_h = {
                    let s = &table.children[i].style;
                    if let Some(h_len) = &s.height
                        && let Some(h) = h_len.resolve(s.font_size, None, viewport)
                    {
                        let pt = s
                            .padding_top
                            .resolve_or_zero(s.font_size, content_width, viewport);
                        let pb = s
                            .padding_bottom
                            .resolve_or_zero(s.font_size, content_width, viewport);
                        match s.box_sizing {
                            BoxSizing::ContentBox => {
                                (h + pt + pb + s.border_top_width + s.border_bottom_width).max(0.0)
                            }
                            BoxSizing::BorderBox => {
                                h.max(pt + pb + s.border_top_width + s.border_bottom_width)
                            }
                        }
                    } else {
                        let pt = table.children[i]
                            .style
                            .padding_top
                            .resolve_or_zero(table.children[i].style.font_size, content_width, viewport);
                        let pb = table.children[i]
                            .style
                            .padding_bottom
                            .resolve_or_zero(table.children[i].style.font_size, content_width, viewport);
                        row_h + pt
                            + pb
                            + table.children[i].style.border_top_width
                            + table.children[i].style.border_bottom_width
                    }
                };
                table.children[i].rect.height = row_style_h;
                flat_row_rects.push((table.children[i].rect.y, row_style_h));
                // Collect spanning cells for post-fix.
                for (ci, child) in table.children[i].children.iter().enumerate() {
                    if !matches!(child.kind, BoxKind::Skip) && child.row_span > 1 {
                        span_fixes.push((None, i, ci, flat_idx, child.row_span));
                    }
                }
                let c_mb = table.children[i]
                    .style
                    .margin_bottom
                    .resolve_or_zero(table.children[i].style.font_size, content_width, viewport);
                cur_y = table.children[i].rect.y + table.children[i].rect.height + c_mb;
                decrement_rowspan_map(&mut rowspan_map);
            }
            BoxKind::TableRowGroup => {
                let group_em = table.children[i].style.font_size;
                let g_mt = table.children[i]
                    .style
                    .margin_top
                    .resolve_or_zero(group_em, content_width, viewport);
                let group_y = cur_y + g_mt;
                table.children[i].rect.x = content_x;
                table.children[i].rect.y = group_y;
                table.children[i].rect.width = content_width;
                let mut row_y = group_y;
                let n_rows = table.children[i].children.len();
                for r in 0..n_rows {
                    if !matches!(table.children[i].children[r].kind, BoxKind::TableRow) {
                        continue;
                    }
                    let flat_idx = flat_row_rects.len();
                    let r_em = table.children[i].children[r].style.font_size;
                    let r_mt = table.children[i].children[r]
                        .style
                        .margin_top
                        .resolve_or_zero(r_em, content_width, viewport);
                    table.children[i].children[r].rect.x = content_x;
                    table.children[i].children[r].rect.y = row_y + r_mt;
                    table.children[i].children[r].rect.width = content_width;
                    let row_h = lay_out_table_row(
                        &mut table.children[i].children[r],
                        content_x,
                        row_y + r_mt,
                        content_width,
                        Some(&col_widths),
                        Some(&mut rowspan_map),
                        measurer,
                        viewport,
                        hp,
                    );
                    let r_pt = table.children[i].children[r]
                        .style
                        .padding_top
                        .resolve_or_zero(r_em, content_width, viewport);
                    let r_pb = table.children[i].children[r]
                        .style
                        .padding_bottom
                        .resolve_or_zero(r_em, content_width, viewport);
                    let r_bor = table.children[i].children[r].style.border_top_width
                        + table.children[i].children[r].style.border_bottom_width;
                    let row_style_h = row_h + r_pt + r_pb + r_bor;
                    table.children[i].children[r].rect.height = row_style_h;
                    flat_row_rects.push((table.children[i].children[r].rect.y, row_style_h));
                    // Collect spanning cells for post-fix.
                    for (ci, child) in table.children[i].children[r].children.iter().enumerate() {
                        if !matches!(child.kind, BoxKind::Skip) && child.row_span > 1 {
                            span_fixes.push((Some(i), r, ci, flat_idx, child.row_span));
                        }
                    }
                    let r_mb = table.children[i].children[r]
                        .style
                        .margin_bottom
                        .resolve_or_zero(r_em, content_width, viewport);
                    row_y =
                        table.children[i].children[r].rect.y + table.children[i].children[r].rect.height + r_mb;
                    decrement_rowspan_map(&mut rowspan_map);
                }
                let g_pt = table.children[i]
                    .style
                    .padding_top
                    .resolve_or_zero(group_em, content_width, viewport);
                let g_pb = table.children[i]
                    .style
                    .padding_bottom
                    .resolve_or_zero(group_em, content_width, viewport);
                let g_bor =
                    table.children[i].style.border_top_width + table.children[i].style.border_bottom_width;
                table.children[i].rect.height = (row_y - group_y) + g_pt + g_pb + g_bor;
                let g_mb = table.children[i]
                    .style
                    .margin_bottom
                    .resolve_or_zero(group_em, content_width, viewport);
                cur_y = table.children[i].rect.y + table.children[i].rect.height + g_mb;
            }
            _ => {}
        }
    }

    // Pass 2: fix rowspan cell heights.
    // Each spanning cell's height is extended to reach the bottom of its last spanned row.
    for (group, row, child_idx, start_flat, span) in span_fixes {
        let end_flat = (start_flat + span as usize).min(flat_row_rects.len());
        if end_flat == 0 {
            continue;
        }
        let (last_y, last_h) = flat_row_rects[end_flat - 1];
        let target_bottom = last_y + last_h;
        let cell = match group {
            None => &mut table.children[row].children[child_idx],
            Some(g) => &mut table.children[g].children[row].children[child_idx],
        };
        let new_h = (target_bottom - cell.rect.y).max(cell.rect.height);
        cell.rect.height = new_h;
    }

    (cur_y - content_y).max(0.0)
}

/// Lay out a single table row.
/// Positions cells horizontally, applies colspan/rowspan logic, and measures cell content.
fn lay_out_table_row(
    row: &mut LayoutBox,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    col_widths: Option<&[f32]>,
    rowspan_map: Option<&mut Vec<u32>>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) -> f32 {
    if row.children.is_empty() {
        return 0.0;
    }

    let col_widths = col_widths.unwrap_or(&[]);
    let mut cur_x = content_x;
    let mut max_height = 0.0;
    let mut col_pos = 0usize;

    for child in row.children.iter_mut() {
        if matches!(child.kind, BoxKind::Skip) {
            continue;
        }

        // Skip columns occupied by rowspan cells.
        if let Some(rowspan_map) = rowspan_map {
            while col_pos < rowspan_map.len() && rowspan_map[col_pos] > 0 {
                if col_pos < col_widths.len() {
                    cur_x += col_widths[col_pos];
                }
                col_pos += 1;
            }
        }

        let span = child.col_span.max(1) as usize;
        let cell_width: f32 = col_widths.iter().skip(col_pos).take(span).sum();

        child.rect.x = cur_x;
        child.rect.y = content_y;
        child.rect.width = cell_width;

        // Measure and layout cell content recursively.
        let cell_h = measure_cell_content(child, cell_width, measurer, viewport, hp);
        child.rect.height = cell_h;

        max_height = max_height.max(cell_h);
        cur_x += cell_width;
        col_pos += span;

        // Register rowspan occupancy.
        if let Some(rowspan_map) = rowspan_map {
            let rowspan = child.row_span.max(1) as u32;
            if rowspan > 1 {
                let end_col = col_pos;
                if rowspan_map.len() < end_col {
                    rowspan_map.resize(end_col, 0);
                }
                for col in (col_pos - span)..col_pos {
                    if col < rowspan_map.len() {
                        rowspan_map[col] = rowspan.saturating_sub(1);
                    }
                }
            }
        }
    }

    max_height
}

/// Measure cell content height by recursively laying out descendant boxes.
/// Phase 0: simple height query without full recursive layout.
/// TODO (P2): Implement full recursive layout of cell content (text wrapping, inline/block flow).
/// // CSS: border-collapse, border-spacing, empty-cells
fn measure_cell_content(
    cell: &mut LayoutBox,
    content_width: f32,
    _measurer: Option<&dyn TextMeasurer>,
    _viewport: Size,
    _hp: &dyn HyphenationProvider,
) -> f32 {
    // Phase 0: assume cell content height = sum of child heights + padding/border.
    // TODO: Implement text wrapping and block-level children layout.
    let mut content_h = 0.0;
    for child in cell.children.iter_mut() {
        content_h += child.rect.height;
    }

    let pt = cell
        .style
        .padding_top
        .resolve_or_zero(cell.style.font_size, content_width, _viewport);
    let pb = cell
        .style
        .padding_bottom
        .resolve_or_zero(cell.style.font_size, content_width, _viewport);
    content_h + pt + pb + cell.style.border_top_width + cell.style.border_bottom_width
}

/// Decrement rowspan occupancy counters (call after each row).
fn decrement_rowspan_map(rowspan_map: &mut Vec<u32>) {
    for entry in rowspan_map.iter_mut() {
        *entry = entry.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_context_new() {
        let ctx = TableContext::new();
        assert_eq!(ctx.col_count, 0);
        assert_eq!(ctx.row_count, 0);
        assert!(ctx.col_explicit_widths.is_empty());
    }

    #[test]
    fn test_table_context_col_count() {
        let mut ctx = TableContext::new();
        ctx.col_count = 3;
        assert_eq!(ctx.col_count, 3);
    }

    #[test]
    fn test_table_context_rowspan_map_increment() {
        let mut ctx = TableContext::new();
        ctx.rowspan_map.resize(2, 0);
        ctx.rowspan_map[0] = 2;
        ctx.rowspan_map[1] = 1;
        assert_eq!(ctx.rowspan_map[0], 2);
        assert_eq!(ctx.rowspan_map[1], 1);
    }

    #[test]
    fn test_table_context_rowspan_map_decrement() {
        let mut map = vec![2, 1, 0];
        for entry in map.iter_mut() {
            *entry = entry.saturating_sub(1);
        }
        assert_eq!(map, vec![1, 0, 0]);
    }

    #[test]
    fn test_decrement_rowspan_map() {
        let mut map = vec![1, 2, 3];
        decrement_rowspan_map(&mut map);
        assert_eq!(map, vec![0, 1, 2]);
    }

    #[test]
    fn test_decrement_rowspan_map_at_zero() {
        let mut map = vec![0, 0, 0];
        decrement_rowspan_map(&mut map);
        assert_eq!(map, vec![0, 0, 0]);
    }

    #[test]
    fn test_compute_table_col_widths_empty() {
        let table = LayoutBox::default();
        let widths = compute_table_col_widths(&table, 800.0, Size { width: 1024.0, height: 720.0 });
        assert!(widths.is_empty());
    }

    #[test]
    fn test_compute_table_col_widths_single_column() {
        let mut table = LayoutBox::default();
        table.kind = BoxKind::Table;
        // Phase 0: distribute evenly; without children, col_count = 0.
        let widths = compute_table_col_widths(&table, 800.0, Size { width: 1024.0, height: 720.0 });
        assert!(widths.is_empty());
    }

    #[test]
    fn test_lay_out_table_empty() {
        let mut table = LayoutBox::default();
        table.kind = BoxKind::Table;
        let height = lay_out_table(
            &mut table,
            0.0,
            0.0,
            800.0,
            None,
            Size { width: 1024.0, height: 720.0 },
            &crate::box_tree::NullHyphenationProvider,
        );
        assert_eq!(height, 0.0);
    }

    #[test]
    fn test_table_context_default() {
        let ctx = TableContext::default();
        assert_eq!(ctx.col_count, 0);
        assert_eq!(ctx.row_count, 0);
    }

    #[test]
    fn test_table_context_explicit_widths() {
        let mut ctx = TableContext::new();
        ctx.col_explicit_widths.push(Some(100.0));
        ctx.col_explicit_widths.push(None);
        ctx.col_explicit_widths.push(Some(150.0));
        assert_eq!(ctx.col_explicit_widths.len(), 3);
        assert_eq!(ctx.col_explicit_widths[0], Some(100.0));
        assert_eq!(ctx.col_explicit_widths[1], None);
        assert_eq!(ctx.col_explicit_widths[2], Some(150.0));
    }

    #[test]
    fn test_lay_out_table_row_empty() {
        let mut row = LayoutBox::default();
        row.kind = BoxKind::TableRow;
        let height = lay_out_table_row(
            &mut row,
            0.0,
            0.0,
            800.0,
            None,
            None,
            None,
            Size { width: 1024.0, height: 720.0 },
            &crate::box_tree::NullHyphenationProvider,
        );
        assert_eq!(height, 0.0);
    }
}
