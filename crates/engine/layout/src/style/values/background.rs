//! Градиенты (`ParsedGradient`/`GradientCorner`/`RadialShape`/
//! `RadialSize`), фон (`background-image`/`-repeat`/`-size`/
//! `-attachment`/`-origin`/`-clip`), маска (`mask-clip`,
//! `BackgroundLayer`), `object-fit`, `image-rendering`.
//!
//! Перенесено батчем SPLIT-ST17 из `crates/engine/layout/src/style.rs`
//! (анкер `enum ParsedGradient` до конца `impl ImageRendering`) без правок тел.

use serde::{Deserialize, Serialize};

use crate::style::values::box_model::MixBlendMode;
use crate::style::values::flexgrid::{ObjectPosition, PositionComponent};
use crate::style::values::transform::GradientStop;

/// CSS Images L3/L4 §3.3/§3.7 — parsed linear / radial / conic gradient.
///
/// Stored instead of the raw CSS string once `parse_background_gradient`
/// has tokenised the gradient function. `Unknown` is kept as fallback for
/// future / malformed variants so they round-trip without information loss.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedGradient {
    /// `linear-gradient(angle, stop, ...)` — angle in CSS degrees measured
    /// clockwise from "to top" (0° = top, 90° = right, 180° = bottom).
    Linear {
        /// Gradient line angle in CSS degrees (0° = to top, 90° = to right).
        /// For a `to <corner>` keyword this is only the square-box (45/135/
        /// 225/315°) placeholder — `corner` carries the keyword so a paint-time
        /// consumer that knows the actual box size can resolve the true,
        /// aspect-ratio-dependent angle via [`GradientCorner::angle_deg`].
        angle_deg: f32,
        /// `Some` when the direction was written as `to <corner>` (CSS Images
        /// L3 §3.1) rather than an explicit `<angle>` — the true gradient-line
        /// angle for a corner keyword depends on the gradient box's aspect
        /// ratio, which is not known at style-parse time.
        corner: Option<GradientCorner>,
        stops: Vec<GradientStop>,
        /// True when the original function was `repeating-linear-gradient`.
        repeating: bool,
    },
    /// `radial-gradient(...)` — radial gradient centred at `(cx, cy)`.
    Radial {
        /// Centre as fraction of box width/height ([0, 1] = [left/top, right/bottom]).
        center_x_pct: f32,
        center_y_pct: f32,
        /// Ending shape — `circle` or `ellipse` (CSS Images L3 §3.5). The radii
        /// are resolved against the box at paint time via [`radial_gradient_radii`].
        shape: RadialShape,
        /// Sizing keyword for the ending shape (default `farthest-corner`).
        size: RadialSize,
        stops: Vec<GradientStop>,
        /// True when the original function was `repeating-radial-gradient`.
        repeating: bool,
    },
    /// CSS Images L4 §3.7 — `conic-gradient([from <angle>]? [at <pos>]?, <stops>)`.
    /// Angular gradient revolving around `(center_x_pct, center_y_pct)` (fraction of
    /// box width/height). `from_angle_deg` is the starting angle in CSS degrees
    /// (0° = top, 90° = right), clockwise. Stops' positions are stored as
    /// `Length::Percent` where 100% corresponds to a full revolution
    /// (angle units `<angle>` are pre-converted to percent on parse).
    Conic {
        center_x_pct: f32,
        center_y_pct: f32,
        /// Starting angle in CSS degrees (0° = top, 90° = right, clockwise).
        from_angle_deg: f32,
        stops: Vec<GradientStop>,
        /// True when the original function was `repeating-conic-gradient`.
        repeating: bool,
    },
    /// Fallback for any future gradient variant not yet rendered.
    Unknown(String),
}

/// CSS Images L3 §3.1 — `to <corner>` keyword of a `linear-gradient`'s
/// direction. Unlike the four side keywords (`to top`/`to right`/…), a corner
/// keyword's true gradient-line angle depends on the gradient box's aspect
/// ratio: the line is defined to pass exactly through the two opposite
/// corners, so on a non-square box it tilts away from the naive 45°
/// diagonal toward whichever side the box is longer along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradientCorner {
    /// `to top right` / `to right top`.
    TopRight,
    /// `to bottom right` / `to right bottom`.
    BottomRight,
    /// `to bottom left` / `to left bottom`.
    BottomLeft,
    /// `to top left` / `to left top`.
    TopLeft,
    /// Легаси `-webkit-*-gradient` (`bottom left` → …): линия градиента идёт
    /// вдоль диагонали бокса от начального угла к этому углу, а не
    /// перпендикулярно «магической» диагонали (угол `90 - atan2(h, w)`).
    DiagTopRight,
    /// То же к нижнему правому углу (`90 + atan2(h, w)`).
    DiagBottomRight,
    /// То же к нижнему левому углу (`270 - atan2(h, w)`).
    DiagBottomLeft,
    /// То же к верхнему левому углу (`270 + atan2(h, w)`).
    DiagTopLeft,
}

