//! Blit-vs-repaint decision for compositor scroll (ADR-016 M3).
//!
//! M3 turns a scroll-only frame (M0.5's [`FrameDelta::OffsetOnly`]) into a
//! *blit* of a retained content surface instead of a full re-raster. The
//! surface is rastered once over the viewport **plus an overscan margin** above
//! and below; while the viewport stays inside that band a scroll step only has
//! to blit the cached texture shifted by the scroll delta — zero display
//! commands re-executed.
//!
//! This module is the pure *decision brain*: given the retained surface's
//! document-space coverage and the new frame's content hash + scroll offset it
//! returns a [`ScrollFramePlan`] (`Blit` or `Repaint`). It holds no GPU state
//! and does no rasterization — the render backend (M3.1) owns the texture and
//! consumes the plan.
//!
//! # Why an overscan *range* check, not a scroll bucket
//!
//! An earlier experiment (2026-07-10) keyed a full-surface cache on a
//! *quantized* scroll value (20 px buckets). A 120 px wheel step jumped a
//! bucket every frame → cache miss every frame → two full renders per frame,
//! 30× *slower*. The lesson: the cache hit test must be a **containment range**
//! (`visible ⊆ covered`), never an equality/bucket test on the scroll value.
//! Inside the overscan band any sub-pixel scroll is a hit; only leaving the band
//! forces a repaint.
//!
//! [`FrameDelta::OffsetOnly`]: crate::display_list::FrameDelta::OffsetOnly

use lumen_core::geom::Rect;

/// Default overscan margin, in CSS px, rastered above and below the viewport
/// into the content surface. A larger band blits across more scroll travel
/// before a repaint, at the cost of a larger texture and more off-screen raster
/// per repaint. 512 px ≈ two wheel notches of slack in each direction.
pub const DEFAULT_OVERSCAN: f32 = 512.0;

/// What the render backend should do with the current frame, given the retained
/// content surface (ADR-016 M3).
///
/// Produced by [`ScrollCache::plan`]. `Blit` is the fast path (no raster);
/// `Repaint` re-rasters the viewport-plus-overscan band and re-seats the cache.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollFramePlan {
    /// The visible viewport is fully inside the cached band — blit the retained
    /// surface with its top-left shifted by `src` (the document-space offset of
    /// the viewport top-left *within* the cached texture). No rasterization.
    Blit {
        /// Document-space offset `(x, y)` from the cached texture's top-left to
        /// the viewport's top-left, i.e. `scroll - cache.origin`. Always
        /// non-negative on each axis when the plan is `Blit`.
        src: (f32, f32),
    },
    /// The cache is empty, the content changed/resized, or the new viewport is
    /// not fully inside the cached band — re-raster the whole band. The backend
    /// rasters a `size`-sized region of document space starting at `origin`
    /// into the surface, then records it via [`ScrollCache::record_repaint`].
    Repaint {
        /// Document-space top-left the surface should map to after the repaint.
        origin: (f32, f32),
        /// Document-space extent `(w, h)` the surface should cover — the
        /// viewport grown by `overscan` on every side (clamped at the document
        /// origin so no band is wasted above/left of `(0, 0)`).
        size: (f32, f32),
    },
}

/// Bookkeeping for the retained scroll-content surface (ADR-016 M3).
///
/// Tracks *which* document-space region the backend's content texture currently
/// holds and the content hash it was rastered for. Pure state + arithmetic — no
/// GPU handle lives here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollCache {
    /// Overscan margin (CSS px) added on each side of the viewport.
    overscan: f32,
    /// Content hash the surface was last rastered for, or `None` when the cache
    /// is empty (fresh, invalidated, or never painted).
    hash: Option<u64>,
    /// Document-space coordinate that maps to the cached texture's top-left.
    origin: (f32, f32),
    /// Document-space extent `(w, h)` the cached texture covers.
    covered: (f32, f32),
}

impl ScrollCache {
    /// Create an empty cache with the given overscan margin (CSS px). A negative
    /// or non-finite margin is clamped to `0`.
    #[must_use]
    pub fn new(overscan: f32) -> Self {
        Self {
            overscan: if overscan.is_finite() { overscan.max(0.0) } else { 0.0 },
            hash: None,
            origin: (0.0, 0.0),
            covered: (0.0, 0.0),
        }
    }

    /// Create an empty cache with [`DEFAULT_OVERSCAN`].
    #[must_use]
    pub fn default_overscan() -> Self {
        Self::new(DEFAULT_OVERSCAN)
    }

    /// Overscan margin (CSS px) this cache rasters on each side of the viewport.
    #[must_use]
    pub fn overscan(&self) -> f32 {
        self.overscan
    }

    /// `true` when the cache holds a valid rastered band (i.e. a `Blit` is at
    /// least possible for a matching content hash).
    #[must_use]
    pub fn is_populated(&self) -> bool {
        self.hash.is_some()
    }

