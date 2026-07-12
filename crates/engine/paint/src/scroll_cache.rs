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
//! returns a [`ScrollFramePlan`] ([`Blit`], [`BlitAndExpose`], or [`Repaint`]).
//! It holds no GPU state and does no rasterization — the render backend owns the
//! texture and consumes the plan.
//!
//! # Incremental band exposure (M3.1)
//!
//! When the viewport leaves the cached band but the freshly re-centered band
//! still *overlaps* the old one (the common case — one wheel notch past the
//! overscan margin), a full-band repaint would re-raster mostly-unchanged
//! pixels. [`BlitAndExpose`] instead blits the overlapping region from the old
//! surface and rasters only the newly revealed strip(s) — the `expose` rects,
//! which together with the retained overlap tile the new band exactly. A full
//! [`Repaint`] is reserved for the cases with no reusable pixels: an empty
//! cache, a changed content hash, or a scroll jump far enough to leave the old
//! band entirely.
//!
//! [`Blit`]: ScrollFramePlan::Blit
//! [`BlitAndExpose`]: ScrollFramePlan::BlitAndExpose
//! [`Repaint`]: ScrollFramePlan::Repaint
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
/// `BlitAndExpose` reuses the overlap with the old band and rasters only the
/// newly revealed strips; `Repaint` re-rasters the whole viewport-plus-overscan
/// band and re-seats the cache.
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
    /// The viewport left the cached band, but the same content is still valid
    /// and the freshly re-centered band overlaps the old one (ADR-016 M3.1):
    /// blit the `retained` overlap from the *old* surface, then raster only the
    /// `expose` strips into the new surface. `retained` plus the `expose` strips
    /// tile the new band `(origin, size)` exactly, with no overlap. After acting
    /// on it the backend records the new band via
    /// [`ScrollCache::record_repaint`] (same as a `Repaint`).
    BlitAndExpose {
        /// Document-space top-left the new surface maps to.
        origin: (f32, f32),
        /// Document-space extent `(w, h)` the new surface covers — the viewport
        /// grown by `overscan` on every side (same sizing as `Repaint`).
        size: (f32, f32),
        /// Document-space region reusable from the *previous* surface (the
        /// overlap of the old cached band and the new band). Blit it from the
        /// old surface at `retained.origin − prev_origin` to the new surface at
        /// `retained.origin − origin`. Always a sub-rect of `(origin, size)`.
        retained: Rect,
        /// Document-space origin the *previous* surface mapped to, so the
        /// backend can locate `retained` within the old texture.
        prev_origin: (f32, f32),
        /// Up to four document-space strips inside the new band that the old
        /// surface did not cover — raster only these into the new surface (each
        /// at `rect.origin − origin`). `None` slots are unused.
        expose: [Option<Rect>; 4],
    },
    /// The cache is empty, the content changed/resized, or the new viewport left
    /// the old band entirely — re-raster the whole band. The backend rasters a
    /// `size`-sized region of document space starting at `origin` into the
    /// surface, then records it via [`ScrollCache::record_repaint`].
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
    /// Returns [`ScrollFramePlan::Blit`] when the cache holds the same content
    /// hash *and* the visible viewport rect is fully contained in the cached
    /// band; [`ScrollFramePlan::BlitAndExpose`] when the hash still matches and
    /// the re-centered band overlaps the old one (reuse the overlap, raster only
    /// the newly revealed strips); otherwise [`ScrollFramePlan::Repaint`] with a
    /// freshly centered band. Pure: it never mutates the cache — the backend
    /// calls [`record_repaint`](Self::record_repaint) after it acts on a
    /// `BlitAndExpose` or `Repaint`.
    #[must_use]
    pub fn plan(
        &self,
        content_hash: u64,
        scroll: (f32, f32),
        viewport: (f32, f32),
    ) -> ScrollFramePlan {
        let visible = Rect::new(scroll.0, scroll.1, viewport.0.max(0.0), viewport.1.max(0.0));
        let hash_ok = self.hash == Some(content_hash);
        if hash_ok && self.covers(visible) {
            return ScrollFramePlan::Blit {
                src: (scroll.0 - self.origin.0, scroll.1 - self.origin.1),
            };
        }
        let (origin, size) = self.recenter(scroll, viewport);
        // M3.1: the viewport left the band, but if the same content's old band
        // overlaps the new one, reuse that overlap and raster only the exposed
        // strips instead of the whole band. A stale hash or a disjoint jump
        // (no reusable pixels) falls through to a full repaint.
        if hash_ok
            && self.is_populated()
            && let Some(retained) = self.band_overlap(origin, size)
        {
            return ScrollFramePlan::BlitAndExpose {
                origin,
                size,
                retained,
                prev_origin: self.origin,
                expose: Self::subtract_inner(origin, size, retained),
            };
        }
        ScrollFramePlan::Repaint { origin, size }
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

    /// Document-space overlap of the currently cached band with a new band at
    /// `(origin, size)`, or `None` when they are disjoint (zero-area contact
    /// counts as disjoint — there is nothing worth blitting).
    fn band_overlap(&self, origin: (f32, f32), size: (f32, f32)) -> Option<Rect> {
        let (ox, oy) = self.origin;
        let (cw, ch) = self.covered;
        let (nx, ny) = origin;
        let (nw, nh) = size;
        let x0 = ox.max(nx);
        let y0 = oy.max(ny);
        let x1 = (ox + cw).min(nx + nw);
        let y1 = (oy + ch).min(ny + nh);
        if x1 > x0 && y1 > y0 {
            Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
        } else {
            None
        }
    }

    /// Decompose the band at `(origin, size)` minus its sub-rect `inner` into up
    /// to four non-overlapping strips (top, bottom, then the left/right pieces
    /// confined to `inner`'s vertical span). Together with `inner` the strips
    /// tile the band exactly. `inner` must be a sub-rect of the band (guaranteed
    /// here: it is the band∩old-band overlap). Absent strips are `None`; the
    /// consumer flattens the array, so `None` gaps are harmless.
    fn subtract_inner(origin: (f32, f32), size: (f32, f32), inner: Rect) -> [Option<Rect>; 4] {
        let (nx, ny) = origin;
        let (nw, nh) = size;
        let bx1 = nx + nw;
        let by1 = ny + nh;
        let (ix0, iy0, ix1, iy1) = (inner.x, inner.y, inner.right(), inner.bottom());
        [
            // Top strip: full band width, above `inner`.
            (iy0 > ny).then(|| Rect::new(nx, ny, nw, iy0 - ny)),
            // Bottom strip: full band width, below `inner`.
            (by1 > iy1).then(|| Rect::new(nx, iy1, nw, by1 - iy1)),
            // Left strip: within `inner`'s vertical span, left of `inner`.
            (ix0 > nx).then(|| Rect::new(nx, iy0, ix0 - nx, iy1 - iy0)),
            // Right strip: within `inner`'s vertical span, right of `inner`.
            (bx1 > ix1).then(|| Rect::new(ix1, iy0, bx1 - ix1, iy1 - iy0)),
        ]
    }

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
            ScrollFramePlan::Blit { .. } | ScrollFramePlan::BlitAndExpose { .. } => {
                unreachable!("empty cache must repaint")
            }
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

    /// Sum of `retained` + `expose` strip areas must equal the whole band, and
    /// no strip may fall outside the band — the tiling invariant `BlitAndExpose`
    /// relies on. Returns the retained rect for further per-test assertions.
    fn assert_tiles_band(plan: ScrollFramePlan) -> Rect {
        let ScrollFramePlan::BlitAndExpose { origin, size, retained, expose, .. } = plan else {
            panic!("expected BlitAndExpose, got {plan:?}");
        };
        let band = Rect::new(origin.0, origin.1, size.0, size.1);
        let mut area = retained.width * retained.height;
        for strip in expose.into_iter().flatten() {
            // Every strip stays inside the band.
            assert!(
                strip.x >= band.x
                    && strip.y >= band.y
                    && strip.right() <= band.right() + f32::EPSILON
                    && strip.bottom() <= band.bottom() + f32::EPSILON,
                "strip {strip:?} escapes band {band:?}"
            );
            area += strip.width * strip.height;
        }
        assert!(
            (area - band.width * band.height).abs() < 1.0,
            "retained + expose ({area}) must tile band ({})",
            band.width * band.height
        );
        retained
    }

    #[test]
    fn leaving_band_below_exposes_only_bottom_strip() {
        // Band 488..2232; visible 1600..2320 leaves the bottom. The re-centered
        // band 1088..2832 overlaps 1088..2232 → blit that, raster only the new
        // bottom strip 2232..2832.
        let c = populated(512.0, 7, (0.0, 1000.0));
        let plan = c.plan(7, (0.0, 1600.0), (VW, VH));
        assert_eq!(
            plan,
            ScrollFramePlan::BlitAndExpose {
                origin: (0.0, 1088.0),
                size: (VW + 1024.0, VH + 1024.0),
                retained: Rect::new(0.0, 1088.0, VW + 1024.0, 1144.0),
                prev_origin: (0.0, 488.0),
                expose: [
                    None,
                    Some(Rect::new(0.0, 2232.0, VW + 1024.0, 600.0)),
                    None,
                    None,
                ],
            }
        );
        assert_tiles_band(plan);
    }

    #[test]
    fn leaving_band_above_exposes_only_top_strip() {
        // Band starts at 488; scrolling up to visible.y=400 re-centers to
        // origin.y=0 (clamped). Overlap 488..1744 is retained; the top strip
        // 0..488 is the only new raster.
        let c = populated(512.0, 7, (0.0, 1000.0));
        let plan = c.plan(7, (0.0, 400.0), (VW, VH));
        assert_eq!(
            plan,
            ScrollFramePlan::BlitAndExpose {
                origin: (0.0, 0.0),
                size: (VW + 1024.0, VH + 1024.0),
                retained: Rect::new(0.0, 488.0, VW + 1024.0, 1256.0),
                prev_origin: (0.0, 488.0),
                expose: [
                    Some(Rect::new(0.0, 0.0, VW + 1024.0, 488.0)),
                    None,
                    None,
                    None,
                ],
            }
        );
        assert_tiles_band(plan);
    }

    #[test]
    fn leaving_band_horizontally_exposes_only_side_strip() {
        // Seat at (2000, 1000): band x 1488..3536. Pan right to visible x
        // 2800..3824 leaves the band; re-center to x 2288..4336 keeps
        // 2288..3536 and exposes only the right strip 3536..4336.
        let c = populated(512.0, 3, (2000.0, 1000.0));
        let plan = c.plan(3, (2800.0, 1000.0), (VW, VH));
        assert_eq!(
            plan,
            ScrollFramePlan::BlitAndExpose {
                origin: (2288.0, 488.0),
                size: (VW + 1024.0, VH + 1024.0),
                retained: Rect::new(2288.0, 488.0, 1248.0, VH + 1024.0),
                prev_origin: (1488.0, 488.0),
                expose: [
                    None,
                    None,
                    None,
                    Some(Rect::new(3536.0, 488.0, 800.0, VH + 1024.0)),
                ],
            }
        );
        assert_tiles_band(plan);
    }

    #[test]
    fn diagonal_move_exposes_two_strips_and_tiles_band() {
        // Move both down and right out of band: expects a bottom strip and a
        // right strip, and the tiling invariant must hold exactly.
        let c = populated(512.0, 7, (0.0, 1000.0));
        let plan = c.plan(7, (600.0, 1600.0), (VW, VH));
        let retained = assert_tiles_band(plan);
        assert_eq!(retained, Rect::new(88.0, 1088.0, 1960.0, 1144.0));
        let ScrollFramePlan::BlitAndExpose { expose, prev_origin, .. } = plan else {
            unreachable!()
        };
        assert_eq!(prev_origin, (0.0, 488.0));
        let strips: Vec<Rect> = expose.into_iter().flatten().collect();
        assert_eq!(strips.len(), 2, "diagonal exit exposes exactly two strips");
    }

    #[test]
    fn far_jump_below_forces_full_repaint() {
        // Band 488..2232; jump so the new band is disjoint from the old → no
        // reusable pixels → a full repaint, not BlitAndExpose.
        let c = populated(512.0, 7, (0.0, 1000.0));
        let plan = c.plan(7, (0.0, 5000.0), (VW, VH));
        assert_eq!(
            plan,
            ScrollFramePlan::Repaint {
                origin: (0.0, 4488.0),
                size: (VW + 1024.0, VH + 1024.0),
            }
        );
    }

    #[test]
    fn stale_hash_with_overlap_forces_full_repaint() {
        // The old band overlaps the new one, but the content hash changed — the
        // overlapping pixels are stale, so a full repaint is mandatory (never a
        // BlitAndExpose of stale content).
        let c = populated(512.0, 7, (0.0, 1000.0));
        let plan = c.plan(8, (0.0, 1600.0), (VW, VH));
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
    fn zero_overscan_blits_at_seat_exposes_on_step_repaints_on_jump() {
        // No slack band (overscan 0): the band is exactly one viewport.
        let c = populated(0.0, 5, (0.0, 1000.0));
        // Exact seat → pure blit.
        assert_eq!(
            c.plan(5, (0.0, 1000.0), (VW, VH)),
            ScrollFramePlan::Blit { src: (0.0, 0.0) }
        );
        // A 1 px step still overlaps almost the whole band → reuse it, raster
        // only the newly revealed 1 px bottom strip.
        assert_eq!(
            c.plan(5, (0.0, 1001.0), (VW, VH)),
            ScrollFramePlan::BlitAndExpose {
                origin: (0.0, 1001.0),
                size: (VW, VH),
                retained: Rect::new(0.0, 1001.0, VW, VH - 1.0),
                prev_origin: (0.0, 1000.0),
                expose: [
                    None,
                    Some(Rect::new(0.0, 1000.0 + VH, VW, 1.0)),
                    None,
                    None,
                ],
            }
        );
        // A jump of a full viewport leaves nothing to reuse → full repaint.
        assert!(matches!(
            c.plan(5, (0.0, 1000.0 + VH), (VW, VH)),
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
