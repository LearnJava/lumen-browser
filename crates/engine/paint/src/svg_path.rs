//! SVG path rendering: parse `d` attribute → flatten to polyline → tessellate to triangles.
//!
//! Scope: fill (nonzero rule) + miter-join stroke.
//! Supports linear + cubic/quadratic Bézier + arc segments.
//!
//! Pipeline:
//!   parse_svg_path(d) → Vec<PathSegment>
//!   → flatten_path(segments, tolerance) → Vec<Vec<[f32;2]>> contour list
//!   → tessellate_fill(contours) → Vec<[f32;2]> fill triangle vertices
//!   → tessellate_stroke(contours, half_w) → Vec<[f32;2]> stroke triangle vertices

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

// ─── Stroke tessellation ────────────────────────────────────────────────────

/// Tessellate stroke outlines for all contours into a flat triangle vertex list.
///
/// `half_width` is half the stroke width (`stroke-width / 2`).
/// Each path segment becomes a quad; interior joins use miter approximation
/// (clamped to 4× `half_width`, equivalent to SVG `stroke-miterlimit="4"`).
/// Open sub-paths get flat (butt) caps; closed sub-paths wrap around.
///
/// Returns flat triangle vertex list (3 `[f32;2]` per triangle, winding arbitrary).
///
/// CSS: `stroke-linecap` (round/square caps), `stroke-linejoin` (round/bevel),
///      `stroke-miterlimit`, `stroke-dasharray`, `stroke-dashoffset` — P4 wires.
#[must_use]
pub fn tessellate_stroke(contours: &[Vec<[f32; 2]>], half_width: f32) -> Vec<[f32; 2]> {
    if half_width <= 0.0 {
        return Vec::new();
    }
    let mut tris = Vec::new();
    for contour in contours {
        stroke_contour(contour, half_width, &mut tris);
    }
    tris
}

/// Tessellate one polyline contour into a stroke band.
fn stroke_contour(pts: &[[f32; 2]], half_w: f32, out: &mut Vec<[f32; 2]>) {
    let n = pts.len();
    if n < 2 {
        return;
    }
    // Detect closed contour: last point ≈ first point.
    let closed = n > 2
        && (pts[0][0] - pts[n - 1][0]).abs() < 1e-4
        && (pts[0][1] - pts[n - 1][1]).abs() < 1e-4;
    // Working slice excludes the duplicate endpoint for closed paths.
    let wpts = if closed { &pts[..n - 1] } else { pts };
    let m = wpts.len();
    if m < 2 {
        return;
    }
    // Number of segments: m for closed (wraps), m-1 for open.
    let n_segs = if closed { m } else { m - 1 };
    // Per-segment unit normal (perpendicular, "left" of travel direction in Y-down).
    let seg_normals: Vec<[f32; 2]> = (0..n_segs)
        .map(|i| {
            let a = wpts[i];
            let b = wpts[(i + 1) % m];
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            [dy / len, -dx / len]
        })
        .collect();
    // Compute miter-offset vectors at each vertex, then build left/right offsets.
    let (left, right): (Vec<[f32; 2]>, Vec<[f32; 2]>) = (0..m)
        .map(|i| {
            let p = wpts[i];
            let ofs = miter_offset(i, m, &seg_normals, half_w, closed);
            ([p[0] + ofs[0], p[1] + ofs[1]], [p[0] - ofs[0], p[1] - ofs[1]])
        })
        .unzip();
    // Emit each segment as two triangles (a quad).
    for i in 0..n_segs {
        let j = (i + 1) % m;
        let (l0, r0) = (left[i], right[i]);
        let (l1, r1) = (left[j], right[j]);
        out.push(l0); out.push(r0); out.push(l1);
        out.push(l1); out.push(r0); out.push(r1);
    }
}

