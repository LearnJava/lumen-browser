//! Recursive box-shift helpers (`shift_y_box`/`shift_tree`) + CSS Shapes L1
//! `shape-outside` value parsers (circle/polygon/ellipse/inset/path) + CSS 2.1
//! §9.5 float-context tracking (`FloatContext`) + polygon-edge scan helpers.
//!
//! Перенесено батчем SPLIT-BT11 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `fn shift_y_box` до конца оставшегося региона, перед `mod bfc;`)
//! без правок тел.

use super::*;

/// Рекурсивно смещает rect.y всего поддерева на dy (для vertical-align).
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
    b.rect.y += dy;
    if let BoxKind::SvgShape { svg_paint_matrix, .. } = &mut b.kind {
        svg_paint_matrix.matrix[5] += dy;
    }
    for child in &mut b.children {
        shift_y_box(child, dy);
    }
}

/// Рекурсивно смещает rect всего поддерева на (dx, dy).
/// Используется при позиционировании абсолютных потомков.
///
/// BUG-424 (в): keeps `svg_paint_matrix` in sync with `rect` — see
/// `shift_y_box` for why this matters.
pub(crate) fn shift_tree(b: &mut LayoutBox, dx: f32, dy: f32) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    b.rect.x += dx;
    b.rect.y += dy;
    if let BoxKind::SvgShape { svg_paint_matrix, .. } = &mut b.kind {
        svg_paint_matrix.matrix[4] += dx;
        svg_paint_matrix.matrix[5] += dy;
    }
    for child in &mut b.children {
        shift_tree(child, dx, dy);
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
        let rect_edge = self.left
            .iter()
            .filter(|(bot, _)| *bot > y)
            .map(|(_, r)| *r)
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
            .filter_map(|p| polygon_right_edge_at_y(&p.points, y))
            .fold(after_circles, f32::max);
        // CSS Shapes L1: ellipse boundary (right edge at y).
        let after_ellipses = self.shape_ellipses
            .iter()
            .filter(|e| e.is_left && e.top_y <= y && e.bottom_y > y)
            .filter_map(|e| {
                let norm = (y - e.cy) / e.ry;
                if norm.abs() > 1.0 { return None; }
                Some(e.cx + e.rx * (1.0 - norm * norm).max(0.0).sqrt())
            })
            .fold(after_polygons, f32::max);
        // CSS Shapes L1: inset() boundary (right edge at y, minus rounded corner).
        let own = self.shape_insets
            .iter()
            .filter(|s| s.is_left && s.top_y <= y && s.bottom_y > y)
            .map(|s| s.right_x - inset_corner_inward(y, s.top_y, s.bottom_y, s.radius))
            .fold(after_ellipses, f32::max);
        // CSS 2.1 §9.5: enclosing-context floats also push the left edge right.
        match &self.inherited {
            Some(p) => p.left_edge_at(y, own),
            None => own,
        }
    }

    /// Right boundary of available inline space at `y` (= leftmost left-edge
    /// of all right floats whose `bottom_y > y`).  Falls back to `default_x`.
    pub(crate) fn right_edge_at(&self, y: f32, default_x: f32) -> f32 {
        let rect_edge = self.right
            .iter()
            .filter(|(bot, _)| *bot > y)
            .map(|(_, l)| *l)
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
            .filter_map(|p| polygon_left_edge_at_y(&p.points, y))
            .fold(after_circles, f32::min);
        // CSS Shapes L1: ellipse boundary (left edge at y).
        let after_ellipses = self.shape_ellipses
            .iter()
            .filter(|e| !e.is_left && e.top_y <= y && e.bottom_y > y)
            .filter_map(|e| {
                let norm = (y - e.cy) / e.ry;
                if norm.abs() > 1.0 { return None; }
                Some(e.cx - e.rx * (1.0 - norm * norm).max(0.0).sqrt())
            })
            .fold(after_polygons, f32::min);
        // CSS Shapes L1: inset() boundary (left edge at y, plus rounded corner).
        let own = self.shape_insets
            .iter()
            .filter(|s| !s.is_left && s.top_y <= y && s.bottom_y > y)
            .map(|s| s.left_x + inset_corner_inward(y, s.top_y, s.bottom_y, s.radius))
            .fold(after_ellipses, f32::min);
        // CSS 2.1 §9.5: enclosing-context floats also pull the right edge left.
        match &self.inherited {
            Some(p) => p.right_edge_at(y, own),
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
// Used only by `mod tests` (super::super::X) beyond `FloatContext::left_edge_at`.
pub(crate) fn polygon_right_edge_at_y(pts: &[(f32, f32)], y: f32) -> Option<f32> {
    polygon_edge_x_at_y(pts, y, true)
}

/// CSS Shapes L1 §4 — leftmost x of polygon boundary at scanline `y`.
// Used only by `mod tests` (super::super::X) beyond `FloatContext::right_edge_at`.
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