impl GradientCorner {
    /// Resolves the keyword to a true gradient-line angle (CSS degrees,
    /// 0° = to top, clockwise) for a box of the given size.
    ///
    /// Per CSS Images L3 §3.1 the gradient line is defined to be
    /// *perpendicular* to the diagonal connecting the two corners the
    /// keyword does *not* name — e.g. for `to bottom right` that diagonal
    /// runs between the top-right and bottom-left corners, direction
    /// `(-width, height)`, so the gradient line itself runs along
    /// `(height, width)`. That makes the base angle `atan2(height, width)`
    /// (note: height first), not `atan2(width, height)` — on a box much
    /// wider than it is tall this angle is *small* (the line tilts toward
    /// vertical, "to bottom"/"to top"), which is the opposite of the naive
    /// "tilts toward the long axis" guess. Verified against a real Edge
    /// render of a 960×160 box: predicted 170.5°, measured ~170.5°.
    /// Reduces to the familiar 45/135/225/315° only when `width == height`.
    pub fn angle_deg(self, width: f32, height: f32) -> f32 {
        let base = height.max(0.0).atan2(width.max(0.0)).to_degrees();
        match self {
            GradientCorner::TopRight => base,
            GradientCorner::BottomRight => 180.0 - base,
            GradientCorner::BottomLeft => 180.0 + base,
            GradientCorner::TopLeft => 360.0 - base,
            GradientCorner::DiagTopRight => 90.0 - base,
            GradientCorner::DiagBottomRight => 90.0 + base,
            GradientCorner::DiagBottomLeft => 270.0 - base,
            GradientCorner::DiagTopLeft => 270.0 + base,
        }
    }
}

/// CSS Images L3 §3.5 — ending-shape of a `radial-gradient`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialShape {
    /// `circle` — isotropic, a single radius along every direction.
    Circle,
    /// `ellipse` (also the default when no shape keyword is given) — independent
    /// horizontal and vertical radii.
    Ellipse,
}

/// CSS Images L3 §3.5 — sizing keyword controlling the radii of a
/// `radial-gradient`'s ending shape. Explicit `<length>` radii are not yet
/// modelled; they fall back to [`RadialSize::FarthestCorner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialSize {
    /// Ending shape meets the side(s) nearest the centre.
    ClosestSide,
    /// Ending shape passes through the corner nearest the centre.
    ClosestCorner,
    /// Ending shape meets the side(s) farthest from the centre.
    FarthestSide,
    /// Ending shape passes through the corner farthest from the centre (default).
    FarthestCorner,
}

/// CSS Images L3 §3.5.1 — resolves a `radial-gradient` ending shape to concrete
/// `(radius_x, radius_y)` in CSS px for a box `w×h` with centre at
/// `(cx_pct·w, cy_pct·h)`. For [`RadialShape::Circle`] both radii are equal.
/// Corner sizes use the aspect ratio of the matching side size and scale the
/// ellipse to pass through the chosen corner (CSS Images L3 §3.5.1, last list
/// item). Radii are clamped to ≥ 1 px to avoid a degenerate gradient.
#[must_use]
pub fn radial_gradient_radii(
    shape: RadialShape, size: RadialSize, cx_pct: f32, cy_pct: f32, w: f32, h: f32,
) -> (f32, f32) {
    let cx = cx_pct * w;
    let cy = cy_pct * h;
    let near_x = cx.abs().min((w - cx).abs());
    let far_x = cx.abs().max((w - cx).abs());
    let near_y = cy.abs().min((h - cy).abs());
    let far_y = cy.abs().max((h - cy).abs());
    // Ellipse with aspect ratio `sx:sy` scaled to pass through corner (cdx, cdy).
    let through_corner = |sx: f32, sy: f32, cdx: f32, cdy: f32| -> (f32, f32) {
        let a = (sx / sy.max(1e-6)).max(1e-6); // rx / ry
        let ry = ((cdx / a).powi(2) + cdy * cdy).sqrt().max(1.0);
        ((a * ry).max(1.0), ry)
    };
    match shape {
        RadialShape::Circle => {
            let r = match size {
                RadialSize::ClosestSide => near_x.min(near_y),
                RadialSize::FarthestSide => far_x.max(far_y),
                RadialSize::ClosestCorner => near_x.hypot(near_y),
                RadialSize::FarthestCorner => far_x.hypot(far_y),
            }
            .max(1.0);
            (r, r)
        }
        RadialShape::Ellipse => match size {
            RadialSize::ClosestSide => (near_x.max(1.0), near_y.max(1.0)),
            RadialSize::FarthestSide => (far_x.max(1.0), far_y.max(1.0)),
            RadialSize::ClosestCorner => through_corner(near_x, near_y, near_x, near_y),
            RadialSize::FarthestCorner => through_corner(far_x, far_y, far_x, far_y),
        },
    }
}

