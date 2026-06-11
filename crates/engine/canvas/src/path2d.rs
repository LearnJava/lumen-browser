//! `Path2D` — reusable 2D path object (HTML Living Standard §4.12.5.1.5).
//!
//! Path2D stores path commands in **user space**. The current transformation
//! matrix (CTM) is applied when the path is used with a `Context2D` via
//! `fill`, `stroke`, or `clip`.

use crate::PathSegment;

/// A reusable 2D path object independent of any rendering context.
///
/// Supports all path commands from `CanvasPath` mixin (HTML LS §4.12.5.1.5)
/// plus construction from SVG path string data.
#[derive(Debug, Clone, Default)]
pub struct Path2dData {
    /// Path segments in user (pre-CTM) coordinates.
    pub segments: Vec<PathSegment>,
    /// Current pen position in user space.
    pen: (f32, f32),
    /// Start of the current sub-path (for `closePath`).
    path_start: Option<(f32, f32)>,
}

impl Path2dData {
    /// Create an empty `Path2D`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse from an SVG path data string (`M 0 0 L 100 0 Z` etc.).
    ///
    /// Supports: `M/m`, `L/l`, `H/h`, `V/v`, `C/c`, `Q/q`, `A/a`, `Z/z`.
    /// Relative commands are converted to absolute. Arc (`A`) commands are
    /// approximated as polylines (same as `Context2D::arc`).
    pub fn from_svg_str(s: &str) -> Self {
        let mut p = Self::new();
        parse_svg_path(s, &mut p);
        p
    }

