//! HTML Canvas 2D API — `CanvasRenderingContext2D` для Lumen.
//!
//! Phase 0 реализация: CPU-растеризация в RGBA-буфер. Буфер загружается в GPU
//! через `Renderer::register_image` и рендерится через `DrawImage`.
//!
//! Покрытые операции: `fillRect`, `clearRect`, `strokeRect`, `beginPath`,
//! `moveTo`, `lineTo`, `closePath`, `fill`, `stroke`, `arc`.
//! Свойства: `fillStyle`, `strokeStyle`, `lineWidth`, `globalAlpha`.
//!
//! Не реализовано (Phase 1+): градиенты, паттерны, трансформации, clip, ImageData,
//! скругления (bezierCurveTo), текст (fillText), тень (shadowColor).

mod color;
mod path;
mod rasterize;

pub use color::CanvasColor;
pub use path::{PathCommand, PathSegment};


/// HTML Canvas 2D rendering context.
///
/// Each `Context2D` owns a RGBA pixel buffer of dimensions `width × height`.
/// Drawing operations write directly to this buffer; call [`Context2D::pixels`]
/// to read the result for upload to GPU.
#[derive(Debug, Clone)]
pub struct Context2D {
    width: u32,
    height: u32,
    /// RGBA8 pixels, row-major, top-left origin.
    pixels: Vec<u8>,

    // Drawing state
    pub fill_style: CanvasColor,
    pub stroke_style: CanvasColor,
    pub line_width: f32,
    pub global_alpha: f32,

    // Current path accumulator
    path: Vec<PathSegment>,
    path_start: Option<(f32, f32)>,
    pen: (f32, f32),
}

impl Context2D {
    /// Create a new context with a transparent black buffer.
    pub fn new(width: u32, height: u32) -> Self {
        let size = (width * height * 4) as usize;
        Self {
            width,
            height,
            pixels: vec![0u8; size],
            fill_style: CanvasColor::rgba(0, 0, 0, 255),
            stroke_style: CanvasColor::rgba(0, 0, 0, 255),
            line_width: 1.0,
            global_alpha: 1.0,
            path: Vec::new(),
            path_start: None,
            pen: (0.0, 0.0),
        }
    }

    pub fn width(&self) -> u32 { self.width }
    pub fn height(&self) -> u32 { self.height }

    /// Raw RGBA8 pixel data.
    pub fn pixels(&self) -> &[u8] { &self.pixels }

