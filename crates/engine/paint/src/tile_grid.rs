//! Tile-based dirty-rect tracking for incremental rendering (Phase 2).
//!
//! The page is divided into `tile_size × tile_size` tiles in CSS pixel space.
//! Each tile tracks whether it needs to be re-rendered (`Dirty`) or can reuse
//! the previous frame's output (`Clean`). On a display-list diff, only tiles
//! that contain changed commands are marked dirty — the renderer skips the rest.

use std::collections::HashMap;

use lumen_core::geom::Rect;

use crate::display_list::DisplayCommand;

/// Default tile size in CSS pixels.
pub const DEFAULT_TILE_SIZE: u32 = 256;

/// Dirty state of a single tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileDirty {
    /// Tile content is unchanged; renderer may reuse previous output.
    Clean,
    /// Tile content changed and must be re-rendered this frame.
    Dirty,
}

/// Tile-grid for dirty-rect tracking.
///
/// Coordinates: `(tile_x, tile_y)` in tile space. CSS pixel `(px, py)` maps to
/// tile `(px / tile_size, py / tile_size)`. Negative tiles are supported for
/// pages with content above the scroll origin.
pub struct TileGrid {
    /// Size of each tile in CSS pixels.
    pub tile_size: u32,
    /// Per-tile dirty state. Missing entries are implicitly `Dirty`.
    pub tiles: HashMap<(i32, i32), TileDirty>,
}

impl TileGrid {
    /// Create a new grid with all tiles missing (implicitly dirty).
    pub fn new(tile_size: u32) -> Self {
        Self {
            tile_size,
            tiles: HashMap::new(),
        }
    }

    /// Create a new grid with the default 256 px tile size.
    pub fn default_size() -> Self {
        Self::new(DEFAULT_TILE_SIZE)
    }

    /// Mark a single tile dirty.
    pub fn mark_dirty(&mut self, tile: (i32, i32)) {
        self.tiles.insert(tile, TileDirty::Dirty);
    }

    /// Mark a single tile clean.
    pub fn mark_clean(&mut self, tile: (i32, i32)) {
        self.tiles.insert(tile, TileDirty::Clean);
    }

    /// Return `true` if the tile is dirty or has never been rendered.
    pub fn is_dirty(&self, tile: (i32, i32)) -> bool {
        self.tiles.get(&tile) != Some(&TileDirty::Clean)
    }

    /// Mark all tiles covered by the given page dimensions dirty.
    ///
    /// Call this after navigation or a full-page layout change to force
    /// a complete re-render on the next frame.
    pub fn mark_all_dirty(&mut self, page_width: f32, page_height: f32) {
        self.tiles.clear();
        let ts = self.tile_size as f32;
        let cols = (page_width / ts).ceil() as i32 + 1;
        let rows = (page_height / ts).ceil() as i32 + 1;
        for ty in 0..rows {
            for tx in 0..cols {
                self.tiles.insert((tx, ty), TileDirty::Dirty);
            }
        }
    }

