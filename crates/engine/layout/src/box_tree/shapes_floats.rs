//! Recursive box-shift helpers (`shift_y_box`/`shift_tree`) + CSS Shapes L1
//! `shape-outside` value parsers (circle/polygon/ellipse/inset/path) + CSS 2.1
//! §9.5 float-context tracking (`FloatContext`) + polygon-edge scan helpers.
//!
//! Перенесено батчем SPLIT-BT11 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `fn shift_y_box` до конца оставшегося региона, перед `mod bfc;`)
//! без правок тел.

use super::*;
use super::shape_resolve::Prim;

/// Смещает rect.y всего поддерева на dy (для vertical-align) — explicit
/// heap-stack pre-order walk (LAYOUT-2 срез 2), not native recursion: nothing
/// after the loop over `children` reads a value the walk produced, so this is
/// the same safe mechanical class LAYOUT-1 already converted
/// (`collect_layout_rects_rec` and siblings) — found independently while
/// tracing `lay_out_flex`'s cross-axis alignment call sites.
///
/// BUG-424 (в): `svg_paint_matrix` (document-space CTM for rotated/skewed SVG
/// shapes, `lay_out_svg_element_position`) bakes in the viewport origin at the
/// time it was computed. When flex/grid cross-axis alignment (`AlignValue::
/// Center`/`End` in `lay_out_flex`) relocates an already-laid-out SVG subtree
/// by patching `rect.y` instead of re-running SVG layout, the matrix used to
/// silently keep the stale origin — `rect` (used by the axis-aligned fast
/// path) moved, the CTM (used only when `has_rot_skew`) did not, drifting the
/// two out of sync by exactly this shift. Translating the matrix in lockstep
/// keeps both representations of the same box consistent.
pub(crate) fn shift_y_box(b: &mut LayoutBox, dy: f32) {
    let mut stack: Vec<&mut LayoutBox> = vec![b];
    while let Some(node) = stack.pop() {
        node.rect.y += dy;
        if let BoxKind::SvgShape { svg_paint_matrix, .. } = &mut node.kind {
            svg_paint_matrix.matrix[5] += dy;
        }
        stack.extend(node.children.iter_mut());
    }
}

/// Смещает rect всего поддерева на (dx, dy) — explicit heap-stack pre-order
/// walk, same conversion and rationale as [`shift_y_box`]. Используется при
/// позиционировании абсолютных потомков.
///
/// BUG-424 (в): keeps `svg_paint_matrix` in sync with `rect` — see
/// `shift_y_box` for why this matters.
pub(crate) fn shift_tree(b: &mut LayoutBox, dx: f32, dy: f32) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    let mut stack: Vec<&mut LayoutBox> = vec![b];
    while let Some(node) = stack.pop() {
        node.rect.x += dx;
        node.rect.y += dy;
        if let BoxKind::SvgShape { svg_paint_matrix, svg_mask, .. } = &mut node.kind {
            svg_paint_matrix.matrix[4] += dx;
            svg_paint_matrix.matrix[5] += dy;
            // The `<mask>` content lives outside `children` but is laid out in
            // document space alongside the shape (`lay_out_svg_children_positions`),
            // so it must travel with it (BUG-341 S42: a replayed probe left it
            // at the probe's origin, detached from its own mask layer).
            if let Some(mask) = svg_mask {
                stack.extend(mask.content.iter_mut());
            }
        }
        stack.extend(node.children.iter_mut());
    }
}

// ─── CSS 2.1 §9.5 — Float context ────────────────────────────────────────────

