//! SVG path rendering: parse `d` attribute → flatten to polyline → tessellate to triangles.
//!
//! Scope: fill-only (even-odd rule), linear + cubic/quadratic Bézier + arc segments.
//! Stroke is implemented as an outline border approximation (deferred GPU path stroking).
//!
//! Pipeline:
//!   parse_svg_path(d) → Vec<PathSegment>
//!   → flatten_path(segments, tolerance) → Vec<[f32;2]> contour list
//!   → tessellate_fill(contours) → Vec<[f32;2]> triangle vertices (flat, 3 verts per tri)

use std::f32::consts::PI;

/// One SVG path command (absolute coords, after normalization).
#[derive(Debug, Clone, PartialEq)]
pub enum PathSegment {
    /// Move to (x, y) — starts a new sub-path.
    MoveTo { x: f32, y: f32 },
    /// Line to (x, y).
    LineTo { x: f32, y: f32 },
    /// Cubic Bézier to (x, y) with control points (cx1, cy1) and (cx2, cy2).
    CubicTo { cx1: f32, cy1: f32, cx2: f32, cy2: f32, x: f32, y: f32 },
    /// Quadratic Bézier to (x, y) with control point (cx, cy).
    QuadTo { cx: f32, cy: f32, x: f32, y: f32 },
    /// Arc to (x, y); SVG arc parameters (rx, ry, x_rotation_deg, large_arc, sweep).
    ArcTo { rx: f32, ry: f32, x_rot_deg: f32, large_arc: bool, sweep: bool, x: f32, y: f32 },
    /// Close the current sub-path.
    Close,
}

/// Parses SVG path `d` attribute into absolute-coordinate segments.
///
/// Handles: M m L l H h V v C c S s Q q T t A a Z z.
/// Ignores unknown tokens. Relative commands are converted to absolute.
#[must_use]
pub fn parse_svg_path(d: &str) -> Vec<PathSegment> {
    let mut out = Vec::new();
    let mut tokens = Tokenizer::new(d);
    let mut cur_x = 0.0f32;
    let mut cur_y = 0.0f32;
    // Last cubic control point (for S), last quad control point (for T).
    let mut last_cp_cubic = None::<(f32, f32)>;
    let mut last_cp_quad = None::<(f32, f32)>;
    // Start of current sub-path (for close).
    let mut sub_start = (0.0f32, 0.0f32);

    while let Some(cmd) = tokens.next_cmd() {
        last_cp_cubic = handle_cmd(
            cmd,
            &mut tokens,
            &mut out,
            &mut cur_x,
            &mut cur_y,
            last_cp_cubic,
            &mut last_cp_quad,
            &mut sub_start,
        );
    }
    out
}