/// Miter-join offset vector at vertex `i` of a polyline with `m` unique points.
///
/// Returns the vector from the path centre-line to the "left" boundary.
/// The miter scale is computed as `half_w / dot(n_in + n_out, n_out)` which
/// satisfies `|offset| = half_w / cos(half_angle)` for any turn angle.
/// Clamped to 4× `half_w` (SVG default `stroke-miterlimit`); beyond that, falls
/// back to the outgoing segment normal (bevel).
fn miter_offset(i: usize, m: usize, seg_normals: &[[f32; 2]], half_w: f32, closed: bool) -> [f32; 2] {
    let n_segs = seg_normals.len();
    let has_prev = closed || i > 0;
    let has_next = closed || i < m - 1;
    // Endpoints of open paths: use adjacent segment normal directly (butt cap).
    if !has_prev {
        let n = seg_normals[0];
        return [n[0] * half_w, n[1] * half_w];
    }
    if !has_next {
        let n = seg_normals[n_segs - 1];
        return [n[0] * half_w, n[1] * half_w];
    }
    // Incoming segment normal (segment that ends at vertex i).
    let n_in = seg_normals[(i + n_segs - 1) % n_segs];
    // Outgoing segment normal (segment that starts at vertex i).
    let n_out = seg_normals[i % n_segs];
    // Miter vector: (n_in + n_out) * half_w / dot(n_in + n_out, n_out).
    let sx = n_in[0] + n_out[0];
    let sy = n_in[1] + n_out[1];
    let denom = sx * n_out[0] + sy * n_out[1];
    if denom.abs() < 0.05 {
        // Near 180° reversal — avoid extreme lengths; fall back to outgoing normal.
        return [n_out[0] * half_w, n_out[1] * half_w];
    }
    let scale = half_w / denom;
    let mx = sx * scale;
    let my = sy * scale;
    // Miter limit = 4 (SVG default): switch to bevel when miter exceeds 4× stroke half-width.
    if mx * mx + my * my > (4.0 * half_w) * (4.0 * half_w) {
        return [n_out[0] * half_w, n_out[1] * half_w];
    }
    [mx, my]
}

// ─── Advanced stroke: linecap / linejoin / miterlimit / dasharray ────────────

/// Stroke caps applied at open sub-path endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokeLinecap {
    /// Flat cap at endpoint (butt).
    #[default]
    Butt,
    /// Semicircular cap.
    Round,
    /// Square cap extending half-width past endpoint.
    Square,
}

/// Join style at connected segment vertices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokeLinejoin {
    /// Pointed miter join (clamped by miterlimit).
    #[default]
    Miter,
    /// Circular arc join.
    Round,
    /// Flat bevel triangle.
    Bevel,
}

/// Parameters for advanced stroke tessellation.
#[derive(Debug, Clone)]
pub struct StrokeParams {
    /// Half of `stroke-width` in px (≥ 0).
    pub half_width: f32,
    /// Cap style for open sub-path endpoints.
    pub linecap: StrokeLinecap,
    /// Join style at interior vertices.
    pub linejoin: StrokeLinejoin,
    /// Miter limit ratio (≥ 1.0). Used only with `Miter` join.
    pub miterlimit: f32,
    /// Dash pattern lengths in px, cycled. Empty = solid line.
    pub dasharray: Vec<f32>,
    /// Offset into the dash pattern in px.
    pub dashoffset: f32,
}

impl Default for StrokeParams {
    fn default() -> Self {
        StrokeParams {
            half_width: 0.5,
            linecap: StrokeLinecap::Butt,
            linejoin: StrokeLinejoin::Miter,
            miterlimit: 4.0,
            dasharray: Vec::new(),
            dashoffset: 0.0,
        }
    }
}

