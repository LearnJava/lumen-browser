//! HTML Canvas 2D API — `CanvasRenderingContext2D` для Lumen.
//!
//! Phase 5 добавляет `Path2D` — переиспользуемые объекты путей (HTML LS §4.12.5.1.5):
//! конструктор из SVG-строки, все команды CanvasPath mixin, `addPath(other, transform?)`,
//! и `ctx.fill/stroke/clip(path2d)` для использования Path2D вместо текущего пути.
//!
//! Покрытые операции: `fillRect`, `clearRect`, `strokeRect`, `beginPath`,
//! `moveTo`, `lineTo`, `closePath`, `fill`, `stroke`, `arc`, `ellipse`,
//! `arcTo`, `bezierCurveTo`, `quadraticCurveTo`, `rect`, `save`, `restore`,
//! `translate`, `rotate`, `scale`, `transform`, `setTransform`, `resetTransform`,
//! `clip`, `drawImage`, `putImageData`, `createImageData`, `fillText`, `strokeText`,
//! `fill(path2d)`, `stroke(path2d)`, `clip(path2d)`.
//! Свойства: `fillStyle`, `strokeStyle`, `lineWidth`, `globalAlpha`,
//! `globalCompositeOperation`, `lineCap`, `lineJoin`, `miterLimit`,
//! `shadowColor`, `shadowBlur`, `shadowOffsetX`, `shadowOffsetY`, `font`.

mod color;
mod path;
pub mod path2d;
mod rasterize;
pub mod fp_noise;

pub use color::CanvasColor;
pub use path::{PathCommand, PathSegment};
pub use path2d::Path2dData;
pub use fp_noise::CanvasNoiseGenerator;
use lumen_core::ColorSpace;

// ── Enums ─────────────────────────────────────────────────────────────────────

/// CSS `globalCompositeOperation` — Porter-Duff compositing mode.
///
/// See HTML Living Standard §4.12.5.1.14.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompositeOperation {
    /// Source painted over destination (default).
    #[default]
    SourceOver,
    /// Source only where it overlaps destination.
    SourceIn,
    /// Source only where it does NOT overlap destination.
    SourceOut,
    /// Source only where destination exists; destination otherwise.
    SourceAtop,
    /// Destination painted over source.
    DestinationOver,
    /// Destination only where source exists.
    DestinationIn,
    /// Destination only where source does NOT exist.
    DestinationOut,
    /// Destination where it does NOT overlap source; source elsewhere.
    DestinationAtop,
    /// Source XOR destination — neither where they overlap.
    Xor,
    /// Source copied to destination (ignores destination alpha).
    Copy,
    /// Sum of source and destination, clamped to 1.
    Lighter,
    /// Multiply source and destination channel values.
    Multiply,
    /// Screen blend: 1 - (1-s)(1-d).
    Screen,
    /// Overlay blend.
    Overlay,
    /// Per-channel minimum.
    Darken,
    /// Per-channel maximum.
    Lighten,
}

impl CompositeOperation {
    /// Parse from the CSS string literal used in `ctx.globalCompositeOperation`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "source-over"      => Some(Self::SourceOver),
            "source-in"        => Some(Self::SourceIn),
            "source-out"       => Some(Self::SourceOut),
            "source-atop"      => Some(Self::SourceAtop),
            "destination-over" => Some(Self::DestinationOver),
            "destination-in"   => Some(Self::DestinationIn),
            "destination-out"  => Some(Self::DestinationOut),
            "destination-atop" => Some(Self::DestinationAtop),
            "xor"              => Some(Self::Xor),
            "copy"             => Some(Self::Copy),
            "lighter"          => Some(Self::Lighter),
            "multiply"         => Some(Self::Multiply),
            "screen"           => Some(Self::Screen),
            "overlay"          => Some(Self::Overlay),
            "darken"           => Some(Self::Darken),
            "lighten"          => Some(Self::Lighten),
            _                  => None,
        }
    }

    /// Canonical CSS string name for this operation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SourceOver      => "source-over",
            Self::SourceIn        => "source-in",
            Self::SourceOut       => "source-out",
            Self::SourceAtop      => "source-atop",
            Self::DestinationOver => "destination-over",
            Self::DestinationIn   => "destination-in",
            Self::DestinationOut  => "destination-out",
            Self::DestinationAtop => "destination-atop",
            Self::Xor             => "xor",
            Self::Copy            => "copy",
            Self::Lighter         => "lighter",
            Self::Multiply        => "multiply",
            Self::Screen          => "screen",
            Self::Overlay         => "overlay",
            Self::Darken          => "darken",
            Self::Lighten         => "lighten",
        }
    }
}

/// CSS `lineCap` — how line endpoints are rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineCap {
    /// Flat edge at the endpoint (default).
    #[default]
    Butt,
    /// Round cap extending beyond the endpoint by `lineWidth/2`.
    Round,
    /// Square cap extending beyond the endpoint by `lineWidth/2`.
    Square,
}

impl LineCap {
    /// Parse from CSS string.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "butt"   => Some(Self::Butt),
            "round"  => Some(Self::Round),
            "square" => Some(Self::Square),
            _        => None,
        }
    }
}

/// CSS `lineJoin` — how line segments connect at corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineJoin {
    /// Sharp mitered corner (default). Clipped to `miterLimit`.
    #[default]
    Miter,
    /// Round corner extending to `lineWidth/2` radius.
    Round,
    /// Bevelled (flat) corner.
    Bevel,
}

impl LineJoin {
    /// Parse from CSS string.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "miter" => Some(Self::Miter),
            "round" => Some(Self::Round),
            "bevel" => Some(Self::Bevel),
            _       => None,
        }
    }
}

// ── DrawState ─────────────────────────────────────────────────────────────────

/// All drawing state captured by `save()` and restored by `restore()`.
///
/// Does NOT include the current path — spec §4.12.5.1.2 says `save/restore`
/// do NOT save the current path.
#[derive(Debug, Clone)]
pub struct DrawState {
    /// Current Transformation Matrix: `[a, b, c, d, e, f]` (column-major affine).
    ///
    /// Transforms user coordinates to canvas device pixels:
    /// `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
    pub ctm: [f32; 6],
    /// Current fill paint source (colour, gradient, or pattern).
    pub fill_style: PaintSource,
    /// Current stroke paint source.
    pub stroke_style: PaintSource,
    /// Stroke line width in user units.
    pub line_width: f32,
    /// Global opacity multiplier `[0.0, 1.0]`.
    pub global_alpha: f32,
    /// Porter-Duff compositing mode.
    pub composite_operation: CompositeOperation,
    /// Line cap style.
    pub line_cap: LineCap,
    /// Line join style.
    pub line_join: LineJoin,
    /// Miter limit for `LineJoin::Miter`.
    pub miter_limit: f32,
    /// Shadow colour (CSS color; default transparent black).
    pub shadow_color: CanvasColor,
    /// Shadow blur radius in pixels (default 0 = no blur).
    pub shadow_blur: f32,
    /// Shadow horizontal offset in pixels.
    pub shadow_offset_x: f32,
    /// Shadow vertical offset in pixels.
    pub shadow_offset_y: f32,
    /// Clipping region bitmap: `None` = no clip; `Some(mask)` = per-pixel bool (true = allowed).
    pub clip_mask: Option<Vec<bool>>,
    /// CSS font string e.g. `"16px sans-serif"`.
    pub font: String,
    /// Horizontal text alignment: `"start" | "end" | "left" | "right" | "center"`. Default `"start"`.
    pub text_align: String,
    /// Vertical text baseline: `"alphabetic" | "top" | "hanging" | "middle" | "ideographic" | "bottom"`. Default `"alphabetic"`.
    pub text_baseline: String,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            ctm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            fill_style: PaintSource::Color(CanvasColor::rgba(0, 0, 0, 255)),
            stroke_style: PaintSource::Color(CanvasColor::rgba(0, 0, 0, 255)),
            line_width: 1.0,
            global_alpha: 1.0,
            composite_operation: CompositeOperation::SourceOver,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            shadow_color: CanvasColor::rgba(0, 0, 0, 0),
            shadow_blur: 0.0,
            shadow_offset_x: 0.0,
            shadow_offset_y: 0.0,
            clip_mask: None,
            font: String::from("10px sans-serif"),
            text_align: String::from("start"),
            text_baseline: String::from("alphabetic"),
        }
    }
}

// ── Gradient / Pattern / PaintSource ─────────────────────────────────────────

/// One colour stop in a [`CanvasGradient`].
#[derive(Debug, Clone, Copy)]
pub struct ColorStop {
    /// Position in `[0.0, 1.0]`.
    pub offset: f32,
    /// RGBA colour at this stop.
    pub color: CanvasColor,
}