    /// `moveTo(x, y)` — start a new sub-path at `(x, y)`.
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.path_start = Some((x, y));
        self.pen = (x, y);
        self.segments.push(PathSegment::Move(x, y));
    }

    /// `lineTo(x, y)` — add a straight line from the current pen to `(x, y)`.
    pub fn line_to(&mut self, x: f32, y: f32) {
        let (x0, y0) = self.pen;
        if self.segments.is_empty() {
            self.path_start = Some((x0, y0));
            self.segments.push(PathSegment::Move(x0, y0));
        }
        self.segments.push(PathSegment::Line(x0, y0, x, y));
        self.pen = (x, y);
    }

    /// `closePath()` — add a line back to the current sub-path start.
    pub fn close_path(&mut self) {
        if let Some((sx, sy)) = self.path_start {
            let (px, py) = self.pen;
            self.segments.push(PathSegment::Line(px, py, sx, sy));
            self.pen = (sx, sy);
        }
    }

    /// `bezierCurveTo(cp1x, cp1y, cp2x, cp2y, x, y)` — cubic Bézier from pen.
    pub fn bezier_curve_to(
        &mut self,
        cp1x: f32, cp1y: f32,
        cp2x: f32, cp2y: f32,
        x: f32, y: f32,
    ) {
        let (x0, y0) = self.pen;
        if self.segments.is_empty() {
            self.path_start = Some((x0, y0));
            self.segments.push(PathSegment::Move(x0, y0));
        }
        self.segments.push(PathSegment::Cubic(x0, y0, cp1x, cp1y, cp2x, cp2y, x, y));
        self.pen = (x, y);
    }

    /// `quadraticCurveTo(cpx, cpy, x, y)` — quadratic Bézier from pen.
    pub fn quadratic_curve_to(&mut self, cpx: f32, cpy: f32, x: f32, y: f32) {
        let (x0, y0) = self.pen;
        if self.segments.is_empty() {
            self.path_start = Some((x0, y0));
            self.segments.push(PathSegment::Move(x0, y0));
        }
        self.segments.push(PathSegment::Quadratic(x0, y0, cpx, cpy, x, y));
        self.pen = (x, y);
    }

    /// `arc(cx, cy, r, startAngle, endAngle[, ccw])` — circular arc tessellated to lines.
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, start: f32, end: f32, ccw: bool) {
        let angle_delta = if ccw { -(end - start) } else { end - start };
        let step_count = ((r * angle_delta.abs()) as u32 + 4).clamp(4, 180);
        let first_x = cx + r * start.cos();
        let first_y = cy + r * start.sin();
        if self.segments.is_empty() {
            self.move_to(first_x, first_y);
        } else {
            self.line_to(first_x, first_y);
        }
        for i in 1..=step_count {
            let t = start + angle_delta * (i as f32 / step_count as f32);
            self.line_to(cx + r * t.cos(), cy + r * t.sin());
        }
    }

    /// `arcTo(x1, y1, x2, y2, radius)` — tangent arc.
    pub fn arc_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) {
        if radius <= 0.0 {
            self.line_to(x1, y1);
            return;
        }
        let (x0, y0) = self.pen;
        let d1x = x0 - x1; let d1y = y0 - y1;
        let d2x = x2 - x1; let d2y = y2 - y1;
        let len1 = (d1x * d1x + d1y * d1y).sqrt();
        let len2 = (d2x * d2x + d2y * d2y).sqrt();
        if len1 < f32::EPSILON || len2 < f32::EPSILON {
            self.line_to(x1, y1);
            return;
        }
        let cos_a = ((d1x * d2x + d1y * d2y) / (len1 * len2)).clamp(-1.0, 1.0);
        let half = cos_a.acos() * 0.5;
        if half.sin().abs() < f32::EPSILON {
            self.line_to(x1, y1);
            return;
        }
        let td = radius / half.tan();
        let tx1 = x1 + d1x * (td / len1);
        let ty1 = y1 + d1y * (td / len1);
        let tx2 = x1 + d2x * (td / len2);
        let ty2 = y1 + d2y * (td / len2);
        self.line_to(tx1, ty1);
        let n1x = -d1y / len1; let n1y = d1x / len1;
        let cross = d1x * d2y - d1y * d2x;
        let sign = if cross > 0.0 { 1.0_f32 } else { -1.0_f32 };
        let acx = tx1 + n1x * radius * sign;
        let acy = ty1 + n1y * radius * sign;
        let a_start = (ty1 - acy).atan2(tx1 - acx);
        let a_end   = (ty2 - acy).atan2(tx2 - acx);
        self.arc(acx, acy, radius, a_start, a_end, cross < 0.0);
    }

    /// `ellipse(cx, cy, rx, ry, rotation, startAngle, endAngle[, ccw])` — elliptical arc.
    #[allow(clippy::too_many_arguments)]
    pub fn ellipse(
        &mut self,
        cx: f32, cy: f32,
        rx: f32, ry: f32,
        rotation: f32,
        start_angle: f32, end_angle: f32,
        ccw: bool,
    ) {
        let span = if ccw {
            let s = start_angle - end_angle;
            if s <= 0.0 { s + std::f32::consts::TAU } else { s }
        } else {
            let s = end_angle - start_angle;
            if s <= 0.0 { s + std::f32::consts::TAU } else { s }
        };
        let steps = ((rx.max(ry) * span).abs() as u32 + 4).clamp(4, 180);
        let cos_r = rotation.cos();
        let sin_r = rotation.sin();
        for i in 0..=steps {
            let t = if ccw {
                start_angle - span * (i as f32 / steps as f32)
            } else {
                start_angle + span * (i as f32 / steps as f32)
            };
            let lx = rx * t.cos();
            let ly = ry * t.sin();
            let ex = cx + lx * cos_r - ly * sin_r;
            let ey = cy + lx * sin_r + ly * cos_r;
            if i == 0 {
                if self.segments.is_empty() { self.move_to(ex, ey); } else { self.line_to(ex, ey); }
            } else {
                self.line_to(ex, ey);
            }
        }
    }

    /// `rect(x, y, w, h)` — add a closed rectangle sub-path.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.move_to(x, y);
        self.line_to(x + w, y);
        self.line_to(x + w, y + h);
        self.line_to(x, y + h);
        self.close_path();
    }

    /// `addPath(path[, transform])` — append another path's segments, optionally transformed.
    ///
    /// `transform` is a CSS matrix `[a, b, c, d, e, f]`; `None` = identity.
    pub fn add_path(&mut self, other: &Path2dData, transform: Option<[f32; 6]>) {
        let t = transform.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        for seg in &other.segments {
            self.segments.push(apply_matrix_to_segment(seg, t));
        }
        // Update pen from the last added segment
        if let Some(last) = other.segments.last() {
            let (ex, ey) = segment_endpoint(last);
            let (tx, ty) = apply_matrix(ex, ey, t);
            self.pen = (tx, ty);
        }
    }

    /// Return segments transformed by a CTM `[a, b, c, d, e, f]`.
    ///
    /// Used by `Context2D::fill_with_path2d` to apply the rendering context's CTM
    /// to the path before rasterization.
    pub fn to_device_space(&self, ctm: [f32; 6]) -> Vec<PathSegment> {
        self.segments.iter().map(|s| apply_matrix_to_segment(s, ctm)).collect()
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn apply_matrix(x: f32, y: f32, [a, b, c, d, e, f]: [f32; 6]) -> (f32, f32) {
    (a * x + c * y + e, b * x + d * y + f)
}

fn apply_matrix_to_segment(seg: &PathSegment, m: [f32; 6]) -> PathSegment {
    match *seg {
        PathSegment::Move(x, y) => {
            let (tx, ty) = apply_matrix(x, y, m);
            PathSegment::Move(tx, ty)
        }
        PathSegment::Line(x0, y0, x1, y1) => {
            let (ax, ay) = apply_matrix(x0, y0, m);
            let (bx, by) = apply_matrix(x1, y1, m);
            PathSegment::Line(ax, ay, bx, by)
        }
        PathSegment::Cubic(x0, y0, c1x, c1y, c2x, c2y, x1, y1) => {
            let (ax, ay) = apply_matrix(x0, y0, m);
            let (bx, by) = apply_matrix(c1x, c1y, m);
            let (cx, cy) = apply_matrix(c2x, c2y, m);
            let (dx, dy) = apply_matrix(x1, y1, m);
            PathSegment::Cubic(ax, ay, bx, by, cx, cy, dx, dy)
        }
        PathSegment::Quadratic(x0, y0, cpx, cpy, x1, y1) => {
            let (ax, ay) = apply_matrix(x0, y0, m);
            let (bx, by) = apply_matrix(cpx, cpy, m);
            let (cx, cy) = apply_matrix(x1, y1, m);
            PathSegment::Quadratic(ax, ay, bx, by, cx, cy)
        }
    }
}

fn segment_endpoint(seg: &PathSegment) -> (f32, f32) {
    match *seg {
        PathSegment::Move(x, y) => (x, y),
        PathSegment::Line(_, _, x, y) => (x, y),
        PathSegment::Cubic(_, _, _, _, _, _, x, y) => (x, y),
        PathSegment::Quadratic(_, _, _, _, x, y) => (x, y),
    }
}

// ── SVG path string parser ────────────────────────────────────────────────────

/// Parse SVG path data string into `path`. Supported commands:
/// `M/m`, `L/l`, `H/h`, `V/v`, `C/c`, `Q/q`, `A/a`, `Z/z`.
fn parse_svg_path(s: &str, path: &mut Path2dData) {
    let mut chars = s.chars().peekable();
    let mut cmd = 'M';

    // Current absolute pen for relative commands
    let mut cur_x: f32 = 0.0;
    let mut cur_y: f32 = 0.0;

    loop {
        skip_wsc(&mut chars);
        // Peek to decide: new command letter or repeated args?
        match chars.peek() {
            None => break,
            Some(&c) if c.is_ascii_alphabetic() => {
                cmd = c;
                chars.next();
            }
            Some(_) => {} // implicit repeat of last command
        }

        match cmd {
            'M' | 'm' => {
                let x = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let y = parse_f32(&mut chars);
                let (ax, ay) = if cmd == 'm' { (cur_x + x, cur_y + y) } else { (x, y) };
                path.move_to(ax, ay);
                cur_x = ax; cur_y = ay;
                // Subsequent args → lineto
                if cmd == 'M' { cmd = 'L'; } else { cmd = 'l'; }
            }
            'Z' | 'z' => {
                path.close_path();
                // After Z, implicit repeat would restart from path-start; break repetition.
                break;
            }
            'L' | 'l' => {
                let x = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let y = parse_f32(&mut chars);
                let (ax, ay) = if cmd == 'l' { (cur_x + x, cur_y + y) } else { (x, y) };
                path.line_to(ax, ay);
                cur_x = ax; cur_y = ay;
            }
            'H' | 'h' => {
                let x = parse_f32(&mut chars);
                let ax = if cmd == 'h' { cur_x + x } else { x };
                path.line_to(ax, cur_y);
                cur_x = ax;
            }
            'V' | 'v' => {
                let y = parse_f32(&mut chars);
                let ay = if cmd == 'v' { cur_y + y } else { y };
                path.line_to(cur_x, ay);
                cur_y = ay;
            }
            'C' | 'c' => {
                let cp1x = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let cp1y = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let cp2x = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let cp2y = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let x    = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let y    = parse_f32(&mut chars);
                let (acp1x, acp1y, acp2x, acp2y, ax, ay) = if cmd == 'c' {
                    (cur_x+cp1x, cur_y+cp1y, cur_x+cp2x, cur_y+cp2y, cur_x+x, cur_y+y)
                } else {
                    (cp1x, cp1y, cp2x, cp2y, x, y)
                };
                path.bezier_curve_to(acp1x, acp1y, acp2x, acp2y, ax, ay);
                cur_x = ax; cur_y = ay;
            }
            'Q' | 'q' => {
                let cpx = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let cpy = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let x   = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let y   = parse_f32(&mut chars);
                let (acpx, acpy, ax, ay) = if cmd == 'q' {
                    (cur_x+cpx, cur_y+cpy, cur_x+x, cur_y+y)
                } else {
                    (cpx, cpy, x, y)
                };
                path.quadratic_curve_to(acpx, acpy, ax, ay);
                cur_x = ax; cur_y = ay;
            }
            'A' | 'a' => {
                let rx   = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let ry   = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let xrot = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let laf  = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let sf   = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let x    = parse_f32(&mut chars); skip_wsc_comma(&mut chars);
                let y    = parse_f32(&mut chars);
                let (ax, ay) = if cmd == 'a' { (cur_x+x, cur_y+y) } else { (x, y) };
                svg_arc_to_lines(path, cur_x, cur_y, rx, ry, xrot, laf != 0.0, sf != 0.0, ax, ay);
                cur_x = ax; cur_y = ay;
            }
            _ => break,
        }
    }
}

/// Convert SVG arc endpoint parameterisation to centre parameterisation and tessellate.
///
/// Reference: SVG 1.1 Appendix F.6.
#[allow(clippy::too_many_arguments)]
fn svg_arc_to_lines(
    path: &mut Path2dData,
    x1: f32, y1: f32,
    rx0: f32, ry0: f32,
    phi_deg: f32,
    large_arc: bool, sweep: bool,
    x2: f32, y2: f32,
) {
    let phi = phi_deg.to_radians();
    let cos_phi = phi.cos();
    let sin_phi = phi.sin();
    let (mut rx, mut ry) = (rx0.abs(), ry0.abs());

    if rx < f32::EPSILON || ry < f32::EPSILON {
        path.line_to(x2, y2);
        return;
    }

    // Step 1 — midpoint
    let dx = (x1 - x2) * 0.5;
    let dy = (y1 - y2) * 0.5;
    let x1p =  cos_phi * dx + sin_phi * dy;
    let y1p = -sin_phi * dx + cos_phi * dy;

    // Step 2 — fix radii if too small
    let lam = (x1p / rx).powi(2) + (y1p / ry).powi(2);
    if lam > 1.0 {
        let lam_sqrt = lam.sqrt();
        rx *= lam_sqrt;
        ry *= lam_sqrt;
    }

    let sq_num = (rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p)
        .max(0.0)
        .sqrt();
    let sq_den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let sq = if sq_den < f32::EPSILON { 0.0 } else { sq_num / sq_den.sqrt() };
    let sign = if large_arc == sweep { -1.0_f32 } else { 1.0_f32 };
    let cxp =  sign * sq * rx * y1p / ry;
    let cyp = -sign * sq * ry * x1p / rx;

    // Step 3 — centre
    let cx = cos_phi * cxp - sin_phi * cyp + (x1 + x2) * 0.5;
    let cy = sin_phi * cxp + cos_phi * cyp + (y1 + y2) * 0.5;

    // Step 4 — angles
    fn angle_between(ux: f32, uy: f32, vx: f32, vy: f32) -> f32 {
        let n = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
        if n < f32::EPSILON { return 0.0; }
        let c = ((ux * vx + uy * vy) / n).clamp(-1.0, 1.0);
        let sign = if ux * vy - uy * vx < 0.0 { -1.0_f32 } else { 1.0_f32 };
        sign * c.acos()
    }

    let theta1 = angle_between(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut d_theta = angle_between(
        (x1p - cxp) / rx, (y1p - cyp) / ry,
        (-x1p - cxp) / rx, (-y1p - cyp) / ry,
    );
    if !sweep && d_theta > 0.0 { d_theta -= std::f32::consts::TAU; }
    if  sweep && d_theta < 0.0 { d_theta += std::f32::consts::TAU; }

    // Tessellate
    let steps = ((rx.max(ry) * d_theta.abs()) as u32 + 4).clamp(4, 180);
    for i in 0..=steps {
        let t = theta1 + d_theta * (i as f32 / steps as f32);
        let px = cos_phi * rx * t.cos() - sin_phi * ry * t.sin() + cx;
        let py = sin_phi * rx * t.cos() + cos_phi * ry * t.sin() + cy;
        if i == 0 {
            // Make sure we start from x1,y1 exactly
            if (px - x1).abs() > 0.5 || (py - y1).abs() > 0.5 {
                path.line_to(px, py);
            }
        } else {
            path.line_to(px, py);
        }
    }
}

// ── Lexer helpers ─────────────────────────────────────────────────────────────

fn skip_wsc(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while chars.peek().map(|c| c.is_ascii_whitespace() || *c == ',').unwrap_or(false) {
        chars.next();
    }
}

fn skip_wsc_comma(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while chars.peek().map(|c| c.is_ascii_whitespace()).unwrap_or(false) { chars.next(); }
    if chars.peek() == Some(&',') { chars.next(); }
    while chars.peek().map(|c| c.is_ascii_whitespace()).unwrap_or(false) { chars.next(); }
}

fn parse_f32(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> f32 {
    skip_wsc(chars);
    let mut s = String::new();
    if chars.peek() == Some(&'-') || chars.peek() == Some(&'+') {
        s.push(chars.next().unwrap());
    }
    while chars.peek().map(|c| c.is_ascii_digit() || *c == '.').unwrap_or(false) {
        s.push(chars.next().unwrap());
    }
    // Optional exponent
    if chars.peek() == Some(&'e') || chars.peek() == Some(&'E') {
        s.push(chars.next().unwrap());
        if chars.peek() == Some(&'-') || chars.peek() == Some(&'+') {
            s.push(chars.next().unwrap());
        }
        while chars.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            s.push(chars.next().unwrap());
        }
    }
    s.parse().unwrap_or(0.0)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path2d_empty_has_no_segments() {
        let p = Path2dData::new();
        assert!(p.segments.is_empty());
    }

    #[test]
    fn path2d_move_line_close() {
        let mut p = Path2dData::new();
        p.move_to(0.0, 0.0);
        p.line_to(100.0, 0.0);
        p.line_to(100.0, 100.0);
        p.close_path();
        // Move + 2 lines + close-line
        assert_eq!(p.segments.len(), 4);
    }

    #[test]
    fn path2d_rect_produces_5_segments() {
        let mut p = Path2dData::new();
        p.rect(0.0, 0.0, 50.0, 50.0);
        // move + 3 lines + close
        assert_eq!(p.segments.len(), 5);
    }

    #[test]
    fn path2d_svg_lineto() {
        let p = Path2dData::from_svg_str("M 10 20 L 30 40 Z");
        assert!(p.segments.len() >= 3);
        // First segment is Move(10, 20)
        assert!(matches!(p.segments[0], PathSegment::Move(x, y) if (x - 10.0).abs() < 0.1 && (y - 20.0).abs() < 0.1));
    }

    #[test]
    fn path2d_svg_relative_move() {
        let p = Path2dData::from_svg_str("m 5 5 l 10 0");
        // First move should be (5,5)
        assert!(matches!(p.segments[0], PathSegment::Move(x, y) if (x - 5.0).abs() < 0.1 && (y - 5.0).abs() < 0.1));
    }

    #[test]
    fn path2d_svg_cubic() {
        let p = Path2dData::from_svg_str("M 0 0 C 10 10 20 10 30 0");
        let has_cubic = p.segments.iter().any(|s| matches!(s, PathSegment::Cubic(..)));
        assert!(has_cubic);
    }

    #[test]
    fn path2d_svg_horizontal_vertical() {
        let p = Path2dData::from_svg_str("M 0 0 H 100 V 50");
        // Should have move + H lineto + V lineto
        assert!(p.segments.len() >= 3);
    }

    #[test]
    fn path2d_to_device_space_identity() {
        let mut p = Path2dData::new();
        p.move_to(10.0, 20.0);
        p.line_to(30.0, 40.0);
        let device = p.to_device_space([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        // Identity CTM → coordinates unchanged
        assert!(matches!(device[0], PathSegment::Move(x, y) if (x - 10.0).abs() < 0.01 && (y - 20.0).abs() < 0.01));
    }

    #[test]
    fn path2d_to_device_space_translate() {
        let mut p = Path2dData::new();
        p.move_to(0.0, 0.0);
        let device = p.to_device_space([1.0, 0.0, 0.0, 1.0, 50.0, 100.0]);
        assert!(matches!(device[0], PathSegment::Move(x, y) if (x - 50.0).abs() < 0.01 && (y - 100.0).abs() < 0.01));
    }

    #[test]
    fn path2d_add_path_identity() {
        let mut a = Path2dData::new();
        a.move_to(0.0, 0.0);
        a.line_to(10.0, 0.0);

        let mut b = Path2dData::new();
        b.add_path(&a, None);
        assert_eq!(b.segments.len(), 2);
    }

    #[test]
    fn path2d_add_path_with_transform() {
        let mut a = Path2dData::new();
        a.move_to(1.0, 0.0);

        let mut b = Path2dData::new();
        // Translate by (10, 20)
        b.add_path(&a, Some([1.0, 0.0, 0.0, 1.0, 10.0, 20.0]));
        assert!(matches!(b.segments[0], PathSegment::Move(x, y) if (x - 11.0).abs() < 0.01 && (y - 20.0).abs() < 0.01));
    }

    #[test]
    fn path2d_arc_produces_lines() {
        let mut p = Path2dData::new();
        p.arc(50.0, 50.0, 40.0, 0.0, std::f32::consts::TAU, false);
        // Should have a Move + many Line segments
        assert!(p.segments.len() > 5);
        assert!(matches!(p.segments[0], PathSegment::Move(..)));
    }

    #[test]
    fn path2d_svg_arc_command() {
        // Simple arc: half circle
        let p = Path2dData::from_svg_str("M 0 0 A 50 50 0 1 0 100 0");
        assert!(p.segments.len() > 3);
    }
}
