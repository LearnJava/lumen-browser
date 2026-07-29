use crate::{Context2D, LineCap, LineJoin, PaintSource, PathSegment};

/// Number of line segments to use when tessellating a Bézier curve.
const BEZIER_STEPS: usize = 32;

/// Vertical sub-samples taken per pixel row when estimating edge coverage.
///
/// Horizontal coverage is exact — a span contributes its fractional overlap to the
/// two end pixels — so the vertical axis is the only one that needs sampling.
const AA_ROWS: u32 = 4;

/// Smallest join wedge, in square pixels, still worth emitting a shape for.
///
/// A tessellated curve turns by a fraction of a degree at every one of its (up to
/// 180) vertices; without this cutoff each of them would cost a join shape while
/// leaving a gap far below one pixel.
const MIN_JOIN_AREA: f32 = 0.05;

/// Smallest bulge, in pixels, that makes a round or mitered join worth its shape.
///
/// Below it the join is emitted as a bevel, which is the same picture for much
/// less work on the near-collinear vertices of a tessellated curve.
const ROUND_FLATNESS: f32 = 0.05;

/// A closed shape as a flat list of `(x0, y0, x1, y1)` edges in device space.
type Edges = Vec<(f32, f32, f32, f32)>;

/// A flattened sub-path in device space.
struct Polyline {
    /// Vertices in draw order; consecutive duplicates removed.
    pts: Vec<(f32, f32)>,
    /// `true` when the last vertex coincides with the first (`closePath()`).
    closed: bool,
}

/// Fill `path` using the even-odd rule with the given paint source.
///
/// Edge pixels are anti-aliased: the paint alpha is scaled by the fraction of the
/// pixel the path covers.
pub fn fill_path(ctx: &mut Context2D, path: &[PathSegment], paint: &PaintSource, alpha: f32) {
    if path.is_empty() { return; }

    let edges = collect_lines(path);
    if edges.is_empty() { return; }

    fill_shapes(ctx, std::slice::from_ref(&edges), paint, alpha);
}

/// Stroke `path` with the context's current `lineCap`, `lineJoin` and `miterLimit`.
///
/// Each segment contributes a quad, each interior vertex a join shape and each open
/// end a cap shape. All of them are rasterized as a single *union*, so the overlap
/// at a join is painted once instead of being composited twice (which darkened
/// joins under `globalAlpha < 1` and left a seam once the quads became
/// anti-aliased).
pub fn stroke_path(ctx: &mut Context2D, path: &[PathSegment], lw: f32, paint: &PaintSource, alpha: f32) {
    if path.is_empty() { return; }
    let half = lw * 0.5;
    // Rejects a NaN/infinite `lineWidth` too — JS can set either.
    if !half.is_finite() || half <= 0.0 { return; }

    let cap = ctx.line_cap;
    let join = ctx.line_join;
    // The miter ratio is never below 1, so any smaller limit (or a NaN one — JS can
    // assign either) simply degrades every miter to a bevel.
    let miter_limit = if ctx.miter_limit.is_finite() { ctx.miter_limit.max(1.0) } else { 1.0 };

    let mut shapes: Vec<Edges> = Vec::new();
    for line in flatten_subpaths(path) {
        stroke_polyline(&line, half, cap, join, miter_limit, &mut shapes);
    }

    if shapes.is_empty() { return; }
    fill_shapes(ctx, &shapes, paint, alpha);
}

// ── stroke geometry ──────────────────────────────────────────────────────────

/// Emit the quads, joins and caps of one flattened sub-path into `shapes`.
fn stroke_polyline(
    line: &Polyline,
    half: f32,
    cap: LineCap,
    join: LineJoin,
    miter_limit: f32,
    shapes: &mut Vec<Edges>,
) {
    let n = line.pts.len();
    if n < 2 { return; }

    for w in line.pts.windows(2) {
        if let Some(quad) = segment_quad(w[0], w[1], half) {
            shapes.push(quad);
        }
    }

    for i in 1..n - 1 {
        push_join(line.pts[i - 1], line.pts[i], line.pts[i + 1], half, join, miter_limit, shapes);
    }

    if line.closed {
        // The last vertex repeats the first, so the closure corner is the only one
        // the loop above did not see.
        push_join(line.pts[n - 2], line.pts[0], line.pts[1], half, join, miter_limit, shapes);
    } else {
        push_cap(line.pts[1], line.pts[0], half, cap, shapes);
        push_cap(line.pts[n - 2], line.pts[n - 1], half, cap, shapes);
    }
}