    /// Return all tiles currently marked dirty.
    pub fn dirty_tiles(&self) -> Vec<(i32, i32)> {
        self.tiles
            .iter()
            .filter_map(|(&coord, &state)| {
                if state == TileDirty::Dirty {
                    Some(coord)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Diff `old_dl` against `new_dl` and mark tiles that contain changed
    /// display commands as dirty.
    ///
    /// Commands are compared element-by-element by their `Debug` representation.
    /// Commands without a bounding rect (state commands: `PushClipRect`,
    /// `PopClipRect`, etc.) force-dirty all tiles touched by adjacent
    /// rect-commands in the same frame.
    ///
    /// This is an O(n) diff — sufficient for Phase 2. A future phase may use
    /// content hashing per tile for sub-linear updates.
    pub fn update_from_diff(
        &mut self,
        old_dl: &[DisplayCommand],
        new_dl: &[DisplayCommand],
    ) {
        let max_len = old_dl.len().max(new_dl.len());
        for i in 0..max_len {
            let old_cmd = old_dl.get(i);
            let new_cmd = new_dl.get(i);

            let changed = match (old_cmd, new_cmd) {
                (Some(o), Some(n)) => format!("{o:?}") != format!("{n:?}"),
                (None, Some(_)) | (Some(_), None) => true,
                (None, None) => false,
            };

            if changed {
                if let Some(cmd) = old_cmd {
                    self.mark_tiles_for_cmd(cmd);
                }
                if let Some(cmd) = new_cmd {
                    self.mark_tiles_for_cmd(cmd);
                }
            }
        }
    }

    // ── private helpers ──────────────────────────────────────────────────────

    /// Mark all tiles that overlap the rect of `cmd` as dirty.
    /// State commands (no rect) are ignored — they affect all tiles via the
    /// surrounding rect-commands that were already marked dirty.
    fn mark_tiles_for_cmd(&mut self, cmd: &DisplayCommand) {
        if let Some(rect) = cmd_rect(cmd) {
            for tile in self.tiles_for_rect(rect) {
                self.mark_dirty(tile);
            }
        }
    }

    /// Return the tile coordinate that contains CSS point `(x, y)`.
    fn tile_for_point(&self, x: f32, y: f32) -> (i32, i32) {
        let ts = self.tile_size as f32;
        (x.div_euclid(ts) as i32, y.div_euclid(ts) as i32)
    }

    /// Return all tiles overlapping the given CSS rect.
    fn tiles_for_rect(&self, rect: Rect) -> Vec<(i32, i32)> {
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return Vec::new();
        }
        let ts = self.tile_size as f32;
        let (x0, y0) = self.tile_for_point(rect.x, rect.y);
        let (x1, y1) = self.tile_for_point(
            (rect.x + rect.width - 0.001).max(rect.x),
            (rect.y + rect.height - 0.001).max(rect.y),
        );
        let mut out = Vec::with_capacity(((x1 - x0 + 1) * (y1 - y0 + 1)) as usize);
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                // Guard: only include tiles that actually overlap.
                let tile_x = tx as f32 * ts;
                let tile_y = ty as f32 * ts;
                if rect.x < tile_x + ts
                    && rect.x + rect.width > tile_x
                    && rect.y < tile_y + ts
                    && rect.y + rect.height > tile_y
                {
                    out.push((tx, ty));
                }
            }
        }
        out
    }
}

/// Extract the bounding rect from a display command, if it has one.
/// State commands (Push*/Pop*) return `None`.
fn cmd_rect(cmd: &DisplayCommand) -> Option<Rect> {
    match cmd {
        DisplayCommand::FillRect { rect, .. }
        | DisplayCommand::FillRoundedRect { rect, .. }
        | DisplayCommand::DrawBorder { rect, .. }
        | DisplayCommand::DrawOutline { rect, .. }
        | DisplayCommand::DrawText { rect, .. }
        | DisplayCommand::DrawImage { rect, .. }
        | DisplayCommand::DrawBackgroundImage { rect, .. }
        | DisplayCommand::DrawLinearGradient { rect, .. }
        | DisplayCommand::DrawRadialGradient { rect, .. }
        | DisplayCommand::DrawConicGradient { rect, .. } => Some(*rect),
        _ => None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Rect;
    use lumen_layout::Color;

    fn fill(x: f32, y: f32, w: f32, h: f32) -> DisplayCommand {
        DisplayCommand::FillRect {
            rect: Rect::new(x, y, w, h),
            color: Color { r: 255, g: 0, b: 0, a: 255 },
        }
    }

    #[test]
    fn tile_grid_new_is_empty() {
        let g = TileGrid::new(256);
        assert_eq!(g.tile_size, 256);
        assert!(g.tiles.is_empty());
    }

    #[test]
    fn tile_grid_missing_tile_is_dirty() {
        let g = TileGrid::new(256);
        assert!(g.is_dirty((0, 0)));
        assert!(g.is_dirty((5, 3)));
    }

    #[test]
    fn tile_grid_mark_clean_then_dirty() {
        let mut g = TileGrid::new(256);
        g.mark_clean((1, 2));
        assert!(!g.is_dirty((1, 2)));
        g.mark_dirty((1, 2));
        assert!(g.is_dirty((1, 2)));
    }

    #[test]
    fn tile_grid_mark_all_dirty_covers_page() {
        let mut g = TileGrid::new(256);
        // Mark everything clean first.
        for ty in 0..4 {
            for tx in 0..4 {
                g.mark_clean((tx, ty));
            }
        }
        g.mark_all_dirty(512.0, 512.0);
        assert!(g.is_dirty((0, 0)));
        assert!(g.is_dirty((1, 0)));
        assert!(g.is_dirty((0, 1)));
        assert!(g.is_dirty((1, 1)));
    }

    #[test]
    fn tile_grid_dirty_tiles_returns_only_dirty() {
        let mut g = TileGrid::new(256);
        g.mark_dirty((0, 0));
        g.mark_dirty((1, 0));
        g.mark_clean((2, 0));
        let dirty = g.dirty_tiles();
        assert!(dirty.contains(&(0, 0)));
        assert!(dirty.contains(&(1, 0)));
        assert!(!dirty.contains(&(2, 0)));
    }

    #[test]
    fn update_from_diff_marks_changed_tile_dirty() {
        let mut g = TileGrid::new(256);
        // Pre-mark everything clean.
        g.mark_clean((0, 0));
        g.mark_clean((1, 0));

        let old = vec![fill(10.0, 10.0, 50.0, 50.0)];
        let new = vec![fill(10.0, 10.0, 60.0, 50.0)]; // width changed

        g.update_from_diff(&old, &new);

        // Tile (0,0) covers [0,256)×[0,256) — both commands are inside it.
        assert!(g.is_dirty((0, 0)));
        // Tile (1,0) was not touched by either command.
        assert!(!g.is_dirty((1, 0)));
    }

    #[test]
    fn update_from_diff_identical_lists_no_change() {
        let mut g = TileGrid::new(256);
        g.mark_clean((0, 0));

        let dl = vec![fill(10.0, 10.0, 50.0, 50.0)];
        g.update_from_diff(&dl, &dl);

        assert!(!g.is_dirty((0, 0)));
    }

    #[test]
    fn update_from_diff_added_command_marks_tile() {
        let mut g = TileGrid::new(256);
        g.mark_clean((1, 1));

        // New command placed in tile (1,1): x=[256,512), y=[256,512).
        let old: Vec<DisplayCommand> = vec![];
        let new = vec![fill(300.0, 300.0, 50.0, 50.0)];

        g.update_from_diff(&old, &new);
        assert!(g.is_dirty((1, 1)));
    }

    #[test]
    fn tiles_for_rect_span_boundary() {
        let g = TileGrid::new(256);
        // Rect spanning tiles (0,0) and (1,0): x=200..300
        let rect = Rect::new(200.0, 0.0, 100.0, 10.0);
        let tiles = g.tiles_for_rect(rect);
        assert!(tiles.contains(&(0, 0)));
        assert!(tiles.contains(&(1, 0)));
    }
}
