//! CSS Motion Path L1 algorithm stub.
//!
//! Computes the position and rotation of an element along an `offset-path`.
//! P4 wires the CSS properties (`offset-path`, `offset-distance`, `offset-rotate`,
//! `offset-anchor`) to `resolve_motion_transform()`.
//!
//! Supported path syntax: `path("<svg-path-d>")`, `ray(<angle> …)`, `none`.
//! Deferred: `url()`, basic-shapes (`circle()`, `ellipse()`, etc.).

// Geometry code naturally has many coordinate parameters.
#![allow(clippy::too_many_arguments)]

use crate::style::OffsetRotate;

/// Result of resolving a motion offset along an `offset-path`.
///
/// All values are in CSS px. `rotation_deg` is the total rotation in degrees
/// (CW positive, matching CSS `rotate(Xdeg)`). Apply as:
///   1. Translate element origin by `(translate_x, translate_y)`.
///   2. Rotate around element's `offset-anchor` by `rotation_deg`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionTransform {
    /// Horizontal displacement from the element's containing-block origin (CSS px).
    pub translate_x: f32,
    /// Vertical displacement from the element's containing-block origin (CSS px).
    pub translate_y: f32,
    /// Clockwise rotation in degrees applied after translation.
    pub rotation_deg: f32,
}

/// Resolve the motion transform for an element with `offset-path: path(...)`
/// or `offset-path: ray(...)`.
///
/// - `path_str`: raw `offset-path` value, e.g. `path("M 0 0 L 100 0")` or
///   `ray(45deg closest-side)`.
/// - `offset_distance_px`: resolved `offset-distance` in CSS px.  Pass the
///   containing-block diagonal multiplied by the percentage, or the raw px value.
/// - `rotate`: resolved `offset-rotate` value from `ComputedStyle`.
///
/// Returns `None` if `path_str` is `"none"`, unparseable, or describes an empty path.
pub fn resolve_motion_transform(
    path_str: &str,
    offset_distance_px: f32,
    rotate: OffsetRotate,
) -> Option<MotionTransform> {
    if let Some(ray_angle_deg) = parse_ray_angle(path_str) {
        return Some(resolve_ray(ray_angle_deg, offset_distance_px, rotate));
    }
    let d = extract_path_d(path_str)?;
    let segs = parse_svg_path(d);
    if segs.is_empty() {
        return None;
    }
    let (x, y, tangent_deg) = point_at_distance(&segs, offset_distance_px);
    let rotation_deg = match rotate {
        OffsetRotate::Auto => tangent_deg,
        OffsetRotate::AutoAngle(extra) => tangent_deg + extra,
        OffsetRotate::Reverse => tangent_deg + 180.0,
        OffsetRotate::Angle(fixed) => fixed,
    };
    Some(MotionTransform { translate_x: x, translate_y: y, rotation_deg })
}

// ─── ray() ─────────────────────────────────────────────────────────────────

/// Resolve `offset-path: ray(<angle> …)`.
///
/// Per CSS Motion Path L1 §2.2, a ray starts at the element's `offset-position`
/// (which in this engine's transform model is the box's normal position, i.e.
/// zero displacement) and extends in the direction `angle`, measured clockwise
/// from the 12-o'clock (straight up) direction — the same convention as
/// `linear-gradient()`. The element is placed `offset-distance` px along that
/// ray.
///
/// The optional `<ray-size>` (`closest-side` etc.), `contain`, and `at <position>`
/// components only affect how *percentage* `offset-distance` values and clamping
/// resolve; with a px `offset-distance` they have no effect, so this Phase 1
/// implementation parses only the angle and ignores the rest.
fn resolve_ray(angle_deg: f32, offset_distance_px: f32, rotate: OffsetRotate) -> MotionTransform {
    let rad = angle_deg.to_radians();
    // 0deg points straight up (−y in this Y-down coordinate space); angle grows
    // clockwise toward +x.
    let dir_x = rad.sin();
    let dir_y = -rad.cos();
    let translate_x = offset_distance_px * dir_x;
    let translate_y = offset_distance_px * dir_y;
    // Tangent measured from +x axis (CW positive in Y-down space), to match the
    // path() tangent convention fed into `OffsetRotate`.
    let tangent_deg = (dir_y as f64).atan2(dir_x as f64).to_degrees() as f32;
    let rotation_deg = match rotate {
        OffsetRotate::Auto => tangent_deg,
        OffsetRotate::AutoAngle(extra) => tangent_deg + extra,
        OffsetRotate::Reverse => tangent_deg + 180.0,
        OffsetRotate::Angle(fixed) => fixed,
    };
    MotionTransform { translate_x, translate_y, rotation_deg }
}

