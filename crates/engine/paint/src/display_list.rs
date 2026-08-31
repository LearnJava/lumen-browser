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

/// Adapter that feeds `core::fmt` output straight into a [`Hasher`] without
/// allocating an intermediate `String`.
/// Адаптер `fmt::Write` → `Hasher`: пишет Debug-представление напрямую в хешер,
/// без промежуточной `String` (нулевые аллокации в горячем пути кадра).
pub(crate) struct HashFmt<'a>(pub(crate) &'a mut std::collections::hash_map::DefaultHasher);

impl std::fmt::Write for HashFmt<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        use std::hash::Hasher;
        self.0.write(s.as_bytes());
        Ok(())
    }
}

/// Writes an `f32` into the hasher by its bit pattern.
///
/// Bit-hashing is *stricter* than the `Debug` text it replaces: `NaN` payloads
/// that print identically hash differently. That direction is safe — it can
/// only produce a spurious "changed" verdict (an extra repaint), never a
/// spurious "identical" one (stale pixels on screen).
#[inline]
fn h_f32(h: &mut std::collections::hash_map::DefaultHasher, v: f32) {
    use std::hash::Hasher;
    h.write_u32(v.to_bits());
}

/// Writes a [`Rect`] into the hasher field-by-field.
#[inline]
fn h_rect(h: &mut std::collections::hash_map::DefaultHasher, r: &Rect) {
    h_f32(h, r.x);
    h_f32(h, r.y);
    h_f32(h, r.width);
    h_f32(h, r.height);
}

/// Writes an RGBA8 [`Color`] into the hasher.
#[inline]
fn h_color(h: &mut std::collections::hash_map::DefaultHasher, c: &Color) {
    use std::hash::Hasher;
    h.write_u8(c.r);
    h.write_u8(c.g);
    h.write_u8(c.b);
    h.write_u8(c.a);
}

/// Writes all eight [`CornerRadii`] components into the hasher.
#[inline]
fn h_radii(h: &mut std::collections::hash_map::DefaultHasher, r: &CornerRadii) {
    for v in [r.tl, r.tl_y, r.tr, r.tr_y, r.br, r.br_y, r.bl, r.bl_y] {
        h_f32(h, v);
    }
}

/// Writes a string into the hasher with a terminator, so that `"ab" + "c"`
/// cannot collide with `"a" + "bc"`.
#[inline]
fn h_str(h: &mut std::collections::hash_map::DefaultHasher, s: &str) {
    use std::hash::Hasher;
    h.write(s.as_bytes());
    h.write_u8(0xff);
}

/// Folds one [`DisplayCommand`] into `h` **structurally** — raw field bytes,
/// no `core::fmt` machinery.
///
/// Why: the frame-skip hash used to fold every command through `{cmd:?}`.
/// `Debug` for `f32` runs the Grisu/Dragon shortest-repr algorithm per float,
/// and a typical frame carries thousands of them — measured at 1.2–2.5 ms per
/// frame on `1000000-final.html` (see EXPERIMENT.md §9 "открытые хвосты").
///
/// **Safety of the fast path.** The hot variants below destructure *every*
/// field explicitly — no `..` rest-pattern — so adding a field to one of them
/// is a compile error, not a silent stale-pixel bug. Every other variant falls
/// through to the original `Debug` fold, which is exhaustive by construction:
/// a newly added variant is hashed correctly (just slower) from day one. The
/// variant tag itself is always folded via `mem::discriminant`, so two variants
/// with structurally identical payloads can never collide.
pub(crate) fn hash_command_into(
    cmd: &DisplayCommand,
    h: &mut std::collections::hash_map::DefaultHasher,
) {
    use std::fmt::Write as _;
    use std::hash::{Hash as _, Hasher as _};

    std::mem::discriminant(cmd).hash(h);

    match cmd {
        DisplayCommand::FillRect { rect, color } => {
            h_rect(h, rect);
            h_color(h, color);
        }
        DisplayCommand::FillRoundedRect { rect, color, radii } => {
            h_rect(h, rect);
            h_color(h, color);
            h_radii(h, radii);
        }
        DisplayCommand::DrawBorder { rect, widths, colors, styles, radii } => {
            h_rect(h, rect);
            for w in widths {
                h_f32(h, *w);
            }
            for c in colors {
                h_color(h, c);
            }
            for s in styles {
                std::mem::discriminant(s).hash(h);
            }
            h_radii(h, radii);
        }
        DisplayCommand::DrawOutline { rect, width, style, color, offset } => {
            h_rect(h, rect);
            h_f32(h, *width);
            std::mem::discriminant(style).hash(h);
            h_color(h, color);
            h_f32(h, *offset);
        }
        DisplayCommand::PushClipRect { rect } => h_rect(h, rect),
        DisplayCommand::PushOpacity { alpha, bounds } => {
            h_f32(h, *alpha);
            if let Some(r) = bounds {
                h_rect(h, r);
            }
        }
        DisplayCommand::PushTransform { matrix } => {
            for v in matrix.0 {
                h_f32(h, v);
            }
        }
        DisplayCommand::DrawText {
            rect,
            text,
            font_size,
            color,
            font_family,
            font_weight,
            font_style,
            font_stretch,
            font_variation_axes,
            font_features,
            font_palette,
            tab_size,
            highlight_name,
            text_orientation,
        } => {
            h_rect(h, rect);
            h_str(h, text);
            h_f32(h, *font_size);
            h_color(h, color);
            h.write_usize(font_family.len());
            for f in font_family {
                h_str(h, f);
            }
            h.write_u16(font_weight.0);
            std::mem::discriminant(font_style).hash(h);
            // Влияет на выбор face-а — стало быть, и на пиксели: без него
            // кадр, где сменился только font-stretch, переиспользует
            // закэшированный тайл со старым (нормальной ширины) face-ом.
            h.write_u16(font_stretch.0);
            h.write_usize(font_variation_axes.len());
            for (tag, v) in font_variation_axes {
                h.write(tag);
                h_f32(h, *v);
            }
            h.write_usize(font_features.len());
            for (tag, v) in font_features {
                h.write(tag);
                h.write_u32(*v);
            }
            // Structurally complex and almost always `None` — `Debug` here costs
            // four bytes and no float formatting.
            {
                let mut hf = HashFmt(h);
                let _ = write!(hf, "{font_palette:?}");
            }
            h_f32(h, *tab_size);
            match highlight_name {
                Some(s) => {
                    h.write_u8(1);
                    h_str(h, s);
                }
                None => h.write_u8(0),
            }
            match text_orientation {
                Some(t) => {
                    h.write_u8(1);
                    std::mem::discriminant(t).hash(h);
                }
                None => h.write_u8(0),
            }
        }
        // Cold variants: gradients, SVG, masks, filters, snapshots, scrollbars,
        // and every unit variant. Folded through `Debug` exactly as before.
        other => {
            let mut hf = HashFmt(h);
            // Errors are impossible: HashFmt::write_str never fails.
            let _ = write!(hf, "{other:?}");
        }
    }
}

/// Хеширует одну команду структурно, без аллокаций.
///
/// [`hash_display_list_dual`] сворачивает кадр через этот дайджест: свёртка
/// команды, попадающая в оба кадровых хэша, считается один раз. Границы команд
/// при этом становятся явными (в непрерывном потоке они были неявными) — это
/// строже, а не слабее: «размазать» поля соседних команд друг в друга нельзя.
pub(crate) fn hash_one_command(cmd: &DisplayCommand) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hash_command_into(cmd, &mut hasher);
    hasher.finish()
}

/// Computes a content hash over a frame's display list plus the viewport state
/// that affects backdrop-filter output (scroll offset and surface size).
///
/// Used by the renderer's `backdrop-filter` cache (CSS Filter Effects L1 §2):
/// if two consecutive frames hash identically, every backdrop element's
/// filtered result is guaranteed identical, so the blur passes can be skipped
/// and the cached texture reused.
///
/// The hash is **total** — it folds every field of every command (see
/// [`hash_command_into`]: explicit fields for the hot variants, `Debug` for the
/// cold ones) — so adding new `DisplayCommand` variants or fields can never
/// silently produce a false cache hit (which would paint stale pixels).
///
/// The hasher (`DefaultHasher`) is process-deterministic and never influences
/// pixel output (only the skip decision), so cross-OS bit-identity is not a
/// concern here.
#[must_use]
pub fn hash_display_list(
    content: &[DisplayCommand],
    overlay: &[DisplayCommand],
    scroll_x: f32,
    scroll_y: f32,
    surface_w: u32,
    surface_h: u32,
) -> u64 {
    use std::hash::Hasher;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write_u32(surface_w);
    hasher.write_u32(surface_h);
    hasher.write_u32(scroll_x.to_bits());
    hasher.write_u32(scroll_y.to_bits());
    // Lane lengths: keeps the "content then overlay" fold unambiguous when a
    // command migrates between lanes.
    hasher.write_usize(content.len());
    hasher.write_usize(overlay.len());
    for cmd in content.iter().chain(overlay.iter()) {
        hash_command_into(cmd, &mut hasher);
    }
    hasher.finish()
}

/// Content-only frame hash (ADR-016 M0.5).
///
/// Unlike [`hash_display_list`], this folds **only** the page-content commands
/// and the surface size into the hash — the scroll offset and the fixed page
/// offset are deliberately excluded. Two frames that differ only in how far the
/// page is scrolled therefore hash identically, which is exactly what lets the
/// compositor tell "same content, new offset" (a blit — M3's fast path) apart
/// from "content changed" (a full re-raster).
///
/// Overlay commands (scrollbar thumb, docked panels, find-bar) are intentionally
/// **not** passed here: they are viewport-locked and cheap to repaint every
/// frame, and the scrollbar thumb in particular is rebuilt from `scroll_y` each
/// frame, so folding it in would make every scroll frame look like a content
/// change and defeat the content/offset split.
///
/// Allocation-free: each command's `Debug` output is streamed straight into the
/// hasher via [`HashFmt`], preserving `hash_display_list`'s totality guarantee
/// (a new `DisplayCommand` variant or field can never silently collide).
#[must_use]
pub fn hash_content(content: &[DisplayCommand], surface_w: u32, surface_h: u32) -> u64 {
    use std::fmt::Write as _;
    use std::hash::Hasher;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write_u32(surface_w);
    hasher.write_u32(surface_h);
    hasher.write_usize(content.len());
    {
        let mut hf = HashFmt(&mut hasher);
        for cmd in content {
            // Errors are impossible: HashFmt::write_str never fails.
            let _ = write!(hf, "{cmd:?}");
        }
    }
    hasher.finish()
}

/// Как [`hash_display_list`], но с выколотыми диапазонами `skip` (static-часть
/// кадра для scroll-инвариантного ключа полосы скролл-композитора).
///
/// Эквивалентен `hash_display_list` от материализованного списка без
/// skip-команд: та же свёртка, длиной content-полосы служит число
/// оставшихся команд. `skip` обязан быть отсортирован и не пересекаться
/// (гарантируется [`build_display_list_ordered_with_anim_split`]).
pub fn hash_display_list_skipping(
    content: &[DisplayCommand],
    skip: &[std::ops::Range<usize>],
    overlay: &[DisplayCommand],
    scroll_x: f32,
    scroll_y: f32,
    surface_w: u32,
    surface_h: u32,
) -> u64 {
    use std::hash::Hasher;

    let skipped: usize = skip.iter().map(std::ops::Range::len).sum();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write_u32(surface_w);
    hasher.write_u32(surface_h);
    hasher.write_u32(scroll_x.to_bits());
    hasher.write_u32(scroll_y.to_bits());
    hasher.write_usize(content.len().saturating_sub(skipped));
    hasher.write_usize(overlay.len());
    let mut skip_iter = skip.iter().peekable();
    for (i, cmd) in content.iter().enumerate() {
        while skip_iter.peek().is_some_and(|r| r.end <= i) {
            skip_iter.next();
        }
        if skip_iter.peek().is_some_and(|r| r.contains(&i)) {
            continue;
        }
        hash_command_into(cmd, &mut hasher);
    }
    for cmd in overlay {
        hash_command_into(cmd, &mut hasher);
    }
    hasher.finish()
}

/// Оба кадровых хэша за ОДИН обход списка (BUG-405 срез 35, пункт 70).
///
/// Возвращает `(хэш кадра, ключ полосы)` — те же две свёртки, что кадр раньше
/// считал двумя раздельными обходами ([`hash_display_list`] по
/// `content` + `overlay` со скроллом и размерами поверхности,
/// [`hash_display_list_skipping`] по статичной части `content` при нулевом
/// скролле и размерах полосы). Значения ДРУГИЕ, чем у пары: оба хэша
/// сравниваются только с хэшем предыдущего кадра того же процесса, поэтому
/// важны их свойства (см. гейты `dual_*` в тестах), а не конкретные числа.
///
/// **Почему один проход дешевле.** Скролл и размеры входят в оба хэша
/// отдельными полями, а не через обход, а сами команды у хэшей общие — при
/// пустом `skip` полностью, иначе с точностью до выколотых диапазонов. Список
/// разбирается один раз, и общая часть сворачивается ОДНИМ потоком SipHash:
/// команда даёт 64-битный дайджест, который уходит в оба хешера. Тройник
/// ([`TeeHasher`]) остаётся только для кадров с непустым `skip`, где у плеч
/// разные множества команд, — там экономится разбор, но не байты.
///
/// `skip` обязан быть отсортирован и не пересекаться — то же требование, что
/// у [`hash_display_list_skipping`]; overlay в ключ не входит вовсе
/// (viewport-locked, см. [`hash_content`]).
#[must_use]
pub fn hash_display_list_dual(
    content: &[DisplayCommand],
    overlay: &[DisplayCommand],
    skip: &[std::ops::Range<usize>],
    scroll: (f32, f32),
    surface: (u32, u32),
    band: (u32, u32),
) -> (u64, u64) {
    hash_display_list_dual_memo(content, overlay, skip, scroll, surface, band, None).0
}

/// Свёртки content-части кадра для обоих кадровых хэшей (BUG-405 срез 39).
///
/// `.0` — поток дайджестов ВСЕХ команд списка (вход хэша кадра), `.1` — тот же
/// поток без выколотых `skip`-диапазонов (вход ключа полосы). Обход и дайджест
/// команды — ровно те же, что считал [`hash_display_list_dual`] до среза 39;
/// новое здесь только то, что результат обхода стал ЗНАЧЕНИЕМ, которое кадр
/// может запомнить и переиспользовать, пока список не менялся.
///
/// `skip` обязан быть отсортирован и не пересекаться.
#[must_use]
pub fn fold_content_dual(content: &[DisplayCommand], skip: &[std::ops::Range<usize>]) -> (u64, u64) {
    use std::hash::Hasher;

    let mut frame = std::collections::hash_map::DefaultHasher::new();
    let mut key = std::collections::hash_map::DefaultHasher::new();
    if skip.is_empty() {
        // Горячий случай (страница без анимируемых сегментов): у плеч одно и
        // то же множество команд, поэтому дайджест команды считается один раз
        // и пишется в оба хешера — байты команды сворачиваются однократно.
        for cmd in content {
            let d = hash_one_command(cmd);
            frame.write_u64(d);
            key.write_u64(d);
        }
    } else {
        let mut skip_iter = skip.iter().peekable();
        for (i, cmd) in content.iter().enumerate() {
            while skip_iter.peek().is_some_and(|r| r.end <= i) {
                skip_iter.next();
            }
            let d = hash_one_command(cmd);
            frame.write_u64(d);
            if !skip_iter.peek().is_some_and(|r| r.contains(&i)) {
                key.write_u64(d);
            }
        }
    }
    (frame.finish(), key.finish())
}

/// Дайджест overlay-списка, один [`hash_one_command`] на элемент (BUG-405
/// срез 47). Раньше он считался НЕЗАВИСИМО в двух местах одного и того же
/// кадра: здесь (внутри [`hash_display_list_dual_memo`], статья `frame-hash`)
/// и в `Renderer::overlay_cache_step` (статья `послекэша` — срез 44 измерил
/// её ~0.12 мс на кадр попадания, срез 43 назвал сам факт безусловного
/// пересчёта). Оба потребителя сравнивают дайджест с одним и тем же
/// определением (`hash_one_command`), так что вызывающий (`render_with_anim`)
/// теперь считает его ОДИН раз и передаёт результат в оба места —
/// [`hash_display_list_dual_memo_with_overlay_digests`] и
/// `overlay_cache_step`.
#[must_use]
pub fn fold_overlay(overlay: &[DisplayCommand]) -> Vec<u64> {
    overlay.iter().map(hash_one_command).collect()
}

/// [`hash_display_list_dual`] с ГОТОВОЙ свёрткой content-части (BUG-405 срез 39).
///
/// `folds` — результат [`fold_content_dual`] для этого же `content`/`skip`,
/// снятый на прошлом кадре; `None` — посчитать заново. Возвращает пару хэшей и
/// свёртку, которой они посчитаны (её и запоминает кадр).
///
/// Зачем: на кадре ПОПАДАНИЯ полосы content не менялся вовсе — страница
/// свёрстана, едет только скролл, — а оба хэша обходили его целиком. Перепись
/// среза 39: 0.76 мс на кадр при 843 + 132 командах, 37 % честного кадра
/// попадания. Скролл, размеры поверхности и полосы, длины и overlay в свёртку
/// не входят и дописываются здесь каждый кадр, поэтому переиспользование
/// свёртки НЕ делает кадр слепым ни к одному из них.
///
/// Ответственность за «список не менялся» лежит на вызывающем
/// ([`RenderBackend::set_content_epoch`](crate::backend::RenderBackend::set_content_epoch)).
///
/// Пересчитывает overlay-дайджест внутри себя ([`fold_overlay`]) — тесты и
/// прочие вызывающие, которым нечего переиспользовать, используют эту форму
/// как раньше. Горячий путь `render_with_anim` вызывает
/// [`hash_display_list_dual_memo_with_overlay_digests`] напрямую с уже
/// посчитанным дайджестом (срез 47).
#[must_use]
pub fn hash_display_list_dual_memo(
    content: &[DisplayCommand],
    overlay: &[DisplayCommand],
    skip: &[std::ops::Range<usize>],
    scroll: (f32, f32),
    surface: (u32, u32),
    band: (u32, u32),
    folds: Option<(u64, u64)>,
) -> ((u64, u64), (u64, u64)) {
    hash_display_list_dual_memo_with_overlay_digests(
        content,
        &fold_overlay(overlay),
        skip,
        scroll,
        surface,
        band,
        folds,
    )
}

/// [`hash_display_list_dual_memo`] с ГОТОВЫМ overlay-дайджестом
/// ([`fold_overlay`]) вместо самого overlay-списка (BUG-405 срез 47) — та же
/// формула хэша, `overlay_digests.len()` заменяет `overlay.len()` (равны по
/// построению: один дайджест на команду).
#[must_use]
pub fn hash_display_list_dual_memo_with_overlay_digests(
    content: &[DisplayCommand],
    overlay_digests: &[u64],
    skip: &[std::ops::Range<usize>],
    scroll: (f32, f32),
    surface: (u32, u32),
    band: (u32, u32),
    folds: Option<(u64, u64)>,
) -> ((u64, u64), (u64, u64)) {
    use std::hash::Hasher;

    let (scroll_x, scroll_y) = scroll;
    let (surface_w, surface_h) = surface;
    let (key_w, key_h) = band;
    let folds = folds.unwrap_or_else(|| fold_content_dual(content, skip));

    let mut frame = std::collections::hash_map::DefaultHasher::new();
    frame.write_u32(surface_w);
    frame.write_u32(surface_h);
    frame.write_u32(scroll_x.to_bits());
    frame.write_u32(scroll_y.to_bits());
    frame.write_usize(content.len());
    frame.write_usize(overlay_digests.len());
    frame.write_u64(folds.0);

    let skipped: usize = skip.iter().map(std::ops::Range::len).sum();
    let mut key = std::collections::hash_map::DefaultHasher::new();
    key.write_u32(key_w);
    key.write_u32(key_h);
    key.write_usize(content.len().saturating_sub(skipped));
    key.write_u64(folds.1);

    for &d in overlay_digests {
        frame.write_u64(d);
    }
    ((frame.finish(), key.finish()), folds)
}

/// How a frame differs from the previously presented one (ADR-016 M0.5).
///
/// Produced by [`FrameFingerprint::delta_from`]. The variants map directly onto
/// the render strategies the staged multithreaded pipeline will pick between:
/// `Identical` → skip, `OffsetOnly` → blit (M3), `ContentChanged` → re-raster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDelta {
    /// Page content, scroll and page offset are all unchanged — the previously
    /// presented framebuffer is still correct and the frame can be skipped.
    Identical,
    /// Page content is unchanged but the scroll and/or fixed page offset moved —
    /// the M3 blit fast path can shift the retained content instead of
    /// re-rasterizing it.
    OffsetOnly,
    /// Page content changed (or the surface was resized) — a full re-raster is
    /// required.
    ContentChanged,
}

/// Split fingerprint of a presented frame (ADR-016 M0.5).
///
/// Separates the content hash (page commands + surface size, scroll excluded)
/// from the raw scroll and page offsets. Keeping the offsets out of the hash —
/// as plain copyable values rather than folded into it — is what lets
/// [`FrameFingerprint::delta_from`] return [`FrameDelta::OffsetOnly`] for a
/// scroll-only frame; those same offsets are also the input the M3 blit needs to
/// know how far to shift the retained content.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameFingerprint {
    /// Hash of the page-content commands and surface size — scroll excluded
    /// (see [`hash_content`]).
    pub content_hash: u64,
    /// Scroll offset `(x, y)`, in the same units the render backend receives.
    pub scroll: (f32, f32),
    /// Fixed page offset `(x, y)` — the left-docked sidebar width and tab-bar
    /// height applied render-side since M0.4.
    pub offset: (f32, f32),
}

impl FrameFingerprint {
    /// Build a fingerprint for the current frame from its page content, surface
    /// size and the two offsets.
    #[must_use]
    pub fn new(
        content: &[DisplayCommand],
        surface_w: u32,
        surface_h: u32,
        scroll: (f32, f32),
        offset: (f32, f32),
    ) -> Self {
        Self {
            content_hash: hash_content(content, surface_w, surface_h),
            scroll,
            offset,
        }
    }

    /// Classify how this frame differs from the previously presented `prev`.
    ///
    /// A differing `content_hash` always wins (`ContentChanged`) — a resize or
    /// any command edit forces a re-raster. Only when the content hash matches do
    /// the offsets decide between `OffsetOnly` (something moved) and `Identical`
    /// (nothing moved).
    #[must_use]
    pub fn delta_from(&self, prev: &FrameFingerprint) -> FrameDelta {
        if self.content_hash != prev.content_hash {
            FrameDelta::ContentChanged
        } else if self.scroll != prev.scroll || self.offset != prev.offset {
            FrameDelta::OffsetOnly
        } else {
            FrameDelta::Identical
        }
    }
}

// ─── Static/animated split: план оверлея + painter's-order guard ────────────

