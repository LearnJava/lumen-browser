//! Display list — линейный список графических команд, выработанных из
//! дерева layout. Растеризатору (renderer) уже не нужно понимать DOM/CSS:
//! он рендерит то, что ему говорят.
//!
//! Координаты — экранные пиксели от верхнего левого угла окна.
//!
//! **ADR-008 Invariant 3 note (paint-pure-audit 10D.2, 2026-05-27):**
//! All display list builder functions (`build_display_list`, `build_display_list_with_anim`,
//! `build_display_list_ordered`, `build_display_list_ordered_with_anim`) are pure functions:
//! they depend only on their function parameters (LayoutBox, optional compositor anim frame,
//! optional stacking tree) and do not depend on hidden global state, thread-locals, or
//! environment variables. No `static mut` / `lazy_static!` / `OnceCell` found in this module.
//! Renderer caching (glyph atlas, image cache, layer snapshots) lives in separate crates
//! (lumen-font, lumen-image) with explicit eviction APIs.

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use std::collections::HashMap;
use std::ops::Range;

use lumen_core::geom::{Rect, Size};
use lumen_dom::InputType;
use lumen_layout::{
    box_can_own_stacking_context, creates_stacking_context, forward_box_transform,
    transform_fns_to_matrix, BoxOrigin, BoxRole, PseudoKind, CompositorAnimFrame, CompositorOverride,
    Appearance, BackfaceVisibility,
    BackgroundClip, BackgroundImage, BackgroundLayer, BackgroundOrigin, BackgroundRepeat, BackgroundSize, BorderCollapse, BorderStyle, BoxKind, MaskClip, MaskComposite, MaskLayer,
    ClipPath, Color, ComputedStyle, ContainFlags, CssColor, Display, EmptyCells, FilterFn, FontOpticalSizing, FontStretch, FontStyle, FontWeight, ShapeValue,
    FillRule, FormControlKind, StrokeLinecap, StrokeLinejoin, SvgShapeKind, SvgTextAnchor, SvgDominantBaseline, SvgBaselineShift,
    SvgGradientDef, SvgGradientUnits, SvgPaint,
    GradientStop, ImageRendering, Isolation, Length, ListStyleType, ParsedGradient,
    InlineFrag, LayoutBox, MarginBox, Mat4, MixBlendMode as LayoutBlendMode, ObjectFit, ObjectPosition,
    OutlineColor, OutlineStyle, Overflow, Page, PaintOrder, PaintPhase, Position, PositionComponent, Resize,
    ScrollbarWidth, SelectionHighlight,
    StackingContextId, StackingTree, TextDecorationSkipInk, TextDecorationStyle, TextDecorationThickness,
    TextEmphasisShape, TextEmphasisStyle, TextOverflow, TextUnderlinePosition,
    TransformStyle,
    Visibility, style::TextOrientation,
    font_palette::{palette_selection, FontPaletteSelection},
};