/// CSS Backgrounds L3 §3.1 / CSS Images L4 §4 — `background-image` value.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum BackgroundImage {
    #[default]
    None,
    /// `url("path")` or raw `image-set(…)` / `-webkit-image-set(…)` string.
    ///
    /// `image-set()` strings are stored verbatim — paint resolves them to the
    /// best URL for the current DPR via `select_image_set_url` (CSS Images L4 §5).
    Url(String),
    /// Parsed gradient. Phase 0 renders linear / radial / conic.
    Gradient(ParsedGradient),
    /// CSS Images L4 §4 — `cross-fade(<image-a>, <image-b>, <percentage>)`.
    ///
    /// `t` is the blend factor in `[0.0, 1.0]`: `0.0` = fully `a`, `1.0` = fully `b`.
    CrossFade {
        /// First image (`t = 0.0`).
        a: Box<BackgroundImage>,
        /// Second image (`t = 1.0`).
        b: Box<BackgroundImage>,
        /// Blend factor clamped to `[0.0, 1.0]`.
        t: f32,
    },
    /// CSS Paint API (Houdini) — `paint(name, args…)`: an image the registered paint
    /// worklet draws at the layer's tile size.
    Paint {
        /// Worklet name given to `registerPaint()`.
        name: String,
        /// Raw text of the extra arguments (`inputArguments`), top-level commas split.
        args: Vec<String>,
    },
}

/// CSS Backgrounds L3 §3.4 — `<repeat-style>` value of one axis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatStyle {
    #[default]
    Repeat,
    NoRepeat,
    Round,
    Space,
}

impl RepeatStyle {
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "repeat" => Some(Self::Repeat),
            "no-repeat" => Some(Self::NoRepeat),
            "round" => Some(Self::Round),
            "space" => Some(Self::Space),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3.4 — `background-repeat` of one layer: a style per axis.
///
/// `repeat-x` / `repeat-y` are sugar for `repeat no-repeat` / `no-repeat repeat`;
/// a single `round` / `space` / `repeat` / `no-repeat` sets both axes.
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundRepeat {
    pub x: RepeatStyle,
    pub y: RepeatStyle,
}

impl BackgroundRepeat {
    pub const REPEAT: Self = Self { x: RepeatStyle::Repeat, y: RepeatStyle::Repeat };
    pub const NO_REPEAT: Self = Self { x: RepeatStyle::NoRepeat, y: RepeatStyle::NoRepeat };
    pub const REPEAT_X: Self = Self { x: RepeatStyle::Repeat, y: RepeatStyle::NoRepeat };
    pub const REPEAT_Y: Self = Self { x: RepeatStyle::NoRepeat, y: RepeatStyle::Repeat };
    pub const ROUND: Self = Self { x: RepeatStyle::Round, y: RepeatStyle::Round };
    pub const SPACE: Self = Self { x: RepeatStyle::Space, y: RepeatStyle::Space };

    /// Parses a whole `<repeat-style>` value: `repeat-x | repeat-y | <style>{1,2}`.
    pub fn parse(s: &str) -> Option<Self> {
        let mut it = s.split_ascii_whitespace();
        let first = it.next()?;
        let second = it.next();
        if it.next().is_some() {
            return None;
        }
        let (r, used) = Self::parse_prefix(first, second)?;
        (used == 1 + usize::from(second.is_some())).then_some(r)
    }