/// Консервативный bbox draw-команды в её локальных координатах (до
/// transform-стека). `None` — команда ничего не рисует (push/pop/PageBreak).
/// `SegBounds::Unbounded` — экстент вычислить нельзя (рисует «где-то»).
fn draw_cmd_local_bbox(cmd: &DisplayCommand) -> Option<SegBounds> {
    fn pts_bbox(iter: impl Iterator<Item = [f32; 2]>) -> SegBounds {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        let mut any = false;
        for [x, y] in iter {
            any = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        if any {
            SegBounds::Rect(Rect::new(min_x, min_y, max_x - min_x, max_y - min_y))
        } else {
            SegBounds::Empty
        }
    }
    Some(match cmd {
        DisplayCommand::FillRect { rect, .. }
        | DisplayCommand::FillRoundedRect { rect, .. }
        | DisplayCommand::DrawBorder { rect, .. }
        | DisplayCommand::DrawImage { rect, .. }
        | DisplayCommand::LazyImageSlot { rect, .. }
        | DisplayCommand::DrawBackgroundImage { rect, .. }
        | DisplayCommand::DrawLinearGradient { rect, .. }
        | DisplayCommand::DrawRadialGradient { rect, .. }
        | DisplayCommand::DrawConicGradient { rect, .. }
        | DisplayCommand::DrawLayerSnapshot { rect, .. } => SegBounds::Rect(*rect),
        // Глифы могут выступать за line-box (курсив, свисания) — запас в
        // половину кегля со всех сторон, строго в большую сторону.
        DisplayCommand::DrawText { rect, font_size, .. } => {
            SegBounds::Rect(inflate_rect(*rect, font_size * 0.5))
        }
        DisplayCommand::DrawOutline { rect, width, offset, .. } => {
            SegBounds::Rect(inflate_rect(*rect, width + offset.max(0.0)))
        }
        DisplayCommand::DrawCrossFade { dest, .. } => SegBounds::Rect(*dest),
        DisplayCommand::BoxModelOverlay { margin, .. } => SegBounds::Rect(*margin),
        DisplayCommand::DrawScrollbar { track_rect, thumb_rect, .. } => {
            SegBounds::Rect(union_rects(*track_rect, *thumb_rect))
        }
        DisplayCommand::DrawSvgPath { vertices, .. } => pts_bbox(vertices.iter().copied()),
        DisplayCommand::DrawSvgFill { contours, .. } => {
            pts_bbox(contours.iter().flatten().copied())
        }
        DisplayCommand::DrawSvgStroke { contours, params, .. } => {
            // Miter-стык может выступать до half_width·miterlimit от осевой.
            let d = params.half_width * params.miterlimit.max(1.0) + 1.0;
            match pts_bbox(contours.iter().flatten().copied()) {
                SegBounds::Rect(r) => SegBounds::Rect(inflate_rect(r, d)),
                other => other,
            }
        }
        DisplayCommand::PageBreak => SegBounds::Empty,
        _ => return None,
    })
}

/// Экстент множества draw-команд: пустой, прямоугольник или «неизвестно».
enum SegBounds {
    /// Ничего не нарисовано.
    Empty,
    /// Всё нарисованное лежит внутри прямоугольника (документные CSS px).
    Rect(Rect),
    /// Экстент вычислить нельзя.
    Unbounded,
}

impl SegBounds {
    fn union(&mut self, other: SegBounds) {
        match (&*self, other) {
            (_, SegBounds::Empty) => {}
            (SegBounds::Unbounded, _) => {}
            (_, SegBounds::Unbounded) => *self = SegBounds::Unbounded,
            (SegBounds::Empty, r @ SegBounds::Rect(_)) => *self = r,
            (SegBounds::Rect(a), SegBounds::Rect(b)) => {
                *self = SegBounds::Rect(union_rects(*a, b));
            }
        }
    }
}

fn inflate_rect(r: Rect, d: f32) -> Rect {
    Rect::new(r.x - d, r.y - d, r.width + 2.0 * d, r.height + 2.0 * d)
}

fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// Аффинный bbox прямоугольника после матрицы (4 угла → min/max).
fn affine_rect_bbox(m: &Mat4, r: Rect) -> Rect {
    let a = m.0[0];
    let b = m.0[1];
    let c = m.0[4];
    let d = m.0[5];
    let e = m.0[12];
    let f = m.0[13];
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for (px, py) in [
        (r.x, r.y),
        (r.x + r.width, r.y),
        (r.x, r.y + r.height),
        (r.x + r.width, r.y + r.height),
    ] {
        let tx = a * px + c * py + e;
        let ty = b * px + d * py + f;
        min_x = min_x.min(tx);
        min_y = min_y.min(ty);
        max_x = max_x.max(tx);
        max_y = max_y.max(ty);
    }
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

/// Суммарная инфляция bbox от blur-функций фильтра — ровно охват ядра
/// нашего блюр-шейдера: `min(ceil(3σ), 32) + 2` текселя (та же формула,
/// что у bbox-scissor фильтр-пассов, EXPERIMENT.md п.16). Займётся
/// downscale-цепочка для σ > 4 — формулу менять синхронно с шейдером.
fn filter_bbox_inflate(filters: &[FilterFn]) -> f32 {
    filters
        .iter()
        .map(|f| match f {
            FilterFn::Blur(r) => (3.0 * r).ceil().min(32.0) + 2.0,
            _ => 0.0,
        })
        .sum()
}

/// Static/animated split (EXPERIMENT.md §2): строит план отрисовки
/// анимируемых сегментов поверх статичной полосы и проверяет, что перенос
/// сегментов в конец painter's order не меняет картинку.
///
/// Возвращает `Some(plan)` — команды сегментов, обёрнутые в реплей их
/// внешнего контекста (transform/clip/scroll-layer), в исходном порядке.
/// `None` — split в этом кадре небезопасен, рисовать монолитом:
/// - контекст сегмента содержит нереплеябельные группы (opacity/filter/
///   blend/mask поверх сегмента);
/// - сегмент несбалансирован по push/pop (не должно случаться по построению);
/// - статичная команда, рисуемая ПОЗЖЕ сегмента, пересекает его bbox —
///   сегмент, нарисованный поверх полосы, перекрыл бы её;
/// - в списке есть `BeginStickyLayer` (нелинейная зависимость от скролла).
pub fn anim_split_compose_plan(
    content: &[DisplayCommand],
    ranges: &[std::ops::Range<usize>],
) -> Option<(DisplayList, Vec<std::ops::Range<usize>>)> {
    // Диагностика причин отказа (LUMEN_FRAME_LOG=2) — один eprintln на кадр.
    macro_rules! bail {
        ($($why:tt)*) => {{
            if crate::frame_log_level() >= 2 {
                eprintln!("[frame:wgpu] anim-split bail: {}", format_args!($($why)*));
            }
            return None;
        }};
    }
    // Валидация диапазонов: отсортированы, не пересекаются, в пределах списка.
    let mut prev_end = 0usize;
    for r in ranges {
        if r.start < prev_end || r.end <= r.start || r.end > content.len() {
            bail!("malformed range {}..{}", r.start, r.end);
        }
        prev_end = r.end;
    }

    /// Push-команда пригодна для реплея вокруг сегмента: чистая
    /// геометрия/клип, без offscreen-групповой семантики.
    fn ctx_replayable(cmd: &DisplayCommand) -> bool {
        matches!(
            cmd,
            DisplayCommand::PushTransform { .. }
                | DisplayCommand::PushClipRect { .. }
                | DisplayCommand::PushClipRoundedRect { .. }
                | DisplayCommand::PushClipPath { .. }
                | DisplayCommand::PushScrollLayer { .. }
        )
    }

    fn pop_for_ctx(cmd: &DisplayCommand) -> DisplayCommand {
        match cmd {
            DisplayCommand::PushTransform { .. } => DisplayCommand::PopTransform,
            DisplayCommand::PushScrollLayer { .. } => DisplayCommand::PopScrollLayer,
            _ => DisplayCommand::PopClip,
        }
    }

    let mut ctx_stack: Vec<usize> = Vec::new(); // индексы активных Push-команд
    let mut mat_stack: Vec<Option<Mat4>> = Vec::new(); // накопленный 2D-аффинный transform (None = не-2D)
    let mut infl_stack: Vec<f32> = Vec::new(); // накопленная blur-инфляция активных фильтров
    let mut seg_bounds: Vec<SegBounds> = Vec::with_capacity(ranges.len());
    let mut seg_ctx: Vec<Vec<usize>> = Vec::with_capacity(ranges.len());
    let mut cur_range: Option<(usize, usize)> = None; // (индекс диапазона, глубина ctx на входе)
    let mut next_range = 0usize;
    let mut cur_bounds = SegBounds::Empty;
    // Первая статичная команда, конфликтующая с bbox сегмента → tail-split.
    let mut violation: Option<usize> = None;

    for (i, cmd) in content.iter().enumerate() {
        if cur_range.is_none() && next_range < ranges.len() && i == ranges[next_range].start {
            if let Some(&ci) = ctx_stack.iter().find(|&&ci| !ctx_replayable(&content[ci])) {
                bail!("ctx not replayable at {}: {}", i, content[ci].variant_name());
            }
            seg_ctx.push(ctx_stack.clone());
            cur_range = Some((next_range, ctx_stack.len()));
            cur_bounds = SegBounds::Empty;
        }

        let cur_mat = mat_stack.last().copied().flatten();
        let cur_infl = infl_stack.last().copied().unwrap_or(0.0);
        let identity_below = mat_stack.is_empty();

        match cmd {
            DisplayCommand::BeginStickyLayer { .. } => bail!("sticky layer at {}", i),
            DisplayCommand::PushTransform { matrix } => {
                let m = if matrix.is_2d_affine() {
                    if identity_below {
                        Some(*matrix)
                    } else {
                        cur_mat.map(|prev| prev.multiply(matrix))
                    }
                } else {
                    None
                };
                ctx_stack.push(i);
                mat_stack.push(m);
                infl_stack.push(cur_infl);
            }
            DisplayCommand::PushScrollLayer { scroll_x, scroll_y, .. } => {
                let t = Mat4::translation_2d(-*scroll_x, -*scroll_y);
                let m = if identity_below { Some(t) } else { cur_mat.map(|prev| prev.multiply(&t)) };
                ctx_stack.push(i);
                mat_stack.push(m);
                infl_stack.push(cur_infl);
            }
            DisplayCommand::PushFilter { filters, .. } => {
                ctx_stack.push(i);
                mat_stack.push(if identity_below { Some(Mat4::IDENTITY) } else { cur_mat });
                infl_stack.push(cur_infl + filter_bbox_inflate(filters));
            }
            DisplayCommand::PushBackdropFilter { filters, bounds } => {
                // Composite backdrop-фильтра пишет в `bounds` — учитываем его
                // как «рисующую» область (плюс blur-инфляция).
                let region = inflate_rect(*bounds, filter_bbox_inflate(filters));
                let eff = if identity_below {
                    SegBounds::Rect(region)
                } else {
                    match cur_mat {
                        Some(m) => SegBounds::Rect(affine_rect_bbox(&m, region)),
                        None => SegBounds::Unbounded,
                    }
                };
                if cur_range.is_some() {
                    cur_bounds.union(eff);
                } else if seg_hit(&seg_bounds, &eff) {
                    violation = Some(i);
                }
                ctx_stack.push(i);
                mat_stack.push(if identity_below { Some(Mat4::IDENTITY) } else { cur_mat });
                infl_stack.push(cur_infl + filter_bbox_inflate(filters));
            }
            DisplayCommand::PushClipRect { .. }
            | DisplayCommand::PushClipRoundedRect { .. }
            | DisplayCommand::PushClipPath { .. }
            | DisplayCommand::PushOpacity { .. }
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PushMaskImage { .. }
            | DisplayCommand::PushMaskLinearGradient { .. }
            | DisplayCommand::PushMaskRadialGradient { .. }
            | DisplayCommand::PushMaskConicGradient { .. }
            | DisplayCommand::PushMaskLayer { .. } => {
                ctx_stack.push(i);
                mat_stack.push(if identity_below { Some(Mat4::IDENTITY) } else { cur_mat });
                infl_stack.push(cur_infl);
            }
            DisplayCommand::PopTransform
            | DisplayCommand::PopClip
            | DisplayCommand::PopOpacity
            | DisplayCommand::PopBlendMode
            | DisplayCommand::PopMask
            | DisplayCommand::PopMaskLayer
            | DisplayCommand::PopFilter
            | DisplayCommand::PopBackdropFilter
            | DisplayCommand::PopScrollLayer
            | DisplayCommand::EndStickyLayer => {
                if ctx_stack.pop().is_none() {
                    bail!("unbalanced pop at {}", i); // malformed список
                }
                mat_stack.pop();
                infl_stack.pop();
                if let Some((_, depth)) = cur_range
                    && ctx_stack.len() < depth
                {
                    bail!("segment pop below entry depth at {}", i);
                }
            }
            _ => {
                if let Some(local) = draw_cmd_local_bbox(cmd) {
                    let eff = match local {
                        SegBounds::Empty => SegBounds::Empty,
                        SegBounds::Unbounded => SegBounds::Unbounded,
                        SegBounds::Rect(r) => {
                            let r = inflate_rect(r, cur_infl);
                            if identity_below {
                                SegBounds::Rect(r)
                            } else {
                                match cur_mat {
                                    Some(m) => SegBounds::Rect(affine_rect_bbox(&m, r)),
                                    None => SegBounds::Unbounded,
                                }
                            }
                        }
                    };
                    if cur_range.is_some() {
                        cur_bounds.union(eff);
                    } else if seg_hit(&seg_bounds, &eff) {
                        violation = Some(i);
                    }
                }
            }
        }

        if violation.is_some() {
            // Конфликт вне сегмента (cur_range == None): стеки заморожены на
            // моменте конфликта — по ним считается точка tail-cut ниже.
            break;
        }

        if let Some((ri, depth)) = cur_range
            && i + 1 == ranges[ri].end
        {
            if ctx_stack.len() != depth {
                bail!("segment unbalanced at {}", i);
            }
            seg_bounds.push(std::mem::replace(&mut cur_bounds, SegBounds::Empty));
            cur_range = None;
            next_range += 1;
        }
    }

    // Tail-split: точка отреза = начало внешней нереплеябельной группы
    // конфликтующей команды (иначе — сама команда). Всё от cut до конца
    // уходит в оверлей; сегменты, завершившиеся до cut, остаются сегментами.
    let (kept, tail): (usize, Option<(usize, Vec<usize>)>) = if let Some(vi) = violation {
        let split_pos = ctx_stack.iter().position(|&ci| !ctx_replayable(&content[ci]));
        let (cut, tail_ctx): (usize, Vec<usize>) = match split_pos {
            Some(p) => (ctx_stack[p], ctx_stack[..p].to_vec()),
            None => (vi, ctx_stack.clone()),
        };
        if cut * 2 < content.len() {
            bail!("tail cut {} too early (dl {})", cut, content.len());
        }
        // Симуляция баланса хвоста: он обязан закрыть реплеенный контекст
        // и выйти в ноль (весь список сбалансирован эмиттером).
        let mut depth = tail_ctx.len() as i64;
        for cmd in &content[cut..] {
            depth += layer_push_pop_delta(cmd);
            if depth < 0 {
                bail!("tail below entry depth");
            }
        }
        if depth != 0 {
            bail!("tail unbalanced at end: {depth}");
        }
        if crate::frame_log_level() >= 2 {
            eprintln!(
                "[frame:wgpu] anim-split tail cut at {} of {} (violation at {})",
                cut,
                content.len(),
                vi,
            );
        }
        (seg_bounds.len(), Some((cut, tail_ctx)))
    } else {
        (ranges.len(), None)
    };

    // План: каждый сегмент — реплей внешнего контекста + команды сегмента +
    // закрывающие Pop-ы в LIFO-порядке; затем хвост (закрывает реплеенный
    // контекст собственными Pop-ами — они в нём уже есть).
    let mut plan: DisplayList = Vec::new();
    let mut effective: Vec<std::ops::Range<usize>> = Vec::with_capacity(kept + 1);
    for (k, r) in ranges.iter().take(kept).enumerate() {
        for &ci in &seg_ctx[k] {
            plan.push(content[ci].clone());
        }
        plan.extend_from_slice(&content[r.clone()]);
        for &ci in seg_ctx[k].iter().rev() {
            plan.push(pop_for_ctx(&content[ci]));
        }
        effective.push(r.clone());
    }
    if let Some((cut, tail_ctx)) = tail {
        for &ci in &tail_ctx {
            plan.push(content[ci].clone());
        }
        plan.extend_from_slice(&content[cut..]);
        effective.push(cut..content.len());
    }
    Some((plan, effective))
}

/// Δ push/pop-глубины layer-команды: +1 для Push*/Begin*, −1 для Pop*/End*.
fn layer_push_pop_delta(cmd: &DisplayCommand) -> i64 {
    match cmd {
        DisplayCommand::PushTransform { .. }
        | DisplayCommand::PushClipRect { .. }
        | DisplayCommand::PushClipRoundedRect { .. }
        | DisplayCommand::PushClipPath { .. }
        | DisplayCommand::PushOpacity { .. }
        | DisplayCommand::PushBlendMode { .. }
        | DisplayCommand::PushFilter { .. }
        | DisplayCommand::PushBackdropFilter { .. }
        | DisplayCommand::PushMaskImage { .. }
        | DisplayCommand::PushMaskLinearGradient { .. }
        | DisplayCommand::PushMaskRadialGradient { .. }
        | DisplayCommand::PushMaskConicGradient { .. }
        | DisplayCommand::PushMaskLayer { .. }
        | DisplayCommand::PushScrollLayer { .. }
        | DisplayCommand::BeginStickyLayer { .. } => 1,
        DisplayCommand::PopTransform
        | DisplayCommand::PopClip
        | DisplayCommand::PopOpacity
        | DisplayCommand::PopBlendMode
        | DisplayCommand::PopFilter
        | DisplayCommand::PopBackdropFilter
        | DisplayCommand::PopMask
        | DisplayCommand::PopMaskLayer
        | DisplayCommand::PopScrollLayer
        | DisplayCommand::EndStickyLayer => -1,
        _ => 0,
    }
}

/// Пересекает ли `eff` какой-либо из завершённых сегментов.
fn seg_hit(seg_bounds: &[SegBounds], eff: &SegBounds) -> bool {
    if matches!(eff, SegBounds::Empty) {
        return false;
    }
    seg_bounds.iter().any(|s| match (s, eff) {
        (SegBounds::Empty, _) | (_, SegBounds::Empty) => false,
        (SegBounds::Unbounded, _) | (_, SegBounds::Unbounded) => true,
        (SegBounds::Rect(a), SegBounds::Rect(b)) => rects_overlap(a, b),
    })
}

/// Результат сравнения двух display-list-ов.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffResult {
    /// Если true, то оба display list-а идентичны — можно пропустить GPU upload.
    pub identical: bool,
    ///累積bounding rectangle всех команд, которые изменились или добавились.
    /// Используется для dirty-rect tracking в renderer-е.
    /// `Rect { x: f32::NAN, y: f32::NAN, width: 0.0, height: 0.0 }` если нет изменений.
    pub changed_rects: Rect,
}

impl DiffResult {
    /// Создаёт DiffResult для идентичных display list-ов.
    #[inline]
    pub fn identical() -> Self {
        Self {
            identical: true,
            changed_rects: Rect {
                x: f32::NAN,
                y: f32::NAN,
                width: 0.0,
                height: 0.0,
            },
        }
    }

    /// Создаёт DiffResult для изменённых display list-ов с заданным bounding rect.
    #[inline]
    pub fn changed(changed_rects: Rect) -> Self {
        Self {
            identical: false,
            changed_rects,
        }
    }
}

/// Сравнивает два display list-а по Debug hash каждой команды.
/// Возвращает DiffResult с флагом `identical` и bounding rectangle всех изменений.
///
/// Алгоритм:
/// 1. Если длины списков различаются → список изменился
/// 2. Для каждой пары команд вычисляем Debug hash и сравниваем
/// 3. Если все хеши совпадают → `identical = true`
/// 4. Если есть отличия → собираем bounding rect всех `rect`-полей из изменённых команд
pub fn diff_display_lists(prev: &[DisplayCommand], next: &[DisplayCommand]) -> DiffResult {
    // Быстрая проверка: если длины различаются, список точно изменился.
    if prev.len() != next.len() {
        return DiffResult::changed(union_all_rects(next));
    }

    // Вычисляем hashes обеих последовательностей и сравниваем поэлементно.
    let mut all_identical = true;
    let mut changed_rects = Rect {
        x: f32::INFINITY,
        y: f32::INFINITY,
        width: 0.0,
        height: 0.0,
    };

    for (prev_cmd, next_cmd) in prev.iter().zip(next.iter()) {
        // Debug-представление через HashFmt — без String-аллокаций на команду.
        let prev_hash = hash_one_command(prev_cmd);
        let next_hash = hash_one_command(next_cmd);

        if prev_hash != next_hash {
            all_identical = false;
            // Собираем rect из обеих команд (старая + новая).
            if let Some(rect) = get_command_rect(prev_cmd) {
                changed_rects = union_rects(changed_rects, rect);
            }
            if let Some(rect) = get_command_rect(next_cmd) {
                changed_rects = union_rects(changed_rects, rect);
            }
        }
    }

    if all_identical {
        DiffResult::identical()
    } else {
        DiffResult::changed(changed_rects)
    }
}

/// Извлекает rect из DisplayCommand, если применимо.
fn get_command_rect(cmd: &DisplayCommand) -> Option<Rect> {
    match cmd {
        DisplayCommand::FillRect { rect, .. } => Some(*rect),
        DisplayCommand::FillRoundedRect { rect, .. } => Some(*rect),
        DisplayCommand::DrawBorder { rect, .. } => Some(*rect),
        DisplayCommand::DrawOutline { rect, .. } => Some(*rect),
        DisplayCommand::DrawText { rect, .. } => Some(*rect),
        DisplayCommand::DrawImage { rect, .. } => Some(*rect),
        DisplayCommand::LazyImageSlot { rect, .. } => Some(*rect),
        DisplayCommand::DrawBackgroundImage { rect, .. } => Some(*rect),
        DisplayCommand::DrawLinearGradient { rect, .. } => Some(*rect),
        DisplayCommand::DrawRadialGradient { rect, .. } => Some(*rect),
        DisplayCommand::DrawConicGradient { rect, .. } => Some(*rect),
        _ => None,
    }
}

/// Объединяет two rectangles в их bounding rect.
fn union_rects(a: Rect, b: Rect) -> Rect {
    if a.width == 0.0 && a.height == 0.0 {
        return b;
    }
    if b.width == 0.0 && b.height == 0.0 {
        return a;
    }

    let x1 = a.x.min(b.x);
    let y1 = a.y.min(b.y);
    let x2 = (a.x + a.width).max(b.x + b.width);
    let y2 = (a.y + a.height).max(b.y + b.height);

    Rect {
        x: x1,
        y: y1,
        width: (x2 - x1).max(0.0),
        height: (y2 - y1).max(0.0),
    }
}

/// Собирает bounding rect всех команд в display list.
fn union_all_rects(cmds: &[DisplayCommand]) -> Rect {
    let mut result = Rect {
        x: f32::INFINITY,
        y: f32::INFINITY,
        width: 0.0,
        height: 0.0,
    };

    for cmd in cmds {
        if let Some(rect) = get_command_rect(cmd) {
            result = union_rects(result, rect);
        }
    }

    // Если нет ни одного rect-команды, вернуть нулевой rect.
    if result.x == f32::INFINITY {
        result = Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
    }

    result
}

pub fn serialize_display_list(dl: &[DisplayCommand]) -> String {
    let mut out = String::new();
    for cmd in dl {
        match cmd {
            DisplayCommand::FillRect { rect, color } => {
                out.push_str(&format!(
                    "FillRect ({:.2}, {:.2}, {:.2}, {:.2}) #{:02x}{:02x}{:02x}{:02x}\n",
                    rect.x, rect.y, rect.width, rect.height,
                    color.r, color.g, color.b, color.a,
                ));
            }
            DisplayCommand::FillRoundedRect { rect, color, radii } => {
                out.push_str(&format!(
                    "FillRoundedRect ({:.2}, {:.2}, {:.2}, {:.2}) #{:02x}{:02x}{:02x}{:02x} r=[{:.2},{:.2},{:.2},{:.2}]\n",
                    rect.x, rect.y, rect.width, rect.height,
                    color.r, color.g, color.b, color.a,
                    radii.tl, radii.tr, radii.br, radii.bl,
                ));
            }
            DisplayCommand::DrawBorder {
                rect,
                widths: [wt, wr, wb, wl],
                colors: [ct, cr, cb, cl],
                styles: [st, sr, sb, sl],
                radii: _,
            } => {
                out.push_str(&format!(
                    "DrawBorder ({:.2}, {:.2}, {:.2}, {:.2}) \
                     w=[{:.2},{:.2},{:.2},{:.2}] \
                     c=[#{:02x}{:02x}{:02x}{:02x},#{:02x}{:02x}{:02x}{:02x},\
                        #{:02x}{:02x}{:02x}{:02x},#{:02x}{:02x}{:02x}{:02x}]",
                    rect.x, rect.y, rect.width, rect.height,
                    wt, wr, wb, wl,
                    ct.r, ct.g, ct.b, ct.a,
                    cr.r, cr.g, cr.b, cr.a,
                    cb.r, cb.g, cb.b, cb.a,
                    cl.r, cl.g, cl.b, cl.a,
                ));
                let any_non_solid = ![*st, *sr, *sb, *sl]
                    .iter()
                    .all(|s| matches!(s, BorderStyle::Solid | BorderStyle::None));
                if any_non_solid {
                    out.push_str(&format!(
                        " s=[{},{},{},{}]",
                        border_style_short(*st),
                        border_style_short(*sr),
                        border_style_short(*sb),
                        border_style_short(*sl),
                    ));
                }
                out.push('\n');
            }
            DisplayCommand::DrawText {
                rect, text, font_size, color, font_family, font_weight, font_style,
                font_stretch, font_variation_axes, font_features, font_palette, tab_size: _,
                highlight_name: _, text_orientation: _,
            } => {
                out.push_str(&format!(
                    "DrawText ({:.2}, {:.2}, {:.2}, {:.2}) {:?} {:.2} #{:02x}{:02x}{:02x}{:02x}",
                    rect.x, rect.y, rect.width, rect.height,
                    text,
                    font_size,
                    color.r, color.g, color.b, color.a,
                ));
                if !font_family.is_empty() {
                    out.push_str(" family=[");
                    for (i, name) in font_family.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        out.push_str(&format!("{name:?}"));
                    }
                    out.push(']');
                }
                if *font_weight != FontWeight::NORMAL {
                    out.push_str(&format!(" w={}", font_weight.0));
                }
                if *font_style != FontStyle::Normal {
                    out.push_str(match font_style {
                        FontStyle::Italic => " style=italic",
                        FontStyle::Oblique => " style=oblique",
                        FontStyle::Normal => "",
                    });
                }
                if *font_stretch != FontStretch::NORMAL {
                    // Проценты, как в layout-снапшоте: stretch=75 ≡ condensed.
                    out.push_str(&format!(" stretch={}", font_stretch.as_percent()));
                }
                if !font_variation_axes.is_empty() {
                    out.push_str(" var=[");
                    for (i, (tag, val)) in font_variation_axes.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        let tag_str = std::str::from_utf8(tag).unwrap_or("????");
                        out.push_str(&format!("{tag_str:?}={val}"));
                    }
                    out.push(']');
                }
                if !font_features.is_empty() {
                    out.push_str(" feat=[");
                    for (i, (tag, val)) in font_features.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        let tag_str = std::str::from_utf8(tag).unwrap_or("????");
                        out.push_str(&format!("{tag_str:?}={val}"));
                    }
                    out.push(']');
                }
                match font_palette {
                    None => {}
                    Some(FontPaletteSelection::Light) => out.push_str(" palette=light"),
                    Some(FontPaletteSelection::Dark) => out.push_str(" palette=dark"),
                    Some(FontPaletteSelection::Custom { base_palette, overrides }) => {
                        out.push_str(&format!(
                            " palette=custom(base={base_palette},overrides={})",
                            overrides.len()
                        ));
                    }
                }
                out.push('\n');
            }
            DisplayCommand::DrawOutline { rect, width, style, color, offset } => {
                out.push_str(&format!(
                    "DrawOutline ({:.2}, {:.2}, {:.2}, {:.2}) w={:.2} \
                     s={} #{:02x}{:02x}{:02x}{:02x}",
                    rect.x, rect.y, rect.width, rect.height,
                    width,
                    outline_style_name(*style),
                    color.r, color.g, color.b, color.a,
                ));
                if *offset != 0.0 {
                    out.push_str(&format!(" off={offset:.2}"));
                }
                out.push('\n');
            }
            DisplayCommand::DrawImage { rect, src, alt, object_fit, object_position, .. } => {
                out.push_str(&format!(
                    "DrawImage ({:.2}, {:.2}, {:.2}, {:.2}) src={src:?} alt={alt:?}",
                    rect.x, rect.y, rect.width, rect.height,
                ));
                if *object_fit != ObjectFit::Fill {
                    out.push_str(&format!(" fit={}", object_fit_name(*object_fit)));
                }
                if *object_position != ObjectPosition::default() {
                    out.push_str(&format!(
                        " pos={} {}",
                        position_component_name(object_position.x),
                        position_component_name(object_position.y),
                    ));
                }
                out.push('\n');
            }
            DisplayCommand::LazyImageSlot { rect, node_id, src, .. } => {
                out.push_str(&format!(
                    "LazyImageSlot ({:.2}, {:.2}, {:.2}, {:.2}) nid={node_id} src={src:?}\n",
                    rect.x, rect.y, rect.width, rect.height,
                ));
            }
            DisplayCommand::DrawBackgroundImage { rect, src, size, position, repeat, .. } => {
                out.push_str(&format!(
                    "DrawBackgroundImage ({:.2}, {:.2}, {:.2}, {:.2}) src={src:?} size={size:?} pos=({:?},{:?}) repeat={repeat:?}\n",
                    rect.x, rect.y, rect.width, rect.height,
                    position.x, position.y,
                ));
            }
            DisplayCommand::DrawLinearGradient { rect, angle_deg, stops, repeating } => {
                out.push_str(&format!(
                    "DrawLinearGradient ({:.2}, {:.2}, {:.2}, {:.2}) angle={angle_deg:.1}deg stops={} repeating={repeating}\n",
                    rect.x, rect.y, rect.width, rect.height, stops.len(),
                ));
            }
            DisplayCommand::DrawRadialGradient {
                rect, center_x_pct, center_y_pct, radius_x, radius_y, stops, repeating,
            } => {
                out.push_str(&format!(
                    "DrawRadialGradient ({:.2}, {:.2}, {:.2}, {:.2}) center=({center_x_pct:.2},{center_y_pct:.2}) radii=({radius_x:.2},{radius_y:.2}) stops={} repeating={repeating}\n",
                    rect.x, rect.y, rect.width, rect.height, stops.len(),
                ));
            }
            DisplayCommand::DrawConicGradient { rect, center_x_pct, center_y_pct, from_angle_deg, stops, repeating } => {
                out.push_str(&format!(
                    "DrawConicGradient ({:.2}, {:.2}, {:.2}, {:.2}) center=({center_x_pct:.2},{center_y_pct:.2}) from={from_angle_deg:.1}deg stops={} repeating={repeating}\n",
                    rect.x, rect.y, rect.width, rect.height, stops.len(),
                ));
            }
            DisplayCommand::PushClipRect { rect } => {
                out.push_str(&format!(
                    "PushClipRect ({:.2}, {:.2}, {:.2}, {:.2})\n",
                    rect.x, rect.y, rect.width, rect.height,
                ));
            }
            DisplayCommand::PushClipRoundedRect { rect, radii } => {
                out.push_str(&format!(
                    "PushClipRoundedRect ({:.2}, {:.2}, {:.2}, {:.2}) radii=[{:.2}, {:.2}, {:.2}, {:.2}]\n",
                    rect.x, rect.y, rect.width, rect.height,
                    radii[0], radii[1], radii[2], radii[3],
                ));
            }
            DisplayCommand::PushClipPath { shape } => {
                match shape {
                    ResolvedClipShape::Circle { cx, cy, r } => {
                        out.push_str(&format!(
                            "PushClipPath circle({cx:.2}, {cy:.2}, r={r:.2})\n"
                        ));
                    }
                    ResolvedClipShape::Ellipse { cx, cy, rx, ry } => {
                        out.push_str(&format!(
                            "PushClipPath ellipse({cx:.2}, {cy:.2}, rx={rx:.2}, ry={ry:.2})\n"
                        ));
                    }
                    ResolvedClipShape::Polygon { verts, even_odd } => {
                        out.push_str(if *even_odd {
                            "PushClipPath polygon evenodd("
                        } else {
                            "PushClipPath polygon("
                        });
                        for (i, (x, y)) in verts.iter().enumerate() {
                            if i > 0 {
                                out.push_str(", ");
                            }
                            out.push_str(&format!("{x:.2} {y:.2}"));
                        }
                        out.push_str(")\n");
                    }
                }
            }
            DisplayCommand::PopClip => {
                out.push_str("PopClip\n");
            }
            DisplayCommand::PushOpacity { alpha, .. } => {
                out.push_str(&format!("PushOpacity {alpha:.3}\n"));
            }
            DisplayCommand::PopOpacity => {
                out.push_str("PopOpacity\n");
            }
            DisplayCommand::PushBlendMode { mode, bounds } => {
                out.push_str(&format!(
                    "PushBlendMode {} bounds=({:.0},{:.0},{:.0},{:.0})\n",
                    blend_mode_name(*mode), bounds.x, bounds.y, bounds.width, bounds.height,
                ));
            }
            DisplayCommand::PopBlendMode => {
                out.push_str("PopBlendMode\n");
            }
            DisplayCommand::DrawLayerSnapshot { id, rect, alpha } => {
                out.push_str(&format!(
                    "DrawLayerSnapshot id={id} ({:.2}, {:.2}, {:.2}, {:.2}) alpha={alpha:.3}\n",
                    rect.x, rect.y, rect.width, rect.height,
                ));
            }
            DisplayCommand::PushTransform { matrix } => {
                // 2D affine: x'=a·x+c·y+e, y'=b·x+d·y+f. Печатаем 6 значимых
                // компонент в snapshot-friendly формате — детерминированный
                // обход, не зависящий от Z/W-колонок (Phase 0 — 2D).
                let [a, b, c, d, e, f] = crate::matrix_util::mat4_to_2d_affine(matrix);
                out.push_str(&format!(
                    "PushTransform [{a:.3} {b:.3} {c:.3} {d:.3} {e:.3} {f:.3}]\n"
                ));
            }
            DisplayCommand::PopTransform => {
                out.push_str("PopTransform\n");
            }
            DisplayCommand::PushFilter { filters, bounds } => {
                let names: Vec<&str> = filters.iter().map(filter_fn_name).collect();
                let bounds_str = bounds
                    .map(|b| format!(" bounds=({:.0},{:.0},{:.0},{:.0})", b.x, b.y, b.width, b.height))
                    .unwrap_or_default();
                out.push_str(&format!("PushFilter [{}]{}\n", names.join(", "), bounds_str));
            }
            DisplayCommand::PopFilter => {
                out.push_str("PopFilter\n");
            }
            DisplayCommand::PushBackdropFilter { filters, bounds } => {
                let names: Vec<&str> = filters.iter().map(filter_fn_name).collect();
                out.push_str(&format!(
                    "PushBackdropFilter [{fns}] bounds=({x:.0},{y:.0},{w:.0},{h:.0})\n",
                    fns = names.join(", "),
                    x = bounds.x, y = bounds.y, w = bounds.width, h = bounds.height,
                ));
            }
            DisplayCommand::PopBackdropFilter => {
                out.push_str("PopBackdropFilter\n");
            }
            DisplayCommand::BeginStickyLayer { flow_rect, top, bottom, left, right } => {
                out.push_str(&format!(
                    "BeginStickyLayer flow=({:.0},{:.0},{:.0},{:.0}) top={} bottom={} left={} right={}\n",
                    flow_rect.x, flow_rect.y, flow_rect.width, flow_rect.height,
                    top.map_or("auto".to_string(), |v| format!("{v:.0}")),
                    bottom.map_or("auto".to_string(), |v| format!("{v:.0}")),
                    left.map_or("auto".to_string(), |v| format!("{v:.0}")),
                    right.map_or("auto".to_string(), |v| format!("{v:.0}")),
                ));
            }
            DisplayCommand::EndStickyLayer => {
                out.push_str("EndStickyLayer\n");
            }
            DisplayCommand::BeginFixedLayer => {
                out.push_str("BeginFixedLayer\n");
            }
            DisplayCommand::EndFixedLayer => {
                out.push_str("EndFixedLayer\n");
            }
            DisplayCommand::PushScrollLayer { clip_rect, scroll_x, scroll_y } => {
                out.push_str(&format!(
                    "PushScrollLayer clip=({:.2},{:.2},{:.2},{:.2}) scroll=({:.2},{:.2})\n",
                    clip_rect.x, clip_rect.y, clip_rect.width, clip_rect.height, scroll_x, scroll_y,
                ));
            }
            DisplayCommand::PopScrollLayer => {
                out.push_str("PopScrollLayer\n");
            }
            DisplayCommand::PushMaskImage { rect, src, size, repeat, .. } => {
                out.push_str(&format!(
                    "PushMaskImage ({:.2}, {:.2}, {:.2}, {:.2}) src={src:?} size={size:?} repeat={repeat:?}\n",
                    rect.x, rect.y, rect.width, rect.height,
                ));
            }
            DisplayCommand::PushMaskLinearGradient { rect, angle_deg, stops, repeating } => {
                out.push_str(&format!(
                    "PushMaskLinearGradient ({:.2}, {:.2}, {:.2}, {:.2}) angle={angle_deg:.1} stops={} repeating={repeating}\n",
                    rect.x, rect.y, rect.width, rect.height, stops.len(),
                ));
            }
            DisplayCommand::PushMaskRadialGradient { rect, center_x_pct, center_y_pct, stops, repeating } => {
                out.push_str(&format!(
                    "PushMaskRadialGradient ({:.2}, {:.2}, {:.2}, {:.2}) center=({:.2},{:.2}) stops={} repeating={repeating}\n",
                    rect.x, rect.y, rect.width, rect.height, center_x_pct, center_y_pct, stops.len(),
                ));
            }
            DisplayCommand::PushMaskConicGradient { rect, center_x_pct, center_y_pct, from_angle_deg, stops, repeating } => {
                out.push_str(&format!(
                    "PushMaskConicGradient ({:.2}, {:.2}, {:.2}, {:.2}) center=({:.2},{:.2}) from={from_angle_deg:.1}deg stops={} repeating={repeating}\n",
                    rect.x, rect.y, rect.width, rect.height, center_x_pct, center_y_pct, stops.len(),
                ));
            }
            DisplayCommand::PopMask => {
                out.push_str("PopMask\n");
            }
            DisplayCommand::PushMaskLayer { rect, mode } => {
                out.push_str(&format!(
                    "PushMaskLayer ({:.2}, {:.2}, {:.2}, {:.2}) mode={mode:?}\n",
                    rect.x, rect.y, rect.width, rect.height,
                ));
            }
            DisplayCommand::PopMaskLayer => {
                out.push_str("PopMaskLayer\n");
            }
            DisplayCommand::DrawSvgPath { vertices, color } => {
                out.push_str(&format!(
                    "DrawSvgPath tris={} #{:02x}{:02x}{:02x}{:02x}\n",
                    vertices.len() / 3,
                    color.r, color.g, color.b, color.a,
                ));
            }
            DisplayCommand::DrawSvgFill { contours, color } => {
                let pts: usize = contours.iter().map(std::vec::Vec::len).sum();
                out.push_str(&format!(
                    "DrawSvgFill contours={} pts={} #{:02x}{:02x}{:02x}{:02x}\n",
                    contours.len(),
                    pts,
                    color.r, color.g, color.b, color.a,
                ));
            }
            DisplayCommand::DrawSvgStroke { contours, color, params } => {
                let pts: usize = contours.iter().map(std::vec::Vec::len).sum();
                out.push_str(&format!(
                    "DrawSvgStroke contours={} pts={} w={:.2} dash={} #{:02x}{:02x}{:02x}{:02x}\n",
                    contours.len(),
                    pts,
                    params.half_width * 2.0,
                    params.dasharray.len(),
                    color.r, color.g, color.b, color.a,
                ));
            }
            DisplayCommand::BoxModelOverlay { margin, border, padding, content } => {
                out.push_str(&format!(
                    "BoxModelOverlay margin=({:.0},{:.0},{:.0},{:.0}) border=({:.0},{:.0},{:.0},{:.0}) padding=({:.0},{:.0},{:.0},{:.0}) content=({:.0},{:.0},{:.0},{:.0})\n",
                    margin.x, margin.y, margin.width, margin.height,
                    border.x, border.y, border.width, border.height,
                    padding.x, padding.y, padding.width, padding.height,
                    content.x, content.y, content.width, content.height,
                ));
            }
            DisplayCommand::DrawScrollbar { track_rect, thumb_rect, vertical, .. } => {
                out.push_str(&format!(
                    "DrawScrollbar {} track=({:.0},{:.0},{:.0},{:.0}) thumb=({:.0},{:.0},{:.0},{:.0})\n",
                    if *vertical { "vertical" } else { "horizontal" },
                    track_rect.x, track_rect.y, track_rect.width, track_rect.height,
                    thumb_rect.x, thumb_rect.y, thumb_rect.width, thumb_rect.height,
                ));
            }
            DisplayCommand::PageBreak => {
                out.push_str("PageBreak\n");
            }
            DisplayCommand::DrawCrossFade { dest, src_a, src_b, progress } => {
                out.push_str(&format!(
                    "DrawCrossFade ({:.2}, {:.2}, {:.2}, {:.2}) a={src_a:?} b={src_b:?} p={progress:.3}\n",
                    dest.x, dest.y, dest.width, dest.height,
                ));
            }
        }
    }
    out
}

fn filter_fn_name(f: &FilterFn) -> &'static str {
    match f {
        FilterFn::Blur(_) => "blur",
        FilterFn::Brightness(_) => "brightness",
        FilterFn::Contrast(_) => "contrast",
        FilterFn::Grayscale(_) => "grayscale",
        FilterFn::HueRotate(_) => "hue-rotate",
        FilterFn::Invert(_) => "invert",
        FilterFn::Opacity(_) => "opacity",
        FilterFn::Saturate(_) => "saturate",
        FilterFn::Sepia(_) => "sepia",
    }
}

fn outline_style_name(s: OutlineStyle) -> &'static str {
    match s {
        OutlineStyle::None => "none",
        OutlineStyle::Auto => "auto",
        OutlineStyle::Solid => "solid",
        OutlineStyle::Dashed => "dashed",
        OutlineStyle::Dotted => "dotted",
    }
}

fn blend_mode_name(m: BlendMode) -> &'static str {
    match m {
        BlendMode::Normal => "normal",
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
        BlendMode::Darken => "darken",
        BlendMode::Lighten => "lighten",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::HardLight => "hard-light",
        BlendMode::SoftLight => "soft-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Color => "color",
        BlendMode::Luminosity => "luminosity",
        BlendMode::PlusLighter => "plus-lighter",
    }
}

pub fn build_display_list(root: &LayoutBox) -> DisplayList {
    let mut list = Vec::new();
    walk(root, &mut list, 1.0, None);
    list
}

/// Like [`build_display_list`] but applies `::selection` CSS highlight styles
/// to text fragments that fall within `sel`.
///
/// Pass `Some(&SelectionHighlight)` to enable `::selection` rendering — selected
/// text receives a `FillRect` background (from `sel.bg_color`) and optionally an
/// overridden text colour (from `sel.fg_color`). Pass `None` to get the same
/// output as `build_display_list`.
///
/// This function is a pure function per ADR-008 Invariant 3: it depends only on
/// the function parameters and carries no hidden global state.
pub fn build_display_list_with_selection(
    root: &LayoutBox,
    sel: Option<&SelectionHighlight>,
) -> DisplayList {
    let mut list = Vec::new();
    walk(root, &mut list, 1.0, sel);
    list
}

/// Like `build_display_list` but applies compositor animation overrides per node.
///
/// For each node that has an entry in `anim`, opacity and/or transform values
/// from the override replace the style's values in the emitted PushOpacity /
/// PushTransform commands. Layout geometry (rect, padding, children) is unchanged —
/// this avoids a full relayout while still producing correct frames.
///
/// Pass `None` (or an empty frame) to fall back to the same output as
/// `build_display_list`.
pub fn build_display_list_with_anim(
    root: &LayoutBox,
    anim: Option<&CompositorAnimFrame>,
) -> DisplayList {
    let mut list = Vec::new();
    walk_with_anim(root, anim, &mut list, 1.0);
    list
}