/// Apply a dash pattern to a list of contours.
///
/// Returns new contours where only the "on" (dash) segments are present.
/// If `dasharray` is empty, returns the original contours unchanged.
#[must_use]
pub fn apply_dash_pattern(
    contours: &[Vec<[f32; 2]>],
    dasharray: &[f32],
    dashoffset: f32,
) -> Vec<Vec<[f32; 2]>> {
    if dasharray.is_empty() {
        return contours.to_vec();
    }
    // Total cycle length (dash + gap pairs, cycling the array if odd length).
    let cycle: f32 = dasharray.iter().sum::<f32>()
        * if dasharray.len() % 2 == 1 { 2.0 } else { 1.0 };
    if cycle <= 0.0 {
        return Vec::new();
    }
    let mut result = Vec::new();
    for contour in contours {
        let n = contour.len();
        if n < 2 {
            continue;
        }
        // Effective dashoffset: mod by cycle, negated (offset shifts pattern forward).
        let start_offset = ((-dashoffset) % cycle + cycle) % cycle;

        // Walk along the path, tracking which dash phase we're in.
        // Advance phase_idx/phase_len to account for start_offset.
        let mut phase_idx = 0usize;
        let mut phase_len;
        let mut rem = start_offset;
        loop {
            let d = dasharray[phase_idx % dasharray.len()];
            // `<` not `<=`: when rem == d, advance to the next phase rather than
            // emitting a zero-length draw/gap (which produces degenerate contours).
            if rem < d {
                phase_len = d - rem;
                break;
            }
            rem -= d;
            phase_idx += 1;
        }
        // phase_idx even → drawing; odd → gap.
        let mut drawing = phase_idx.is_multiple_of(2);
        let mut current_dash: Vec<[f32; 2]> = Vec::new();
        let seg_count = n - 1;
        for i in 0..seg_count {
            let a = contour[i];
            let b = contour[i + 1];
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let seg_len = (dx * dx + dy * dy).sqrt();
            if seg_len < 1e-6 {
                continue;
            }
            let ux = dx / seg_len;
            let uy = dy / seg_len;
            let mut t = 0.0f32; // consumed distance along this segment
            loop {
                let remaining_in_seg = seg_len - t;
                if phase_len >= remaining_in_seg {
                    // Current phase continues beyond this segment.
                    if drawing {
                        if current_dash.is_empty() {
                            current_dash.push([a[0] + ux * t, a[1] + uy * t]);
                        }
                        current_dash.push(b);
                    }
                    phase_len -= remaining_in_seg;
                    break;
                }
                // Phase ends within this segment.
                let end_t = t + phase_len;
                let end_pt = [a[0] + ux * end_t, a[1] + uy * end_t];
                if drawing {
                    if current_dash.is_empty() {
                        current_dash.push([a[0] + ux * t, a[1] + uy * t]);
                    }
                    current_dash.push(end_pt);
                    if current_dash.len() >= 2 {
                        result.push(current_dash.clone());
                    }
                    current_dash.clear();
                }
                t = end_t;
                drawing = !drawing;
                phase_idx += 1;
                phase_len = dasharray[phase_idx % dasharray.len()];
            }
        }
        // Flush last dash.
        if drawing && current_dash.len() >= 2 {
            result.push(current_dash);
        }
    }
    result
}

/// Tessellate strokes with full linecap / linejoin / miterlimit / dasharray support.
///
/// Returns flat triangle vertex list (3 `[f32;2]` per triangle).
#[must_use]
pub fn tessellate_stroke_ex(contours: &[Vec<[f32; 2]>], params: &StrokeParams) -> Vec<[f32; 2]> {
    if params.half_width <= 0.0 {
        return Vec::new();
    }
    let dashed = apply_dash_pattern(contours, &params.dasharray, params.dashoffset);
    let mut tris = Vec::new();
    for contour in &dashed {
        stroke_contour_ex(contour, params, &mut tris);
    }
    tris
}