/// CSS Shapes L1 §4 — parse `path([<fill-rule>,]? "<svg-path>")`.
/// Flattens the SVG path `d` string into a vertex list in float-local
/// (reference-box-relative) px coordinates via [`crate::motion_path::flatten_path_to_polygon`].
/// The optional `<fill-rule>` (nonzero | evenodd) is accepted but ignored — float
/// wrapping uses the filled outline regardless. The `d` string must be quoted
/// (`"…"` or `'…'`); its letter case is preserved (SVG commands are case-sensitive).
/// `path()` coordinates are always px (no percentages per spec). Returns `None`
/// for any unknown syntax or a degenerate (< 3 vertices) outline.
pub(crate) fn parse_shape_path_px(s: &str) -> Option<Vec<(f32, f32)>> {
    let s = s.trim();
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    if close <= open {
        return None;
    }
    // Only the function name is case-folded; the inner `d` string keeps its case.
    if !s[..open].trim().eq_ignore_ascii_case("path") {
        return None;
    }
    let inner = s[open + 1..close].trim();
    // Strip an optional leading `<fill-rule>,` (ignored for wrapping geometry).
    let inner = match inner.split_once(',') {
        Some((head, rest))
            if head.trim().eq_ignore_ascii_case("nonzero")
                || head.trim().eq_ignore_ascii_case("evenodd") =>
        {
            rest.trim()
        }
        _ => inner,
    };
    let path_str = inner
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .or_else(|| inner.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')))?;
    let pts = crate::motion_path::flatten_path_to_polygon(path_str);
    if pts.len() >= 3 { Some(pts) } else { None }
}

/// One piece of a vertical-mode float's `shape-outside` contour in the float
/// context's frame (`x` = inline axis, `y` = block axis), see [`super::shape_resolve`].
#[derive(Clone)]
pub(crate) struct GenShape {
    pub(crate) prim: Prim,
    /// `true` = left (line-left) float, `false` = right.
    pub(crate) is_left: bool,
    /// Block range of the float's margin box: the contour acts only inside it.
    pub(crate) top_y: f32,
    pub(crate) bottom_y: f32,
    /// Inline range of the margin box: the wrap edge is clamped to it.
    pub(crate) bound_lo: f32,
    pub(crate) bound_hi: f32,
    /// `shape-margin` in px.
    pub(crate) margin: f32,
}

impl GenShape {
    fn moved(&self, dx: f32, dy: f32) -> GenShape {
        let mut g = self.clone();
        g.prim.translate(dx, dy);
        g.top_y += dy;
        g.bottom_y += dy;
        g.bound_lo += dx;
        g.bound_hi += dx;
        g
    }
}

/// CSS 2.1 §9.5 — tracks float placements within a single block formatting
/// context.  Simplified Phase-0 implementation: only axis-aligned rectangles,
/// no shape-outside wrapping.  All coordinates are in the same space as the
/// block container's content area (i.e. not relative to viewport).
#[derive(Clone)]
pub(crate) struct FloatContext {
    /// Left floats: `(bottom_y, right_edge)` — right edge of the float margin
    /// box in content-area coordinates.  Active while `bottom_y > query_y`.
    pub(crate) left: Vec<(f32, f32)>,
    /// Right floats: `(bottom_y, left_edge)` — left edge of the float margin
    /// box.  Active while `bottom_y > query_y`.
    pub(crate) right: Vec<(f32, f32)>,
    /// Margin-box top of each entry of [`Self::left`] (parallel vector). Only
    /// the normal-flow queries `*_edge_in_flow` read it: a float that was
    /// pushed below the current line (`clear`, rule 8) does not affect
    /// content that sits entirely above it.
    left_tops: Vec<f32>,
    /// Same as [`Self::left_tops`], for [`Self::right`].
    right_tops: Vec<f32>,
    /// `shape-outside` contours of the floats (CSS Shapes L1), already in this
    /// context's logical frame, so they survive a change of frame
    /// ([`Self::rebased_block`] and friends).
    shape_prims: Vec<GenShape>,
    /// Indices into [`Self::left`] / [`Self::right`] of floats that have a contour
    /// in [`Self::shape_prims`]. Their rectangular margin-box entry stays (it still
    /// bounds float enclosure, `clear` and single-band consumers) but a per-line
    /// query ([`Self::line_band`]) follows the contour instead of the rectangle.
    gen_left: Vec<usize>,
    gen_right: Vec<usize>,
    /// CSS 2.1 §9.5 — floats belonging to an *enclosing* block formatting
    /// context, inherited by a non-BFC child so its line boxes are shortened by
    /// the parent's floats (the child does not own them: they are excluded from
    /// this context's height enclosure and float placement). Coordinates are
    /// absolute (same space as the owned floats). Chains through nesting levels.
    inherited: Option<Box<FloatContext>>,
}

impl FloatContext {
    pub(crate) fn new() -> Self {
        Self {
            left: Vec::new(),
            right: Vec::new(),
            left_tops: Vec::new(),
            right_tops: Vec::new(),
            shape_prims: Vec::new(),
            gen_left: Vec::new(),
            gen_right: Vec::new(),
            inherited: None,
        }
    }

    /// CSS 2.1 §9.5 — a fresh context for a non-BFC child that inherits all
    /// floats currently visible in `parent` (the parent's own floats *and* any
    /// the parent itself inherited). The child adds its own floats to the empty
    /// owned buckets; queries (`left_edge_at`/`clear_y`/…) see both via the
    /// `inherited` chain. Coordinates are absolute, so no translation is needed.
    pub(crate) fn inheriting(parent: &FloatContext) -> Self {
        let mut c = Self::new();
        c.inherited = Some(Box::new(parent.clone()));
        c
    }

    /// Left boundary of available inline space at `y` (= rightmost right-edge
    /// of all left floats whose `bottom_y > y`).  Falls back to `default_x`.
    pub(crate) fn left_edge_at(&self, y: f32, default_x: f32) -> f32 {
        self.left_edge_impl(y, default_x, false, false)
    }

    /// [`Self::left_edge_at`] for a normal-flow box whose top is `y`: floats
    /// whose margin box starts below `y` are ignored (CSS 2.1 §9.5 — a box
    /// is only displaced by floats it can actually meet).
    pub(crate) fn left_edge_in_flow(&self, y: f32, default_x: f32) -> f32 {
        self.left_edge_impl(y, default_x, false, true)
    }

    /// [`Self::left_edge_at`]; with `exact` a float that has a `shape-outside`
    /// contributes its contour only, not its rectangular margin box.
    fn left_edge_impl(&self, y: f32, default_x: f32, exact: bool, in_flow: bool) -> f32 {
        let rect_edge = self.left
            .iter()
            .enumerate()
            .filter(|(i, (bot, _))| {
                *bot > y
                    && !(in_flow && self.left_tops.get(*i).is_some_and(|t| *t > y + 0.01))
                    && !(exact && (self.gen_left.contains(i)))
            })
            .map(|(_, (_, r))| *r)
            .fold(default_x, f32::max);
        // CSS Shapes L1 contours.
        let own = self.shape_prims
            .iter()
            .filter(|g| g.is_left && g.top_y <= y && g.bottom_y > y)
            .filter_map(|g| g.prim.extent_at(y, g.margin).map(|(_, r)| r.min(g.bound_hi)))
            .fold(rect_edge, f32::max);
        // CSS 2.1 §9.5: enclosing-context floats also push the left edge right.
        match &self.inherited {
            Some(p) => p.left_edge_impl(y, own, exact, in_flow),
            None => own,
        }
    }

    /// Right boundary of available inline space at `y` (= leftmost left-edge
    /// of all right floats whose `bottom_y > y`).  Falls back to `default_x`.
    pub(crate) fn right_edge_at(&self, y: f32, default_x: f32) -> f32 {
        self.right_edge_impl(y, default_x, false, false)
    }

    /// [`Self::right_edge_at`] for a normal-flow box: see [`Self::left_edge_in_flow`].
    pub(crate) fn right_edge_in_flow(&self, y: f32, default_x: f32) -> f32 {
        self.right_edge_impl(y, default_x, false, true)
    }

    /// [`Self::right_edge_at`]; `exact` as in [`Self::left_edge_impl`].
    fn right_edge_impl(&self, y: f32, default_x: f32, exact: bool, in_flow: bool) -> f32 {
        let rect_edge = self.right
            .iter()
            .enumerate()
            .filter(|(i, (bot, _))| {
                *bot > y
                    && !(in_flow && self.right_tops.get(*i).is_some_and(|t| *t > y + 0.01))
                    && !(exact && (self.gen_right.contains(i)))
            })
            .map(|(_, (_, l))| *l)
            .fold(default_x, f32::min);
        // CSS Shapes L1 contours.
        let own = self.shape_prims
            .iter()
            .filter(|g| !g.is_left && g.top_y <= y && g.bottom_y > y)
            .filter_map(|g| g.prim.extent_at(y, g.margin).map(|(l, _)| l.max(g.bound_lo)))
            .fold(rect_edge, f32::min);
        // CSS 2.1 §9.5: enclosing-context floats also pull the right edge left.
        match &self.inherited {
            Some(p) => p.right_edge_impl(y, own, exact, in_flow),
            None => own,
        }
    }

    /// Record a left float occupying `[y_top, bottom_y)` with right margin
    /// edge at `right_edge`.
    pub(crate) fn add_left(&mut self, top_y: f32, bottom_y: f32, right_edge: f32) {
        self.left.push((bottom_y, right_edge));
        self.left_tops.push(top_y);
    }

    /// Record a right float occupying `[y_top, bottom_y)` with left margin
    /// edge at `left_edge`.
    pub(crate) fn add_right(&mut self, top_y: f32, bottom_y: f32, left_edge: f32) {
        self.right.push((bottom_y, left_edge));
        self.right_tops.push(top_y);
    }

    /// Attaches `prims` (already in this context's frame) as the contour of the float
    /// just added with [`Self::add_left`] / [`Self::add_right`]. `window` is the
    /// float's margin-box block range, `bound` its inline range — the contour never
    /// reaches beyond them (CSS Shapes L1 §6.3) — and `margin` the `shape-margin`.
    pub(crate) fn add_shape_prims(
        &mut self,
        prims: Vec<Prim>,
        is_left: bool,
        window: (f32, f32),
        bound: (f32, f32),
        margin: f32,
    ) {
        let list_len = if is_left { self.left.len() } else { self.right.len() };
        let slot = if is_left { &mut self.gen_left } else { &mut self.gen_right };
        if let Some(i) = list_len.checked_sub(1) {
            slot.push(i);
        }
        for prim in prims {
            self.shape_prims.push(GenShape {
                prim,
                is_left,
                top_y: window.0,
                bottom_y: window.1,
                bound_lo: bound.0,
                bound_hi: bound.1,
                margin,
            });
        }
    }

    /// Lowest `bottom_y` of any recorded `shape-outside` visible here (owned or
    /// inherited), `f32::NEG_INFINITY` when there is none. A line box that
    /// starts at or below it cannot be affected by a contour, so such a run
    /// keeps the plain single-band layout.
    pub(crate) fn shapes_bottom(&self) -> f32 {
        let own = self.shape_prims.iter().map(|g| g.bottom_y)
            .fold(f32::NEG_INFINITY, f32::max);
        self.inherited.as_ref().map_or(own, |p| own.max(p.shapes_bottom()))
    }

    /// CSS Shapes L1 §3 / CSS 2.1 §9.4.2 — the inline band `(left, right)` a
    /// line box spanning `[y_top, y_bot)` may occupy: the tightest edges over
    /// the whole box (sampled every px plus the last row), with a shaped float
    /// narrowing it by its contour rather than its margin rectangle.
    pub(crate) fn line_band(
        &self,
        y_top: f32,
        y_bot: f32,
        left_default: f32,
        right_default: f32,
    ) -> (f32, f32) {
        let mut left = left_default;
        let mut right = right_default;
        let last = (y_bot - 0.01).max(y_top);
        let mut y = y_top;
        loop {
            left = left.max(self.left_edge_impl(y, left_default, true, false));
            right = right.min(self.right_edge_impl(y, right_default, true, false));
            if y >= last {
                break;
            }
            y = (y + 1.0).min(last);
        }
        (left, right)
    }

    /// CSS 2.1 §9.5.2 — advance `y` past all floats on the given side.
    pub(crate) fn clear_y(&self, y: f32, side: ClearSide) -> f32 {
        let mut result = y;
        let do_left  = matches!(side, ClearSide::Left  | ClearSide::Both);
        let do_right = matches!(side, ClearSide::Right | ClearSide::Both);
        if do_left  { for (bot, _) in &self.left  { result = result.max(*bot); } }
        if do_right { for (bot, _) in &self.right { result = result.max(*bot); } }
        // CSS 2.1 §9.5.2: `clear` on a nested block clears the enclosing
        // context's floats too (their bottoms are absolute, like ours).
        match &self.inherited {
            Some(p) => p.clear_y(result, side),
            None => result,
        }
    }

    /// A copy of the rectangular floats with every block-axis coordinate moved
    /// by `-db` (so a box starting at `db` in this context sees itself at `0`);
    /// the inline-axis edges are kept. Used to hand a vertical `InlineRun` the
    /// floats of its container in the run's own frame (CSS Writing Modes L3 §7.1).
    /// Only contours of vertical-mode floats ([`Self::shape_prims`]) are carried over.
    pub(crate) fn rebased_block(&self, db: f32) -> FloatContext {
        let mut c = Self::new();
        c.left = self.left.iter().map(|&(bot, e)| (bot - db, e)).collect();
        c.right = self.right.iter().map(|&(bot, e)| (bot - db, e)).collect();
        c.left_tops = self.left_tops.iter().map(|t| t - db).collect();
        c.right_tops = self.right_tops.iter().map(|t| t - db).collect();
        c.shape_prims = self.shape_prims.iter().map(|g| g.moved(0.0, -db)).collect();
        c.gen_left = self.gen_left.clone();
        c.gen_right = self.gen_right.clone();
        c.inherited = self.inherited.as_ref().map(|p| Box::new(p.rebased_block(db)));
        c
    }

    /// The rectangular floats of this context (owned and inherited) seen from a
    /// frame whose origin sits at `(db, di)` of this one — block coordinates
    /// move by `-db`, inline ones by `-di`. Only the vertical-mode contours ([`Self::shape_prims`]) are kept.
    fn translated(&self, db: f32, di: f32) -> FloatContext {
        let mut c = Self::new();
        c.left = self.left.iter().map(|&(bot, e)| (bot - db, e - di)).collect();
        c.right = self.right.iter().map(|&(bot, e)| (bot - db, e - di)).collect();
        c.left_tops = self.left_tops.iter().map(|t| t - db).collect();
        c.right_tops = self.right_tops.iter().map(|t| t - db).collect();
        c.shape_prims = self.shape_prims.iter().map(|g| g.moved(-di, -db)).collect();
        c.gen_left = self.gen_left.clone();
        c.gen_right = self.gen_right.clone();
        c.inherited = self.inherited.as_ref().map(|p| Box::new(p.translated(db, di)));
        c
    }

    /// CSS 2.1 §9.5 in a vertical writing mode: the context of a non-BFC block
    /// whose own logical frame starts at `(db, di)` of this one. It sees every
    /// float of the enclosing context (so its line boxes and floats are
    /// displaced by them) and keeps its own in the owned buckets.
    pub(crate) fn inheriting_vertical(&self, db: f32, di: f32) -> FloatContext {
        if self.is_empty() {
            return Self::new();
        }
        let mut c = Self::new();
        c.inherited = Some(Box::new(self.translated(db, di)));
        c
    }

    /// Takes over the floats `child` placed itself (its frame starts at
    /// `(db, di)` of this one): a float belongs to its block formatting context
    /// whichever non-BFC block it sits in, so the siblings that follow the
    /// child must meet it too.
    pub(crate) fn absorb_vertical(&mut self, child: &FloatContext, db: f32, di: f32) {
        let (left_base, right_base) = (self.left.len(), self.right.len());
        self.gen_left.extend(child.gen_left.iter().map(|i| i + left_base));
        self.gen_right.extend(child.gen_right.iter().map(|i| i + right_base));
        self.shape_prims.extend(child.shape_prims.iter().map(|g| g.moved(di, db)));
        for (i, &(bot, e)) in child.left.iter().enumerate() {
            self.left.push((bot + db, e + di));
            self.left_tops.push(child.left_tops[i] + db);
        }
        for (i, &(bot, e)) in child.right.iter().enumerate() {
            self.right.push((bot + db, e + di));
            self.right_tops.push(child.right_tops[i] + db);
        }
    }

    /// True when there are no active floats at all (owned or inherited).
    pub(crate) fn is_empty(&self) -> bool {
        self.left.is_empty()
            && self.right.is_empty()
            && self.inherited.as_ref().is_none_or(|p| p.is_empty())
    }

    /// CSS 2.1 §9.5.1 rule 8 — the smallest float bottom strictly below `y`
    /// across both sides. A float that does not fit beside the current floats
    /// drops to the next such bottom, where the line widens. Returns `None`
    /// when no float ends below `y` (nothing left to clear).
    pub(crate) fn next_float_bottom(&self, y: f32) -> Option<f32> {
        let own = self.left.iter().chain(self.right.iter())
            .map(|(bot, _)| *bot)
            .filter(|bot| *bot > y + 0.01)
            .fold(None, |acc, bot| Some(acc.map_or(bot, |a: f32| a.min(bot))));
        // CSS 2.1 §9.5.1 rule 8: enclosing-context floats also widen the band.
        let inh = self.inherited.as_ref().and_then(|p| p.next_float_bottom(y));
        match (own, inh) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

/// CSS Shapes L1 §4 — rightmost x of polygon boundary at scanline `y`.
/// Scans all edges that cross `y`; returns `None` if no edge crosses.
#[cfg(test)]
pub(crate) fn polygon_right_edge_at_y(pts: &[(f32, f32)], y: f32) -> Option<f32> {
    polygon_edge_x_at_y(pts, y, true)
}

/// CSS Shapes L1 §4 — leftmost x of polygon boundary at scanline `y`.
#[cfg(test)]
pub(crate) fn polygon_left_edge_at_y(pts: &[(f32, f32)], y: f32) -> Option<f32> {
    polygon_edge_x_at_y(pts, y, false)
}

/// Shared kernel: iterate polygon edges, return rightmost (want_max=true) or
/// leftmost (want_max=false) x intersection with horizontal scanline at `y`.
fn polygon_edge_x_at_y(pts: &[(f32, f32)], y: f32, want_max: bool) -> Option<f32> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let mut best: Option<f32> = None;
    for i in 0..n {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % n];
        // Edge crosses y iff exactly one endpoint is strictly below y.
        // Use half-open interval [min, max) to avoid double-counting vertices.
        if (y0 <= y && y < y1) || (y1 <= y && y < y0) {
            let x_at_y = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
            best = Some(match best {
                None => x_at_y,
                Some(prev) => if want_max { prev.max(x_at_y) } else { prev.min(x_at_y) },
            });
        }
    }
    best
}

/// CSS Shapes L1 §6.3 — like [`polygon_edge_x_at_y`], but for the polygon grown
/// outward by `margin` px (the union of the polygon, a rectangle of half-width
/// `margin` around every edge, and a disc of radius `margin` at every vertex).
/// Unlike the bare polygon it also yields a value at scanlines *beyond* the
/// polygon's vertical extent (within `margin` of it). `margin <= 0` is the bare
/// polygon. Returns the rightmost (`want_max`) or leftmost boundary x.
pub(crate) fn polygon_edge_x_at_y_margin(
    pts: &[(f32, f32)],
    y: f32,
    margin: f32,
    want_max: bool,
) -> Option<f32> {
    let mut best = polygon_edge_x_at_y(pts, y, want_max);
    if margin <= 0.0 {
        return best;
    }
    let mut take = |x: f32| {
        best = Some(match best {
            None => x,
            Some(prev) => if want_max { prev.max(x) } else { prev.min(x) },
        });
    };
    let n = pts.len();
    for i in 0..n {
        let (ax, ay) = pts[i];
        let (bx, by) = pts[(i + 1) % n];
        // Vertex disc.
        let dy = y - ay;
        if dy.abs() <= margin {
            let hw = (margin * margin - dy * dy).max(0.0).sqrt();
            take(if want_max { ax + hw } else { ax - hw });
        }
        // Edge rectangle: the segment pushed out by `±margin` along its normal.
        // A horizontal edge's rectangle is bounded by its end discs (above).
        let (ex, ey) = (bx - ax, by - ay);
        let len = (ex * ex + ey * ey).sqrt();
        if len < 1e-6 || ey.abs() < 1e-6 {
            continue;
        }
        let (nx, ny) = (-ey / len * margin, ex / len * margin);
        for sign in [1.0_f32, -1.0] {
            let (x0, y0) = (ax + sign * nx, ay + sign * ny);
            let (x1, y1) = (bx + sign * nx, by + sign * ny);
            if (y0 <= y && y <= y1) || (y1 <= y && y <= y0) {
                take(x0 + (y - y0) * (x1 - x0) / (y1 - y0));
            }
        }
    }
    best
}