/// Processes one path command letter and all its coordinate groups.
/// Returns `Some((cx, cy))` when this command leaves a cubic CP (for S), else `None`.
#[allow(clippy::too_many_arguments)]
fn handle_cmd(
    cmd: char,
    tokens: &mut Tokenizer,
    out: &mut Vec<PathSegment>,
    cx: &mut f32,
    cy: &mut f32,
    mut last_cubic: Option<(f32, f32)>,
    last_quad: &mut Option<(f32, f32)>,
    sub_start: &mut (f32, f32),
) -> Option<(f32, f32)> {
    let abs = cmd.is_uppercase();
    match cmd.to_ascii_uppercase() {
        'M' => {
            // First coord pair is MoveTo; subsequent pairs are implicit LineTo.
            let mut first = true;
            while let Some([px, py]) = tokens.try_pair() {
                let (ax, ay) = if abs { (px, py) } else { (*cx + px, *cy + py) };
                *cx = ax; *cy = ay;
                if first {
                    *sub_start = (ax, ay);
                    out.push(PathSegment::MoveTo { x: ax, y: ay });
                    first = false;
                } else {
                    out.push(PathSegment::LineTo { x: ax, y: ay });
                }
                last_cubic = None;
                *last_quad = None;
            }
        }
        'L' => {
            while let Some([px, py]) = tokens.try_pair() {
                let (ax, ay) = if abs { (px, py) } else { (*cx + px, *cy + py) };
                *cx = ax; *cy = ay;
                out.push(PathSegment::LineTo { x: ax, y: ay });
                last_cubic = None; *last_quad = None;
            }
        }
        'H' => {
            while let Some(px) = tokens.try_number() {
                let ax = if abs { px } else { *cx + px };
                *cx = ax;
                out.push(PathSegment::LineTo { x: ax, y: *cy });
                last_cubic = None; *last_quad = None;
            }
        }
        'V' => {
            while let Some(py) = tokens.try_number() {
                let ay = if abs { py } else { *cy + py };
                *cy = ay;
                out.push(PathSegment::LineTo { x: *cx, y: ay });
                last_cubic = None; *last_quad = None;
            }
        }
        'C' => {
            while let Some([x1, y1, x2, y2, x, y]) = tokens.try_sextuple() {
                let (ax1, ay1) = if abs { (x1, y1) } else { (*cx + x1, *cy + y1) };
                let (ax2, ay2) = if abs { (x2, y2) } else { (*cx + x2, *cy + y2) };
                let (ax, ay)   = if abs { (x,  y)  } else { (*cx + x,  *cy + y)  };
                last_cubic = Some((ax2, ay2));
                *last_quad = None;
                *cx = ax; *cy = ay;
                out.push(PathSegment::CubicTo { cx1: ax1, cy1: ay1, cx2: ax2, cy2: ay2, x: ax, y: ay });
            }
        }
        'S' => {
            while let Some([x2, y2, x, y]) = tokens.try_quadruple() {
                let (ax2, ay2) = if abs { (x2, y2) } else { (*cx + x2, *cy + y2) };
                let (ax, ay)   = if abs { (x,  y)  } else { (*cx + x,  *cy + y)  };
                let (cx1, cy1) = last_cubic
                    .map(|(lx, ly)| (2.0 * *cx - lx, 2.0 * *cy - ly))
                    .unwrap_or((*cx, *cy));
                last_cubic = Some((ax2, ay2));
                *last_quad = None;
                *cx = ax; *cy = ay;
                out.push(PathSegment::CubicTo { cx1, cy1, cx2: ax2, cy2: ay2, x: ax, y: ay });
            }
        }
        'Q' => {
            while let Some([qx, qy, x, y]) = tokens.try_quadruple() {
                let (aqx, aqy) = if abs { (qx, qy) } else { (*cx + qx, *cy + qy) };
                let (ax, ay)   = if abs { (x,  y)  } else { (*cx + x,  *cy + y)  };
                *last_quad = Some((aqx, aqy));
                last_cubic = None;
                *cx = ax; *cy = ay;
                out.push(PathSegment::QuadTo { cx: aqx, cy: aqy, x: ax, y: ay });
            }
        }
        'T' => {
            while let Some([x, y]) = tokens.try_pair() {
                let (ax, ay) = if abs { (x, y) } else { (*cx + x, *cy + y) };
                let (qx, qy) = last_quad
                    .map(|(lx, ly)| (2.0 * *cx - lx, 2.0 * *cy - ly))
                    .unwrap_or((*cx, *cy));
                *last_quad = Some((qx, qy));
                last_cubic = None;
                *cx = ax; *cy = ay;
                out.push(PathSegment::QuadTo { cx: qx, cy: qy, x: ax, y: ay });
            }
        }
        'A' => {
            while let Some([rx, ry, xr, la, sw, x, y]) = tokens.try_arc_params() {
                let (ax, ay) = if abs { (x, y) } else { (*cx + x, *cy + y) };
                last_cubic = None; *last_quad = None;
                *cx = ax; *cy = ay;
                out.push(PathSegment::ArcTo {
                    rx, ry,
                    x_rot_deg: xr,
                    large_arc: la != 0.0,
                    sweep: sw != 0.0,
                    x: ax, y: ay,
                });
            }
        }
        'Z' => {
            out.push(PathSegment::Close);
            *cx = sub_start.0;
            *cy = sub_start.1;
            last_cubic = None; *last_quad = None;
        }
        _ => {}
    }
    last_cubic
}

// ─── Tokenizer ──────────────────────────────────────────────────────────────