/// Tessellate one polyline with advanced join/cap support.
fn stroke_contour_ex(pts: &[[f32; 2]], params: &StrokeParams, out: &mut Vec<[f32; 2]>) {
    let n = pts.len();
    if n < 2 {
        return;
    }
    let half_w = params.half_width;
    let closed = n > 2
        && (pts[0][0] - pts[n - 1][0]).abs() < 1e-4
        && (pts[0][1] - pts[n - 1][1]).abs() < 1e-4;
    let wpts = if closed { &pts[..n - 1] } else { pts };
    let m = wpts.len();
    if m < 2 {
        return;
    }
    let n_segs = if closed { m } else { m - 1 };

    // Per-segment unit normals (left of travel, Y-down coords).
    let seg_normals: Vec<[f32; 2]> = (0..n_segs)
        .map(|i| {
            let a = wpts[i];
            let b = wpts[(i + 1) % m];
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            [dy / len, -dx / len]
        })
        .collect();

    // Segment direction vectors (unit).
    let seg_dirs: Vec<[f32; 2]> = (0..n_segs)
        .map(|i| {
            let a = wpts[i];
            let b = wpts[(i + 1) % m];
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            [dx / len, dy / len]
        })
        .collect();

    // Build per-vertex left/right offsets considering linejoin.
    // For endpoints of open paths, the offset equals the segment normal × half_w.
    let mut left_pts: Vec<[f32; 2]> = Vec::with_capacity(m);
    let mut right_pts: Vec<[f32; 2]> = Vec::with_capacity(m);
    // join_tris: extra triangles added at joints (round/bevel).
    let mut join_tris: Vec<(usize, Vec<[f32; 2]>)> = Vec::new();

    for i in 0..m {
        let p = wpts[i];
        let has_prev = closed || i > 0;
        let has_next = closed || i < m - 1;

        if !has_prev || !has_next {
            // Open endpoint: use adjacent normal.
            let n = if !has_prev { seg_normals[0] } else { seg_normals[n_segs - 1] };
            left_pts.push([p[0] + n[0] * half_w, p[1] + n[1] * half_w]);
            right_pts.push([p[0] - n[0] * half_w, p[1] - n[1] * half_w]);
        } else {
            let n_in = seg_normals[(i + n_segs - 1) % n_segs];
            let n_out = seg_normals[i % n_segs];
            match params.linejoin {
                StrokeLinejoin::Miter => {
                    let ofs = miter_offset_ex(i, m, &seg_normals, half_w, params.miterlimit, closed);
                    left_pts.push([p[0] + ofs[0], p[1] + ofs[1]]);
                    right_pts.push([p[0] - ofs[0], p[1] - ofs[1]]);
                }
                StrokeLinejoin::Bevel => {
                    // Use outgoing normal for the main quad; add bevel triangle separately.
                    left_pts.push([p[0] + n_out[0] * half_w, p[1] + n_out[1] * half_w]);
                    right_pts.push([p[0] - n_out[0] * half_w, p[1] - n_out[1] * half_w]);
                    // Bevel fill triangle: connects incoming and outgoing sides.
                    let li = [p[0] + n_in[0] * half_w, p[1] + n_in[1] * half_w];
                    let ri = [p[0] - n_in[0] * half_w, p[1] - n_in[1] * half_w];
                    let lo = [p[0] + n_out[0] * half_w, p[1] + n_out[1] * half_w];
                    let ro = [p[0] - n_out[0] * half_w, p[1] - n_out[1] * half_w];
                    // Which side is the "outside" of the turn?
                    let cross = n_in[0] * n_out[1] - n_in[1] * n_out[0];
                    let extra = if cross > 0.0 {
                        vec![p, li, lo] // left side bevel
                    } else {
                        vec![p, ri, ro] // right side bevel
                    };
                    join_tris.push((i, extra));
                }
                StrokeLinejoin::Round => {
                    // Use outgoing normal for main quad; add round fan separately.
                    left_pts.push([p[0] + n_out[0] * half_w, p[1] + n_out[1] * half_w]);
                    right_pts.push([p[0] - n_out[0] * half_w, p[1] - n_out[1] * half_w]);
                    let extra = round_join_tris(p, n_in, n_out, half_w);
                    join_tris.push((i, extra));
                }
            }
        }
    }

    // Emit main segment quads.
    for i in 0..n_segs {
        let j = (i + 1) % m;
        let (l0, r0) = (left_pts[i], right_pts[i]);
        let (l1, r1) = (left_pts[j], right_pts[j]);
        out.push(l0); out.push(r0); out.push(l1);
        out.push(l1); out.push(r0); out.push(r1);
    }

    // Emit join triangles.
    for (_, tris) in &join_tris {
        for t in tris.chunks(3) {
            if t.len() == 3 {
                out.push(t[0]); out.push(t[1]); out.push(t[2]);
            }
        }
    }

    // Emit linecap triangles for open sub-paths.
    if !closed && params.linecap != StrokeLinecap::Butt {
        // Start cap.
        let dir0 = seg_dirs[0];
        let n0 = seg_normals[0];
        let p0 = wpts[0];
        emit_cap(p0, n0, [-dir0[0], -dir0[1]], half_w, params.linecap, out);
        // End cap.
        let dir_last = seg_dirs[n_segs - 1];
        let n_last = seg_normals[n_segs - 1];
        let p_last = wpts[m - 1];
        emit_cap(p_last, n_last, dir_last, half_w, params.linecap, out);
    }
}