/// Split `path` into flattened sub-paths, tessellating Bézier curves.
///
/// A sub-path ends at a `Move` or wherever a segment does not start where the
/// previous one ended, so caps land only on the genuine ends of a stroke.
fn flatten_subpaths(path: &[PathSegment]) -> Vec<Polyline> {
    let mut out: Vec<Polyline> = Vec::new();
    let mut cur: Vec<(f32, f32)> = Vec::new();

    for seg in path {
        match *seg {
            PathSegment::Move(x, y) => {
                flush_subpath(&mut cur, &mut out);
                cur.push((x, y));
            }
            PathSegment::Line(x0, y0, x1, y1) => {
                begin_at(&mut cur, &mut out, x0, y0);
                push_vertex(&mut cur, x1, y1);
            }
            PathSegment::Cubic(x0, y0, c1x, c1y, c2x, c2y, x1, y1) => {
                begin_at(&mut cur, &mut out, x0, y0);
                let mut lines = Vec::new();
                tessellate_cubic(x0, y0, c1x, c1y, c2x, c2y, x1, y1, &mut lines);
                for (_, _, x, y) in lines {
                    push_vertex(&mut cur, x, y);
                }
            }
            PathSegment::Quadratic(x0, y0, cx, cy, x1, y1) => {
                begin_at(&mut cur, &mut out, x0, y0);
                let mut lines = Vec::new();
                tessellate_quadratic(x0, y0, cx, cy, x1, y1, &mut lines);
                for (_, _, x, y) in lines {
                    push_vertex(&mut cur, x, y);
                }
            }
        }
    }
    flush_subpath(&mut cur, &mut out);
    out
}

/// Make sure the sub-path under construction starts at `(x, y)`.
fn begin_at(cur: &mut Vec<(f32, f32)>, out: &mut Vec<Polyline>, x: f32, y: f32) {
    match cur.last() {
        Some(&(lx, ly)) if lx == x && ly == y => {}
        Some(_) => {
            flush_subpath(cur, out);
            cur.push((x, y));
        }
        None => cur.push((x, y)),
    }
}

/// Append a vertex, dropping it when it repeats the previous one.
fn push_vertex(cur: &mut Vec<(f32, f32)>, x: f32, y: f32) {
    if let Some(&(lx, ly)) = cur.last()
        && lx == x
        && ly == y
    {
        return;
    }
    cur.push((x, y));
}

/// Move the sub-path under construction into `out`, dropping degenerate ones.
fn flush_subpath(cur: &mut Vec<(f32, f32)>, out: &mut Vec<Polyline>) {
    if cur.len() >= 2 {
        let closed = cur.first() == cur.last();
        out.push(Polyline { pts: std::mem::take(cur), closed });
    } else {
        cur.clear();
    }
}

/// Unit direction from `a` to `b`, or `None` when the two coincide.
fn unit(a: (f32, f32), b: (f32, f32)) -> Option<(f32, f32)> {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len = (dx * dx + dy * dy).sqrt();
    if !len.is_finite() || len < f32::EPSILON { return None; }
    Some((dx / len, dy / len))
}

/// Closed edge list of the polygon through `pts`.
fn polygon(pts: &[(f32, f32)]) -> Edges {
    (0..pts.len())
        .map(|i| {
            let (ax, ay) = pts[i];
            let (bx, by) = pts[(i + 1) % pts.len()];
            (ax, ay, bx, by)
        })
        .collect()
}