use crate::gap_decorations::{emit_gap_rules, GapDecorationContext, GapSegment};

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

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayCommand {
    FillRect {
        rect: Rect,
        color: Color,
    },
    /// CSS Backgrounds L3 §5 — `border-radius`: filled rect with rounded corners.
    /// Rendered via SDF in the GPU fragment shader; anti-aliased at sub-pixel level.
    /// Used instead of `FillRect` when any corner radius > 0.
    FillRoundedRect {
        rect: Rect,
        color: Color,
        /// Corner radii in CSS px (tl, tr, br, bl).
        radii: CornerRadii,
    },
    DrawBorder {
        rect: Rect,
        /// Ширины сторон: [top, right, bottom, left].
        widths: [f32; 4],
        /// Цвета сторон: [top, right, bottom, left].
        colors: [Color; 4],
        /// Стили сторон: [top, right, bottom, left]. CSS Backgrounds L3 §6.
        /// `None` обычно фильтруется emit-side через `is_visible()`, в команду
        /// попадает Solid / Dashed / Dotted (по текущему `BorderStyle` enum).
        /// Renderer разворачивает Dashed/Dotted в pattern из штрихов / точек.
        styles: [BorderStyle; 4],
        /// Corner radii in CSS px (tl, tr, br, bl). Zero = rectangular corners.
        radii: CornerRadii,
    },
    /// CSS Basic UI L4 §5 — `outline`. Рисуется СНАРУЖИ box-а (в отличие
    /// от border, который часть box-model), не занимает место в layout,
    /// может перекрывать соседей и не ловит pointer-события. `rect` —
    /// исходная коробка box-а (renderer сам расширит её на `offset` и
    /// `width`). `style` ≠ None / Hidden — иначе emit не происходит.
    /// `color` уже разрешён в конкретный `Color` на emission-стороне
    /// (Auto / CurrentColor резолвится в `style.color`).
    /// Phase 0: renderer рисует `Auto` как Solid (UA focus ring без хвоста).
    /// `Dashed`/`Dotted` реализованы через `emit_outline_side`. `Double`
    /// маппится на Solid в `parse_outline_style_opt` (нет отдельного variant-а).
    DrawOutline {
        rect: Rect,
        width: f32,
        style: OutlineStyle,
        color: Color,
        offset: f32,
    },
    DrawText {
        rect: Rect,
        text: String,
        font_size: f32,
        color: Color,
        /// CSS Fonts L4 §3.1 — приоритизированный список имён семейств.
        /// Пустой Vec означает «никакой явной family-инструкции» — renderer
        /// использует bundled-шрифт (Inter Regular). Renderer перебирает имена
        /// через `FontProvider::pick_face`; первый найденный face побеждает.
        font_family: Vec<String>,
        /// CSS-вес 1..1000. По умолчанию 400 (Regular). Передаётся в
        /// `FontProvider::pick_face`; алгоритм матчинга — CSS Fonts L4 §5.2.
        font_weight: FontWeight,
        /// `font-style`. По умолчанию Normal.
        font_style: FontStyle,
        /// CSS Fonts L4 §2.5 — `font-stretch` для **статического** подбора
        /// face-а: renderer отдаёт его в `FontProvider::pick_face`, где он
        /// сопоставляется с `usWidthClass` из OS/2 каждого face-а семейства
        /// (§5.2). Это выбирает отдельный condensed/expanded файл там, где
        /// семейство их имеет.
        ///
        /// Ортогонально оси `wdth` в `font_variation_axes`: variable-шрифт
        /// интерполируется осью, статическое семейство — этим полем; на
        /// шрифте, у которого есть и то и другое, работают оба. Хранится в
        /// десятых долях процента (`FontStretch::NORMAL` = 1000 = 100%),
        /// в проценты для matcher-а переводится `FontStretch::as_percent`.
        font_stretch: FontStretch,
        /// CSS Fonts L4 §7 — user-space variation axes из `font-variation-settings`.
        /// Пары `(tag, value)` в user units — нормализация через fvar+avar
        /// выполняется в renderer-е, который имеет доступ к шрифтовым таблицам.
        /// Пустой Vec = `normal` (default-instance без variation deltas).
        /// CSS: font-optical-sizing — P4 должен добавить opsz значение в этот Vec.
        font_variation_axes: Vec<([u8; 4], f32)>,
        /// CSS Fonts L3 §6 — `font-feature-settings` overrides. Пары
        /// `(tag, value)`: 0 = выключить фичу, ≥1 = включить. Пустой Vec =
        /// `normal` (default-набор фич шейпера: liga/clig/calt/rlig/ccmp +
        /// kern). Применяется на путях, шейпящих через lumen-font
        /// (CPU-растр, векторный variable-font путь femtovg); нативный
        /// femtovg-текст шейпит сам и переопределения игнорирует.
        font_features: Vec<([u8; 4], u32)>,
        /// CSS Fonts L4 §11.3 — `font-palette` selection for COLR color
        /// glyphs. `None` = `normal` (default CPAL palette 0). `Light`/`Dark`
        /// pick the first CPAL palette with the matching paletteType flag;
        /// `Custom` carries a resolved `@font-palette-values` rule (base
        /// index + per-slot color overrides). Renderer currently ignores the
        /// field: lumen-font has no COLR/CPAL rasterization yet (deferred) —
        /// the value is wired so palette data is display-list-complete.
        font_palette: Option<FontPaletteSelection>,
        /// CSS Text L3 §10.1 — pixel width for a tab character (\t).
        /// 0.0 means no tab characters in text (renderer skips tab expansion).
        tab_size: f32,
        highlight_name: Option<String>,
        /// CSS Writing Modes L3 §6.5 — `text-orientation`. `None` = horizontal text;
        /// `Some(...)` signals vertical layout: paint rotates glyphs 90° CW for
        /// `Sideways`, and applies per-glyph mixed-mode in `Mixed` (deferred to
        /// Phase 2+; Phase 1 treats `Mixed` as `Sideways`).
        text_orientation: Option<TextOrientation>,
    },
    /// Растровое изображение из `<img>`. `rect` — итоговая коробка после
    /// расчёта по CSS (width/height + HTML presentational hints), `src` —
    /// строка ссылки на ресурс из исходного атрибута (декодирование и
    /// загрузка пикселей — отдельная задача, см. roadmap). `alt` — alternate
    /// text для случаев, когда renderer не может отобразить картинку.
    /// `object_fit` / `object_position` (CSS Images L3 §5.5) определяют,
    /// как intrinsic-размер изображения вписывается в `rect`; renderer
    /// читает их вместе с известным intrinsic-размером (доступен на
    /// GPU-cache стороне) для расчёта итогового quad.
    ///
    /// Renderer Phase 0 рисует placeholder rect (светло-серый прямоугольник),
    /// если картинка не зарегистрирована в GPU-cache.
    DrawImage {
        rect: Rect,
        src: String,
        alt: String,
        object_fit: ObjectFit,
        object_position: ObjectPosition,
        image_rendering: ImageRendering,
    },
    /// Slot for an `<img loading="lazy">`.
    ///
    /// Rendered as a grey rect *until* its image is registered; once the shell
    /// fetches and registers the image (keyed by `src`), the backend draws it
    /// in place — identical to a `DrawImage` whose bytes have arrived. This is
    /// why `object_fit`/`object_position` are carried here too: a lazy image
    /// must honour the same CSS fitting rules as an eager one once loaded.
    /// `node_id` is the DOM node index — lets the shell correlate this slot with
    /// the proximity check (`_lumen_request_lazy_image_load`).
    LazyImageSlot {
        rect: Rect,
        node_id: u32,
        src: String,
        object_fit: ObjectFit,
        object_position: ObjectPosition,
    },
    /// CSS Backgrounds L3 §3.10 — `background-image: url(...)`.
    ///
    /// `rect` — background painting area (clip box), computed from `background-clip`
    /// (border-box / padding-box / content-box). Defines where pixels are actually drawn.
    ///
    /// `origin_rect` — background positioning area, computed from `background-origin`
    /// (CSS Backgrounds L3 §3.5). Defines the coordinate space for `background-size`
    /// (cover/contain/%) and `background-position` (% offsets). Differs from `rect`
    /// when `background-origin != background-clip` (e.g., origin: content-box,
    /// clip: border-box — common pattern).
    ///
    /// `src` — URL, same key as `Renderer::register_image`.
    /// `size`, `position`, `repeat` — CSS Backgrounds L3 §3.3/3.4/3.5.
    ///
    /// Порядок: после `FillRect` для background-color, до border.
    /// Если картинка не зарегистрирована в GPU-cache — визуально no-op.
    DrawBackgroundImage {
        /// Background painting area — from `background-clip`. Pixels only drawn inside.
        rect: Rect,
        /// Background positioning area — from `background-origin`. Used for size/position math.
        origin_rect: Rect,
        src: String,
        size: BackgroundSize,
        position: ObjectPosition,
        repeat: BackgroundRepeat,
        image_rendering: ImageRendering,
    },
    /// CSS Images L3 §3.3 — `linear-gradient(angle, stop, ...)`.
    ///
    /// `angle_deg` — CSS-convention degrees (0° = to top, 90° = to right,
    /// 180° = to bottom, 270° = to left). Renderer converts to a gradient
    /// line and samples stops linearly (or repeats when `repeating = true`).
    ///
    /// Emitted by `emit_background_image` for `BackgroundImage::Gradient(
    /// ParsedGradient::Linear { … })`. P2 renderer implements the actual
    /// GPU-side gradient fill. Coordinate: after FillRect (bg-color), before
    /// border per CSS Backgrounds L3 §3.10 painting order.
    DrawLinearGradient {
        rect: Rect,
        /// CSS degrees clockwise from "to top".
        angle_deg: f32,
        stops: Vec<GradientStop>,
        repeating: bool,
    },
    /// CSS Images L3 §3.3 — `radial-gradient(...)`.
    ///
    /// Elliptical gradient centred at `(center_x_pct, center_y_pct)` in
    /// box-relative coordinates ([0,1] = [left/top, right/bottom]).
    /// Renderer maps stops along the radius to the box extents.
    DrawRadialGradient {
        rect: Rect,
        center_x_pct: f32,
        center_y_pct: f32,
        /// Horizontal radius of the ending shape in CSS px (`radius_x == radius_y`
        /// for a `circle`). Resolved from the CSS shape/size keywords against the
        /// box by [`lumen_layout::radial_gradient_radii`] (CSS Images L3 §3.5).
        radius_x: f32,
        /// Vertical radius of the ending shape in CSS px.
        radius_y: f32,
        stops: Vec<GradientStop>,
        repeating: bool,
    },
    /// CSS Images L4 §3.7 — `conic-gradient(...)`.
    ///
    /// Angular gradient revolving clockwise around `(center_x_pct,
    /// center_y_pct)` in box-relative coordinates ([0,1] = [left/top,
    /// right/bottom]). `from_angle_deg` is the starting angle in CSS
    /// degrees (0° = top, 90° = right, clockwise). Stops' positions are
    /// percentages where 100% = a full revolution (angle stops are
    /// pre-converted to percent on parse).
    DrawConicGradient {
        rect: Rect,
        center_x_pct: f32,
        center_y_pct: f32,
        from_angle_deg: f32,
        stops: Vec<GradientStop>,
        repeating: bool,
    },
    /// Sprint 0 P2 stub. Открывает rect-клип: все последующие команды до
    /// парного `PopClip` рисуются только в пределах `rect`. Используется
    /// для `overflow: hidden`, `clip-path: inset(...)`. Phase 0: эмиттер
    /// в `build_display_list` не выпускает, renderer игнорирует. Когда
    /// P1 п.2A (stacking contexts impl) заполнит данные, эмиттер начнёт
    /// выпускать; до этого момента — interface-first stub.
    PushClipRect { rect: Rect },
    /// P2 BUG-132 fix: Открывает скруглённый rect-клип с border-radius.
    /// Все последующие команды до парного `PopClip` рисуются только в пределах
    /// скруглённого прямоугольника. Используется для `overflow: hidden`
    /// с `border-radius` (взамен scissor-теста PushClipRect). Каждый
    /// corner определен через `radii[0..4]` (top-left, top-right, bottom-right,
    /// bottom-left). Phase 0: реализация в backends/femtovg_backend.rs.
    PushClipRoundedRect { rect: Rect, radii: [f32; 4] },
    /// BUG-140: открывает клип произвольной basic-shape (`clip-path:
    /// circle/ellipse/polygon`), разрешённой в page-координаты (px,
    /// пространство ДО transform элемента). Эмитится ВНУТРИ
    /// `PushTransform` элемента, чтобы форма переносилась его трансформом
    /// (CSS Masking L1 §9: clip-path задан в локальной системе элемента).
    /// Парный Pop — общий `PopClip`. `inset(...)` без скруглений эмитится
    /// как `PushClipRect` (точно представим прямоугольником).
    PushClipPath { shape: ResolvedClipShape },
    /// Закрывает клип (rect, rounded-rect или shape), открытый ближайшим
    /// `PushClipRect`/`PushClipRoundedRect`/`PushClipPath`. Парность
    /// гарантируется эмиттером.
    PopClip,
    /// Sprint 0 P2 stub. Открывает opacity-группу: все последующие
    /// команды до парного `PopOpacity` композитятся как off-screen-layer
    /// и накладываются с `alpha`. Используется для `opacity != 1`. Phase 0:
    /// эмиттер не выпускает (нужен compositor с layer-pipeline-ом —
    /// roadmap-задача), renderer игнорирует.
    ///
    /// `bounds` — document-space CSS px bbox of the element this group belongs
    /// to (same convention as [`Self::PushBlendMode`]/[`Self::PushFilter`]).
    /// BUG-272 (bbox-layer track): backends use it to skip the whole
    /// offscreen-composite bracket when it lands outside the viewport (same
    /// mechanism as BUG-273 срез 1 for blend groups). `None` — the group has no
    /// element bbox (e.g. a full-page view-transition fade) and is never culled.
    PushOpacity { alpha: f32, bounds: Option<Rect> },
    /// Закрывает opacity-группу.
    PopOpacity,
    /// Открывает blend-группу с указанным режимом смешения
    /// (CSS Compositing & Blending L1 §5). Все последующие команды до
    /// парного `PopBlendMode` применяются поверх родительского контекста
    /// через `mode`. `BlendMode::Normal` — стандартный alpha-over (no-op).
    /// Phase 0: renderer отслеживает стек через `current_blend_mode()`,
    /// но использует Normal pipeline для всех режимов; реальный pipeline
    /// switch — P2 1B.4.
    ///
    /// `bounds` — document-space CSS px bbox of the element this group
    /// belongs to (same convention as [`Self::PushFilter`]/
    /// [`Self::PushBackdropFilter`]). BUG-273 срез 1: backends use it to skip
    /// the whole offscreen-composite bracket when it lands outside the viewport.
    PushBlendMode { mode: BlendMode, bounds: Rect },
    /// Закрывает blend-группу.
    PopBlendMode,
    /// Рисует ранее загруженный GPU-снимок слоя (см. `Renderer::upload_layer_snapshot`)
    /// как текстурированный quad в `rect`. UV покрывает весь снимок ([0,0]→[1,1]).
    /// `alpha` — финальная прозрачность (0.0=прозрачный, 1.0=непрозрачный).
    /// Если снимок с `id` не зарегистрирован — команда молча игнорируется.
    /// Используется compositor-ом для повторного использования неизменных слоёв.
    DrawLayerSnapshot { id: u64, rect: Rect, alpha: f32 },
    /// CSS Masking L1 §4 — открывает mask-группу для URL-изображения.
    /// Содержимое элемента (включая детей) рендерится в offscreen-слой;
    /// `PopMask` применяет mask-image как alpha-маску (channel: alpha).
    /// `src` — тот же ключ, что `Renderer::register_image`. `size`/`repeat` —
    /// аналогично `DrawBackgroundImage`. `position` — `mask-position` (Phase 0:
    /// фиксирован в `0% 0%`, т.к. свойство не парсится). Если изображение не
    /// зарегистрировано в GPU-cache — PopMask composites с alpha=1.0 (без маски).
    PushMaskImage {
        rect: Rect,
        src: String,
        size: BackgroundSize,
        position: ObjectPosition,
        repeat: BackgroundRepeat,
        image_rendering: ImageRendering,
    },
    /// CSS Masking L1 §4 — linear-gradient mask. Offscreen содержимое
    /// composites с alpha, управляемым градиентом.
    /// Phase 0: renderer открывает offscreen-слой; PopMask composites
    /// используя stops для вычисления alpha (gradient direction = angle_deg).
    PushMaskLinearGradient {
        rect: Rect,
        angle_deg: f32,
        stops: Vec<GradientStop>,
        repeating: bool,
    },
    /// CSS Masking L1 §4 — radial-gradient mask.
    PushMaskRadialGradient {
        rect: Rect,
        center_x_pct: f32,
        center_y_pct: f32,
        stops: Vec<GradientStop>,
        repeating: bool,
    },
    /// CSS Masking L1 §4 — conic-gradient mask.
    PushMaskConicGradient {
        rect: Rect,
        center_x_pct: f32,
        center_y_pct: f32,
        from_angle_deg: f32,
        stops: Vec<GradientStop>,
        repeating: bool,
    },
    /// Закрывает mask-группу, открытую ближайшим `PushMask*`. Composites
    /// offscreen-слой с alpha, определённой соответствующим PushMask*.
    PopMask,
    /// CSS Masking L1 §5 — открывает offscreen-слой для **содержимого маски**.
    ///
    /// Команды между `PushMaskLayer` и `PopMaskLayer` рендерятся в отдельный
    /// offscreen-слой; `PopMaskLayer` применяет этот слой как маску к
    /// содержимому **родительского** слоя в пределах `rect`.
    ///
    /// Используется для SVG `<mask>` элементов и `mask: url(#id)` источников,
    /// где маска — произвольный rendered контент (пути, формы, градиенты).
    /// Отличие от `PushMaskImage`: маска рендерится в реальном времени
    /// из произвольного поддерева, а не из статической текстуры.
    ///
    /// `mode` — как извлекать значение маски из rendered слоя (alpha или luminance).
    PushMaskLayer {
        /// Border-box rect маскируемого элемента в CSS-пикселях.
        rect: Rect,
        /// Способ вычисления значения маски из rendered mask-слоя.
        mode: MaskMode,
    },
    /// Закрывает mask-layer, открытый `PushMaskLayer`. Применяет rendered маску
    /// к родительскому слою: `parent_pixel *= mask_value(mask_layer_pixel, mode)`.
    /// Пиксели за пределами `rect` не затрагиваются.
    PopMaskLayer,
    /// CSS Transforms L1 §13 — открывает transform-группу. Все последующие
    /// команды до парного `PopTransform` рисуются с применением `matrix` к
    /// координатам вершин (forward-матрица в viewport-системе, уже включает
    /// `T(pivot)·M·T(-pivot)` по `transform-origin`). Phase 0 — 2D affine:
    /// translate / rotate / scale / skew / matrix2d. Z/W-колонки игнорируются.
    ///
    /// Стек transform-ов в renderer-е перемножается с предыдущим топом, что
    /// корректно отражает CSS-семантику вложенных трансформов (каждый transform
    /// создаёт SC и применяется к собственному поддереву + детям).
    ///
    /// Phase 0 ограничения:
    /// - `PushClipRect` под не-identity transform-ом использует axis-aligned
    ///   bounding box трансформированного rect-а как scissor. Для осевых
    ///   трансформов (translate/scale/flip) этот bbox точен. Под rotate/skew
    ///   он шире самого клипа, и **wgpu**-бэкенд (BUG-277 срез 14) переводит
    ///   такой клип на точный контур через offscreen-уровень; `cpu_raster` и
    ///   femtovg остаются на bbox. Повёрнутый клип со скруглением — на bbox
    ///   везде.
    /// - DrawBorder / DrawOutline эмитят 4 axis-aligned rect-а под стороны;
    ///   при rotate они трансформируются по-отдельности, что выглядит
    ///   корректно для translate/scale, но может рассинхронизировать стыки
    ///   углов при больших углах rotate. Mitre-углы — отдельная задача.
    PushTransform { matrix: Mat4 },
    /// Закрывает transform-группу.
    PopTransform,
    /// CSS Filter Effects L1 §5 — открывает filter-группу. Содержимое до
    /// парного `PopFilter` рендерится в offscreen-слой; при PopFilter
    /// применяются все функции из `filters` в порядке объявления (spec §5.1)
    /// и результат composites в родительский слой.
    ///
    /// Phase 0: color-matrix фильтры (grayscale/sepia/brightness/contrast/
    /// saturate/invert/opacity/hue-rotate) реализованы через GPU-шейдер;
    /// blur реализован через двухпроходный Gaussian GPU-шейдер.
    ///
    /// `bounds` — примерная область, которую займёт отфильтрованное содержимое
    /// (CSS px). Используется для оптимизации размера offscreen-слоя; если None —
    /// fallback на full viewport. Для box-shadow это rect тени; для text-shadow —
    /// bounds текста плюс смещение и blur-spread.
    PushFilter { filters: Vec<FilterFn>, bounds: Option<Rect> },
    /// Закрывает filter-группу.
    PopFilter,
    /// CSS Filter Effects L1 §2 / Compositing L1 §13 — backdrop-filter.
    ///
    /// Открывает stacking-context-слой для элемента. При `PopBackdropFilter`
    /// рендерер:
    ///   1. Копирует содержимое parent-слоя в scratch (backdrop snapshot).
    ///   2. Применяет `filters` к snapshot-у (те же GPU-проходы, что и
    ///      `PushFilter`: Gaussian blur + color-matrix).
    ///   3. Заменяет (REPLACE blend) область `bounds` в parent-слое
    ///      отфильтрованным snapshot-ом.
    ///   4. Composites содержимое element-слоя поверх parent (ALPHA_BLENDING).
    ///
    /// `bounds` — border-box элемента в CSS px (layout-координаты).
    ///
    /// Phase 0 limitation: работает только когда parent-слой является
    /// offscreen layer (from_level > 1). При from_level == 1 (parent =
    /// surface texture) backdrop-filter пропускается — surface texture
    /// не поддерживает TEXTURE_BINDING в текущей конфигурации.
    PushBackdropFilter { filters: Vec<FilterFn>, bounds: Rect },
    /// Закрывает backdrop-filter-группу.
    PopBackdropFilter,
    /// CSS Positioning L3 §6.3 — position:sticky layer.
    ///
    /// All content between `BeginStickyLayer` and `EndStickyLayer` is rendered
    /// with a scroll-clamped offset: the element stays at its normal-flow
    /// position until the scroll would push it past a sticky inset, then it
    /// sticks at that inset until the scroll moves it back.
    ///
    /// `flow_rect` — the element's border-box in normal-flow coordinates
    ///   (absolute page coords, same coordinate system as all other rects in
    ///   the display list).
    /// `top` / `bottom` / `left` / `right` — resolved sticky insets in CSS px
    ///   (`None` = `auto`, no constraint on that side).
    ///
    /// Renderer computes `sticky_dy = clamp(-scroll_y, top - flow_y, …)` at
    /// draw time so the layer stays viewport-relative.
    BeginStickyLayer {
        /// Element's border-box in normal-flow (page) coordinates.
        flow_rect: lumen_core::geom::Rect,
        /// Distance from the top of the viewport to stick at. `None` = auto.
        top: Option<f32>,
        /// Distance from the bottom of the viewport to stick at. `None` = auto.
        bottom: Option<f32>,
        /// Distance from the left of the viewport to stick at. `None` = auto.
        left: Option<f32>,
        /// Distance from the right of the viewport to stick at. `None` = auto.
        right: Option<f32>,
    },
    /// Closes the sticky layer opened by `BeginStickyLayer`.
    EndStickyLayer,
    /// CSS Positioning L3 §6.1 — position:fixed layer marker (ADR-016 M3.2.1c).
    ///
    /// A **pure bracket** with no payload: it marks where a `position:fixed`
    /// element (and its subtree) begins in the scroll-independent display list so
    /// the compositor scroll-blit fast path can split it out of the scrollable
    /// band and redraw it per frame (see [`overlay_ranges`]). Unlike
    /// `BeginStickyLayer` it carries **no** insets and applies **no** draw-time
    /// offset: fixed content is already placed at its viewport-fixed coordinates
    /// by layout (BUG-159 keeps it from inheriting the scroll translate), so every
    /// backend renders this marker as a no-op. It exists solely as partition
    /// metadata for the overlay layer.
    ///
    /// [`overlay_ranges`]: crate::overlay_partition::overlay_ranges
    BeginFixedLayer,
    /// Closes the fixed layer opened by `BeginFixedLayer`. No-op in every backend.
    EndFixedLayer,
    /// CSS Overflow L3 §3.2 — `overflow: scroll` / `overflow: auto` scroll region.
    ///
    /// Clips rendering to `clip_rect` (padding-box of the container) and translates
    /// all content by `(-scroll_x, -scroll_y)`. Renderer: pushes `clip_rect` onto the
    /// clip stack (GPU scissor) and pushes a `translation_2d(-scroll_x, -scroll_y)` onto
    /// the transform stack. `PopScrollLayer` unwinds both.
    ///
    /// Emitter sets `scroll_x`/`scroll_y` from `LayoutBox.scroll_x/scroll_y`, which
    /// the shell updates via `set_scroll_position()` on wheel/touch events.
    ///
    /// # CSS: overflow
    /// P4 wires: in `box_layer_ops` replace the `PushClipRect` for `Overflow::Scroll|Auto`
    /// with `PushScrollLayer { clip_rect, scroll_x: b.scroll_x, scroll_y: b.scroll_y }`.
    PushScrollLayer {
        /// Padding-box of the scroll container in CSS px (document-relative).
        clip_rect: Rect,
        /// Horizontal scroll offset in CSS px. Content is shifted left by this amount.
        scroll_x: f32,
        /// Vertical scroll offset in CSS px. Content is shifted up by this amount.
        scroll_y: f32,
    },
    /// Closes the scroll layer opened by `PushScrollLayer`. Pops the transform
    /// (scroll translate) first, then the clip.
    PopScrollLayer,
    /// SVG `<path>` fill: pre-tessellated triangle list produced by
    /// `svg_path::tessellate_fill`. Every 3 consecutive `[x, y]` entries
    /// form one triangle in CSS-pixel coordinates (same coordinate system as
    /// all other rects in the display list). Color is the resolved `fill`
    /// value after opacity.
    ///
    /// CSS: fill, stroke — P4 wires once fill/stroke are in ComputedStyle.
    DrawSvgPath {
        /// Flat list of triangle vertices — length is always a multiple of 3.
        vertices: Vec<[f32; 2]>,
        /// Resolved fill colour (already has `fill-opacity` applied).
        color: Color,
    },
    /// SVG `<path>`/`<polygon>` **nonzero** area fill, given as the raw closed
    /// outline contours instead of a pre-tessellated triangle soup (BUG-247 /
    /// BUG-173). Backends that own an analytic rasteriser (femtovg, tiny_skia
    /// CPU) fill these contours natively, so anti-aliasing is applied only on
    /// the true shape boundary — a triangle soup made femtovg/tiny_skia fringe
    /// every *internal* shared edge, producing ~1px seams across the fill that
    /// diverged from Edge. The GPU/wgpu backend, which has no native path fill,
    /// tessellates these contours with `svg_path::tessellate_fill` and renders
    /// the resulting triangles — bit-identical to the old `DrawSvgPath` fill.
    ///
    /// Filled with the **nonzero** winding rule (each contour keeps its source
    /// direction, so holes wound opposite to the outer ring are honoured).
    /// `fill-rule: evenodd` is *not* routed here — it stays on `DrawSvgPath`
    /// via `svg_path::tessellate_fill_even_odd` (femtovg/wgpu have no even-odd
    /// path-fill mode).
    DrawSvgFill {
        /// Closed sub-path outlines in CSS-pixel page coordinates (same system
        /// as all other rects). Already shifted into document space.
        contours: Vec<Vec<[f32; 2]>>,
        /// Resolved fill colour (already has `fill-opacity` applied).
        color: Color,
    },
    /// SVG `<path>` **stroke** given as the raw source contours plus the full
    /// stroke parameters, instead of a pre-tessellated triangle soup (BUG-247).
    /// Backends that own an analytic stroker (femtovg) stroke these contours
    /// natively, so anti-aliasing lands only on the true stroke boundary — the
    /// old `DrawSvgPath` triangle soup made femtovg fringe every *internal*
    /// shared edge, producing ~1px seams along curved and dashed strokes that
    /// diverged from Edge (the dominant TEST-134 dash / TEST-136 curve error).
    /// Backends with no native stroker (CPU tiny_skia, GPU/wgpu) call
    /// `svg_path::tessellate_stroke_ex` on the same contours and render the
    /// resulting triangles — bit-identical to the old `DrawSvgPath` stroke.
    ///
    /// The contours are already shifted into document space; dash splitting is
    /// deferred to the backend (`params.dasharray`/`dashoffset`) so the native
    /// stroker and the tessellating fallback dash identically.
    DrawSvgStroke {
        /// Source stroke contours (flattened polylines) in CSS-pixel page
        /// coordinates. A contour whose first point equals its last is closed.
        contours: Vec<Vec<[f32; 2]>>,
        /// Resolved stroke colour (already has `stroke-opacity` applied).
        color: Color,
        /// Width, caps, joins, miter limit and dash pattern.
        params: crate::svg_path::StrokeParams,
    },
    /// DevTools box model overlay (7E.3). Draws four semi-transparent coloured
    /// layers (orange margin, yellow border, green padding, blue content)
    /// stacked from outermost to innermost. Each rect is the outer edge of
    /// the corresponding box (margin-edge, border-edge, padding-edge, content).
    ///
    /// Coordinate system: same CSS-pixel page coordinates as all other rects.
    BoxModelOverlay {
        /// Outer edge of the margin box (border-box + margin on all sides).
        margin: Rect,
        /// Outer edge of the border box (padding-box + border on all sides).
        border: Rect,
        /// Outer edge of the padding box (content-box + padding on all sides).
        padding: Rect,
        /// Content box rect.
        content: Rect,
    },
    /// Scrollbar track and thumb for an `overflow: scroll` / `overflow: auto`
    /// container. Drawn in document-space CSS px, outside the scroll layer so
    /// it does not translate with scrolled content.
    ///
    /// Colors and gutter width come from `ComputedStyle.scrollbar_color` /
    /// `scrollbar_width` (CSS Scrollbars L1). `scrollbar-width: none` suppresses
    /// this command entirely — the scroll container still scrolls, just invisibly.
    DrawScrollbar {
        /// Full track rectangle (document-space CSS px). Fills the scrollbar gutter.
        track_rect: Rect,
        /// Thumb rectangle inside the track (document-space CSS px). Proportional
        /// to viewport/content ratio and positioned by current scroll offset.
        thumb_rect: Rect,
        /// `true` = vertical scrollbar (right edge); `false` = horizontal (bottom edge).
        vertical: bool,
        /// Thumb fill color in linear-light sRGB [r, g, b, a] (pre-multiplied alpha not used).
        thumb_color: [f32; 4],
        /// Track fill color in linear-light sRGB [r, g, b, a].
        track_color: [f32; 4],
    },

    /// Marks a page boundary in a print display list.
    ///
    /// Used by `build_print_display_list` to separate pages. The renderer treats this
    /// as a split point: commands before `PageBreak` render on page N, commands after
    /// render on page N+1. Has no visual effect in on-screen rendering.
    PageBreak,

    /// CSS Images L4 §4 — `cross-fade(image-a, image-b, progress%)`.
    ///
    /// GPU two-texture blend: samples `src_a` and `src_b` at the same UV (covers
    /// the full destination rect [0,1]×[0,1]) and outputs
    /// `mix(color_a, color_b, progress)` per pixel. Equivalent to the spec's
    /// linear interpolation between two image samples with no extra alpha
    /// scaling on the result — straight-alpha inputs are blended, then the
    /// result is treated as the source colour for normal premultiplied alpha
    /// compositing onto the destination.
    ///
    /// `dest` — destination rectangle in CSS-pixel page coordinates (same
    /// coordinate system as all other rects in the display list).
    ///
    /// `src_a` / `src_b` — image URLs registered through
    /// [`Renderer::register_image`](crate::Renderer::register_image). If either
    /// texture is missing from the GPU cache, the renderer silently skips the
    /// command (analogous to `DrawBackgroundImage` for an unregistered URL) —
    /// callers may emit a fallback `FillRect` or placeholder beforehand.
    ///
    /// `progress` — blend factor in `[0.0, 1.0]`. `0.0` = fully `src_a`,
    /// `1.0` = fully `src_b`. Values outside the range are clamped by the
    /// renderer (the WGSL `mix` would extrapolate otherwise). Emitters should
    /// already clamp at parse time per CSS Images L4 §4.2.
    ///
    /// CSS: `image()` / `cross-fade()` source for `background-image`,
    /// `mask-image`, `border-image-source`, `list-style-image`, content
    /// property values. P4 wires the emit side once `cross-fade()` is parsed
    /// in `lumen-css-parser` into a `BackgroundImage::CrossFade { a, b, t }`
    /// variant and `emit_background_image` produces this command.
    DrawCrossFade {
        /// Destination rectangle (CSS-pixel page coordinates).
        dest: Rect,
        /// URL key of the first image (`progress = 0.0`).
        src_a: String,
        /// URL key of the second image (`progress = 1.0`).
        src_b: String,
        /// Blend factor in `[0.0, 1.0]`. `0.0` = pure `src_a`, `1.0` = pure `src_b`.
        progress: f32,
    },
}