/// Gradient kind — stores the defining geometry in user (pre-CTM) space.
#[derive(Debug, Clone)]
pub enum GradientKind {
    /// CSS linear gradient: line from `(x0,y0)` to `(x1,y1)`.
    Linear { x0: f32, y0: f32, x1: f32, y1: f32 },
    /// CSS radial gradient: inner circle `(x0,y0,r0)` → outer circle `(x1,y1,r1)`.
    Radial { x0: f32, y0: f32, r0: f32, x1: f32, y1: f32, r1: f32 },
    /// CSS conic gradient: start angle (radians) around centre `(cx,cy)`.
    Conic { angle: f32, cx: f32, cy: f32 },
}

/// Canvas gradient object (`createLinearGradient` / `createRadialGradient` / `createConicGradient`).
///
/// Coordinates are in user space (canvas coordinate system before CTM).
/// `sample(x, y)` returns the interpolated colour at device-pixel `(x, y)`.
#[derive(Debug, Clone)]
pub struct CanvasGradient {
    /// Geometry of this gradient.
    pub kind: GradientKind,
    /// Colour stops sorted by `offset`.
    pub stops: Vec<ColorStop>,
}

impl CanvasGradient {
    /// Create a linear gradient from `(x0,y0)` to `(x1,y1)`.
    pub fn linear(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { kind: GradientKind::Linear { x0, y0, x1, y1 }, stops: Vec::new() }
    }
    /// Create a radial gradient between two circles.
    pub fn radial(x0: f32, y0: f32, r0: f32, x1: f32, y1: f32, r1: f32) -> Self {
        Self { kind: GradientKind::Radial { x0, y0, r0, x1, y1, r1 }, stops: Vec::new() }
    }
    /// Create a conic gradient starting at `angle` (radians) around `(cx,cy)`.
    pub fn conic(angle: f32, cx: f32, cy: f32) -> Self {
        Self { kind: GradientKind::Conic { angle, cx, cy }, stops: Vec::new() }
    }

    /// Add a colour stop at `offset ∈ [0,1]`.
    pub fn add_color_stop(&mut self, offset: f32, color: CanvasColor) {
        self.stops.push(ColorStop { offset, color });
        self.stops.sort_by(|a, b| a.offset.partial_cmp(&b.offset).unwrap_or(core::cmp::Ordering::Equal));
    }

    /// Sample the gradient colour at device pixel `(x, y)`.
    pub fn sample(&self, x: f32, y: f32) -> CanvasColor {
        let t = self.compute_t(x, y);
        self.sample_at(t)
    }

    fn compute_t(&self, x: f32, y: f32) -> f32 {
        match self.kind {
            GradientKind::Linear { x0, y0, x1, y1 } => {
                let dx = x1 - x0;
                let dy = y1 - y0;
                let len_sq = dx * dx + dy * dy;
                if len_sq < f32::EPSILON { return 0.0; }
                ((x - x0) * dx + (y - y0) * dy) / len_sq
            }
            GradientKind::Radial { x0, y0, r0, x1, y1, r1 } => {
                // Project onto gradient axis, then solve for t using distance to focus
                let dx = x1 - x0;
                let dy = y1 - y0;
                let dr = r1 - r0;
                let axis_len = (dx * dx + dy * dy).sqrt();
                if axis_len < f32::EPSILON && dr.abs() < f32::EPSILON { return 0.0; }
                // Simple concentric case
                let px = x - x0;
                let py = y - y0;
                let dist = (px * px + py * py).sqrt();
                if (r1 - r0).abs() < f32::EPSILON {
                    if r0 < f32::EPSILON { return 0.0; }
                    dist / r0
                } else {
                    (dist - r0) / (r1 - r0)
                }
            }
            GradientKind::Conic { angle, cx, cy } => {
                let px = x - cx;
                let py = y - cy;
                let a = atan2_approx(py, px);
                let t = (a - angle) / (2.0 * core::f32::consts::PI);
                t - t.floor()
            }
        }
    }

    fn sample_at(&self, t: f32) -> CanvasColor {
        if self.stops.is_empty() { return CanvasColor::rgba(0, 0, 0, 255); }
        if self.stops.len() == 1 { return self.stops[0].color; }
        let t = t.clamp(0.0, 1.0);
        for i in 0..self.stops.len() - 1 {
            let s0 = &self.stops[i];
            let s1 = &self.stops[i + 1];
            if t >= s0.offset && t <= s1.offset {
                let range = s1.offset - s0.offset;
                if range < f32::EPSILON { return s1.color; }
                let frac = (t - s0.offset) / range;
                return lerp_color(s0.color, s1.color, frac);
            }
        }
        if t < self.stops[0].offset { return self.stops[0].color; }
        self.stops.last().unwrap().color
    }
}

/// Pattern repetition mode (`createPattern` second argument).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    /// Tile in both directions (default).
    #[default]
    Repeat,
    /// Tile only horizontally.
    RepeatX,
    /// Tile only vertically.
    RepeatY,
    /// No repetition — pattern drawn once at origin.
    NoRepeat,
}

/// Canvas pattern object (`createPattern`).
///
/// Wraps a source RGBA8 image that is tiled according to `repeat` mode.
#[derive(Debug, Clone)]
pub struct CanvasPattern {
    /// RGBA8 pixels of the source image, row-major.
    pub pixels: Vec<u8>,
    /// Source image width.
    pub width: u32,
    /// Source image height.
    pub height: u32,
    /// Tiling mode.
    pub repeat: RepeatMode,
}

impl CanvasPattern {
    /// Create a new pattern from RGBA8 pixel data.
    pub fn new(pixels: Vec<u8>, width: u32, height: u32, repeat: RepeatMode) -> Self {
        Self { pixels, width, height, repeat }
    }

    /// Sample the pattern colour at device pixel `(x, y)`.
    pub fn sample(&self, x: f32, y: f32) -> CanvasColor {
        if self.width == 0 || self.height == 0 { return CanvasColor::rgba(0, 0, 0, 0); }
        let xi = x as i32;
        let yi = y as i32;
        let px = match self.repeat {
            RepeatMode::Repeat | RepeatMode::RepeatX => {
                xi.rem_euclid(self.width as i32) as u32
            }
            RepeatMode::RepeatY | RepeatMode::NoRepeat => {
                if xi < 0 || xi >= self.width as i32 { return CanvasColor::rgba(0, 0, 0, 0); }
                xi as u32
            }
        };
        let py = match self.repeat {
            RepeatMode::Repeat | RepeatMode::RepeatY => {
                yi.rem_euclid(self.height as i32) as u32
            }
            RepeatMode::RepeatX | RepeatMode::NoRepeat => {
                if yi < 0 || yi >= self.height as i32 { return CanvasColor::rgba(0, 0, 0, 0); }
                yi as u32
            }
        };
        let idx = ((py * self.width + px) * 4) as usize;
        if idx + 3 >= self.pixels.len() { return CanvasColor::rgba(0, 0, 0, 0); }
        CanvasColor::rgba(self.pixels[idx], self.pixels[idx + 1], self.pixels[idx + 2], self.pixels[idx + 3])
    }
}

/// Paint source: a solid colour, a gradient, or a pattern.
///
/// Used as `fillStyle` and `strokeStyle` in [`Context2D`].
#[derive(Debug, Clone)]
pub enum PaintSource {
    /// Solid colour.
    Color(CanvasColor),
    /// Gradient (linear, radial, or conic).
    Gradient(CanvasGradient),
    /// Repeating image pattern.
    Pattern(CanvasPattern),
}

impl Default for PaintSource {
    fn default() -> Self { PaintSource::Color(CanvasColor::rgba(0, 0, 0, 255)) }
}

impl PaintSource {
    /// Sample the paint at device pixel centre `(x + 0.5, y + 0.5)`.
    pub fn sample(&self, x: f32, y: f32) -> CanvasColor {
        match self {
            PaintSource::Color(c) => *c,
            PaintSource::Gradient(g) => g.sample(x, y),
            PaintSource::Pattern(p) => p.sample(x, y),
        }
    }

    /// Return the solid colour, or transparent black if this is a gradient/pattern.
    ///
    /// Convenience for tests and fallbacks; not for rendering (use `sample` instead).
    pub fn as_color_or_black(&self) -> CanvasColor {
        match self {
            PaintSource::Color(c) => *c,
            _ => CanvasColor::rgba(0, 0, 0, 0),
        }
    }
}

impl From<CanvasColor> for PaintSource {
    fn from(c: CanvasColor) -> Self { PaintSource::Color(c) }
}

