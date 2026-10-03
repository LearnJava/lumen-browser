//! Recursive box-shift helpers (`shift_y_box`/`shift_tree`) + CSS Shapes L1
//! `shape-outside` value parsers (circle/polygon/ellipse/inset/path) + CSS 2.1
//! §9.5 float-context tracking (`FloatContext`) + polygon-edge scan helpers.
//!
//! Перенесено батчем SPLIT-BT11 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `fn shift_y_box` до конца оставшегося региона, перед `mod bfc;`)
//! без правок тел.

use super::*;

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

/// CSS Shapes L1 §5.1 — parse `circle(<length-px>)` from a raw shape string.
/// Returns the radius in px. Only handles `circle(Npx)` without `at` clause.
/// Returns `None` for any unrecognised syntax (fallback to rectangular float).
pub(crate) fn parse_circle_px(s: &str) -> Option<f32> {
    let s = s.trim().to_ascii_lowercase();
    let inner = s.strip_prefix("circle(")?.strip_suffix(')')?;
    let token = inner.split_whitespace().next()?;
    // Accept "50px" or bare "50" (assume px).
    let digits = token.strip_suffix("px").unwrap_or(token);
    digits.parse::<f32>().ok().filter(|&r| r > 0.0)
}

/// CSS Shapes L1 §5.2 — parse `polygon([<fill-rule>,] x1 y1, x2 y2, ...)`.
/// Returns vertex list in float-local (margin-box-relative) px coordinates.
/// Accepts `Npx` or bare `N` (assumed px). Returns `None` for any unknown syntax.
pub(crate) fn parse_shape_polygon_px(s: &str) -> Option<Vec<(f32, f32)>> {
    let s = s.trim().to_ascii_lowercase();
    let inner = s.strip_prefix("polygon(")?.strip_suffix(')')?;
    // Strip optional fill-rule keyword (nonzero | evenodd).
    let coords_str = if inner.trim_start().starts_with("nonzero")
        || inner.trim_start().starts_with("evenodd")
    {
        inner.split_once(',').map(|x| x.1).unwrap_or("")
    } else {
        inner
    };
    let mut pts: Vec<(f32, f32)> = Vec::new();
    for pair in coords_str.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let mut it = pair.split_whitespace();
        let xs = it.next()?;
        let ys = it.next()?;
        let x = xs.strip_suffix("px").unwrap_or(xs).parse::<f32>().ok()?;
        let y = ys.strip_suffix("px").unwrap_or(ys).parse::<f32>().ok()?;
        pts.push((x, y));
    }
    if pts.len() >= 3 { Some(pts) } else { None }
}

/// CSS Shapes L1 §5.2 — parse `ellipse(<rx> <ry> at <cx> <cy>)`.
/// Returns `(rx, ry, cx, cy)` in float-local (margin-box-relative) px coords.
/// Returns `None` for any unknown syntax or zero/negative radii.
pub(crate) fn parse_shape_ellipse_px(s: &str) -> Option<(f32, f32, f32, f32)> {
    let s = s.trim().to_ascii_lowercase();
    let inner = s.strip_prefix("ellipse(")?.strip_suffix(')')?;
    // Expected: "rxpx rypx at cxpx cypx"
    let at_pos = inner.find(" at ")?;
    let radii_part = inner[..at_pos].trim();
    let center_part = inner[at_pos + 4..].trim();
    let mut ri = radii_part.split_whitespace();
    let mut ci = center_part.split_whitespace();
    let rxs = ri.next()?;
    let rys = ri.next()?;
    let cxs = ci.next()?;
    let cys = ci.next()?;
    let rx = rxs.strip_suffix("px").unwrap_or(rxs).parse::<f32>().ok()?;
    let ry = rys.strip_suffix("px").unwrap_or(rys).parse::<f32>().ok()?;
    let cx = cxs.strip_suffix("px").unwrap_or(cxs).parse::<f32>().ok()?;
    let cy = cys.strip_suffix("px").unwrap_or(cys).parse::<f32>().ok()?;
    if rx > 0.0 && ry > 0.0 { Some((rx, ry, cx, cy)) } else { None }
}