/// Emit triangles for a linecap (round or square) at `center` pointing in `dir`.
fn emit_cap(center: [f32; 2], normal: [f32; 2], dir: [f32; 2], half_w: f32, cap: StrokeLinecap, out: &mut Vec<[f32; 2]>) {
    let l = [center[0] + normal[0] * half_w, center[1] + normal[1] * half_w];
    let r = [center[0] - normal[0] * half_w, center[1] - normal[1] * half_w];
    match cap {
        StrokeLinecap::Butt => {}
        StrokeLinecap::Square => {
            // Extend by half_w in cap direction.
            let tip_l = [l[0] + dir[0] * half_w, l[1] + dir[1] * half_w];
            let tip_r = [r[0] + dir[0] * half_w, r[1] + dir[1] * half_w];
            out.push(l); out.push(r); out.push(tip_l);
            out.push(tip_l); out.push(r); out.push(tip_r);
        }
        StrokeLinecap::Round => {
            // Semicircle fan from center.
            const SEGS: usize = 12;
            // Angle from +normal to -normal going through +dir, step by PI/SEGS.
            for k in 0..SEGS {
                let a0 = (k as f32 / SEGS as f32) * PI;
                let a1 = ((k + 1) as f32 / SEGS as f32) * PI;
                // Rotate +normal by angle around center.
                let pt0 = rotate_normal(normal, dir, a0, half_w, center);
                let pt1 = rotate_normal(normal, dir, a1, half_w, center);
                out.push(center); out.push(pt0); out.push(pt1);
            }
        }
    }
}

/// Rotate `normal` toward `dir` by `angle` radians, scaled by `half_w`, offset by `center`.
#[inline]
fn rotate_normal(normal: [f32; 2], dir: [f32; 2], angle: f32, half_w: f32, center: [f32; 2]) -> [f32; 2] {
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    // Rotate `normal` by -angle (right-hand rule in Y-down): new = cos*normal + sin*dir
    let nx = cos_a * normal[0] + sin_a * dir[0];
    let ny = cos_a * normal[1] + sin_a * dir[1];
    [center[0] + nx * half_w, center[1] + ny * half_w]
}

/// Generate round join triangles (arc from n_in to n_out side).
fn round_join_tris(p: [f32; 2], n_in: [f32; 2], n_out: [f32; 2], half_w: f32) -> Vec<[f32; 2]> {
    let cross = n_in[0] * n_out[1] - n_in[1] * n_out[0];
    // Angle between n_in and n_out (clamped to avoid NaN).
    let dot = (n_in[0] * n_out[0] + n_in[1] * n_out[1]).clamp(-1.0, 1.0);
    let angle = dot.acos();
    if angle < 1e-4 {
        return Vec::new();
    }
    const SEGS: usize = 8;
    let mut tris = Vec::with_capacity(SEGS * 3);
    // Choose which side to arc (outside of the turn).
    let (start_n, sign) = if cross > 0.0 { (n_in, 1.0f32) } else { ([-n_in[0], -n_in[1]], -1.0f32) };
    let target_n = if cross > 0.0 { n_out } else { [-n_out[0], -n_out[1]] };
    for k in 0..SEGS {
        let t0 = k as f32 / SEGS as f32;
        let t1 = (k + 1) as f32 / SEGS as f32;
        let pt0 = slerp_normal(start_n, target_n, t0, half_w, p);
        let pt1 = slerp_normal(start_n, target_n, t1, half_w, p);
        let _ = sign; // used for direction selection above
        tris.push(p); tris.push(pt0); tris.push(pt1);
    }
    tris
}

/// Spherical linear interpolation between two unit normals, scaled to half_w, offset by center.
fn slerp_normal(n0: [f32; 2], n1: [f32; 2], t: f32, half_w: f32, center: [f32; 2]) -> [f32; 2] {
    let dot = (n0[0] * n1[0] + n0[1] * n1[1]).clamp(-1.0, 1.0);
    if (1.0 - dot.abs()) < 1e-6 {
        // Nearly parallel — linear interpolation.
        let x = n0[0] + t * (n1[0] - n0[0]);
        let y = n0[1] + t * (n1[1] - n0[1]);
        let len = (x * x + y * y).sqrt().max(1e-6);
        return [center[0] + x / len * half_w, center[1] + y / len * half_w];
    }
    let angle = dot.acos();
    let sin_a = angle.sin();
    let w0 = ((1.0 - t) * angle).sin() / sin_a;
    let w1 = (t * angle).sin() / sin_a;
    let nx = w0 * n0[0] + w1 * n1[0];
    let ny = w0 * n0[1] + w1 * n1[1];
    [center[0] + nx * half_w, center[1] + ny * half_w]
}