/// Билдер display list-а, **уважающий painting order** (CSS 2.1 Appendix E).
///
/// Разница с [`build_display_list`]: для документа с несколькими
/// stacking-контекстами child-SC рисуются в правильных слотах parent SC
/// (negative-z до контента, auto/0 и positive-z после).
///
/// Phase 0 упрощение: фазы `BlockBackgrounds` / `Floats` / `InlineContent`
/// лумпятся в один «контент» bucket per SC, эмитимый при фазе
/// `InlineContent`. Точное разделение по фазам 3/4/5 (block vs float vs
/// inline-level descendant) — отдельная задача после flex / float layout.
///
/// Bucket-per-SC структура:
/// - `pre`: layer-ops, открываемые при входе в SC (PushOpacity / PushBlendMode
///   / PushClipRect) — собственный SC-owner с `opacity<1` / `mix-blend-mode`
///   ≠ normal / `overflow` ≠ visible.
/// - `root_bg`: bg/border SC-owner box-а (фаза 1 «RootBackground»).
/// - `contents`: всё остальное содержимое SC (descendants, исключая собственно
///   SC-creating потомков — те идут в свои buckets).
/// - `post`: парные Pop-команды, в обратном порядке к `pre`.
///
/// **Layer-ops nesting invariant:** `pre` / `post` SC-owner-а охватывают
/// `root_bg + contents` собственного SC **и все child-SC потомков**. Это
/// реализуется через `PaintPhase::CloseLayer`: `post` эмитится в `CloseLayer`,
/// которая добавляется в `paint_sc` последней — уже ПОСЛЕ всех дочерних SC.
/// Таким образом Pop-команды родителя (PopTransform и т.д.) приходят после
/// Push-команд всех детей — nested transforms и opacity корректно компонуются
/// (BUG-139). Старый подход (post в InlineContent) был Phase-0-заглушкой.
pub fn build_display_list_ordered(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
) -> (DisplayList, ProvenanceIndex) {
    build_display_list_ordered_dpr(root, tree, order, 1.0)
}

/// Like [`build_display_list_ordered`] but resolves `image-set()` background
/// variants for the device pixel ratio `dpr` (CSS Images L4 §5). Shell passes
/// the window scale factor; `build_display_list_ordered` defaults to `1.0`.
///
/// The returned [`ProvenanceIndex`] (ADR-025 §3, DEVX-7 п.4) is built by
/// translating the `RawSpan`s `fill_buckets` recorded — local to one
/// `ScBucket` field — into global indices at the exact point each field is
/// flushed into `out` below. This keeps span-tracking decoupled from the
/// four-phase bucket assembly: `fill_buckets` never sees the final list.
pub fn build_display_list_ordered_dpr(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
    dpr: f32,
) -> (DisplayList, ProvenanceIndex) {
    let n_sc = tree.contexts.len().max(1);
    let mut buckets: Vec<ScBucket> = vec![ScBucket::default(); n_sc];
    let mut next_sc_id: u32 = 1;
    let mut split = SplitTracker::disabled();
    let mut raw_spans: Vec<RawSpan> = Vec::new();
    fill_buckets(root, StackingContextId::ROOT, &mut next_sc_id, &mut buckets, true, None, dpr, &[], &mut split, &mut raw_spans);

    let mut spans_by_field: HashMap<(u32, BucketField), Vec<RawSpan>> = HashMap::new();
    for rs in raw_spans {
        spans_by_field.entry((rs.sc, rs.field)).or_default().push(rs);
    }

    let mut out = Vec::new();
    let mut final_spans: Vec<ProvenanceSpan> = Vec::new();
    let mut flush = |field_vec: &mut Vec<DisplayCommand>,
                      sc: u32,
                      field: BucketField,
                      out: &mut Vec<DisplayCommand>,
                      final_spans: &mut Vec<ProvenanceSpan>| {
        let offset = out.len();
        out.append(field_vec);
        if let Some(list) = spans_by_field.remove(&(sc, field)) {
            for rs in list {
                final_spans.push(ProvenanceSpan {
                    range: (offset + rs.range.start)..(offset + rs.range.end),
                    origin: rs.origin,
                    fragment: rs.fragment,
                    // Filled below by `annotate_clip_depth` once `out` is complete.
                    clip_depth: 0,
                });
            }
        }
    };
    for (sc_id, phase) in &order.steps {
        let idx = sc_id.0 as usize;
        if idx >= buckets.len() {
            continue;
        }
        let bucket = &mut buckets[idx];
        match phase {
            PaintPhase::RootBackground => {
                flush(&mut bucket.pre, sc_id.0, BucketField::Pre, &mut out, &mut final_spans);
                flush(&mut bucket.root_bg, sc_id.0, BucketField::RootBg, &mut out, &mut final_spans);
            }
            PaintPhase::InlineContent => {
                flush(&mut bucket.contents, sc_id.0, BucketField::Contents, &mut out, &mut final_spans);
                // post (PopTransform / PopOpacity / etc.) is now in CloseLayer —
                // emitted AFTER all child SCs so nested transforms compose correctly
                // (BUG-139). Do NOT move post back here.
            }
            // CloseLayer is emitted last in paint_sc, after all child SCs, so the
            // parent's Pop-commands wrap the children's Push-commands correctly.
            PaintPhase::CloseLayer => {
                flush(&mut bucket.post, sc_id.0, BucketField::Post, &mut out, &mut final_spans);
            }
            // Phase 0: BlockBackgrounds / Floats merged into InlineContent;
            // marker-фазы (NegativeZ / PositionedAndZAuto / PositiveZ) в
            // выводе `PaintOrder::from_tree` не появляются — рекурсия
            // энкодирует их позицию через линейный порядок.
            _ => {}
        }
    }
    annotate_clip_depth(&out, &mut final_spans);
    let index = ProvenanceIndex { spans: final_spans };
    #[cfg(debug_assertions)]
    crate::invariants::check(&out, &index, root);
    (out, index)
}

/// Post-processes `spans` in place with `clip_depth` (ADR-025 §3): the number
/// of open rect/rounded-rect/path clips at each span's first command. A
/// single linear scan over the finished list is simpler and cheaper than
/// threading a running counter through `fill_buckets`'s recursion, and gives
/// the same answer since clip nesting is a property of the final painting
/// order, not of the bucket-assembly process that produced it.
fn annotate_clip_depth(out: &[DisplayCommand], spans: &mut [ProvenanceSpan]) {
    let mut depth_at: Vec<u16> = Vec::with_capacity(out.len() + 1);
    let mut depth: i32 = 0;
    for cmd in out {
        depth_at.push(depth.max(0) as u16);
        match cmd {
            DisplayCommand::PushClipRect { .. }
            | DisplayCommand::PushClipRoundedRect { .. }
            | DisplayCommand::PushClipPath { .. } => depth += 1,
            DisplayCommand::PopClip => depth -= 1,
            _ => {}
        }
    }
    depth_at.push(depth.max(0) as u16);
    for s in spans.iter_mut() {
        s.clip_depth = depth_at.get(s.range.start).copied().unwrap_or(0);
    }
}

/// Like [`build_display_list_ordered`] but applies compositor animation overrides per node.
///
/// Opacity and transform values from `anim` replace the style's values in the emitted
/// PushOpacity / PushTransform commands. Stacking context paint ordering is preserved.
/// Pass `None` to get the same output as `build_display_list_ordered`.
pub fn build_display_list_ordered_with_anim(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
    anim: Option<&CompositorAnimFrame>,
) -> DisplayList {
    build_display_list_ordered_with_anim_dpr(root, tree, order, anim, 1.0)
}

/// Like [`build_display_list_ordered_with_anim`] but resolves `image-set()`
/// background variants for the device pixel ratio `dpr` (CSS Images L4 §5).
pub fn build_display_list_ordered_with_anim_dpr(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
    anim: Option<&CompositorAnimFrame>,
    dpr: f32,
) -> DisplayList {
    ordered_with_anim_internal(root, tree, order, anim, dpr, false).0
}

/// Static/animated split (EXPERIMENT.md §2): как
/// [`build_display_list_ordered_with_anim`], но дополнительно возвращает
/// отсортированные непересекающиеся диапазоны команд итогового списка,
/// содержимое которых зависит от anim-override-ов (поддеревья анимируемых
/// узлов целиком: layer-ops + собственные команды + потомки).
///
/// Пустой Vec означает «split в этом кадре неприменим» (нет overrides,
/// override на корне, либо SC-потомок разрывает inline-спан non-SC узла) —
/// список при этом валиден и идентичен обычной anim-сборке.
///
/// Скролл-композитор использует диапазоны, чтобы кэшировать статичную часть
/// страницы в полосе (ключ полосы считается ТОЛЬКО по статике), а анимируемые
/// сегменты рисовать поверх каждым кадром.
pub fn build_display_list_ordered_with_anim_split(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
    anim: Option<&CompositorAnimFrame>,
) -> (DisplayList, Vec<std::ops::Range<usize>>) {
    ordered_with_anim_internal(root, tree, order, anim, 1.0, true)
}

/// Трекер static/animated split — собирается в `fill_buckets`, конвертируется
/// в диапазоны итогового списка при сборке бакетов.
struct SplitTracker {
    /// Собираем ли split-метаданные (false в обычных сборках — нулевая цена).
    enabled: bool,
    /// SC-owner-ы с anim-override: диапазон = RootBackground..CloseLayer их SC.
    animated_scs: Vec<u32>,
    /// Спаны non-SC override-узлов в `contents` их бакета: (sc, start, end).
    content_spans: Vec<(u32, usize, usize)>,
    /// Split невозможен в этом кадре (override на корневом SC, SC-потомок
    /// внутри inline-спана и т.п.).
    invalid: bool,
    /// Счётчик входов в SC-ветку — детектор «SC-потомок сбежал из спана
    /// в собственный бакет» (его команды не были бы покрыты диапазоном).
    sc_entries: u32,
}

impl SplitTracker {
    fn disabled() -> Self {
        Self {
            enabled: false,
            animated_scs: Vec::new(),
            content_spans: Vec::new(),
            invalid: false,
            sc_entries: 0,
        }
    }
}

/// Общее тело ordered-сборки с anim-override-ами. При `track_split` собирает
/// диапазоны анимируемых сегментов (см. [`build_display_list_ordered_with_anim_split`]);
/// иначе возвращает пустой Vec диапазонов и ведёт себя байт-в-байт как раньше.
fn ordered_with_anim_internal(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
    anim: Option<&CompositorAnimFrame>,
    dpr: f32,
    track_split: bool,
) -> (DisplayList, Vec<std::ops::Range<usize>>) {
    let n_sc = tree.contexts.len().max(1);
    let mut buckets: Vec<ScBucket> = vec![ScBucket::default(); n_sc];
    let mut next_sc_id: u32 = 1;
    let mut split = SplitTracker::disabled();
    split.enabled = track_split && anim.is_some_and(|a| !a.is_empty());
    // Compositor-animation path does not consume provenance (only the
    // introspection-facing `build_display_list_ordered*` does) — discard.
    fill_buckets(root, StackingContextId::ROOT, &mut next_sc_id, &mut buckets, true, anim, dpr, &[], &mut split, &mut Vec::new());

    let animated_scs: std::collections::HashSet<u32> =
        split.animated_scs.iter().copied().collect();
    let mut spans_by_sc: std::collections::HashMap<u32, Vec<(usize, usize)>> =
        std::collections::HashMap::new();
    for &(sc, s, e) in &split.content_spans {
        spans_by_sc.entry(sc).or_default().push((s, e));
    }

    let mut out = Vec::new();
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    // Открытый диапазон анимируемого SC: (sc_id, старт в out). Вложенные
    // анимируемые SC/спаны внутри открытого диапазона уже покрыты им.
    let mut open: Option<(u32, usize)> = None;
    for (sc_id, phase) in &order.steps {
        let idx = sc_id.0 as usize;
        if idx >= buckets.len() {
            continue;
        }
        let bucket = &mut buckets[idx];
        match phase {
            PaintPhase::RootBackground => {
                if open.is_none() && animated_scs.contains(&sc_id.0) {
                    open = Some((sc_id.0, out.len()));
                }
                out.append(&mut bucket.pre);
                out.append(&mut bucket.root_bg);
            }
            PaintPhase::InlineContent => {
                let base = out.len();
                out.append(&mut bucket.contents);
                // post (PopTransform / PopOpacity / etc.) is now in CloseLayer —
                // emitted AFTER all child SCs so nested transforms compose correctly
                // (BUG-139). Do NOT move post back here.
                if open.is_none()
                    && let Some(spans) = spans_by_sc.get(&sc_id.0)
                {
                    for &(s, e) in spans {
                        if e > s {
                            ranges.push(base + s..base + e);
                        }
                    }
                }
            }
            // CloseLayer is emitted last in paint_sc, after all child SCs, so the
            // parent's Pop-commands wrap the children's Push-commands correctly.
            PaintPhase::CloseLayer => {
                out.append(&mut bucket.post);
                if let Some((id, start)) = open
                    && id == sc_id.0
                {
                    if out.len() > start {
                        ranges.push(start..out.len());
                    }
                    open = None;
                }
            }
            _ => {}
        }
    }
    // Незакрытый диапазон (SC без CloseLayer-шага) — split невалиден.
    if split.invalid || open.is_some() {
        return (out, Vec::new());
    }
    // Сортировка + выбрасывание вложенных спанов (спан текстового потомка
    // внутри спана его элемента и т.п.). Частичное пересечение диапазонов
    // невозможно по построению; если встретилось — split невалиден.
    ranges.sort_by_key(|r| (r.start, std::cmp::Reverse(r.end)));
    let mut dedup: Vec<std::ops::Range<usize>> = Vec::with_capacity(ranges.len());
    for r in ranges {
        match dedup.last() {
            Some(last) if r.start < last.end => {
                if r.end <= last.end {
                    continue; // вложенный — покрыт внешним
                }
                return (out, Vec::new()); // частичное пересечение — не бывает
            }
            _ => dedup.push(r),
        }
    }
    (out, dedup)
}

/// Builds a print display list from paginated layout.
///
/// Each page's fragments are translated to page-relative coordinates using
/// `PushTransform` / `PopTransform`. Pages are separated by `PageBreak` markers.
/// Use `split_at_page_breaks` to get per-page command slices for rendering.
///
/// If a page has `page_box` set, margin-box text fragments (@page headers, footers,
/// page numbers) are emitted as `DrawText` commands positioned at absolute page
/// coordinates (not inside the content-area transform).
///
/// Coordinate convention: page origin = (0, 0) at top-left of content area.
/// Fragment y-offset is relative to the content area, not the page box.
/// Margin-box positions are relative to the page box origin (top-left of full page).
pub fn build_print_display_list(pages: &[Page]) -> DisplayList {
    let mut cmds: DisplayList = Vec::new();
    for (page_idx, page) in pages.iter().enumerate() {
        if page_idx > 0 {
            cmds.push(DisplayCommand::PageBreak);
        }
        for frag in &page.fragments {
            // Translate from document-flow y to page-local y.
            let dy = frag.page_y_offset - frag.layout_box.rect.y;
            let matrix = Mat4::translation_2d(0.0, dy);
            cmds.push(DisplayCommand::PushTransform { matrix });
            walk(&frag.layout_box, &mut cmds, 1.0, None);
            cmds.push(DisplayCommand::PopTransform);
        }
        // Emit margin-box text content (headers, footers, page numbers).
        if let Some(page_box) = &page.page_box {
            for margin_box in page_box.margin_boxes.values() {
                emit_margin_box_text(margin_box, &mut cmds);
            }
        }
    }
    cmds
}

/// Emits `DrawText` commands for each text fragment in a margin-box.
///
/// Positions are absolute page coordinates: `margin_box.x + fragment.x` and
/// `margin_box.y + fragment.y`. Text uses the page default: 10px black,
/// no explicit font family (renderer falls back to bundled Inter).
fn emit_margin_box_text(margin_box: &MarginBox, cmds: &mut DisplayList) {
    let default_font_size = 10.0_f32;
    let text_color = Color { r: 0, g: 0, b: 0, a: 255 };
    for frag in &margin_box.text_fragments {
        if frag.text.is_empty() {
            continue;
        }
        let rect = Rect {
            x: margin_box.x + frag.x,
            y: margin_box.y + frag.y,
            width: frag.width,
            height: frag.height,
        };
        cmds.push(DisplayCommand::DrawText {
            font_stretch: FontStretch::NORMAL,
            rect,
            text: frag.text.clone(),
            font_size: default_font_size,
            color: text_color,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: None,
        });
    }
}

/// Splits a print display list at `PageBreak` markers.
///
/// Returns one `Vec<DisplayCommand>` per page. The `PageBreak` commands are
/// consumed (not included in any page's slice). An empty input yields an empty
/// outer `Vec`. A list with no `PageBreak` yields a single-element outer `Vec`.
pub fn split_at_page_breaks(cmds: Vec<DisplayCommand>) -> Vec<Vec<DisplayCommand>> {
    let mut pages: Vec<Vec<DisplayCommand>> = Vec::new();
    let mut current: Vec<DisplayCommand> = Vec::new();
    for cmd in cmds {
        if matches!(cmd, DisplayCommand::PageBreak) {
            pages.push(current);
            current = Vec::new();
        } else {
            current.push(cmd);
        }
    }
    pages.push(current);
    pages
}

/// Removes background-graphics paint commands from each print page when the
/// user disabled "Background graphics" in the print dialog (CC-8).
///
/// Mirrors Chrome's "Background graphics" print toggle: when `print_backgrounds`
/// is `false`, the CSS-background paint family is stripped — solid background
/// fills (`FillRect`, `FillRoundedRect`), `background-image`s, and the three
/// gradient kinds (linear/radial/conic). Foreground content — text, borders,
/// outlines, `<img>` raster images, and SVG paths — is preserved.
///
/// No-op when `print_backgrounds` is `true`. Operates in place, page by page;
/// `Push*`/`Pop*` nesting stays balanced because only leaf paint commands are
/// removed.
pub fn strip_background_graphics(pages: &mut [Vec<DisplayCommand>], print_backgrounds: bool) {
    if print_backgrounds {
        return;
    }
    for page in pages.iter_mut() {
        page.retain(|cmd| !is_background_graphic(cmd));
    }
}

/// Classifies a [`DisplayCommand`] as a CSS background-graphics paint op —
/// the set removed when "Background graphics" is off (see
/// [`strip_background_graphics`]).
fn is_background_graphic(cmd: &DisplayCommand) -> bool {
    matches!(
        cmd,
        DisplayCommand::FillRect { .. }
            | DisplayCommand::FillRoundedRect { .. }
            | DisplayCommand::DrawBackgroundImage { .. }
            | DisplayCommand::DrawLinearGradient { .. }
            | DisplayCommand::DrawRadialGradient { .. }
            | DisplayCommand::DrawConicGradient { .. }
    )
}

#[derive(Default, Clone)]
struct ScBucket {
    /// PushOpacity / PushBlendMode / PushClipRect — открывают layer-effects
    /// SC-owner-а перед собственным фоном.
    pre: Vec<DisplayCommand>,
    /// CSS 2.1 Appendix E phase 1 — bg/border SC-owner box-а.
    root_bg: Vec<DisplayCommand>,
    /// Фазы 3/4/5 — descendants SC-owner-а кроме child-SC-creating box-ов.
    contents: Vec<DisplayCommand>,
    /// Pop* в обратном порядке к `pre`. Эмитится после `contents` в фазе
    /// `InlineContent`. См. Phase 0 ограничение в docstring
    /// `build_display_list_ordered`.
    post: Vec<DisplayCommand>,
}

/// Which [`ScBucket`] field a [`RawSpan`] was recorded against. `fill_buckets`
/// only ever appends to one field at a time per call, so this plus the SC id
/// is enough to find the field again when `build_display_list_ordered_dpr`
/// flushes buckets into the final command list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BucketField {
    Pre,
    RootBg,
    Contents,
    Post,
}

/// A [`ProvenanceSpan`] before translation to global command-list indices.
/// `range` is local to one `ScBucket` field (`fill_buckets` only ever sees
/// that field's own, not-yet-flushed `Vec`); `build_display_list_ordered_dpr`
/// offsets it by that field's position in the final list once flushed.
struct RawSpan {
    sc: u32,
    field: BucketField,
    range: Range<usize>,
    origin: BoxOrigin,
    fragment: u32,
}

/// Records `[start, end)` of `field` as one `RawSpan` for `origin`, unless
/// empty (a box that emitted nothing this call — e.g. `display:none` subtree,
/// zero-size overflow clip — gets no span rather than a degenerate one).
fn record_span(
    spans: &mut Vec<RawSpan>,
    sc: u32,
    field: BucketField,
    start: usize,
    end: usize,
    origin: BoxOrigin,
    fragment: u32,
) {
    if end > start {
        spans.push(RawSpan { sc, field, range: start..end, origin, fragment });
    }
}

/// CSS Compositing & Blending L1 §5: маппинг style-уровневого `MixBlendMode`
/// (lumen-layout) в paint-уровневый `BlendMode` (lumen-paint). Enum-ы
/// разные, чтобы не тянуть зависимость paint → layout в обратную сторону;
/// варианты совпадают 1:1.
fn map_blend_mode(m: LayoutBlendMode) -> BlendMode {
    match m {
        LayoutBlendMode::Normal => BlendMode::Normal,
        LayoutBlendMode::Multiply => BlendMode::Multiply,
        LayoutBlendMode::Screen => BlendMode::Screen,
        LayoutBlendMode::Overlay => BlendMode::Overlay,
        LayoutBlendMode::Darken => BlendMode::Darken,
        LayoutBlendMode::Lighten => BlendMode::Lighten,
        LayoutBlendMode::ColorDodge => BlendMode::ColorDodge,
        LayoutBlendMode::ColorBurn => BlendMode::ColorBurn,
        LayoutBlendMode::HardLight => BlendMode::HardLight,
        LayoutBlendMode::SoftLight => BlendMode::SoftLight,
        LayoutBlendMode::Difference => BlendMode::Difference,
        LayoutBlendMode::Exclusion => BlendMode::Exclusion,
        LayoutBlendMode::Hue => BlendMode::Hue,
        LayoutBlendMode::Saturation => BlendMode::Saturation,
        LayoutBlendMode::Color => BlendMode::Color,
        LayoutBlendMode::Luminosity => BlendMode::Luminosity,
        LayoutBlendMode::PlusLighter => BlendMode::PlusLighter,
    }
}

/// CSS Overflow L3 §3.2: значения, при которых overflow создаёт clip-bound
/// для содержимого. `Visible` не клипает.
fn overflow_clips(o: Overflow) -> bool {
    matches!(
        o,
        Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
    )
}

/// Em-fraction for approximating U+2026 HORIZONTAL ELLIPSIS advance width.
/// Empirically derived from Inter Regular; the outer overflow:hidden clip
/// prevents pixel bleed if the renderer's actual advance differs slightly.
const ELLIPSIS_EM: f32 = 0.65;

/// Центр basic-shape в page-координатах: `at cx cy` (cx — % от ширины,
/// cy — % от высоты border-box) либо дефолт 50% 50% (CSS Shapes L1 §5.1).
fn resolve_shape_center(center: Option<(ShapeValue, ShapeValue)>, r: Rect) -> (f32, f32) {
    center
        .map(|(x, y)| (r.x + x.resolve(r.width), r.y + y.resolve(r.height)))
        .unwrap_or((r.x + r.width * 0.5, r.y + r.height * 0.5))
}

/// CSS Masking L1 §9 — bounding-box rect for a `clip-path` shape relative to
/// the element's border-box `r`. Для `inset(...)` это точное представление;
/// для circle/ellipse/polygon — bounding box (используется fallback-путями;
/// точная форма идёт через `clip_path_to_shape` → `PushClipPath`, BUG-140).
fn clip_path_to_rect(clip: &ClipPath, r: Rect) -> Rect {
    match clip_path_to_shape(clip, r) {
        Some(shape) => shape.bounding_rect(),
        None => {
            let ClipPath::Inset(sides) = clip else { return r };
            let rs = |v: &ShapeValue, basis: f32| v.resolve(basis);
            let (top, right, bottom, left) = match sides.as_slice() {
                [a] => (rs(a, r.height), rs(a, r.width), rs(a, r.height), rs(a, r.width)),
                [tb, rl] => (rs(tb, r.height), rs(rl, r.width), rs(tb, r.height), rs(rl, r.width)),
                [t, rl, b] => (rs(t, r.height), rs(rl, r.width), rs(b, r.height), rs(rl, r.width)),
                [t, ri, b, l] => (rs(t, r.height), rs(ri, r.width), rs(b, r.height), rs(l, r.width)),
                _ => (0.0, 0.0, 0.0, 0.0),
            };
            Rect::new(
                r.x + left,
                r.y + top,
                (r.width - left - right).max(0.0),
                (r.height - top - bottom).max(0.0),
            )
        }
    }
}

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

/// Returns the Unicode string for a CSS `text-emphasis-style` symbol.
/// Returns empty string for `None`.
fn emphasis_mark_str(style: &TextEmphasisStyle) -> &str {
    match style {
        TextEmphasisStyle::None => "",
        TextEmphasisStyle::String(s) => s.as_str(),
        TextEmphasisStyle::Symbol { filled, shape } => match (filled, shape) {
            (true,  TextEmphasisShape::Dot)          => "\u{2022}", // •
            (false, TextEmphasisShape::Dot)          => "\u{25E6}", // ◦
            (true,  TextEmphasisShape::Circle)       => "\u{25CF}", // ●
            (false, TextEmphasisShape::Circle)       => "\u{25CB}", // ○
            (true,  TextEmphasisShape::DoubleCircle) => "\u{25C9}", // ◉
            (false, TextEmphasisShape::DoubleCircle) => "\u{25CE}", // ◎
            (true,  TextEmphasisShape::Triangle)     => "\u{25B2}", // ▲
            (false, TextEmphasisShape::Triangle)     => "\u{25B3}", // △
            (true,  TextEmphasisShape::Sesame)       => "\u{FE45}", // ﹅
            (false, TextEmphasisShape::Sesame)       => "\u{FE46}", // ﹆
        },
    }
}

/// CSS Text Decoration L4 §5 — emits per-character emphasis marks above or
/// below each grapheme cluster of `frag.text`.
///
/// Phase 0: distributes marks uniformly over the fragment width (no per-glyph
/// advance measurement). Accurate spacing requires a measurer at paint time
/// (deferred to Phase 1).
fn emit_text_emphasis_marks(
    out: &mut Vec<DisplayCommand>,
    container_x: f32,
    line_h: f32,
    frag_y: f32,
    frag: &InlineFrag,
) {
    let mark = emphasis_mark_str(&frag.style.text_emphasis_style);
    if mark.is_empty() {
        return;
    }
    let char_count = frag.text.chars().count();
    if char_count == 0 {
        return;
    }
    let mark_size = frag.style.font_size * 0.5;
    let is_over = frag.style.text_emphasis_position.is_over();
    let mark_y = if is_over {
        frag_y - mark_size * 1.2
    } else {
        frag_y + line_h
    };
    let color = frag.style.text_emphasis_color.resolve(frag.style.color);
    let char_w = frag.width / char_count as f32;
    let frag_x = container_x + frag.x;
    for i in 0..char_count {
        out.push(DisplayCommand::DrawText {
            font_stretch: frag.style.font_stretch,
            rect: Rect::new(frag_x + i as f32 * char_w, mark_y, char_w, mark_size * 1.5),
            text: mark.to_string(),
            font_size: mark_size,
            color,
            font_family: frag.style.font_family.clone(),
            font_weight: frag.style.font_weight,
            font_style: frag.style.font_style,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: None,
        });
    }
}

/// Emits shadow + DrawText + decorations for every visible frag in `line`.
///
/// When `sel` is `Some`, fragments that overlap the active selection range
/// receive a `FillRect` highlight background before the text, and optionally
/// have their text colour overridden by `sel.fg_color` (CSS Pseudo-elements
/// L4 §5.6 `::selection`).
///
/// Phase 0 limitation: selection pixel bounds are estimated proportionally
/// by byte offset, which is accurate for ASCII but approximate for non-ASCII.
/// Per-glyph accuracy requires a `TextMeasurer` which is not available here.
fn emit_text_frags(
    line: &[InlineFrag],
    container_x: f32,
    container_width: f32,
    line_y: f32,
    line_h: f32,
    sel: Option<&SelectionHighlight>,
    out: &mut Vec<DisplayCommand>,
) {
    for frag in line {
        if !matches!(frag.style.visibility, Visibility::Visible) {
            continue;
        }
        let frag_y = line_y + frag.y_offset;
        // Inline-replaced image: emit LazyImageSlot or DrawImage, skip text rendering.
        if let Some(src) = &frag.img_src {
            let img_rect = Rect::new(container_x + frag.x, frag_y, frag.width, line_h);
            if frag.img_is_lazy {
                // node_id unavailable in InlineFrag (no box reference); use 0 as sentinel.
                // The shell's proximity check uses the display list rects, not node_id here.
                out.push(DisplayCommand::LazyImageSlot {
                    rect: img_rect,
                    node_id: 0,
                    src: src.clone(),
                    object_fit: frag.style.object_fit,
                    object_position: frag.style.object_position,
                });
            } else {
                out.push(DisplayCommand::DrawImage {
                    rect: img_rect,
                    src: src.clone(),
                    alt: frag.text.clone(),
                    object_fit: frag.style.object_fit,
                    object_position: frag.style.object_position,
                    image_rendering: frag.style.image_rendering,
                });
            }
            continue;
        }

        // ::selection highlight — emit FillRect for selected portion before text.
        let sel_fg = sel.and_then(|s| {
            let hi = frag_selection_highlight(frag, s);
            if let Some((sel_x, sel_w)) = hi {
                out.push(DisplayCommand::FillRect {
                    rect: Rect::new(container_x + sel_x, line_y, sel_w, line_h),
                    color: s.bg_color,
                });
            }
            if hi.is_some() { s.fg_color } else { None }
        });

        let text_color = sel_fg.unwrap_or(frag.style.color);
        let base_rect = Rect::new(container_x + frag.x, frag_y, container_width, line_h);
        emit_text_shadows(out, base_rect, line_h, frag);
        out.push(DisplayCommand::DrawText {
            font_stretch: frag.style.font_stretch,
            rect: base_rect,
            // UAX #9 L2/L4 — a right-to-left fragment is handed to the
            // rasterizers already reversed and mirrored: they advance strictly
            // left to right and do no bidi work of their own. `frag.text` stays
            // logical, so Selection/Range offsets are unaffected.
            text: lumen_layout::bidi::visual_text(&frag.text, frag.bidi_level).into_owned(),
            font_size: frag.style.font_size,
            color: text_color,
            font_family: frag.style.font_family.clone(),
            font_weight: frag.style.font_weight,
            font_style: frag.style.font_style,
            font_features: lumen_layout::style::text_font_features(&frag.style),
            font_palette: palette_selection(&frag.style),
            font_variation_axes: {
                let mut axes: Vec<([u8; 4], f32)> = frag.style.font_variation_settings
                    .iter().map(|a| (a.tag, a.value)).collect();
                if frag.style.font_optical_sizing == FontOpticalSizing::Auto
                    && !axes.iter().any(|(t, _)| t == b"opsz")
                {
                    axes.push((*b"opsz", frag.style.font_size));
                }
                if frag.style.font_stretch != FontStretch::NORMAL
                    && !axes.iter().any(|(t, _)| t == b"wdth")
                {
                    axes.push((*b"wdth", frag.style.font_stretch.0 as f32 / 10.0));
                }
                axes
            },
            tab_size: frag.style.tab_size,
            highlight_name: None,
            text_orientation: if frag.style.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
                Some(frag.style.text_orientation)
            } else {
                None
            },
        });
        push_text_decoration(out, container_x, frag_y, frag);
        emit_text_emphasis_marks(out, container_x, line_h, frag_y, frag);
    }
}

/// Compute the (frag-relative x, width) pixel span that is covered by the
/// active selection for a single inline fragment.
///
/// Returns `None` when the fragment is outside the selection range.
///
/// Uses byte-proportional estimation for sub-fragment boundaries.  Accurate
/// for ASCII text; approximate for variable-width or multi-byte characters.
fn frag_selection_highlight(frag: &InlineFrag, sel: &SelectionHighlight) -> Option<(f32, f32)> {
    let range = &sel.range;
    if range.is_collapsed() {
        return None;
    }
    let frag_end = frag.source_char_offset + frag.text.len() as u32;
    let same_start = range.start.container == frag.source_node;
    let same_end = range.end.container == frag.source_node;

    // byte offsets within the frag's text
    let (byte_start, byte_end): (u32, u32) = if same_start && same_end {
        let s = range.start.offset.max(frag.source_char_offset).min(frag_end)
            - frag.source_char_offset;
        let e = range.end.offset.max(frag.source_char_offset).min(frag_end)
            - frag.source_char_offset;
        if e <= s { return None; }
        (s, e)
    } else if same_start {
        let s = range.start.offset.max(frag.source_char_offset).min(frag_end)
            - frag.source_char_offset;
        (s, frag.text.len() as u32)
    } else if same_end {
        let e = range.end.offset.max(frag.source_char_offset).min(frag_end)
            - frag.source_char_offset;
        if e == 0 { return None; }
        (0, e)
    } else {
        // Frag node is between range endpoints: fully selected, but multi-node
        // selection depth is not tracked in Phase 0 without tree traversal.
        return None;
    };

    let total = frag.text.len() as f32;
    if total <= 0.0 {
        return None;
    }
    let x_start = frag.x + frag.width * (byte_start as f32 / total);
    let x_end   = frag.x + frag.width * (byte_end   as f32 / total);
    Some((x_start, (x_end - x_start).max(0.0)))
}