    /// Drop the retained surface: the next [`plan`](Self::plan) returns
    /// `Repaint`. Call on navigation, backend resize, or any event that makes
    /// the cached pixels meaningless.
    pub fn invalidate(&mut self) {
        self.hash = None;
        self.covered = (0.0, 0.0);
    }

    /// Decide blit-vs-repaint for the frame with page-content hash
    /// `content_hash`, scrolled to `scroll` `(x, y)`, with viewport `(vw, vh)`
    /// in CSS px.
    ///
    /// Returns [`ScrollFramePlan::Blit`] only when the cache holds the same
    /// content hash *and* the visible viewport rect is fully contained in the
    /// cached band; otherwise [`ScrollFramePlan::Repaint`] with a freshly
    /// centered band. Pure: it never mutates the cache — the backend calls
    /// [`record_repaint`](Self::record_repaint) after it actually rasters a
    /// `Repaint`.
    #[must_use]
    pub fn plan(
        &self,
        content_hash: u64,
        scroll: (f32, f32),
        viewport: (f32, f32),
    ) -> ScrollFramePlan {
        let visible = Rect::new(scroll.0, scroll.1, viewport.0.max(0.0), viewport.1.max(0.0));
        if self.hash == Some(content_hash) && self.covers(visible) {
            ScrollFramePlan::Blit {
                src: (scroll.0 - self.origin.0, scroll.1 - self.origin.1),
            }
        } else {
            let (origin, size) = self.recenter(scroll, viewport);
            ScrollFramePlan::Repaint { origin, size }
        }
    }

    /// Record that the backend rastered the surface to cover `size` document
    /// space starting at `origin` for `content_hash`. Must be called after
    /// acting on a [`ScrollFramePlan::Repaint`] so subsequent frames can blit.
    pub fn record_repaint(&mut self, content_hash: u64, origin: (f32, f32), size: (f32, f32)) {
        self.hash = Some(content_hash);
        self.origin = origin;
        self.covered = (size.0.max(0.0), size.1.max(0.0));
    }

    // ── private helpers ──────────────────────────────────────────────────────

    /// `true` when `visible` (document space) lies fully within the cached band.
    fn covers(&self, visible: Rect) -> bool {
        let (ox, oy) = self.origin;
        let (cw, ch) = self.covered;
        visible.x >= ox
            && visible.y >= oy
            && visible.x + visible.width <= ox + cw
            && visible.y + visible.height <= oy + ch
    }