struct Tokenizer<'a> {
    s: &'a str,
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    fn new(s: &'a str) -> Self { Self { s, pos: 0 } }

    fn skip_ws_comma(&mut self) {
        while self.pos < self.s.len() {
            let b = self.s.as_bytes()[self.pos];
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' || b == b',' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// Reads the next path command letter; returns `None` at EOF.
    fn next_cmd(&mut self) -> Option<char> {
        self.skip_ws_comma();
        let b = *self.s.as_bytes().get(self.pos)?;
        if b.is_ascii_alphabetic() {
            self.pos += 1;
            Some(b as char)
        } else {
            None
        }
    }

    /// Tries to read one floating-point number. Returns `None` if the next
    /// token is a command letter or EOF (so callers can loop on coordinate groups).
    fn try_number(&mut self) -> Option<f32> {
        self.skip_ws_comma();
        let rest = &self.s[self.pos..];
        if rest.is_empty() { return None; }
        let b = rest.as_bytes()[0];
        // If the next char is a command letter, stop consuming numbers.
        if b.is_ascii_alphabetic() { return None; }
        // Parse a float: optional sign, digits, optional dot+digits, optional exponent.
        let end = parse_float_end(rest);
        if end == 0 { return None; }
        let num: f32 = rest[..end].parse().ok()?;
        self.pos += end;
        Some(num)
    }

    fn try_pair(&mut self) -> Option<[f32; 2]> {
        let x = self.try_number()?;
        let y = self.try_number()?;
        Some([x, y])
    }

    fn try_quadruple(&mut self) -> Option<[f32; 4]> {
        let [a, b] = self.try_pair()?;
        let [c, d] = self.try_pair()?;
        Some([a, b, c, d])
    }

    fn try_sextuple(&mut self) -> Option<[f32; 6]> {
        let [a, b] = self.try_pair()?;
        let [c, d] = self.try_pair()?;
        let [e, f] = self.try_pair()?;
        Some([a, b, c, d, e, f])
    }

    /// Reads 7 numbers for arc: rx ry x-rotation large-arc-flag sweep-flag x y.
    /// Flags are 0 or 1 (no comma separator allowed between them per spec,
    /// but we accept comma for leniency).
    fn try_arc_params(&mut self) -> Option<[f32; 7]> {
        let rx = self.try_number()?;
        let ry = self.try_number()?;
        let xr = self.try_number()?;
        let la = self.try_number()?;
        let sw = self.try_number()?;
        let x  = self.try_number()?;
        let y  = self.try_number()?;
        Some([rx, ry, xr, la, sw, x, y])
    }
}

/// Returns the byte-length of the floating-point prefix of `s`, or 0 if none.
fn parse_float_end(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 0;
    // optional sign
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') { i += 1; }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() { i += 1; }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() { i += 1; }
    }
    if i == digits_start { return 0; } // no digits at all
    // optional exponent
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let exp_start = i;
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') { i += 1; }
        let exp_digits = i;
        while i < b.len() && b[i].is_ascii_digit() { i += 1; }
        if i == exp_digits { i = exp_start; } // no exponent digits → back up
    }
    i
}

// ─── Flattening (segments → polylines) ──────────────────────────────────────