impl DisplayCommand {
    /// Имя варианта команды для диагностики (`LUMEN_FRAME_LOG=2`:
    /// разбивка времени paint-фазы по типам команд).
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::FillRect { .. } => "FillRect",
            Self::FillRoundedRect { .. } => "FillRoundedRect",
            Self::DrawBorder { .. } => "DrawBorder",
            Self::DrawOutline { .. } => "DrawOutline",
            Self::DrawText { .. } => "DrawText",
            Self::DrawImage { .. } => "DrawImage",
            Self::LazyImageSlot { .. } => "LazyImageSlot",
            Self::DrawBackgroundImage { .. } => "DrawBackgroundImage",
            Self::DrawLinearGradient { .. } => "DrawLinearGradient",
            Self::DrawRadialGradient { .. } => "DrawRadialGradient",
            Self::DrawConicGradient { .. } => "DrawConicGradient",
            Self::PushClipRect { .. } => "PushClipRect",
            Self::PushClipRoundedRect { .. } => "PushClipRoundedRect",
            Self::PushClipPath { .. } => "PushClipPath",
            Self::PopClip => "PopClip",
            Self::PushOpacity { .. } => "PushOpacity",
            Self::PopOpacity => "PopOpacity",
            Self::PushBlendMode { .. } => "PushBlendMode",
            Self::PopBlendMode => "PopBlendMode",
            Self::DrawLayerSnapshot { .. } => "DrawLayerSnapshot",
            Self::PushMaskImage { .. } => "PushMaskImage",
            Self::PushMaskLinearGradient { .. } => "PushMaskLinearGradient",
            Self::PushMaskRadialGradient { .. } => "PushMaskRadialGradient",
            Self::PushMaskConicGradient { .. } => "PushMaskConicGradient",
            Self::PushMaskLayer { .. } => "PushMaskLayer",
            Self::PopMaskLayer => "PopMaskLayer",
            Self::PopMask => "PopMask",
            Self::PushTransform { .. } => "PushTransform",
            Self::PopTransform => "PopTransform",
            Self::PushFilter { .. } => "PushFilter",
            Self::PopFilter => "PopFilter",
            Self::PushBackdropFilter { .. } => "PushBackdropFilter",
            Self::PopBackdropFilter => "PopBackdropFilter",
            Self::BeginStickyLayer { .. } => "BeginStickyLayer",
            Self::EndStickyLayer => "EndStickyLayer",
            Self::BeginFixedLayer => "BeginFixedLayer",
            Self::EndFixedLayer => "EndFixedLayer",
            Self::PushScrollLayer { .. } => "PushScrollLayer",
            Self::PopScrollLayer => "PopScrollLayer",
            Self::DrawSvgPath { .. } => "DrawSvgPath",
            Self::DrawSvgFill { .. } => "DrawSvgFill",
            Self::DrawSvgStroke { .. } => "DrawSvgStroke",
            Self::BoxModelOverlay { .. } => "BoxModelOverlay",
            Self::DrawScrollbar { .. } => "DrawScrollbar",
            Self::DrawCrossFade { .. } => "DrawCrossFade",
            Self::PageBreak => "PageBreak",
        }
    }

    /// Axis-aligned bounding box of everything this command paints, in
    /// document-space CSS px (the same coordinate system the command's own
    /// rects already use — *before* the scroll/transform translation a backend
    /// applies at draw time).
    ///
    /// Returns `Some(rect)` only for **self-contained leaf draws**: commands
    /// that paint nothing outside the returned box and have no effect on the
    /// clip / transform / layer stack. Backends use it for viewport culling
    /// (ADR-016 M0.2) — a leaf whose box, mapped through the current CTM, lands
    /// fully outside the viewport can be skipped without changing the picture.
    ///
    /// Returns `None` for every structural command (`Push*` / `Pop*`, the
    /// sticky / scroll layer markers, `PageBreak`): those must always execute
    /// to keep the render stack balanced and must never be culled. `None` is
    /// the safe default — an unrecognised or non-leaf command is simply never
    /// skipped.
    pub fn cull_rect(&self) -> Option<Rect> {
        /// Inflate a rect by `d` CSS px on every side.
        fn grow(r: Rect, d: f32) -> Rect {
            Rect::new(r.x - d, r.y - d, r.width + 2.0 * d, r.height + 2.0 * d)
        }
        /// AABB of a flat `[x, y]` vertex list, or `None` if empty.
        fn verts_bounds(pts: &[[f32; 2]]) -> Option<Rect> {
            points_bounds(pts.iter().copied())
        }
        /// AABB of any sequence of points — the contour variants stream their
        /// points through here instead of flattening into a temporary `Vec`
        /// (BUG-405 срез 16: `cull_rect` runs on every command of every frame,
        /// and that allocation was 0.4 ms per scroll run of `lenta.ru`, ~10 %
        /// of what a `DrawSvgStroke` command costs).
        fn points_bounds(pts: impl Iterator<Item = [f32; 2]>) -> Option<Rect> {
            let (mut mn_x, mut mn_y) = (f32::MAX, f32::MAX);
            let (mut mx_x, mut mx_y) = (f32::MIN, f32::MIN);
            for p in pts {
                mn_x = mn_x.min(p[0]);
                mn_y = mn_y.min(p[1]);
                mx_x = mx_x.max(p[0]);
                mx_y = mx_y.max(p[1]);
            }
            (mn_x <= mx_x).then(|| {
                Rect::new(mn_x, mn_y, (mx_x - mn_x).max(0.0), (mx_y - mn_y).max(0.0))
            })
        }
        match self {
            Self::FillRect { rect, .. }
            | Self::FillRoundedRect { rect, .. }
            | Self::DrawBorder { rect, .. }
            | Self::DrawText { rect, .. }
            | Self::DrawImage { rect, .. }
            | Self::LazyImageSlot { rect, .. }
            | Self::DrawBackgroundImage { rect, .. }
            | Self::DrawLinearGradient { rect, .. }
            | Self::DrawRadialGradient { rect, .. }
            | Self::DrawConicGradient { rect, .. }
            | Self::DrawLayerSnapshot { rect, .. } => Some(*rect),

            Self::DrawCrossFade { dest, .. } => Some(*dest),
            Self::BoxModelOverlay { margin, .. } => Some(*margin),

            // `outline` paints *outside* the box by `offset` then `width`.
            Self::DrawOutline { rect, width, offset, .. } => {
                Some(grow(*rect, offset.max(0.0) + width.max(0.0)))
            }

            // Scrollbar spans both track and thumb.
            Self::DrawScrollbar { track_rect, thumb_rect, .. } => Some(Rect::new(
                track_rect.x.min(thumb_rect.x),
                track_rect.y.min(thumb_rect.y),
                (track_rect.x + track_rect.width)
                    .max(thumb_rect.x + thumb_rect.width)
                    - track_rect.x.min(thumb_rect.x),
                (track_rect.y + track_rect.height)
                    .max(thumb_rect.y + thumb_rect.height)
                    - track_rect.y.min(thumb_rect.y),
            )),

            // SVG geometry: bound the raw contour / triangle vertices. Stroke
            // paints `half_width` outside the path centreline, so inflate by it
            // times the miter limit (a conservative bound on miter spikes).
            Self::DrawSvgPath { vertices, .. } => verts_bounds(vertices),
            Self::DrawSvgFill { contours, .. } => {
                points_bounds(contours.iter().flatten().copied())
            }
            Self::DrawSvgStroke { contours, params, .. } => {
                let out = params.half_width.max(0.0) * params.miterlimit.max(1.0);
                points_bounds(contours.iter().flatten().copied()).map(|r| grow(r, out))
            }

            // Structural / stack-affecting / no-op commands — never cull.
            _ => None,
        }
    }
}