    /// Compute the `(origin, size)` for a repaint that centers `viewport` in the
    /// overscan band, clamping the origin at the document origin `(0, 0)` so the
    /// band is never wasted above/left of the page. The band keeps its full
    /// `viewport + 2·overscan` size even when the origin is clamped, so extra
    /// slack simply lands on the trailing side.
    fn recenter(&self, scroll: (f32, f32), viewport: (f32, f32)) -> ((f32, f32), (f32, f32)) {
        let ov = self.overscan;
        let origin = (
            (scroll.0 - ov).max(0.0),
            (scroll.1 - ov).max(0.0),
        );
        let size = (
            viewport.0.max(0.0) + 2.0 * ov,
            viewport.1.max(0.0) + 2.0 * ov,
        );
        (origin, size)
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const VW: f32 = 1024.0;
    const VH: f32 = 720.0;

    /// Populate a cache the way the backend would after acting on a `Repaint`.
    fn populated(overscan: f32, hash: u64, scroll: (f32, f32)) -> ScrollCache {
        let mut c = ScrollCache::new(overscan);
        match c.plan(hash, scroll, (VW, VH)) {
            ScrollFramePlan::Repaint { origin, size } => c.record_repaint(hash, origin, size),
            ScrollFramePlan::Blit { .. } => unreachable!("empty cache must repaint"),
        }
        c
    }

    #[test]
    fn empty_cache_repaints_and_centers_band() {
        let c = ScrollCache::new(512.0);
        assert!(!c.is_populated());
        // At scroll (0, 1000): origin clamps x to 0, y = 1000 - 512 = 488.
        let plan = c.plan(1, (0.0, 1000.0), (VW, VH));
        assert_eq!(
            plan,
            ScrollFramePlan::Repaint {
                origin: (0.0, 488.0),
                size: (VW + 1024.0, VH + 1024.0),
            }
        );
    }

    #[test]
    fn origin_clamps_at_document_top_left() {
        let c = ScrollCache::new(512.0);
        // Scroll near the top: origin cannot go negative.
        let plan = c.plan(1, (0.0, 100.0), (VW, VH));
        assert_eq!(
            plan,
            ScrollFramePlan::Repaint {
                origin: (0.0, 0.0),
                size: (VW + 1024.0, VH + 1024.0),
            }
        );
    }

    #[test]
    fn scroll_within_band_blits_with_delta() {
        // Cache seated at scroll y=1000 → origin.y = 488, band 488..2232.
        let c = populated(512.0, 7, (0.0, 1000.0));
        assert!(c.is_populated());
        // Scroll down 120 px, viewport 720 → visible 1120..1840, inside 488..2232.
        let plan = c.plan(7, (0.0, 1120.0), (VW, VH));
        assert_eq!(plan, ScrollFramePlan::Blit { src: (0.0, 632.0) });
    }

    #[test]
    fn small_step_that_burned_the_old_experiment_is_a_blit() {
        // The 20 px-bucket experiment missed on every 120 px step; the range
        // check hits as long as the viewport stays inside the band.
        let c = populated(512.0, 7, (0.0, 1000.0));
        for step in 1..=4 {
            let y = 1000.0 + 120.0 * step as f32; // 1120, 1240, 1360, 1480
            match c.plan(7, (0.0, y), (VW, VH)) {
                ScrollFramePlan::Blit { .. } => {}
                other => panic!("step {step} should blit, got {other:?}"),
            }
        }
    }

    #[test]
    fn leaving_band_below_forces_repaint() {
        // Band 488..2232; a visible bottom past 2232 must repaint.
        let c = populated(512.0, 7, (0.0, 1000.0));
        // Scroll so visible = 1600..2320 > 2232.
        let plan = c.plan(7, (0.0, 1600.0), (VW, VH));
        assert_eq!(
            plan,
            ScrollFramePlan::Repaint {
                origin: (0.0, 1088.0),
                size: (VW + 1024.0, VH + 1024.0),
            }
        );
    }

    #[test]
    fn leaving_band_above_forces_repaint() {
        // Band starts at 488; scrolling up so visible.y < 488 must repaint.
        let c = populated(512.0, 7, (0.0, 1000.0));
        let plan = c.plan(7, (0.0, 400.0), (VW, VH));
        assert!(matches!(plan, ScrollFramePlan::Repaint { .. }));
    }

    #[test]
    fn content_change_forces_repaint_even_inside_band() {
        let c = populated(512.0, 7, (0.0, 1000.0));
        // Same scroll, different content hash → cannot blit stale pixels.
        let plan = c.plan(8, (0.0, 1000.0), (VW, VH));
        assert!(matches!(plan, ScrollFramePlan::Repaint { .. }));
    }

    #[test]
    fn record_repaint_reseats_the_band() {
        let mut c = populated(512.0, 7, (0.0, 1000.0));
        // Move far away, repaint, then a nearby scroll blits against the new seat.
        match c.plan(7, (0.0, 5000.0), (VW, VH)) {
            ScrollFramePlan::Repaint { origin, size } => c.record_repaint(7, origin, size),
            other => panic!("expected repaint, got {other:?}"),
        }
        // origin.y = 5000 - 512 = 4488; scroll 5100 → src.y = 612, inside band.
        assert_eq!(
            c.plan(7, (0.0, 5100.0), (VW, VH)),
            ScrollFramePlan::Blit { src: (0.0, 612.0) }
        );
    }

    #[test]
    fn invalidate_forces_repaint() {
        let mut c = populated(512.0, 7, (0.0, 1000.0));
        assert!(matches!(
            c.plan(7, (0.0, 1000.0), (VW, VH)),
            ScrollFramePlan::Blit { .. }
        ));
        c.invalidate();
        assert!(!c.is_populated());
        assert!(matches!(
            c.plan(7, (0.0, 1000.0), (VW, VH)),
            ScrollFramePlan::Repaint { .. }
        ));
    }

    #[test]
    fn horizontal_scroll_inside_band_blits() {
        // Seat at scroll (2000, 1000): origin = (1488, 488).
        let c = populated(512.0, 3, (2000.0, 1000.0));
        // Pan right 200 px: visible x 2200..3224, band x 1488..3536 → inside.
        let plan = c.plan(3, (2200.0, 1000.0), (VW, VH));
        assert_eq!(plan, ScrollFramePlan::Blit { src: (712.0, 512.0) });
    }

    #[test]
    fn zero_overscan_blits_only_at_exact_seat() {
        // No slack: any move leaves the band.
        let c = populated(0.0, 5, (0.0, 1000.0));
        assert_eq!(
            c.plan(5, (0.0, 1000.0), (VW, VH)),
            ScrollFramePlan::Blit { src: (0.0, 0.0) }
        );
        assert!(matches!(
            c.plan(5, (0.0, 1001.0), (VW, VH)),
            ScrollFramePlan::Repaint { .. }
        ));
    }

    #[test]
    fn non_finite_overscan_clamped_to_zero() {
        assert_eq!(ScrollCache::new(f32::NAN).overscan(), 0.0);
        assert_eq!(ScrollCache::new(f32::INFINITY).overscan(), 0.0);
        assert_eq!(ScrollCache::new(-10.0).overscan(), 0.0);
    }
}
