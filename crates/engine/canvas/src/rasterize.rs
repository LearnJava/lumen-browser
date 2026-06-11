use crate::{Context2D, PaintSource, PathSegment};

/// Number of line segments to use when tessellating a Bézier curve.
const BEZIER_STEPS: usize = 32;

/// Fill `path` using the even-odd scanline algorithm with the given paint source.
pub fn fill_path(ctx: &mut Context2D, path: &[PathSegment], paint: &PaintSource, alpha: f32) {
    if path.is_empty() { return; }

    let segments = collect_lines(path);
    if segments.is_empty() { return; }

    let h = ctx.height();
    let w = ctx.width();

    for y in 0..h {
        let yf = y as f32 + 0.5;
        let mut xs: Vec<f32> = Vec::new();

        for &(x0, y0, x1, y1) in &segments {
            let (miny, maxy) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
            if yf < miny || yf >= maxy { continue; }
            let t = (yf - y0) / (y1 - y0);
            xs.push(x0 + t * (x1 - x0));
        }

        xs.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());

        let mut i = 0;
        while i + 1 < xs.len() {
            let xa = xs[i].max(0.0) as u32;
            let xb = xs[i + 1].min(w as f32) as u32;
            for x in xa..xb {
                if !ctx.pixel_allowed(x, y) { i += 2; break; }
                let mut color = paint.sample(x as f32 + 0.5, y as f32 + 0.5);
                color.a = (color.a as f32 * alpha) as u8;
                ctx.composite_pixel(x, y, color);
            }
            i += 2;
        }
    }
}

/// Stroke `path` by drawing each line segment as a thick rectangle.
pub fn stroke_path(ctx: &mut Context2D, path: &[PathSegment], lw: f32, paint: &PaintSource, alpha: f32) {
    if path.is_empty() { return; }
    let half = lw * 0.5;
    let segments = collect_lines(path);

    for (x0, y0, x1, y1) in segments {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt();
        if len < f32::EPSILON { continue; }

        let nx = -dy / len * half;
        let ny =  dx / len * half;

        let corners = [
            (x0 + nx, y0 + ny),
            (x0 - nx, y0 - ny),
            (x1 - nx, y1 - ny),
            (x1 + nx, y1 + ny),
        ];

        fill_quad(ctx, &corners, paint, alpha);
    }
}

/// Build a boolean clip mask by rasterizing `path` with even-odd rule.
///
/// Returns a `width × height` flat vector; `true` = pixel is inside the path.
pub fn build_clip_mask(path: &[PathSegment], w: u32, h: u32) -> Vec<bool> {
    let segments = collect_lines(path);
    let mut mask = vec![false; (w * h) as usize];

    for y in 0..h {
        let yf = y as f32 + 0.5;
        let mut xs: Vec<f32> = Vec::new();

        for &(x0, y0, x1, y1) in &segments {
            let (miny, maxy) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
            if yf < miny || yf >= maxy { continue; }
            let t = (yf - y0) / (y1 - y0);
            xs.push(x0 + t * (x1 - x0));
        }

        xs.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());

        let mut i = 0;
        while i + 1 < xs.len() {
            let xa = xs[i].max(0.0) as u32;
            let xb = xs[i + 1].min(w as f32) as u32;
            for x in xa..xb {
                mask[(y * w + x) as usize] = true;
            }
            i += 2;
        }
    }

    mask
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

/// Scanline-fill an arbitrary convex quad (4 vertices, winding order ignored).
fn fill_quad(ctx: &mut Context2D, pts: &[(f32, f32); 4], paint: &PaintSource, alpha: f32) {
    let raw_miny = pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
    let raw_maxy = pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);

    let miny = raw_miny.max(0.0) as u32;
    let maxy = ((raw_maxy + 1.0).ceil()).min(ctx.height() as f32) as u32;

    let edges: Vec<(f32, f32, f32, f32)> = (0..4)
        .map(|i| {
            let (x0, y0) = pts[i];
            let (x1, y1) = pts[(i + 1) % 4];
            (x0, y0, x1, y1)
        })
        .collect();

    for y in miny..maxy {
        let yf = y as f32 + 0.5;
        let mut xs: Vec<f32> = Vec::new();

        for &(x0, y0, x1, y1) in &edges {
            if (y1 - y0).abs() < f32::EPSILON { continue; }
            let (miny_e, maxy_e) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
            if yf < miny_e || yf > maxy_e { continue; }
            let t = (yf - y0) / (y1 - y0);
            xs.push(x0 + t * (x1 - x0));
        }

        if xs.len() < 2 { continue; }
        xs.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
        let xa = xs[0].max(0.0) as u32;
        let xb = xs[xs.len() - 1].min(ctx.width() as f32) as u32;
        for x in xa..xb {
            if !ctx.pixel_allowed(x, y) { continue; }
            let mut color = paint.sample(x as f32 + 0.5, y as f32 + 0.5);
            color.a = (color.a as f32 * alpha) as u8;
            ctx.composite_pixel(x, y, color);
        }
    }
}