/// Flatten path segments to a list of closed contours.
///
/// `tolerance` is the max deviation in CSS pixels for Bézier approximation.
/// Each inner `Vec<[f32;2]>` is one closed contour (with the last point = first point
/// implied, i.e. the contour does NOT repeat the start point at the end).
///
/// The current pen position is passed so Arc-to can be converted to Bézier curves.
#[must_use]
pub fn flatten_path(segments: &[PathSegment], tolerance: f32) -> Vec<Vec<[f32; 2]>> {
    let tol = tolerance.max(0.01);
    let mut contours: Vec<Vec<[f32; 2]>> = Vec::new();
    let mut current: Vec<[f32; 2]> = Vec::new();
    let mut cx = 0.0f32;
    let mut cy = 0.0f32;
    let mut sub_start = [0.0f32, 0.0f32];

    for seg in segments {
        match seg {
            PathSegment::MoveTo { x, y } => {
                if current.len() >= 2 {
                    contours.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
                cx = *x; cy = *y;
                sub_start = [cx, cy];
                current.push([cx, cy]);
            }
            PathSegment::LineTo { x, y } => {
                cx = *x; cy = *y;
                current.push([cx, cy]);
            }
            PathSegment::CubicTo { cx1, cy1, cx2, cy2, x, y } => {
                flatten_cubic(cx, cy, *cx1, *cy1, *cx2, *cy2, *x, *y, tol, &mut current);
                cx = *x; cy = *y;
            }
            PathSegment::QuadTo { cx: qx, cy: qy, x, y } => {
                // Elevate quadratic to cubic: CP1 = P0 + 2/3*(Q-P0), CP2 = P1 + 2/3*(Q-P1).
                let p0x = cx; let p0y = cy;
                let p3x = *x; let p3y = *y;
                let c1x = p0x + 2.0 / 3.0 * (*qx - p0x);
                let c1y = p0y + 2.0 / 3.0 * (*qy - p0y);
                let c2x = p3x + 2.0 / 3.0 * (*qx - p3x);
                let c2y = p3y + 2.0 / 3.0 * (*qy - p3y);
                flatten_cubic(p0x, p0y, c1x, c1y, c2x, c2y, p3x, p3y, tol, &mut current);
                cx = p3x; cy = p3y;
            }
            PathSegment::ArcTo { rx, ry, x_rot_deg, large_arc, sweep, x, y } => {
                // Convert SVG arc to cubic Bézier curves.
                flatten_svg_arc(cx, cy, *rx, *ry, *x_rot_deg, *large_arc, *sweep, *x, *y, tol, &mut current);
                cx = *x; cy = *y;
            }
            PathSegment::Close => {
                if !current.is_empty() {
                    // Close back to sub-path start.
                    let first = sub_start;
                    let needs_close_point = current.last().is_some_and(|last| {
                        (last[0] - first[0]).abs() > 1e-4 || (last[1] - first[1]).abs() > 1e-4
                    });
                    if needs_close_point {
                        current.push(first);
                    }
                    cx = first[0]; cy = first[1];
                    if current.len() >= 2 {
                        contours.push(std::mem::take(&mut current));
                    } else {
                        current.clear();
                    }
                }
            }
        }
    }
    if current.len() >= 2 {
        contours.push(current);
    }
    contours
}

/// Flatten a cubic Bézier P0→P3 with control points P1, P2 into `pts`.
/// Uses recursive subdivision until the curve is within `tol` pixels of a line.
#[allow(clippy::too_many_arguments)]
fn flatten_cubic(
    p0x: f32, p0y: f32,
    p1x: f32, p1y: f32,
    p2x: f32, p2y: f32,
    p3x: f32, p3y: f32,
    tol: f32,
    pts: &mut Vec<[f32; 2]>,
) {
    // Flatness test: max deviation of control polygon from chord P0→P3.
    let dx = p3x - p0x;
    let dy = p3y - p0y;
    let len_sq = dx * dx + dy * dy;
    let d1 = if len_sq < 1e-10 {
        let ex = p1x - p0x; let ey = p1y - p0y;
        (ex * ex + ey * ey).sqrt()
    } else {
        let len = len_sq.sqrt();
        let nx = dy / len; let ny = -dx / len;
        ((p1x - p0x) * nx + (p1y - p0y) * ny).abs()
            .max(((p2x - p0x) * nx + (p2y - p0y) * ny).abs())
    };
    if d1 <= tol {
        pts.push([p3x, p3y]);
        return;
    }
    // De Casteljau midpoint subdivision.
    let m01x = (p0x + p1x) * 0.5; let m01y = (p0y + p1y) * 0.5;
    let m12x = (p1x + p2x) * 0.5; let m12y = (p1y + p2y) * 0.5;
    let m23x = (p2x + p3x) * 0.5; let m23y = (p2y + p3y) * 0.5;
    let m012x = (m01x + m12x) * 0.5; let m012y = (m01y + m12y) * 0.5;
    let m123x = (m12x + m23x) * 0.5; let m123y = (m12y + m23y) * 0.5;
    let mx = (m012x + m123x) * 0.5; let my = (m012y + m123y) * 0.5;
    flatten_cubic(p0x, p0y, m01x, m01y, m012x, m012y, mx, my, tol, pts);
    flatten_cubic(mx, my, m123x, m123y, m23x, m23y, p3x, p3y, tol, pts);
}

/// Convert SVG arc to cubic Bézier curves and append flattened points.
/// Implements the W3C SVG spec §B.2.4 endpoint-to-center conversion.
#[allow(clippy::too_many_arguments)]
fn flatten_svg_arc(
    x1: f32, y1: f32,
    mut rx: f32, mut ry: f32,
    x_rot_deg: f32,
    large_arc: bool,
    sweep: bool,
    x2: f32, y2: f32,
    tol: f32,
    pts: &mut Vec<[f32; 2]>,
) {
    if rx == 0.0 || ry == 0.0 {
        pts.push([x2, y2]);
        return;
    }
    if (x1 - x2).abs() < 1e-6 && (y1 - y2).abs() < 1e-6 {
        return;
    }
    rx = rx.abs();
    ry = ry.abs();

    let phi = x_rot_deg * PI / 180.0;
    let (sin_phi, cos_phi) = phi.sin_cos();

    // Step 1: compute (x1', y1').
    let dx = (x1 - x2) * 0.5;
    let dy = (y1 - y2) * 0.5;
    let x1p =  cos_phi * dx + sin_phi * dy;
    let y1p = -sin_phi * dx + cos_phi * dy;

    // Step 2: compute (cx', cy').
    let x1p2 = x1p * x1p;
    let y1p2 = y1p * y1p;
    let rx2 = rx * rx;
    let ry2 = ry * ry;

    // Scale radii if needed.
    let lambda = x1p2 / rx2 + y1p2 / ry2;
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s; ry *= s;
    }
    let rx2 = rx * rx;
    let ry2 = ry * ry;

    let num = (rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2).max(0.0);
    let den = rx2 * y1p2 + ry2 * x1p2;
    let sq = if den.abs() < 1e-10 { 0.0 } else { (num / den).sqrt() };
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let cxp =  sign * sq * rx * y1p / ry;
    let cyp = -sign * sq * ry * x1p / rx;

    // Step 3: compute (cx, cy).
    let cx = cos_phi * cxp - sin_phi * cyp + (x1 + x2) * 0.5;
    let cy = sin_phi * cxp + cos_phi * cyp + (y1 + y2) * 0.5;

    // Step 4: compute θ1 and Δθ.
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;
    let theta1 = angle_vec(1.0, 0.0, ux, uy);
    let mut d_theta = angle_vec(ux, uy, vx, vy);
    if !sweep && d_theta > 0.0 { d_theta -= 2.0 * PI; }
    if  sweep && d_theta < 0.0 { d_theta += 2.0 * PI; }

    // Split arc into segments of at most 90° each and convert each to cubic.
    let n = (d_theta.abs() / (PI / 2.0)).ceil() as i32;
    let n = n.max(1);
    let d = d_theta / n as f32;

    let mut p_cur = [x1, y1];
    for i in 0..n {
        let t0 = theta1 + d * i as f32;
        let t1 = theta1 + d * (i as f32 + 1.0);
        let (c0x, c0y, c1x, c1y, ex, ey) = arc_seg_to_cubic(cx, cy, rx, ry, phi, t0, t1);
        // The cubic start is p_cur; append flattened cubic.
        flatten_cubic(p_cur[0], p_cur[1], c0x, c0y, c1x, c1y, ex, ey, tol, pts);
        p_cur = [ex, ey];
    }
}