/// The `half`-wide band around segment `a → b`, or `None` for a zero-length one.
fn segment_quad(a: (f32, f32), b: (f32, f32), half: f32) -> Option<Edges> {
    let (dx, dy) = unit(a, b)?;
    let nx = -dy * half;
    let ny = dx * half;
    Some(polygon(&[
        (a.0 + nx, a.1 + ny),
        (a.0 - nx, a.1 - ny),
        (b.0 - nx, b.1 - ny),
        (b.0 + nx, b.1 + ny),
    ]))
}

/// Regular polygon approximating the disc of radius `r` centred at `c`.
fn disc(c: (f32, f32), r: f32) -> Edges {
    let steps = ((r * 2.0) as u32 + 8).clamp(8, 64);
    let pts: Vec<(f32, f32)> = (0..steps)
        .map(|i| {
            let t = i as f32 / steps as f32 * std::f32::consts::TAU;
            (c.0 + r * t.cos(), c.1 + r * t.sin())
        })
        .collect();
    polygon(&pts)
}

/// Fill the wedge the two segment quads leave open on the outside of a corner.
fn push_join(
    prev: (f32, f32),
    v: (f32, f32),
    next: (f32, f32),
    half: f32,
    join: LineJoin,
    miter_limit: f32,
    shapes: &mut Vec<Edges>,
) {
    let Some(d0) = unit(prev, v) else { return };
    let Some(d1) = unit(v, next) else { return };
    let cross = d0.0 * d1.1 - d0.1 * d1.0;
    let dot = d0.0 * d1.0 + d0.1 * d1.1;

    // The open wedge has area `half² · sin θ / 2`; below a fraction of a pixel it
    // cannot show up, so a nearly straight vertex costs nothing.
    if half * half * cross.abs() * 0.5 < MIN_JOIN_AREA {
        // A full reversal is the one near-collinear corner with a real gap, and
        // only a round join has anything to put there.
        if dot < 0.0 && join == LineJoin::Round {
            shapes.push(disc(v, half));
        }
        return;
    }

    // The outer side of the turn is `-n` when turning one way and `+n` the other.
    let s = if cross > 0.0 { -1.0 } else { 1.0 };
    let a = (v.0 - s * d0.1 * half, v.1 + s * d0.0 * half);
    let b = (v.0 - s * d1.1 * half, v.1 + s * d1.0 * half);
    // `dot` is cos θ of the turn, so this is cos(θ/2).
    let cos_half = ((1.0 + dot) * 0.5).max(0.0).sqrt();

    match join {
        LineJoin::Round if half * (1.0 - cos_half) >= ROUND_FLATNESS => {
            shapes.push(disc(v, half));
            return;
        }
        LineJoin::Miter if cos_half > f32::EPSILON => {
            let ratio = 1.0 / cos_half;
            if ratio <= miter_limit && half * (ratio - cos_half) >= ROUND_FLATNESS {
                let mx = (a.0 - v.0) + (b.0 - v.0);
                let my = (a.1 - v.1) + (b.1 - v.1);
                let mlen = (mx * mx + my * my).sqrt();
                if mlen > f32::EPSILON {
                    let tip = (v.0 + mx / mlen * half * ratio, v.1 + my / mlen * half * ratio);
                    shapes.push(polygon(&[v, a, tip, b]));
                    return;
                }
            }
        }
        _ => {}
    }
    shapes.push(polygon(&[v, a, b]));
}

/// Cap the open end at `at`, extending away from `from`.
fn push_cap(from: (f32, f32), at: (f32, f32), half: f32, cap: LineCap, shapes: &mut Vec<Edges>) {
    match cap {
        LineCap::Butt => {}
        LineCap::Round => shapes.push(disc(at, half)),
        LineCap::Square => {
            let Some((dx, dy)) = unit(from, at) else { return };
            let (ex, ey) = (dx * half, dy * half);
            let (nx, ny) = (-dy * half, dx * half);
            shapes.push(polygon(&[
                (at.0 + nx, at.1 + ny),
                (at.0 + ex + nx, at.1 + ey + ny),
                (at.0 + ex - nx, at.1 + ey - ny),
                (at.0 - nx, at.1 - ny),
            ]));
        }
    }
}