/// CSS Writing Modes L4 §3–4 — paints an InlineRun's lines when the box is in
/// a vertical writing mode (`vertical-rl`/`vertical-lr`/`sideways-*`).
///
/// `wrap_inline_run_vertical` (`lumen-layout`) repurposes `InlineFrag.x` as the
/// cumulative offset along the inline axis (physical Y for vertical writing
/// modes, top→bottom) and `InlineFrag.width` as the frag's own extent along
/// that axis — the same two fields the horizontal path in [`emit_inline_run`]
/// reads as a physical X offset and physical width. Before this function
/// existed, `emit_inline_run` ran unconditionally and misread those fields as
/// horizontal geometry: every frag in a line landed at the same physical Y
/// (`line_y`, correct only for a horizontal row) with X spread out by what is
/// actually the vertical cursor, and each word's DrawText got the column's
/// *width* (`b.rect.width`, one column wide) as its rect height — silently
/// breaking every real vertical `<div>` (caught by `graphic_tests/145-writing-mode.html`,
/// Ph3 writing-mode vertical Срез 5; the layout side — `vertical.rs` — was
/// already correct, only this paint conversion was never ported to the axis
/// swap).
///
/// Each outer `lines` entry is one wrapped column along the block axis;
/// column 0 sits at `b.rect.x` (already correctly placed by
/// `lay_out_vertical_inline_run`/the vertical block-stacking cursor in
/// `vertical.rs`), so wrapped column N shifts by `N * col_width` — leftward
/// for `vertical-rl`/`sideways-rl` (later columns sit further from the
/// right-edge start), rightward for `vertical-lr`/`sideways-lr`.
///
/// Not yet ported to this axis (Phase 0, same class of gap the horizontal
/// path documents elsewhere): `vertical-align` (`frag.y_offset`), inline
/// replaced content (images), `::selection` highlight, and
/// `text-overflow: ellipsis`.
fn emit_inline_run_vertical(b: &LayoutBox, lines: &[Vec<InlineFrag>], out: &mut Vec<DisplayCommand>) {
    let col_width = b.rect.width;
    let is_rtl = matches!(
        b.style.writing_mode,
        lumen_layout::style::WritingMode::VerticalRl | lumen_layout::style::WritingMode::SidewaysRl
    );
    for (line_idx, line) in lines.iter().enumerate() {
        let column_x = if is_rtl {
            b.rect.x - line_idx as f32 * col_width
        } else {
            b.rect.x + line_idx as f32 * col_width
        };
        for frag in line {
            if !matches!(frag.style.visibility, Visibility::Visible) || frag.img_src.is_some() {
                continue;
            }
            let rect = Rect::new(column_x, b.rect.y + frag.x, col_width, frag.width);
            emit_text_shadows(out, rect, rect.height, frag);
            out.push(DisplayCommand::DrawText {
                font_stretch: frag.style.font_stretch,
                rect,
                text: frag.text.clone(),
                font_size: frag.style.font_size,
                color: frag.style.color,
                font_family: frag.style.font_family.clone(),
                font_weight: frag.style.font_weight,
                font_style: frag.style.font_style,
                font_features: lumen_layout::style::text_font_features(&frag.style),
                font_palette: palette_selection(&frag.style),
                font_variation_axes: {
                    let mut axes: Vec<([u8; 4], f32)> = frag.style.font_variation_settings
                        .iter().map(|a| (a.tag, a.value)).collect();
                    if frag.style.font_optical_sizing == FontOpticalSizing::Auto
                        && !axes.iter().any(|(t, _)| t == b"opsz")
                    {
                        axes.push((*b"opsz", frag.style.font_size));
                    }
                    if frag.style.font_stretch != FontStretch::NORMAL
                        && !axes.iter().any(|(t, _)| t == b"wdth")
                    {
                        axes.push((*b"wdth", frag.style.font_stretch.0 as f32 / 10.0));
                    }
                    axes
                },
                tab_size: frag.style.tab_size,
                highlight_name: None,
                text_orientation: Some(frag.style.text_orientation),
            });
        }
    }
}

/// Renders all lines of a [`BoxKind::InlineRun`].
///
/// When `text-overflow: ellipsis` (CSS UI L4 §3) is active on the box style
/// AND a line's text extends past `b.rect.width`, the line is rendered with:
/// 1. A [`DisplayCommand::PushClipRect`] narrowed by the ellipsis glyph width.
/// 2. Normal text emission inside the clip.
/// 3. [`DisplayCommand::PopClip`].
/// 4. A [`DisplayCommand::DrawText`] "…" at the clip boundary.
///
/// Requires `overflow_x != visible` on the box (CSS UI L4 §3 precondition).
/// The parent block's overflow:hidden clip ensures no pixel escapes the container.
fn emit_inline_run(
    b: &LayoutBox,
    lines: &[Vec<InlineFrag>],
    sel: Option<&SelectionHighlight>,
    dpr: f32,
    out: &mut Vec<DisplayCommand>,
) {
    if b.style.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
        emit_inline_run_vertical(b, lines, out);
        return;
    }
    // CSS Rhythmic Sizing L1 §2 — line-height-step rounds each line box up to a
    // multiple of the step so paint stacks lines at the same rhythm layout used.
    let raw_line_h = b.style.font_size * b.style.line_height;
    let line_h = if b.style.line_height_step > 0.0 {
        (raw_line_h / b.style.line_height_step).ceil() * b.style.line_height_step
    } else {
        raw_line_h
    };
    let wants_ellipsis = matches!(b.style.text_overflow, TextOverflow::Ellipsis)
        && overflow_clips(b.style.overflow_x);

    emit_first_line_background(b, lines, line_h, dpr, out);

    for (line_idx, line) in lines.iter().enumerate() {
        let line_y = b.rect.y + line_idx as f32 * line_h;

        // Phase 1: inline frag backgrounds (under text).
        for frag in line.iter() {
            if !matches!(frag.style.visibility, Visibility::Visible) {
                continue;
            }
            emit_inline_frag_box(out, b.rect.x, line_y + frag.y_offset, line_h, frag);
        }

        // Detect text-overflow: find first visible frag that extends past container.
        let overflow_frag = if wants_ellipsis {
            line.iter().find(|f| {
                matches!(f.style.visibility, Visibility::Visible)
                    && f.x + f.width > b.rect.width
            })
        } else {
            None
        };

        // Phase 2: text — with or without ellipsis clip.
        if let Some(ef) = overflow_frag {
            let ew = ef.style.font_size * ELLIPSIS_EM;
            let clip_w = (b.rect.width - ew).max(0.0);
            out.push(DisplayCommand::PushClipRect {
                rect: Rect::new(b.rect.x, line_y, clip_w, line_h),
            });
            emit_text_frags(line, b.rect.x, b.rect.width, line_y, line_h, sel, out);
            out.push(DisplayCommand::PopClip);
            out.push(DisplayCommand::DrawText {
                font_stretch: ef.style.font_stretch,
                rect: Rect::new(b.rect.x + clip_w, line_y, ew, line_h),
                text: "\u{2026}".to_string(),
                font_size: ef.style.font_size,
                color: ef.style.color,
                font_family: ef.style.font_family.clone(),
                font_weight: ef.style.font_weight,
                font_style: ef.style.font_style,
                font_features: lumen_layout::style::text_font_features(&ef.style),
                font_palette: palette_selection(&ef.style),
                font_variation_axes: {
                    let mut axes: Vec<([u8; 4], f32)> = ef.style.font_variation_settings
                        .iter().map(|a| (a.tag, a.value)).collect();
                    if ef.style.font_optical_sizing == FontOpticalSizing::Auto
                        && !axes.iter().any(|(t, _)| t == b"opsz")
                    {
                        axes.push((*b"opsz", ef.style.font_size));
                    }
                    if ef.style.font_stretch != FontStretch::NORMAL
                        && !axes.iter().any(|(t, _)| t == b"wdth")
                    {
                        axes.push((*b"wdth", ef.style.font_stretch.0 as f32 / 10.0));
                    }
                    axes
                },
                tab_size: 0.0,
                highlight_name: None,
                text_orientation: if ef.style.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
                    Some(ef.style.text_orientation)
                } else {
                    None
                },
            });
        } else {
            emit_text_frags(line, b.rect.x, b.rect.width, line_y, line_h, sel, out);
        }
    }
}

/// BUG-432 — background of the `::first-line` pseudo-element.
///
/// CSS Pseudo-elements L4 §4.1 lists all background properties among those that
/// apply to `::first-line`, and `split_first_line_boxes` already puts the whole
/// pseudo-element `ComputedStyle` on the box holding the first formatted line —
/// but `emit_inline_run` only ever drew text, so the background was dropped.
///
/// §4.1 also makes the pseudo-element behave as a fictional inline tag wrapping
/// the line's content, so the painted extent is the union of the line's
/// fragments, **not** the full width of the containing block (`b.rect.width`,
/// which is what the box itself carries). Box-model properties — margin,
/// padding, border — do not apply to `::first-line` and are not painted here.
///
/// Keyed on the box role rather than on the style: every other `InlineRun` is
/// built through `anon_style`, which clears `background_color`, so this is a
/// guard against a future anonymous run that inherits one, not a filter that
/// currently discriminates.
fn emit_first_line_background(
    b: &LayoutBox,
    lines: &[Vec<InlineFrag>],
    line_h: f32,
    dpr: f32,
    out: &mut Vec<DisplayCommand>,
) {
    if !matches!(b.origin.role, BoxRole::Pseudo(PseudoKind::FirstLine)) {
        return;
    }
    if !matches!(b.style.visibility, Visibility::Visible) {
        return;
    }
    let has_color = b.style.background_color.and_then(|c| c.to_color_opt()).is_some_and(|c| c.a > 0);
    if !has_color && b.style.background_layers.is_empty() {
        return;
    }
    // The pseudo-element covers the first formatted line only; the box produced
    // by the split holds it as `lines[0]`.
    let Some(line) = lines.first() else { return };
    let Some(rect) = first_line_content_rect(b, line, line_h) else { return };

    let radii = CornerRadii::from_style_and_box(&b.style, rect.width, rect.height);
    if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
        && bg.a > 0
    {
        if radii.all_zero() {
            out.push(DisplayCommand::FillRect { rect, color: bg });
        } else {
            out.push(DisplayCommand::FillRoundedRect { rect, color: bg, radii });
        }
    }
    if !b.style.background_layers.is_empty() {
        // `emit_background_image` derives every clip/origin box from `b.rect`,
        // which here spans the whole containing block — hand it a copy narrowed
        // to the line's own extent so a gradient covers the same area the solid
        // colour does. Cheap: the copy is one line of fragments, no children.
        let mut fl = b.clone();
        fl.rect = rect;
        emit_background_image(out, &fl, dpr);
    }
}

/// Union of a line's visible fragments, as a painting rect for
/// [`emit_first_line_background`]. `None` when the line contributes no extent.
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

/// Layer-ops одного бокса, разделённые на эффекты и overflow-клип.
///
/// Per CSS Overflow L3 §3.2 overflow-клип обрезает только **детей** до
/// padding-box; собственные background/border бокса не клиппятся (BUG-123 —
/// рамка scroll-контейнера целиком срезалась scissor-ом своего же
/// PushScrollLayer). Caller эмитит `pre` → bg/border → `overflow_pre` →
/// дети → `overflow_post` → `post` — зеркало порядка не-композиторного
/// `walk` (bg/border до PushScrollLayer).
struct BoxLayerOps {
    /// Эффекты, оборачивающие весь painted output бокса (включая его
    /// собственные background/border): clip-path, blend, opacity,
    /// transform, backdrop-filter, filter.
    pre: Vec<DisplayCommand>,
    /// Парные Pop к `pre`, уже в обратном (LIFO) порядке.
    post: Vec<DisplayCommand>,
    /// Overflow-клип / scroll-слой — оборачивает только детей.
    overflow_pre: Vec<DisplayCommand>,
    /// Парные Pop к `overflow_pre`.
    overflow_post: Vec<DisplayCommand>,
}

/// Собирает layer-effect триггеры одного box-а в [`BoxLayerOps`].
/// Push-команды складываются в `pre` в порядке, парные `Pop` в `post` —
/// в обратном порядке (LIFO). Возвращает пустые векторы для боксов без
/// триггеров **или для анонимных боксов** (InlineRun / Skip), у которых
/// нет своего DOM-элемента, к которому компилятор стиля привязал бы
/// triggering свойство.
///
/// Симметрия с `box_can_own_stacking_context` / `box_can_own_property_node`:
/// анонимные InlineRun-ы клонируют style родителя (включая opacity и
/// overflow), и эмиссия layer-ops для них дала бы фантомные парные
/// Push/Pop поверх настоящих от parent-Block-а. Та же защита здесь.
///
/// Триггеры:
/// - `opacity < 1.0` → `PushOpacity { alpha } / PopOpacity`.
/// - `mix-blend-mode != Normal` → `PushBlendMode { mode } / PopBlendMode`.
/// - `overflow-x / overflow-y` ∈ {hidden, clip, scroll, auto} →
///   `PushClipRect { rect: b.rect } / PopClip`.
/// - `transform != []` → `PushTransform { matrix } / PopTransform`.
///   Matrix считается через `forward_box_transform`: T(pivot)·M·T(-pivot)
///   в viewport-координатах, pivot = b.rect.origin + transform_origin.
///
/// Порядок Push-команд (для child compositor-а смысла не несёт, но
/// детерминирован для тестируемости): Blend → Opacity → Transform →
/// ClipPath → BackdropFilter → Filter. Pop — в обратном порядке. Transform
/// пушится до clip-path: клип задан в локальной системе элемента и
/// переносится его transform-ом (CSS Masking L1 §9, BUG-140), при этом
/// transform преобразует всё содержимое SC (включая собственные
/// background/border бокса, эмитимые в `root_bg`).
fn box_layer_ops(b: &LayoutBox, ov: Option<&CompositorOverride>) -> BoxLayerOps {
    let mut pre = Vec::new();
    let mut post = Vec::new();
    let mut overflow_pre = Vec::new();
    let mut overflow_post = Vec::new();
    if !box_can_own_stacking_context(b) {
        // SVG §7.4: the outermost SVG viewport establishes a clip (UA default
        // `overflow: hidden`). With object-fit: cover (or a viewBox larger than
        // the viewport) the scaled content overflows the SVG box; without this
        // clip it would paint over sibling boxes. SvgRoot is not a stacking-
        // context owner, so emit the viewport clip here. BUG-110.
        if matches!(b.kind, BoxKind::SvgRoot { .. }) {
            let s = &b.style;
            let px = b.rect.x + s.border_left_width;
            let py = b.rect.y + s.border_top_width;
            let pw = (b.rect.width - s.border_left_width - s.border_right_width).max(0.0);
            let ph = (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0);
            overflow_pre.push(DisplayCommand::PushClipRect { rect: Rect::new(px, py, pw, ph) });
            overflow_post.push(DisplayCommand::PopClip);
        }
        return BoxLayerOps { pre, post, overflow_pre, overflow_post };
    }
    let s = &b.style;

    // CSS Masking L1 §4 (BUG-183): mask-image wraps the fully composited element
    // (background + border + content + children), so it must be the OUTERMOST
    // layer. Pushed first into `pre` and `post` here — after `post.reverse()` its
    // PopMask becomes the last command, balancing the PushMask. `walk` emits the
    // same pair inline via `emit_push_mask`; the SC bucket path lost it before
    // (mask-image makes the box a stacking context → painted via `fill_buckets`/
    // `emit_box_self`, which never opened the mask group).
    // Слоёв может быть несколько (`mask-composite: intersect` — вложенные
    // группы, см. `rendered_mask_layers`), поэтому закрываем ровно столько
    // `PopMask`, сколько групп открылось.
    let mask_groups = emit_push_mask(&mut pre, b);
    if mask_groups > 0 {
        for _ in 0..mask_groups {
            post.push(DisplayCommand::PopMask);
        }
        // CSS Masking L1 §4.6 — `mask-clip` restricts the masked painting to the
        // padding/content box. Pushed inside the mask group (after PushMask); the
        // clip result is identical whether the scissor sits inside or outside the
        // offscreen. `post` is reversed later, so pushing PopClip after PopMask
        // yields `… PopClip PopMask` — PopClip nests inside the mask group.
        if let Some(clip) = mask_clip_paint_rect(b) {
            pre.push(DisplayCommand::PushClipRect { rect: clip });
            post.push(DisplayCommand::PopClip);
        }
    }

    // CSS Overflow L3 §3.2: overflow clip to padding-box edge; unconstrained
    // axis uses a BIG sentinel so the GPU scissor doesn't cut off content in
    // that direction. CSS Containment L3 §3.5: contain:paint clips both axes.
    // CSS: overflow — P4 wires: once overflow:scroll/auto are parsed, the
    // PushScrollLayer branch below automatically picks them up.
    let paint_contain = s.contain.0 & ContainFlags::PAINT.0 != 0;
    let clip_x = overflow_clips(s.overflow_x) || paint_contain;
    let clip_y = overflow_clips(s.overflow_y) || paint_contain;
    if clip_x || clip_y {
        const BIG: f32 = 1_000_000.0;
        let px = b.rect.x + s.border_left_width;
        let py = b.rect.y + s.border_top_width;
        let pw = (b.rect.width - s.border_left_width - s.border_right_width).max(0.0);
        let ph = (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0);
        let cr = Rect::new(
            if clip_x { px } else { -BIG },
            if clip_y { py } else { -BIG },
            if clip_x { pw } else { 2.0 * BIG },
            if clip_y { ph } else { 2.0 * BIG },
        );
        // scroll/auto → PushScrollLayer (applies clip + scroll translate).
        // hidden/clip/paint-contain → PushClipRect (clip only, no scroll).
        // BUG-132 fix: если есть border-radius, использовать PushClipRoundedRect
        // вместо PushClipRect (scissors) для скруглённого клипа.
        let is_scroll_x = matches!(s.overflow_x, Overflow::Scroll | Overflow::Auto);
        let is_scroll_y = matches!(s.overflow_y, Overflow::Scroll | Overflow::Auto);
        if (is_scroll_x || is_scroll_y) && !paint_contain {
            overflow_pre.push(DisplayCommand::PushScrollLayer {
                clip_rect: cr,
                scroll_x: b.scroll_x,
                scroll_y: b.scroll_y,
            });
            overflow_post.push(DisplayCommand::PopScrollLayer);
            // BUG-220: the ordered (stacking-context) path lost the scrollbar —
            // only `walk` emitted DrawScrollbar. Emit it here too, into
            // `overflow_post` after PopScrollLayer (caller flushes overflow_post
            // after children, so the bars render at a fixed position over the
            // scrolled content). Same helper as `walk` for pixel parity.
            emit_scrollbars(b, (px, py, pw, ph), is_scroll_x, is_scroll_y, &mut overflow_post);
        } else {
            // BUG-132: скруглённый клип для border-radius + overflow:hidden
            // Разрешаем border-radius значения используя padding-box width как basis
            // (аналогично CornerRadii::from_style в display_list.rs:188).
            let padding_w = b.rect.width - s.border_left_width - s.border_right_width;

            let resolve_radius = |len: &Length, basis: f32| -> f32 {
                match len {
                    Length::Px(v) => *v,
                    Length::Percent(p) => (p / 100.0) * basis,
                    _ => 0.0,
                }
            };

            let tl = resolve_radius(&s.border_top_left_radius, padding_w);
            let tr = resolve_radius(&s.border_top_right_radius, padding_w);
            let br = resolve_radius(&s.border_bottom_right_radius, padding_w);
            let bl = resolve_radius(&s.border_bottom_left_radius, padding_w);
            let has_border_radius = tl > 0.0 || tr > 0.0 || br > 0.0 || bl > 0.0;

            if has_border_radius {
                // PushClipRoundedRect: скруглённый клип с border-radius
                let radii = [tl, tr, br, bl];
                overflow_pre.push(DisplayCommand::PushClipRoundedRect { rect: cr, radii });
            } else {
                // Стандартный PushClipRect (rect-только)
                overflow_pre.push(DisplayCommand::PushClipRect { rect: cr });
            }
            overflow_post.push(DisplayCommand::PopClip);
        }
    }
    if s.mix_blend_mode != LayoutBlendMode::Normal {
        pre.push(DisplayCommand::PushBlendMode {
            mode: map_blend_mode(s.mix_blend_mode),
            bounds: b.rect,
        });
        post.push(DisplayCommand::PopBlendMode);
    }
    // Opacity: animation override wins over style value. CSS Transforms L2
    // §5.1 — `backface-visibility: hidden` culls the whole box (self +
    // descendants) once its 3D transform has rotated the face away from the
    // viewer; reusing the opacity-0 compositing layer is the SC-bucket path's
    // equivalent of `walk`'s early return, since a box with any 3D rotation
    // already owns a stacking context (`creates_stacking_context`).
    let effective_opacity = if is_backface_hidden(b) {
        0.0
    } else {
        ov.and_then(|o| o.opacity).unwrap_or(s.opacity)
    };
    if effective_opacity < 1.0 {
        pre.push(DisplayCommand::PushOpacity { alpha: effective_opacity, bounds: Some(b.rect) });
        post.push(DisplayCommand::PopOpacity);
    } else if s.isolation == Isolation::Isolate
        && box_can_own_stacking_context(b)
        && s.filter.is_empty()
        && s.backdrop_filter.is_empty()
        && s.mix_blend_mode == LayoutBlendMode::Normal
    {
        // CSS Compositing & Blending L1 §2.1 — `isolation: isolate` turns the
        // element into an isolated group: descendant `mix-blend-mode`s must
        // composite against a transparent backdrop that only contains the
        // group's own content, never the page behind it. When any of
        // opacity/filter/backdrop-filter/mix-blend-mode is present the element
        // already renders through an offscreen group layer (which is isolated),
        // so the dedicated layer is only needed when `isolate` is the sole
        // trigger. Reuse the opacity offscreen layer at full alpha: it clears a
        // transparent backdrop, redirects the subtree into it, then composites
        // the result back unchanged — exactly the isolated-group semantics.
        pre.push(DisplayCommand::PushOpacity { alpha: 1.0, bounds: Some(b.rect) });
        post.push(DisplayCommand::PopOpacity);
    }
    // Transform: animation override wins over style value.
    let transform = if let Some(fns) = ov.and_then(|o| o.transform.as_deref()) {
        let (ox, oy, _) = s.transform_origin;
        transform_fns_to_matrix(fns, b.rect.x + ox.resolve(b.rect.width), b.rect.y + oy.resolve(b.rect.height))
    } else {
        forward_box_transform(b)
    };
    if let Some(matrix) = transform {
        pre.push(DisplayCommand::PushTransform { matrix });
        post.push(DisplayCommand::PopTransform);
    }
    // CSS Masking L1 §9 + BUG-140: clip-path задан в локальной системе
    // элемента и переносится его transform-ом — эмитится ВНУТРИ
    // PushTransform, но снаружи filter/backdrop-filter (клип применяется к
    // отфильтрованному выводу, CSS Filter Effects L1 §4).
    if let Some(clip) = &s.clip_path {
        match clip_path_to_shape(clip, b.rect) {
            Some(shape) => pre.push(DisplayCommand::PushClipPath { shape }),
            None => pre.push(DisplayCommand::PushClipRect {
                rect: clip_path_to_rect(clip, b.rect),
            }),
        }
        post.push(DisplayCommand::PopClip);
    }
    // backdrop-filter: outermost SC — captures parent content, filters it, then
    // composites element on top. Must wrap PushFilter so the element's own `filter`
    // applies to the element content before it's blended over the filtered backdrop.
    if !s.backdrop_filter.is_empty() {
        pre.push(DisplayCommand::PushBackdropFilter {
            filters: s.backdrop_filter.clone(),
            bounds: b.rect,
        });
        post.push(DisplayCommand::PopBackdropFilter);
    }
    if !s.filter.is_empty() {
        pre.push(DisplayCommand::PushFilter {
            filters: s.filter.clone(),
            bounds: Some(b.rect),
        });
        post.push(DisplayCommand::PopFilter);
    }
    // post в LIFO порядке относительно pre.
    post.reverse();
    BoxLayerOps { pre, post, overflow_pre, overflow_post }
}

/// Walk-функция, идентичная по триггерам `StackingTree::build`: pre-order,
/// SC-id присваивается монотонно при обнаружении SC-creating потомка.
/// Boxes без SC-trigger остаются в `current_sc`.
///
/// Layer-ops эмиссия:
/// - Для SC-owner (`is_sc_root == true`) Push идёт в `bucket.pre`, Pop в
///   `bucket.post`.
/// - Для non-SC box-а (typically `overflow: hidden` без других триггеров —
///   opacity/blend сами триггерят SC) Push/Pop эмитятся inline в
///   `bucket.contents` вокруг собственного contents-emit-а и потомков.
///
/// `inherited_clips` (BUG-131): rect-клипы (`PushClipRect` /
/// `PushClipRoundedRect`) от non-SC предков, чьи inline push/pop остались в
/// бакете родительского SC и уже закрылись там. Дочерний stacking context
/// рисуется в более позднем слоте painting order, поэтому эти клипы к моменту
/// его отрисовки уже неактивны — их надо переустановить как внешний слой
/// данного SC (push в начало `pre`, pop после `post`/CloseLayer). Без этого
/// трансформированный ребёнок (собственный SC) сбегает из `overflow:hidden`
/// предка.
#[allow(clippy::too_many_arguments)]
fn fill_buckets(
    b: &LayoutBox,
    current_sc: StackingContextId,
    next_sc_id: &mut u32,
    buckets: &mut [ScBucket],
    is_sc_root: bool,
    anim: Option<&CompositorAnimFrame>,
    dpr: f32,
    inherited_clips: &[DisplayCommand],
    split: &mut SplitTracker,
    raw_spans: &mut Vec<RawSpan>,
) {
    let ov = anim.and_then(|a| a.get(b.node));
    let ops = box_layer_ops(b, ov);

    if is_sc_root {
        split.sc_entries += 1;
        if split.enabled && ov.is_some() {
            if current_sc == StackingContextId::ROOT {
                // Override на владельце корневого SC анимирует всю страницу —
                // статики не остаётся, split бессмыслен.
                split.invalid = true;
            } else {
                split.animated_scs.push(current_sc.0);
            }
        }
        let bucket = &mut buckets[current_sc.0 as usize];
        // BUG-131: переустановить клипы non-SC предков как внешний слой SC.
        // Note (ADR-025): re-established clip commands are physically new
        // `DisplayCommand`s, but conceptually belong to whichever ancestor
        // established them first (already spanned there) — attributing this
        // copy to `b` too is a documented over-approximation, not a lie: `b`
        // genuinely re-emits them as its own layer wrapper.
        let pre_start = bucket.pre.len();
        for clip in inherited_clips {
            bucket.pre.push(clip.clone());
        }
        bucket.pre.extend(ops.pre);
        let pre_end = bucket.pre.len();
        record_span(raw_spans, current_sc.0, BucketField::Pre, pre_start, pre_end, b.origin, 0);

        let bg_start = bucket.root_bg.len();
        emit_box_self(b, &mut bucket.root_bg, dpr, None, ov);
        // Overflow-клип — после собственных bg/border (они не клиппятся
        // своим overflow, BUG-123), но до contents с детьми.
        bucket.root_bg.extend(ops.overflow_pre);
        let bg_end = bucket.root_bg.len();
        record_span(raw_spans, current_sc.0, BucketField::RootBg, bg_start, bg_end, b.origin, 0);

        // `post` эмитится в фазе InlineContent после descendants — заполним
        // его сейчас, чтобы не повторно вычислять триггеры.
        let post_start = bucket.post.len();
        bucket.post.extend(ops.overflow_post);
        bucket.post.extend(ops.post);
        // PopClip для переустановленных клипов — в LIFO порядке, после
        // собственных Pop-команд SC (CloseLayer).
        for clip in inherited_clips.iter().rev() {
            bucket.post.push(clip_pop_for(clip));
        }
        let post_end = bucket.post.len();
        record_span(raw_spans, current_sc.0, BucketField::Post, post_start, post_end, b.origin, 0);

        // Этот SC становится новым clip-anchor: его собственный клип +
        // переустановленные inherited-клипы охватывают дочерние SC через
        // root_bg/post (PopClip в CloseLayer после всех детей). Цепочка
        // сбрасывается.
        for child in &b.children {
            let child_creates_sc =
                box_can_own_stacking_context(child) && creates_stacking_context(&child.style);
            if child_creates_sc {
                let id = StackingContextId(*next_sc_id);
                *next_sc_id += 1;
                fill_buckets(child, id, next_sc_id, buckets, true, anim, dpr, &[], split, raw_spans);
            } else {
                fill_buckets(child, current_sc, next_sc_id, buckets, false, anim, dpr, &[], split, raw_spans);
            }
        }
        // BUG-200: redraw collapsed cell borders on top of all cell backgrounds —
        // see the non-SC branch below for the full rationale.
        if collapse_border_repass_applies(b) {
            let mut cells: Vec<&LayoutBox> = Vec::new();
            collect_table_cells(b, &mut cells);
            let bucket = &mut buckets[current_sc.0 as usize];
            for cell in &cells {
                let start = bucket.post.len();
                emit_table_cell_border(cell, &mut bucket.post);
                let end = bucket.post.len();
                record_span(raw_spans, current_sc.0, BucketField::Post, start, end, cell.origin, 0);
            }
        }
    } else {
        // Non-SC box: inline Push/Pop в contents текущего SC. Это нужно для
        // `overflow:hidden` на обычном in-flow box-е (opacity/blend
        // триггерят SC сами, до сюда не дойдут с не-пустым pre).
        // Static/animated split: non-SC узел с override эмитит всё поддерево
        // (layer-ops + self + потомки) подряд в contents текущего SC —
        // запоминаем спан. SC-потомок внутри спана уводит свои команды в
        // собственный бакет (другая позиция painting order) — спан рвётся,
        // split этого кадра инвалидируется через счётчик sc_entries.
        let split_span_start = (split.enabled && ov.is_some())
            .then(|| (buckets[current_sc.0 as usize].contents.len(), split.sc_entries));
        let bucket = &mut buckets[current_sc.0 as usize];
        let lead_start = bucket.contents.len();
        bucket.contents.extend(ops.pre);
        emit_box_self(b, &mut bucket.contents, dpr, None, ov);
        // Overflow-клип после собственных bg/border (BUG-123).
        bucket.contents.extend(ops.overflow_pre.iter().cloned());
        let lead_end = bucket.contents.len();
        record_span(raw_spans, current_sc.0, BucketField::Contents, lead_start, lead_end, b.origin, 0);

        // BUG-131: собственный rect-клип этого non-SC box-а добавляется к
        // цепочке для дочерних SC (его inline push/pop их не охватывает).
        // BUG-159: scroll-слой такого non-SC box-а наследуем ТОЖЕ. Плоский
        // `overflow:auto`/`scroll` контейнер, не являющийся SC-owner, эмитит
        // `PushScrollLayer`/`PopScrollLayer` inline в `contents` текущего SC;
        // их Pop закрывается ДО того, как дочерний stacking context рисуется
        // (более поздний слот painting order), поэтому потомок сбегал бы и из
        // scroll-клипа, и из scroll-translate — вёл бы себя как `position:fixed`
        // (не скроллился при прокрутке). Переустанавливаем scroll-слой как
        // внешний слой каждого дочернего SC, зеркалом clip-наследования. Если
        // же scroll-контейнер сам owns stacking context — он оборачивает
        // потомков через root_bg/post (PushScrollLayer в RootBackground,
        // PopScrollLayer в CloseLayer после всех детей), и сюда не попадает.
        let mut child_clips: Vec<DisplayCommand> = inherited_clips.to_vec();
        for cmd in &ops.overflow_pre {
            if matches!(
                cmd,
                DisplayCommand::PushClipRect { .. }
                    | DisplayCommand::PushClipRoundedRect { .. }
                    | DisplayCommand::PushScrollLayer { .. }
            ) {
                child_clips.push(cmd.clone());
            }
        }

        for child in &b.children {
            let child_creates_sc =
                box_can_own_stacking_context(child) && creates_stacking_context(&child.style);
            if child_creates_sc {
                // BUG-159: `position:fixed` привязан к viewport, `sticky` имеет
                // собственную scroll-aware машинерию — ни тот, ни другой не
                // должны наследовать scroll-translate предка, иначе fixed-оверлей
                // уезжал бы вместе со страницей. Rect-клипы они по-прежнему
                // наследуют (поведение BUG-131 без изменений).
                let child_layers: Vec<DisplayCommand> =
                    if matches!(child.style.position, Position::Fixed | Position::Sticky) {
                        child_clips
                            .iter()
                            .filter(|c| !matches!(c, DisplayCommand::PushScrollLayer { .. }))
                            .cloned()
                            .collect()
                    } else {
                        child_clips.clone()
                    };
                let id = StackingContextId(*next_sc_id);
                *next_sc_id += 1;
                fill_buckets(child, id, next_sc_id, buckets, true, anim, dpr, &child_layers, split, raw_spans);
            } else {
                fill_buckets(child, current_sc, next_sc_id, buckets, false, anim, dpr, &child_clips, split, raw_spans);
            }
        }

        let bucket = &mut buckets[current_sc.0 as usize];
        // BUG-200: under `border-collapse: collapse` adjacent cells overlap by the
        // shared grid-line width (layout pulls them together). Cells are emitted in
        // DOM order, each filling its background then drawing its border. When a later
        // cell has a thinner border than its earlier neighbour (e.g. a 1px `thin` cell
        // after a 3px `thick` one), the later cell's background overpaints the part of
        // the neighbour's collapsed border in the overlap region, leaving only the
        // thinner cell's 1px line instead of the spec's max width (CSS 2.1 §17.6.2).
        // Redraw every cell border once more, on top of all cell backgrounds, so the
        // shared edges composite to the wider border. Borders sit inside the cells'
        // padding, away from content, so the repass is visually a no-op except on the
        // shared grid lines.
        if collapse_border_repass_applies(b) {
            let mut cells: Vec<&LayoutBox> = Vec::new();
            collect_table_cells(b, &mut cells);
            for cell in &cells {
                let start = bucket.contents.len();
                emit_table_cell_border(cell, &mut bucket.contents);
                let end = bucket.contents.len();
                record_span(raw_spans, current_sc.0, BucketField::Contents, start, end, cell.origin, 0);
            }
        }
        let trail_start = bucket.contents.len();
        bucket.contents.extend(ops.overflow_post);
        bucket.contents.extend(ops.post);
        let trail_end = bucket.contents.len();
        record_span(raw_spans, current_sc.0, BucketField::Contents, trail_start, trail_end, b.origin, 0);
        if let Some((start, sc_before)) = split_span_start {
            if split.sc_entries != sc_before {
                split.invalid = true;
            } else {
                let end = buckets[current_sc.0 as usize].contents.len();
                split.content_spans.push((current_sc.0, start, end));
            }
        }
    }
}