/// Converts one arc segment [t0, t1] (both in radians, |t1-t0| ≤ π/2) to
/// a cubic Bézier approximation. Returns (cp1x, cp1y, cp2x, cp2y, endx, endy).
fn arc_seg_to_cubic(
    cx: f32, cy: f32, rx: f32, ry: f32, phi: f32, t0: f32, t1: f32,
) -> (f32, f32, f32, f32, f32, f32) {
    let alpha = (4.0 / 3.0) * ((t1 - t0) / 4.0).tan();
    let (sin_t0, cos_t0) = t0.sin_cos();
    let (sin_t1, cos_t1) = t1.sin_cos();
    let (sin_phi, cos_phi) = phi.sin_cos();

    let dx1 = -rx * sin_t0;
    let dy1 =  ry * cos_t0;
    let dx2 = -rx * sin_t1;
    let dy2 =  ry * cos_t1;

    let p1x = cx + cos_phi * rx * cos_t0 - sin_phi * ry * sin_t0;
    let p1y = cy + sin_phi * rx * cos_t0 + cos_phi * ry * sin_t0;
    let p4x = cx + cos_phi * rx * cos_t1 - sin_phi * ry * sin_t1;
    let p4y = cy + sin_phi * rx * cos_t1 + cos_phi * ry * sin_t1;

    let c1x = p1x + alpha * (cos_phi * dx1 - sin_phi * dy1);
    let c1y = p1y + alpha * (sin_phi * dx1 + cos_phi * dy1);
    let c2x = p4x - alpha * (cos_phi * dx2 - sin_phi * dy2);
    let c2y = p4y - alpha * (sin_phi * dx2 + cos_phi * dy2);

    (c1x, c1y, c2x, c2y, p4x, p4y)
}

/// Signed angle from vector (ux,uy) to (vx,vy), in radians ∈ (-π, π].
fn angle_vec(ux: f32, uy: f32, vx: f32, vy: f32) -> f32 {
    let dot = ux * vx + uy * vy;
    let cross = ux * vy - uy * vx;
    let len_u = (ux * ux + uy * uy).sqrt().max(1e-10);
    let len_v = (vx * vx + vy * vy).sqrt().max(1e-10);
    let cos_a = (dot / (len_u * len_v)).clamp(-1.0, 1.0);
    let a = cos_a.acos();
    if cross < 0.0 { -a } else { a }
}

// ─── Triangulation (ear-clipping) ───────────────────────────────────────────