    /// Parses `first` plus an optional following token. Returns the value and
    /// how many tokens it consumed (1 or 2); for the shorthand parsers, where
    /// the next token may belong to another layer component.
    pub fn parse_prefix(first: &str, next: Option<&str>) -> Option<(Self, usize)> {
        if first.eq_ignore_ascii_case("repeat-x") {
            return Some((Self::REPEAT_X, 1));
        }
        if first.eq_ignore_ascii_case("repeat-y") {
            return Some((Self::REPEAT_Y, 1));
        }
        let x = RepeatStyle::parse(first)?;
        match next.and_then(RepeatStyle::parse) {
            Some(y) => Some((Self { x, y }, 2)),
            None => Some((Self { x, y: x }, 1)),
        }
    }
}

impl std::fmt::Debug for BackgroundRepeat {
    /// Keeps the one-word names of the single-keyword forms so display-list
    /// dumps and goldens stay byte-identical; per-axis mixes print `X/Y`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::REPEAT => f.write_str("Repeat"),
            Self::NO_REPEAT => f.write_str("NoRepeat"),
            Self::REPEAT_X => f.write_str("RepeatX"),
            Self::REPEAT_Y => f.write_str("RepeatY"),
            Self::ROUND => f.write_str("Round"),
            Self::SPACE => f.write_str("Space"),
            Self { x, y } => write!(f, "{x:?}/{y:?}"),
        }
    }
}

/// CSS Backgrounds L3 §3.5 — one axis of an explicit `background-size` value.
///
/// `Px`/`Percent` are resolved against the positioning area extent along this
/// axis at paint time; `Auto` derives the extent from the other axis (preserving
/// the image's intrinsic aspect ratio).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BgSizeAxis {
    /// Derive this axis from the other axis / the image's intrinsic ratio.
    Auto,
    /// Fixed length in CSS px.
    Px(f32),
    /// Percentage of the positioning area along this axis (fraction `0.0..`).
    Percent(f32),
}

impl BgSizeAxis {
    /// Resolve to a concrete px extent against `area` (the positioning-area
    /// size along this axis). Returns `None` for `Auto` (caller derives it from
    /// the other axis / intrinsic ratio).
    #[must_use]
    pub fn resolve(self, area: f32) -> Option<f32> {
        match self {
            BgSizeAxis::Auto => None,
            BgSizeAxis::Px(v) => Some(v),
            BgSizeAxis::Percent(p) => Some(p * area),
        }
    }
}

/// CSS Backgrounds L3 §3.5 — `background-size`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum BackgroundSize {
    #[default]
    Auto,
    Cover,
    Contain,
    /// Explicit width / height, each `auto` | `<length>` | `<percentage>`.
    /// Percentages resolve against the positioning area at paint time.
    Length(BgSizeAxis, BgSizeAxis),
}

/// Списки значений per-layer-свойств так, как их записал автор (BUG-1297): computed-значение
/// `background-repeat: repeat-x` при двух слоях остаётся `repeat-x`, а три значения при одном
/// слое — тремя. `background_layers` циклически растягивает/обрезает списки под число
/// изображений и этого не помнит; пустой список = свойство не задано (читаются слои).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BackgroundSpecified {
    pub repeat: Vec<BackgroundRepeat>,
    pub size: Vec<BackgroundSize>,
    pub attachment: Vec<BackgroundAttachment>,
    pub origin: Vec<BackgroundOrigin>,
    pub clip: Vec<BackgroundClip>,
    pub position_x: Vec<PositionComponent>,
    pub position_y: Vec<PositionComponent>,
}

/// CSS Backgrounds L3 §3.6 — `background-attachment`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundAttachment {
    #[default]
    Scroll,
    Fixed,
    Local,
}

impl BackgroundAttachment {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "scroll" => Some(Self::Scroll),
            "fixed" => Some(Self::Fixed),
            "local" => Some(Self::Local),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3.7 — `background-origin`. Non-inherited.
///
/// Определяет, к какому **краю box-а** привязана позиционная система
/// для `background-image` (initial = padding edge). На `background-color`
/// не влияет (тот всегда заливает border-edge независимо от origin).
///
/// **Phase 0 ограничение:** parsing + storage only. Реальное смещение
/// origin-у в paint pipeline (выбор `border_box` / `padding_box` /
/// `content_box` rect при расчёте начала tile-тиления) — отдельная
/// задача с согласованием P2 (crate-ownership matrix).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundOrigin {
    /// `border-box` — позиционная система начинается с border-edge.
    BorderBox,
    /// `padding-box` (initial) — с padding-edge (= внутренний край border-а).
    #[default]
    PaddingBox,
    /// `content-box` — с content-edge (= внутренний край padding-а).
    ContentBox,
}

