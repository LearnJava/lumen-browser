/// A single segment in a 2D path (HTML Canvas 2D §4.12.4).
#[derive(Debug, Clone)]
pub enum PathSegment {
    /// `moveTo(x, y)` — start a new sub-path at (x, y).
    Move(f32, f32),
    /// `lineTo` — straight line from `(x0, y0)` to `(x1, y1)`.
    Line(f32, f32, f32, f32),
    /// `bezierCurveTo` — cubic Bézier from `(x0,y0)` through control points
    /// `(cp1x,cp1y)` and `(cp2x,cp2y)` to endpoint `(x1,y1)`.
    Cubic(f32, f32, f32, f32, f32, f32, f32, f32),
    /// `quadraticCurveTo` — quadratic Bézier from `(x0,y0)` through `(cpx,cpy)` to `(x1,y1)`.
    Quadratic(f32, f32, f32, f32, f32, f32),
}

/// Alias kept for API symmetry with the HTML spec (`PathCommand` = verb).
pub type PathCommand = PathSegment;