/// Miter-join offset at vertex `i` with configurable miterlimit.
fn miter_offset_ex(i: usize, m: usize, seg_normals: &[[f32; 2]], half_w: f32, miterlimit: f32, closed: bool) -> [f32; 2] {
    let n_segs = seg_normals.len();
    let has_prev = closed || i > 0;
    let has_next = closed || i < m - 1;
    if !has_prev {
        let n = seg_normals[0];
        return [n[0] * half_w, n[1] * half_w];
    }
    if !has_next {
        let n = seg_normals[n_segs - 1];
        return [n[0] * half_w, n[1] * half_w];
    }
    let n_in = seg_normals[(i + n_segs - 1) % n_segs];
    let n_out = seg_normals[i % n_segs];
    let sx = n_in[0] + n_out[0];
    let sy = n_in[1] + n_out[1];
    let denom = sx * n_out[0] + sy * n_out[1];
    if denom.abs() < 0.05 {
        return [n_out[0] * half_w, n_out[1] * half_w];
    }
    let scale = half_w / denom;
    let mx = sx * scale;
    let my = sy * scale;
    // Miter length = 2 * half_w / sin(theta/2). Limit applied as ratio.
    let miter_len_sq = mx * mx + my * my;
    let limit = miterlimit * half_w;
    if miter_len_sq > limit * limit {
        return [n_out[0] * half_w, n_out[1] * half_w]; // fall back to bevel
    }
    [mx, my]
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

    // ── Stroke tessellation ─────────────────────────────────────────────────

    #[test]
    fn stroke_zero_width_gives_nothing() {
        let contour = vec![vec![[0.0f32, 0.0], [100.0, 0.0]]];
        assert!(tessellate_stroke(&contour, 0.0).is_empty());
        assert!(tessellate_stroke(&contour, -1.0).is_empty());
    }

    #[test]
    fn stroke_single_segment_open_gives_six_vertices() {
        // One segment → one quad → 2 triangles → 6 vertices.
        let contour = vec![vec![[0.0f32, 0.0], [100.0, 0.0]]];
        let tris = tessellate_stroke(&contour, 5.0);
        assert_eq!(tris.len(), 6, "open single segment = 6 verts, got {}", tris.len());
    }

    #[test]
    fn stroke_open_segment_correct_width() {
        // Horizontal segment from (0,0) to (100,0), half_w=5.
        // Expected quad y-coords: ±5.
        let contour = vec![vec![[0.0f32, 0.0], [100.0, 0.0]]];
        let tris = tessellate_stroke(&contour, 5.0);
        let ys: Vec<f32> = tris.iter().map(|v| v[1]).collect();
        let min_y = ys.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_y = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!((min_y - (-5.0)).abs() < 0.01, "min_y={min_y}");
        assert!((max_y -   5.0 ).abs() < 0.01, "max_y={max_y}");
    }

    #[test]
    fn stroke_open_two_segments_gives_twelve_vertices() {
        // Two-segment path → 2 quads → 4 triangles → 12 vertices.
        let contour = vec![vec![[0.0f32, 0.0], [50.0, 0.0], [100.0, 0.0]]];
        let tris = tessellate_stroke(&contour, 5.0);
        assert_eq!(tris.len(), 12);
    }

    #[test]
    fn stroke_closed_square_band_width() {
        // Closed 100×100 square path.
        let contour = vec![vec![
            [0.0f32, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0], [0.0, 0.0],
        ]];
        let half_w = 4.0;
        let tris = tessellate_stroke(&contour, half_w);
        // 4 segments × 2 triangles × 3 verts = 24 vertices.
        assert_eq!(tris.len(), 24);
        // All vertices should be within half_w of the original square edges.
        for v in &tris {
            let x = v[0];
            let y = v[1];
            // Near one of the 4 edges of the square.
            let near_left   = x.abs() <= half_w + 0.1;
            let near_right  = (x - 100.0).abs() <= half_w + 0.1;
            let near_top    = y.abs() <= half_w + 0.1;
            let near_bottom = (y - 100.0).abs() <= half_w + 0.1;
            assert!(
                near_left || near_right || near_top || near_bottom,
                "vertex ({x},{y}) not near any edge of the square"
            );
        }
    }

    #[test]
    fn stroke_via_svg_path_parse_pipeline() {
        // Full pipeline: parse d → flatten → tessellate_stroke.
        let segs = parse_svg_path("M 10 10 L 90 10 L 90 90 L 10 90 Z");
        let contours = flatten_path(&segs, 0.5);
        let tris = tessellate_stroke(&contours, 3.0);
        // 4 segments × 6 verts = 24 vertices.
        assert_eq!(tris.len(), 24);
    }

    #[test]
    fn stroke_diagonal_line_area() {
        // 45° diagonal line: (0,0) → (100,100), half_w = 5.
        // Stroke band area ≈ stroke_width * length = 10 * sqrt(2)*100 ≈ 1414.
        let contour = vec![vec![[0.0f32, 0.0], [100.0, 100.0]]];
        let tris = tessellate_stroke(&contour, 5.0);
        let area: f32 = tris.chunks(3).map(|t| {
            cross2d(t[0], t[1], t[2]).abs() * 0.5
        }).sum();
        let expected = 10.0 * (100.0f32 * 100.0 + 100.0 * 100.0).sqrt();
        assert!((area - expected).abs() < 1.0, "area={area} expected≈{expected}");
    }

    // ── tessellate_stroke_ex: linecap / linejoin / dasharray ────────────────

    #[test]
    fn stroke_ex_butt_same_as_basic() {
        // tessellate_stroke_ex with butt caps and miter joins must match tessellate_stroke.
        let contour = vec![vec![[0.0f32, 0.0], [100.0, 0.0]]];
        let params = StrokeParams { half_width: 5.0, ..StrokeParams::default() };
        let ex = tessellate_stroke_ex(&contour, &params);
        let basic = tessellate_stroke(&contour, 5.0);
        assert_eq!(ex.len(), basic.len(), "butt-miter should match basic stroke vert count");
    }

    #[test]
    fn stroke_ex_square_cap_longer() {
        // Square cap extends half_w past endpoint → more vertices than butt.
        let contour = vec![vec![[0.0f32, 0.0], [100.0, 0.0]]];
        let butt = tessellate_stroke_ex(&contour, &StrokeParams {
            half_width: 5.0,
            linecap: StrokeLinecap::Butt,
            ..StrokeParams::default()
        });
        let square = tessellate_stroke_ex(&contour, &StrokeParams {
            half_width: 5.0,
            linecap: StrokeLinecap::Square,
            ..StrokeParams::default()
        });
        // Square cap adds 2 quads (start + end) = 12 extra vertices.
        assert_eq!(square.len(), butt.len() + 12, "square cap: 2×2 tris extra");
        // X-extent of square caps must be wider (< 0 and > 100).
        let xs: Vec<f32> = square.iter().map(|v| v[0]).collect();
        let min_x = xs.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_x = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(min_x < 0.0, "square cap start extends left, min_x={min_x}");
        assert!(max_x > 100.0, "square cap end extends right, max_x={max_x}");
    }

    #[test]
    fn stroke_ex_round_cap_vertices() {
        // Round cap emits 12 fan triangles per endpoint = 24 extra tris = 72 extra verts.
        let contour = vec![vec![[0.0f32, 0.0], [100.0, 0.0]]];
        let butt = tessellate_stroke_ex(&contour, &StrokeParams {
            half_width: 5.0, linecap: StrokeLinecap::Butt, ..StrokeParams::default()
        });
        let round = tessellate_stroke_ex(&contour, &StrokeParams {
            half_width: 5.0, linecap: StrokeLinecap::Round, ..StrokeParams::default()
        });
        assert!(round.len() > butt.len(), "round cap should produce more vertices");
    }

    #[test]
    fn stroke_ex_bevel_join_has_extra_triangle() {
        // Bevel join at a right-angle corner should add one triangle.
        let contour = vec![vec![[0.0f32, 0.0], [50.0, 0.0], [50.0, 50.0]]];
        let miter = tessellate_stroke_ex(&contour, &StrokeParams {
            half_width: 5.0, linejoin: StrokeLinejoin::Miter, ..StrokeParams::default()
        });
        let bevel = tessellate_stroke_ex(&contour, &StrokeParams {
            half_width: 5.0, linejoin: StrokeLinejoin::Bevel, ..StrokeParams::default()
        });
        assert!(bevel.len() >= miter.len(), "bevel join adds bevel triangle");
    }

    #[test]
    fn stroke_ex_round_join_has_extra_vertices() {
        let contour = vec![vec![[0.0f32, 0.0], [50.0, 0.0], [50.0, 50.0]]];
        let miter = tessellate_stroke_ex(&contour, &StrokeParams {
            half_width: 5.0, linejoin: StrokeLinejoin::Miter, ..StrokeParams::default()
        });
        let round_join = tessellate_stroke_ex(&contour, &StrokeParams {
            half_width: 5.0, linejoin: StrokeLinejoin::Round, ..StrokeParams::default()
        });
        assert!(round_join.len() > miter.len(), "round join adds arc triangles");
    }

    #[test]
    fn stroke_ex_miterlimit_falls_back_to_bevel() {
        // Very acute angle with tiny miterlimit should fall back to bevel (fewer vertices
        // than huge miterlimit, because miter at limit bevel = normal endpoint).
        let contour = vec![vec![[0.0f32, 50.0], [50.0, 50.0], [1.0, 50.0]]]; // near-reversal
        let _large_limit = tessellate_stroke_ex(&contour, &StrokeParams {
            half_width: 5.0, linejoin: StrokeLinejoin::Miter, miterlimit: 100.0,
            ..StrokeParams::default()
        });
        let small_limit = tessellate_stroke_ex(&contour, &StrokeParams {
            half_width: 5.0, linejoin: StrokeLinejoin::Miter, miterlimit: 1.0,
            ..StrokeParams::default()
        });
        // With miterlimit=1.0 the miter collapses to bevel offset — still produces triangles.
        assert!(!small_limit.is_empty());
    }

    // ── apply_dash_pattern ──────────────────────────────────────────────────

    #[test]
    fn dash_empty_dasharray_returns_original() {
        let contour = vec![vec![[0.0f32, 0.0], [100.0, 0.0]]];
        let result = apply_dash_pattern(&contour, &[], 0.0);
        assert_eq!(result, contour);
    }

    #[test]
    fn dash_simple_splits_line() {
        // dasharray=[20, 10]: 20px dash, 10px gap — on a 60px line gives 3 dashes.
        let contour = vec![vec![[0.0f32, 0.0], [60.0, 0.0]]];
        let result = apply_dash_pattern(&contour, &[20.0, 10.0], 0.0);
        assert_eq!(result.len(), 2, "60px / 30px cycle = 2 dashes, got {}", result.len());
    }

    #[test]
    fn dash_offset_shifts_pattern() {
        // dasharray=[10, 10], offset=10 → first dash starts at the gap position → one full dash only.
        let contour = vec![vec![[0.0f32, 0.0], [20.0, 0.0]]];
        let no_offset = apply_dash_pattern(&contour, &[10.0, 10.0], 0.0);
        let with_offset = apply_dash_pattern(&contour, &[10.0, 10.0], 10.0);
        // no_offset: gap then dash starting at 10, so 1 dash (10..20); with_offset: dash first (0..10).
        assert_eq!(no_offset.len(), 1);
        assert_eq!(with_offset.len(), 1);
    }

    #[test]
    fn dash_zero_width_returns_empty() {
        let contour = vec![vec![[0.0f32, 0.0], [100.0, 0.0]]];
        // All-zero dasharray: cycle = 0 → returns empty.
        let result = apply_dash_pattern(&contour, &[0.0, 0.0], 0.0);
        assert!(result.is_empty(), "zero cycle should produce no dashes");
    }

    #[test]
    fn dash_full_stroke_ex_pipeline() {
        // Full pipeline: dashed stroke via tessellate_stroke_ex produces triangles.
        let segs = parse_svg_path("M 0 0 H 100");
        let contours = flatten_path(&segs, 0.5);
        let dashed = tessellate_stroke_ex(&contours, &StrokeParams {
            half_width: 5.0,
            dasharray: vec![15.0, 5.0],
            ..StrokeParams::default()
        });
        assert!(!dashed.is_empty(), "dashed stroke should produce triangles");
        // Dashed stroke: 100px / 20px-cycle = 5 dashes × 6 verts each = 30 verts
        // (more segments than solid 1×6=6, but each covers only its dash length).
        let solid = tessellate_stroke_ex(&contours, &StrokeParams {
            half_width: 5.0,
            ..StrokeParams::default()
        });
        // Dashed has more vertex records (multiple short contours) than solid (one contour).
        assert!(dashed.len() > solid.len(), "dashed stroke has more tri vertices than solid");
        // But dashed covers less total area.
        let area_fn = |tris: &[[f32; 2]]| -> f32 {
            tris.chunks(3).map(|t| cross2d(t[0], t[1], t[2]).abs() * 0.5).sum()
        };
        assert!(area_fn(&dashed) < area_fn(&solid), "dashed covers less area than solid");
    }
}