pub type DisplayList = Vec<DisplayCommand>;

/// Provenance for a display list (ADR-025 §3): a side index, not a field on
/// `DisplayCommand`. Answers "which layout box produced this command" without
/// touching the ~40-variant enum rebuilt every frame.
#[derive(Debug, Clone, Default)]
pub struct ProvenanceIndex {
    pub(crate) spans: Vec<ProvenanceSpan>,
}

impl ProvenanceIndex {
    /// All spans, in emission order. Not sorted by range — a box's spans can
    /// be interleaved with unrelated boxes' spans (see `ProvenanceSpan` docs).
    pub fn spans(&self) -> &[ProvenanceSpan] {
        &self.spans
    }

    /// Spans produced by exactly this origin — the primitive `explain_element`
    /// (DEVX-10) answers "which commands did this node produce" with.
    pub fn spans_for(&self, origin: BoxOrigin) -> impl Iterator<Item = &ProvenanceSpan> {
        self.spans.iter().filter(move |s| s.origin == origin)
    }
}

/// One contiguous run of commands produced by a single layout box's own
/// paint (ADR-025 §3). A box with descendants owns *more than one* span in
/// general: its own background/border is emitted before its children and its
/// closing layer-ops after them, with the children's own spans sitting in
/// between — `range` is contiguous, but "all of this box's spans" is not.
/// This is the resolution of the `p1-introspection-track.md` DEVX-7 finding
/// that `Range<usize>` cannot describe a *box*, only one of its spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceSpan {
    /// Half-open range into the final command list.
    pub range: Range<usize>,
    pub origin: BoxOrigin,
    /// Fragment index within the box — line box / column / page break.
    /// MVP: always `0` (the engine does not fragment single spans yet beyond
    /// the stacking-context bucket phases already captured by having several
    /// `ProvenanceSpan`s per box).
    pub fragment: u32,
    /// Number of open rect/rounded-rect/path clips at this span's first
    /// command — pairs with `PushClipRect`/`PushClipRoundedRect`/
    /// `PushClipPath` vs. `PopClip`. Scroll layers (`PushScrollLayer`/
    /// `PopScrollLayer`) are a separate stack and not counted here.
    pub clip_depth: u16,
}