/// Build an 8-bit clip coverage mask by rasterizing `path` with the even-odd rule.
///
/// Returns a `width × height` flat vector; `0` = pixel fully clipped out, `255` =
/// fully inside, intermediate values = the anti-aliased boundary.
pub fn build_clip_mask(path: &[PathSegment], w: u32, h: u32) -> Vec<u8> {
    let mut mask = vec![0u8; (w as usize) * (h as usize)];
    if w == 0 || h == 0 { return mask; }

    let edges = collect_lines(path);
    if edges.is_empty() { return mask; }
    let shapes = std::slice::from_ref(&edges);

    let Some((first_row, last_row)) = row_range(shapes, h) else { return mask };

    let mut cov = vec![0.0f32; w as usize];
    let mut scratch = Scratch::default();
    for y in first_row..last_row {
        let Some((first_col, last_col)) = scratch.coverage_row(shapes, y, w, &mut cov) else { continue };
        let base = (y as usize) * (w as usize);
        for x in first_col as usize..last_col as usize {
            let c = cov[x].clamp(0.0, 1.0);
            mask[base + x] = (c * 255.0 + 0.5) as u8;
        }
    }

    mask
}

// ── coverage rasterizer ──────────────────────────────────────────────────────

/// Composite the union of `shapes` into `ctx`, scaling alpha by per-pixel coverage.
fn fill_shapes(ctx: &mut Context2D, shapes: &[Edges], paint: &PaintSource, alpha: f32) {
    let w = ctx.width();
    let h = ctx.height();
    if w == 0 || h == 0 { return; }

    let Some((first_row, last_row)) = row_range(shapes, h) else { return };

    let mut cov = vec![0.0f32; w as usize];
    let mut scratch = Scratch::default();
    for y in first_row..last_row {
        let Some((first_col, last_col)) = scratch.coverage_row(shapes, y, w, &mut cov) else { continue };
        for x in first_col..last_col {
            let c = cov[x as usize].clamp(0.0, 1.0);
            if c <= 0.0 { continue; }
            let clip = ctx.clip_coverage(x, y);
            if clip <= 0.0 { continue; }
            let mut color = paint.sample(x as f32 + 0.5, y as f32 + 0.5);
            color.a = (color.a as f32 * alpha * c * clip) as u8;
            ctx.composite_pixel(x, y, color);
        }
    }
}

/// Vertical pixel range `[first, last)` that `shapes` can touch, clamped to `h`.
///
/// `None` when the shapes are empty or degenerate (zero height — nothing to fill).
fn row_range(shapes: &[Edges], h: u32) -> Option<(u32, u32)> {
    let mut top = f32::INFINITY;
    let mut bottom = f32::NEG_INFINITY;
    for shape in shapes {
        for &(_, y0, _, y1) in shape {
            top = top.min(y0).min(y1);
            bottom = bottom.max(y0).max(y1);
        }
    }
    if !top.is_finite() || !bottom.is_finite() || bottom <= top { return None; }

    let first = top.max(0.0) as u32;
    let last = (bottom.max(0.0).ceil() as u32).min(h);
    if first >= last { None } else { Some((first, last)) }
}

/// Reusable per-row scratch buffers of the coverage rasterizer.
#[derive(Default)]
struct Scratch {
    /// X coordinates where one sub-scanline crosses the edges of a single shape.
    crossings: Vec<f32>,
    /// `(start, end)` spans of all shapes on one sub-scanline, before merging.
    spans: Vec<(f32, f32)>,
}