/// CSS Shapes L1 §5.1 — parse `inset(<top> <right> <bottom> <left> [round <r>])`.
/// Returns `(top, right, bottom, left, radius)` insets in px from the reference
/// box edges, plus a single uniform corner radius (`0` = sharp corners).
/// Lengths follow the margin-shorthand expansion (1–4 values). The optional
/// `round` clause keeps only the first radius value (elliptical radii collapse
/// to their horizontal component). Returns `None` for any unknown syntax.
pub(crate) fn parse_shape_inset_px(s: &str) -> Option<(f32, f32, f32, f32, f32)> {
    let s = s.trim().to_ascii_lowercase();
    let inner = s.strip_prefix("inset(")?.strip_suffix(')')?;
    // Split off the optional `round <border-radius>` clause.
    let (lens_part, radius) = match inner.split_once(" round ") {
        Some((l, r)) => {
            let rstr = r.split_whitespace().next()?;
            let rad = rstr
                .strip_suffix("px")
                .unwrap_or(rstr)
                .parse::<f32>()
                .ok()
                .filter(|v| *v >= 0.0)?;
            (l, rad)
        }
        None => (inner, 0.0),
    };
    let mut vals: Vec<f32> = Vec::new();
    for tok in lens_part.split_whitespace() {
        let v = tok.strip_suffix("px").unwrap_or(tok).parse::<f32>().ok()?;
        vals.push(v);
    }
    let (t, r, b, l) = match vals.len() {
        1 => (vals[0], vals[0], vals[0], vals[0]),
        2 => (vals[0], vals[1], vals[0], vals[1]),
        3 => (vals[0], vals[1], vals[2], vals[1]),
        4 => (vals[0], vals[1], vals[2], vals[3]),
        _ => return None,
    };
    Some((t, r, b, l, radius))
}

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

/// CSS Shapes L1 §5.2 — polygon shape for `shape-outside` on a float.
/// Points are stored in content-area coordinates (same as FloatContext).
#[derive(Clone)]
pub(crate) struct ShapePolygon {
    pub(crate) top_y: f32,
    pub(crate) bottom_y: f32,
    /// `true` = left float, `false` = right float.
    pub(crate) is_left: bool,
    /// Polygon vertices in content-area coordinates.
    pub(crate) points: Vec<(f32, f32)>,
    /// CSS Shapes L1 §6.3 — `shape-margin` (px): the wrapping contour is the
    /// polygon grown outward by this distance. `0` = the bare polygon.
    pub(crate) margin: f32,
    /// Float-area clamp (§6.3 note: a shape-margin never extends the float area
    /// past the float's margin box). For a left float the wrap edge is capped
    /// at this x (`+inf` = no cap); for a right float it is floored at it (`-inf`).
    pub(crate) bound_x: f32,
}

/// CSS Shapes L1 §5.2 — ellipse shape for `shape-outside` on a float.
/// All coordinates are in content-area space (same as FloatContext).
#[derive(Clone)]
pub(crate) struct ShapeEllipse {
    pub(crate) top_y: f32,
    pub(crate) bottom_y: f32,
    /// `true` = left float, `false` = right float.
    pub(crate) is_left: bool,
    pub(crate) cx: f32,
    pub(crate) cy: f32,
    pub(crate) rx: f32,
    pub(crate) ry: f32,
    /// Margin-box clamp of the wrap edge — see [`ShapePolygon::bound_x`].
    pub(crate) bound_x: f32,
}

/// CSS Shapes L1 §5.1 — `inset()` rectangle shape for `shape-outside` on a float.
/// All coordinates are in content-area space (same as FloatContext). The rectangle
/// spans `[left_x, right_x] × [top_y, bottom_y]` with optional uniform corner
/// rounding of `radius` px.
#[derive(Clone)]
pub(crate) struct ShapeInset {
    pub(crate) top_y: f32,
    pub(crate) bottom_y: f32,
    /// `true` = left float, `false` = right float.
    pub(crate) is_left: bool,
    pub(crate) left_x: f32,
    pub(crate) right_x: f32,
    /// Uniform corner radius in px (`0` = sharp corners).
    pub(crate) radius: f32,
    /// Vertical extent of the (shape-margin-grown) rectangle *before* clipping
    /// to the float's margin box — the rounded-corner bands hang off these, not
    /// off the clipped `top_y`/`bottom_y` window.
    pub(crate) corner_top: f32,
    pub(crate) corner_bottom: f32,
    /// Margin-box clamp of the wrap edge — see [`ShapePolygon::bound_x`].
    pub(crate) bound_x: f32,
}