fn object_fit_name(f: ObjectFit) -> &'static str {
    match f {
        ObjectFit::Fill => "fill",
        ObjectFit::Contain => "contain",
        ObjectFit::Cover => "cover",
        ObjectFit::None => "none",
        ObjectFit::ScaleDown => "scale-down",
    }
}

fn position_component_name(p: PositionComponent) -> String {
    match p {
        PositionComponent::Px(px) => format!("{px:.2}px"),
        PositionComponent::Percent(pc) => format!("{:.2}%", pc * 100.0),
    }
}

/// CSS Images L3 §5.5 — `object-fit` placement: где располагается
/// «полное» изображение внутри коробки (intrinsic-картинка после scale,
/// без обрезки). Возвращённый прямоугольник может быть больше `box_rect`
/// (cover / none на крупной картинке) — обрезку по box делает
/// [`fit_image_quad`] на стадии вычисления GPU-quad-а.
///
/// `intrinsic_size = (w, h)` — натуральный пиксельный размер декодированного
/// изображения; нулевые / отрицательные стороны коробки → возврат самой
/// коробки без масштабирования (deg fallback, рисовать всё равно нечего).
#[must_use]
pub fn fit_image_rect(
    box_rect: Rect,
    intrinsic_size: (u32, u32),
    fit: ObjectFit,
    position: ObjectPosition,
) -> Rect {
    let (iw, ih) = intrinsic_size;
    if iw == 0 || ih == 0 || box_rect.width <= 0.0 || box_rect.height <= 0.0 {
        return box_rect;
    }
    let iw = iw as f32;
    let ih = ih as f32;
    let bw = box_rect.width;
    let bh = box_rect.height;

    let (cw, ch) = match fit {
        ObjectFit::Fill => (bw, bh),
        ObjectFit::None => (iw, ih),
        ObjectFit::Contain => fit_with_ratio(iw, ih, bw, bh, /*cover*/ false),
        ObjectFit::Cover => fit_with_ratio(iw, ih, bw, bh, /*cover*/ true),
        ObjectFit::ScaleDown => {
            // `min(none, contain)` — выбираем результат с меньшей площадью.
            let (nw, nh) = (iw, ih);
            let (kw, kh) = fit_with_ratio(iw, ih, bw, bh, false);
            if nw * nh <= kw * kh { (nw, nh) } else { (kw, kh) }
        }
    };

    let free_x = bw - cw;
    let free_y = bh - ch;
    let off_x = position.x.resolve(free_x);
    let off_y = position.y.resolve(free_y);
    Rect::new(box_rect.x + off_x, box_rect.y + off_y, cw, ch)
}