impl Scratch {
    /// Fill `cov[0..w]` with the coverage of the union of `shapes` over pixel row `y`.
    ///
    /// Returns the `[first, last)` column range that received any coverage, so callers
    /// walk only the touched part of the row; `None` when the row is empty.
    fn coverage_row(&mut self, shapes: &[Edges], y: u32, w: u32, cov: &mut [f32]) -> Option<(u32, u32)> {
        cov[..w as usize].fill(0.0);
        let weight = 1.0 / AA_ROWS as f32;
        let mut first_col = u32::MAX;
        let mut last_col = 0u32;

        for sub in 0..AA_ROWS {
            let yf = y as f32 + (sub as f32 + 0.5) / AA_ROWS as f32;

            self.spans.clear();
            for shape in shapes {
                self.crossings.clear();
                for &(x0, y0, x1, y1) in shape {
                    let (top, bottom) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
                    if yf < top || yf >= bottom { continue; }
                    let t = (yf - y0) / (y1 - y0);
                    self.crossings.push(x0 + t * (x1 - x0));
                }
                self.crossings.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
                let mut i = 0;
                while i + 1 < self.crossings.len() {
                    self.spans.push((self.crossings[i], self.crossings[i + 1]));
                    i += 2;
                }
            }

            // Merge overlapping spans so that a pixel shared by two shapes (e.g. two
            // stroke quads meeting at a join) is still counted at most once.
            self.spans.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal));
            let mut merged: Option<(f32, f32)> = None;
            for &(start, end) in &self.spans {
                match merged {
                    None => merged = Some((start, end)),
                    Some((ms, me)) if start <= me => merged = Some((ms, me.max(end))),
                    Some((ms, me)) => {
                        if let Some((a, b)) = add_span(cov, ms, me, weight, w) {
                            first_col = first_col.min(a);
                            last_col = last_col.max(b);
                        }
                        merged = Some((start, end));
                    }
                }
            }
            if let Some((ms, me)) = merged
                && let Some((a, b)) = add_span(cov, ms, me, weight, w)
            {
                first_col = first_col.min(a);
                last_col = last_col.max(b);
            }
        }

        if first_col < last_col { Some((first_col, last_col)) } else { None }
    }
}

