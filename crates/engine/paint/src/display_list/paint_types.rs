//! P1/SPLIT-DL19: типы примитивов — `enum FilterMode` … до конца
//! `impl ResolvedClipShape`. Вынесено из `display_list.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-19).

use super::*;

/// CSS Images L3 §4.3 — image-rendering filter mode (scaling algorithm).
/// Determines how textures are sampled when an image is scaled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterMode {
    /// `auto` (default), `smooth`, `high-quality` — high-quality scaling (bilinear).
    #[default]
    Linear,
    /// `crisp-edges`, `pixelated` — preserve sharp edges (nearest-neighbour).
    Nearest,
}

impl FilterMode {
    /// Преобразует `ImageRendering` в `FilterMode`.
    /// `auto`/`smooth`/`high-quality` → `Linear` (bilinear).
    /// `crisp-edges`/`pixelated` → `Nearest` (pixel-perfect).
    #[must_use]
    pub fn from_image_rendering(ir: ImageRendering) -> Self {
        match ir {
            ImageRendering::Auto | ImageRendering::Smooth | ImageRendering::HighQuality => Self::Linear,
            ImageRendering::CrispEdges | ImageRendering::Pixelated => Self::Nearest,
        }
    }
}

/// CSS Compositing & Blending L1 §5 — blend mode. Phase 0 содержит только
/// `Normal` (no-op); остальные 16 mode-ов парсятся в CSS-каскаде, но
/// реальный composite-pipeline для них — задача P2 п.4 (mix-blend-mode).
/// `PlusLighter` — из CSS Compositing & Blending L2 §6, реализуется
/// как additive compositing с pre-multiplied alpha.
/// Хранится в `DisplayCommand::PushBlendMode` как stub-значение, чтобы
/// расширить enum без правки потребителей.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    PlusLighter,
}

impl BlendMode {
    /// Парсит CSS-keyword `mix-blend-mode` / `background-blend-mode` (CSS
    /// Compositing & Blending L1 §5). Case-insensitive — `MULTIPLY` и
    /// `multiply` оба возвращают `Multiply`. Возвращает `None` на
    /// нераспознанной строке; caller (CSS-каскад) трактует это как
    /// invalid declaration и применяет initial value (`Normal`).
    #[must_use]
    pub fn from_keyword(s: &str) -> Option<Self> {
        // ASCII case fold — keyword-ы CSS все ASCII, дешёвый match
        // через to_ascii_lowercase в стек-буфер не нужен (хватает
        // `eq_ignore_ascii_case`).
        for (kw, mode) in [
            ("normal", Self::Normal),
            ("multiply", Self::Multiply),
            ("screen", Self::Screen),
            ("overlay", Self::Overlay),
            ("darken", Self::Darken),
            ("lighten", Self::Lighten),
            ("color-dodge", Self::ColorDodge),
            ("color-burn", Self::ColorBurn),
            ("hard-light", Self::HardLight),
            ("soft-light", Self::SoftLight),
            ("difference", Self::Difference),
            ("exclusion", Self::Exclusion),
            ("hue", Self::Hue),
            ("saturation", Self::Saturation),
            ("color", Self::Color),
            ("luminosity", Self::Luminosity),
            ("plus-lighter", Self::PlusLighter),
        ] {
            if s.eq_ignore_ascii_case(kw) {
                return Some(mode);
            }
        }
        None
    }
}

/// CSS Masking L1 §6 — how to derive the mask value from rendered mask-layer pixels.
///
/// `Alpha` is the default for raster images (§6.2). `Luminance` converts the mask
/// layer's RGB colour to relative luminance per ITU-R BT.709, then multiplies by
/// the alpha channel — identical to SVG `mask-type: luminance` (§6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaskMode {
    /// Use the mask layer's alpha channel directly as the mask value (default).
    #[default]
    Alpha,
    /// Convert the mask layer's colour to luminance: `luma = 0.2126·R + 0.7152·G + 0.0722·B`,
    /// then multiply by alpha. White opaque → mask=1, black opaque → mask=0.
    Luminance,
}