fn fit_with_ratio(iw: f32, ih: f32, bw: f32, bh: f32, cover: bool) -> (f32, f32) {
    // contain = min(scale_w, scale_h); cover = max(...).
    let sx = bw / iw;
    let sy = bh / ih;
    let s = if cover { sx.max(sy) } else { sx.min(sy) };
    (iw * s, ih * s)
}

/// One classified run of a `text-orientation: mixed` string (CSS Writing
/// Modes L4 §4): a CJK ideograph paints upright (no rotation, stacked below
/// the previous glyph); a run of consecutive non-CJK characters (Latin,
/// digits, punctuation, whitespace) paints as one rotated block so kerning
/// and ligatures inside a Latin word stay intact. Produced by
/// [`split_mixed_runs`]; consumed by the CPU rasterizer
/// (`cpu_raster::rasterize_text_mixed`), the wgpu renderer
/// (`renderer::push_text_glyphs_mixed`) and the femtovg backend
/// (`femtovg_backend::FemtovgBackend::draw_text_mixed`) — every backend
/// rotates glyphs, so the CJK/Latin split rule lives here once.
#[cfg(any(feature = "backend-wgpu", feature = "cpu-render", feature = "backend-femtovg"))]
pub(crate) enum MixedSegment {
    /// A single CJK ideograph, rendered upright.
    Cjk(char),
    /// A run of consecutive non-CJK characters, rendered as one rotated block.
    Other(String),
}

/// Splits `text` into [`MixedSegment`]s for `text-orientation: mixed` paint —
/// see that type's docs for the CJK/Latin split rule.
#[cfg(any(feature = "backend-wgpu", feature = "cpu-render", feature = "backend-femtovg"))]
pub(crate) fn split_mixed_runs(text: &str) -> Vec<MixedSegment> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in text.chars() {
        if lumen_layout::vertical::is_cjk(ch) {
            if !buf.is_empty() {
                out.push(MixedSegment::Other(std::mem::take(&mut buf)));
            }
            out.push(MixedSegment::Cjk(ch));
        } else {
            buf.push(ch);
        }
    }
    if !buf.is_empty() {
        out.push(MixedSegment::Other(buf));
    }
    out
}

/// Per-axis tiling geometry for `background-repeat: space` /
/// `mask-repeat: space` (CSS Backgrounds L3 §3.4, CSS Masking L1 §4.4).
///
/// Given the positioning-area leading edge `area_origin`, its extent `area`
/// along the axis, the tile size `tile`, and the `position` offset `pos_off`
/// (from the leading edge), returns `(start, step, repeat)`:
/// * `start` — absolute coordinate of the first tile's leading edge;
/// * `step` — distance between successive tile origins (tile size + gap);
/// * `repeat` — whether more than one tile is laid out along the axis.
///
/// When two or more whole tiles fit, the first and last are pinned to the two
/// edges and the leftover space is distributed evenly as equal gaps (the
/// `position` offset is ignored on that axis, per spec). When at most one whole
/// tile fits, a single tile is placed at the `position` offset and the axis does
/// not repeat (identical to `no-repeat`).
///
/// Shared by every tiling path (femtovg + CPU via [`bg_tile_geometry`], and the
/// GPU renderer's inline background/mask loops) so `space` places tiles
/// identically everywhere.
#[must_use]
pub(crate) fn space_axis_geometry(
    area_origin: f32,
    area: f32,
    tile: f32,
    pos_off: f32,
) -> (f32, f32, bool) {
    if tile > 0.0 {
        let n = (area / tile).floor();
        if n >= 2.0 {
            let gap = (area - n * tile) / (n - 1.0);
            return (area_origin, tile + gap, true);
        }
    }
    (area_origin + pos_off, tile, false)
}

/// Tile geometry for a background image from `background-size` /
/// `background-position` / `background-repeat` (CSS Backgrounds L3 §3.3–3.5).
///
/// Pure (GL-free) so both the femtovg backend and the deterministic CPU
/// rasterizer derive identical placement. `img_w`/`img_h` — intrinsic image
/// size; `oarea_*` — the `background-origin` positioning area (x/y/width/height).
///
/// Returns `(tile_w, tile_h, tile_x_start, tile_y_start, repeat_x, repeat_y,
/// step_x, step_y)`: one tile's size, the top-left corner of the first tile, the
/// per-axis repeat flags, and the per-axis step between successive tile origins.
/// The caller tiles from `(tile_x_start, tile_y_start)` across the painting area,
/// stepping by `(step_x, step_y)` while the corresponding repeat flag is set,
/// clipping to the painting rect. `step_*` equals `tile_*` for every repeat mode
/// except `space`, where it includes the inter-tile gap (CSS Backgrounds L3 §3.4).
// BUG-235: only the femtovg window and the tiny-skia CPU snapshot tile
// backgrounds via this helper; the wgpu renderer tiles on the GPU. Gate it to
// its consumers so a wgpu-only build (e.g. lumen-driver default features) does
// not flag it as dead code under `-D warnings`.
#[cfg(any(feature = "backend-femtovg", feature = "cpu-render"))]
#[allow(clippy::too_many_arguments)]
#[must_use]
pub(crate) fn bg_tile_geometry(
    size: BackgroundSize,
    position: &ObjectPosition,
    repeat: BackgroundRepeat,
    img_w: f32,
    img_h: f32,
    oarea_w: f32,
    oarea_h: f32,
    oarea_x: f32,
    oarea_y: f32,
) -> (f32, f32, f32, f32, bool, bool, f32, f32) {
    let (tile_w, tile_h) = match size {
        BackgroundSize::Auto => (img_w, img_h),
        BackgroundSize::Cover => {
            let s = (oarea_w / img_w).max(oarea_h / img_h);
            (img_w * s, img_h * s)
        }
        BackgroundSize::Contain => {
            let s = (oarea_w / img_w).min(oarea_h / img_h);
            (img_w * s, img_h * s)
        }
        BackgroundSize::Length(w, h) => {
            // CSS Backgrounds L3 §3.5: percent axes resolve against the
            // positioning area; an `auto` axis derives from the other via the
            // image's intrinsic aspect ratio.
            match (w.resolve(oarea_w), h.resolve(oarea_h)) {
                (Some(tw), Some(th)) => (tw.max(1.0), th.max(1.0)),
                (Some(tw), None) => {
                    let tw = tw.max(1.0);
                    (tw, (img_h * (tw / img_w)).max(1.0))
                }
                (None, Some(th)) => {
                    let th = th.max(1.0);
                    ((img_w * (th / img_h)).max(1.0), th)
                }
                (None, None) => (img_w, img_h),
            }
        }
    };

    // CSS Backgrounds L3 §3.4 — `round`: rescale the tile so a whole number of
    // copies exactly fills the positioning area along each axis (no clipped
    // partial tiles at the far edge). `n = max(1, round(area / tile))`, then the
    // tile is stretched to `area / n`. Both axes are rounded independently, which
    // may distort the aspect ratio — matching the reference rendering (the spec's
    // "note" explicitly permits distortion when only one axis, or a size-auto
    // axis, is involved). Applied before offset resolution so percentage
    // positions resolve against the rounded tile size.
    let (tile_w, tile_h) = if repeat == BackgroundRepeat::Round {
        let round_axis = |area: f32, tile: f32| -> f32 {
            if tile > 0.0 && area > 0.0 {
                let n = (area / tile).round().max(1.0);
                area / n
            } else {
                tile
            }
        };
        (round_axis(oarea_w, tile_w), round_axis(oarea_h, tile_h))
    } else {
        (tile_w, tile_h)
    };

    let off_x = match position.x {
        PositionComponent::Px(px) => px,
        PositionComponent::Percent(p) => (oarea_w - tile_w) * p,
    };
    let off_y = match position.y {
        PositionComponent::Px(py) => py,
        PositionComponent::Percent(p) => (oarea_h - tile_h) * p,
    };
    let tile_x0 = oarea_x + off_x;
    let tile_y0 = oarea_y + off_y;

    let (tile_x_start, step_x, repeat_x, tile_y_start, step_y, repeat_y) = match repeat {
        BackgroundRepeat::NoRepeat => (tile_x0, tile_w, false, tile_y0, tile_h, false),
        BackgroundRepeat::RepeatX => (
            tile_x0 - (off_x / tile_w).ceil() * tile_w,
            tile_w,
            true,
            tile_y0,
            tile_h,
            false,
        ),
        BackgroundRepeat::RepeatY => (
            tile_x0,
            tile_w,
            false,
            tile_y0 - (off_y / tile_h).ceil() * tile_h,
            tile_h,
            true,
        ),
        BackgroundRepeat::Repeat | BackgroundRepeat::Round => (
            tile_x0 - (off_x / tile_w).ceil() * tile_w,
            tile_w,
            true,
            tile_y0 - (off_y / tile_h).ceil() * tile_h,
            tile_h,
            true,
        ),
        BackgroundRepeat::Space => {
            let (sx, step_x, rx) = space_axis_geometry(oarea_x, oarea_w, tile_w, off_x);
            let (sy, step_y, ry) = space_axis_geometry(oarea_y, oarea_h, tile_h, off_y);
            (sx, step_x, rx, sy, step_y, ry)
        }
    };

    (tile_w, tile_h, tile_x_start, tile_y_start, repeat_x, repeat_y, step_x, step_y)
}