impl BackgroundOrigin {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "border-box" => Some(Self::BorderBox),
            "padding-box" => Some(Self::PaddingBox),
            "content-box" => Some(Self::ContentBox),
            _ => None,
        }
    }

    /// `<geometry-box>` of `mask-origin` (CSS Masking L1 §4.5): the three CSS
    /// boxes plus the SVG ones, which CSS Box 4 §1 maps onto CSS boxes —
    /// `fill-box` is the content box, `stroke-box` and `view-box` the border box.
    pub fn parse_geometry_box(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fill-box" => Some(Self::ContentBox),
            "stroke-box" | "view-box" => Some(Self::BorderBox),
            other => Self::parse(other),
        }
    }
}

/// CSS Backgrounds L3 §3.8 — `background-clip`. Non-inherited.
///
/// Определяет, к какому **краю box-а** обрезается `background-color`
/// и `background-image` (initial = border edge, т.е. фон видно даже
/// сквозь полупрозрачную рамку).
///
/// Variant `Text` (CSS Backgrounds L4) клипает фон по форме глифов —
/// классический паттерн «gradient text» через `background-clip: text`
/// и `color: transparent`. Реализация в paint требует подмаски через
/// glyph-cache mask-image — отдельная задача с согласованием P2.
///
/// **Phase 0 ограничение:** parsing + storage only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundClip {
    /// `border-box` (initial) — фон под border-ом виден.
    #[default]
    BorderBox,
    /// `padding-box` — фон обрезается до внутреннего края border-а.
    PaddingBox,
    /// `content-box` — фон только в content-area.
    ContentBox,
    /// `text` (CSS Backgrounds L4) — фон клипается по форме текста
    /// внутри box-а. Phase 0 хранит как atom, реальный clip — P2.
    Text,
    /// `border-area` (CSS Backgrounds L4 §3.8) — фон виден только там, где
    /// нарисована рамка (border box минус padding box, со скруглениями и
    /// разрывами `border-style`). Paint: `emit_border_area_clipped`.
    BorderArea,
}

impl BackgroundClip {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "border-box" => Some(Self::BorderBox),
            "padding-box" => Some(Self::PaddingBox),
            "content-box" => Some(Self::ContentBox),
            "text" => Some(Self::Text),
            "border-area" => Some(Self::BorderArea),
            _ => None,
        }
    }
}

/// CSS Masking L1 §4.6 — `mask-clip: <coord-box> | no-clip`.
///
/// `<coord-box>` = `content-box | padding-box | border-box | fill-box |
/// stroke-box | view-box`. Unlike `background-clip`, `mask-clip` also accepts
/// the SVG reference boxes and the `no-clip` keyword. For elements laid out
/// with the CSS box model (non-SVG HTML boxes) the SVG-specific boxes fall
/// back to their box-model equivalents (CSS Box 4 §1 "Choosing the layout
/// box"): `fill-box` → content box, `stroke-box`/`view-box` → border box.
/// `no-clip` disables the mask painting-area clip entirely. Non-inherited,
/// initial `border-box`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MaskClip {
    /// `border-box` (initial) — mask painting area is the border box.
    #[default]
    BorderBox,
    /// `padding-box` — clip to the inner border edge.
    PaddingBox,
    /// `content-box` — clip to the content area.
    ContentBox,
    /// `fill-box` — object bounding box; for CSS boxes equals the content box.
    FillBox,
    /// `stroke-box` — stroke bounding box; for CSS boxes equals the border box.
    StrokeBox,
    /// `view-box` — nearest SVG viewport; for CSS boxes equals the border box.
    ViewBox,
    /// `no-clip` — the mask painting area is not clipped.
    NoClip,
}

