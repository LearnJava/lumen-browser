/// A single segment in a 2D path.
#[derive(Debug, Clone)]
pub enum PathSegment {
    /// `moveTo(x, y)`
    Move(f32, f32),
    /// `lineTo` — from `(x0, y0)` to `(x1, y1)`.
    Line(f32, f32, f32, f32),
}

/// Alias kept for API symmetry with the HTML spec (`PathCommand` = verb).
pub type PathCommand = PathSegment;