/// Corner radii for CSS `border-radius`. Values are in CSS pixels, clamped to ≥ 0.
/// Each corner stores separate horizontal (x) and vertical (y) radii supporting
/// elliptical corners (`border-radius: 10px / 20px`). When x == y the corner is circular.
/// Order matches CSS shorthand resolution: top-left, top-right, bottom-right, bottom-left.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CornerRadii {
    /// Top-left horizontal (x) radius in CSS px.
    pub tl: f32,
    /// Top-left vertical (y) radius in CSS px.
    pub tl_y: f32,
    /// Top-right horizontal (x) radius in CSS px.
    pub tr: f32,
    /// Top-right vertical (y) radius in CSS px.
    pub tr_y: f32,
    /// Bottom-right horizontal (x) radius in CSS px.
    pub br: f32,
    /// Bottom-right vertical (y) radius in CSS px.
    pub br_y: f32,
    /// Bottom-left horizontal (x) radius in CSS px.
    pub bl: f32,
    /// Bottom-left vertical (y) radius in CSS px.
    pub bl_y: f32,
}

impl CornerRadii {
    /// Returns `true` if all eight radii are zero (no rounding needed).
    #[must_use]
    pub fn all_zero(&self) -> bool {
        self.tl == 0.0 && self.tr == 0.0 && self.br == 0.0 && self.bl == 0.0
            && self.tl_y == 0.0 && self.tr_y == 0.0 && self.br_y == 0.0 && self.bl_y == 0.0
    }

    fn resolve_radius(len: &Length, basis: f32) -> f32 {
        match len {
            Length::Px(v) => *v,
            Length::Percent(p) => p / 100.0 * basis,
            _ => 0.0,
        }
    }

    /// Builds `CornerRadii` from a `ComputedStyle` and the element's border-box dimensions.
    /// `border_w` / `border_h` resolve `border-radius: N%` per CSS Backgrounds L3 §5.5:
    /// H radii use width as basis, V radii use height.
    pub fn from_style_and_box(s: &ComputedStyle, border_w: f32, border_h: f32) -> Self {
        Self {
            tl:   Self::resolve_radius(&s.border_top_left_radius,     border_w),
            tl_y: Self::resolve_radius(&s.border_top_left_radius_y,   border_h),
            tr:   Self::resolve_radius(&s.border_top_right_radius,    border_w),
            tr_y: Self::resolve_radius(&s.border_top_right_radius_y,  border_h),
            br:   Self::resolve_radius(&s.border_bottom_right_radius,   border_w),
            br_y: Self::resolve_radius(&s.border_bottom_right_radius_y, border_h),
            bl:   Self::resolve_radius(&s.border_bottom_left_radius,   border_w),
            bl_y: Self::resolve_radius(&s.border_bottom_left_radius_y, border_h),
        }
    }

    /// Builds `CornerRadii` from a `ComputedStyle`. `border-radius: N%` values are
    /// resolved as 0 because box dimensions are unavailable here. Prefer
    /// `from_style_and_box` when the border-box rect is known.
    pub fn from_style(s: &ComputedStyle) -> Self {
        Self::from_style_and_box(s, 0.0, 0.0)
    }

    /// Clamps every radius via the CSS Backgrounds L3 §5.5 corner-overlap rule.
    /// `w`/`h` are the box dimensions the radii apply to. A single scale factor
    /// `f ≤ 1` is chosen so the sum of the two radii along every edge fits that
    /// edge's length, then **all** radii are multiplied by `f`. Computed from the
    /// specified radii per-axis (x-radii against `w`, y-radii against `h`), so a
    /// wide-but-short elliptical corner (`rx ≠ ry`, e.g. an SVG `<ellipse>` mapped
    /// to a 240×90 box) keeps its aspect. The earlier naive `min(w/2, h/2)` cap
    /// collapsed such corners into circles (BUG-198), turning ellipses into
    /// stadiums in the femtovg/border paths. For uniform radii the result is
    /// unchanged from that cap.
    #[must_use]
    pub fn clamped_to_box(&self, w: f32, h: f32) -> Self {
        if w <= 0.0 || h <= 0.0 {
            return Self::default();
        }
        // Per-edge ratio: edge length / sum of the two radii along it (capped at 1).
        let ratio = |len: f32, sum: f32| if sum > len { len / sum } else { 1.0 };
        let f = ratio(w, self.tl + self.tr)        // top edge (x-radii)
            .min(ratio(h, self.tr_y + self.br_y))  // right edge (y-radii)
            .min(ratio(w, self.br + self.bl))      // bottom edge (x-radii)
            .min(ratio(h, self.bl_y + self.tl_y))  // left edge (y-radii)
            .clamp(0.0, 1.0);
        let s = |r: f32| (r * f).max(0.0);
        Self {
            tl: s(self.tl),   tl_y: s(self.tl_y),
            tr: s(self.tr),   tr_y: s(self.tr_y),
            br: s(self.br),   br_y: s(self.br_y),
            bl: s(self.bl),   bl_y: s(self.bl_y),
        }
    }