/// Deterministic atan2 approximation (no libm) — matches `gradient_math.rs` in lumen-paint.
fn atan2_approx(y: f32, x: f32) -> f32 {
    use core::f32::consts::{FRAC_PI_4, PI};
    if x == 0.0 && y == 0.0 { return 0.0; }
    let ax = x.abs();
    let ay = y.abs();
    let (a, swap) = if ay > ax { (ax / ay, true) } else { (ay / ax, false) };
    let s = a * a;
    let r = ((-0.046_496_47 * s + 0.159_314_22) * s - 0.327_622_76) * s * a + a;
    let r = if swap { FRAC_PI_4 * 2.0 - r } else { r };
    let r = if x < 0.0 { PI - r } else { r };
    if y < 0.0 { -r } else { r }
}

/// Linear colour interpolation between `a` and `b` in sRGB.
fn lerp_color(a: CanvasColor, b: CanvasColor, t: f32) -> CanvasColor {
    CanvasColor::rgba(
        lerp_u8(a.r, b.r, t),
        lerp_u8(a.g, b.g, t),
        lerp_u8(a.b, b.b, t),
        lerp_u8(a.a, b.a, t),
    )
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8
}

// ── Context2D ─────────────────────────────────────────────────────────────────

/// HTML Canvas 2D rendering context.
///
/// Each `Context2D` owns a RGBA pixel buffer of dimensions `width × height`.
/// Drawing operations write directly to this buffer; call [`Context2D::pixels`]
/// to read the result for upload to GPU.
///
/// Optional fingerprint randomization: when a `noise_generator` is set,
/// [`get_image_data()`] applies per-session noise to pixel data before returning.
/// This is used for anti-detection fingerprint randomization (ADR-007).
#[derive(Debug, Clone)]
pub struct Context2D {
    /// Canvas width in device pixels.
    width: u32,
    /// Canvas height in device pixels.
    height: u32,
    /// RGBA8 pixels, row-major, top-left origin.
    pixels: Vec<u8>,

    // ── Drawing state (also saved by save/restore) ──────────────────────────
    /// Current Transformation Matrix `[a, b, c, d, e, f]`.
    pub ctm: [f32; 6],
    /// Current fill paint (colour, gradient, or pattern).
    pub fill_style: PaintSource,
    /// Current stroke paint.
    pub stroke_style: PaintSource,
    /// Stroke line width in user units.
    pub line_width: f32,
    /// Global opacity `[0.0, 1.0]`.
    pub global_alpha: f32,
    /// Porter-Duff compositing mode.
    pub composite_operation: CompositeOperation,
    /// Line cap style.
    pub line_cap: LineCap,
    /// Line join style.
    pub line_join: LineJoin,
    /// Miter limit.
    pub miter_limit: f32,
    /// Shadow colour (default: transparent black = no shadow).
    pub shadow_color: CanvasColor,
    /// Shadow blur radius in pixels.
    pub shadow_blur: f32,
    /// Shadow horizontal offset in pixels.
    pub shadow_offset_x: f32,
    /// Shadow vertical offset in pixels.
    pub shadow_offset_y: f32,
    /// CSS font string, e.g. `"16px sans-serif"`.
    pub font: String,
    /// Horizontal text alignment: `"start" | "end" | "left" | "right" | "center"`. Default `"start"`.
    pub text_align: String,
    /// Vertical text baseline: `"alphabetic" | "top" | "hanging" | "middle" | "ideographic" | "bottom"`. Default `"alphabetic"`.
    pub text_baseline: String,

    // ── Path accumulator ────────────────────────────────────────────────────
    /// Segments accumulated since the last `beginPath()`.
    path: Vec<PathSegment>,
    /// Start of the current sub-path (for `closePath`).
    path_start: Option<(f32, f32)>,
    /// Current pen position in *device* pixels (post-CTM).
    pen: (f32, f32),

    // ── Clipping region ─────────────────────────────────────────────────────
    /// Per-pixel clip mask: `None` = no clip; `Some(v)` where `v[y*w+x]` is `true`
    /// when the pixel is within the clip region (may be drawn).
    clip_mask: Option<Vec<bool>>,

    // ── State stack ─────────────────────────────────────────────────────────
    /// Stack of saved drawing states (via `save()`).
    state_stack: Vec<DrawState>,

    /// Optional per-session noise generator for canvas fingerprint randomization.
    /// Set by BrowserSession when creating a context; if set, getImageData() applies noise.
    noise_generator: Option<CanvasNoiseGenerator>,

    /// Canvas color space: sRGB (default), Display P3, or Rec2020.
    /// Used for getImageData() to identify the color space of pixel data.
    color_space: ColorSpace,
}

impl Context2D {
    /// Create a new context with a transparent black buffer and identity CTM.
    pub fn new(width: u32, height: u32) -> Self {
        let size = (width * height * 4) as usize;
        Self {
            width,
            height,
            pixels: vec![0u8; size],
            ctm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            fill_style: PaintSource::Color(CanvasColor::rgba(0, 0, 0, 255)),
            stroke_style: PaintSource::Color(CanvasColor::rgba(0, 0, 0, 255)),
            line_width: 1.0,
            global_alpha: 1.0,
            composite_operation: CompositeOperation::SourceOver,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            shadow_color: CanvasColor::rgba(0, 0, 0, 0),
            shadow_blur: 0.0,
            shadow_offset_x: 0.0,
            shadow_offset_y: 0.0,
            font: String::from("10px sans-serif"),
            text_align: String::from("start"),
            text_baseline: String::from("alphabetic"),
            path: Vec::new(),
            path_start: None,
            pen: (0.0, 0.0),
            clip_mask: None,
            state_stack: Vec::new(),
            noise_generator: None,
            color_space: ColorSpace::Srgb,
        }
    }

    /// Set the optional noise generator for fingerprint randomization.
    ///
    /// Called by BrowserSession with a per-session seed.
    /// When set, `get_image_data()` will apply noise to returned pixel data.
    pub fn set_noise_generator(&mut self, generator: CanvasNoiseGenerator) {
        self.noise_generator = Some(generator);
    }

    /// Get a copy of pixel data with optional noise applied (for `getImageData()`).
    ///
    /// If a noise generator is set, returns a noisy copy; otherwise returns raw pixels.
    pub fn get_image_data(&self) -> Vec<u8> {
        let mut data = self.pixels.clone();
        if let Some(mut noise_gen) = self.noise_generator.clone() {
            noise_gen.apply_noise_to_buffer(&mut data);
        }
        data
    }

    /// Create a context pre-filled with the given RGBA8 pixel buffer.
    ///
    /// `pixels` must be exactly `width * height * 4` bytes (RGBA8, row-major).
    /// If the length mismatches, the buffer is zero-filled instead.
    /// Color space defaults to sRGB; use set_color_space() for wide-gamut.
    pub fn from_pixels(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        let expected = (width * height * 4) as usize;
        let mut ctx = Self::new(width, height);
        if pixels.len() == expected {
            ctx.pixels = pixels;
        }
        ctx
    }

    /// Canvas width in device pixels.
    pub fn width(&self) -> u32 { self.width }
    /// Canvas height in device pixels.
    pub fn height(&self) -> u32 { self.height }

    /// Canvas color space (sRGB, Display P3, or Rec2020).
    pub fn color_space(&self) -> ColorSpace { self.color_space }

    /// Set the canvas color space for wide-gamut image handling.
    pub fn set_color_space(&mut self, space: ColorSpace) { self.color_space = space; }

    /// Raw RGBA8 pixel data (no noise applied).
    pub fn pixels(&self) -> &[u8] { &self.pixels }