/// Parse the leading `<angle>` from a `ray(...)` value, returning the angle in
/// degrees. Returns `None` if `s` is not a `ray(...)` value or carries no angle.
///
/// Accepts `deg`, `grad`, `rad`, and `turn` units. Non-angle components
/// (size keywords, `contain`, `at <position>`) are skipped.
fn parse_ray_angle(s: &str) -> Option<f32> {
    let inner = s.trim().strip_prefix("ray(")?.strip_suffix(')')?;
    inner.split_whitespace().find_map(parse_angle_token)
}

/// Parse a single CSS `<angle>` token (`45deg`, `0.5turn`, `1.57rad`, `50grad`)
/// to degrees. Returns `None` for non-angle tokens.
fn parse_angle_token(tok: &str) -> Option<f32> {
    let tok = tok.trim();
    // Order matters: "grad" must be checked before "rad", since "50grad" ends in "rad".
    if let Some(v) = tok.strip_suffix("grad") {
        return v.trim().parse::<f32>().ok().map(|g| g * 0.9);
    }
    if let Some(v) = tok.strip_suffix("turn") {
        return v.trim().parse::<f32>().ok().map(|t| t * 360.0);
    }
    if let Some(v) = tok.strip_suffix("deg") {
        return v.trim().parse::<f32>().ok();
    }
    if let Some(v) = tok.strip_suffix("rad") {
        return v.trim().parse::<f32>().ok().map(f32::to_degrees);
    }
    None
}

// ─── SVG path parsing ────────────────────────────────────────────────────────

/// One normalised absolute-coordinate segment produced by the path parser.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PathSeg {
    /// Move to (x, y) — opens a new sub-path, zero length.
    MoveTo { x: f32, y: f32 },
    /// Line to (x, y).
    LineTo { x: f32, y: f32 },
    /// Cubic Bézier to (x, y) with control points (cx1,cy1) and (cx2,cy2).
    CubicTo { cx1: f32, cy1: f32, cx2: f32, cy2: f32, x: f32, y: f32 },
    /// Quadratic Bézier to (x, y) with control point (cx, cy).
    QuadTo { cx: f32, cy: f32, x: f32, y: f32 },
    /// Close path — line from current point back to sub-path start.
    Close,
}

/// Parse an SVG path `d` attribute string into a list of normalised absolute segments.
///
/// All relative commands (`m`, `l`, `h`, `v`, `c`, `s`, `q`, `t`, `z`) are converted
/// to their absolute equivalents. Arc commands (`A`/`a`) are approximated as cubic
/// Bézier curves via the standard W3C 4-arc decomposition.
pub(crate) fn parse_svg_path(d: &str) -> Vec<PathSeg> {
    let mut segs = Vec::new();
    let mut cx = 0.0_f32;
    let mut cy = 0.0_f32;
    let mut sub_start_x = 0.0_f32;
    let mut sub_start_y = 0.0_f32;
    let mut last_ctrl: Option<(f32, f32)> = None;

    let mut nums_buf = Vec::<f32>::new();
    let bytes = d.as_bytes();
    let mut i = 0;
    let mut cmd = b'M';

    while i <= bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        if bytes[i].is_ascii_alphabetic() {
            cmd = bytes[i];
            i += 1;
            continue;
        }
        let start = i;
        if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
            i += 1;
        }
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
            i += 1;
        }
        if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
            i += 1;
            if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
                i += 1;
            }
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i == start {
            i += 1;
            continue;
        }
        if let Ok(n) = d[start..i].parse::<f32>() {
            nums_buf.push(n);
        }
        dispatch_command(
            cmd,
            &mut nums_buf,
            &mut cx,
            &mut cy,
            &mut sub_start_x,
            &mut sub_start_y,
            &mut last_ctrl,
            &mut segs,
        );
    }
    dispatch_command(cmd, &mut nums_buf, &mut cx, &mut cy, &mut sub_start_x, &mut sub_start_y, &mut last_ctrl, &mut segs);
    segs
}