    /// Computes the inner-edge corner radii for a border of per-side widths
    /// `[top, right, bottom, left]` (CSS px). Each inner radius is the outer
    /// radius minus the adjacent border width, floored at 0 — the standard CSS
    /// border inner-radius rule (CSS Backgrounds L3 §5.5). A corner's horizontal
    /// radius is reduced by the adjacent vertical border (left/right), its
    /// vertical radius by the adjacent horizontal border (top/bottom).
    #[must_use]
    pub fn inner_for_border(&self, widths: [f32; 4]) -> Self {
        let [top, right, bottom, left] = widths;
        Self {
            tl:   (self.tl   - left).max(0.0),
            tl_y: (self.tl_y - top).max(0.0),
            tr:   (self.tr   - right).max(0.0),
            tr_y: (self.tr_y - top).max(0.0),
            br:   (self.br   - right).max(0.0),
            br_y: (self.br_y - bottom).max(0.0),
            bl:   (self.bl   - left).max(0.0),
            bl_y: (self.bl_y - bottom).max(0.0),
        }
    }
}

/// BUG-140: `clip-path` basic-shape, разрешённая эмиттером в page-координаты
/// (px) относительно border-box элемента. Координаты — в пространстве ДО
/// transform элемента: команда `PushClipPath` эмитится внутри
/// `PushTransform`, бэкенд переносит форму активной матрицей канвы.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedClipShape {
    /// `circle(r at cx cy)`: центр и радиус в page px.
    Circle {
        /// Центр X (page px).
        cx: f32,
        /// Центр Y (page px).
        cy: f32,
        /// Радиус (px).
        r: f32,
    },
    /// `ellipse(rx ry at cx cy)`: центр и полуоси в page px.
    Ellipse {
        /// Центр X (page px).
        cx: f32,
        /// Центр Y (page px).
        cy: f32,
        /// Горизонтальная полуось (px).
        rx: f32,
        /// Вертикальная полуось (px).
        ry: f32,
    },
    /// `polygon(...)` / `path(...)`: вершины в page px. `even_odd` выбирает
    /// правило заливки самопересекающихся контуров (CSS Shapes L1 §3/§4):
    /// `true` → even-odd (дырки в перекрытиях), `false` → nonzero (default).
    Polygon {
        /// Вершины формы в page px (до transform элемента).
        verts: Vec<(f32, f32)>,
        /// `true` = even-odd fill rule, `false` = nonzero.
        even_odd: bool,
    },
}

impl ResolvedClipShape {
    /// Axis-aligned bounding box формы (page px, до transform). Используется
    /// fallback-путями, не умеющими клиппить произвольную форму (wgpu
    /// scissor, hit-test).
    pub fn bounding_rect(&self) -> Rect {
        match self {
            Self::Circle { cx, cy, r } => {
                Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r)
            }
            Self::Ellipse { cx, cy, rx, ry } => {
                Rect::new(cx - rx, cy - ry, 2.0 * rx, 2.0 * ry)
            }
            Self::Polygon { verts, .. } => {
                if verts.is_empty() {
                    return Rect::new(0.0, 0.0, 0.0, 0.0);
                }
                let mut mn_x = f32::MAX;
                let mut mn_y = f32::MAX;
                let mut mx_x = f32::MIN;
                let mut mx_y = f32::MIN;
                for (x, y) in verts {
                    mn_x = mn_x.min(*x);
                    mn_y = mn_y.min(*y);
                    mx_x = mx_x.max(*x);
                    mx_y = mx_y.max(*y);
                }
                Rect::new(mn_x, mn_y, (mx_x - mn_x).max(0.0), (mx_y - mn_y).max(0.0))
            }
        }
    }
}
