//! CSS Grid Layout L2 §9 — Subgrid algorithm.
//!
//! When a grid item has `grid-template-columns: subgrid` or
//! `grid-template-rows: subgrid`, it inherits resolved track sizes from
//! the spanning tracks of its parent grid.  This module provides:
//!
//! * `SubgridContext` — the inherited track sizes/offsets for one axis.
//! * Thread-locals `SUBGRID_COL_CTX` / `SUBGRID_ROW_CTX` — the parent grid
//!   sets them immediately before calling `lay_out` on a subgrid child; the
//!   child's `lay_out_grid` reads and clears them at entry.
//! * `SubgridContextGuard` — RAII helper that clears both thread-locals on drop,
//!   so a panicking layout cannot leave stale state.
//! * `SubgridItem` + `collect_subgrid_items` — public API for P4 wiring.

use std::cell::RefCell;

/// Resolved track sizes and cumulative offsets for one grid axis (columns or rows)
/// inherited from the parent grid's spanning tracks.
///
/// `sizes[i]` is the width (column) or height (row) of the i-th inherited track in pixels.
/// `offsets[i]` is the cumulative start offset of that track relative to the subgrid
/// item's content origin (i.e. `offsets[0] == 0.0`).
#[derive(Debug, Clone)]
pub struct SubgridContext {
    /// Pixel size of each inherited track.
    pub sizes: Vec<f32>,
    /// Cumulative pixel start offset for each track (relative to item origin).
    pub offsets: Vec<f32>,
    /// Gap between tracks (gap already included in offsets; stored for spacing items).
    pub gap: f32,
}

impl SubgridContext {
    /// Build from a slice of parent track sizes and the gap value used between them.
    pub fn from_parent_tracks(sizes: &[f32], gap: f32) -> Self {
        let mut offsets = Vec::with_capacity(sizes.len());
        let mut cursor = 0.0_f32;
        for &s in sizes {
            offsets.push(cursor);
            cursor += s + gap;
        }
        Self { sizes: sizes.to_vec(), offsets, gap }
    }

    /// Total span width/height occupied by all inherited tracks (including inter-track gaps).
    pub fn total_size(&self) -> f32 {
        let n = self.sizes.len();
        if n == 0 {
            return 0.0;
        }
        self.sizes.iter().sum::<f32>() + self.gap * (n - 1) as f32
    }
}

// ── Thread-local subgrid context ─────────────────────────────────────────────

thread_local! {
    /// Column-axis subgrid context set by the parent grid before laying out a
    /// subgrid child.  Cleared immediately when the child's `lay_out_grid` starts.
    pub(crate) static SUBGRID_COL_CTX: RefCell<Option<SubgridContext>> = const { RefCell::new(None) };

    /// Row-axis subgrid context (same lifecycle as `SUBGRID_COL_CTX`).
    pub(crate) static SUBGRID_ROW_CTX: RefCell<Option<SubgridContext>> = const { RefCell::new(None) };
}

/// RAII guard: sets the thread-local subgrid contexts and clears them on drop.
/// The parent `lay_out_grid` creates this guard in the same scope as the call
/// to `lay_out` on the subgrid child; the Rust drop order ensures cleanup even
/// if the child layout panics.
pub(crate) struct SubgridContextGuard;

impl SubgridContextGuard {
    /// Install `col` and `row` contexts (either may be `None` if that axis is
    /// not subgridded).
    pub(crate) fn set(col: Option<SubgridContext>, row: Option<SubgridContext>) -> Self {
        SUBGRID_COL_CTX.with(|c| *c.borrow_mut() = col);
        SUBGRID_ROW_CTX.with(|c| *c.borrow_mut() = row);
        Self
    }
}

impl Drop for SubgridContextGuard {
    fn drop(&mut self) {
        SUBGRID_COL_CTX.with(|c| *c.borrow_mut() = None);
        SUBGRID_ROW_CTX.with(|c| *c.borrow_mut() = None);
    }
}

// ── Public API for P4 wiring ──────────────────────────────────────────────────

/// A grid item that is itself a subgrid container for at least one axis.
///
/// Returned by [`collect_subgrid_items`] for each layout box that has
/// `grid-template-columns: subgrid` or `grid-template-rows: subgrid`.
#[derive(Debug)]
pub struct SubgridItem {
    /// DOM node identifier.
    pub node_id: u32,
    /// True when the column axis is subgridded (`grid-template-columns: subgrid`).
    pub subgrid_columns: bool,
    /// True when the row axis is subgridded (`grid-template-rows: subgrid`).
    pub subgrid_rows: bool,
}

use crate::{LayoutBox, BoxKind};

/// Collect all layout boxes in the tree that are subgrid containers.
///
/// Walks the entire `LayoutBox` tree (depth-first) and returns a
/// `SubgridItem` for every box whose `grid-template-columns` or
/// `grid-template-rows` is the sentinel `[GridTrackSize::Subgrid]`.
/// This list can be used by P4 or the shell to query or debug subgrid state.
pub fn collect_subgrid_items(root: &LayoutBox) -> Vec<SubgridItem> {
    let mut items = Vec::new();
    collect_recursive(root, &mut items);
    items
}

fn collect_recursive(node: &LayoutBox, out: &mut Vec<SubgridItem>) {
    use crate::GridTrackSize;

    if matches!(node.kind, BoxKind::Skip) {
        return;
    }

    let sc = node.style.grid_template_columns.first() == Some(&GridTrackSize::Subgrid);
    let sr = node.style.grid_template_rows.first() == Some(&GridTrackSize::Subgrid);
    if sc || sr {
        out.push(SubgridItem {
            node_id: node.node.index() as u32,
            subgrid_columns: sc,
            subgrid_rows: sr,
        });
    }

    for child in &node.children {
        collect_recursive(child, out);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subgrid_context_offsets() {
        let ctx = SubgridContext::from_parent_tracks(&[100.0, 200.0, 150.0], 10.0);
        assert_eq!(ctx.sizes, vec![100.0, 200.0, 150.0]);
        assert_eq!(ctx.offsets, vec![0.0, 110.0, 320.0]);
    }

    #[test]
    fn subgrid_context_total_size() {
        let ctx = SubgridContext::from_parent_tracks(&[100.0, 200.0], 20.0);
        // 100 + 200 + 20 (one gap) = 320
        assert!((ctx.total_size() - 320.0).abs() < 0.01);
    }

    #[test]
    fn subgrid_context_single_track() {
        let ctx = SubgridContext::from_parent_tracks(&[80.0], 10.0);
        assert_eq!(ctx.offsets, vec![0.0]);
        assert!((ctx.total_size() - 80.0).abs() < 0.01);
    }

    #[test]
    fn subgrid_context_empty() {
        let ctx = SubgridContext::from_parent_tracks(&[], 10.0);
        assert!(ctx.sizes.is_empty());
        assert!(ctx.offsets.is_empty());
        assert!((ctx.total_size() - 0.0).abs() < 0.01);
    }

    #[test]
    fn guard_clears_on_drop() {
        {
            let _g = SubgridContextGuard::set(
                Some(SubgridContext::from_parent_tracks(&[50.0], 0.0)),
                None,
            );
            SUBGRID_COL_CTX.with(|c| assert!(c.borrow().is_some()));
        }
        // After guard drops, context is cleared.
        SUBGRID_COL_CTX.with(|c| assert!(c.borrow().is_none()));
        SUBGRID_ROW_CTX.with(|c| assert!(c.borrow().is_none()));
    }
}