/// Tessellate a single closed polygon (no holes) using ear-clipping.
/// Returns flat triangle vertex list (3 `[f32;2]` per triangle).
///
/// Uses the even-odd fill rule by tessellating each contour independently.
/// For paths with multiple contours (holes), the caller runs this per contour
/// and the GPU even-odd stencil pass handles the winding — or we use the
/// simple nonzero rule (fills everything).
///
/// Phase 0: nonzero fill (single contour per call).
#[must_use]
pub fn tessellate_polygon(pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    if pts.len() < 3 {
        return Vec::new();
    }
    // Remove duplicate last point if polygon is explicitly closed.
    let ring: Vec<[f32; 2]> = if pts.len() > 1 {
        let first = pts[0];
        let last  = *pts.last().unwrap();
        if (first[0] - last[0]).abs() < 1e-6 && (first[1] - last[1]).abs() < 1e-6 {
            pts[..pts.len() - 1].to_vec()
        } else {
            pts.to_vec()
        }
    } else {
        pts.to_vec()
    };

    if ring.len() < 3 {
        return Vec::new();
    }

    // Ensure counter-clockwise winding (positive area) so ear detection is correct.
    let mut verts = ring;
    if signed_area(&verts) < 0.0 {
        verts.reverse();
    }

    ear_clip(&verts)
}

/// Tessellate a path (all contours) into triangles. Multi-contour paths are
/// simply all tessellated and concatenated — this gives correct results for
/// most SVG icons where contours are disjoint.
#[must_use]
pub fn tessellate_fill(contours: &[Vec<[f32; 2]>]) -> Vec<[f32; 2]> {
    contours.iter().flat_map(|c| tessellate_polygon(c)).collect()
}

/// Signed area of polygon (positive = CCW in Y-down coordinate system).
fn signed_area(pts: &[[f32; 2]]) -> f32 {
    let n = pts.len();
    let mut area = 0.0f32;
    for i in 0..n {
        let j = (i + 1) % n;
        area += pts[i][0] * pts[j][1];
        area -= pts[j][0] * pts[i][1];
    }
    area * 0.5
}

/// Ear-clipping triangulation for a simple (non-self-intersecting) polygon.
/// Time: O(n²). Correct for convex and most concave polygons.
fn ear_clip(orig: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut tris = Vec::with_capacity((orig.len() - 2) * 3);
    // Working index list.
    let mut idx: Vec<usize> = (0..orig.len()).collect();

    let mut iter = 0usize;
    while idx.len() > 3 {
        let n = idx.len();
        let mut found = false;
        for i in 0..n {
            let a = idx[(i + n - 1) % n];
            let b = idx[i];
            let c = idx[(i + 1) % n];
            if is_ear(orig, &idx, a, b, c) {
                tris.push(orig[a]);
                tris.push(orig[b]);
                tris.push(orig[c]);
                idx.remove(i);
                found = true;
                break;
            }
        }
        // Guard against infinite loop (degenerate polygons).
        iter += 1;
        if !found || iter > orig.len() * orig.len() + 10 {
            break;
        }
    }
    // Last triangle.
    if idx.len() == 3 {
        tris.push(orig[idx[0]]);
        tris.push(orig[idx[1]]);
        tris.push(orig[idx[2]]);
    }
    tris
}

/// True if triangle (a, b, c) is a valid ear: convex and contains no other vertex.
fn is_ear(pts: &[[f32; 2]], idx: &[usize], a: usize, b: usize, c: usize) -> bool {
    let pa = pts[a]; let pb = pts[b]; let pc = pts[c];
    // Must be CCW (convex ear).
    if cross2d(pa, pb, pc) <= 0.0 { return false; }
    // No other polygon vertex inside triangle.
    for &j in idx {
        if j == a || j == b || j == c { continue; }
        if point_in_triangle(pts[j], pa, pb, pc) { return false; }
    }
    true
}