/// True when the box is a table using the collapsing-borders model and therefore
/// needs the BUG-200 cell-border repass (cells overlap on shared grid lines).
fn collapse_border_repass_applies(b: &LayoutBox) -> bool {
    matches!(b.kind, BoxKind::Table)
        && matches!(b.style.border_collapse, BorderCollapse::Collapse)
}

/// Парный `Pop` для переустановленного push-клипа (BUG-131 clip inheritance).
/// `inherited_clips` содержит только `PushClipRect` / `PushClipRoundedRect`
/// (scroll-слои отфильтрованы), поэтому всегда `PopClip`; match оставлен общим
/// на случай расширения набора наследуемых клипов.
fn clip_pop_for(push: &DisplayCommand) -> DisplayCommand {
    match push {
        DisplayCommand::PushScrollLayer { .. } => DisplayCommand::PopScrollLayer,
        _ => DisplayCommand::PopClip,
    }
}

/// Если у box-а видимый `outline` — эмитит `DrawOutline`. Caller гарантирует
/// правильный порядок (outline рисуется ПОВЕРХ контента box-а и его детей,
/// но в **рамках своей stacking phase** — Phase 0 без точного разделения
/// фаз outline эмитится сразу после background/border bounding-box-а у
/// `emit_box_self` и после children в `walk`, чтобы потомки не закрывали
/// его пиксели в случае negative `outline-offset`).
///
/// Per CSS Basic UI L4 §5.4: `OutlineColor::Auto` / `CurrentColor`
/// резолвятся в `style.color` (Phase 0 без UA contrast-цвета).
/// Эмитит per-fragment text-shadow DrawText-команды ПЕРЕД основным
/// DrawText. Несколько теней в списке: spec CSS Text Decoration L3 §6
/// — «the first shadow is on top, subsequent shadows are layered
/// behind it», что в painter's order означает обратный обход
/// (последний рисуется первым, первый — последним за основным
/// текстом). Phase 0 — без `blur`: тень = тот же текст со смещением
/// Рисует фон и рамку inline-элемента для одного `InlineFrag`.
///
/// `container_x` — левый край InlineRun-бокса.
/// `frag.x` — смещение текста от container_x (уже учитывает padding_left + border_left).
/// Фон рисуется от border-box левого края до border-box правого края.
fn emit_inline_frag_box(
    out: &mut Vec<DisplayCommand>,
    container_x: f32,
    line_y: f32,
    line_h: f32,
    frag: &InlineFrag,
) {
    if !frag.is_element_box {
        return;
    }
    let s = &frag.style;
    let bl = s.border_left_width;
    let br = s.border_right_width;
    let bt = s.border_top_width;
    let bb = s.border_bottom_width;

    // Border-box left edge = text_x - padding_left - border_left.
    // Snap to integer CSS pixels for consistent rendering with block-level boxes (BUG-084 partial).
    let box_x = (container_x + frag.x - frag.padding_left - bl).round();
    // Border-box width = border_left + padding_left + text + padding_right + border_right.
    let box_w = (bl + frag.padding_left + frag.width + frag.padding_right + br).round();
    let box_h = line_h.round();
    let box_y = line_y.round();

    let radii = CornerRadii::from_style_and_box(s, box_w, box_h);

    // Background (CSS Backgrounds L3: painted over padding+border area).
    if let Some(CssColor::Rgba(bg)) = s.background_color
        && bg.a > 0
        && box_w > 0.0
    {
        let r = Rect::new(box_x, box_y, box_w, box_h);
        if radii.all_zero() {
            out.push(DisplayCommand::FillRect { rect: r, color: bg });
        } else {
            out.push(DisplayCommand::FillRoundedRect { rect: r, color: bg, radii });
        }
    }

    // Border.
    let has_border = s.border_top_style.is_visible()
        || s.border_right_style.is_visible()
        || s.border_bottom_style.is_visible()
        || s.border_left_style.is_visible();
    if has_border && box_w > 0.0 {
        let cur = s.color;
        out.push(DisplayCommand::DrawBorder {
            rect: Rect::new(box_x, box_y, box_w, box_h),
            widths: [bt, br, bb, bl],
            colors: [
                s.border_top_color.resolve(cur),
                s.border_right_color.resolve(cur),
                s.border_bottom_color.resolve(cur),
                s.border_left_color.resolve(cur),
            ],
            styles: [
                s.border_top_style,
                s.border_right_style,
                s.border_bottom_style,
                s.border_left_style,
            ],
            radii,
        });
    }
}

/// (offset_x, offset_y) и shadow.color (None → currentColor =
/// frag.style.color).
/// Эмитит per-fragment text-shadow DrawText-команды ПЕРЕД основным DrawText.
///
/// * Несколько теней: spec CSS Text Decoration L3 §6 — «the first shadow is
///   on top» — обратный обход (последняя в CSS-списке рисуется первой).
/// * `blur > 0`: DrawText заворачивается в `PushFilter { Blur(sigma) }` /
///   `PopFilter`. Renderer применяет двухпроходный Gaussian GPU-шейдер.
///   sigma = blur / 2.0 (то же соглашение, что box-shadow: CSS Text
///   Decoration L3 §6 — blur-radius = стандартное отклонение × 2).
/// * `blur == 0`: DrawText напрямую, без off-screen pass.
fn emit_text_shadows(
    out: &mut Vec<DisplayCommand>,
    base_rect: Rect,
    line_h: f32,
    frag: &InlineFrag,
) {
    if frag.style.text_shadow.is_empty() {
        return;
    }
    for shadow in frag.style.text_shadow.iter().rev() {
        let color = shadow.color.unwrap_or(frag.style.color);
        let sigma = shadow.blur / 2.0;
        let text_shadow_rect = Rect::new(
            base_rect.x + shadow.offset_x,
            base_rect.y + shadow.offset_y,
            base_rect.width,
            line_h,
        );
        if sigma > 0.0 {
            out.push(DisplayCommand::PushFilter {
                filters: vec![FilterFn::Blur(sigma)],
                bounds: Some(text_shadow_rect),
            });
        }
        out.push(DisplayCommand::DrawText {
            font_stretch: frag.style.font_stretch,
            rect: text_shadow_rect,
            text: frag.text.clone(),
            font_size: frag.style.font_size,
            color,
            font_family: frag.style.font_family.clone(),
            font_weight: frag.style.font_weight,
            font_style: frag.style.font_style,
            // CSS Fonts L4 §7.12: for `auto`, inject opsz = font_size so the renderer
            // normalizes it via fvar like any other axis. Skipped for `none` to let
            // font-variation-settings control opsz directly.
            font_features: lumen_layout::style::text_font_features(&frag.style),
            font_palette: palette_selection(&frag.style),
            font_variation_axes: {
                let mut axes: Vec<([u8; 4], f32)> = frag.style.font_variation_settings
                    .iter().map(|s| (s.tag, s.value)).collect();
                if frag.style.font_optical_sizing == FontOpticalSizing::Auto {
                    let has_opsz = axes.iter().any(|(tag, _)| tag == b"opsz");
                    if !has_opsz {
                        axes.push((*b"opsz", frag.style.font_size));
                    }
                }
                if frag.style.font_stretch != FontStretch::NORMAL
                    && !axes.iter().any(|(t, _)| t == b"wdth")
                {
                    axes.push((*b"wdth", frag.style.font_stretch.0 as f32 / 10.0));
                }
                axes
            },
            tab_size: frag.style.tab_size,
            highlight_name: None,
            text_orientation: if frag.style.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
                Some(frag.style.text_orientation)
            } else {
                None
            },
        });
        if sigma > 0.0 {
            out.push(DisplayCommand::PopFilter);
        }
    }
}

/// CSS Backgrounds L3 §3.8 — `background-clip` clip rect для фона.
/// Phase 0 (без border-radius — углы прямоугольные):
/// * `BorderBox` (initial): `b.rect` без изменений.
/// * `PaddingBox`: shrink на border-widths по всем сторонам.
/// * `ContentBox`: shrink на border + padding.
/// * `Text` (L4): Phase 0 fallback на `BorderBox` (реальный glyph-mask
///   clip требует off-screen alpha-pass, P2 п.4+).
///
/// `max(0.0)` страхует от negative-w/h на очень узких box-ах.
/// Возвращает painting area для background с учётом `clip` значения.
///
/// CSS Backgrounds L3 §3.8: border-box = b.rect; padding-box = rect без border-а;
/// content-box = rect без border-а и padding-а. Text трактуется как border-box (Phase 0).
pub(crate) fn background_clip_rect(b: &LayoutBox, clip: BackgroundClip) -> Rect {
    let s = &b.style;
    match clip {
        BackgroundClip::BorderBox | BackgroundClip::Text => b.rect,
        BackgroundClip::PaddingBox => Rect::new(
            b.rect.x + s.border_left_width,
            b.rect.y + s.border_top_width,
            (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
            (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0),
        ),
        BackgroundClip::ContentBox => content_box_rect(b),
    }
}

/// Content box of `b` — `b.rect` (the border box) shrunk by borders and padding.
///
/// CSS Box L3 §1: the content box is where a replaced element's own bitmap is
/// painted, so this is the destination rect for `<canvas>` (BUG-099) as well as
/// the `content-box` arm of [`background_clip_rect`].
fn content_box_rect(b: &LayoutBox) -> Rect {
    let s = &b.style;
    Rect::new(
        b.rect.x + s.border_left_width + s.padding_left.px(),
        b.rect.y + s.border_top_width + s.padding_top.px(),
        (b.rect.width
            - s.border_left_width
            - s.border_right_width
            - s.padding_left.px()
            - s.padding_right.px())
        .max(0.0),
        (b.rect.height
            - s.border_top_width
            - s.border_bottom_width
            - s.padding_top.px()
            - s.padding_bottom.px())
        .max(0.0),
    )
}

/// CSS Backgrounds L3 §3.10: clip для `background-color` — last layer's clip (или default).
pub(crate) fn background_color_clip(b: &LayoutBox) -> BackgroundClip {
    b.style.background_layers.last().map_or(BackgroundClip::BorderBox, |l| l.clip)
}

/// CSS Masking L1 §4.6 — the `mask-clip` painting area for a masked element.
///
/// Returns `Some(rect)` for the boxes that shrink the painting area below the
/// border box (`padding-box`, `content-box`, and `fill-box` — the latter maps
/// to the content box for CSS boxes, CSS Box 4 §1); the caller wraps the mask
/// group in a `PushClipRect` / `PopClip` pair around this rect.
///
/// Returns `None` for the values whose painting area equals the element's
/// border-box `b.rect` (`border-box`, plus `stroke-box`/`view-box` which fall
/// back to the border box for CSS boxes) and for `no-clip` (painting is not
/// clipped) — the clip would be a no-op scissor, so unmasked-default rendering
/// stays byte-identical.
///
/// Covers every layer [`rendered_mask_layers`] actually emits, not just the top
/// one: each layer's `mask-clip` bounds that layer's own contribution, and the
/// emitted layers combine by `intersect` (alpha multiplication), so restricting
/// each factor to its own rect is the same as restricting the product to the
/// **intersection** of those rects. A single rect therefore expresses the whole
/// chain exactly. Layers whose clip is a no-op (`border-box` and friends) drop
/// out of the intersection, so the common single-layer case is unchanged.
fn mask_clip_paint_rect(b: &LayoutBox) -> Option<Rect> {
    rendered_mask_layers(b)
        .iter()
        .filter_map(|l| mask_clip_layer_rect(b, l.clip))
        .reduce(intersect_rects)
}

/// `mask-clip` of a single layer → the rect it restricts painting to, or `None`
/// when that value's painting area is the element's border box (`border-box`,
/// plus `stroke-box`/`view-box` which fall back to it for CSS boxes) or when
/// painting is not clipped at all (`no-clip`). A `None` here means the clip
/// would be a no-op scissor, so unmasked-default rendering stays byte-identical.
fn mask_clip_layer_rect(b: &LayoutBox, clip: MaskClip) -> Option<Rect> {
    match clip {
        MaskClip::PaddingBox => Some(background_clip_rect(b, BackgroundClip::PaddingBox)),
        // fill-box has no SVG geometry on a CSS box → object bounding box = content box.
        MaskClip::ContentBox | MaskClip::FillBox => {
            Some(background_clip_rect(b, BackgroundClip::ContentBox))
        }
        // border-box / stroke-box / view-box all reduce to the border box for a
        // CSS box (= `b.rect`); no-clip disables the clip. All → no-op.
        MaskClip::BorderBox | MaskClip::StrokeBox | MaskClip::ViewBox | MaskClip::NoClip => None,
    }
}

/// Пересечение двух прямоугольников. Непересекающиеся дают прямоугольник
/// нулевого размера (не отрицательного): scissor нулевой площади означает
/// «ничего не рисуется» — верный результат для пустого пересечения.
fn intersect_rects(a: Rect, c: Rect) -> Rect {
    let x = a.x.max(c.x);
    let y = a.y.max(c.y);
    let right = (a.x + a.width).min(c.x + c.width);
    let bottom = (a.y + a.height).min(c.y + c.height);
    Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
}

/// Converts `background-origin` to the equivalent `BackgroundClip` for rect computation.
///
/// CSS Backgrounds L3 §3.5: background-origin has the same box keywords as background-clip
/// except it never has `text` (text-clip only). The conversion is 1:1 for the three box values.
fn origin_to_clip(o: BackgroundOrigin) -> BackgroundClip {
    match o {
        BackgroundOrigin::BorderBox  => BackgroundClip::BorderBox,
        BackgroundOrigin::PaddingBox => BackgroundClip::PaddingBox,
        BackgroundOrigin::ContentBox => BackgroundClip::ContentBox,
    }
}

/// Computes the background positioning area from `background-origin` (CSS Backgrounds L3 §3.5).
///
/// This rect is used for `background-size` (cover/contain/%) and `background-position` (% offsets).
/// Distinct from the painting/clip area computed by [`background_clip_rect`].
fn background_origin_rect(b: &LayoutBox, origin: BackgroundOrigin) -> Rect {
    background_clip_rect(b, origin_to_clip(origin))
}

/// ASCII case-insensitive `starts_with`.
fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// CSS Images L4 §5 — is `value` an `image-set()` / `-webkit-image-set()` expression?
///
/// Used by [`emit_background_layer`] to decide whether to run resolution
/// selection via [`select_image_set_url`] before emitting a `DrawBackgroundImage`.
#[must_use]
pub fn is_image_set(value: &str) -> bool {
    let v = value.trim_start();
    starts_with_ci(v, "image-set(") || starts_with_ci(v, "-webkit-image-set(")
}

/// Strips an outer `image-set( … )` / `-webkit-image-set( … )` wrapper,
/// returning the comma-separated option list. `None` if `s` is not wrapped.
fn strip_image_set_wrapper(s: &str) -> Option<&str> {
    if !s.ends_with(')') {
        return None;
    }
    for prefix in ["image-set(", "-webkit-image-set("] {
        if starts_with_ci(s, prefix) {
            return Some(&s[prefix.len()..s.len() - 1]);
        }
    }
    None
}

/// Splits `s` on top-level commas — commas inside `(…)` or quotes are ignored.
/// Each returned slice is a subslice of `s` (no allocation of contents). Needed
/// because `url(data:…,…)` and function values may contain literal commas.
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut in_quote: Option<u8> = None;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        match in_quote {
            Some(q) => {
                if c == q {
                    in_quote = None;
                }
            }
            None => match c {
                b'"' | b'\'' => in_quote = Some(c),
                b'(' => depth += 1,
                b')' => depth -= 1,
                b',' if depth == 0 => {
                    parts.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            },
        }
        i += 1;
    }
    parts.push(&s[start..]);
    parts
}

/// Strips matching surrounding single/double quotes from `s` (if present).
fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Parses a CSS `<resolution>` token (first whitespace-separated token of
/// `rest`) into device-pixel-ratio units (dppx). Supports `x` / `dppx`
/// (1× = 1 dppx), `dpi` (÷96), `dpcm` (×2.54/96). `None` if not a resolution.
fn parse_resolution(rest: &str) -> Option<f32> {
    let tok = rest.split_whitespace().next()?;
    let lower = tok.to_ascii_lowercase();
    let (num_str, factor) = if let Some(n) = lower.strip_suffix("dppx") {
        (n, 1.0)
    } else if let Some(n) = lower.strip_suffix("dpcm") {
        (n, 2.54 / 96.0)
    } else if let Some(n) = lower.strip_suffix("dpi") {
        (n, 1.0 / 96.0)
    } else {
        let n = lower.strip_suffix('x')?;
        (n, 1.0)
    };
    let v: f32 = num_str.trim().parse().ok()?;
    Some(v * factor)
}

/// Parses one `image-set()` option `<url-or-string> [<resolution>]` into a
/// `(url, resolution_dppx)` pair. URL is returned with the `url(…)` wrapper
/// and any surrounding quotes stripped (a subslice of `opt`). Missing
/// resolution defaults to `1.0` (1×).
fn parse_image_set_option(opt: &str) -> (&str, f32) {
    let opt = opt.trim();
    let bytes = opt.as_bytes();
    let (url, rest): (&str, &str) = if starts_with_ci(opt, "url(") {
        if let Some(close) = opt.find(')') {
            (strip_quotes(opt[4..close].trim()), opt[close + 1..].trim_start())
        } else {
            (strip_quotes(opt[4..].trim()), "")
        }
    } else if bytes.first() == Some(&b'"') || bytes.first() == Some(&b'\'') {
        let q = bytes[0] as char;
        if let Some(rel) = opt[1..].find(q) {
            (&opt[1..1 + rel], opt[1 + rel + 1..].trim_start())
        } else {
            (&opt[1..], "")
        }
    } else {
        match opt.find(char::is_whitespace) {
            Some(sp) => (&opt[..sp], opt[sp..].trim_start()),
            None => (opt, ""),
        }
    };
    (url, parse_resolution(rest).unwrap_or(1.0))
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

/// CSS Backgrounds L3 §3.3–3.5 — прямоугольники-плитки для градиентного слоя с
/// явным `background-size`.
///
/// У градиента нет ни внутреннего размера, ни соотношения сторон (CSS Images),
/// поэтому при `background-size: <length>` он рисуется плитками этого размера,
/// размещёнными по `background-position` и повторёнными по `background-repeat`;
/// `auto`-ось разрешается в размер positioning area по этой оси (не
/// пропорциональное масштабирование — соотношения нет). Возвращает по одному
/// rect на плитку: каждый отображает цветовую линию/окружность градиента в свою
/// плитку. Геометрия зеркалит [`super::backends`] image-tiling
/// (`bg_tile_geometry` + loop), чтобы градиенты и картинки плитковались
/// одинаково. Вызывается только для `BackgroundSize::Length`;
/// auto/cover/contain заливают всю area одной командой.
fn gradient_tile_rects(
    tile_w: f32,
    tile_h: f32,
    position: ObjectPosition,
    repeat: BackgroundRepeat,
    origin: Rect,
    clip: Rect,
) -> Vec<Rect> {
    if tile_w <= 0.0 || tile_h <= 0.0 {
        return Vec::new();
    }
    let off_x = position.x.resolve(origin.width - tile_w);
    let off_y = position.y.resolve(origin.height - tile_h);
    let tile_x0 = origin.x + off_x;
    let tile_y0 = origin.y + off_y;

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
            let (sx, step_x, rx) = space_axis_geometry(origin.x, origin.width, tile_w, off_x);
            let (sy, step_y, ry) = space_axis_geometry(origin.y, origin.height, tile_h, off_y);
            (sx, step_x, rx, sy, step_y, ry)
        }
    };

    // Cap, чтобы крошечная плитка с repeat не породила взрывное число команд.
    const MAX_TILES: usize = 4096;
    let mut rects = Vec::new();
    let x_end = clip.x + clip.width;
    let y_end = clip.y + clip.height;
    let mut ty = tile_y_start;
    loop {
        if ty >= y_end || rects.len() >= MAX_TILES {
            break;
        }
        if ty + tile_h > clip.y {
            let mut tx = tile_x_start;
            loop {
                if tx >= x_end || rects.len() >= MAX_TILES {
                    break;
                }
                if tx + tile_w > clip.x {
                    rects.push(Rect::new(tx, ty, tile_w, tile_h));
                }
                if !repeat_x {
                    break;
                }
                tx += step_x;
            }
        }
        if !repeat_y {
            break;
        }
        ty += step_y;
    }
    rects
}

/// CSS Backgrounds L3 §3.3–3.5 — список rect-ов, в которые рисуется градиентный
/// слой, и нужно ли клипировать их по painting area.
///
/// `BackgroundSize::Length` → плитки через [`gradient_tile_rects`] (требуют клипа
/// по `clip`, т.к. плитка может выходить за painting area). Auto/Cover/Contain
/// (у градиента нет внутреннего размера/ratio) → одна команда на всю painting
/// area (`clip`) — историческое поведение, клип не нужен.
fn gradient_paint_rects(layer: &BackgroundLayer, origin: Rect, clip: Rect) -> (Vec<Rect>, bool) {
    match layer.size {
        BackgroundSize::Length(w, h) => {
            // Gradients have no intrinsic size/ratio: an `auto` axis falls back
            // to the positioning-area extent; percent resolves against it.
            let tile_w = w.resolve(origin.width).unwrap_or(origin.width).max(1.0);
            let tile_h = h.resolve(origin.height).unwrap_or(origin.height).max(1.0);
            let tiles =
                gradient_tile_rects(tile_w, tile_h, layer.position, layer.repeat, origin, clip);
            (tiles, true)
        }
        _ => (vec![clip], false),
    }
}

/// Эмитит одну background-layer команду.
///
/// CSS Compositing L1 §8.3: если `layer.blend_mode != Normal`, оборачивает
/// draw-команду в PushBlendMode/PopBlendMode. Слои рисуются снизу вверх,
/// каждый с указанным blend mode относительно уже нарисованных слоёв ниже.
///
/// `dpr` — device pixel ratio, передаётся в [`select_image_set_url`] для
/// выбора варианта `image-set()` (CSS Images L4 §5).
fn emit_background_layer(
    out: &mut Vec<DisplayCommand>,
    b: &LayoutBox,
    layer: &BackgroundLayer,
    dpr: f32,
    // CSS Compositing L1 §8.3: the bottom-most background layer blends with transparent
    // background-color. For premultiplied alpha, multiply(src, 0) = src (identity), so
    // blend mode has no visible effect — skip PushBlendMode to avoid blending against the
    // stacking context instead of an isolated background canvas.
    suppress_blend: bool,
    // CSS Backgrounds L3 §4.3: a gradient background is clipped to the same
    // rounded painting-area box as a solid `background-color` (BUG-631) — the
    // border-box radii, reused as-is for any `background-clip` box (same
    // simplification the solid-color path already makes at the `FillRoundedRect`
    // call site above).
    radii: CornerRadii,
) {
    let clip = background_clip_rect(b, layer.clip);
    if clip.width <= 0.0 || clip.height <= 0.0 {
        return;
    }
    // CSS Backgrounds L3 §3.5: positioning area (background-origin) is independent of
    // the painting/clip area (background-clip). size/position calculations use origin_rect.
    let origin = background_origin_rect(b, layer.origin);
    let use_blend = !suppress_blend && layer.blend_mode != LayoutBlendMode::Normal;
    if use_blend {
        out.push(DisplayCommand::PushBlendMode { mode: map_blend_mode(layer.blend_mode), bounds: clip });
    }
    match &layer.image {
        BackgroundImage::Url(src) if !src.is_empty() => {
            // CSS: image-set — resolve image-set() to the best URL for the
            // current device pixel ratio; plain urls pass through unchanged.
            // P4 wires parsing: keep the raw `image-set(…)` string in
            // BackgroundImage::Url so this resolution triggers (CSS Images L4 §5).
            let resolved = if is_image_set(src) {
                select_image_set_url(src, dpr)
            } else {
                src.as_str()
            };
            if !resolved.is_empty() {
                out.push(DisplayCommand::DrawBackgroundImage {
                    rect: clip,
                    origin_rect: origin,
                    src: resolved.to_string(),
                    size: layer.size,
                    position: layer.position,
                    repeat: layer.repeat,
                    image_rendering: b.style.image_rendering,
                });
            }
        }
        BackgroundImage::Gradient(ParsedGradient::Linear { angle_deg, corner, stops, repeating }) => {
            let (rects, needs_clip) = gradient_paint_rects(layer, origin, clip);
            // BUG-631: a rounded box needs its gradient clipped to the rounded
            // painting area even when `needs_clip` is false (single full-`clip`
            // rect, otherwise unclipped) — square corners must not leak through.
            let has_radii = !radii.all_zero();
            if (needs_clip || has_radii) && !rects.is_empty() {
                if has_radii {
                    out.push(DisplayCommand::PushClipRoundedRect {
                        rect: clip,
                        radii: [radii.tl, radii.tr, radii.br, radii.bl],
                    });
                } else {
                    out.push(DisplayCommand::PushClipRect { rect: clip });
                }
            }
            for r in &rects {
                // CSS Images L3 §3.1 — a `to <corner>` keyword's true angle
                // depends on this paint rect's aspect ratio; an explicit
                // `<angle>` is box-independent and passes through unchanged.
                let resolved_angle = corner.map_or(*angle_deg, |c| c.angle_deg(r.width, r.height));
                out.push(DisplayCommand::DrawLinearGradient {
                    rect: *r,
                    angle_deg: resolved_angle,
                    stops: stops.clone(),
                    repeating: *repeating,
                });
            }
            if (needs_clip || has_radii) && !rects.is_empty() {
                out.push(DisplayCommand::PopClip);
            }
        }
        BackgroundImage::Gradient(ParsedGradient::Radial {
            center_x_pct, center_y_pct, shape, size, stops, repeating,
        }) => {
            let (rects, needs_clip) = gradient_paint_rects(layer, origin, clip);
            let has_radii = !radii.all_zero();
            if (needs_clip || has_radii) && !rects.is_empty() {
                if has_radii {
                    out.push(DisplayCommand::PushClipRoundedRect {
                        rect: clip,
                        radii: [radii.tl, radii.tr, radii.br, radii.bl],
                    });
                } else {
                    out.push(DisplayCommand::PushClipRect { rect: clip });
                }
            }
            for r in &rects {
                // Resolve the CSS ending-shape/size to concrete px radii against
                // this paint rect (CSS Images L3 §3.5.1) — circle keeps rx == ry,
                // ellipse gets independent radii (BUG-239).
                let (radius_x, radius_y) = lumen_layout::radial_gradient_radii(
                    *shape, *size, *center_x_pct, *center_y_pct, r.width, r.height,
                );
                out.push(DisplayCommand::DrawRadialGradient {
                    rect: *r,
                    center_x_pct: *center_x_pct,
                    center_y_pct: *center_y_pct,
                    radius_x,
                    radius_y,
                    stops: stops.clone(),
                    repeating: *repeating,
                });
            }
            if (needs_clip || has_radii) && !rects.is_empty() {
                out.push(DisplayCommand::PopClip);
            }
        }
        BackgroundImage::Gradient(ParsedGradient::Conic {
            center_x_pct, center_y_pct, from_angle_deg, stops, repeating
        }) => {
            let (rects, needs_clip) = gradient_paint_rects(layer, origin, clip);
            let has_radii = !radii.all_zero();
            if (needs_clip || has_radii) && !rects.is_empty() {
                if has_radii {
                    out.push(DisplayCommand::PushClipRoundedRect {
                        rect: clip,
                        radii: [radii.tl, radii.tr, radii.br, radii.bl],
                    });
                } else {
                    out.push(DisplayCommand::PushClipRect { rect: clip });
                }
            }
            for r in &rects {
                out.push(DisplayCommand::DrawConicGradient {
                    rect: *r,
                    center_x_pct: *center_x_pct,
                    center_y_pct: *center_y_pct,
                    from_angle_deg: *from_angle_deg,
                    stops: stops.clone(),
                    repeating: *repeating,
                });
            }
            if (needs_clip || has_radii) && !rects.is_empty() {
                out.push(DisplayCommand::PopClip);
            }
        }
        BackgroundImage::CrossFade { a, b, t } => {
            // CSS Images L4 §4 — emit DrawCrossFade for two-URL cross-fade.
            // Gradient sides are not composited via DrawCrossFade (Phase 0 scope).
            if let (BackgroundImage::Url(url_a), BackgroundImage::Url(url_b)) =
                (a.as_ref(), b.as_ref())
            {
                let src_a = if is_image_set(url_a) {
                    select_image_set_url(url_a, dpr).to_string()
                } else {
                    url_a.clone()
                };
                let src_b = if is_image_set(url_b) {
                    select_image_set_url(url_b, dpr).to_string()
                } else {
                    url_b.clone()
                };
                if !src_a.is_empty() && !src_b.is_empty() {
                    out.push(DisplayCommand::DrawCrossFade {
                        dest: clip,
                        src_a,
                        src_b,
                        progress: *t,
                    });
                }
            }
        }
        BackgroundImage::Paint(name) => {
            // CSS Paint API (Houdini) — paint(name) generates dynamic image via registered worklet.
            // Phase 0: render as grey placeholder `DrawImage`; Phase 1: invoke worklet paint() callback.
            // `// CSS: background: paint(name)`
            out.push(DisplayCommand::DrawBackgroundImage {
                rect: clip,
                origin_rect: origin,
                src: format!("paint:{}", name),  // Prefixed to distinguish from URL images.
                size: layer.size,
                position: layer.position,
                repeat: layer.repeat,
                image_rendering: b.style.image_rendering,
            });
        }
        _ => {}
    }
    if use_blend {
        out.push(DisplayCommand::PopBlendMode);
    }
}

/// CSS Backgrounds L3 §3.10 — эмитит все фоновые слои элемента.
///
/// CSS Backgrounds L3 §3: слои рисуются снизу вверх — последний в списке (Vec)
/// рисуется первым (самый нижний), первый в списке — последним (самый верхний).
/// Пустых layers → no-op.
///
/// CSS Compositing L1 §8.3: background creates an isolated compositing group.
/// The bottom-most layer blends against transparent background-color; for common
/// blend modes (multiply, screen etc.) this is identity for premultiplied alpha,
/// so we suppress PushBlendMode for that layer.
///
/// BUG-277 slice 2: when a non-bottom layer actually blends, the layer stack is
/// wrapped in its own `PushOpacity{alpha:1.0}`/`PopOpacity` isolation group. Without
/// it, the wgpu renderer's level-based compositor has no readable "parent" texture
/// for a top-level (non-nested) box — `PopBlendMode`'s composite silently falls back
/// to plain alpha-over, dropping the blend effect entirely (renderer.rs `Composite`
/// requires `from_level > 1` to read a parent layer; a box with no ancestor
/// stacking context sits at `from_level == 1`, whose "parent" is the real
/// swapchain surface, which has no `TEXTURE_BINDING` usage and can't be sampled).
/// Forcing an isolate group gives the blend pair its own two-level offscreen stack
/// (bottom layer at the isolate's level, top layer nested one level above it)
/// regardless of ancestor nesting, matching cpu_raster/femtovg (whose immediate-mode
/// canvas already contains only this box's own painted content at this point) and
/// the CSS spec's "background forms an isolated group" semantics.
fn emit_background_image(out: &mut Vec<DisplayCommand>, b: &LayoutBox, dpr: f32) {
    // Isolation is needed only if some non-bottom layer actually blends (i == 0 is
    // always suppressed, see `emit_background_layer`'s `suppress_blend`).
    let needs_isolation = b
        .style
        .background_layers
        .iter()
        .rev()
        .enumerate()
        .any(|(i, layer)| i > 0 && layer.blend_mode != LayoutBlendMode::Normal);
    if needs_isolation {
        out.push(DisplayCommand::PushOpacity { alpha: 1.0, bounds: Some(b.rect) });
    }
    // CSS Backgrounds L3 §4.3 — same border-box radii the solid `background-color`
    // path uses (BUG-631: a gradient background must be clipped to the same
    // rounded box, not a square `PushClipRect`).
    let radii = CornerRadii::from_style_and_box(&b.style, b.rect.width, b.rect.height);
    // Рисуем в обратном порядке: последний слой = нижний (рисуется первым).
    for (i, layer) in b.style.background_layers.iter().rev().enumerate() {
        // i == 0 is the bottom-most layer; suppress its blend mode (identity effect).
        emit_background_layer(out, b, layer, dpr, i == 0, radii);
    }
    if needs_isolation {
        out.push(DisplayCommand::PopOpacity);
    }
}

/// CSS Masking L1 §4 — эмитит PushMask* перед элементом + его детьми.
/// Возвращает `true` если команда была эмитирована (нужен парный PopMask).
/// `rect` = border-box элемента (mask painting area).
/// CSS Masking L1 §6.4 `mask-mode: luminance` — rewrites each gradient stop so
/// its alpha channel encodes `luminance(rgb)·alpha`. The mask backends
/// (`composite_mask_layer` in femtovg, `render_mask` in cpu_raster) read only
/// the rendered gradient's **alpha** under a `DestinationIn` composite, so
/// baking luminance into the alpha here makes a dark mask pixel hide the element
/// even when it is fully opaque — without threading the mode into the backends.
/// For `mask-mode: alpha` (default) the stops are returned unchanged.
///
/// Luminance is exact across a linear gradient: `luma` is a linear combination
/// of R, G, B, so `luma(lerp(c0, c1, t)) == lerp(luma(c0), luma(c1), t)`.
fn mask_stops_for_mode(stops: &[GradientStop], mode: lumen_layout::MaskMode) -> Vec<GradientStop> {
    match mode {
        lumen_layout::MaskMode::Alpha => stops.to_vec(),
        lumen_layout::MaskMode::Luminance => stops
            .iter()
            .map(|s| {
                let c = s.color;
                let luma = 0.2126 * f32::from(c.r)
                    + 0.7152 * f32::from(c.g)
                    + 0.0722 * f32::from(c.b);
                let a = (luma / 255.0 * f32::from(c.a)).round().clamp(0.0, 255.0) as u8;
                GradientStop {
                    color: Color { a, ..c },
                    color_space: s.color_space,
                    position: s.position.clone(),
                }
            })
            .collect(),
    }
}