/// Consume numbers from `buf` and emit segments for command `cmd`.
fn dispatch_command(
    cmd: u8,
    buf: &mut Vec<f32>,
    cx: &mut f32,
    cy: &mut f32,
    sub_x: &mut f32,
    sub_y: &mut f32,
    last_ctrl: &mut Option<(f32, f32)>,
    out: &mut Vec<PathSeg>,
) {
    let cmd_upper = cmd.to_ascii_uppercase();
    let stride: usize = match cmd_upper {
        b'M' | b'L' | b'T' => 2,
        b'H' | b'V' => 1,
        b'S' | b'Q' => 4,
        b'C' => 6,
        b'A' => 7,
        b'Z' => 0,
        _ => return,
    };
    if cmd_upper == b'Z' {
        out.push(PathSeg::Close);
        *cx = *sub_x;
        *cy = *sub_y;
        *last_ctrl = None;
        buf.clear();
        return;
    }
    while buf.len() >= stride.max(1) {
        let nums: Vec<f32> = buf.drain(..stride).collect();
        let is_rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if is_rel { (*cx, *cy) } else { (0.0, 0.0) };
        match cmd_upper {
            b'M' => {
                let (x, y) = (ox + nums[0], oy + nums[1]);
                *sub_x = x;
                *sub_y = y;
                *cx = x;
                *cy = y;
                out.push(PathSeg::MoveTo { x, y });
                *last_ctrl = None;
            }
            b'L' => {
                let (x, y) = (ox + nums[0], oy + nums[1]);
                out.push(PathSeg::LineTo { x, y });
                *cx = x;
                *cy = y;
                *last_ctrl = None;
            }
            b'H' => {
                let x = if is_rel { *cx + nums[0] } else { nums[0] };
                out.push(PathSeg::LineTo { x, y: *cy });
                *cx = x;
                *last_ctrl = None;
            }
            b'V' => {
                let y = if is_rel { *cy + nums[0] } else { nums[0] };
                out.push(PathSeg::LineTo { x: *cx, y });
                *cy = y;
                *last_ctrl = None;
            }
            b'C' => {
                let (cx1, cy1) = (ox + nums[0], oy + nums[1]);
                let (cx2, cy2) = (ox + nums[2], oy + nums[3]);
                let (x, y) = (ox + nums[4], oy + nums[5]);
                out.push(PathSeg::CubicTo { cx1, cy1, cx2, cy2, x, y });
                *last_ctrl = Some((cx2, cy2));
                *cx = x;
                *cy = y;
            }
            b'S' => {
                let (cx1, cy1) = match *last_ctrl {
                    Some((lx, ly)) if matches!(out.last(), Some(PathSeg::CubicTo { .. })) => {
                        (2.0 * *cx - lx, 2.0 * *cy - ly)
                    }
                    _ => (*cx, *cy),
                };
                let (cx2, cy2) = (ox + nums[0], oy + nums[1]);
                let (x, y) = (ox + nums[2], oy + nums[3]);
                out.push(PathSeg::CubicTo { cx1, cy1, cx2, cy2, x, y });
                *last_ctrl = Some((cx2, cy2));
                *cx = x;
                *cy = y;
            }
            b'Q' => {
                let (qcx, qcy) = (ox + nums[0], oy + nums[1]);
                let (x, y) = (ox + nums[2], oy + nums[3]);
                out.push(PathSeg::QuadTo { cx: qcx, cy: qcy, x, y });
                *last_ctrl = Some((qcx, qcy));
                *cx = x;
                *cy = y;
            }
            b'T' => {
                let (qcx, qcy) = match *last_ctrl {
                    Some((lx, ly)) if matches!(out.last(), Some(PathSeg::QuadTo { .. })) => {
                        (2.0 * *cx - lx, 2.0 * *cy - ly)
                    }
                    _ => (*cx, *cy),
                };
                let (x, y) = (ox + nums[0], oy + nums[1]);
                out.push(PathSeg::QuadTo { cx: qcx, cy: qcy, x, y });
                *last_ctrl = Some((qcx, qcy));
                *cx = x;
                *cy = y;
            }
            b'A' => {
                let (rx, ry) = (nums[0].abs(), nums[1].abs());
                let x_rot = nums[2];
                let large = nums[3] != 0.0;
                let sweep = nums[4] != 0.0;
                let (x2, y2) = (ox + nums[5], oy + nums[6]);
                arc_to_cubics(*cx, *cy, rx, ry, x_rot, large, sweep, x2, y2, out);
                *cx = x2;
                *cy = y2;
                *last_ctrl = None;
            }
            _ => {}
        }
        // After M/m, implicit repetitions become L/l.
        if cmd == b'M' {
            if !buf.is_empty() { dispatch_command(b'L', buf, cx, cy, sub_x, sub_y, last_ctrl, out); }
            return;
        }
        if cmd == b'm' {
            if !buf.is_empty() { dispatch_command(b'l', buf, cx, cy, sub_x, sub_y, last_ctrl, out); }
            return;
        }
        if stride == 0 { break; }
    }
}

// ─── Arc → cubic approximation ───────────────────────────────────────────────