/// 2-D cross product of vectors (b-a) × (c-a). Positive = CCW in Y-down.
fn cross2d(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// True if point p is strictly inside triangle (a, b, c) — all cross products same sign.
fn point_in_triangle(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    let d0 = cross2d(a, b, p);
    let d1 = cross2d(b, c, p);
    let d2 = cross2d(c, a, p);
    let has_neg = d0 < 0.0 || d1 < 0.0 || d2 < 0.0;
    let has_pos = d0 > 0.0 || d1 > 0.0 || d2 > 0.0;
    !(has_neg && has_pos)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Parser ──────────────────────────────────────────────────────────────

    #[test]
    fn parse_moveto_lineto() {
        let segs = parse_svg_path("M 10 20 L 30 40");
        assert_eq!(segs, vec![
            PathSegment::MoveTo { x: 10.0, y: 20.0 },
            PathSegment::LineTo { x: 30.0, y: 40.0 },
        ]);
    }

    #[test]
    fn parse_relative_moveto_lineto() {
        let segs = parse_svg_path("m 10 20 l 5 5");
        assert_eq!(segs, vec![
            PathSegment::MoveTo { x: 10.0, y: 20.0 },
            PathSegment::LineTo { x: 15.0, y: 25.0 },
        ]);
    }

    #[test]
    fn parse_close_resets_pen() {
        let segs = parse_svg_path("M 0 0 L 10 0 Z M 20 20");
        assert_eq!(segs[0], PathSegment::MoveTo { x: 0.0, y: 0.0 });
        assert_eq!(segs[1], PathSegment::LineTo { x: 10.0, y: 0.0 });
        assert_eq!(segs[2], PathSegment::Close);
        assert_eq!(segs[3], PathSegment::MoveTo { x: 20.0, y: 20.0 });
    }

    #[test]
    fn parse_horizontal_vertical() {
        let segs = parse_svg_path("M 0 0 H 10 V 20");
        assert_eq!(segs, vec![
            PathSegment::MoveTo { x: 0.0, y: 0.0 },
            PathSegment::LineTo { x: 10.0, y: 0.0 },
            PathSegment::LineTo { x: 10.0, y: 20.0 },
        ]);
    }

    #[test]
    fn parse_cubic_bezier() {
        let segs = parse_svg_path("M 0 0 C 1 2 3 4 5 6");
        assert_eq!(segs, vec![
            PathSegment::MoveTo { x: 0.0, y: 0.0 },
            PathSegment::CubicTo { cx1: 1.0, cy1: 2.0, cx2: 3.0, cy2: 4.0, x: 5.0, y: 6.0 },
        ]);
    }

    #[test]
    fn parse_quadratic_bezier() {
        let segs = parse_svg_path("M 0 0 Q 5 10 10 0");
        assert_eq!(segs, vec![
            PathSegment::MoveTo { x: 0.0, y: 0.0 },
            PathSegment::QuadTo { cx: 5.0, cy: 10.0, x: 10.0, y: 0.0 },
        ]);
    }

    #[test]
    fn parse_smooth_cubic_s() {
        // S uses mirror of previous cubic CP2.
        let segs = parse_svg_path("M 0 0 C 1 2 3 4 5 6 S 7 8 10 0");
        // After C: last CP2 = (3,4), end = (5,6). Mirror = 2*(5,6)-(3,4) = (7,8).
        assert_eq!(segs.len(), 3);
        if let PathSegment::CubicTo { cx1, cy1, cx2, cy2, x, y } = segs[2] {
            assert!((cx1 - 7.0).abs() < 1e-4, "cx1={cx1}");
            assert!((cy1 - 8.0).abs() < 1e-4, "cy1={cy1}");
            assert!((cx2 - 7.0).abs() < 1e-4, "cx2={cx2}");
            assert!((cy2 - 8.0).abs() < 1e-4, "cy2={cy2}");
            assert!((x   - 10.0).abs() < 1e-4, "x={x}");
            assert!((y   - 0.0).abs()  < 1e-4, "y={y}");
        } else {
            panic!("expected CubicTo for S, got {:?}", segs[2]);
        }
    }

    #[test]
    fn parse_arc() {
        let segs = parse_svg_path("M 0 0 A 25 26 -30 0 1 50 50");
        assert_eq!(segs.len(), 2);
        if let PathSegment::ArcTo { rx, ry, x_rot_deg, large_arc, sweep, x, y } = segs[1] {
            assert!((rx - 25.0).abs() < 1e-4);
            assert!((ry - 26.0).abs() < 1e-4);
            assert!((x_rot_deg - (-30.0)).abs() < 1e-4);
            assert!(!large_arc);
            assert!(sweep);
            assert!((x - 50.0).abs() < 1e-4);
            assert!((y - 50.0).abs() < 1e-4);
        } else {
            panic!("expected ArcTo, got {:?}", segs[1]);
        }
    }

    #[test]
    fn parse_implicit_lineto_after_moveto() {
        // Per SVG spec, extra pairs after M are implicit L commands.
        let segs = parse_svg_path("M 0 0 10 20 30 40");
        assert_eq!(segs, vec![
            PathSegment::MoveTo { x: 0.0, y: 0.0 },
            PathSegment::LineTo { x: 10.0, y: 20.0 },
            PathSegment::LineTo { x: 30.0, y: 40.0 },
        ]);
    }

    #[test]
    fn parse_empty_path() {
        assert!(parse_svg_path("").is_empty());
        assert!(parse_svg_path("   ").is_empty());
    }

    // ── Flattening ──────────────────────────────────────────────────────────

    #[test]
    fn flatten_triangle() {
        let segs = parse_svg_path("M 0 0 L 10 0 L 5 10 Z");
        let contours = flatten_path(&segs, 0.5);
        assert_eq!(contours.len(), 1);
        // Triangle: 3 vertices + close-point = 4 in raw, but we trim the repeated start.
        let c = &contours[0];
        assert!(c.len() >= 3 && c.len() <= 4, "len={}", c.len());
    }

    #[test]
    fn flatten_cubic_circle_approx() {
        // Approximate circle with 4 cubic Bézier segments; tolerance 0.5 px.
        let segs = parse_svg_path(
            "M 100 50 \
             C 100 77.614 77.614 100 50 100 \
             C 22.386 100 0 77.614 0 50 \
             C 0 22.386 22.386 0 50 0 \
             C 77.614 0 100 22.386 100 50 Z"
        );
        let contours = flatten_path(&segs, 0.5);
        assert_eq!(contours.len(), 1);
        assert!(contours[0].len() > 10, "circle should have many pts: {}", contours[0].len());
    }

    #[test]
    fn flatten_arc_semicircle() {
        // Semicircle arc from (100,50) to (-100,50) via lower half.
        let segs = parse_svg_path("M 100 50 A 100 100 0 0 1 -100 50");
        let contours = flatten_path(&segs, 0.5);
        assert_eq!(contours.len(), 1);
        assert!(contours[0].len() > 3, "arc should generate >3 pts: {}", contours[0].len());
    }

    // ── Tessellation ────────────────────────────────────────────────────────

    #[test]
    fn tessellate_triangle_gives_one_triangle() {
        let pts = vec![[0.0f32, 0.0], [10.0, 0.0], [5.0, 10.0]];
        let tris = tessellate_polygon(&pts);
        assert_eq!(tris.len(), 3, "one triangle = 3 vertices");
    }

    #[test]
    fn tessellate_quad_gives_two_triangles() {
        // Axis-aligned rectangle: 4 verts → 2 triangles → 6 vertices.
        let pts = vec![[0.0f32, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let tris = tessellate_polygon(&pts);
        assert_eq!(tris.len(), 6, "quad → 2 triangles → 6 verts");
    }

    #[test]
    fn tessellate_degenerate_line_gives_nothing() {
        let pts = vec![[0.0f32, 0.0], [10.0, 0.0]];
        assert!(tessellate_polygon(&pts).is_empty());
    }

    #[test]
    fn tessellate_empty_gives_nothing() {
        assert!(tessellate_polygon(&[]).is_empty());
    }

    #[test]
    fn tessellate_pentagon() {
        // Regular pentagon: 5 verts → 3 triangles → 9 vertices.
        let n = 5;
        let pts: Vec<[f32; 2]> = (0..n).map(|i| {
            let a = 2.0 * PI * i as f32 / n as f32 - PI / 2.0;
            [50.0 + 40.0 * a.cos(), 50.0 + 40.0 * a.sin()]
        }).collect();
        let tris = tessellate_polygon(&pts);
        assert_eq!(tris.len(), (n - 2) * 3, "pentagon → 3 triangles");
    }

    #[test]
    fn tessellate_fill_two_contours() {
        let contour1 = vec![[0.0f32,0.0],[10.0,0.0],[5.0,10.0]];
        let contour2 = vec![[20.0,0.0],[30.0,0.0],[25.0,10.0]];
        let tris = tessellate_fill(&[contour1, contour2]);
        assert_eq!(tris.len(), 6); // 1 tri * 3 + 1 tri * 3
    }

    #[test]
    fn full_pipeline_rect_path() {
        // SVG rectangle rendered as path: M x y H x2 V y2 H x1 Z
        let segs = parse_svg_path("M 0 0 H 100 V 50 H 0 Z");
        let contours = flatten_path(&segs, 0.5);
        let tris = tessellate_fill(&contours);
        assert!(!tris.is_empty());
        // Area of triangles ≈ 100 * 50 = 5000.
        let area: f32 = tris.chunks(3).map(|t| {
            cross2d(t[0], t[1], t[2]).abs() * 0.5
        }).sum();
        assert!((area - 5000.0).abs() < 50.0, "area={area}");
    }
}