/// Add the horizontal span `[start, end)` to `cov`, weighted by one sub-scanline.
///
/// The two end pixels receive only the fraction of their width the span covers.
/// Returns the `[first, last)` column range written, or `None` for an empty span.
fn add_span(cov: &mut [f32], start: f32, end: f32, weight: f32, w: u32) -> Option<(u32, u32)> {
    // `f32::max`/`min` return the non-NaN operand, so the clamps also scrub a NaN
    // crossing (a degenerate edge) before it can reach the comparison below.
    let start = start.max(0.0);
    let end = end.min(w as f32);
    if end <= start { return None; }

    let first = start as u32;
    let last = (end.ceil() as u32).min(w);
    for x in first..last {
        let left = (x as f32).max(start);
        let right = ((x + 1) as f32).min(end);
        if right > left {
            cov[x as usize] += (right - left) * weight;
        }
    }
    if first < last { Some((first, last)) } else { None }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Extract `(x0, y0, x1, y1)` line tuples from `path`, tessellating Bézier curves.
pub fn collect_lines(path: &[PathSegment]) -> Vec<(f32, f32, f32, f32)> {
    let mut out = Vec::new();
    for seg in path {
        match *seg {
            PathSegment::Move(_, _) => {}
            PathSegment::Line(x0, y0, x1, y1) => {
                out.push((x0, y0, x1, y1));
            }
            PathSegment::Cubic(x0, y0, cp1x, cp1y, cp2x, cp2y, x1, y1) => {
                tessellate_cubic(x0, y0, cp1x, cp1y, cp2x, cp2y, x1, y1, &mut out);
            }
            PathSegment::Quadratic(x0, y0, cpx, cpy, x1, y1) => {
                tessellate_quadratic(x0, y0, cpx, cpy, x1, y1, &mut out);
            }
        }
    }
    out
}

/// Tessellate a cubic Bézier curve into `BEZIER_STEPS` line segments.
#[allow(clippy::too_many_arguments)]
fn tessellate_cubic(
    x0: f32, y0: f32,
    cp1x: f32, cp1y: f32,
    cp2x: f32, cp2y: f32,
    x1: f32, y1: f32,
    out: &mut Vec<(f32, f32, f32, f32)>,
) {
    let mut prev_x = x0;
    let mut prev_y = y0;
    for i in 1..=BEZIER_STEPS {
        let t = i as f32 / BEZIER_STEPS as f32;
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let mt3 = mt2 * mt;
        let t2 = t * t;
        let t3 = t2 * t;
        let x = mt3 * x0 + 3.0 * mt2 * t * cp1x + 3.0 * mt * t2 * cp2x + t3 * x1;
        let y = mt3 * y0 + 3.0 * mt2 * t * cp1y + 3.0 * mt * t2 * cp2y + t3 * y1;
        out.push((prev_x, prev_y, x, y));
        prev_x = x;
        prev_y = y;
    }
}

/// Tessellate a quadratic Bézier curve into `BEZIER_STEPS` line segments.
fn tessellate_quadratic(
    x0: f32, y0: f32,
    cpx: f32, cpy: f32,
    x1: f32, y1: f32,
    out: &mut Vec<(f32, f32, f32, f32)>,
) {
    let mut prev_x = x0;
    let mut prev_y = y0;
    for i in 1..=BEZIER_STEPS {
        let t = i as f32 / BEZIER_STEPS as f32;
        let mt = 1.0 - t;
        let x = mt * mt * x0 + 2.0 * mt * t * cpx + t * t * x1;
        let y = mt * mt * y0 + 2.0 * mt * t * cpy + t * t * y1;
        out.push((prev_x, prev_y, x, y));
        prev_x = x;
        prev_y = y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CanvasColor;

    /// Closed rectangle path in device space.
    fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Vec<PathSegment> {
        vec![
            PathSegment::Move(x, y),
            PathSegment::Line(x, y, x + w, y),
            PathSegment::Line(x + w, y, x + w, y + h),
            PathSegment::Line(x + w, y + h, x, y + h),
            PathSegment::Line(x, y + h, x, y),
        ]
    }

    /// Alpha channel of pixel `(x, y)`.
    fn alpha_at(ctx: &Context2D, x: u32, y: u32) -> u8 {
        ctx.pixels()[((y * ctx.width() + x) * 4 + 3) as usize]
    }

    #[test]
    fn pixel_aligned_edges_stay_fully_opaque() {
        let mut ctx = Context2D::new(8, 8);
        ctx.fill_style = PaintSource::Color(CanvasColor::rgba(255, 0, 0, 255));
        rasterize_fill(&mut ctx, &rect_path(2.0, 2.0, 4.0, 4.0));

        assert_eq!(alpha_at(&ctx, 2, 2), 255, "top-left inside pixel");
        assert_eq!(alpha_at(&ctx, 5, 5), 255, "bottom-right inside pixel");
        assert_eq!(alpha_at(&ctx, 1, 2), 0, "pixel left of the rect");
        assert_eq!(alpha_at(&ctx, 6, 5), 0, "pixel right of the rect");
    }

    #[test]
    fn fractional_edge_gets_partial_coverage() {
        let mut ctx = Context2D::new(8, 8);
        ctx.fill_style = PaintSource::Color(CanvasColor::rgba(255, 0, 0, 255));
        // Left edge cuts pixel column 2 at 25 %, right edge cuts column 5 at 50 %.
        rasterize_fill(&mut ctx, &rect_path(2.75, 2.0, 2.75, 4.0));

        let left = alpha_at(&ctx, 2, 3);
        let right = alpha_at(&ctx, 5, 3);
        assert!((left as i32 - 63).abs() <= 2, "left edge coverage 25 % → ~63, got {left}");
        assert!((right as i32 - 127).abs() <= 2, "right edge coverage 50 % → ~127, got {right}");
        assert_eq!(alpha_at(&ctx, 3, 3), 255, "interior stays opaque");
    }

    #[test]
    fn horizontal_edge_gets_partial_coverage() {
        let mut ctx = Context2D::new(8, 8);
        ctx.fill_style = PaintSource::Color(CanvasColor::rgba(255, 0, 0, 255));
        rasterize_fill(&mut ctx, &rect_path(2.0, 2.5, 4.0, 3.0));

        let top = alpha_at(&ctx, 3, 2);
        assert!((top as i32 - 127).abs() <= 2, "top edge covers half the row, got {top}");
        assert_eq!(alpha_at(&ctx, 3, 3), 255, "interior row stays opaque");
        assert_eq!(alpha_at(&ctx, 3, 1), 0, "row above stays untouched");
    }

    #[test]
    fn stroke_join_is_not_composited_twice() {
        // An L-shaped polyline: the two segment quads overlap in the corner. Painting
        // them separately composites the overlap twice, so a translucent stroke came
        // out darker at every join.
        let mut ctx = Context2D::new(16, 12);
        let path = vec![
            PathSegment::Move(2.0, 3.0),
            PathSegment::Line(2.0, 3.0, 10.0, 3.0),
            PathSegment::Line(10.0, 3.0, 10.0, 8.0),
        ];
        stroke_path(&mut ctx, &path, 2.0, &PaintSource::Color(CanvasColor::rgba(0, 0, 0, 255)), 0.5);

        let away = alpha_at(&ctx, 5, 3);
        let at_join = alpha_at(&ctx, 9, 3);
        assert_eq!(away, 127, "half-transparent stroke away from the join");
        assert_eq!(at_join, away, "join pixel must not be double-composited");
    }

    #[test]
    fn clip_mask_edge_is_anti_aliased() {
        let mask = build_clip_mask(&rect_path(2.5, 2.0, 3.0, 4.0), 8, 8);
        let inside = mask[3 * 8 + 3];
        let edge = mask[3 * 8 + 2];
        let outside = mask[3 * 8 + 1];
        assert_eq!(inside, 255, "fully covered pixel");
        assert!((edge as i32 - 127).abs() <= 2, "half-covered pixel → ~127, got {edge}");
        assert_eq!(outside, 0, "pixel outside the path");
    }

    #[test]
    fn clip_coverage_scales_the_fill() {
        let mut ctx = Context2D::new(8, 8);
        ctx.move_to(2.5, 0.0);
        ctx.line_to(5.5, 0.0);
        ctx.line_to(5.5, 8.0);
        ctx.line_to(2.5, 8.0);
        ctx.close_path();
        ctx.clip();
        ctx.begin_path();
        ctx.fill_style = PaintSource::Color(CanvasColor::rgba(255, 0, 0, 255));
        ctx.fill_rect(0.0, 0.0, 8.0, 8.0);

        assert_eq!(alpha_at(&ctx, 3, 3), 255, "inside the clip");
        let edge = alpha_at(&ctx, 2, 3);
        assert!((edge as i32 - 127).abs() <= 2, "half-clipped pixel → ~127, got {edge}");
        assert_eq!(alpha_at(&ctx, 1, 3), 0, "outside the clip");
    }

    #[test]
    fn spans_left_of_the_canvas_do_not_shift_coverage() {
        let mut ctx = Context2D::new(8, 8);
        ctx.fill_style = PaintSource::Color(CanvasColor::rgba(255, 0, 0, 255));
        rasterize_fill(&mut ctx, &rect_path(-4.0, 2.0, 6.5, 4.0));

        assert_eq!(alpha_at(&ctx, 0, 3), 255, "clamped left edge stays opaque");
        let edge = alpha_at(&ctx, 2, 3);
        assert!((edge as i32 - 127).abs() <= 2, "right edge at x=2.5 → ~127, got {edge}");
    }

    /// Fill `path` with the context's current fill style at full opacity.
    fn rasterize_fill(ctx: &mut Context2D, path: &[PathSegment]) {
        let paint = ctx.fill_style.clone();
        fill_path(ctx, path, &paint, 1.0);
    }

    // ── caps and joins ───────────────────────────────────────────────────────

    /// An L: right along `y = 4`, then down along `x = 14`. With `lineWidth = 4`
    /// the two quads meet at `(14, 4)` and leave the corner above-right of it open.
    fn corner_path() -> Vec<PathSegment> {
        vec![
            PathSegment::Move(4.0, 4.0),
            PathSegment::Line(4.0, 4.0, 14.0, 4.0),
            PathSegment::Line(14.0, 4.0, 14.0, 14.0),
        ]
    }

    /// Stroke `corner_path` with the given join and return the alpha of the pixel
    /// that only a miter reaches: inside the miter square `(14,2)–(16,4)`, outside
    /// the bevel triangle, and clipped by the round arc.
    fn corner_tip_alpha(join: LineJoin, miter_limit: f32) -> u8 {
        let mut ctx = Context2D::new(20, 20);
        ctx.line_join = join;
        ctx.miter_limit = miter_limit;
        stroke_path(&mut ctx, &corner_path(), 4.0, &PaintSource::Color(CanvasColor::rgba(0, 0, 0, 255)), 1.0);
        alpha_at(&ctx, 15, 2)
    }

    #[test]
    fn miter_join_fills_the_outer_corner() {
        assert_eq!(corner_tip_alpha(LineJoin::Miter, 10.0), 255, "miter tip must be solid");
    }

    #[test]
    fn bevel_join_leaves_the_miter_tip_empty() {
        assert_eq!(corner_tip_alpha(LineJoin::Bevel, 10.0), 0, "bevel must not reach the tip");
    }

    #[test]
    fn round_join_covers_part_of_the_miter_tip() {
        // The arc bulges past the bevel chord into that pixel but stops short of the
        // miter tip, so the coverage is strictly between the other two joins.
        let a = corner_tip_alpha(LineJoin::Round, 10.0);
        assert!(a > 0 && a < 200, "round join partial coverage expected, got {a}");
    }

    #[test]
    fn miter_limit_below_the_ratio_falls_back_to_bevel() {
        // A 90° turn has a miter ratio of √2, so a limit of 1.2 must reject it.
        assert_eq!(corner_tip_alpha(LineJoin::Miter, 1.2), 0, "miter over the limit must bevel");
        assert_eq!(corner_tip_alpha(LineJoin::Miter, 1.5), 255, "miter under the limit stays");
    }

    /// Stroke a horizontal segment ending at `(14, 4)` with the given cap and return
    /// the alpha of a pixel just past the end: a square cap covers it fully, a round
    /// cap clips it, a butt cap leaves it untouched.
    fn cap_past_end_alpha(cap: LineCap) -> u8 {
        let mut ctx = Context2D::new(20, 20);
        ctx.line_cap = cap;
        let path = vec![
            PathSegment::Move(4.0, 4.0),
            PathSegment::Line(4.0, 4.0, 14.0, 4.0),
        ];
        stroke_path(&mut ctx, &path, 4.0, &PaintSource::Color(CanvasColor::rgba(0, 0, 0, 255)), 1.0);
        alpha_at(&ctx, 15, 2)
    }

    #[test]
    fn butt_cap_stops_at_the_endpoint() {
        assert_eq!(cap_past_end_alpha(LineCap::Butt), 0, "butt cap must not extend");
    }

    #[test]
    fn square_cap_extends_past_the_endpoint() {
        assert_eq!(cap_past_end_alpha(LineCap::Square), 255, "square cap covers half a width");
    }

    #[test]
    fn round_cap_extends_but_stays_inside_the_disc() {
        let a = cap_past_end_alpha(LineCap::Round);
        assert!(a > 0 && a < 200, "round cap partial coverage expected, got {a}");
    }

    #[test]
    fn a_closed_sub_path_is_joined_rather_than_capped() {
        // The seam of a closed rectangle sits at its top-left corner: a square cap
        // would stick out past it, a join must not.
        let mut ctx = Context2D::new(20, 20);
        ctx.line_cap = LineCap::Square;
        stroke_path(&mut ctx, &rect_path(5.0, 5.0, 10.0, 10.0), 2.0, &PaintSource::Color(CanvasColor::rgba(0, 0, 0, 255)), 1.0);

        assert_eq!(alpha_at(&ctx, 4, 4), 255, "the mitered corner itself is covered");
        assert_eq!(alpha_at(&ctx, 3, 4), 0, "nothing may stick out past the corner");
        assert_eq!(alpha_at(&ctx, 4, 3), 0, "nothing may stick out above the corner");
    }
}
