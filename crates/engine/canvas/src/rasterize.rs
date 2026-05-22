use crate::{CanvasColor, Context2D, PathSegment};

/// Fill `path` using the even-odd scanline algorithm.
pub fn fill_path(ctx: &mut Context2D, path: &[PathSegment], color: CanvasColor) {
    if path.is_empty() { return; }

    // Collect all line segments (Move is just positioning).
    let segments = collect_lines(path);
    if segments.is_empty() { return; }

    let h = ctx.height();
    let w = ctx.width();

    for y in 0..h {
        let yf = y as f32 + 0.5; // scanline center
        let mut xs: Vec<f32> = Vec::new();

        for &(x0, y0, x1, y1) in &segments {
            let (miny, maxy) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
            if yf < miny || yf >= maxy { continue; }
            // x intersection
            let t = (yf - y0) / (y1 - y0);
            xs.push(x0 + t * (x1 - x0));
        }

        xs.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());

        let mut i = 0;
        while i + 1 < xs.len() {
            let xa = xs[i].max(0.0) as u32;
            let xb = xs[i + 1].min(w as f32) as u32;
            for x in xa..xb {
                ctx.set_pixel(x, y, color);
            }
            i += 2;
        }
    }
}

/// Stroke `path` by drawing each line segment as a thick rectangle.
pub fn stroke_path(ctx: &mut Context2D, path: &[PathSegment], lw: f32, color: CanvasColor) {
    if path.is_empty() { return; }
    let half = lw * 0.5;
    let segments = collect_lines(path);

    for (x0, y0, x1, y1) in segments {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt();
        if len < f32::EPSILON { continue; }

        // Perpendicular unit vector scaled by half line width
        let nx = -dy / len * half;
        let ny =  dx / len * half;

        // Four corners of the stroke quad
        let corners = [
            (x0 + nx, y0 + ny),
            (x0 - nx, y0 - ny),
            (x1 - nx, y1 - ny),
            (x1 + nx, y1 + ny),
        ];

        fill_quad(ctx, &corners, color);
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Extract `(x0, y0, x1, y1)` tuples from `path`, skipping Move segments.
fn collect_lines(path: &[PathSegment]) -> Vec<(f32, f32, f32, f32)> {
    path.iter()
        .filter_map(|seg| {
            if let PathSegment::Line(x0, y0, x1, y1) = *seg {
                Some((x0, y0, x1, y1))
            } else {
                None
            }
        })
        .collect()
}

/// Scanline-fill an arbitrary convex quad (4 vertices, winding order ignored).
fn fill_quad(ctx: &mut Context2D, pts: &[(f32, f32); 4], color: CanvasColor) {
    let miny = pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min).max(0.0) as u32;
    let maxy = pts
        .iter()
        .map(|p| p.1)
        .fold(f32::NEG_INFINITY, f32::max)
        .min(ctx.height() as f32) as u32;

    // Build edge list (cyclic)
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
            let (miny_e, maxy_e) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
            if yf < miny_e || yf >= maxy_e { continue; }
            let t = (yf - y0) / (y1 - y0);
            xs.push(x0 + t * (x1 - x0));
        }

        if xs.len() < 2 { continue; }
        xs.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
        let xa = xs[0].max(0.0) as u32;
        let xb = xs[xs.len() - 1].min(ctx.width() as f32) as u32;
        for x in xa..xb {
            ctx.set_pixel(x, y, color);
        }
    }
}