/// CSS Masking L1 §4.7/§4.9 — какие слои из [`ComputedStyle::mask_layers`]
/// реально попадают в display list.
///
/// Один `PushMask*` несёт ровно один mask-канал, но группы **вкладываются**, а
/// вложение перемножает альфы: содержимое под `PushMask(A) PushMask(B) … PopMask
/// PopMask` получает `alpha · b · a`. Умножение — это ровно Porter-Duff
/// source-in, то есть `mask-composite: intersect`. Поэтому цепочку, где каждый
/// слой поверх нижнего складывается через `intersect`, можно отрендерить точно,
/// не собирая маску в отдельный офскрин и не трогая бэкенды. Порядок вложения
/// не важен — умножение коммутативно.
///
/// Условия, при которых эмитятся все слои:
/// * у каждого слоя есть рисуемый источник (`url(...)` или градиент) — слой
///   `none` даёт прозрачную маску и в `intersect` обнулил бы результат;
/// * у всех слоёв, кроме нижнего, `composite: intersect`;
/// * у нижнего слоя `composite` **не** `intersect` — его оператор применяется к
///   прозрачному фону, где `add`/`subtract`/`exclude` дают сам слой, а
///   `intersect` (source-in с прозрачным) вычистил бы маску целиком. Реализация
///   этого вырожденного случая расходится между браузерами, поэтому он уходит в
///   тот же fallback, а не рендерится по букве спеки.
///
/// Иначе — `// CSS: mask-composite` — рендерится только верхний слой (прежнее
/// поведение). `add`/`subtract`/`exclude` между слоями вложением не выражаются:
/// им нужна сборка маски в отдельный офскрин во всех трёх бэкендах (femtovg,
/// wgpu `renderer.rs`, `cpu_raster.rs`), что уже renderer-side задача, а не
/// стилевая.
fn rendered_mask_layers(b: &LayoutBox) -> &[MaskLayer] {
    let layers = &b.style.mask_layers;
    let Some((bottom, upper)) = layers.split_last() else {
        return &[];
    };
    let all_intersect = !upper.is_empty()
        && upper.iter().all(|l| l.composite == MaskComposite::Intersect)
        && bottom.composite != MaskComposite::Intersect
        && layers.iter().all(is_renderable_mask_source);
    if all_intersect { layers } else { &layers[..1] }
}

/// Есть ли у слоя источник, который [`emit_push_mask`] умеет превратить в
/// `PushMask*`. `mask-image: none` и пустой `url()` — нет.
fn is_renderable_mask_source(l: &MaskLayer) -> bool {
    match &l.image {
        BackgroundImage::Url(src) => !src.is_empty(),
        BackgroundImage::Gradient(_) => true,
        _ => false,
    }
}

/// Эмитит mask-группы элемента. Возвращает число открытых групп — столько же
/// `PopMask` обязан выставить вызывающий.
fn emit_push_mask(out: &mut Vec<DisplayCommand>, b: &LayoutBox) -> usize {
    let mut opened = 0;
    // Верхний слой идёт наружу. Для `intersect` порядок безразличен, но так
    // display list читается в том же порядке, что и CSS-список слоёв.
    for layer in rendered_mask_layers(b) {
        if emit_push_mask_layer(out, b, layer) {
            opened += 1;
        }
    }
    opened
}

/// Эмитит `PushMask*` одного слоя. `false` — источник не рисуемый, группа не
/// открыта (парный `PopMask` не нужен).
fn emit_push_mask_layer(out: &mut Vec<DisplayCommand>, b: &LayoutBox, layer: &MaskLayer) -> bool {
    // CSS Masking L1 §4.5 — `mask-origin` sets the mask **positioning area**
    // (border/padding/content box). Reuses the background-origin geometry; for
    // the default `border-box` this equals `b.rect`, so existing behaviour is
    // unchanged.
    let rect = background_origin_rect(b, layer.origin);
    let mode = layer.mode;
    // CSS Masking L1 §4.6 — `mask-clip` restricts the masked element's painting
    // area. It is wired at the call sites by wrapping the whole mask group in a
    // `PushClipRect` / `PopClip` pair (see `mask_clip_paint_rect`), reusing the
    // existing scissor path instead of threading a clip rect through the mask
    // commands + every backend.
    match &layer.image {
        BackgroundImage::Url(src) if !src.is_empty() => {
            out.push(DisplayCommand::PushMaskImage {
                rect,
                src: src.clone(),
                size: layer.size,
                // CSS Masking L1 §4.4 — `mask-position` (same syntax as
                // background-position). Applies to image masks; gradient masks
                // derive their geometry from `rect` above.
                position: layer.position,
                repeat: layer.repeat,
                image_rendering: b.style.image_rendering,
            });
            true
        }
        BackgroundImage::Gradient(ParsedGradient::Linear { angle_deg, corner, stops, repeating }) => {
            let resolved_angle = corner.map_or(*angle_deg, |c| c.angle_deg(rect.width, rect.height));
            out.push(DisplayCommand::PushMaskLinearGradient {
                rect,
                angle_deg: resolved_angle,
                stops: mask_stops_for_mode(stops, mode),
                repeating: *repeating,
            });
            true
        }
        BackgroundImage::Gradient(ParsedGradient::Radial {
            center_x_pct, center_y_pct, stops, repeating, ..
        }) => {
            out.push(DisplayCommand::PushMaskRadialGradient {
                rect,
                center_x_pct: *center_x_pct,
                center_y_pct: *center_y_pct,
                stops: mask_stops_for_mode(stops, mode),
                repeating: *repeating,
            });
            true
        }
        BackgroundImage::Gradient(ParsedGradient::Conic {
            center_x_pct, center_y_pct, from_angle_deg, stops, repeating
        }) => {
            out.push(DisplayCommand::PushMaskConicGradient {
                rect,
                center_x_pct: *center_x_pct,
                center_y_pct: *center_y_pct,
                from_angle_deg: *from_angle_deg,
                stops: mask_stops_for_mode(stops, mode),
                repeating: *repeating,
            });
            true
        }
        _ => false,
    }
}

/// Эмитит outset box-shadow ПЕРЕД background (painter's order по CSS
/// Backgrounds L3 §4.6 — shadow «cast … behind the element», то есть
/// под background-color).
/// * `blur > 0`: shadow рисуется через `PushFilter { Blur(sigma) }` +
///   `FillRect` + `PopFilter`. Renderer применяет двухпроходный Gaussian
///   GPU-шейдер. sigma = blur / 2.0 (CSS Backgrounds L3 §4.6 — blur-radius
///   = standard deviation × 2, аналогично Edge/Chrome/Firefox).
/// * `blur == 0`: резкий `FillRect` напрямую (без offscreen pass).
/// * `inset` тени рисуются отдельно — `emit_inset_box_shadows` после
///   background и до border, по спеке §3.5.1 «inset shadows are drawn
///   inside the box, above the background and below the border».
/// * Multiple shadows: per spec «the first shadow is on top» —
///   эмитим в reverse iter (последняя в CSS-списке рисуется первой /
///   ниже всех, первая — последней-перед-background).
/// * `spread`: расширяет / сжимает rect ± по всем сторонам перед
///   смещением. Полностью схлопывающийся rect (w/h ≤ 0) — skip.
/// * Полностью прозрачная shadow (color.a == 0) — skip.
fn emit_box_shadows(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    if s.box_shadow.is_empty() {
        return;
    }
    for shadow in s.box_shadow.iter().rev() {
        if shadow.inset {
            continue;
        }
        let color = shadow.color.unwrap_or(s.color);
        if color.a == 0 {
            continue;
        }
        // Snap shadow rect to integer CSS pixels — offset/spread are CSS lengths that can be
        // fractional; unsnapped values produce sub-pixel shadows vs Edge (BUG-084 partial).
        let x = (b.rect.x + shadow.offset_x - shadow.spread).round();
        let y = (b.rect.y + shadow.offset_y - shadow.spread).round();
        let w = (b.rect.width + 2.0 * shadow.spread).round();
        let h = (b.rect.height + 2.0 * shadow.spread).round();
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let sigma = shadow.blur / 2.0;
        let shadow_rect = Rect::new(x, y, w, h);
        // CSS Backgrounds L3 §7.1.1: the shadow shape is the border box expanded by
        // `spread`, and each corner with a non-zero border-radius is rounded with its
        // radius increased by the spread distance (square corners stay square). Without
        // this, a hard/blurred shadow on a rounded box renders as a square silhouette.
        let base_radii = CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height);
        if sigma > 0.0 {
            out.push(DisplayCommand::PushFilter {
                filters: vec![FilterFn::Blur(sigma)],
                bounds: Some(shadow_rect),
            });
        }
        if base_radii.all_zero() {
            out.push(DisplayCommand::FillRect {
                rect: shadow_rect,
                color,
            });
        } else {
            out.push(DisplayCommand::FillRoundedRect {
                rect: shadow_rect,
                color,
                radii: spread_corner_radii(&base_radii, shadow.spread),
            });
        }
        if sigma > 0.0 {
            out.push(DisplayCommand::PopFilter);
        }
    }
}

/// Expands a box's resolved `CornerRadii` to the corner radii of its outer
/// box-shadow shape per CSS Backgrounds L3 §7.1.1: a corner with a non-zero
/// border-radius gets its radius increased by the spread distance (clamped at
/// zero for large negative spread); a square corner (radius 0) stays square.
fn spread_corner_radii(base: &CornerRadii, spread: f32) -> CornerRadii {
    let grow = |r: f32| if r > 0.0 { (r + spread).max(0.0) } else { 0.0 };
    CornerRadii {
        tl: grow(base.tl),
        tl_y: grow(base.tl_y),
        tr: grow(base.tr),
        tr_y: grow(base.tr_y),
        br: grow(base.br),
        br_y: grow(base.br_y),
        bl: grow(base.bl),
        bl_y: grow(base.bl_y),
    }
}

/// Эмитит inset box-shadow МЕЖДУ background и border (CSS Backgrounds
/// L3 §3.5.1: «inset shadows are drawn inside the padding edge of the
/// box, above the background but below the border and content»).
///
/// Геометрия per spec:
/// * **outer** = padding-box (border-rect минус border-widths) — это
///   область, в которой видна тень; тень клипается outer-ом.
/// * **inner** = `outer`, **смещённый** на `(offset_x, offset_y)` и
///   **сжатый** на `spread` (положительный spread → меньший inner →
///   шире кольцо тени; отрицательный spread → inner может выйти за
///   outer → тень коллапсирует к нулю).
///
/// Видимая тень = `outer \ (inner ∩ outer)` — кольцо/каёмка. Phase 0
/// без border-radius / blur разворачивается в 4 FillRect-а (top /
/// bottom / left / right), окаймляющие «дырку» внутри outer. Если
/// inner полностью НЕ пересекается с outer — заливаем весь outer
/// одним FillRect (тень закрывает всё). Если inner полностью покрывает
/// outer (отрицательный spread достаточной величины) — ничего не
/// эмитим.
///
/// Multiple inset shadows: тот же reverse-iter, что у outset — «first
/// shadow on top» (последняя в CSS-списке кладётся первой, первая —
/// последней; верхние перекрывают нижние). Несколько inset друг над
/// другом — нормальный паттерн под «двойную» обводку.
///
/// Phase 0 ограничения:
/// * `blur` игнорируется — inset blur требует clip-маски вокруг padding-box,
///   иначе размытие вытекает за границы элемента. Clip-маски будут реализованы
///   как часть stacking context (P1 п.2A). Outset blur реализован через
///   PushFilter/PopFilter без clip.
/// * Полностью прозрачная shadow (`color.a == 0`) — skip.
/// * `currentColor` для `color: None` берётся из `s.color`.
fn emit_inset_box_shadows(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    if s.box_shadow.is_empty() {
        return;
    }
    let outer_x = b.rect.x + s.border_left_width;
    let outer_y = b.rect.y + s.border_top_width;
    let outer_w = (b.rect.width - s.border_left_width - s.border_right_width).max(0.0);
    let outer_h = (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0);
    if outer_w <= 0.0 || outer_h <= 0.0 {
        return;
    }
    let outer_right = outer_x + outer_w;
    let outer_bottom = outer_y + outer_h;
    for shadow in s.box_shadow.iter().rev() {
        if !shadow.inset {
            continue;
        }
        let color = shadow.color.unwrap_or(s.color);
        if color.a == 0 {
            continue;
        }
        // inner = outer, translated by offset, then inset by spread.
        let inner_x = outer_x + shadow.offset_x + shadow.spread;
        let inner_y = outer_y + shadow.offset_y + shadow.spread;
        let inner_right = outer_right + shadow.offset_x - shadow.spread;
        let inner_bottom = outer_bottom + shadow.offset_y - shadow.spread;
        // Inner полностью покрывает outer — кольцо нулевое, тени не видно.
        if inner_x <= outer_x
            && inner_y <= outer_y
            && inner_right >= outer_right
            && inner_bottom >= outer_bottom
        {
            continue;
        }
        // Inner не пересекает outer — тень покрывает весь outer.
        let no_overlap = inner_x >= outer_right
            || inner_y >= outer_bottom
            || inner_right <= outer_x
            || inner_bottom <= outer_y;
        if no_overlap {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(outer_x, outer_y, outer_w, outer_h),
                color,
            });
            continue;
        }
        // Hole = inner clamped to outer.
        let hole_left = inner_x.max(outer_x);
        let hole_top = inner_y.max(outer_y);
        let hole_right = inner_right.min(outer_right);
        let hole_bottom = inner_bottom.min(outer_bottom);
        // Top frame.
        if hole_top > outer_y {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(outer_x, outer_y, outer_w, hole_top - outer_y),
                color,
            });
        }
        // Bottom frame.
        if hole_bottom < outer_bottom {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(outer_x, hole_bottom, outer_w, outer_bottom - hole_bottom),
                color,
            });
        }
        // Left frame.
        if hole_left > outer_x {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(outer_x, hole_top, hole_left - outer_x, hole_bottom - hole_top),
                color,
            });
        }
        // Right frame.
        if hole_right < outer_right {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(
                    hole_right,
                    hole_top,
                    outer_right - hole_right,
                    hole_bottom - hole_top,
                ),
                color,
            });
        }
    }
}

/// Default scrollbar gutter width for `scrollbar-width: auto` in CSS px.
const SCROLLBAR_WIDTH: f32 = 12.0;
/// Scrollbar gutter width for `scrollbar-width: thin` in CSS px.
const SCROLLBAR_WIDTH_THIN: f32 = 6.0;
/// Minimum thumb length in CSS px so it stays clickable at large scroll ranges.
const SCROLLBAR_MIN_THUMB: f32 = 20.0;
/// Default track color: very light translucent grey.
const SCROLLBAR_TRACK_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.08];
/// Default thumb color: semi-transparent dark pill.
const SCROLLBAR_THUMB_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.38];

/// Convert a CSS `Color` (u8 sRGB) to a linear `[f32; 4]` array for the renderer.
fn color_u8_to_f32(c: Color) -> [f32; 4] {
    [
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
        c.a as f32 / 255.0,
    ]
}

/// Input geometry for `scrollbar_rects`.
struct ScrollbarInput {
    /// Padding-box origin and size in document-space CSS px.
    pub clip_x: f32,
    pub clip_y: f32,
    pub clip_w: f32,
    pub clip_h: f32,
    /// Current scroll offset in CSS px.
    pub scroll_x: f32,
    pub scroll_y: f32,
    /// Total content width / height in CSS px.
    pub content_w: f32,
    pub content_h: f32,
    /// Emit vertical scrollbar when content_h > clip_h.
    pub need_v: bool,
    /// Emit horizontal scrollbar when content_w > clip_w.
    pub need_h: bool,
    /// Scrollbar gutter width/height in CSS px. From `scrollbar-width`: auto=12, thin=6.
    pub gutter_px: f32,
}

/// One axis result: `(track_rect, thumb_rect)` in document-space CSS px.
type ScrollbarAxis = Option<(Rect, Rect)>;

/// Compute track and thumb rects for the vertical and horizontal scrollbar axes.
///
/// Returns `(vertical, horizontal)` where each is `Some((track, thumb))` if the
/// axis overflows, or `None` if the content fits within the clip rect for that axis.
fn scrollbar_rects(i: &ScrollbarInput) -> (ScrollbarAxis, ScrollbarAxis) {
    let g = i.gutter_px;
    // Minimum thumb length scales with gutter so thin scrollbars stay clickable.
    let min_thumb = SCROLLBAR_MIN_THUMB.min(g * 2.0).max(g);
    // Inset from track edge — 2px for auto, 1px for thin.
    let inset = if g >= 10.0 { 2.0 } else { 1.0 };

    let v = if i.need_v && i.content_h > i.clip_h {
        let track = Rect::new(
            i.clip_x + i.clip_w - g,
            i.clip_y,
            g,
            i.clip_h,
        );
        let thumb_h = ((i.clip_h / i.content_h) * i.clip_h).max(min_thumb).min(i.clip_h);
        let max_scroll = (i.content_h - i.clip_h).max(0.0);
        let thumb_y = if max_scroll > 0.0 {
            i.clip_y + (i.scroll_y / max_scroll) * (i.clip_h - thumb_h)
        } else {
            i.clip_y
        };
        let thumb = Rect::new(
            track.x + inset,
            thumb_y.clamp(i.clip_y, i.clip_y + i.clip_h - thumb_h),
            g - inset * 2.0,
            thumb_h,
        );
        Some((track, thumb))
    } else {
        None
    };

    let h = if i.need_h && i.content_w > i.clip_w {
        let track = Rect::new(
            i.clip_x,
            i.clip_y + i.clip_h - g,
            i.clip_w,
            g,
        );
        let thumb_w = ((i.clip_w / i.content_w) * i.clip_w).max(min_thumb).min(i.clip_w);
        let max_scroll = (i.content_w - i.clip_w).max(0.0);
        let thumb_x = if max_scroll > 0.0 {
            i.clip_x + (i.scroll_x / max_scroll) * (i.clip_w - thumb_w)
        } else {
            i.clip_x
        };
        let thumb = Rect::new(
            thumb_x.clamp(i.clip_x, i.clip_x + i.clip_w - thumb_w),
            track.y + inset,
            thumb_w,
            g - inset * 2.0,
        );
        Some((track, thumb))
    } else {
        None
    };

    (v, h)
}

/// Emit `DrawScrollbar` track+thumb commands for a scroll container's padding box.
///
/// Shared by the legacy `walk` path and the ordered (stacking-context)
/// `box_layer_ops` path (BUG-220) so both render identical scrollbars. The
/// caller MUST emit these AFTER `PopScrollLayer`, so the bars stay at a fixed
/// position instead of translating with the scrolled content.
///
/// `padding_box` is `(px, py, pw, ph)` — padding-box origin and size in
/// document-space CSS px (border excluded). Content extent is measured relative
/// to the padding-box origin and floored at the padding-box size, so a border
/// does not inflate `content_w`/`content_h` past the clip and spawn a phantom
/// scrollbar.
///
/// No-op when `scrollbar-width: none` (gutter collapses to 0) — the container
/// still scrolls via keyboard/JS, only the visual bar is suppressed.
fn emit_scrollbars(
    b: &LayoutBox,
    padding_box: (f32, f32, f32, f32),
    is_scroll_x: bool,
    is_scroll_y: bool,
    out: &mut Vec<DisplayCommand>,
) {
    let (px, py, pw, ph) = padding_box;
    let gutter_px = match b.style.scrollbar_width {
        ScrollbarWidth::Auto => SCROLLBAR_WIDTH,
        ScrollbarWidth::Thin => SCROLLBAR_WIDTH_THIN,
        ScrollbarWidth::None => 0.0,
    };
    // Only emit when the scrollbar is visible (gutter_px > 0).
    if gutter_px <= 0.0 {
        return;
    }
    let (thumb_color, track_color) = match b.style.scrollbar_color {
        Some((thumb, track)) => (color_u8_to_f32(thumb), color_u8_to_f32(track)),
        None => (SCROLLBAR_THUMB_COLOR, SCROLLBAR_TRACK_COLOR),
    };
    // Content extent relative to padding-box origin, floored at padding-box size
    // (not border-box): a border must not make content_w exceed clip_w and fake
    // a horizontal scrollbar.
    let content_w = b
        .children
        .iter()
        .fold(pw, |acc, c| acc.max(c.rect.x + c.rect.width - px));
    let content_h = b
        .children
        .iter()
        .fold(ph, |acc, c| acc.max(c.rect.y + c.rect.height - py));
    let (v_bars, h_bars) = scrollbar_rects(&ScrollbarInput {
        clip_x: px,
        clip_y: py,
        clip_w: pw,
        clip_h: ph,
        scroll_x: b.scroll_x,
        scroll_y: b.scroll_y,
        content_w,
        content_h,
        need_v: is_scroll_y,
        need_h: is_scroll_x,
        gutter_px,
    });
    if let Some((track, thumb)) = v_bars {
        out.push(DisplayCommand::DrawScrollbar {
            track_rect: track,
            thumb_rect: thumb,
            vertical: true,
            thumb_color,
            track_color,
        });
    }
    if let Some((track, thumb)) = h_bars {
        out.push(DisplayCommand::DrawScrollbar {
            track_rect: track,
            thumb_rect: thumb,
            vertical: false,
            thumb_color,
            track_color,
        });
    }
}

/// Геометрия scroll-слоя overflow-контейнера — зеркало вычислений
/// `box_layer_ops`, которыми заполняются `PushScrollLayer` и `emit_scrollbars`
/// на ordered-пути. Дрейф с `box_layer_ops` ловят equivalence-тесты
/// `patch_scroll_layer_*` (патч против полной пересборки).
struct ScrollLayerGeometry {
    /// Значение `PushScrollLayer.clip_rect` (может содержать BIG-сентинели).
    clip_rect: Rect,
    /// Padding-box `(px, py, pw, ph)` — вход `emit_scrollbars`.
    padding_box: (f32, f32, f32, f32),
    /// `overflow-x` ∈ {scroll, auto}.
    is_scroll_x: bool,
    /// `overflow-y` ∈ {scroll, auto}.
    is_scroll_y: bool,
}

/// `None`, если бокс не открывает scroll-слой (не скроллится, `contain: paint`,
/// анонимный бокс).
fn scroll_layer_geometry(b: &LayoutBox) -> Option<ScrollLayerGeometry> {
    if !box_can_own_stacking_context(b) {
        return None;
    }
    let s = &b.style;
    let paint_contain = s.contain.0 & ContainFlags::PAINT.0 != 0;
    let clip_x = overflow_clips(s.overflow_x) || paint_contain;
    let clip_y = overflow_clips(s.overflow_y) || paint_contain;
    if !(clip_x || clip_y) {
        return None;
    }
    const BIG: f32 = 1_000_000.0;
    let px = b.rect.x + s.border_left_width;
    let py = b.rect.y + s.border_top_width;
    let pw = (b.rect.width - s.border_left_width - s.border_right_width).max(0.0);
    let ph = (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0);
    let cr = Rect::new(
        if clip_x { px } else { -BIG },
        if clip_y { py } else { -BIG },
        if clip_x { pw } else { 2.0 * BIG },
        if clip_y { ph } else { 2.0 * BIG },
    );
    let is_scroll_x = matches!(s.overflow_x, Overflow::Scroll | Overflow::Auto);
    let is_scroll_y = matches!(s.overflow_y, Overflow::Scroll | Overflow::Auto);
    if (is_scroll_x || is_scroll_y) && !paint_contain {
        Some(ScrollLayerGeometry {
            clip_rect: cr,
            padding_box: (px, py, pw, ph),
            is_scroll_x,
            is_scroll_y,
        })
    } else {
        None
    }
}

/// In-place патч скролл-позиции overflow-контейнера в готовом display list —
/// быстрый путь скролла без полной пересборки (`build_display_list_ordered`).
///
/// Полная пересборка после `lumen_layout::set_scroll_position` отличается от
/// старого списка ровно двумя вещами (layout детей не меняется — мутируются
/// только `scroll_x`/`scroll_y` контейнера): значениями скролла в
/// `PushScrollLayer` контейнера (включая BUG-159-переустановленные копии
/// вокруг дочерних stacking context'ов — у них тот же `clip_rect`) и
/// thumb-прямоугольниками его `DrawScrollbar`. Патч выполняет обе правки теми
/// же хелперами, что и построитель (`scroll_layer_geometry` /
/// `emit_scrollbars`), поэтому результат побайтно совпадает с пересборкой.
///
/// Возвращает `false`, если ожидания не сошлись (контейнер не найден,
/// найденные слои несут разные старые значения скролла, набор скроллбаров не
/// совпал по числу) — вызывающий обязан выполнить полную пересборку.
pub fn patch_scroll_layer(dl: &mut DisplayList, b: &LayoutBox) -> bool {
    let Some(g) = scroll_layer_geometry(b) else {
        return false;
    };
    let cr = g.clip_rect;
    let same_rect = |r: &Rect| {
        r.x.to_bits() == cr.x.to_bits()
            && r.y.to_bits() == cr.y.to_bits()
            && r.width.to_bits() == cr.width.to_bits()
            && r.height.to_bits() == cr.height.to_bits()
    };
    // Все PushScrollLayer контейнера: оригинал + переустановленные (BUG-159).
    // Они — клоны одной команды, поэтому старые значения скролла обязаны
    // совпадать; расхождение значит, что clip_rect делят разные контейнеры.
    let mut push_idxs: Vec<usize> = Vec::new();
    let mut old_scroll: Option<(u32, u32)> = None;
    for (i, cmd) in dl.iter().enumerate() {
        if let DisplayCommand::PushScrollLayer { clip_rect, scroll_x, scroll_y } = cmd
            && same_rect(clip_rect)
        {
            let sxy = (scroll_x.to_bits(), scroll_y.to_bits());
            match old_scroll {
                None => old_scroll = Some(sxy),
                Some(prev) if prev == sxy => {}
                Some(_) => return false,
            }
            push_idxs.push(i);
        }
    }
    let Some(&first_push) = push_idxs.first() else {
        return false;
    };
    // Балансирующий PopScrollLayer оригинального (первого) слоя.
    let mut depth = 0usize;
    let mut pop_idx = None;
    for (i, cmd) in dl.iter().enumerate().skip(first_push) {
        match cmd {
            DisplayCommand::PushScrollLayer { .. } => depth += 1,
            DisplayCommand::PopScrollLayer => {
                depth -= 1;
                if depth == 0 {
                    pop_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(pop_idx) = pop_idx else {
        return false;
    };
    // Скроллбары контейнера лежат подряд сразу после PopScrollLayer
    // (`box_layer_ops` кладёт их в overflow_post). Состав баров не зависит от
    // скролла (только от геометрии контента), поэтому пересобранный тем же
    // хелпером набор обязан совпасть по числу команд.
    let mut fresh: DisplayList = Vec::new();
    emit_scrollbars(b, g.padding_box, g.is_scroll_x, g.is_scroll_y, &mut fresh);
    let bars_start = pop_idx + 1;
    let mut bars_end = bars_start;
    while bars_end < dl.len() && matches!(dl[bars_end], DisplayCommand::DrawScrollbar { .. }) {
        bars_end += 1;
    }
    if bars_end - bars_start != fresh.len() {
        return false;
    }
    for (slot, new_cmd) in dl[bars_start..bars_end].iter_mut().zip(fresh) {
        *slot = new_cmd;
    }
    for &i in &push_idxs {
        if let DisplayCommand::PushScrollLayer { scroll_x, scroll_y, .. } = &mut dl[i] {
            *scroll_x = b.scroll_x;
            *scroll_y = b.scroll_y;
        }
    }
    true
}

fn emit_outline(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    if !s.outline_style.is_visible() || s.outline_width <= 0.0 {
        return;
    }
    let color = match s.outline_color {
        OutlineColor::Color(c) => c,
        OutlineColor::Auto | OutlineColor::CurrentColor => s.color,
    };
    out.push(DisplayCommand::DrawOutline {
        rect: b.rect,
        width: s.outline_width,
        style: s.outline_style,
        color,
        offset: s.outline_offset.px(),
    });
}

/// Рисует grip для resize property на overflow≠visible элементах.
/// 12px grip в углу как FillRoundedRect. // CSS: resize
fn emit_resize_grip(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;

    // resize свойство должно быть не None и overflow не Visible
    if s.resize == Resize::None {
        return;
    }

    // Проверяем, что overflow != Visible (есть прокрутка или обрезание)
    let overflow_x_hidden = matches!(s.overflow_x, Overflow::Hidden | Overflow::Clip | Overflow::Auto | Overflow::Scroll);
    let overflow_y_hidden = matches!(s.overflow_y, Overflow::Hidden | Overflow::Clip | Overflow::Auto | Overflow::Scroll);

    if !overflow_x_hidden && !overflow_y_hidden {
        return;
    }

    // 12px grip в углу (bottom-right по умолчанию)
    let grip_size = 12.0;
    let grip_x = b.rect.x + b.rect.width - grip_size;
    let grip_y = b.rect.y + b.rect.height - grip_size;

    // Рисуем grip как белый закруглённый квадрат (Phase 0)
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect { x: grip_x, y: grip_y, width: grip_size, height: grip_size },
        color: Color { r: 200, g: 200, b: 200, a: 255 },
        radii: CornerRadii { tl: 2.0, tl_y: 2.0, tr: 2.0, tr_y: 2.0, br: 2.0, br_y: 2.0, bl: 2.0, bl_y: 2.0 },
    });
}

/// Возвращает `true`, если точка (`px`, `py`) попадает в resize-grip элемента.
///
/// Grip — это 12×12 px область в правом нижнем углу `b.rect`. Присутствует
/// только когда `resize != None` и хотя бы одна ось `overflow` ≠ Visible.
pub fn point_on_resize_grip(b: &LayoutBox, px: f32, py: f32) -> bool {
    let s = &b.style;
    if s.resize == Resize::None {
        return false;
    }
    let overflow_hidden = matches!(s.overflow_x, Overflow::Hidden | Overflow::Clip | Overflow::Auto | Overflow::Scroll)
        || matches!(s.overflow_y, Overflow::Hidden | Overflow::Clip | Overflow::Auto | Overflow::Scroll);
    if !overflow_hidden {
        return false;
    }
    let grip_size = 12.0_f32;
    let grip_x = b.rect.x + b.rect.width - grip_size;
    let grip_y = b.rect.y + b.rect.height - grip_size;
    px >= grip_x && px < grip_x + grip_size && py >= grip_y && py < grip_y + grip_size
}

/// CSS Multi-column Layout L1 §3.3 — рисует разделители колонок
/// (`column-rule`) между каждой парой соседних колонок.
///
/// Разделитель центрируется в gap между колонками. Геометрия колонок
/// вычисляется заново по тем же формулам, что и в `lay_out_multicol_children`,
/// поскольку после layout она не сохраняется в LayoutBox.
///
/// Реализует только Solid / Dashed / Dotted через существующий `DrawBorder`
/// (правая сторона rect = rule rect); Double и прочие — как Solid (Phase 0).
/// Порядок рисования: после фона и бордера контейнера, перед children
/// (CSS Multi-column L1 §3.3: «above the border of the multi-column element»).
fn emit_column_rules(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    if s.column_count.is_none() && s.column_width.is_none() {
        return;
    }
    if !s.column_rule_style.is_visible() || s.column_rule_width <= 0.0 {
        return;
    }

    // Content box — mirrors lay_out_multicol_children content_x/y/w/h.
    let em = s.font_size;
    let content_x = b.rect.x + s.border_left_width + s.padding_left.px();
    let content_y = b.rect.y + s.border_top_width + s.padding_top.px();
    let content_w = (b.rect.width
        - s.border_left_width
        - s.border_right_width
        - s.padding_left.px()
        - s.padding_right.px())
    .max(0.0);
    let content_h = (b.rect.height
        - s.border_top_width
        - s.border_bottom_width
        - s.padding_top.px()
        - s.padding_bottom.px())
    .max(0.0);
    if content_w <= 0.0 || content_h <= 0.0 {
        return;
    }

    // Sentinel viewport for length resolution (good enough for px/em/%).
    let vp = Size::new(content_w, content_h);
    let col_gap = s.column_gap.resolve_or_zero(em, content_w, vp).max(0.0);

    // Mirror column count computation from lay_out_multicol_children.
    let n_cols: u32 = match (s.column_count, &s.column_width) {
        (Some(n), Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(content_w), vp)
                && w > 0.0
            {
                let n_from_w = ((content_w + col_gap) / (w + col_gap)).floor() as u32;
                n.min(n_from_w).max(1)
            } else {
                n.max(1)
            }
        }
        (Some(n), None) => n.max(1),
        (None, Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(content_w), vp)
                && w > 0.0
            {
                ((content_w + col_gap) / (w + col_gap)).floor() as u32
            } else {
                1
            }
        }
        (None, None) => 1,
    }
    .max(1);

    if n_cols <= 1 || col_gap <= 0.0 {
        return;
    }

    let col_w = ((content_w - col_gap * (n_cols - 1) as f32) / n_cols as f32).max(0.0);
    let rule_w = s.column_rule_width;
    let rule_color = s.column_rule_color.resolve(s.color);

    for i in 0..(n_cols - 1) {
        // Left edge of gap after column i.
        let gap_left = content_x + (i + 1) as f32 * col_w + i as f32 * col_gap;
        // Rule centered in the gap.
        let sep_x = gap_left + (col_gap - rule_w) * 0.5;

        // Reuse DrawBorder: emit as right-side only with rect.width = rule_w.
        // Renderer draws right side at: rect.x + rect.width - wr = sep_x ✓.
        out.push(DisplayCommand::DrawBorder {
            rect: Rect::new(sep_x, content_y, rule_w, content_h),
            widths: [0.0, rule_w, 0.0, 0.0],
            colors: [Color::TRANSPARENT, rule_color, Color::TRANSPARENT, Color::TRANSPARENT],
            styles: [
                BorderStyle::None,
                s.column_rule_style,
                BorderStyle::None,
                BorderStyle::None,
            ],
            radii: CornerRadii::default(),
        });
    }
}