impl MaskClip {
    /// Parses a single `mask-clip` keyword (CSS Masking L1 §4.6).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "border-box" => Some(Self::BorderBox),
            "padding-box" => Some(Self::PaddingBox),
            "content-box" => Some(Self::ContentBox),
            "fill-box" => Some(Self::FillBox),
            "stroke-box" => Some(Self::StrokeBox),
            "view-box" => Some(Self::ViewBox),
            "no-clip" => Some(Self::NoClip),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3 — один фоновый слой. Первый в Vec = верхний (рисуется последним).
///
/// Все поля — initial values из спецификации. `background_color` не входит
/// в слой — он всегда одиночный и хранится в `ComputedStyle.background_color`.
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundLayer {
    /// `background-image` для этого слоя.
    pub image: BackgroundImage,
    /// Computed-запись `background-image` (BUG-1428): градиент/`image-set()`/`cross-fade()` целиком,
    /// как её отдаёт `getComputedStyle`. `None` — запись строится из `image`.
    pub image_css: Option<std::sync::Arc<str>>,
    /// `background-repeat` для этого слоя.
    pub repeat: BackgroundRepeat,
    /// `background-size` для этого слоя.
    pub size: BackgroundSize,
    /// `background-position` для этого слоя.
    pub position: ObjectPosition,
    /// `background-attachment` для этого слоя.
    pub attachment: BackgroundAttachment,
    /// `background-origin` для этого слоя.
    pub origin: BackgroundOrigin,
    /// `background-clip` для этого слоя.
    pub clip: BackgroundClip,
    /// CSS Compositing L1 §8.3 — `background-blend-mode` для этого слоя.
    /// Initial: normal. Не наследуется. Применяется при слиянии background
    /// layers между собой (не с контентом элемента).
    pub blend_mode: MixBlendMode,
}

impl Default for BackgroundLayer {
    fn default() -> Self {
        Self {
            image: BackgroundImage::None,
            image_css: None,
            repeat: BackgroundRepeat::REPEAT,
            size: BackgroundSize::Auto,
            position: ObjectPosition::background_initial(),
            attachment: BackgroundAttachment::Scroll,
            origin: BackgroundOrigin::PaddingBox,
            clip: BackgroundClip::BorderBox,
            blend_mode: MixBlendMode::Normal,
        }
    }
}

/// CSS Images L3 §5.5 — `object-fit`. Применяется к replaced elements
/// (`<img>`, `<video>`, `<canvas>` и т.д.) и определяет, как «коробка»
/// заливается содержимым с учётом intrinsic-размеров. Не наследуется.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectFit {
    /// `fill` (default) — растянуть на размер коробки без сохранения
    /// aspect ratio. Картинка может быть искажена.
    #[default]
    Fill,
    /// `contain` — максимально большой размер с сохранением aspect ratio,
    /// при котором изображение **умещается** целиком (letterbox / pillarbox).
    Contain,
    /// `cover` — минимально большой размер с сохранением aspect ratio,
    /// при котором изображение **покрывает** коробку. Излишки клипятся
    /// по `object-position`.
    Cover,
    /// `none` — без масштабирования (intrinsic-размер 1:1). Излишки
    /// клипятся; недостаток заполняется по `object-position`.
    None,
    /// `scale-down` — `min(none, contain)`: если intrinsic-размер меньше
    /// коробки, ведёт себя как `none`; иначе как `contain`.
    ScaleDown,
}

impl ObjectFit {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fill" => Some(Self::Fill),
            "contain" => Some(Self::Contain),
            "cover" => Some(Self::Cover),
            "none" => Some(Self::None),
            "scale-down" => Some(Self::ScaleDown),
            _ => None,
        }
    }
}

/// CSS Images L3 §6.1 — `image-rendering`. Hint для движка о том, как
/// масштабировать растровое изображение (применимо к `<img>`, background-image,
/// canvas, и т.д.). Inherited.
///
/// Phase 0: parsing + storage. Реальное переключение GPU sampler filter
/// (`Linear` для `auto`/`smooth`/`high-quality`, `Nearest` для `pixelated`/
/// `crisp-edges`) в `lumen-paint` — отдельная задача с согласованием P2.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageRendering {
    /// `auto` (default) — UA выбирает алгоритм. Обычно — bilinear.
    #[default]
    Auto,
    /// `smooth` — high-quality scaling, оптимизирован для smooth gradient.
    /// На практике в современных движках = `auto`.
    Smooth,
    /// `high-quality` — высочайшее качество масштабирования (тяжелее `smooth`).
    /// Спецификация добавлена в CSS Images L4; считается переименованием
    /// `optimizeQuality` из L3 (которое теперь deprecated).
    HighQuality,
    /// `crisp-edges` — сохраняет контраст и резкость границ (pixel art /
    /// vector graphics). UA может использовать nearest-neighbour или
    /// edge-preserving алгоритм.
    CrispEdges,
    /// `pixelated` — nearest-neighbour. Полезно для масштабирования pixel art.
    Pixelated,
}

impl ImageRendering {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "smooth" => Some(Self::Smooth),
            "high-quality" => Some(Self::HighQuality),
            "crisp-edges" => Some(Self::CrispEdges),
            "pixelated" => Some(Self::Pixelated),
            _ => None,
        }
    }
}