/// Финальный GPU-quad для `<img>`: пересечение «полного» placement-rect
/// (см. [`fit_image_rect`]) с `box_rect` плюс соответствующие UV-bounds
/// исходной текстуры. Спецификация CSS Images L3 §5.5 требует «clipped to
/// the content box» — для cover / none, когда картинка выходит за коробку,
/// мы делаем clip через UV (рисуем меньший quad с поджатыми UV), без
/// scissor-state в GPU pipeline.
///
/// Возвращает `None`, если intrinsic-размер нулевой, коробка пуста или
/// пересечение placement и box пусто (placement полностью снаружи box —
/// в норме не случается, но возможны deg-edge с отрицательным
/// `object-position`).
#[must_use]
pub fn fit_image_quad(
    box_rect: Rect,
    intrinsic_size: (u32, u32),
    fit: ObjectFit,
    position: ObjectPosition,
) -> Option<(Rect, [f32; 2], [f32; 2])> {
    let (iw, ih) = intrinsic_size;
    if iw == 0 || ih == 0 || box_rect.width <= 0.0 || box_rect.height <= 0.0 {
        return None;
    }
    let placed = fit_image_rect(box_rect, intrinsic_size, fit, position);
    if placed.width <= 0.0 || placed.height <= 0.0 {
        return None;
    }
    let bx0 = box_rect.x;
    let by0 = box_rect.y;
    let bx1 = box_rect.x + box_rect.width;
    let by1 = box_rect.y + box_rect.height;
    let px0 = placed.x;
    let py0 = placed.y;
    let px1 = placed.x + placed.width;
    let py1 = placed.y + placed.height;
    let vx0 = px0.max(bx0);
    let vy0 = py0.max(by0);
    let vx1 = px1.min(bx1);
    let vy1 = py1.min(by1);
    if vx1 <= vx0 || vy1 <= vy0 {
        return None;
    }
    let visible = Rect::new(vx0, vy0, vx1 - vx0, vy1 - vy0);
    let u0 = (vx0 - px0) / placed.width;
    let v0 = (vy0 - py0) / placed.height;
    let u1 = (vx1 - px0) / placed.width;
    let v1 = (vy1 - py0) / placed.height;
    Some((visible, [u0, v0], [u1, v1]))
}

/// Сериализует display list в детерминированный текст для snapshot-тестов.
///
/// Формат (одна команда — одна строка):
/// - `FillRect (x.xx, y.xx, w.xx, h.xx) #rrggbbaa`
/// - `DrawBorder (x.xx, y.xx, w.xx, h.xx) w=[t,r,b,l] c=[#top,#right,#bottom,#left]`
///   плюс `s=[t,r,b,l]` если хоть один стиль ≠ Solid (bw-compat: чистый
///   Solid-border печатается как раньше, snapshot-ы не ломаются).
/// - `DrawText (x.xx, y.xx, w.xx, h.xx) "text" fs.xx #rrggbbaa`
///
/// Сокращённый префикс `BorderStyle` для snapshot-сериализатора.
/// None уже фильтруется emit-side, но обрабатываем для устойчивости.
fn border_style_short(s: BorderStyle) -> &'static str {
    match s {
        BorderStyle::None => "n",
        BorderStyle::Solid => "s",
        BorderStyle::Dashed => "da",
        BorderStyle::Dotted => "do",
        BorderStyle::Double => "db",
    }
}

/// Returns `true` if the display list contains any `backdrop-filter` element.
///
/// Cull a display list to only commands that intersect the given tile region.
///
/// `tile_x` and `tile_y` are tile-space coordinates; the tile covers CSS pixels
/// `[tile_x*tile_size, (tile_x+1)*tile_size) × [tile_y*tile_size, (tile_y+1)*tile_size)`.
///
/// Commands that carry a bounding rect are included only when their rect
/// overlaps the tile (AABB test). State commands (`PushClipRect`, `PopClipRect`,
/// `PushScrollLayer`, `PopScrollLayer`, `PushOpacity`, `PopOpacity`,
/// `PushTransform`, `PopTransform`, `PushBlendMode`, `PopBlendMode`, etc.)
/// always pass through unchanged so that the GPU state machine remains correct.
///
/// Returns owned clones of the matching commands, ready to pass to the renderer.
#[must_use]
pub fn cull_display_list(
    dl: &[DisplayCommand],
    tile_x: i32,
    tile_y: i32,
    tile_size: f32,
) -> Vec<DisplayCommand> {
    let tx = tile_x as f32 * tile_size;
    let ty = tile_y as f32 * tile_size;

    let mut out = Vec::new();
    for cmd in dl {
        match get_command_rect(cmd) {
            Some(r) => {
                // AABB intersection: both axes must overlap.
                let overlaps_x = r.x < tx + tile_size && r.x + r.width > tx;
                let overlaps_y = r.y < ty + tile_size && r.y + r.height > ty;
                if overlaps_x && overlaps_y {
                    out.push(cmd.clone());
                }
            }
            // State / stack commands always pass through.
            None => out.push(cmd.clone()),
        }
    }
    out
}

/// Cheap pre-check the renderer uses to decide whether computing a frame
/// content hash for [`hash_display_list`] is worthwhile — pages without a
/// backdrop-filter pay zero hashing cost.
#[must_use]
pub fn contains_backdrop_filter(content: &[DisplayCommand], overlay: &[DisplayCommand]) -> bool {
    content
        .iter()
        .chain(overlay.iter())
        .any(|c| matches!(c, DisplayCommand::PushBackdropFilter { .. }))
}

mod fingerprint;
pub(crate) use fingerprint::{hash_command_into, hash_one_command};
// Used only by `display_list/tests/svg_table_and_hash.rs` (via `super::*`).
#[cfg(test)]
pub(crate) use fingerprint::{h_color, h_f32, h_rect, h_str, HashFmt};
// Needed here for `cull_display_list`, above the DL-16 region.
use fingerprint::get_command_rect;
pub use fingerprint::{
    anim_split_compose_plan, diff_display_lists, fold_content_dual, fold_overlay,
    fold_overlay_with_reuse, hash_content, hash_display_list, hash_display_list_dual,
    hash_display_list_dual_memo, hash_display_list_dual_memo_with_overlay_digests,
    hash_display_list_skipping, DiffResult, FrameDelta, FrameFingerprint,
};

mod serialize;
pub use serialize::serialize_display_list;

mod builder;
pub use builder::{
    build_display_list, build_display_list_ordered, build_display_list_ordered_dpr,
    build_display_list_ordered_with_anim, build_display_list_ordered_with_anim_dpr,
    build_display_list_ordered_with_anim_split, build_display_list_with_anim,
    build_display_list_with_selection,
};
// Used only by `display_list/box_layer.rs` (via `super::*`).
use builder::SplitTracker;