/// CSS Display L3 §4 — `visibility: hidden` (и `collapse` для не-table
/// per spec) делает box-self **не-рисуемым** (background, border,
/// outline, box-shadow, content), но layout остаётся (`Skip` иной
/// семантики). Children по-прежнему обходятся: visibility наследуется,
/// но child может явно вернуть себя через `visibility: visible`.
pub(crate) fn is_paint_visible(b: &LayoutBox) -> bool {
    matches!(b.style.visibility, Visibility::Visible)
}

/// CSS Color L3 §3.2 — `opacity: 0` создаёт stacking context, и после
/// off-screen compositor pass весь subtree даёт fully-transparent
/// результат. Phase 0 без compositor-pass-ов: pure-pixel skip всего
/// subtree (children тоже не рисуются — это отличие от visibility:
/// hidden, где children могут override через `:visible`). Сравнение
/// `<= 0.0` страхует от sub-normal значений, попавших в opacity
/// через клипанг — layout cascade clamp-ит в `[0.0, 1.0]`, но
/// defensive check дешёвый. opacity > 0 && < 1 Phase 0 не обрабатывается
/// (требует off-screen pass с per-pixel alpha multiply — P2 п.4+).
pub(crate) fn is_opacity_subtree_painted(b: &LayoutBox) -> bool {
    b.style.opacity > 0.0
}

/// UA default accent for form controls when `accent-color: auto`. The same
/// blue previously hard-coded across checkbox / radio / range / progress.
const ACCENT_DEFAULT: Color = Color { r: 21, g: 90, b: 192, a: 255 };

/// Render checkbox checkmark or radio dot for checked form controls.
/// P2 note: this renders a simple filled rectangle as indicator; a full
/// vector checkmark / circle belongs to the renderer GPU primitive set.
/// HTML rendering §15.5 — default UA label for a button-type `<input>` that has
/// no `value` attribute. `submit`/`reset` have UA labels; a bare `button` has
/// none and renders empty.
fn default_button_label(input_type: &InputType) -> String {
    match input_type {
        InputType::Submit => "Submit".to_owned(),
        InputType::Reset => "Reset".to_owned(),
        _ => String::new(),
    }
}

/// HTML rendering §15.5.5 — paint the static `value` text of a form control
/// inside its content box.
///
/// `center` horizontally centers the text (button-like controls); otherwise the
/// text is left-aligned with a small inset (text fields). Password fields
/// (`input_type == Password`) mask each character with U+2022 BULLET. The text
/// is vertically centered within the content box and clipped to it so long
/// values do not overflow the border. The content box is the border box minus
/// the border widths; a fixed 2px inset approximates the native control padding.
fn emit_input_value_text(
    b: &LayoutBox,
    value: &str,
    input_type: &InputType,
    center: bool,
    out: &mut Vec<DisplayCommand>,
) {
    if value.is_empty() {
        return;
    }
    let s = &b.style;
    // Password masking: obscure each character (grapheme-approximate by char).
    let text = if *input_type == InputType::Password {
        "\u{2022}".repeat(value.chars().count())
    } else {
        value.to_owned()
    };

    let bl = s.border_left_width;
    let bt = s.border_top_width;
    let br = s.border_right_width;
    let bb = s.border_bottom_width;
    let inset = 2.0_f32;
    let content_x = b.rect.x + bl + inset;
    let content_y = b.rect.y + bt;
    let content_w = (b.rect.width - bl - br - inset * 2.0).max(1.0);
    let content_h = (b.rect.height - bt - bb).max(1.0);
    let font_size = s.font_size;

    // Horizontal placement. `draw_text` has no alignment, so a centered label
    // is positioned with the same per-glyph advance approximation used for SVG
    // text anchoring (a real TextMeasurer is not available in this crate).
    let text_x = if center {
        let approx_w = font_size * 0.5 * text.chars().count() as f32;
        content_x + ((content_w - approx_w) / 2.0).max(0.0)
    } else {
        content_x
    };
    // Vertical centering: `draw_text` places the glyph top at `y`, so offset by
    // half the leftover vertical space inside the content box.
    let text_y = content_y + ((content_h - font_size) / 2.0).max(0.0);

    // Clip to the content box so overflowing text stays inside the border.
    out.push(DisplayCommand::PushClipRect {
        rect: Rect::new(content_x, content_y, content_w, content_h),
    });
    out.push(DisplayCommand::DrawText {
        font_stretch: s.font_stretch,
        rect: Rect::new(text_x, text_y, content_w, font_size),
        text,
        font_size,
        color: s.color,
        font_family: s.font_family.clone(),
        font_weight: s.font_weight,
        font_style: s.font_style,
        font_variation_axes: vec![],
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        highlight_name: None,
        text_orientation: if s.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
            Some(s.text_orientation)
        } else {
            None
        },
    });
    out.push(DisplayCommand::PopClip);
}

/// Paint an empty text input's `placeholder` attribute as a grey hint
/// (HTML rendering §15.5.5). Left-aligned, vertically centered and clipped to
/// the content box, mirroring `emit_input_value_text` but with a fixed grey
/// colour (`#757575`, the UA default) and no password masking.
///
/// `placeholder_style` is the computed `input::placeholder` override (CSS
/// Pseudo-Elements L4 §4.10), when an author rule matched. Only `color`,
/// `opacity` (folded into the drawn color's alpha) and `font-*` are honoured —
/// the same restricted-subset approach as `::selection`.
fn emit_input_placeholder_text(
    b: &LayoutBox,
    placeholder: &str,
    placeholder_style: Option<&lumen_layout::style::ComputedStyle>,
    out: &mut Vec<DisplayCommand>,
) {
    if placeholder.is_empty() {
        return;
    }
    let s = &b.style;
    let bl = s.border_left_width;
    let bt = s.border_top_width;
    let br = s.border_right_width;
    let bb = s.border_bottom_width;
    let inset = 2.0_f32;
    let content_x = b.rect.x + bl + inset;
    let content_y = b.rect.y + bt;
    let content_w = (b.rect.width - bl - br - inset * 2.0).max(1.0);
    let content_h = (b.rect.height - bt - bb).max(1.0);
    let font_size = placeholder_style.map_or(s.font_size, |ps| ps.font_size);
    let text_y = content_y + ((content_h - font_size) / 2.0).max(0.0);

    let default_color = Color { r: 0x75, g: 0x75, b: 0x75, a: 255 };
    let color = match placeholder_style {
        Some(ps) => Color { a: (ps.color.a as f32 * ps.opacity).round() as u8, ..ps.color },
        None => default_color,
    };
    let (font_family, font_weight, font_style) = match placeholder_style {
        Some(ps) => (ps.font_family.clone(), ps.font_weight, ps.font_style),
        None => (s.font_family.clone(), s.font_weight, s.font_style),
    };

    out.push(DisplayCommand::PushClipRect {
        rect: Rect::new(content_x, content_y, content_w, content_h),
    });
    out.push(DisplayCommand::DrawText {
        font_stretch: s.font_stretch,
        rect: Rect::new(content_x, text_y, content_w, font_size),
        text: placeholder.to_owned(),
        font_size,
        color,
        font_family,
        font_weight,
        font_style,
        font_variation_axes: vec![],
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        highlight_name: None,
        text_orientation: if s.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
            Some(s.text_orientation)
        } else {
            None
        },
    });
    out.push(DisplayCommand::PopClip);
}

/// Build the white checkmark glyph for a checked checkbox as a triangle soup
/// (for [`DisplayCommand::DrawSvgPath`]). The tick is a two-segment thick
/// polyline (short stroke down to a vertex, long stroke up to the top-right),
/// positioned and scaled inside `fill` (the accent-filled control box).
fn checkmark_triangles(fill: Rect) -> Vec<[f32; 2]> {
    let sz = fill.width.min(fill.height);
    // Normalised tick anchor points (origin top-left, y downwards).
    let pt = |nx: f32, ny: f32| [fill.x + nx * fill.width, fill.y + ny * fill.height];
    let p0 = pt(0.22, 0.52);
    let p1 = pt(0.42, 0.72);
    let p2 = pt(0.78, 0.30);
    let half = (sz * 0.09).max(1.0);

    let mut v = Vec::with_capacity(12);
    push_thick_segment(&mut v, p0, p1, half);
    push_thick_segment(&mut v, p1, p2, half);
    v
}

/// Append the two triangles of a thick line segment from `a` to `b` with
/// half-width `half` to `out` (6 vertices). Used to draw the checkmark strokes.
fn push_thick_segment(out: &mut Vec<[f32; 2]>, a: [f32; 2], b: [f32; 2], half: f32) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
    // Perpendicular unit vector scaled by half-width.
    let nx = -dy / len * half;
    let ny = dx / len * half;
    let a1 = [a[0] + nx, a[1] + ny];
    let a2 = [a[0] - nx, a[1] - ny];
    let b1 = [b[0] + nx, b[1] + ny];
    let b2 = [b[0] - nx, b[1] - ny];
    out.extend_from_slice(&[a1, a2, b1, a2, b2, b1]);
}

fn emit_form_control_indicator(b: &LayoutBox, kind: &FormControlKind, out: &mut Vec<DisplayCommand>) {
    // CSS Basic UI L4 §4.2 — `appearance: none` (and the legacy `-webkit-`/
    // `-moz-` aliases, normalised to `Appearance::None` at parse time) removes
    // the native "primitive appearance" of a form control: the checkbox tick,
    // radio dot, range slider, progress bar, meter bar and select arrow. The box
    // (border/padding/background) is already stripped in
    // `strip_ua_appearance_box_styling` (before the author cascade); here we
    // suppress the painted indicator so authors can fully restyle it.
    // BUG-225: the suppression is scoped to the native primitives only (color
    // swatch, checkbox tick, radio dot, range slider, progress/meter bar, select
    // arrow). Text-input `value`/`placeholder` and button labels are author
    // content, not a UA primitive, so they keep rendering under `appearance:none`.
    let suppress_primitive = b.style.appearance == Appearance::None;
    // CSS UI L4 §6.1 — accent-color tints the "accent" of checkbox, radio,
    // range and progress controls. `auto` (None) keeps the UA default blue.
    // <meter> is intentionally excluded: its bar keeps the semantic
    // green/yellow/red coloring from HTML §4.10.14, not the accent color.
    let accent = b.style.accent_color.unwrap_or(ACCENT_DEFAULT);
    match kind {
        FormControlKind::Input { input_type, checked, value_text, placeholder, placeholder_style } => {
            // HTML §4.10.5.1.15 — a color input renders its value as a swatch
            // filling the content area, independent of any author `background`
            // (the native color widget ignores author bg). Default value is
            // `#000000`. Drawn before the `checked` gate since color is not
            // a checkable type.
            if *input_type == InputType::Color {
                // The swatch is the native primitive — suppressed under
                // `appearance:none` (the control has no text value to fall back
                // to, so nothing else is painted here).
                if !suppress_primitive {
                    let swatch = lumen_layout::style::parse_color(value_text)
                        .unwrap_or(Color { r: 0, g: 0, b: 0, a: 255 });
                    let bl = b.style.border_left_width;
                    let bt = b.style.border_top_width;
                    let br = b.style.border_right_width;
                    let bb = b.style.border_bottom_width;
                    let pad = 2.0;
                    out.push(DisplayCommand::FillRect {
                        rect: Rect::new(
                            b.rect.x + bl + pad,
                            b.rect.y + bt + pad,
                            (b.rect.width  - bl - br - pad * 2.0).max(1.0),
                            (b.rect.height - bt - bb - pad * 2.0).max(1.0),
                        ),
                        color: swatch,
                    });
                }
                return;
            }
            // HTML rendering §15.5.5 — text-like inputs paint their `value` as
            // static content (left-aligned, vertically centered, clipped to the
            // content box); button-like inputs (submit/reset/button) paint the
            // `value` as a centered label. Checkable types (checkbox/radio) fall
            // through to the dot/tick indicator below; `range`/`color`/`file`/
            // `hidden`/`image` never render a text value here.
            match input_type {
                InputType::Text | InputType::Email | InputType::Password
                | InputType::Tel | InputType::Url | InputType::Number
                | InputType::Search | InputType::Date | InputType::DateTimeLocal
                | InputType::Time | InputType::Month | InputType::Week => {
                    if value_text.is_empty() && !placeholder.is_empty() {
                        // HTML rendering §15.5.5 — an empty text input paints its
                        // `placeholder` as a grey hint (never masked, even for
                        // password). Drawn left-aligned, vertically centered and
                        // clipped to the content box, like the value text.
                        emit_input_placeholder_text(b, placeholder, placeholder_style.as_deref(), out);
                    } else {
                        emit_input_value_text(b, value_text, input_type, false, out);
                    }
                    return;
                }
                InputType::Submit | InputType::Reset | InputType::Button => {
                    let label = if value_text.is_empty() {
                        default_button_label(input_type)
                    } else {
                        value_text.clone()
                    };
                    emit_input_value_text(b, &label, input_type, true, out);
                    return;
                }
                _ => {}
            }
            // The checked checkbox tick / radio dot is a native primitive —
            // suppressed under `appearance:none`.
            if suppress_primitive { return; }
            if !checked { return; }
            if *input_type != InputType::Checkbox && *input_type != InputType::Radio {
                return;
            }
            // Native checked checkbox/radio (Chromium/Edge default appearance):
            // the whole control fills with accent colour — overriding any author
            // `background` — and a white glyph is drawn on top: a tick for the
            // checkbox, a centre dot for the radio.
            let bl = b.style.border_left_width;
            let bt = b.style.border_top_width;
            let br = b.style.border_right_width;
            let bb = b.style.border_bottom_width;
            let fill = Rect::new(
                b.rect.x + bl,
                b.rect.y + bt,
                (b.rect.width  - bl - br).max(1.0),
                (b.rect.height - bt - bb).max(1.0),
            );
            let white = Color { r: 255, g: 255, b: 255, a: 255 };
            match input_type {
                InputType::Radio => {
                    // Solid accent disc filling the control, then a small white
                    // centre dot (radius ≈ 0.22 of the box) — the native look.
                    let r = fill.width.min(fill.height) / 2.0;
                    out.push(DisplayCommand::FillRoundedRect {
                        rect: fill,
                        radii: crate::CornerRadii { tl: r, tr: r, br: r, bl: r, ..Default::default() },
                        color: accent,
                    });
                    let dot_d = (fill.width.min(fill.height) * 0.44).max(2.0);
                    let dot = Rect::new(
                        fill.x + (fill.width  - dot_d) / 2.0,
                        fill.y + (fill.height - dot_d) / 2.0,
                        dot_d,
                        dot_d,
                    );
                    let dr = dot_d / 2.0;
                    out.push(DisplayCommand::FillRoundedRect {
                        rect: dot,
                        radii: crate::CornerRadii { tl: dr, tr: dr, br: dr, bl: dr, ..Default::default() },
                        color: white,
                    });
                }
                _ => {
                    out.push(DisplayCommand::FillRect { rect: fill, color: accent });
                    out.push(DisplayCommand::DrawSvgPath {
                        vertices: checkmark_triangles(fill),
                        color: white,
                    });
                }
            }
        }
        FormControlKind::Select { selected_text } => {
            // The select arrow is the native primitive; the selected option text
            // is author-visible content and keeps rendering. `emit_select_indicator`
            // draws both, so pass the suppression flag down rather than gating here.
            emit_select_indicator(b, selected_text, suppress_primitive, out);
        }
        FormControlKind::Button | FormControlKind::Textarea { .. } => {}
        FormControlKind::Range { value, min, max } => {
            if !suppress_primitive {
                emit_range_slider(b, *value, *min, *max, accent, out);
            }
        }
        FormControlKind::Progress { value, max } => {
            if !suppress_primitive {
                emit_progress_bar(b, *value, *max, accent, out);
            }
        }
        FormControlKind::Meter { value, min, max, low, high, optimum } => {
            if !suppress_primitive {
                emit_meter_bar(b, *value, *min, *max, *low, *high, *optimum, out);
            }
        }
    }
}

/// Draw a range slider: gray track, accent-colored filled portion, circular thumb.
///
/// `accent` is the resolved `accent-color` (UA default blue when `auto`); it
/// tints both the filled track portion and the thumb per CSS UI L4 §6.1.
fn emit_range_slider(b: &LayoutBox, value: f32, min: f32, max: f32, accent: Color, out: &mut Vec<DisplayCommand>) {
    let range = (max - min).max(f32::EPSILON);
    let fraction = ((value - min) / range).clamp(0.0, 1.0);

    let track_h = 4.0_f32;
    let thumb_r = 8.0_f32; // thumb diameter
    let track_y = b.rect.y + (b.rect.height - track_h) / 2.0;
    let track_x = b.rect.x + thumb_r / 2.0;
    let track_w = (b.rect.width - thumb_r).max(1.0);

    let gray = Color { r: 200, g: 200, b: 200, a: 255 };
    let blue = accent;
    let track_radius = crate::CornerRadii { tl: 2.0, tr: 2.0, br: 2.0, bl: 2.0, ..Default::default() };

    // Gray background track.
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(track_x, track_y, track_w, track_h),
        radii: track_radius,
        color: gray,
    });

    // Blue filled portion (left of thumb).
    let fill_w = (track_w * fraction).max(0.0);
    if fill_w > 0.0 {
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(track_x, track_y, fill_w, track_h),
            radii: track_radius,
            color: blue,
        });
    }

    // Circular thumb.
    let thumb_cx = track_x + track_w * fraction;
    let thumb_y = b.rect.y + (b.rect.height - thumb_r) / 2.0;
    let hr = thumb_r / 2.0;
    let thumb_radii = crate::CornerRadii { tl: hr, tr: hr, br: hr, bl: hr, ..Default::default() };
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(thumb_cx - thumb_r / 2.0, thumb_y, thumb_r, thumb_r),
        radii: thumb_radii,
        color: blue,
    });
}

/// Draw a `<progress>` bar inside the border box.
///
/// Determinate: `accent`-colored fill proportional to `value / max`.
/// Indeterminate (`value` is `None`): static 30% fill to indicate pending state.
/// `accent` is the resolved `accent-color` (UA default blue when `auto`).
fn emit_progress_bar(b: &LayoutBox, value: Option<f32>, max: f32, accent: Color, out: &mut Vec<DisplayCommand>) {
    let pad = 2.0_f32;
    let bar_x = b.rect.x + pad;
    let bar_y = b.rect.y + pad;
    let bar_max_w = (b.rect.width - pad * 2.0).max(0.0);
    let bar_h = (b.rect.height - pad * 2.0).max(1.0);
    let blue = accent;
    let radii = crate::CornerRadii { tl: 2.0, tr: 2.0, br: 2.0, bl: 2.0, ..Default::default() };

    let fraction = match value {
        None => 0.3,
        Some(v) => (v / max.max(f32::EPSILON)).clamp(0.0, 1.0),
    };

    let fill_w = (bar_max_w * fraction).max(0.0);
    if fill_w > 0.0 {
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(bar_x, bar_y, fill_w, bar_h),
            radii,
            color: blue,
        });
    }
}

/// Draw a `<meter>` gauge bar inside the border box (HTML5 §4.10.14).
///
/// Fill color: green = optimal zone, yellow = sub-optimal, red = bad.
#[allow(clippy::too_many_arguments)]
fn emit_meter_bar(
    b: &LayoutBox,
    value: f32,
    min: f32,
    max: f32,
    low: f32,
    high: f32,
    optimum: f32,
    out: &mut Vec<DisplayCommand>,
) {
    let range = (max - min).max(f32::EPSILON);
    let fraction = ((value - min) / range).clamp(0.0, 1.0);

    let pad = 2.0_f32;
    let bar_x = b.rect.x + pad;
    let bar_y = b.rect.y + pad;
    let bar_max_w = (b.rect.width - pad * 2.0).max(0.0);
    let bar_h = (b.rect.height - pad * 2.0).max(1.0);
    let radii = crate::CornerRadii { tl: 2.0, tr: 2.0, br: 2.0, bl: 2.0, ..Default::default() };

    let fill_color = meter_gauge_color(value, min, max, low, high, optimum);
    let fill_w = (bar_max_w * fraction).max(0.0);
    if fill_w > 0.0 {
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(bar_x, bar_y, fill_w, bar_h),
            radii,
            color: fill_color,
        });
    }
}

/// HTML5 §4.10.14 — determine meter gauge fill color from value and thresholds.
///
/// Optimum zone → green, adjacent zone → yellow, far zone → red.
pub(crate) fn meter_gauge_color(value: f32, _min: f32, _max: f32, low: f32, high: f32, optimum: f32) -> Color {
    let green  = Color { r: 100, g: 180, b:  60, a: 255 };
    let yellow = Color { r: 210, g: 175, b:  20, a: 255 };
    let red    = Color { r: 200, g:  60, b:  60, a: 255 };

    // Where does optimum fall?
    let opt_in_low    = optimum <= low;
    let opt_in_high   = optimum >= high;
    let opt_in_middle = !opt_in_low && !opt_in_high;

    let val_in_low    = value < low;
    let val_in_high   = value > high;
    let val_in_middle = !val_in_low && !val_in_high;

    if opt_in_middle {
        if val_in_middle { green } else { yellow }
    } else if opt_in_low {
        if val_in_low { green } else if val_in_middle { yellow } else { red }
    } else {
        // opt_in_high
        if val_in_high { green } else if val_in_middle { yellow } else { red }
    }
}

/// Draw the selected option label and a dropdown arrow (▼) inside a `<select>` box.
///
/// `suppress_primitive` (set by `appearance: none`, BUG-225) drops the native
/// separator line and dropdown arrow; the selected option label is author-visible
/// content and is always painted.
fn emit_select_indicator(b: &LayoutBox, selected_text: &str, suppress_primitive: bool, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    let fg = s.color;
    let font_size = s.font_size.clamp(10.0, 14.0);
    let pad = 4.0;
    // Arrow column width (enough for "▼" glyph). When the native arrow is
    // suppressed the label reclaims that column.
    let arrow_w = font_size + pad * 2.0;
    let reserved = if suppress_primitive { 0.0 } else { arrow_w };
    let text_w = (b.rect.width - reserved - pad * 2.0).max(1.0);

    // Selected label — clipped to available width.
    if !selected_text.is_empty() {
        out.push(DisplayCommand::DrawText {
            font_stretch: s.font_stretch,
            rect: Rect::new(b.rect.x + pad, b.rect.y + pad, text_w, b.rect.height - pad * 2.0),
            text: selected_text.to_owned(),
            font_size,
            color: fg,
            font_family: s.font_family.clone(),
            font_weight: s.font_weight,
            font_style: s.font_style,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: if s.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
                Some(s.text_orientation)
            } else {
                None
            },
        });
    }

    // Native separator line + dropdown arrow — suppressed under `appearance:none`.
    if !suppress_primitive {
        // Separator line before the arrow.
        let sep_x = b.rect.x + b.rect.width - arrow_w;
        out.push(DisplayCommand::DrawBorder {
            rect: Rect::new(sep_x, b.rect.y, 1.0, b.rect.height),
            widths: [0.0, 0.0, 0.0, 1.0],
            colors: [fg; 4],
            styles: [lumen_layout::BorderStyle::Solid; 4],
            radii: crate::CornerRadii::default(),
        });

        // Dropdown arrow "▼".
        out.push(DisplayCommand::DrawText {
            font_stretch: s.font_stretch,
            rect: Rect::new(sep_x + pad, b.rect.y + pad, arrow_w - pad, b.rect.height - pad * 2.0),
            text: "\u{25BC}".to_owned(),
            font_size: font_size * 0.75,
            color: fg,
            font_family: s.font_family.clone(),
            font_weight: s.font_weight,
            font_style: s.font_style,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: if s.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
                Some(s.text_orientation)
            } else {
                None
            },
        });
    }
}

/// CSS Lists L3 §2.1 — renders the `::marker` pseudo-element.
/// Bullet types (disc/circle/square) are drawn as geometric shapes to avoid
/// relying on specific Unicode glyphs in the bundled font.
/// Counter types (decimal/roman/alpha/greek) are rendered as text.
fn emit_list_marker(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let BoxKind::Marker { ref text, ref list_style_type, ref image, .. } = b.kind else { return };
    if !is_paint_visible(b) {
        return;
    }
    let s = &b.style;
    // CSS Lists L3 §2.3 — `list-style-image` takes precedence over the marker
    // type/text: the bullet is replaced by the image. Drawn `contain`-fitted
    // inside the marker box; if the URL is not registered the DrawImage is a no-op.
    if let Some(src) = image
        && !src.is_empty()
    {
        out.push(DisplayCommand::DrawImage {
            rect: b.rect,
            src: src.clone(),
            alt: String::new(),
            object_fit: ObjectFit::Contain,
            object_position: ObjectPosition::default(),
            image_rendering: s.image_rendering,
        });
        return;
    }
    let color = s.color;
    let em = s.font_size;
    let cx = b.rect.x + b.rect.width * 0.5;
    let cy = b.rect.y + b.rect.height * 0.5;
    // CSS Lists L3 §2.1 / Pseudo-elements L4 §14.2 — a non-empty `text` means the
    // marker carries a string: either a counter glyph (decimal/roman/alpha) or an
    // explicit `::marker { content: … }` override. In both cases the string wins over
    // the bullet glyph, so the disc/circle/square shapes only draw when `text` is empty
    // (otherwise a `list-style-type: disc` list with `::marker { content: "→ " }` would
    // paint the disc instead of the arrow — BUG-185).
    match list_style_type {
        ListStyleType::Disc if text.is_empty() => {
            // Filled circle ~0.4em in diameter, centered in marker rect.
            let d = em * 0.40;
            let r = d * 0.5;
            let rect = Rect::new(cx - r, cy - r, d, d);
            let radii = CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r };
            out.push(DisplayCommand::FillRoundedRect { rect, color, radii });
        }
        ListStyleType::Circle if text.is_empty() => {
            // Hollow circle ~0.4em in diameter, border ~0.08em thick.
            let d = em * 0.40;
            let r = d * 0.5;
            let bw = (em * 0.08).max(1.0);
            let rect = Rect::new(cx - r, cy - r, d, d);
            let radii = CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r };
            out.push(DisplayCommand::DrawBorder {
                rect,
                widths: [bw; 4],
                colors: [color; 4],
                styles: [BorderStyle::Solid; 4],
                radii,
            });
        }
        ListStyleType::Square if text.is_empty() => {
            // Filled square ~0.35em side, centered in marker rect.
            let d = em * 0.35;
            let rect = Rect::new(cx - d * 0.5, cy - d * 0.5, d, d);
            out.push(DisplayCommand::FillRect { rect, color });
        }
        _ => {
            // Counter types (decimal, roman, alpha, greek) and `::marker { content }`
            // overrides — render the string.
            if !text.is_empty() {
                out.push(DisplayCommand::DrawText {
                    font_stretch: s.font_stretch,
                    rect: b.rect,
                    text: text.clone(),
                    font_size: em,
                    color,
                    font_family: s.font_family.clone(),
                    font_weight: s.font_weight,
                    font_style: s.font_style,
                    font_features: lumen_layout::style::text_font_features(s),
                    font_palette: palette_selection(s),
                    font_variation_axes: {
                        let mut axes: Vec<([u8; 4], f32)> = s.font_variation_settings
                            .iter().map(|a| (a.tag, a.value)).collect();
                        if s.font_optical_sizing == FontOpticalSizing::Auto
                            && !axes.iter().any(|(t, _)| t == b"opsz")
                        {
                            axes.push((*b"opsz", em));
                        }
                        if s.font_stretch != FontStretch::NORMAL
                            && !axes.iter().any(|(t, _)| t == b"wdth")
                        {
                            axes.push((*b"wdth", s.font_stretch.0 as f32 / 10.0));
                        }
                        axes
                     },
                     tab_size: 0.0,
                     highlight_name: None,
                     text_orientation: if s.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
                         Some(s.text_orientation)
                     } else {
                         None
                     },
                 });
             }
         }
     }
}

/// CSS Tables L2 §17.6.1.1 — true when `b` is a table cell that must suppress its
/// borders and background under `empty-cells: hide`. Applies only in the separated-
/// borders model (`border-collapse: separate`) and only when the cell has no in-flow
/// content. Under `border-collapse: collapse` the property has no effect.
pub(crate) fn is_hidden_empty_cell(b: &LayoutBox) -> bool {
    b.style.display == Display::TableCell
        && b.style.empty_cells == EmptyCells::Hide
        && b.style.border_collapse == BorderCollapse::Separate
        && !table_cell_has_content(b)
}

/// True when a table cell has in-flow content: any descendant box that generates
/// text, a replaced element, or a block. Whitespace-only inline runs and `Skip`
/// boxes do not count (CSS Tables L2 §17.6.1.1 "empty" definition).
fn table_cell_has_content(b: &LayoutBox) -> bool {
    b.children.iter().any(box_generates_content)
}

/// Whether a single child box contributes in-flow content for the empty-cell test.
fn box_generates_content(c: &LayoutBox) -> bool {
    match &c.kind {
        BoxKind::Skip => false,
        BoxKind::InlineRun { lines, .. } => lines
            .iter()
            .any(|line| line.iter().any(|f| f.img_src.is_some() || !f.text.trim().is_empty())),
        _ => true,
    }
}