    /// Resize the canvas (clears the buffer and resets the CTM to identity).
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        let size = (width * height * 4) as usize;
        self.pixels = vec![0u8; size];
    }

    // ── State stack ───────────────────────────────────────────────────────────

    /// `save()` — push the current drawing state onto the stack.
    ///
    /// Does NOT save the current path (spec §4.12.5.1.2).
    pub fn save(&mut self) {
        self.state_stack.push(DrawState {
            ctm: self.ctm,
            fill_style: self.fill_style.clone(),
            stroke_style: self.stroke_style.clone(),
            line_width: self.line_width,
            global_alpha: self.global_alpha,
            composite_operation: self.composite_operation,
            line_cap: self.line_cap,
            line_join: self.line_join,
            miter_limit: self.miter_limit,
            shadow_color: self.shadow_color,
            shadow_blur: self.shadow_blur,
            shadow_offset_x: self.shadow_offset_x,
            shadow_offset_y: self.shadow_offset_y,
            clip_mask: self.clip_mask.clone(),
            font: self.font.clone(),
            text_align: self.text_align.clone(),
            text_baseline: self.text_baseline.clone(),
        });
    }

    /// `restore()` — pop and restore the most recently saved drawing state.
    ///
    /// No-op if the stack is empty (spec §4.12.5.1.2).
    pub fn restore(&mut self) {
        if let Some(state) = self.state_stack.pop() {
            self.ctm = state.ctm;
            self.fill_style = state.fill_style;
            self.stroke_style = state.stroke_style;
            self.line_width = state.line_width;
            self.global_alpha = state.global_alpha;
            self.composite_operation = state.composite_operation;
            self.line_cap = state.line_cap;
            self.line_join = state.line_join;
            self.miter_limit = state.miter_limit;
            self.shadow_color = state.shadow_color;
            self.shadow_blur = state.shadow_blur;
            self.shadow_offset_x = state.shadow_offset_x;
            self.shadow_offset_y = state.shadow_offset_y;
            self.clip_mask = state.clip_mask;
            self.font = state.font;
            self.text_align = state.text_align;
            self.text_baseline = state.text_baseline;
        }
    }

    // ── Transforms ────────────────────────────────────────────────────────────

    /// `translate(tx, ty)` — apply a translation to the current CTM.
    pub fn translate(&mut self, tx: f32, ty: f32) {
        // Post-multiply: CTM = CTM × T(tx, ty)
        let [a, b, c, d, e, f] = self.ctm;
        self.ctm = [a, b, c, d, e + a * tx + c * ty, f + b * tx + d * ty];
    }

    /// `rotate(angle)` — rotate by `angle` radians clockwise around the origin.
    pub fn rotate(&mut self, angle: f32) {
        let cos = angle.cos();
        let sin = angle.sin();
        self.transform(cos, sin, -sin, cos, 0.0, 0.0);
    }

    /// `scale(sx, sy)` — apply a uniform or non-uniform scale.
    pub fn scale(&mut self, sx: f32, sy: f32) {
        let [a, b, c, d, e, f] = self.ctm;
        self.ctm = [a * sx, b * sx, c * sy, d * sy, e, f];
    }

    /// `transform(a, b, c, d, e, f)` — post-multiply the CTM by the given matrix.
    ///
    /// Matrix columns: `[a, b]` (x-axis), `[c, d]` (y-axis), `[e, f]` (translation).
    pub fn transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        let [ma, mb, mc, md, me, mf] = self.ctm;
        self.ctm = [
            ma * a + mc * b,
            mb * a + md * b,
            ma * c + mc * d,
            mb * c + md * d,
            ma * e + mc * f + me,
            mb * e + md * f + mf,
        ];
    }

    /// `setTransform(a, b, c, d, e, f)` — replace the CTM with the given matrix.
    pub fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.ctm = [a, b, c, d, e, f];
    }

    /// `resetTransform()` — reset the CTM to the identity matrix.
    pub fn reset_transform(&mut self) {
        self.ctm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    }

    /// Apply the current CTM to a user-space point.
    ///
    /// Returns device-pixel coordinates `(x', y')`.
    fn apply_ctm(&self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.ctm;
        (a * x + c * y + e, b * x + d * y + f)
    }

    // ── Rect operations ───────────────────────────────────────────────────────

    /// `clearRect(x, y, w, h)` — erase region to transparent black.
    ///
    /// Direct write (not source-over) — matches the spec's "copy" semantics.
    /// Ignores the current CTM (operates in device space per spec).
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
    ///
    /// Affected by the current CTM (translate, rotate, scale).
    /// Does not modify the current path (spec §4.12.5.1.9).
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let paint = self.fill_style.clone();
        let alpha = self.global_alpha;
        let path = self.build_rect_path(x, y, w, h);
        rasterize::fill_path(self, &path, &paint, alpha);
    }

    /// `strokeRect(x, y, w, h)` — stroke the outline of a rectangle.
    ///
    /// Affected by the current CTM. Does not modify the current path.
    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let paint = self.stroke_style.clone();
        let alpha = self.global_alpha;
        let lw = self.line_width;
        let path = self.build_rect_path(x, y, w, h);
        rasterize::stroke_path(self, &path, lw, &paint, alpha);
    }

    // ── Path API ──────────────────────────────────────────────────────────────

    /// `beginPath()` — discard current path.
    pub fn begin_path(&mut self) {
        self.path.clear();
        self.path_start = None;
    }

    /// `moveTo(x, y)` — start a new sub-path at user-space `(x, y)`.
    pub fn move_to(&mut self, x: f32, y: f32) {
        let (px, py) = self.apply_ctm(x, y);
        self.pen = (px, py);
        self.path_start = Some((px, py));
        self.path.push(PathSegment::Move(px, py));
    }

    /// `lineTo(x, y)` — add a line segment from pen to `(x, y)`.
    pub fn line_to(&mut self, x: f32, y: f32) {
        let (px, py) = self.apply_ctm(x, y);
        if self.path.is_empty() {
            self.path_start = Some((px, py));
            self.path.push(PathSegment::Move(px, py));
        } else {
            self.path.push(PathSegment::Line(self.pen.0, self.pen.1, px, py));
        }
        self.pen = (px, py);
    }

    /// `closePath()` — add a line back to the current sub-path start.
    pub fn close_path(&mut self) {
        if let Some((sx, sy)) = self.path_start {
            let (px, py) = self.pen;
            self.path.push(PathSegment::Line(px, py, sx, sy));
            self.pen = (sx, sy);
        }
    }

    /// `bezierCurveTo(cp1x, cp1y, cp2x, cp2y, x, y)` — cubic Bézier from pen.
    ///
    /// All coordinates are in user space and transformed by the current CTM.
    pub fn bezier_curve_to(
        &mut self,
        cp1x: f32, cp1y: f32,
        cp2x: f32, cp2y: f32,
        x: f32, y: f32,
    ) {
        let (x0, y0) = self.pen;
        let (c1x, c1y) = self.apply_ctm(cp1x, cp1y);
        let (c2x, c2y) = self.apply_ctm(cp2x, cp2y);
        let (ex, ey) = self.apply_ctm(x, y);
        if self.path.is_empty() {
            self.path_start = Some((x0, y0));
            self.path.push(PathSegment::Move(x0, y0));
        }
        self.path.push(PathSegment::Cubic(x0, y0, c1x, c1y, c2x, c2y, ex, ey));
        self.pen = (ex, ey);
    }

    /// `quadraticCurveTo(cpx, cpy, x, y)` — quadratic Bézier from pen.
    ///
    /// All coordinates are in user space and transformed by the current CTM.
    pub fn quadratic_curve_to(&mut self, cpx: f32, cpy: f32, x: f32, y: f32) {
        let (x0, y0) = self.pen;
        let (cx, cy) = self.apply_ctm(cpx, cpy);
        let (ex, ey) = self.apply_ctm(x, y);
        if self.path.is_empty() {
            self.path_start = Some((x0, y0));
            self.path.push(PathSegment::Move(x0, y0));
        }
        self.path.push(PathSegment::Quadratic(x0, y0, cx, cy, ex, ey));
        self.pen = (ex, ey);
    }

    /// `arc(cx, cy, r, startAngle, endAngle[, anticlockwise])` — add circular arc.
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

    /// `ellipse(cx, cy, rx, ry, rotation, startAngle, endAngle[, anticlockwise])`.
    ///
    /// Approximated by line segments; the CTM is applied to each generated point.
    #[allow(clippy::too_many_arguments)]
    pub fn ellipse(
        &mut self,
        cx: f32, cy: f32,
        rx: f32, ry: f32,
        rotation: f32,
        start_angle: f32, end_angle: f32,
        ccw: bool,
    ) {
        let angle_span = if ccw {
            let span = start_angle - end_angle;
            if span <= 0.0 { span + std::f32::consts::TAU } else { span }
        } else {
            let span = end_angle - start_angle;
            if span <= 0.0 { span + std::f32::consts::TAU } else { span }
        };

        let step_count = ((rx.max(ry) * angle_span).abs() as u32 + 4).clamp(4, 180);
        let cos_rot = rotation.cos();
        let sin_rot = rotation.sin();

        for i in 0..=step_count {
            let frac = i as f32 / step_count as f32;
            let t = if ccw {
                start_angle - angle_span * frac
            } else {
                start_angle + angle_span * frac
            };
            let lx = rx * t.cos();
            let ly = ry * t.sin();
            // Apply ellipse rotation
            let ex = cx + lx * cos_rot - ly * sin_rot;
            let ey = cy + lx * sin_rot + ly * cos_rot;
            if i == 0 {
                if self.path.is_empty() {
                    self.move_to(ex, ey);
                } else {
                    self.line_to(ex, ey);
                }
            } else {
                self.line_to(ex, ey);
            }
        }
    }

    /// `arcTo(x1, y1, x2, y2, radius)` — tangent arc between two lines.
    ///
    /// Approximated via circular arc geometry; CTM applied to all generated points.
    pub fn arc_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) {
        if radius <= 0.0 {
            self.line_to(x1, y1);
            return;
        }
        // Current pen in user space requires inverting CTM — for simplicity
        // we fall back to a straight line when CTM is non-identity.
        // For identity CTM this gives the correct tangent arc.
        let (x0, y0) = self.pen_user_space();
        let d1x = x0 - x1;
        let d1y = y0 - y1;
        let d2x = x2 - x1;
        let d2y = y2 - y1;
        let len1 = (d1x * d1x + d1y * d1y).sqrt();
        let len2 = (d2x * d2x + d2y * d2y).sqrt();
        if len1 < f32::EPSILON || len2 < f32::EPSILON {
            self.line_to(x1, y1);
            return;
        }
        // Angle between the two direction vectors
        let cos_angle = (d1x * d2x + d1y * d2y) / (len1 * len2);
        let angle = cos_angle.clamp(-1.0, 1.0).acos();
        let half = angle * 0.5;
        if half.sin().abs() < f32::EPSILON {
            self.line_to(x1, y1);
            return;
        }
        // Distance from corner to tangent points
        let tangent_dist = radius / half.tan();
        let t1 = tangent_dist / len1;
        let t2 = tangent_dist / len2;
        // Tangent points
        let tx1 = x1 + d1x * t1;
        let ty1 = y1 + d1y * t1;
        let tx2 = x1 + d2x * t2;
        let ty2 = y1 + d2y * t2;
        self.line_to(tx1, ty1);
        // Arc centre: perpendicular from tx1 toward the centre
        let n1x = -d1y / len1;
        let n1y =  d1x / len1;
        let cross = d1x * d2y - d1y * d2x;
        let sign = if cross > 0.0 { 1.0 } else { -1.0 };
        let cx = tx1 + n1x * radius * sign;
        let cy = ty1 + n1y * radius * sign;
        let start = (ty1 - cy).atan2(tx1 - cx);
        let end   = (ty2 - cy).atan2(tx2 - cx);
        self.arc(cx, cy, radius, start, end, cross < 0.0);
    }

    /// `rect(x, y, w, h)` — add a closed rectangle sub-path.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.move_to(x, y);
        self.line_to(x + w, y);
        self.line_to(x + w, y + h);
        self.line_to(x, y + h);
        self.close_path();
    }

    /// `fill()` — fill the current path with `fillStyle`.
    pub fn fill(&mut self) {
        let paint = self.fill_style.clone();
        let alpha = self.global_alpha;
        let shadow = self.shadow_effective();
        let path = self.path.clone();
        if let Some((sx, sy, sc)) = shadow {
            let shifted = shift_path(&path, sx, sy);
            rasterize::fill_path(self, &shifted, &PaintSource::Color(sc), alpha);
        }
        rasterize::fill_path(self, &path, &paint, alpha);
    }

    /// `stroke()` — stroke the current path with `strokeStyle`.
    pub fn stroke(&mut self) {
        let paint = self.stroke_style.clone();
        let alpha = self.global_alpha;
        let lw = self.line_width;
        let shadow = self.shadow_effective();
        let path = self.path.clone();
        if let Some((sx, sy, sc)) = shadow {
            let shifted = shift_path(&path, sx, sy);
            rasterize::stroke_path(self, &shifted, lw, &PaintSource::Color(sc), alpha);
        }
        rasterize::stroke_path(self, &path, lw, &paint, alpha);
    }

    /// Returns `Some((offset_x, offset_y, color))` when shadow should be drawn.
    fn shadow_effective(&self) -> Option<(f32, f32, CanvasColor)> {
        if self.shadow_color.a == 0 { return None; }
        if self.shadow_offset_x == 0.0 && self.shadow_offset_y == 0.0 && self.shadow_blur == 0.0 {
            return None;
        }
        Some((self.shadow_offset_x, self.shadow_offset_y, self.shadow_color))
    }

    // ── Phase 3: clip / image / text ─────────────────────────────────────────

    /// `clip()` — intersect the current clipping region with the current path (even-odd rule).
    ///
    /// After calling, subsequent drawing operations only affect pixels inside the clipped region.
    pub fn clip(&mut self) {
        let path = self.path.clone();
        let w = self.width;
        let h = self.height;
        let new_mask = rasterize::build_clip_mask(&path, w, h);
        self.clip_mask = Some(match self.clip_mask.take() {
            None => new_mask,
            Some(old) => old.iter().zip(new_mask.iter()).map(|(a, b)| *a && *b).collect(),
        });
    }

    // ── Phase 5: Path2D ─────────────────────────────────────────────────────

    /// `fill(path2d)` — fill a `Path2D` object using the current `fillStyle`.
    ///
    /// The path is converted to device space using the current CTM at the time of this call,
    /// per HTML LS §4.12.5.1.5 (CTM applied at use-time, not at path-creation time).
    pub fn fill_with_path2d(&mut self, path2d: &Path2dData) {
        let path = path2d.to_device_space(self.ctm);
        let paint = self.fill_style.clone();
        let alpha = self.global_alpha;
        let shadow = self.shadow_effective();
        if let Some((sx, sy, sc)) = shadow {
            let shifted = shift_path(&path, sx, sy);
            rasterize::fill_path(self, &shifted, &PaintSource::Color(sc), alpha);
        }
        rasterize::fill_path(self, &path, &paint, alpha);
    }

    /// `stroke(path2d)` — stroke a `Path2D` object using the current `strokeStyle`.
    ///
    /// CTM applied at use-time per HTML LS §4.12.5.1.5.
    pub fn stroke_with_path2d(&mut self, path2d: &Path2dData) {
        let path = path2d.to_device_space(self.ctm);
        let paint = self.stroke_style.clone();
        let alpha = self.global_alpha;
        let lw = self.line_width;
        let shadow = self.shadow_effective();
        if let Some((sx, sy, sc)) = shadow {
            let shifted = shift_path(&path, sx, sy);
            rasterize::stroke_path(self, &shifted, lw, &PaintSource::Color(sc), alpha);
        }
        rasterize::stroke_path(self, &path, lw, &paint, alpha);
    }

    /// `clip(path2d)` — intersect the clipping region with a `Path2D` object (even-odd rule).
    ///
    /// CTM applied at use-time per HTML LS §4.12.5.1.5.
    pub fn clip_with_path2d(&mut self, path2d: &Path2dData) {
        let path = path2d.to_device_space(self.ctm);
        let w = self.width;
        let h = self.height;
        let new_mask = rasterize::build_clip_mask(&path, w, h);
        self.clip_mask = Some(match self.clip_mask.take() {
            None => new_mask,
            Some(old) => old.iter().zip(new_mask.iter()).map(|(a, b)| *a && *b).collect(),
        });
    }

    /// `isPointInPath(path2d, x, y)` — test whether `(x, y)` lies inside a `Path2D`.
    ///
    /// Uses the even-odd rule.  Returns `false` for degenerate paths.
    /// This is a conservative stub: builds a 1×1 clip mask at pixel (x,y) and checks the bit.
    pub fn is_point_in_path2d(&self, path2d: &Path2dData, x: f32, y: f32) -> bool {
        let path = path2d.to_device_space(self.ctm);
        if path.is_empty() { return false; }
        let xi = x as u32;
        let yi = y as u32;
        if xi >= self.width || yi >= self.height { return false; }
        // Build a minimal mask just for the 1×1 region around (x,y).
        let mask = rasterize::build_clip_mask(&path, self.width, self.height);
        mask.get((yi * self.width + xi) as usize).copied().unwrap_or(false)
    }

    /// `drawImage(src_pixels, src_w, src_h, dx, dy, dw, dh)` — blit source image onto canvas.
    ///
    /// The source is scaled from `src_w × src_h` to `dw × dh` device pixels at offset `(dx, dy)`.
    /// The current CTM is applied to the destination rectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image(
        &mut self,
        src_pixels: &[u8],
        src_w: u32,
        src_h: u32,
        dx: f32,
        dy: f32,
        dw: f32,
        dh: f32,
    ) {
        if src_w == 0 || src_h == 0 || dw <= 0.0 || dh <= 0.0 { return; }
        let alpha = self.global_alpha;
        let p0 = self.apply_ctm(dx, dy);
        let p1 = self.apply_ctm(dx + dw, dy + dh);
        let x0 = p0.0.min(p1.0);
        let y0 = p0.1.min(p1.1);
        let x1 = p0.0.max(p1.0);
        let y1 = p0.1.max(p1.1);
        let dest_w = (x1 - x0).max(1.0);
        let dest_h = (y1 - y0).max(1.0);
        let xi0 = x0.floor() as i32;
        let yi0 = y0.floor() as i32;
        let xi1 = x1.ceil() as i32;
        let yi1 = y1.ceil() as i32;
        for dy_px in yi0..yi1 {
            for dx_px in xi0..xi1 {
                if dx_px < 0 || dy_px < 0 || dx_px >= self.width as i32 || dy_px >= self.height as i32 {
                    continue;
                }
                if !self.pixel_allowed(dx_px as u32, dy_px as u32) { continue; }
                let u = (dx_px as f32 - x0) / dest_w;
                let v = (dy_px as f32 - y0) / dest_h;
                let sx = (u * src_w as f32).clamp(0.0, (src_w - 1) as f32) as u32;
                let sy = (v * src_h as f32).clamp(0.0, (src_h - 1) as f32) as u32;
                let si = ((sy * src_w + sx) * 4) as usize;
                if si + 3 >= src_pixels.len() { continue; }
                let src_color = CanvasColor::rgba(
                    src_pixels[si],
                    src_pixels[si + 1],
                    src_pixels[si + 2],
                    ((src_pixels[si + 3] as f32) * alpha) as u8,
                );
                self.composite_pixel(dx_px as u32, dy_px as u32, src_color);
            }
        }
    }

    /// `putImageData(data, sw, sh, dx, dy)` — write RGBA8 pixel data directly to canvas.
    ///
    /// Bypasses CTM, globalAlpha, compositing mode, and clipping (spec §4.12.5.1.16).
    pub fn put_image_data(&mut self, data: &[u8], sw: u32, sh: u32, dx: i32, dy: i32) {
        for row in 0..sh {
            for col in 0..sw {
                let dest_x = dx + col as i32;
                let dest_y = dy + row as i32;
                if dest_x < 0 || dest_y < 0
                    || dest_x >= self.width as i32
                    || dest_y >= self.height as i32
                {
                    continue;
                }
                let si = ((row * sw + col) * 4) as usize;
                if si + 3 >= data.len() { continue; }
                let di = ((dest_y as u32 * self.width + dest_x as u32) * 4) as usize;
                self.pixels[di]     = data[si];
                self.pixels[di + 1] = data[si + 1];
                self.pixels[di + 2] = data[si + 2];
                self.pixels[di + 3] = data[si + 3];
            }
        }
    }

    /// `createImageData(sw, sh)` — return a zero-filled RGBA8 buffer of `sw × sh` pixels.
    pub fn create_image_data(sw: u32, sh: u32) -> Vec<u8> {
        vec![0u8; (sw * sh * 4) as usize]
    }

    /// Draw pre-rasterized glyph bitmaps at text position.
    ///
    /// `glyphs` is a list of `(x_offset, baseline_y, glyph_w, glyph_h, coverage, color)` where
    /// `coverage` is a grayscale bitmap (one byte per pixel, 0=transparent, 255=opaque).
    /// The colour `color` is multiplied by coverage to get the final RGBA.
    #[allow(clippy::type_complexity)]
    pub fn fill_text_glyphs(
        &mut self,
        glyphs: &[(f32, f32, u32, u32, &[u8], CanvasColor)],
    ) {
        let alpha = self.global_alpha;
        for &(gx, gy, gw, gh, coverage, color) in glyphs {
            let p0 = self.apply_ctm(gx, gy);
            for row in 0..gh {
                for col in 0..gw {
                    let cx_dev = (p0.0 + col as f32) as i32;
                    let cy_dev = (p0.1 + row as f32) as i32;
                    if cx_dev < 0 || cy_dev < 0
                        || cx_dev >= self.width as i32
                        || cy_dev >= self.height as i32
                    {
                        continue;
                    }
                    let ux = cx_dev as u32;
                    let uy = cy_dev as u32;
                    if !self.pixel_allowed(ux, uy) { continue; }
                    let ci = (row * gw + col) as usize;
                    if ci >= coverage.len() { continue; }
                    let cov = coverage[ci] as f32 / 255.0;
                    if cov < f32::EPSILON { continue; }
                    let final_alpha = ((color.a as f32 / 255.0) * cov * alpha * 255.0) as u8;
                    let c = CanvasColor::rgba(color.r, color.g, color.b, final_alpha);
                    self.composite_pixel(ux, uy, c);
                }
            }
        }
    }

    /// Returns `true` when pixel `(x, y)` is within the current clipping region.
    pub(crate) fn pixel_allowed(&self, x: u32, y: u32) -> bool {
        if let Some(mask) = &self.clip_mask {
            let idx = (y * self.width + x) as usize;
            idx < mask.len() && mask[idx]
        } else {
            true
        }
    }

    // ── Internal path helpers ─────────────────────────────────────────────────

    /// Build a closed rectangle path through the current CTM without touching `self.path`.
    fn build_rect_path(&self, x: f32, y: f32, w: f32, h: f32) -> Vec<PathSegment> {
        let p0 = self.apply_ctm(x,     y);
        let p1 = self.apply_ctm(x + w, y);
        let p2 = self.apply_ctm(x + w, y + h);
        let p3 = self.apply_ctm(x,     y + h);
        vec![
            PathSegment::Move(p0.0, p0.1),
            PathSegment::Line(p0.0, p0.1, p1.0, p1.1),
            PathSegment::Line(p1.0, p1.1, p2.0, p2.1),
            PathSegment::Line(p2.0, p2.1, p3.0, p3.1),
            PathSegment::Line(p3.0, p3.1, p0.0, p0.1),
        ]
    }

    // ── Low-level helpers ─────────────────────────────────────────────────────

    /// Alpha-composite `color` over the pixel at `(x, y)` using `composite_operation`.
    #[allow(dead_code)]
    pub(crate) fn set_pixel(&mut self, x: u32, y: u32, color: CanvasColor) {
        self.composite_pixel(x, y, color);
    }

    /// Apply `composite_operation` to blend `src` colour into pixel `(x, y)`.
    fn composite_pixel(&mut self, x: u32, y: u32, src: CanvasColor) {
        if x >= self.width || y >= self.height { return; }
        let idx = ((y * self.width + x) * 4) as usize;
        let dst = &mut self.pixels[idx..idx + 4];

        let sr = src.r as f32 / 255.0;
        let sg = src.g as f32 / 255.0;
        let sb = src.b as f32 / 255.0;
        let sa = src.a as f32 / 255.0;

        let dr = dst[0] as f32 / 255.0;
        let dg = dst[1] as f32 / 255.0;
        let db = dst[2] as f32 / 255.0;
        let da = dst[3] as f32 / 255.0;

        let (r, g, b, a) = match self.composite_operation {
            CompositeOperation::SourceOver => {
                let oa = sa + da * (1.0 - sa);
                if oa < f32::EPSILON { (0.0, 0.0, 0.0, 0.0) } else {
                    let k = da * (1.0 - sa);
                    ((sr * sa + dr * k) / oa,
                     (sg * sa + dg * k) / oa,
                     (sb * sa + db * k) / oa,
                     oa)
                }
            }
            CompositeOperation::SourceIn => {
                let oa = sa * da;
                (sr, sg, sb, oa)
            }
            CompositeOperation::SourceOut => {
                let oa = sa * (1.0 - da);
                (sr, sg, sb, oa)
            }
            CompositeOperation::SourceAtop => {
                let oa = da;
                let k = 1.0 - sa;
                ((sr * sa + dr * k) / oa.max(f32::EPSILON),
                 (sg * sa + dg * k) / oa.max(f32::EPSILON),
                 (sb * sa + db * k) / oa.max(f32::EPSILON),
                 oa)
            }
            CompositeOperation::DestinationOver => {
                let oa = da + sa * (1.0 - da);
                if oa < f32::EPSILON { (0.0, 0.0, 0.0, 0.0) } else {
                    let k = sa * (1.0 - da);
                    ((dr * da + sr * k) / oa,
                     (dg * da + sg * k) / oa,
                     (db * da + sb * k) / oa,
                     oa)
                }
            }
            CompositeOperation::DestinationIn => {
                let oa = da * sa;
                (dr, dg, db, oa)
            }
            CompositeOperation::DestinationOut => {
                let oa = da * (1.0 - sa);
                (dr, dg, db, oa)
            }
            CompositeOperation::DestinationAtop => {
                let oa = sa;
                let k = 1.0 - da;
                ((dr * da + sr * k) / oa.max(f32::EPSILON),
                 (dg * da + sg * k) / oa.max(f32::EPSILON),
                 (db * da + sb * k) / oa.max(f32::EPSILON),
                 oa)
            }
            CompositeOperation::Xor => {
                let oa = sa * (1.0 - da) + da * (1.0 - sa);
                if oa < f32::EPSILON { (0.0, 0.0, 0.0, 0.0) } else {
                    ((sr * sa * (1.0 - da) + dr * da * (1.0 - sa)) / oa,
                     (sg * sa * (1.0 - da) + dg * da * (1.0 - sa)) / oa,
                     (sb * sa * (1.0 - da) + db * da * (1.0 - sa)) / oa,
                     oa)
                }
            }
            CompositeOperation::Copy => {
                (sr, sg, sb, sa)
            }
            CompositeOperation::Lighter => {
                let oa = (sa + da).min(1.0);
                let r = (sr * sa + dr * da).min(1.0);
                let g = (sg * sa + dg * da).min(1.0);
                let b = (sb * sa + db * da).min(1.0);
                (r, g, b, oa)
            }
            CompositeOperation::Multiply => {
                let oa = sa + da - sa * da;
                if oa < f32::EPSILON { (0.0, 0.0, 0.0, 0.0) } else {
                    let blend_r = sr * dr;
                    let blend_g = sg * dg;
                    let blend_b = sb * db;
                    let r = (blend_r * sa * da + sr * sa * (1.0 - da) + dr * da * (1.0 - sa)) / oa;
                    let g = (blend_g * sa * da + sg * sa * (1.0 - da) + dg * da * (1.0 - sa)) / oa;
                    let b = (blend_b * sa * da + sb * sa * (1.0 - da) + db * da * (1.0 - sa)) / oa;
                    (r, g, b, oa)
                }
            }
            CompositeOperation::Screen => {
                let oa = sa + da - sa * da;
                if oa < f32::EPSILON { (0.0, 0.0, 0.0, 0.0) } else {
                    let blend_r = 1.0 - (1.0 - sr) * (1.0 - dr);
                    let blend_g = 1.0 - (1.0 - sg) * (1.0 - dg);
                    let blend_b = 1.0 - (1.0 - sb) * (1.0 - db);
                    let r = (blend_r * sa * da + sr * sa * (1.0 - da) + dr * da * (1.0 - sa)) / oa;
                    let g = (blend_g * sa * da + sg * sa * (1.0 - da) + dg * da * (1.0 - sa)) / oa;
                    let b = (blend_b * sa * da + sb * sa * (1.0 - da) + db * da * (1.0 - sa)) / oa;
                    (r, g, b, oa)
                }
            }
            CompositeOperation::Overlay => {
                let oa = sa + da - sa * da;
                if oa < f32::EPSILON { (0.0, 0.0, 0.0, 0.0) } else {
                    let blend = |s: f32, d: f32| -> f32 {
                        if 2.0 * d < 1.0 { 2.0 * s * d } else { 1.0 - 2.0 * (1.0 - s) * (1.0 - d) }
                    };
                    let blend_r = blend(sr, dr);
                    let blend_g = blend(sg, dg);
                    let blend_b = blend(sb, db);
                    let r = (blend_r * sa * da + sr * sa * (1.0 - da) + dr * da * (1.0 - sa)) / oa;
                    let g = (blend_g * sa * da + sg * sa * (1.0 - da) + dg * da * (1.0 - sa)) / oa;
                    let b = (blend_b * sa * da + sb * sa * (1.0 - da) + db * da * (1.0 - sa)) / oa;
                    (r, g, b, oa)
                }
            }
            CompositeOperation::Darken => {
                let oa = sa + da - sa * da;
                if oa < f32::EPSILON { (0.0, 0.0, 0.0, 0.0) } else {
                    let blend_r = sr * da.min(dr * sa);
                    let blend_g = sg * da.min(dg * sa);
                    let blend_b = sb * da.min(db * sa);
                    let r = (blend_r + sr * sa * (1.0 - da) + dr * da * (1.0 - sa)) / oa;
                    let g = (blend_g + sg * sa * (1.0 - da) + dg * da * (1.0 - sa)) / oa;
                    let b = (blend_b + sb * sa * (1.0 - da) + db * da * (1.0 - sa)) / oa;
                    (r, g, b, oa)
                }
            }
            CompositeOperation::Lighten => {
                let oa = sa + da - sa * da;
                if oa < f32::EPSILON { (0.0, 0.0, 0.0, 0.0) } else {
                    let blend_r = sr * da.max(dr * sa);
                    let blend_g = sg * da.max(dg * sa);
                    let blend_b = sb * da.max(db * sa);
                    let r = (blend_r + sr * sa * (1.0 - da) + dr * da * (1.0 - sa)) / oa;
                    let g = (blend_g + sg * sa * (1.0 - da) + dg * da * (1.0 - sa)) / oa;
                    let b = (blend_b + sb * sa * (1.0 - da) + db * da * (1.0 - sa)) / oa;
                    (r, g, b, oa)
                }
            }
        };

        dst[0] = (r.clamp(0.0, 1.0) * 255.0) as u8;
        dst[1] = (g.clamp(0.0, 1.0) * 255.0) as u8;
        dst[2] = (b.clamp(0.0, 1.0) * 255.0) as u8;
        dst[3] = (a.clamp(0.0, 1.0) * 255.0) as u8;
    }

    /// Current pen position in user space (inverse CTM of device pen).
    ///
    /// Used only by `arc_to` which needs the user-space pen to compute geometry.
    fn pen_user_space(&self) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.ctm;
        let det = a * d - b * c;
        if det.abs() < f32::EPSILON {
            return self.pen; // degenerate CTM — return device coords as fallback
        }
        let (px, py) = self.pen;
        let ix = px - e;
        let iy = py - f;
        (( d * ix - c * iy) / det,
         (-b * ix + a * iy) / det)
    }
}