mod print;
pub use print::{build_print_display_list, split_at_page_breaks, strip_background_graphics};
// Used by `display_list/{box_layer,walk}.rs` and `display_list/tests/anim_and_chrome.rs`
// (via `super::*` / explicit `use super::clip_path_to_rect`).
use print::clip_path_to_rect;
// Used only by `display_list/text_run.rs` (via `super::*`).
use print::ELLIPSIS_EM;
// Used by `display_list/{box_layer,walk,background_mask}.rs` (via `super::*`).
use print::map_blend_mode;
// Used by `display_list/{box_layer,scrollbars,text_run,walk}.rs` (via `super::*`).
use print::overflow_clips;
// Used only by `display_list/box_layer.rs` (via `super::*`).
use print::{record_span, BucketField, RawSpan, ScBucket};
// Used by `clip_path_to_shape`, still in this file just below the DL-15 region.
use print::resolve_shape_center;

/// BUG-140: резолвит `clip-path` в точную форму в page-координатах
/// (пространство до transform элемента) относительно border-box `r`.
/// `None` для `inset(...)` — он точно представим прямоугольником и эмитится
/// как `PushClipRect` (см. `clip_path_to_rect`). Базисы процентов —
/// CSS Shapes L1 §5: x/width, y/height, радиус circle — `sqrt(w²+h²)/√2`.
fn clip_path_to_shape(clip: &ClipPath, r: Rect) -> Option<ResolvedClipShape> {
    match clip {
        ClipPath::Inset(_) => None,
        ClipPath::Circle { radius, center } => {
            let (cx, cy) = resolve_shape_center(*center, r);
            let diag = ((r.width * r.width + r.height * r.height) * 0.5).sqrt();
            Some(ResolvedClipShape::Circle { cx, cy, r: radius.resolve(diag) })
        }
        ClipPath::Ellipse { rx, ry, center } => {
            let (cx, cy) = resolve_shape_center(*center, r);
            Some(ResolvedClipShape::Ellipse {
                cx,
                cy,
                rx: rx.resolve(r.width),
                ry: ry.resolve(r.height),
            })
        }
        ClipPath::Polygon(vertices, fill_rule) => {
            if vertices.is_empty() {
                return None;
            }
            Some(ResolvedClipShape::Polygon {
                verts: vertices
                    .iter()
                    .map(|(x, y)| (r.x + x.resolve(r.width), r.y + y.resolve(r.height)))
                    .collect(),
                even_odd: matches!(fill_rule, FillRule::EvenOdd),
            })
        }
        // CSS Shapes L1 §4 — `path()`: точки уже флэттены в px системы пути
        // (origin = верхний левый угол reference box). Смещаем на позицию box.
        ClipPath::Path(points, fill_rule) => {
            if points.len() < 3 {
                return None;
            }
            Some(ResolvedClipShape::Polygon {
                verts: points.iter().map(|(x, y)| (r.x + x, r.y + y)).collect(),
                even_odd: matches!(fill_rule, FillRule::EvenOdd),
            })
        }
    }
}

/// Union of a line's visible fragments, as a painting rect for
/// [`text_run::emit_first_line_background`]. `None` when the line contributes no extent.
///
/// Fragment padding/border are included only for real inline element boxes —
/// for anonymous text fragments those style fields belong to the enclosing
/// element and would widen the union by space the text does not occupy.
fn first_line_content_rect(b: &LayoutBox, line: &[InlineFrag], line_h: f32) -> Option<Rect> {
    let mut left = f32::MAX;
    let mut right = f32::MIN;
    for frag in line.iter() {
        if !matches!(frag.style.visibility, Visibility::Visible) {
            continue;
        }
        let (pad_l, pad_r, bl, br) = if frag.is_element_box {
            (
                frag.padding_left,
                frag.padding_right,
                frag.style.border_left_width,
                frag.style.border_right_width,
            )
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };
        let l = b.rect.x + frag.x - pad_l - bl;
        let r = b.rect.x + frag.x + frag.width + pad_r + br;
        if r <= l {
            continue;
        }
        left = left.min(l);
        right = right.max(r);
    }
    if right <= left {
        return None;
    }
    // Same integer-pixel snapping `emit_inline_frag_box` applies, so an inline
    // background and a ::first-line background under it share their edges.
    Some(Rect::new(left.round(), b.rect.y.round(), (right - left).round(), line_h.round()))
}

/// CSS Images L4 §5 — selects the best `image-set()` candidate URL for `dpr`.
///
/// Parses an `image-set( <option># )` expression where each option is
/// `<url-or-string> [<resolution>]`. Resolution defaults to `1x`. Supported
/// resolution units: `x` / `dppx` (device pixel ratio), `dpi`, `dpcm`.
///
/// Returns the URL (with `url(…)` wrapper and surrounding quotes stripped)
/// whose resolution is closest to `dpr`; ties prefer the higher resolution
/// (sharper asset). If `value` is not an `image-set()` expression the whole
/// trimmed value is treated as a single 1× option, so plain URLs pass through
/// unchanged. Returns `""` when no candidate can be parsed.
///
/// The result is a subslice of `value` — no allocation.
#[must_use]
pub fn select_image_set_url(value: &str, dpr: f32) -> &str {
    let trimmed = value.trim();
    let inner = strip_image_set_wrapper(trimmed).unwrap_or(trimmed);

    let mut best: Option<(&str, f32)> = None;
    for opt in split_top_level_commas(inner) {
        let opt = opt.trim();
        if opt.is_empty() {
            continue;
        }
        let (url, res) = parse_image_set_option(opt);
        if url.is_empty() {
            continue;
        }
        let better = match best {
            None => true,
            Some((_, bres)) => {
                let d = (res - dpr).abs();
                let bd = (bres - dpr).abs();
                d < bd || (d == bd && res > bres)
            }
        };
        if better {
            best = Some((url, res));
        }
    }
    best.map_or("", |(u, _)| u)
}

mod text_run;
use text_run::emit_inline_run;

mod box_layer;
use box_layer::fill_buckets;

mod inline_frag;
use inline_frag::{
    emit_inline_frag_box, emit_text_shadows, mask_clip_paint_rect, parse_image_set_option,
    split_top_level_commas, strip_image_set_wrapper,
};
pub use inline_frag::is_image_set;
pub(crate) use inline_frag::{background_clip_rect, background_color_clip};
// Used only by `display_list/background_mask.rs` (via `super::*`).
use inline_frag::background_origin_rect;
// Used only by `display_list/walk.rs` (via `super::*`).
use inline_frag::content_box_rect;

mod background_mask;
use background_mask::{emit_background_image, emit_push_mask, rendered_mask_layers};
// Used only by `display_list/tests/background_and_layers.rs` (via `super::*`).
#[cfg(test)]
use background_mask::gradient_tile_rects;
// Used only by `display_list/tests/anim_and_chrome.rs` (via `super::*`).
#[cfg(test)]
use background_mask::mask_stops_for_mode;

mod box_shadow;
use box_shadow::{emit_box_shadows, emit_inset_box_shadows};

mod scrollbars;
use scrollbars::emit_scrollbars;
pub use scrollbars::patch_scroll_layer;
// Used only by `display_list/tests/anim_and_chrome.rs` (via `super::*`).
#[cfg(test)]
use scrollbars::{SCROLLBAR_THUMB_COLOR, SCROLLBAR_TRACK_COLOR, SCROLLBAR_WIDTH_THIN};

mod outline_misc;
use outline_misc::{emit_column_rules, emit_outline, emit_resize_grip};
pub use outline_misc::point_on_resize_grip;
pub(crate) use outline_misc::{is_opacity_subtree_painted, is_paint_visible};

mod form_controls;
use form_controls::{emit_form_control_indicator, emit_list_marker, push_thick_segment};
pub(crate) use form_controls::is_hidden_empty_cell;
// Used only by `display_list/tests/svg_table_and_hash.rs` (via `super::meter_gauge_color`).
#[cfg(test)]
use form_controls::meter_gauge_color;

mod svg_text_decoration;
use svg_text_decoration::{emit_svg_shape, emit_svg_shape_masked, emit_svg_text, push_text_decoration, walk_with_anim};

#[cfg(test)]
#[path = "display_list/tests/text_and_images.rs"]
mod text_and_images;

mod text_highlight;
pub use text_highlight::emit_text_with_highlights;

mod table;
use table::{collect_table_cells, emit_table_box, emit_table_cell_border};

mod walk;
use walk::{
    depth_sorted_child_order, emit_box_self, establishes_3d_rendering_context, is_backface_hidden,
    walk,
};
// Used only by `display_list/tests/shadows_and_transforms.rs` (via `use super::*`).
#[cfg(test)]
use walk::depth_order_by_z;

#[cfg(test)]
#[path = "display_list/tests/highlight.rs"]
mod highlight;

#[cfg(test)]
#[path = "display_list/tests/svg_table_and_hash.rs"]
mod svg_table_and_hash;

#[cfg(test)]
#[path = "display_list/tests/anim_and_chrome.rs"]
mod anim_and_chrome;

#[cfg(test)]
#[path = "display_list/tests/shadows_and_transforms.rs"]
mod shadows_and_transforms;

#[cfg(test)]
#[path = "display_list/tests/background_and_layers.rs"]
mod background_and_layers;

#[cfg(test)]
#[path = "display_list/tests/ordered_build_scroll.rs"]
mod ordered_build_scroll;

#[cfg(test)]
#[path = "display_list/tests/form_controls_caret.rs"]
mod form_controls_caret;