/// Эмитит DisplayCommand-ы для одного box-а БЕЗ рекурсии в детей. Аналог
/// тела `walk` для одного box-а.
fn emit_box_self(
    b: &LayoutBox,
    out: &mut Vec<DisplayCommand>,
    dpr: f32,
    sel: Option<&SelectionHighlight>,
    ov: Option<&CompositorOverride>,
) {
    // opacity:0 → whole-subtree invisible (см. is_opacity_subtree_painted).
    // emit_box_self не идёт в children, но self-content тоже skip-аем.
    if !is_opacity_subtree_painted(b) {
        return;
    }
    // BUG-231: remember where this box's own commands start so an animated
    // background-color / color compositor override can be patched into them
    // afterwards (see `apply_color_override`) without relayout.
    let cmd_start = out.len();
    match &b.kind {
        BoxKind::Skip => {}
        BoxKind::Block | BoxKind::FlowRoot | BoxKind::TableRow
        | BoxKind::Table | BoxKind::TableRowGroup => {
            if !is_paint_visible(b) {
                return;
            }
            // CSS Tables L2 §17.6.1.1 — `empty-cells: hide`: an empty cell draws
            // neither borders nor background. Cell has no children to recurse into,
            // so skipping self-emission fully hides it.
            if is_hidden_empty_cell(b) {
                return;
            }
            emit_box_shadows(b, out);
            let s = &b.style;
            let radii = CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height);
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    if radii.all_zero() {
                        out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                    } else {
                        out.push(DisplayCommand::FillRoundedRect { rect: clip, color: bg, radii });
                    }
                }
            }
            emit_background_image(out, b, dpr);
            emit_inset_box_shadows(b, out);
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width,
                        s.border_right_width,
                        s.border_bottom_width,
                        s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style,
                        s.border_right_style,
                        s.border_bottom_style,
                        s.border_left_style,
                    ],
                    radii,
                });
            }
            emit_column_rules(b, out);
            emit_outline(b, out);
        }
        BoxKind::InlineRun { lines, .. } => {
            emit_inline_run(b, lines, sel, dpr, out);
        }
        BoxKind::InlineBlockRow | BoxKind::InlineSpace | BoxKind::Contents => {}
        BoxKind::Marker { .. } => {
            emit_list_marker(b, out);
        }
        BoxKind::FormControl { kind } => {
            if !is_paint_visible(b) {
                return;
            }
            emit_box_shadows(b, out);
            let s = &b.style;
            let radii = CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height);
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    if radii.all_zero() {
                        out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                    } else {
                        out.push(DisplayCommand::FillRoundedRect { rect: clip, color: bg, radii });
                    }
                }
            }
            emit_background_image(out, b, dpr);
            emit_inset_box_shadows(b, out);
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width,
                        s.border_right_width,
                        s.border_bottom_width,
                        s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style,
                        s.border_right_style,
                        s.border_bottom_style,
                        s.border_left_style,
                    ],
                    radii,
                });
            }
            emit_outline(b, out);
            emit_form_control_indicator(b, kind, out);
        }
        BoxKind::Image { src, alt, is_lazy } => {
            if !is_paint_visible(b) {
                return;
            }
            emit_box_shadows(b, out);
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b, dpr);
            emit_inset_box_shadows(b, out);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width,
                        s.border_right_width,
                        s.border_bottom_width,
                        s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style,
                        s.border_right_style,
                        s.border_bottom_style,
                        s.border_left_style,
                    ],
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // BUG-431: bitmap belongs in the content box, same rule as <canvas>
            // (BUG-099) — painting at the border box slides it under the border.
            if *is_lazy {
                out.push(DisplayCommand::LazyImageSlot {
                    rect: content_box_rect(b),
                    node_id: b.node.index() as u32,
                    src: src.clone(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                });
            } else {
                out.push(DisplayCommand::DrawImage {
                    rect: content_box_rect(b),
                    src: src.clone(),
                    alt: alt.clone(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                    image_rendering: b.style.image_rendering,
                });
            }
            emit_outline(b, out);
        }
        BoxKind::Video { src, poster } => {
            if !is_paint_visible(b) {
                return;
            }
            emit_box_shadows(b, out);
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b, dpr);
            emit_inset_box_shadows(b, out);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width,
                        s.border_right_width,
                        s.border_bottom_width,
                        s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style,
                        s.border_right_style,
                        s.border_bottom_style,
                        s.border_left_style,
                    ],
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // Phase 1: GIF-backed <video> — frame uploaded by shell under "video:{nid}".
            // Non-GIF src or no src: fall back to poster image (Phase 0 behaviour).
            // The shell's tick loop re-registers the current GIF frame under this key
            // on every render tick, so the DrawImage command always shows the live frame.
            // CSS: object-fit — P4 wires ComputedStyle.object_fit to scale the frame.
            // BUG-431: destination is the content box, not the border box.
            let nid = b.node.index();
            let is_gif_src = src.to_ascii_lowercase().ends_with(".gif") && !src.is_empty();
            if is_gif_src {
                out.push(DisplayCommand::DrawImage {
                    rect: content_box_rect(b),
                    src: format!("video:{nid}"),
                    alt: String::new(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                    image_rendering: b.style.image_rendering,
                });
            } else if !poster.is_empty() {
                out.push(DisplayCommand::DrawImage {
                    rect: content_box_rect(b),
                    src: poster.clone(),
                    alt: String::new(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                    image_rendering: b.style.image_rendering,
                });
            }
            emit_outline(b, out);
        }
        BoxKind::Canvas { .. } => {
            // HTML LS §4.12.4: <canvas> is a replaced element. Painter's order:
            // box-shadows → background → bg-image → border → bitmap → outline.
            if !is_paint_visible(b) {
                return;
            }
            emit_box_shadows(b, out);
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b, dpr);
            emit_inset_box_shadows(b, out);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width,
                        s.border_right_width,
                        s.border_bottom_width,
                        s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style,
                        s.border_right_style,
                        s.border_bottom_style,
                        s.border_left_style,
                    ],
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // Bitmap is uploaded by the shell under `canvas:{node_id}`. Until JS
            // draws anything the key is unregistered → transparent placeholder.
            // BUG-099: the bitmap belongs in the *content* box — painting it at
            // `b.rect` slid it under the border by the border width.
            let nid = b.node.index();
            out.push(DisplayCommand::DrawImage {
                rect: content_box_rect(b),
                src: format!("canvas:{nid}"),
                alt: String::new(),
                object_fit: ObjectFit::Fill,
                object_position: b.style.object_position,
                image_rendering: b.style.image_rendering,
            });
            emit_outline(b, out);
        }
        BoxKind::Audio { controls, .. } => {
            if !is_paint_visible(b) || !controls || b.rect.width <= 0.0 || b.rect.height <= 0.0 {
                return;
            }
            // Phase 0: render a grey bar representing the audio controls UI.
            let grey = Color { r: 200, g: 200, b: 200, a: 255 };
            out.push(DisplayCommand::FillRect { rect: b.rect, color: grey });
            emit_outline(b, out);
        }
        BoxKind::Iframe { src, .. } => {
            if !is_paint_visible(b) || b.rect.width <= 0.0 || b.rect.height <= 0.0 {
                return;
            }
            emit_box_shadows(b, out);
            // Phase 0: grey placeholder — no sub-document navigation.
            // Using DrawImage with src as key: unregistered key → grey placeholder
            // (same pattern as Video). The src string identifies this iframe to
            // the shell for potential future navigation.
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width,
                        s.border_right_width,
                        s.border_bottom_width,
                        s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style,
                        s.border_right_style,
                        s.border_bottom_style,
                        s.border_left_style,
                    ],
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // BUG-431: destination is the content box, not the border box.
            out.push(DisplayCommand::DrawImage {
                rect: content_box_rect(b),
                src: src.clone(),
                alt: String::new(),
                object_fit: b.style.object_fit,
                object_position: b.style.object_position,
                image_rendering: b.style.image_rendering,
            });
            emit_outline(b, out);
        }
        // SVG elements: in the ordered (stacking-context) path `fill_buckets`
        // already recurses into children, so each box paints only its own
        // content here — no child recursion, unlike `walk` (which descends
        // SvgRoot's shape/text children itself).
        BoxKind::SvgRoot { .. } => {
            if is_paint_visible(b)
                && let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                out.push(DisplayCommand::FillRect { rect: b.rect, color: bg });
            }
        }
        BoxKind::SvgShape { shape, .. } => {
            emit_svg_shape(b, shape, out);
        }
        BoxKind::SvgText { text, text_anchor, dominant_baseline, baseline_shift, .. } => {
            emit_svg_text(b, text, *text_anchor, *dominant_baseline, *baseline_shift, out);
        }
    }
    // BUG-231: apply animated background-color / color compositor override to the
    // commands this box just emitted (range `cmd_start..`), before the resize grip.
    if let Some(ov) = ov
        && (ov.background_color.is_some() || ov.color.is_some())
    {
        apply_color_override(b, ov, &mut out[cmd_start..]);
    }
    emit_resize_grip(b, out);
}

/// BUG-231: patch a box's own background fill and currentColor-derived border /
/// outline colours with the compositor override `ov`, in place, without relayout.
///
/// The background fill is identified by its exact clip rect: drop-shadow fills use
/// a different (offset/spread-expanded) rect, so they are left untouched. Borders
/// and outline are re-resolved from the box style against the overridden
/// currentColor. Only fills already present are patched — a transition starting
/// from a transparent background still needs relayout to inject a fill.
fn apply_color_override(b: &LayoutBox, ov: &CompositorOverride, cmds: &mut [DisplayCommand]) {
    if let Some(bg) = ov.background_color {
        let clip = background_clip_rect(b, background_color_clip(b));
        for c in cmds.iter_mut() {
            match c {
                DisplayCommand::FillRect { rect, color } if *rect == clip => *color = bg,
                DisplayCommand::FillRoundedRect { rect, color, .. } if *rect == clip => *color = bg,
                _ => {}
            }
        }
    }
    if let Some(cur) = ov.color {
        let s = &b.style;
        let outline_uses_current = matches!(
            s.outline_color,
            OutlineColor::Auto | OutlineColor::CurrentColor
        );
        for c in cmds.iter_mut() {
            match c {
                DisplayCommand::DrawBorder { colors, .. } => {
                    *colors = [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ];
                }
                DisplayCommand::DrawOutline { color, .. } if outline_uses_current => *color = cur,
                _ => {}
            }
        }
    }
}

/// CSS Transforms L2 §6.1 — does this box establish a **3D rendering context**
/// for its children? When `true`, the children share one 3D coordinate space
/// and are painted in depth order (see [`depth_sorted_child_order`]) instead of
/// being flattened to z=0 individually and painted in document order.
///
/// A box establishes a 3D rendering context iff `transform-style: preserve-3d`.
fn establishes_3d_rendering_context(b: &LayoutBox) -> bool {
    b.style.transform_style == TransformStyle::Preserve3d
}

/// CSS Transforms L2 §5.1 — `backface-visibility: hidden` culls a box once
/// its own 3D transform has rotated its face past 90° from the viewer.
///
/// The box's face normal in its own coordinate space is `(0, 0, 1)`; the
/// linear part of `forward_box_transform` maps it to `(m[8], m[9], m[10])`
/// (translation columns don't affect direction vectors), so `m[10]` alone —
/// the same raw z used by [`child_z_depth`]'s `transform_z` — tells which way
/// the face points: negative means it has flipped into the screen.
fn is_backface_hidden(b: &LayoutBox) -> bool {
    b.style.backface_visibility == BackfaceVisibility::Hidden
        && matches!(forward_box_transform(b), Some(m) if m.0[10] < 0.0)
}

/// Transformed depth of a box's center within its parent's 3D rendering
/// context. Applies the box's own forward transform (`forward_box_transform`,
/// which includes `transform-origin` pivot) to the box-center at z=0 and takes
/// the **raw** transformed z (`Mat4::transform_z`, no perspective divide — see
/// its doc for why). Boxes without a transform sit at z=0. Larger z = nearer
/// the viewer (CSS convention).
fn child_z_depth(b: &LayoutBox) -> f32 {
    match forward_box_transform(b) {
        Some(m) => {
            let cx = b.rect.x + b.rect.width * 0.5;
            let cy = b.rect.y + b.rect.height * 0.5;
            m.transform_z(cx, cy, 0.0)
        }
        None => 0.0,
    }
}

/// CSS Transforms L2 §6.2 — painting order inside a 3D rendering context.
///
/// Returns indices into `children` ordered **back-to-front**: the child with
/// the smallest transformed z ([`child_z_depth`]) is painted first (farthest
/// from the viewer), the largest z last (nearest, so it correctly occludes the
/// others). The sort is **stable** — children at equal depth keep document
/// order, preserving the normal stacking rule for coplanar siblings.
///
/// This is the painter's-algorithm depth sort. Pixel-exact handling of mutually
/// *intersecting* planes (BSP / plane splitting) is a future extension; for the
/// common case of non-intersecting transformed planes this yields correct
/// occlusion. A GPU depth buffer is the alternative; see STATUS-P2.
fn depth_sorted_child_order(children: &[LayoutBox]) -> Vec<usize> {
    let z: Vec<f32> = children.iter().map(child_z_depth).collect();
    depth_order_by_z(&z)
}

/// Pure back-to-front ordering of indices `0..z.len()` by depth `z[i]`.
/// Smallest z first (farthest), largest last (nearest). Stable: equal depths
/// keep their original order. `NaN` depths compare as equal (treated as
/// coplanar) so a degenerate transform never panics or reorders unpredictably.
/// Split out from [`depth_sorted_child_order`] so the ordering logic is unit-
/// testable without constructing a layout tree.
fn depth_order_by_z(z: &[f32]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..z.len()).collect();
    // `sort_by` is stable: coplanar siblings retain document order.
    order.sort_by(|&a, &b| z[a].partial_cmp(&z[b]).unwrap_or(std::cmp::Ordering::Equal));
    order
}

/// Collects `GapSegment`s for `gap-rule-*` rendering in flex/grid containers.
///
/// Scans child box right-edges and top-edges against the container's `column_gap`
/// and `row_gap` values; emits one `GapSegment` per actual gap found. Works for
/// both single-line and multi-line flex, and for grid containers.
///
/// Returns an empty `Vec` when the container is not flex/grid, or when both gap
/// values are zero, or when `gap_rule_style` is `None` / `gap_rule_width` ≤ 0.
fn collect_gap_segments(b: &LayoutBox) -> Vec<GapSegment> {
    let s = &b.style;
    // Only flex/grid containers produce gap rules.
    let is_flex_or_grid = matches!(
        s.display,
        Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
    );
    if !is_flex_or_grid {
        return Vec::new();
    }
    if !s.gap_rule_style.is_visible() || s.gap_rule_width <= 0.0 {
        return Vec::new();
    }

    // Content area of the container (border-box minus border+padding).
    let em = s.font_size;
    let cw = (b.rect.width
        - s.border_left_width
        - s.border_right_width
        - s.padding_left.px()
        - s.padding_right.px())
    .max(0.0);
    let ch = (b.rect.height
        - s.border_top_width
        - s.border_bottom_width
        - s.padding_top.px()
        - s.padding_bottom.px())
    .max(0.0);
    let cx = b.rect.x + s.border_left_width + s.padding_left.px();
    let cy = b.rect.y + s.border_top_width + s.padding_top.px();
    let vp = Size::new(cw, ch);

    let col_gap_px = s.column_gap.resolve_or_zero(em, cw, vp);
    let row_gap_px = s.row_gap.resolve_or_zero(em, ch, vp);

    // Collect in-flow (non-absolutely-positioned, non-skip) children.
    let children: Vec<_> = b
        .children
        .iter()
        .filter(|c| {
            !matches!(c.kind, BoxKind::Skip | BoxKind::Contents | BoxKind::Marker { .. })
                && !matches!(c.style.position, Position::Absolute | Position::Fixed)
        })
        .collect();

    if children.len() < 2 {
        return Vec::new();
    }

    let mut segments: Vec<GapSegment> = Vec::new();
    const EPS: f32 = 1.5; // tolerance for float layout rounding

    if col_gap_px > 0.0 {
        // Collect unique right-edges of children.
        let mut rights: Vec<f32> =
            children.iter().map(|c| c.rect.x + c.rect.width).collect();
        rights.sort_by(|a, x| a.partial_cmp(x).unwrap_or(std::cmp::Ordering::Equal));
        rights.dedup_by(|a, x| (*a - *x).abs() < EPS);

        // For each right-edge, check if a child starts right_edge + col_gap away.
        let lefts: Vec<f32> = children.iter().map(|c| c.rect.x).collect();
        for right in &rights {
            let expected = right + col_gap_px;
            if lefts.iter().any(|l| (*l - expected).abs() < EPS) {
                segments.push(GapSegment {
                    rect: Rect::new(*right, cy, col_gap_px, ch),
                    horizontal: false,
                });
            }
        }
    }

    if row_gap_px > 0.0 {
        // Collect unique bottom-edges of children.
        let mut bottoms: Vec<f32> =
            children.iter().map(|c| c.rect.y + c.rect.height).collect();
        bottoms.sort_by(|a, x| a.partial_cmp(x).unwrap_or(std::cmp::Ordering::Equal));
        bottoms.dedup_by(|a, x| (*a - *x).abs() < EPS);

        let tops: Vec<f32> = children.iter().map(|c| c.rect.y).collect();
        for bottom in &bottoms {
            let expected = bottom + row_gap_px;
            if tops.iter().any(|t| (*t - expected).abs() < EPS) {
                segments.push(GapSegment {
                    rect: Rect::new(cx, *bottom, cw, row_gap_px),
                    horizontal: true,
                });
            }
        }
    }

    segments
}

fn walk(b: &LayoutBox, out: &mut DisplayList, dpr: f32, sel: Option<&SelectionHighlight>) {
    // CSS Color L3 §3.2 — opacity:0 на box-е делает весь subtree после
    // composite полностью прозрачным. Phase 0 эмулирует это pure-pixel
    // skip-ом (отличие от visibility:hidden, где children могут
    // override через `:visible` — opacity-0 такого override не имеет).
    if !is_opacity_subtree_painted(b) {
        return;
    }
    // CSS Transforms L2 §5.1 — `backface-visibility: hidden` culls the box
    // (and its subtree) once its own 3D transform has rotated its face past
    // 90°, so it points away from the viewer.
    if is_backface_hidden(b) {
        return;
    }
    // CSS Positioning L3 §6.3 — position:sticky. Wraps the entire box in a
    // BeginStickyLayer/EndStickyLayer pair so the renderer can apply a
    // scroll-clamped offset at draw time without rebuilding the display list.
    let is_sticky = matches!(b.style.position, Position::Sticky);
    if is_sticky {
        let s = &b.style;
        out.push(DisplayCommand::BeginStickyLayer {
            flow_rect: b.rect,
            top:    s.top.to_px_opt(),
            bottom: s.bottom.to_px_opt(),
            left:   s.left.to_px_opt(),
            right:  s.right.to_px_opt(),
        });
    }
    // CSS Positioning L3 §6.1 — position:fixed. Brackets the box (and subtree)
    // with a BeginFixedLayer/EndFixedLayer pair so the compositor scroll-blit can
    // split it out of the scrollable band (ADR-016 M3.2.1c). No draw-time offset:
    // fixed content is already at viewport-fixed coords (BUG-159), so the markers
    // render as no-ops — they are partition metadata only.
    let is_fixed = matches!(b.style.position, Position::Fixed);
    if is_fixed {
        out.push(DisplayCommand::BeginFixedLayer);
    }
    match &b.kind {
        BoxKind::Skip | BoxKind::Contents => {}
        BoxKind::Block | BoxKind::FlowRoot | BoxKind::TableRow
        | BoxKind::Table | BoxKind::TableRowGroup => {
            // CSS Masking L1 §4: mask-image wraps the entire element (opacity+transform+content).
            // Emitted outermost so the mask applies to the fully composited element.
            // `mask_groups` > 1 — вложенные группы `mask-composite: intersect`
            // (см. `rendered_mask_layers`); закрываются столькими же PopMask.
            let mask_groups = emit_push_mask(out, b);
            let has_mask = mask_groups > 0;
            // CSS Masking L1 §4.6 — `mask-clip` restricts the masked painting to
            // the padding/content box. Pushed inside the mask group; popped before
            // PopMask below.
            let mask_clip = if has_mask { mask_clip_paint_rect(b) } else { None };
            if let Some(clip) = mask_clip {
                out.push(DisplayCommand::PushClipRect { rect: clip });
            }
            // CSS Masking L1 §9: clip-path clips the fully composited element;
            // эмитится ниже — ВНУТРИ PushTransform (BUG-140).
            let has_clip_path = b.style.clip_path.is_some();
            // CSS Compositing & Blending L1 §5: mix-blend-mode wraps opacity so
            // the element (faded by its own opacity) blends against the backdrop
            // (order Clip → Blend → Opacity, mirroring `box_layer_ops`).
            let has_blend = b.style.mix_blend_mode != LayoutBlendMode::Normal;
            if has_blend {
                out.push(DisplayCommand::PushBlendMode {
                    mode: map_blend_mode(b.style.mix_blend_mode),
                    bounds: b.rect,
                });
            }
            // CSS Color L3 §3: opacity < 1.0 creates compositing layer.
            let has_opacity = b.style.opacity < 1.0; // >0.0 already checked above
            if has_opacity {
                out.push(DisplayCommand::PushOpacity { alpha: b.style.opacity, bounds: Some(b.rect) });
            }
            // CSS Transforms L1 §13: forward-матрица применяется до родителя,
            // т.е. PushTransform — ВНУТРИ opacity-layer-а. Применяется ко
            // всему содержимому box-а (включая собственный background/border).
            let transform = forward_box_transform(b);
            if let Some(matrix) = transform {
                out.push(DisplayCommand::PushTransform { matrix });
            }
            // CSS Masking L1 §9 + BUG-140: clip-path задан в локальной системе
            // элемента и переносится его transform-ом — эмитится внутри
            // PushTransform, снаружи filter/backdrop-filter.
            if let Some(clip) = &b.style.clip_path {
                match clip_path_to_shape(clip, b.rect) {
                    Some(shape) => out.push(DisplayCommand::PushClipPath { shape }),
                    None => out.push(DisplayCommand::PushClipRect {
                        rect: clip_path_to_rect(clip, b.rect),
                    }),
                }
            }
            // CSS Filter Effects L1 §6.2 — `backdrop-filter` filters the content
            // already painted *behind* the element, clipped to its border box,
            // before the element's own content paints on top. Emitted after the
            // transform (mirroring `box_layer_ops` ordering) and outermost
            // relative to the element's own `filter`, so the element content
            // composites over the filtered backdrop.
            let has_backdrop = !b.style.backdrop_filter.is_empty();
            if has_backdrop {
                out.push(DisplayCommand::PushBackdropFilter {
                    filters: b.style.backdrop_filter.clone(),
                    bounds: b.rect,
                });
            }
            // CSS Filter Effects L1 §4 — the element's own `filter` wraps the
            // element's full painted output (shadows + background + border +
            // children + outline) as the innermost layer; the matching
            // `PopFilter` applies the chain and composites the result down.
            let has_filter = !b.style.filter.is_empty();
            if has_filter {
                out.push(DisplayCommand::PushFilter {
                    filters: b.style.filter.clone(),
                    bounds: Some(b.rect),
                });
            }
            // CSS Display L3 §4 — `visibility: hidden`: self не рисуется
            // (фон/border/outline/shadow), но children обходятся (inherited
            // visibility, но child может вернуть себя через `:visible`).
            // CSS Tables L2 §17.6.1.1 — `empty-cells: hide` suppresses an empty
            // cell's background and borders the same way (children still walked).
            let self_visible = is_paint_visible(b) && !is_hidden_empty_cell(b);
            if self_visible {
                emit_box_shadows(b, out);
                if let Some(CssColor::Rgba(bg)) = b.style.background_color
                    && bg.a > 0
                {
                    let clip = background_clip_rect(b, background_color_clip(b));
                    if clip.width > 0.0 && clip.height > 0.0 {
                        out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                    }
                }
                emit_background_image(out, b, dpr);
                emit_inset_box_shadows(b, out);
                let s = &b.style;
                let has_border = s.border_top_style.is_visible()
                    || s.border_right_style.is_visible()
                    || s.border_bottom_style.is_visible()
                    || s.border_left_style.is_visible();
                if has_border {
                    let cur = s.color;
                    out.push(DisplayCommand::DrawBorder {
                        rect: b.rect,
                        widths: [
                            s.border_top_width, s.border_right_width,
                            s.border_bottom_width, s.border_left_width,
                        ],
                        colors: [
                            s.border_top_color.resolve(cur),
                            s.border_right_color.resolve(cur),
                            s.border_bottom_color.resolve(cur),
                            s.border_left_color.resolve(cur),
                        ],
                        styles: [
                            s.border_top_style, s.border_right_style,
                            s.border_bottom_style, s.border_left_style,
                        ],
                        radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                    });
                }
                emit_column_rules(b, out);
            }
            // CSS Overflow L3 §3.2: overflow: hidden/scroll/auto/clip clips
            // descendant content to the padding-box edge. Per-axis: only the
            // clipping axis is constrained; the unconstrained axis uses a large
            // sentinel so the GPU scissor doesn't cut off content in that
            // direction (the renderer clamps to surface bounds automatically).
            // scroll/auto → PushScrollLayer (clip + scroll translate).
            // hidden/clip/paint-contain → PushClipRect (clip only).
            let clip_x = overflow_clips(b.style.overflow_x);
            let clip_y = overflow_clips(b.style.overflow_y);
            let has_overflow_clip = clip_x || clip_y;
            let is_scroll_x = matches!(b.style.overflow_x, Overflow::Scroll | Overflow::Auto);
            let is_scroll_y = matches!(b.style.overflow_y, Overflow::Scroll | Overflow::Auto);
            let use_scroll_layer = (is_scroll_x || is_scroll_y) && has_overflow_clip;
            // Capture padding-box rect for scrollbar geometry (used after PopScrollLayer).
            let scroll_padding_box: Option<(f32, f32, f32, f32)> = if use_scroll_layer {
                let s = &b.style;
                let px = b.rect.x + s.border_left_width;
                let py = b.rect.y + s.border_top_width;
                let pw = (b.rect.width - s.border_left_width - s.border_right_width).max(0.0);
                let ph = (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0);
                Some((px, py, pw, ph))
            } else {
                None
            };
            if has_overflow_clip {
                const BIG: f32 = 1_000_000.0;
                let s = &b.style;
                let px = b.rect.x + s.border_left_width;
                let py = b.rect.y + s.border_top_width;
                let pw = (b.rect.width - s.border_left_width - s.border_right_width).max(0.0);
                let ph = (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0);
                let mut cr = Rect::new(
                    if clip_x { px } else { -BIG },
                    if clip_y { py } else { -BIG },
                    if clip_x { pw } else { 2.0 * BIG },
                    if clip_y { ph } else { 2.0 * BIG },
                );

                // CSS Overflow L3: overflow-clip-margin расширяет clip region для overflow:clip.
                let is_overflow_clip_x = matches!(b.style.overflow_x, Overflow::Clip);
                let is_overflow_clip_y = matches!(b.style.overflow_y, Overflow::Clip);
                if (is_overflow_clip_x || is_overflow_clip_y)
                    && let Some(margin) = &s.overflow_clip_margin
                    && let Some(margin_px) = margin.resolve(s.font_size, Some(pw.max(ph)), Size::new(pw, ph))
                {
                    if is_overflow_clip_x {
                        cr.x -= margin_px;
                        cr.width += 2.0 * margin_px;
                    }
                    if is_overflow_clip_y {
                        cr.y -= margin_px;
                        cr.height += 2.0 * margin_px;
                    }
                }

                if use_scroll_layer {
                    out.push(DisplayCommand::PushScrollLayer {
                        clip_rect: cr,
                        scroll_x: b.scroll_x,
                        scroll_y: b.scroll_y,
                    });
                } else {
                    out.push(DisplayCommand::PushClipRect { rect: cr });
                }
            }
            // CSS Transforms L2 §6.2: inside a `preserve-3d` 3D rendering
            // context children paint back-to-front by transformed depth;
            // otherwise document order (flat compositing).
            // Special handling for Table: emit table-specific layout (cells, borders, etc).
            if matches!(b.kind, BoxKind::Table) {
                emit_table_box(b, out, dpr);
            } else if establishes_3d_rendering_context(b) {
                for i in depth_sorted_child_order(&b.children) {
                    walk(&b.children[i], out, dpr, sel);
                }
            } else {
                for child in &b.children {
                    walk(child, out, dpr, sel);
                }
            }
            // CSS Gap Decorations L1 — emit gap rules for flex/grid containers.
            if self_visible {
                let gap_segs = collect_gap_segments(b);
                if !gap_segs.is_empty() {
                    let s = &b.style;
                    let ctx = GapDecorationContext {
                        rule_width: s.gap_rule_width,
                        rule_style: s.gap_rule_style,
                        rule_color: s.gap_rule_color.resolve(s.color),
                    };
                    out.extend(emit_gap_rules(&b.children, &gap_segs, &ctx));
                }
            }
            if has_overflow_clip {
                if use_scroll_layer {
                    out.push(DisplayCommand::PopScrollLayer);
                    // Emit scrollbar track + thumb after the scroll layer so they
                    // render at a fixed position (not translated with scrolled content).
                    // BUG-220: shared with the ordered `box_layer_ops` path.
                    if let Some(padding_box) = scroll_padding_box {
                        emit_scrollbars(b, padding_box, is_scroll_x, is_scroll_y, out);
                    }
                } else {
                    out.push(DisplayCommand::PopClip);
                }
            }
            if self_visible {
                // CSS Basic UI L4 §5: outline рисуется поверх контента box-а
                // (включая children), снаружи bounding-box-а. Phase 0 без
                // деления paint phases для outline — эмитим в конце box-walk-а.
                emit_outline(b, out);
            }
            if has_filter {
                out.push(DisplayCommand::PopFilter);
            }
            if has_backdrop {
                out.push(DisplayCommand::PopBackdropFilter);
            }
            if has_clip_path {
                out.push(DisplayCommand::PopClip);
            }
            if transform.is_some() {
                out.push(DisplayCommand::PopTransform);
            }
            if has_opacity {
                out.push(DisplayCommand::PopOpacity);
            }
            if has_blend {
                out.push(DisplayCommand::PopBlendMode);
            }
            if mask_clip.is_some() {
                out.push(DisplayCommand::PopClip);
            }
            for _ in 0..mask_groups {
                out.push(DisplayCommand::PopMask);
            }
        }
        BoxKind::FormControl { kind } => {
            // Replaced element: background + border box (Phase 0, no content).
            if !is_paint_visible(b) {
                return;
            }
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width, s.border_right_width,
                        s.border_bottom_width, s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style, s.border_right_style,
                        s.border_bottom_style, s.border_left_style,
                    ],
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            emit_outline(b, out);
            emit_form_control_indicator(b, kind, out);
        }
        BoxKind::InlineBlockRow => {
            // Анонимный контейнер: нет фона/бордера собственного.
            // Просто рекурсивно рисуем всех дочерних (BoxKind::Block).
            for child in &b.children {
                walk(child, out, dpr, sel);
            }
        }
        BoxKind::InlineSpace => {}
        BoxKind::Marker { .. } => {
            emit_list_marker(b, out);
        }
        BoxKind::InlineRun { lines, .. } => {
            emit_inline_run(b, lines, sel, dpr, out);
        }
        BoxKind::Image { src, alt, is_lazy } => {
            // visibility:hidden на `<img>` пропускает всё (no children).
            if !is_paint_visible(b) {
                return;
            }
            // Painter's order для replaced element: фон → bg-image → border → <img>.
            // background/border у `<img>` валидны по CSS — например, для
            // подложки на время загрузки или рамки вокруг картинки.
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b, dpr);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width, s.border_right_width,
                        s.border_bottom_width, s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style, s.border_right_style,
                        s.border_bottom_style, s.border_left_style,
                    ],
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // BUG-431: bitmap belongs in the content box, same rule as <canvas>
            // (BUG-099) — painting at the border box slides it under the border.
            if *is_lazy {
                out.push(DisplayCommand::LazyImageSlot {
                    rect: content_box_rect(b),
                    node_id: b.node.index() as u32,
                    src: src.clone(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                });
            } else {
                // object-fit / object-position читаются на render-стадии вместе
                // с известным intrinsic-размером изображения.
                out.push(DisplayCommand::DrawImage {
                    rect: content_box_rect(b),
                    src: src.clone(),
                    alt: alt.clone(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                    image_rendering: b.style.image_rendering,
                });
            }
            emit_outline(b, out);
        }
        BoxKind::Video { src, poster } => {
            // visibility:hidden на `<video>` пропускает всё (no children).
            if !is_paint_visible(b) {
                return;
            }
            // Painter's order для replaced element: фон → bg-image → border → placeholder.
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b, dpr);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width, s.border_right_width,
                        s.border_bottom_width, s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style, s.border_right_style,
                        s.border_bottom_style, s.border_left_style,
                    ],
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // Phase 1: GIF-backed <video> — frame uploaded by shell under "video:{nid}".
            // Non-GIF src or no src: fall back to poster image (Phase 0 behaviour).
            // The shell's tick loop re-registers the current GIF frame under this key
            // on every render tick, so the DrawImage command always shows the live frame.
            // CSS: object-fit — P4 wires ComputedStyle.object_fit to scale the frame.
            // BUG-431: destination is the content box, not the border box.
            let nid = b.node.index();
            let is_gif_src = src.to_ascii_lowercase().ends_with(".gif") && !src.is_empty();
            if is_gif_src {
                out.push(DisplayCommand::DrawImage {
                    rect: content_box_rect(b),
                    src: format!("video:{nid}"),
                    alt: String::new(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                    image_rendering: b.style.image_rendering,
                });
            } else if !poster.is_empty() {
                out.push(DisplayCommand::DrawImage {
                    rect: content_box_rect(b),
                    src: poster.clone(),
                    alt: String::new(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                    image_rendering: b.style.image_rendering,
                });
            }
            emit_outline(b, out);
        }
        BoxKind::Canvas { .. } => {
            // visibility:hidden on <canvas> skips everything (no children).
            if !is_paint_visible(b) {
                return;
            }
            // Painter's order for replaced element: background → bg-image → border → bitmap.
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b, dpr);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width, s.border_right_width,
                        s.border_bottom_width, s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style, s.border_right_style,
                        s.border_bottom_style, s.border_left_style,
                    ],
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // Bitmap uploaded by shell under `canvas:{node_id}`; unregistered → transparent.
            // BUG-099: destination is the content box, not the border box.
            let nid = b.node.index();
            out.push(DisplayCommand::DrawImage {
                rect: content_box_rect(b),
                src: format!("canvas:{nid}"),
                alt: String::new(),
                object_fit: ObjectFit::Fill,
                object_position: b.style.object_position,
                image_rendering: b.style.image_rendering,
            });
            emit_outline(b, out);
        }
        BoxKind::Audio { controls, .. } => {
            if !is_paint_visible(b) || !controls || b.rect.width <= 0.0 || b.rect.height <= 0.0 {
                return;
            }
            // Phase 0: grey bar for audio controls UI.
            let grey = Color { r: 200, g: 200, b: 200, a: 255 };
            out.push(DisplayCommand::FillRect { rect: b.rect, color: grey });
            emit_outline(b, out);
        }
        BoxKind::Iframe { src, .. } => {
            if !is_paint_visible(b) || b.rect.width <= 0.0 || b.rect.height <= 0.0 {
                return;
            }
            // Phase 0: grey placeholder — no sub-document navigation.
            // DrawImage with src as key: unregistered key → grey placeholder (same as Video).
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b, dpr);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width, s.border_right_width,
                        s.border_bottom_width, s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style, s.border_right_style,
                        s.border_bottom_style, s.border_left_style,
                    ],
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // BUG-431: destination is the content box, not the border box.
            out.push(DisplayCommand::DrawImage {
                rect: content_box_rect(b),
                src: src.clone(),
                alt: String::new(),
                object_fit: b.style.object_fit,
                object_position: b.style.object_position,
                image_rendering: b.style.image_rendering,
            });
            emit_outline(b, out);
        }
        BoxKind::SvgRoot { .. } => {
            // SVG root: draw optional background/border, then recurse into shape children.
            if is_paint_visible(b)
                && let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                out.push(DisplayCommand::FillRect { rect: b.rect, color: bg });
            }
            // SVG §7.4: the outermost SVG viewport clips its content (UA default
            // `overflow: hidden`) — object-fit: cover / oversized viewBox content
            // must not paint outside the SVG box. BUG-110.
            let s = &b.style;
            let clip = Rect::new(
                b.rect.x + s.border_left_width,
                b.rect.y + s.border_top_width,
                (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
                (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0),
            );
            out.push(DisplayCommand::PushClipRect { rect: clip });
            for child in &b.children {
                walk(child, out, dpr, sel);
            }
            out.push(DisplayCommand::PopClip);
        }
        BoxKind::SvgShape { shape, .. } => {
            // CSS: fill, stroke, stroke-width — P4 wires ComputedStyle svg_fill/svg_stroke fields.
            // Default SVG presentation: fill=black (SVG spec §11.2), no stroke.
            emit_svg_shape(b, shape, out);
        }
        BoxKind::SvgText { text, text_anchor, dominant_baseline, baseline_shift, .. } => {
            // SVG text element: emit DrawText command with proper positioning.
            // CSS: fill, stroke, font-family, font-size — P4 wires ComputedStyle fields.
            // // CSS: text-anchor, dominant-baseline, baseline-shift
            emit_svg_text(b, text, *text_anchor, *dominant_baseline, *baseline_shift, out);
        }
    }
    if is_fixed {
        out.push(DisplayCommand::EndFixedLayer);
    }
    if is_sticky {
        out.push(DisplayCommand::EndStickyLayer);
    }
}

mod svg_text_decoration;
use svg_text_decoration::{emit_svg_shape, emit_svg_text, push_text_decoration, walk_with_anim};

#[cfg(test)]
#[path = "display_list/tests/text_and_images.rs"]
mod text_and_images;

mod text_highlight;
pub use text_highlight::emit_text_with_highlights;

mod table;
use table::{collect_table_cells, emit_table_box, emit_table_cell_border};

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