// ── Free helpers ─────────────────────────────────────────────────────────────

/// Return a copy of `path` with every coordinate shifted by `(dx, dy)`.
///
/// Used by shadow rendering: the shadow is drawn as the same path rendered in the
/// shadow colour at a pixel offset before the actual fill/stroke.
fn shift_path(path: &[PathSegment], dx: f32, dy: f32) -> Vec<PathSegment> {
    path.iter().map(|seg| match *seg {
        PathSegment::Move(x, y) => PathSegment::Move(x + dx, y + dy),
        PathSegment::Line(x0, y0, x1, y1) => PathSegment::Line(x0 + dx, y0 + dy, x1 + dx, y1 + dy),
        PathSegment::Cubic(x0, y0, c1x, c1y, c2x, c2y, x1, y1) =>
            PathSegment::Cubic(x0 + dx, y0 + dy, c1x + dx, c1y + dy, c2x + dx, c2y + dy, x1 + dx, y1 + dy),
        PathSegment::Quadratic(x0, y0, cx, cy, x1, y1) =>
            PathSegment::Quadratic(x0 + dx, y0 + dy, cx + dx, cy + dy, x1 + dx, y1 + dy),
    }).collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

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
        ctx.fill_style = CanvasColor::rgba(255, 0, 0, 255).into();
        ctx.fill_rect(0.0, 0.0, 10.0, 10.0);
        let p = ctx.pixels();
        assert_eq!(p[0], 255);
        assert_eq!(p[1], 0);
        assert_eq!(p[2], 0);
        assert_eq!(p[3], 255);
    }

    #[test]
    fn clear_rect_erases() {
        let mut ctx = Context2D::new(4, 4);
        ctx.fill_style = CanvasColor::rgba(255, 0, 0, 255).into();
        ctx.fill_rect(0.0, 0.0, 4.0, 4.0);
        ctx.clear_rect(0.0, 0.0, 4.0, 4.0);
        assert!(ctx.pixels().iter().all(|&b| b == 0));
    }

    #[test]
    fn fill_rect_clips_to_bounds() {
        let mut ctx = Context2D::new(4, 4);
        ctx.fill_style = CanvasColor::rgba(0, 255, 0, 255).into();
        ctx.fill_rect(-2.0, -2.0, 10.0, 10.0);
        let p = ctx.pixels();
        assert_eq!(p[1], 255);
    }

    #[test]
    fn stroke_rect_draws_border() {
        let mut ctx = Context2D::new(10, 10);
        ctx.stroke_style = CanvasColor::rgba(0, 0, 255, 255).into();
        ctx.line_width = 1.0;
        ctx.stroke_rect(0.0, 0.0, 10.0, 10.0);
        let p = ctx.pixels();
        assert_eq!(p[2], 255);
    }

    #[test]
    fn path_fill_triangle() {
        let mut ctx = Context2D::new(20, 20);
        ctx.fill_style = CanvasColor::rgba(255, 255, 0, 255).into();
        ctx.begin_path();
        ctx.move_to(10.0, 0.0);
        ctx.line_to(20.0, 20.0);
        ctx.line_to(0.0, 20.0);
        ctx.close_path();
        ctx.fill();
        let idx = (19 * 20) * 4;
        let p = ctx.pixels();
        assert_eq!(p[idx], 255);
        assert_eq!(p[idx + 1], 255);
    }

    #[test]
    fn global_alpha_applied() {
        let mut ctx = Context2D::new(4, 4);
        ctx.fill_style = CanvasColor::rgba(255, 0, 0, 255).into();
        ctx.global_alpha = 0.5;
        ctx.fill_rect(0.0, 0.0, 4.0, 4.0);
        let p = ctx.pixels();
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
        ctx.fill_style = CanvasColor::rgba(255, 0, 0, 255).into();
        ctx.fill_rect(0.0, 0.0, 4.0, 4.0);
        ctx.resize(8, 8);
        assert!(ctx.pixels().iter().all(|&b| b == 0));
        assert_eq!(ctx.width(), 8);
        assert_eq!(ctx.height(), 8);
    }

    #[test]
    fn get_image_data_without_noise() {
        let mut ctx = Context2D::new(4, 4);
        ctx.fill_style = CanvasColor::rgba(100, 150, 200, 255).into();
        ctx.fill_rect(0.0, 0.0, 4.0, 4.0);
        let data = ctx.get_image_data();
        assert_eq!(data, ctx.pixels());
    }

    #[test]
    fn get_image_data_with_noise() {
        let mut ctx = Context2D::new(4, 4);
        ctx.fill_style = CanvasColor::rgba(100, 150, 200, 255).into();
        ctx.fill_rect(0.0, 0.0, 4.0, 4.0);
        ctx.set_noise_generator(CanvasNoiseGenerator::new(42));
        let data = ctx.get_image_data();
        assert_ne!(data, ctx.pixels());
        for i in 0..4 {
            assert_eq!(data[i * 4 + 3], 255, "pixel {} alpha must be unchanged", i);
        }
    }

    #[test]
    fn get_image_data_noise_deterministic() {
        let mut ctx1 = Context2D::new(2, 2);
        ctx1.fill_style = CanvasColor::rgba(100, 150, 200, 255).into();
        ctx1.fill_rect(0.0, 0.0, 2.0, 2.0);
        ctx1.set_noise_generator(CanvasNoiseGenerator::new(42));

        let mut ctx2 = Context2D::new(2, 2);
        ctx2.fill_style = CanvasColor::rgba(100, 150, 200, 255).into();
        ctx2.fill_rect(0.0, 0.0, 2.0, 2.0);
        ctx2.set_noise_generator(CanvasNoiseGenerator::new(42));

        assert_eq!(ctx1.get_image_data(), ctx2.get_image_data());
    }

    // ── Phase 2: new tests ────────────────────────────────────────────────────

    #[test]
    fn save_restore_preserves_state() {
        let mut ctx = Context2D::new(4, 4);
        ctx.fill_style = CanvasColor::rgba(255, 0, 0, 255).into();
        ctx.global_alpha = 0.5;
        ctx.save();
        ctx.fill_style = CanvasColor::rgba(0, 255, 0, 255).into();
        ctx.global_alpha = 1.0;
        ctx.restore();
        let c = ctx.fill_style.as_color_or_black();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert!((ctx.global_alpha - 0.5).abs() < 0.001);
    }

    #[test]
    fn save_restore_does_not_save_path() {
        let mut ctx = Context2D::new(10, 10);
        ctx.begin_path();
        ctx.move_to(0.0, 0.0);
        ctx.save();
        ctx.begin_path();
        ctx.restore();
        ctx.fill(); // should not panic — path is empty after begin_path
    }

    #[test]
    fn restore_empty_stack_is_noop() {
        let mut ctx = Context2D::new(4, 4);
        ctx.restore(); // should not panic
    }

    #[test]
    fn translate_moves_drawing() {
        let mut ctx = Context2D::new(20, 20);
        ctx.fill_style = CanvasColor::rgba(255, 0, 0, 255).into();
        ctx.translate(10.0, 0.0);
        ctx.fill_rect(0.0, 0.0, 5.0, 5.0);
        let p = ctx.pixels();
        assert_eq!(p[3], 0, "origin pixel should be transparent after translate");
        let idx = 10 * 4;
        assert_eq!(p[idx], 255, "translated pixel should be red");
    }

    #[test]
    fn scale_expands_drawing() {
        let mut ctx = Context2D::new(20, 20);
        ctx.fill_style = CanvasColor::rgba(0, 255, 0, 255).into();
        ctx.scale(2.0, 2.0);
        ctx.fill_rect(0.0, 0.0, 5.0, 5.0);
        let p = ctx.pixels();
        let idx = (9 * 20 + 9) * 4;
        assert_eq!(p[idx + 1], 255, "scaled pixel (9,9) should be green");
    }

    #[test]
    fn ctm_identity_after_reset() {
        let mut ctx = Context2D::new(4, 4);
        ctx.translate(5.0, 5.0);
        ctx.reset_transform();
        assert_eq!(ctx.ctm, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn bezier_curve_to_produces_segments() {
        let mut ctx = Context2D::new(100, 100);
        ctx.begin_path();
        ctx.move_to(10.0, 50.0);
        ctx.bezier_curve_to(10.0, 10.0, 90.0, 10.0, 90.0, 50.0);
        // Should have Move + 1 Cubic segment
        assert_eq!(ctx.path.len(), 2);
        assert!(matches!(ctx.path[1], PathSegment::Cubic(..)));
    }

    #[test]
    fn quadratic_curve_to_produces_segment() {
        let mut ctx = Context2D::new(100, 100);
        ctx.begin_path();
        ctx.move_to(10.0, 50.0);
        ctx.quadratic_curve_to(50.0, 10.0, 90.0, 50.0);
        assert_eq!(ctx.path.len(), 2);
        assert!(matches!(ctx.path[1], PathSegment::Quadratic(..)));
    }

    #[test]
    fn rect_path_is_closed() {
        let mut ctx = Context2D::new(20, 20);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 10.0, 10.0);
        ctx.fill_style = CanvasColor::rgba(255, 0, 0, 255).into();
        ctx.fill();
        let idx = (5 * 20 + 5) * 4;
        let p = ctx.pixels();
        assert_eq!(p[idx], 255);
    }

    #[test]
    fn composite_source_over_is_default() {
        let ctx = Context2D::new(1, 1);
        assert_eq!(ctx.composite_operation, CompositeOperation::SourceOver);
    }

    #[test]
    fn composite_copy_replaces_pixel() {
        let mut ctx = Context2D::new(4, 4);
        ctx.fill_style = CanvasColor::rgba(255, 0, 0, 255).into();
        ctx.fill_rect(0.0, 0.0, 4.0, 4.0);
        ctx.composite_operation = CompositeOperation::Copy;
        ctx.fill_style = CanvasColor::rgba(0, 0, 255, 128).into();
        ctx.fill_rect(0.0, 0.0, 4.0, 4.0);
        let p = ctx.pixels();
        assert_eq!(p[2], 255); // B
        assert_eq!(p[3], 128); // A
    }

    #[test]
    fn composite_operation_parse() {
        assert_eq!(CompositeOperation::from_str("source-over"), Some(CompositeOperation::SourceOver));
        assert_eq!(CompositeOperation::from_str("multiply"), Some(CompositeOperation::Multiply));
        assert_eq!(CompositeOperation::from_str("unknown"), None);
    }

    #[test]
    fn line_cap_parse() {
        assert_eq!(LineCap::from_str("butt"), Some(LineCap::Butt));
        assert_eq!(LineCap::from_str("round"), Some(LineCap::Round));
        assert_eq!(LineCap::from_str("square"), Some(LineCap::Square));
    }

    #[test]
    fn line_join_parse() {
        assert_eq!(LineJoin::from_str("miter"), Some(LineJoin::Miter));
        assert_eq!(LineJoin::from_str("round"), Some(LineJoin::Round));
        assert_eq!(LineJoin::from_str("bevel"), Some(LineJoin::Bevel));
    }

    #[test]
    fn ellipse_produces_path() {
        let mut ctx = Context2D::new(100, 100);
        ctx.begin_path();
        ctx.ellipse(50.0, 50.0, 30.0, 20.0, 0.0, 0.0, std::f32::consts::TAU, false);
        assert!(!ctx.path.is_empty());
        ctx.fill_style = CanvasColor::rgba(0, 128, 255, 255).into();
        ctx.fill(); // should not panic
    }

    #[test]
    fn color_space_defaults_to_srgb() {
        let ctx = Context2D::new(100, 100);
        assert_eq!(ctx.color_space(), ColorSpace::Srgb);
    }

    #[test]
    fn color_space_can_be_set() {
        let mut ctx = Context2D::new(100, 100);
        ctx.set_color_space(ColorSpace::DisplayP3);
        assert_eq!(ctx.color_space(), ColorSpace::DisplayP3);
        ctx.set_color_space(ColorSpace::Rec2020);
        assert_eq!(ctx.color_space(), ColorSpace::Rec2020);
    }

    #[test]
    fn color_space_preserved_in_from_pixels() {
        let pixels = vec![255, 0, 0, 255];
        let mut ctx = Context2D::from_pixels(1, 1, pixels);
        assert_eq!(ctx.color_space(), ColorSpace::Srgb);
        ctx.set_color_space(ColorSpace::DisplayP3);
        assert_eq!(ctx.color_space(), ColorSpace::DisplayP3);
    }
}