/// Convert an SVG arc segment into cubic Bézier segments.
/// Implements the W3C endpoint → center parameterisation algorithm.
fn arc_to_cubics(
    x1: f32, y1: f32,
    mut rx: f32, mut ry: f32,
    x_rot_deg: f32,
    large: bool, sweep: bool,
    x2: f32, y2: f32,
    out: &mut Vec<PathSeg>,
) {
    if (x1 - x2).abs() < 1e-4 && (y1 - y2).abs() < 1e-4 { return; }
    if rx < 1e-6 || ry < 1e-6 {
        out.push(PathSeg::LineTo { x: x2, y: y2 });
        return;
    }
    let phi = x_rot_deg.to_radians();
    let (cos_phi, sin_phi) = (phi.cos(), phi.sin());
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p =  cos_phi * dx + sin_phi * dy;
    let y1p = -sin_phi * dx + cos_phi * dy;
    let x1p2 = x1p * x1p;
    let y1p2 = y1p * y1p;
    let rx2 = rx * rx;
    let ry2 = ry * ry;
    let lambda = x1p2 / rx2 + y1p2 / ry2;
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let rx2 = rx * rx;
    let ry2 = ry * ry;
    let num = (rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2).max(0.0);
    let den = rx2 * y1p2 + ry2 * x1p2;
    let sq = if den < 1e-10 { 0.0 } else { (num / den).sqrt() };
    let sign = if large == sweep { -1.0_f32 } else { 1.0_f32 };
    let cxp =  sign * sq * rx * y1p / ry;
    let cyp = -sign * sq * ry * x1p / rx;
    let cx = cos_phi * cxp - sin_phi * cyp + (x1 + x2) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (y1 + y2) / 2.0;
    let theta1 = angle_between(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let dtheta = {
        let mut d = angle_between(
            (x1p - cxp) / rx, (y1p - cyp) / ry,
            (-x1p - cxp) / rx, (-y1p - cyp) / ry,
        );
        if !sweep && d > 0.0 { d -= std::f32::consts::TAU; }
        if sweep  && d < 0.0 { d += std::f32::consts::TAU; }
        d
    };
    let n = ((dtheta.abs() / std::f32::consts::FRAC_PI_2).ceil() as usize).max(1);
    let step = dtheta / n as f32;
    let mut t = theta1;
    for _ in 0..n {
        arc_segment_to_cubic(cx, cy, rx, ry, phi, t, step, out);
        t += step;
    }
}

fn arc_segment_to_cubic(
    cx: f32, cy: f32, rx: f32, ry: f32,
    phi: f32, theta: f32, d_theta: f32,
    out: &mut Vec<PathSeg>,
) {
    let alpha = (4.0 / 3.0) * (d_theta / 2.0).tan();
    let (cos_phi, sin_phi) = (phi.cos(), phi.sin());
    let (cos_t1, sin_t1) = (theta.cos(), theta.sin());
    let (cos_t2, sin_t2) = ((theta + d_theta).cos(), (theta + d_theta).sin());
    let dx1 = -rx * sin_t1;
    let dy1 =  ry * cos_t1;
    let p1x = cx + cos_phi * rx * cos_t1 - sin_phi * ry * sin_t1;
    let p1y = cy + sin_phi * rx * cos_t1 + cos_phi * ry * sin_t1;
    let cp1x = p1x + alpha * (cos_phi * dx1 - sin_phi * dy1);
    let cp1y = p1y + alpha * (sin_phi * dx1 + cos_phi * dy1);
    let dx2 = -rx * sin_t2;
    let dy2 =  ry * cos_t2;
    let p2x = cx + cos_phi * rx * cos_t2 - sin_phi * ry * sin_t2;
    let p2y = cy + sin_phi * rx * cos_t2 + cos_phi * ry * sin_t2;
    let cp2x = p2x - alpha * (cos_phi * dx2 - sin_phi * dy2);
    let cp2y = p2y - alpha * (sin_phi * dx2 + cos_phi * dy2);
    out.push(PathSeg::CubicTo { cx1: cp1x, cy1: cp1y, cx2: cp2x, cy2: cp2y, x: p2x, y: p2y });
}

fn angle_between(ux: f32, uy: f32, vx: f32, vy: f32) -> f32 {
    let dot = ux * vx + uy * vy;
    let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
    let cos_a = (dot / len).clamp(-1.0, 1.0);
    let sign = if ux * vy - uy * vx < 0.0 { -1.0_f32 } else { 1.0_f32 };
    sign * cos_a.acos()
}

// ─── Path geometry ───────────────────────────────────────────────────────────

/// Compute the (x, y, tangent_deg) at `dist` CSS px along the path.
///
/// `dist` is clamped to [0, path_length]. Returns the position of the first
/// `MoveTo` with rotation 0 if the path has zero length.
pub(crate) fn point_at_distance(segs: &[PathSeg], dist: f32) -> (f32, f32, f32) {
    let mut walked = 0.0_f32;
    let mut px = 0.0_f32;
    let mut py = 0.0_f32;
    let mut sub_start = (0.0_f32, 0.0_f32);

    for seg in segs {
        match seg {
            PathSeg::MoveTo { x, y } => {
                px = *x;
                py = *y;
                sub_start = (*x, *y);
            }
            PathSeg::LineTo { x, y } => {
                let len = line_length(px, py, *x, *y);
                if dist <= walked + len {
                    let t = if len < 1e-8 { 0.0 } else { (dist - walked) / len };
                    let qx = px + t * (*x - px);
                    let qy = py + t * (*y - py);
                    let angle = ((*y - py) as f64).atan2((*x - px) as f64) as f32;
                    return (qx, qy, angle.to_degrees());
                }
                walked += len;
                px = *x;
                py = *y;
            }
            PathSeg::CubicTo { cx1, cy1, cx2, cy2, x, y } => {
                let (ex, ey) = (*x, *y);
                if let Some((qx, qy, angle)) =
                    walk_cubic(px, py, *cx1, *cy1, *cx2, *cy2, ex, ey, dist - walked)
                {
                    return (qx, qy, angle);
                }
                walked += cubic_length(px, py, *cx1, *cy1, *cx2, *cy2, ex, ey);
                px = ex;
                py = ey;
            }
            PathSeg::QuadTo { cx, cy, x, y } => {
                // Elevate quadratic to cubic.
                let cx1 = px + 2.0 / 3.0 * (*cx - px);
                let cy1 = py + 2.0 / 3.0 * (*cy - py);
                let cx2 = *x + 2.0 / 3.0 * (*cx - *x);
                let cy2 = *y + 2.0 / 3.0 * (*cy - *y);
                if let Some((qx, qy, angle)) =
                    walk_cubic(px, py, cx1, cy1, cx2, cy2, *x, *y, dist - walked)
                {
                    return (qx, qy, angle);
                }
                walked += cubic_length(px, py, cx1, cy1, cx2, cy2, *x, *y);
                px = *x;
                py = *y;
            }
            PathSeg::Close => {
                let (ex, ey) = sub_start;
                let len = line_length(px, py, ex, ey);
                if dist <= walked + len {
                    let t = if len < 1e-8 { 0.0 } else { (dist - walked) / len };
                    let qx = px + t * (ex - px);
                    let qy = py + t * (ey - py);
                    let angle = ((ey - py) as f64).atan2((ex - px) as f64) as f32;
                    return (qx, qy, angle.to_degrees());
                }
                walked += len;
                px = ex;
                py = ey;
            }
        }
    }
    (px, py, 0.0)
}

fn line_length(x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    (dx * dx + dy * dy).sqrt()
}

/// Walk a cubic Bézier; return `(x, y, angle_deg)` at `target` distance along it,
/// or `None` if `target` exceeds the segment length.
fn walk_cubic(
    x0: f32, y0: f32,
    cx1: f32, cy1: f32, cx2: f32, cy2: f32,
    x3: f32, y3: f32,
    target: f32,
) -> Option<(f32, f32, f32)> {
    if target < 0.0 { return None; }
    let len = cubic_length(x0, y0, cx1, cy1, cx2, cy2, x3, y3);
    if target > len { return None; }
    walk_cubic_rec(x0, y0, cx1, cy1, cx2, cy2, x3, y3, target, 0.0, 1.0, len)
}

fn walk_cubic_rec(
    x0: f32, y0: f32,
    cx1: f32, cy1: f32, cx2: f32, cy2: f32,
    x3: f32, y3: f32,
    target: f32, t_lo: f32, t_hi: f32,
    seg_len: f32,
) -> Option<(f32, f32, f32)> {
    if seg_len < 0.5 || (t_hi - t_lo) < 1e-5 {
        let t = (t_lo + t_hi) / 2.0;
        let (x, y) = cubic_at(x0, y0, cx1, cy1, cx2, cy2, x3, y3, t);
        let angle = cubic_tangent_deg(x0, y0, cx1, cy1, cx2, cy2, x3, y3, t);
        return Some((x, y, angle));
    }
    let t_mid = (t_lo + t_hi) / 2.0;
    // Split once, reuse both halves.
    let (lx0, ly0, lcx1, lcy1, lcx2, lcy2, lx3, ly3,
         rx0, ry0, rcx1, rcy1, rcx2, rcy2, rx3, ry3) =
        split_cubic(x0, y0, cx1, cy1, cx2, cy2, x3, y3, t_mid);
    let left_len = cubic_length(lx0, ly0, lcx1, lcy1, lcx2, lcy2, lx3, ly3);
    if target <= left_len {
        walk_cubic_rec(lx0, ly0, lcx1, lcy1, lcx2, lcy2, lx3, ly3,
            target, t_lo, t_mid, left_len)
    } else {
        walk_cubic_rec(rx0, ry0, rcx1, rcy1, rcx2, rcy2, rx3, ry3,
            target - left_len, t_mid, t_hi, seg_len - left_len)
    }
}

/// De Casteljau split of cubic Bézier at parameter `t`.
/// Returns `(left × 8, right × 8)` control points.
#[allow(clippy::type_complexity)]
fn split_cubic(
    x0: f32, y0: f32,
    cx1: f32, cy1: f32, cx2: f32, cy2: f32,
    x3: f32, y3: f32,
    t: f32,
) -> (f32, f32, f32, f32, f32, f32, f32, f32, f32, f32, f32, f32, f32, f32, f32, f32) {
    let lerp = |a: f32, b: f32| a + t * (b - a);
    let (m1x, m1y) = (lerp(x0, cx1), lerp(y0, cy1));
    let (m2x, m2y) = (lerp(cx1, cx2), lerp(cy1, cy2));
    let (m3x, m3y) = (lerp(cx2, x3), lerp(cy2, y3));
    let (n1x, n1y) = (lerp(m1x, m2x), lerp(m1y, m2y));
    let (n2x, n2y) = (lerp(m2x, m3x), lerp(m2y, m3y));
    let (ox, oy) = (lerp(n1x, n2x), lerp(n1y, n2y));
    (x0, y0, m1x, m1y, n1x, n1y, ox, oy,
     ox, oy, n2x, n2y, m3x, m3y, x3, y3)
}

fn cubic_at(
    x0: f32, y0: f32,
    cx1: f32, cy1: f32, cx2: f32, cy2: f32,
    x3: f32, y3: f32,
    t: f32,
) -> (f32, f32) {
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let mt3 = mt2 * mt;
    let t2 = t * t;
    let t3 = t2 * t;
    (mt3 * x0 + 3.0 * mt2 * t * cx1 + 3.0 * mt * t2 * cx2 + t3 * x3,
     mt3 * y0 + 3.0 * mt2 * t * cy1 + 3.0 * mt * t2 * cy2 + t3 * y3)
}

fn cubic_tangent_deg(
    x0: f32, y0: f32,
    cx1: f32, cy1: f32, cx2: f32, cy2: f32,
    x3: f32, y3: f32,
    t: f32,
) -> f32 {
    let mt = 1.0 - t;
    let dx = 3.0 * (mt * mt * (cx1 - x0) + 2.0 * mt * t * (cx2 - cx1) + t * t * (x3 - cx2));
    let dy = 3.0 * (mt * mt * (cy1 - y0) + 2.0 * mt * t * (cy2 - cy1) + t * t * (y3 - cy2));
    if dx.abs() < 1e-8 && dy.abs() < 1e-8 {
        return 0.0;
    }
    (dy as f64).atan2(dx as f64).to_degrees() as f32
}

/// Approximate cubic Bézier arc length using 5-point Gauss-Legendre quadrature.
fn cubic_length(
    x0: f32, y0: f32,
    cx1: f32, cy1: f32, cx2: f32, cy2: f32,
    x3: f32, y3: f32,
) -> f32 {
    // Abscissae and weights for 5-point Gauss-Legendre on [-1, 1].
    const ABSC: [f32; 5] = [0.0, 0.538_469_3, -0.538_469_3, 0.906_179_9, -0.906_179_9];
    const WGHT: [f32; 5] = [0.568_888_9, 0.478_628_7, 0.478_628_7, 0.236_926_9, 0.236_926_9];
    let mut len = 0.0_f32;
    for (&a, &w) in ABSC.iter().zip(WGHT.iter()) {
        let t = 0.5 * (1.0 + a);
        let mt = 1.0 - t;
        let dx = 3.0 * (mt * mt * (cx1 - x0) + 2.0 * mt * t * (cx2 - cx1) + t * t * (x3 - cx2));
        let dy = 3.0 * (mt * mt * (cy1 - y0) + 2.0 * mt * t * (cy2 - cy1) + t * t * (y3 - cy2));
        len += w * (dx * dx + dy * dy).sqrt();
    }
    len * 0.5
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Extract the `d` argument from `path("...")` syntax.
/// Returns `None` for `none`, `url(...)`, `ray(...)`, or unrecognised values.
pub(crate) fn extract_path_d(s: &str) -> Option<&str> {
    let s = s.trim();
    if s == "none" { return None; }
    let inner = s.strip_prefix("path(")?.trim_start();
    let inner = inner.strip_suffix(')')?.trim_end();
    let inner = if (inner.starts_with('"') && inner.ends_with('"'))
        || (inner.starts_with('\'') && inner.ends_with('\''))
    {
        &inner[1..inner.len() - 1]
    } else {
        return None;
    };
    Some(inner)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::OffsetRotate;

    #[test]
    fn extract_path_d_basic() {
        assert_eq!(extract_path_d(r#"path("M 0 0 L 100 0")"#), Some("M 0 0 L 100 0"));
        assert_eq!(extract_path_d("none"), None);
        assert_eq!(extract_path_d("url(#p)"), None);
    }

    #[test]
    fn parse_moveto_lineto() {
        let segs = parse_svg_path("M 0 0 L 100 0");
        assert_eq!(segs.len(), 2);
        assert!(matches!(segs[0], PathSeg::MoveTo { x, y } if x == 0.0 && y == 0.0));
        assert!(matches!(segs[1], PathSeg::LineTo { x, y } if x == 100.0 && y == 0.0));
    }

    #[test]
    fn parse_relative_commands() {
        let segs = parse_svg_path("M 10 20 l 30 0");
        assert!(matches!(segs[1], PathSeg::LineTo { x, y } if (x - 40.0).abs() < 0.01 && (y - 20.0).abs() < 0.01));
    }

    #[test]
    fn parse_h_v_commands() {
        let segs = parse_svg_path("M 0 0 H 50 V 80");
        assert!(matches!(segs[1], PathSeg::LineTo { x, y } if (x - 50.0).abs() < 0.01 && y == 0.0));
        assert!(matches!(segs[2], PathSeg::LineTo { x, y } if x == 50.0 && (y - 80.0).abs() < 0.01));
    }

    #[test]
    fn parse_cubic_bezier() {
        let segs = parse_svg_path("M 0 0 C 25 50 75 50 100 0");
        assert!(matches!(segs[1], PathSeg::CubicTo { .. }));
    }

    #[test]
    fn parse_close_path() {
        let segs = parse_svg_path("M 0 0 L 100 0 L 100 100 Z");
        assert!(matches!(segs.last(), Some(PathSeg::Close)));
    }

    #[test]
    fn straight_line_midpoint() {
        let segs = parse_svg_path("M 0 0 L 100 0");
        let (x, y, ang) = point_at_distance(&segs, 50.0);
        assert!((x - 50.0).abs() < 0.1, "x={x}");
        assert!(y.abs() < 0.1, "y={y}");
        assert!(ang.abs() < 0.1, "angle={ang}");
    }

    #[test]
    fn straight_line_end_point() {
        let segs = parse_svg_path("M 0 0 L 100 0");
        let (x, y, _) = point_at_distance(&segs, 100.0);
        assert!((x - 100.0).abs() < 0.5, "x={x}");
        assert!(y.abs() < 0.5, "y={y}");
    }

    #[test]
    fn vertical_line_tangent() {
        // Vertical line going down → tangent should be 90°.
        let segs = parse_svg_path("M 0 0 L 0 100");
        let (_, _, ang) = point_at_distance(&segs, 50.0);
        assert!((ang - 90.0).abs() < 0.5, "angle={ang}");
    }

    #[test]
    fn closed_path_midpoint_on_return_segment() {
        // M 0 0 L 100 0 Z → forward segment (len 100) + close (len 100)
        // Distance 150 is 50 along the return segment → (50, 0).
        let segs = parse_svg_path("M 0 0 L 100 0 Z");
        let (x, y, _) = point_at_distance(&segs, 150.0);
        assert!((x - 50.0).abs() < 1.0, "x={x}");
        assert!(y.abs() < 1.0, "y={y}");
    }

    #[test]
    fn resolve_motion_auto_rotation() {
        let mt = resolve_motion_transform(
            r#"path("M 0 0 L 100 0")"#,
            50.0,
            OffsetRotate::Auto,
        ).unwrap();
        assert!((mt.translate_x - 50.0).abs() < 0.5, "tx={}", mt.translate_x);
        assert!(mt.translate_y.abs() < 0.5, "ty={}", mt.translate_y);
        assert!(mt.rotation_deg.abs() < 0.5, "rot={}", mt.rotation_deg);
    }

    #[test]
    fn resolve_motion_fixed_rotation() {
        let mt = resolve_motion_transform(
            r#"path("M 0 0 L 100 0")"#,
            50.0,
            OffsetRotate::Angle(45.0),
        ).unwrap();
        assert!((mt.rotation_deg - 45.0).abs() < 0.5);
    }

    #[test]
    fn resolve_motion_none_returns_none() {
        assert!(resolve_motion_transform("none", 50.0, OffsetRotate::Auto).is_none());
    }

    #[test]
    fn resolve_motion_reverse_rotation() {
        // Horizontal path → auto tangent 0° → reverse should be 180°.
        let mt = resolve_motion_transform(
            r#"path("M 0 0 L 100 0")"#,
            50.0,
            OffsetRotate::Reverse,
        ).unwrap();
        assert!((mt.rotation_deg.abs() - 180.0).abs() < 1.0, "rot={}", mt.rotation_deg);
    }

    #[test]
    fn ray_angle_parsing_units() {
        assert_eq!(parse_ray_angle("ray(45deg)"), Some(45.0));
        assert_eq!(parse_ray_angle("ray( 90deg closest-side )"), Some(90.0));
        assert_eq!(parse_ray_angle("ray(0.5turn)"), Some(180.0));
        assert_eq!(parse_ray_angle("ray(100grad)"), Some(90.0));
        assert!((parse_ray_angle("ray(3.14159rad)").unwrap() - 180.0).abs() < 0.1);
        assert_eq!(parse_ray_angle("path(\"M 0 0\")"), None);
        assert_eq!(parse_ray_angle("none"), None);
        assert_eq!(parse_ray_angle("ray(contain)"), None);
    }

    #[test]
    fn ray_zero_deg_goes_up() {
        // 0deg → straight up: translate_y negative, translate_x ≈ 0.
        let mt = resolve_motion_transform("ray(0deg)", 100.0, OffsetRotate::Angle(0.0)).unwrap();
        assert!(mt.translate_x.abs() < 0.01, "tx={}", mt.translate_x);
        assert!((mt.translate_y + 100.0).abs() < 0.01, "ty={}", mt.translate_y);
    }

    #[test]
    fn ray_ninety_deg_goes_right() {
        // 90deg → straight right: translate_x positive, translate_y ≈ 0.
        let mt = resolve_motion_transform("ray(90deg)", 100.0, OffsetRotate::Angle(0.0)).unwrap();
        assert!((mt.translate_x - 100.0).abs() < 0.01, "tx={}", mt.translate_x);
        assert!(mt.translate_y.abs() < 0.01, "ty={}", mt.translate_y);
    }

    #[test]
    fn ray_auto_rotation_tracks_direction() {
        // 90deg ray travels along +x → tangent 0° → auto rotation ≈ 0°.
        let right = resolve_motion_transform("ray(90deg)", 50.0, OffsetRotate::Auto).unwrap();
        assert!(right.rotation_deg.abs() < 0.5, "rot={}", right.rotation_deg);
        // 0deg ray travels up (−y) → tangent −90° → auto rotation ≈ −90°.
        let up = resolve_motion_transform("ray(0deg)", 50.0, OffsetRotate::Auto).unwrap();
        assert!((up.rotation_deg + 90.0).abs() < 0.5, "rot={}", up.rotation_deg);
    }

    #[test]
    fn ray_fixed_rotation_ignores_direction() {
        let mt = resolve_motion_transform("ray(180deg)", 30.0, OffsetRotate::Angle(45.0)).unwrap();
        assert!((mt.rotation_deg - 45.0).abs() < 0.5, "rot={}", mt.rotation_deg);
        // 180deg → straight down.
        assert!((mt.translate_y - 30.0).abs() < 0.01, "ty={}", mt.translate_y);
        assert!(mt.translate_x.abs() < 0.01, "tx={}", mt.translate_x);
    }

    #[test]
    fn ray_ignores_size_and_position_keywords() {
        let mt = resolve_motion_transform(
            "ray(90deg farthest-corner contain at center)",
            100.0,
            OffsetRotate::Angle(0.0),
        )
        .unwrap();
        assert!((mt.translate_x - 100.0).abs() < 0.01, "tx={}", mt.translate_x);
    }

    #[test]
    fn cubic_path_smooth_position() {
        // S-curve: the midpoint should be finite.
        let mt = resolve_motion_transform(
            r#"path("M 0 0 C 50 0 50 100 100 100")"#,
            50.0,
            OffsetRotate::Auto,
        ).unwrap();
        assert!(mt.translate_x.is_finite() && mt.translate_y.is_finite());
        assert!(mt.rotation_deg.is_finite());
    }
}