/// CSS Shapes L1 §5.1 — horizontal inward offset of a rounded `inset()` corner
/// at scanline `y`. Returns `0` outside the corner bands or for a `0` radius.
/// Within `radius` px of the top/bottom edge the boundary follows a quarter
/// circle, so the inline edge recedes by `radius − √(radius² − dy²)`.
// Used only by `mod tests` (super::super::X) — never called from this file's
// own non-test code beyond `FloatContext::{left_edge_at, right_edge_at}`.
pub(crate) fn inset_corner_inward(y: f32, top_y: f32, bottom_y: f32, radius: f32) -> f32 {
    if radius <= 0.0 {
        return 0.0;
    }
    let top_band = top_y + radius;
    let bot_band = bottom_y - radius;
    let dy = if y < top_band {
        top_band - y
    } else if y > bot_band {
        y - bot_band
    } else {
        return 0.0;
    };
    let dy = dy.min(radius);
    radius - (radius * radius - dy * dy).max(0.0).sqrt()
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
    /// CSS Shapes L1 — `shape-outside: circle(r)` overrides.
    /// `(top_y, bottom_y, is_left, center_x, center_y, radius)`.
    /// `is_left=true` → left float, `false` → right float.
    pub(crate) shape_circles: Vec<(f32, f32, bool, f32, f32, f32)>,
    /// CSS Shapes L1 — `shape-outside: polygon(...)` overrides.
    pub(crate) shape_polygons: Vec<ShapePolygon>,
    /// CSS Shapes L1 — `shape-outside: ellipse(...)` overrides.
    pub(crate) shape_ellipses: Vec<ShapeEllipse>,
    /// CSS Shapes L1 — `shape-outside: inset(...)` overrides.
    pub(crate) shape_insets: Vec<ShapeInset>,
    /// Indices into [`Self::left`] of floats whose `shape-outside` was recorded
    /// above. Their rectangular margin-box entry stays (it still bounds float
    /// enclosure, `clear` and single-band consumers) but a per-line query
    /// ([`Self::line_band`]) follows the shape instead of the rectangle.
    shaped_left: Vec<usize>,
    /// Same as [`Self::shaped_left`], for [`Self::right`].
    shaped_right: Vec<usize>,
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
            shape_circles: Vec::new(),
            shape_polygons: Vec::new(),
            shape_ellipses: Vec::new(),
            shape_insets: Vec::new(),
            shaped_left: Vec::new(),
            shaped_right: Vec::new(),
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
        self.left_edge_impl(y, default_x, false)
    }

    /// [`Self::left_edge_at`]; with `exact` a float that has a `shape-outside`
    /// contributes its contour only, not its rectangular margin box.
    fn left_edge_impl(&self, y: f32, default_x: f32, exact: bool) -> f32 {
        let rect_edge = self.left
            .iter()
            .enumerate()
            .filter(|(i, (bot, _))| *bot > y && !(exact && self.shaped_left.contains(i)))
            .map(|(_, (_, r))| *r)
            .fold(default_x, f32::max);
        // CSS Shapes L1: circle boundary.
        let after_circles = self.shape_circles
            .iter()
            .filter(|(top, bot, is_left, ..)| *is_left && *top <= y && *bot > y)
            .map(|(_, _, _, cx, cy, r)| {
                let dy = y - cy;
                let hw = (r * r - dy * dy).max(0.0_f32).sqrt();
                cx + hw
            })
            .fold(rect_edge, f32::max);
        // CSS Shapes L1: polygon boundary (rightmost edge at y).
        let after_polygons = self.shape_polygons
            .iter()
            .filter(|p| p.is_left && p.top_y <= y && p.bottom_y > y)
            .filter_map(|p| {
                polygon_edge_x_at_y_margin(&p.points, y, p.margin, true).map(|x| x.min(p.bound_x))
            })
            .fold(after_circles, f32::max);
        // CSS Shapes L1: ellipse boundary (right edge at y).
        let after_ellipses = self.shape_ellipses
            .iter()
            .filter(|e| e.is_left && e.top_y <= y && e.bottom_y > y)
            .filter_map(|e| {
                let norm = (y - e.cy) / e.ry;
                if norm.abs() > 1.0 { return None; }
                Some((e.cx + e.rx * (1.0 - norm * norm).max(0.0).sqrt()).min(e.bound_x))
            })
            .fold(after_polygons, f32::max);
        // CSS Shapes L1: inset() boundary (right edge at y, minus rounded corner).
        let own = self.shape_insets
            .iter()
            .filter(|s| s.is_left && s.top_y <= y && s.bottom_y > y)
            .map(|s| {
                (s.right_x - inset_corner_inward(y, s.corner_top, s.corner_bottom, s.radius))
                    .min(s.bound_x)
            })
            .fold(after_ellipses, f32::max);
        // CSS 2.1 §9.5: enclosing-context floats also push the left edge right.
        match &self.inherited {
            Some(p) => p.left_edge_impl(y, own, exact),
            None => own,
        }
    }

    /// Right boundary of available inline space at `y` (= leftmost left-edge
    /// of all right floats whose `bottom_y > y`).  Falls back to `default_x`.
    pub(crate) fn right_edge_at(&self, y: f32, default_x: f32) -> f32 {
        self.right_edge_impl(y, default_x, false)
    }

    /// [`Self::right_edge_at`]; `exact` as in [`Self::left_edge_impl`].
    fn right_edge_impl(&self, y: f32, default_x: f32, exact: bool) -> f32 {
        let rect_edge = self.right
            .iter()
            .enumerate()
            .filter(|(i, (bot, _))| *bot > y && !(exact && self.shaped_right.contains(i)))
            .map(|(_, (_, l))| *l)
            .fold(default_x, f32::min);
        // CSS Shapes L1: circle boundary.
        let after_circles = self.shape_circles
            .iter()
            .filter(|(top, bot, is_left, ..)| !is_left && *top <= y && *bot > y)
            .map(|(_, _, _, cx, cy, r)| {
                let dy = y - cy;
                let hw = (r * r - dy * dy).max(0.0_f32).sqrt();
                cx - hw
            })
            .fold(rect_edge, f32::min);
        // CSS Shapes L1: polygon boundary (leftmost edge at y).
        let after_polygons = self.shape_polygons
            .iter()
            .filter(|p| !p.is_left && p.top_y <= y && p.bottom_y > y)
            .filter_map(|p| {
                polygon_edge_x_at_y_margin(&p.points, y, p.margin, false).map(|x| x.max(p.bound_x))
            })
            .fold(after_circles, f32::min);
        // CSS Shapes L1: ellipse boundary (left edge at y).
        let after_ellipses = self.shape_ellipses
            .iter()
            .filter(|e| !e.is_left && e.top_y <= y && e.bottom_y > y)
            .filter_map(|e| {
                let norm = (y - e.cy) / e.ry;
                if norm.abs() > 1.0 { return None; }
                Some((e.cx - e.rx * (1.0 - norm * norm).max(0.0).sqrt()).max(e.bound_x))
            })
            .fold(after_polygons, f32::min);
        // CSS Shapes L1: inset() boundary (left edge at y, plus rounded corner).
        let own = self.shape_insets
            .iter()
            .filter(|s| !s.is_left && s.top_y <= y && s.bottom_y > y)
            .map(|s| {
                (s.left_x + inset_corner_inward(y, s.corner_top, s.corner_bottom, s.radius))
                    .max(s.bound_x)
            })
            .fold(after_ellipses, f32::min);
        // CSS 2.1 §9.5: enclosing-context floats also pull the right edge left.
        match &self.inherited {
            Some(p) => p.right_edge_impl(y, own, exact),
            None => own,
        }
    }

    /// Record a left float occupying `[y_top, bottom_y)` with right margin
    /// edge at `right_edge`.
    pub(crate) fn add_left(&mut self, bottom_y: f32, right_edge: f32) {
        self.left.push((bottom_y, right_edge));
    }

    /// Record a right float occupying `[y_top, bottom_y)` with left margin
    /// edge at `left_edge`.
    pub(crate) fn add_right(&mut self, bottom_y: f32, left_edge: f32) {
        self.right.push((bottom_y, left_edge));
    }

    /// Marks the float just added with [`Self::add_left`] / [`Self::add_right`]
    /// as shaped (its `shape-outside` was recorded): see [`Self::line_band`].
    pub(crate) fn mark_last_shaped(&mut self, is_left: bool) {
        if is_left {
            if let Some(i) = self.left.len().checked_sub(1) {
                self.shaped_left.push(i);
            }
        } else if let Some(i) = self.right.len().checked_sub(1) {
            self.shaped_right.push(i);
        }
    }

    /// Lowest `bottom_y` of any recorded `shape-outside` visible here (owned or
    /// inherited), `f32::NEG_INFINITY` when there is none. A line box that
    /// starts at or below it cannot be affected by a contour, so such a run
    /// keeps the plain single-band layout.
    pub(crate) fn shapes_bottom(&self) -> f32 {
        let own = self.shape_circles.iter().map(|c| c.1)
            .chain(self.shape_polygons.iter().map(|p| p.bottom_y))
            .chain(self.shape_ellipses.iter().map(|e| e.bottom_y))
            .chain(self.shape_insets.iter().map(|i| i.bottom_y))
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
            left = left.max(self.left_edge_impl(y, left_default, true));
            right = right.min(self.right_edge_impl(y, right_default, true));
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

/// Placement of a float's boxes, as [`register_shape_outside`] needs them.
/// All x/y are in the same absolute content-area space as [`FloatContext`].
pub(crate) struct FloatShapeGeom {
    /// `true` = left float, `false` = right float.
    pub(crate) is_left: bool,
    /// Top of the margin box (shape-local coordinates are relative to
    /// `(box_left, child_y)`).
    pub(crate) child_y: f32,
    /// Top / bottom of the wrapping window (`child_y + margin-top` …
    /// margin-box bottom).
    pub(crate) top_y: f32,
    pub(crate) bot_y: f32,
    /// Left / right edges of the margin box.
    pub(crate) box_left: f32,
    pub(crate) box_right: f32,
    /// Border-box centre — the reference of `circle()`.
    pub(crate) center_x: f32,
    pub(crate) center_y: f32,
}

/// CSS Shapes L1 §4/§5/§6.3 — turn a float's `shape-outside` value into the
/// wrapping geometry recorded in `fc`, grown outward by `margin` px
/// (`shape-margin`, already resolved). Unrecognised syntax records nothing
/// (the float keeps its rectangular wrapping). With `margin > 0` the wrap edge
/// is additionally clamped to the float's margin box (§6.3: the float area never
/// extends past it); with `margin == 0` the pre-existing geometry is kept as is.
/// Returns whether a shape was recorded.
pub(crate) fn register_shape_outside(
    fc: &mut FloatContext,
    sv: &str,
    g: &FloatShapeGeom,
    margin: f32,
) -> bool {
    let m = margin.max(0.0);
    let bound_x = match (m > 0.0, g.is_left) {
        (false, true) => f32::INFINITY,
        (false, false) => f32::NEG_INFINITY,
        (true, true) => g.box_right,
        (true, false) => g.box_left,
    };
    if let Some(r) = parse_circle_px(sv) {
        if m > 0.0 {
            // A circle grown by `m` is a circle of radius `r + m`; stored as an
            // ellipse so it carries the margin-box clamp.
            fc.shape_ellipses.push(ShapeEllipse {
                top_y: g.top_y, bottom_y: g.bot_y, is_left: g.is_left,
                cx: g.center_x, cy: g.center_y, rx: r + m, ry: r + m, bound_x,
            });
        } else {
            fc.shape_circles.push((g.top_y, g.bot_y, g.is_left, g.center_x, g.center_y, r));
        }
    } else if let Some(local_pts) = parse_shape_path_px(sv).or_else(|| parse_shape_polygon_px(sv)) {
        let points = local_pts.into_iter()
            .map(|(px, py)| (px + g.box_left, py + g.child_y))
            .collect();
        fc.shape_polygons.push(ShapePolygon {
            top_y: g.top_y, bottom_y: g.bot_y, is_left: g.is_left, points, margin: m, bound_x,
        });
    } else if let Some((rx, ry, ecx, ecy)) = parse_shape_ellipse_px(sv) {
        fc.shape_ellipses.push(ShapeEllipse {
            top_y: g.top_y, bottom_y: g.bot_y, is_left: g.is_left,
            cx: ecx + g.box_left, cy: ecy + g.child_y, rx: rx + m, ry: ry + m, bound_x,
        });
    } else if let Some((it, ir, ib, il, irad)) = parse_shape_inset_px(sv) {
        // Reference box = margin box. Grown by `m` the rectangle gains `m` on
        // every side and its corner radius grows by `m` (a sharp corner becomes
        // a quarter circle of radius `m`).
        let corner_top = g.child_y + it - m;
        let corner_bottom = g.bot_y - ib + m;
        let top_y = corner_top.max(g.child_y).min(g.bot_y);
        let bottom_y = corner_bottom.min(g.bot_y).max(top_y);
        fc.shape_insets.push(ShapeInset {
            top_y, bottom_y, is_left: g.is_left,
            left_x: g.box_left + il - m,
            right_x: g.box_right - ir + m,
            radius: if irad > 0.0 || m > 0.0 { irad + m } else { 0.0 },
            corner_top, corner_bottom, bound_x,
        });
    } else {
        return false;
    }
    true
}