    /// Resize the canvas (clears the buffer).
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        let size = (width * height * 4) as usize;
        self.pixels = vec![0u8; size];
    }

    // ── Rect operations ───────────────────────────────────────────────────

    /// `clearRect(x, y, w, h)` — erase region to transparent black.
    ///
    /// Direct write (not source-over) — matches the spec's "copy" semantics.
    pub fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        if w <= 0.0 || h <= 0.0 { return; }
        let x0 = x.max(0.0) as u32;
        let y0 = y.max(0.0) as u32;
        let x1 = (x + w).min(self.width as f32) as u32;
        let y1 = (y + h).min(self.height as f32) as u32;
        for row in y0..y1 {
            for col in x0..x1 {
                let idx = ((row * self.width + col) * 4) as usize;
                self.pixels[idx..idx + 4].fill(0);
            }
        }
    }

    /// `fillRect(x, y, w, h)` — fill region with current `fillStyle`.
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let color = self.fill_style.with_alpha_mult(self.global_alpha);
        self.fill_rect_color(x, y, w, h, color);
    }

    /// `strokeRect(x, y, w, h)` — stroke the outline of a rectangle.
    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let lw = self.line_width;
        let half = lw * 0.5;
        let color = self.stroke_style.with_alpha_mult(self.global_alpha);
        // Four sides: top, bottom, left, right
        self.fill_rect_color(x,         y,         w,   lw,  color); // top
        self.fill_rect_color(x,         y + h - lw, w,  lw,  color); // bottom
        self.fill_rect_color(x,         y + half,   lw, h - lw, color); // left
        self.fill_rect_color(x + w - lw, y + half,  lw, h - lw, color); // right
    }

    // ── Path API ──────────────────────────────────────────────────────────

    /// `beginPath()` — discard current path.
    pub fn begin_path(&mut self) {
        self.path.clear();
        self.path_start = None;
    }

    /// `moveTo(x, y)` — start a new sub-path.
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.pen = (x, y);
        if self.path_start.is_none() {
            self.path_start = Some((x, y));
        }
        self.path.push(PathSegment::Move(x, y));
    }

    /// `lineTo(x, y)` — add a line segment.
    pub fn line_to(&mut self, x: f32, y: f32) {
        if self.path.is_empty() {
            self.path_start = Some((x, y));
            self.path.push(PathSegment::Move(x, y));
        } else {
            self.path.push(PathSegment::Line(self.pen.0, self.pen.1, x, y));
        }
        self.pen = (x, y);
    }

    /// `closePath()` — add a line back to the sub-path start.
    pub fn close_path(&mut self) {
        if let Some((sx, sy)) = self.path_start {
            let (px, py) = self.pen;
            self.path.push(PathSegment::Line(px, py, sx, sy));
            self.pen = (sx, sy);
        }
    }

    /// `arc(cx, cy, r, start_angle, end_angle[, anticlockwise])` — add an arc.
    /// `anticlockwise` = false by default.
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, start: f32, end: f32, ccw: bool) {
        let step_count = ((r * (end - start).abs()) as u32 + 4).clamp(4, 180);
        let steps = step_count as f32;
        let delta = if ccw { -(end - start) } else { end - start };
        let first_x = cx + r * start.cos();
        let first_y = cy + r * start.sin();
        if self.path.is_empty() {
            self.move_to(first_x, first_y);
        } else {
            self.line_to(first_x, first_y);
        }
        for i in 1..=step_count {
            let t = start + delta * (i as f32 / steps);
            let x = cx + r * t.cos();
            let y = cy + r * t.sin();
            self.line_to(x, y);
        }
    }

    /// `fill()` — fill the current path with `fillStyle`.
    pub fn fill(&mut self) {
        let color = self.fill_style.with_alpha_mult(self.global_alpha);
        let path = self.path.clone();
        rasterize::fill_path(self, &path, color);
    }

    /// `stroke()` — stroke the current path with `strokeStyle`.
    pub fn stroke(&mut self) {
        let color = self.stroke_style.with_alpha_mult(self.global_alpha);
        let lw = self.line_width;
        let path = self.path.clone();
        rasterize::stroke_path(self, &path, lw, color);
    }

    // ── Low-level helpers ─────────────────────────────────────────────────

    /// Internal: fill an axis-aligned rect with an explicit color.
    pub(crate) fn fill_rect_color(&mut self, x: f32, y: f32, w: f32, h: f32, color: CanvasColor) {
        if w <= 0.0 || h <= 0.0 { return; }
        let x0 = x.max(0.0) as u32;
        let y0 = y.max(0.0) as u32;
        let x1 = (x + w).min(self.width as f32) as u32;
        let y1 = (y + h).min(self.height as f32) as u32;
        for row in y0..y1 {
            for col in x0..x1 {
                self.set_pixel(col, row, color);
            }
        }
    }

    /// Alpha-composite `color` over the pixel at `(x, y)`.
    pub(crate) fn set_pixel(&mut self, x: u32, y: u32, color: CanvasColor) {
        if x >= self.width || y >= self.height { return; }
        let idx = ((y * self.width + x) * 4) as usize;
        let dst = &mut self.pixels[idx..idx + 4];
        // Porter-Duff source-over (straight alpha).
        let sa = color.a as f32 / 255.0;
        let da = dst[3] as f32 / 255.0;
        let oa = sa + da * (1.0 - sa);
        if oa < f32::EPSILON {
            dst.fill(0);
            return;
        }
        dst[0] = ((color.r as f32 * sa + dst[0] as f32 * da * (1.0 - sa)) / oa) as u8;
        dst[1] = ((color.g as f32 * sa + dst[1] as f32 * da * (1.0 - sa)) / oa) as u8;
        dst[2] = ((color.b as f32 * sa + dst[2] as f32 * da * (1.0 - sa)) / oa) as u8;
        dst[3] = (oa * 255.0) as u8;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_context_transparent() {
        let ctx = Context2D::new(4, 4);
        assert!(ctx.pixels().iter().all(|&b| b == 0));
    }

    #[test]
    fn fill_rect_paints_region() {
        let mut ctx = Context2D::new(10, 10);
        ctx.fill_style = CanvasColor::rgba(255, 0, 0, 255);
        ctx.fill_rect(0.0, 0.0, 10.0, 10.0);
        // First pixel should be red
        let p = ctx.pixels();
        assert_eq!(p[0], 255); // R
        assert_eq!(p[1], 0);   // G
        assert_eq!(p[2], 0);   // B
        assert_eq!(p[3], 255); // A
    }

    #[test]
    fn clear_rect_erases() {
        let mut ctx = Context2D::new(4, 4);
        ctx.fill_style = CanvasColor::rgba(255, 0, 0, 255);
        ctx.fill_rect(0.0, 0.0, 4.0, 4.0);
        ctx.clear_rect(0.0, 0.0, 4.0, 4.0);
        assert!(ctx.pixels().iter().all(|&b| b == 0));
    }

    #[test]
    fn fill_rect_clips_to_bounds() {
        let mut ctx = Context2D::new(4, 4);
        ctx.fill_style = CanvasColor::rgba(0, 255, 0, 255);
        // Rect extends beyond canvas
        ctx.fill_rect(-2.0, -2.0, 10.0, 10.0);
        // All pixels should be green
        let p = ctx.pixels();
        assert_eq!(p[1], 255); // G channel of pixel (0,0)
    }

    #[test]
    fn stroke_rect_draws_border() {
        let mut ctx = Context2D::new(10, 10);
        ctx.stroke_style = CanvasColor::rgba(0, 0, 255, 255);
        ctx.line_width = 1.0;
        ctx.stroke_rect(0.0, 0.0, 10.0, 10.0);
        // Top row pixel (0,0) should be blue
        let p = ctx.pixels();
        assert_eq!(p[2], 255); // B channel
    }

    #[test]
    fn path_fill_triangle() {
        let mut ctx = Context2D::new(20, 20);
        ctx.fill_style = CanvasColor::rgba(255, 255, 0, 255);
        ctx.begin_path();
        ctx.move_to(10.0, 0.0);
        ctx.line_to(20.0, 20.0);
        ctx.line_to(0.0, 20.0);
        ctx.close_path();
        ctx.fill();
        // Bottom-left pixel (0,19) should be yellow
        let idx = (19 * 20) * 4;
        let p = ctx.pixels();
        assert_eq!(p[idx], 255); // R
        assert_eq!(p[idx + 1], 255); // G
    }

    #[test]
    fn global_alpha_applied() {
        let mut ctx = Context2D::new(4, 4);
        ctx.fill_style = CanvasColor::rgba(255, 0, 0, 255);
        ctx.global_alpha = 0.5;
        ctx.fill_rect(0.0, 0.0, 4.0, 4.0);
        let p = ctx.pixels();
        // Alpha should be ~128 (half)
        assert!(p[3] > 100 && p[3] < 150, "alpha={}", p[3]);
    }

    #[test]
    fn canvas_color_parse_hex() {
        let c = CanvasColor::from_css_str("#ff8040").unwrap();
        assert_eq!(c.r, 0xff);
        assert_eq!(c.g, 0x80);
        assert_eq!(c.b, 0x40);
        assert_eq!(c.a, 255);
    }

    #[test]
    fn canvas_color_parse_rgb() {
        let c = CanvasColor::from_css_str("rgb(10, 20, 30)").unwrap();
        assert_eq!(c.r, 10);
        assert_eq!(c.g, 20);
        assert_eq!(c.b, 30);
    }

    #[test]
    fn canvas_color_parse_named_red() {
        let c = CanvasColor::from_css_str("red").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn resize_clears_buffer() {
        let mut ctx = Context2D::new(4, 4);
        ctx.fill_style = CanvasColor::rgba(255, 0, 0, 255);
        ctx.fill_rect(0.0, 0.0, 4.0, 4.0);
        ctx.resize(8, 8);
        assert!(ctx.pixels().iter().all(|&b| b == 0));
        assert_eq!(ctx.width(), 8);
        assert_eq!(ctx.height(), 8);
    }
}
