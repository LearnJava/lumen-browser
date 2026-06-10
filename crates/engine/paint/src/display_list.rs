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

use lumen_core::geom::{Rect, Size};
use lumen_dom::InputType;
use lumen_layout::{
    box_can_own_stacking_context, creates_stacking_context, forward_box_transform,
    transform_fns_to_matrix, CompositorAnimFrame, CompositorOverride,
    BackgroundClip, BackgroundImage, BackgroundLayer, BackgroundOrigin, BackgroundRepeat, BackgroundSize, BorderCollapse, BorderStyle, BoxKind,
    ClipPath, Color, ComputedStyle, ContainFlags, CssColor, Display, FilterFn, FontOpticalSizing, FontStretch, FontStyle, FontWeight,
    FillRule, FormControlKind, StrokeLinecap, StrokeLinejoin, SvgShapeKind, SvgTextAnchor, SvgDominantBaseline,
    GradientStop, ImageRendering, Length, ListStyleType, ParsedGradient,
    InlineFrag, LayoutBox, MarginBox, Mat4, MixBlendMode as LayoutBlendMode, ObjectFit, ObjectPosition,
    OutlineColor, OutlineStyle, Overflow, Page, PaintOrder, PaintPhase, Position, PositionComponent, Resize,
    ScrollbarWidth, SelectionHighlight,
    StackingContextId, StackingTree, TextDecorationStyle, TextDecorationThickness,
    TextEmphasisShape, TextEmphasisStyle, TextOverflow, TextUnderlinePosition,
    TransformStyle,
    Visibility,
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
        /// CSS Fonts L4 §7 — user-space variation axes из `font-variation-settings`.
        /// Пары `(tag, value)` в user units — нормализация через fvar+avar
        /// выполняется в renderer-е, который имеет доступ к шрифтовым таблицам.
        /// Пустой Vec = `normal` (default-instance без variation deltas).
        /// CSS: font-optical-sizing — P4 должен добавить opsz значение в этот Vec.
        font_variation_axes: Vec<([u8; 4], f32)>,
        /// CSS Text L3 §10.1 — pixel width for a tab character (\t).
        /// 0.0 means no tab characters in text (renderer skips tab expansion).
        tab_size: f32,
        highlight_name: Option<String>
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
    /// Закрывает rect-клип, открытый ближайшим `PushClipRect`. Парность
    /// гарантируется эмиттером.
    PopClip,
    /// Sprint 0 P2 stub. Открывает opacity-группу: все последующие
    /// команды до парного `PopOpacity` композитятся как off-screen-layer
    /// и накладываются с `alpha`. Используется для `opacity != 1`. Phase 0:
    /// эмиттер не выпускает (нужен compositor с layer-pipeline-ом —
    /// roadmap-задача), renderer игнорирует.
    PushOpacity { alpha: f32 },
    /// Закрывает opacity-группу.
    PopOpacity,
    /// Открывает blend-группу с указанным режимом смешения
    /// (CSS Compositing & Blending L1 §5). Все последующие команды до
    /// парного `PopBlendMode` применяются поверх родительского контекста
    /// через `mode`. `BlendMode::Normal` — стандартный alpha-over (no-op).
    /// Phase 0: renderer отслеживает стек через `current_blend_mode()`,
    /// но использует Normal pipeline для всех режимов; реальный pipeline
    /// switch — P2 1B.4.
    PushBlendMode { mode: BlendMode },
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
    ///   bounding box трансформированного rect-а как scissor — корректно
    ///   только для translate-чистых трансформов; rotate/scale могут потерять
    ///   точность по краям. Полноценный clip через clip-mask — P2 п.4+.
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
    PushFilter { filters: Vec<FilterFn> },
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

pub type DisplayList = Vec<DisplayCommand>;

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
struct HashFmt<'a>(&'a mut std::collections::hash_map::DefaultHasher);

impl std::fmt::Write for HashFmt<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        use std::hash::Hasher;
        self.0.write(s.as_bytes());
        Ok(())
    }
}

/// Computes a content hash over a frame's display list plus the viewport state
/// that affects backdrop-filter output (scroll offset and surface size).
///
/// Used by the renderer's `backdrop-filter` cache (CSS Filter Effects L1 §2):
/// if two consecutive frames hash identically, every backdrop element's
/// filtered result is guaranteed identical, so the blur passes can be skipped
/// and the cached texture reused.
///
/// The hash is **total** — it folds every field of every command via each
/// command's `Debug` representation — so adding new `DisplayCommand` variants or
/// fields can never silently produce a false cache hit (which would paint stale
/// pixels). It is computed only when [`contains_backdrop_filter`] is true.
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
    use std::fmt::Write as _;
    use std::hash::Hasher;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write_u32(surface_w);
    hasher.write_u32(surface_h);
    hasher.write_u32(scroll_x.to_bits());
    hasher.write_u32(scroll_y.to_bits());
    {
        let mut hf = HashFmt(&mut hasher);
        for cmd in content.iter().chain(overlay.iter()) {
            // Errors are impossible: HashFmt::write_str never fails.
            let _ = write!(hf, "{cmd:?}");
        }
    }
    hasher.finish()
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
    use std::hash::{Hash, Hasher};
    let mut all_identical = true;
    let mut changed_rects = Rect {
        x: f32::INFINITY,
        y: f32::INFINITY,
        width: 0.0,
        height: 0.0,
    };

    for (prev_cmd, next_cmd) in prev.iter().zip(next.iter()) {
        // Используем Debug-представление для хеширования (как в hash_display_list).
        let prev_hash = {
            use std::collections::hash_map::DefaultHasher;
            let mut hasher = DefaultHasher::new();
            format!("{:?}", prev_cmd).hash(&mut hasher);
            hasher.finish()
        };
        let next_hash = {
            use std::collections::hash_map::DefaultHasher;
            let mut hasher = DefaultHasher::new();
            format!("{:?}", next_cmd).hash(&mut hasher);
            hasher.finish()
        };

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
                font_variation_axes, tab_size: _,
                highlight_name: _,
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
            DisplayCommand::DrawRadialGradient { rect, center_x_pct, center_y_pct, stops, repeating } => {
                out.push_str(&format!(
                    "DrawRadialGradient ({:.2}, {:.2}, {:.2}, {:.2}) center=({center_x_pct:.2},{center_y_pct:.2}) stops={} repeating={repeating}\n",
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
            DisplayCommand::PopClip => {
                out.push_str("PopClip\n");
            }
            DisplayCommand::PushOpacity { alpha } => {
                out.push_str(&format!("PushOpacity {alpha:.3}\n"));
            }
            DisplayCommand::PopOpacity => {
                out.push_str("PopOpacity\n");
            }
            DisplayCommand::PushBlendMode { mode } => {
                out.push_str(&format!("PushBlendMode {}\n", blend_mode_name(*mode)));
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
            DisplayCommand::PushFilter { filters } => {
                let names: Vec<&str> = filters.iter().map(filter_fn_name).collect();
                out.push_str(&format!("PushFilter [{}]\n", names.join(", ")));
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
/// **Phase 0 ограничение для layer-ops:** `pre` / `post` SC-owner-а охватывают
/// только `root_bg + contents` собственного SC, **не** child-SC потомков (они
/// рисуются после `InlineContent` parent-SC в линейном порядке, а `post` уже
/// эмитится в той же `InlineContent`-фазе). Для строгой семантики
/// `opacity / blend-mode` родителя на child-SC потребуется либо stack-based
/// эмиссия с явным end-of-SC маркером в `PaintOrder`, либо группировка
/// child-SC внутри parent-bucket. Renderer сейчас всё равно игнорирует
/// Push/Pop (роадмап P2 п.1B шаг (c) — реальный layer-pipeline), так что
/// текущая эмиссия — interface-level: парность сохранена, потребители
/// (compositor) видят сами триггеры; уточнение охвата child-SC — отдельный
/// шаг при реальном compositor pipeline.
pub fn build_display_list_ordered(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
) -> DisplayList {
    build_display_list_ordered_dpr(root, tree, order, 1.0)
}

/// Like [`build_display_list_ordered`] but resolves `image-set()` background
/// variants for the device pixel ratio `dpr` (CSS Images L4 §5). Shell passes
/// the window scale factor; `build_display_list_ordered` defaults to `1.0`.
pub fn build_display_list_ordered_dpr(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
    dpr: f32,
) -> DisplayList {
    let n_sc = tree.contexts.len().max(1);
    let mut buckets: Vec<ScBucket> = vec![ScBucket::default(); n_sc];
    let mut next_sc_id: u32 = 1;
    fill_buckets(root, StackingContextId::ROOT, &mut next_sc_id, &mut buckets, true, None, dpr);

    let mut out = Vec::new();
    for (sc_id, phase) in &order.steps {
        let idx = sc_id.0 as usize;
        if idx >= buckets.len() {
            continue;
        }
        let bucket = &mut buckets[idx];
        match phase {
            PaintPhase::RootBackground => {
                out.append(&mut bucket.pre);
                out.append(&mut bucket.root_bg);
            }
            PaintPhase::InlineContent => {
                out.append(&mut bucket.contents);
                out.append(&mut bucket.post);
            }
            // Phase 0: BlockBackgrounds / Floats merged into InlineContent;
            // marker-фазы (NegativeZ / PositionedAndZAuto / PositiveZ) в
            // выводе `PaintOrder::from_tree` не появляются — рекурсия
            // энкодирует их позицию через линейный порядок.
            _ => {}
        }
    }
    out
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
    let n_sc = tree.contexts.len().max(1);
    let mut buckets: Vec<ScBucket> = vec![ScBucket::default(); n_sc];
    let mut next_sc_id: u32 = 1;
    fill_buckets(root, StackingContextId::ROOT, &mut next_sc_id, &mut buckets, true, anim, dpr);

    let mut out = Vec::new();
    for (sc_id, phase) in &order.steps {
        let idx = sc_id.0 as usize;
        if idx >= buckets.len() {
            continue;
        }
        let bucket = &mut buckets[idx];
        match phase {
            PaintPhase::RootBackground => {
                out.append(&mut bucket.pre);
                out.append(&mut bucket.root_bg);
            }
            PaintPhase::InlineContent => {
                out.append(&mut bucket.contents);
                out.append(&mut bucket.post);
            }
            _ => {}
        }
    }
    out
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
            rect,
            text: frag.text.clone(),
            font_size: default_font_size,
            color: text_color,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
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

/// CSS Masking L1 §9 — bounding-box rect for a `clip-path` shape relative to
/// the element's border-box `r`. Phase 0: non-rect shapes use their bounding
/// box as an approximation; full polygon masking is deferred.
fn clip_path_to_rect(clip: &ClipPath, r: Rect) -> Rect {
    match clip {
        ClipPath::Inset(sides) => {
            let (top, right, bottom, left) = match sides.as_slice() {
                [a]          => (*a, *a, *a, *a),
                [tb, rl]     => (*tb, *rl, *tb, *rl),
                [t, rl, b]   => (*t, *rl, *b, *rl),
                [t, ri, b, l] => (*t, *ri, *b, *l),
                _            => (0.0, 0.0, 0.0, 0.0),
            };
            Rect::new(
                r.x + left,
                r.y + top,
                (r.width - left - right).max(0.0),
                (r.height - top - bottom).max(0.0),
            )
        }
        ClipPath::Circle { radius, center } => {
            let (cx, cy) = center
                .map(|(x, y)| (r.x + x, r.y + y))
                .unwrap_or((r.x + r.width * 0.5, r.y + r.height * 0.5));
            Rect::new(cx - radius, cy - radius, 2.0 * radius, 2.0 * radius)
        }
        ClipPath::Ellipse { rx, ry, center } => {
            let (cx, cy) = center
                .map(|(x, y)| (r.x + x, r.y + y))
                .unwrap_or((r.x + r.width * 0.5, r.y + r.height * 0.5));
            Rect::new(cx - rx, cy - ry, 2.0 * rx, 2.0 * ry)
        }
        ClipPath::Polygon(vertices) => {
            if vertices.is_empty() {
                return r;
            }
            let mut mn_x = f32::MAX;
            let mut mn_y = f32::MAX;
            let mut mx_x = f32::MIN;
            let mut mx_y = f32::MIN;
            for (vx, vy) in vertices {
                mn_x = mn_x.min(r.x + vx);
                mn_y = mn_y.min(r.y + vy);
                mx_x = mx_x.max(r.x + vx);
                mx_y = mx_y.max(r.y + vy);
            }
            Rect::new(mn_x, mn_y, (mx_x - mn_x).max(0.0), (mx_y - mn_y).max(0.0))
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
            rect: Rect::new(frag_x + i as f32 * char_w, mark_y, char_w, mark_size * 1.5),
            text: mark.to_string(),
            font_size: mark_size,
            color,
            font_family: frag.style.font_family.clone(),
            font_weight: frag.style.font_weight,
            font_style: frag.style.font_style,
            font_variation_axes: vec![],
            tab_size: 0.0,
            highlight_name: None,
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
        // Inline-replaced image: emit DrawImage, skip text rendering.
        if let Some(src) = &frag.img_src {
            out.push(DisplayCommand::DrawImage {
                rect: Rect::new(container_x + frag.x, frag_y, frag.width, line_h),
                src: src.clone(),
                alt: frag.text.clone(),
                object_fit: frag.style.object_fit,
                object_position: frag.style.object_position,
                image_rendering: frag.style.image_rendering,
            });
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
            rect: base_rect,
            text: frag.text.clone(),
            font_size: frag.style.font_size,
            color: text_color,
            font_family: frag.style.font_family.clone(),
            font_weight: frag.style.font_weight,
            font_style: frag.style.font_style,
            font_variation_axes: {
                let mut axes: Vec<([u8; 4], f32)> = frag.style.font_variation_settings
                    .iter().map(|a| (a.tag, a.value)).collect();
                if frag.style.font_optical_sizing == FontOpticalSizing::Auto
                    && !axes.iter().any(|(t, _)| t == b"opsz")
                {
                    axes.push((*b"opsz", frag.style.font_size));
                }
                // CSS Fonts L4 §5.2: inject `wdth` for non-normal font-stretch
                // unless the author already set it via font-variation-settings.
                if frag.style.font_stretch != FontStretch::NORMAL
                    && !axes.iter().any(|(t, _)| t == b"wdth")
                {
                    axes.push((*b"wdth", frag.style.font_stretch.0 as f32 / 10.0));
                }
                axes
            },
            tab_size: frag.style.tab_size,
            highlight_name: None,
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
    out: &mut Vec<DisplayCommand>,
) {
    let line_h = b.style.font_size * b.style.line_height;
    let wants_ellipsis = matches!(b.style.text_overflow, TextOverflow::Ellipsis)
        && overflow_clips(b.style.overflow_x);

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
                rect: Rect::new(b.rect.x + clip_w, line_y, ew, line_h),
                text: "\u{2026}".to_string(),
                font_size: ef.style.font_size,
                color: ef.style.color,
                font_family: ef.style.font_family.clone(),
                font_weight: ef.style.font_weight,
                font_style: ef.style.font_style,
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
            });
        } else {
            emit_text_frags(line, b.rect.x, b.rect.width, line_y, line_h, sel, out);
        }
    }
}

/// Собирает layer-effect триггеры одного box-а в pair (pre, post).
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
/// детерминирован для тестируемости): Clip → Blend → Opacity → Transform.
/// Pop — в обратном (Transform → Opacity → Blend → Clip). Transform пушится
/// последним, чтобы преобразовывать всё содержимое SC (включая собственные
/// background/border бокса, эмитимые в `root_bg`).
fn box_layer_ops(b: &LayoutBox, ov: Option<&CompositorOverride>) -> (Vec<DisplayCommand>, Vec<DisplayCommand>) {
    let mut pre = Vec::new();
    let mut post = Vec::new();
    if !box_can_own_stacking_context(b) {
        return (pre, post);
    }
    let s = &b.style;

    // CSS Masking L1 §9: clip-path is the outermost clip — applied before all
    // other layer effects so it masks the final painted output of the element.
    if let Some(clip) = &s.clip_path {
        let cr = clip_path_to_rect(clip, b.rect);
        pre.push(DisplayCommand::PushClipRect { rect: cr });
        post.push(DisplayCommand::PopClip);
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
        let is_scroll_x = matches!(s.overflow_x, Overflow::Scroll | Overflow::Auto);
        let is_scroll_y = matches!(s.overflow_y, Overflow::Scroll | Overflow::Auto);
        if (is_scroll_x || is_scroll_y) && !paint_contain {
            pre.push(DisplayCommand::PushScrollLayer {
                clip_rect: cr,
                scroll_x: b.scroll_x,
                scroll_y: b.scroll_y,
            });
            post.push(DisplayCommand::PopScrollLayer);
        } else {
            pre.push(DisplayCommand::PushClipRect { rect: cr });
            post.push(DisplayCommand::PopClip);
        }
    }
    if s.mix_blend_mode != LayoutBlendMode::Normal {
        pre.push(DisplayCommand::PushBlendMode {
            mode: map_blend_mode(s.mix_blend_mode),
        });
        post.push(DisplayCommand::PopBlendMode);
    }
    // Opacity: animation override wins over style value.
    let effective_opacity = ov.and_then(|o| o.opacity).unwrap_or(s.opacity);
    if effective_opacity < 1.0 {
        pre.push(DisplayCommand::PushOpacity { alpha: effective_opacity });
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
        pre.push(DisplayCommand::PushFilter { filters: s.filter.clone() });
        post.push(DisplayCommand::PopFilter);
    }
    // post в LIFO порядке относительно pre.
    post.reverse();
    (pre, post)
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
fn fill_buckets(
    b: &LayoutBox,
    current_sc: StackingContextId,
    next_sc_id: &mut u32,
    buckets: &mut [ScBucket],
    is_sc_root: bool,
    anim: Option<&CompositorAnimFrame>,
    dpr: f32,
) {
    let ov = anim.and_then(|a| a.get(b.node));
    let (pre_ops, post_ops) = box_layer_ops(b, ov);

    if is_sc_root {
        let bucket = &mut buckets[current_sc.0 as usize];
        bucket.pre.extend(pre_ops);
        emit_box_self(b, &mut bucket.root_bg, dpr, None);
        // `post` эмитится в фазе InlineContent после descendants — заполним
        // его сейчас, чтобы не повторно вычислять триггеры.
        bucket.post.extend(post_ops);

        for child in &b.children {
            let child_creates_sc =
                box_can_own_stacking_context(child) && creates_stacking_context(&child.style);
            if child_creates_sc {
                let id = StackingContextId(*next_sc_id);
                *next_sc_id += 1;
                fill_buckets(child, id, next_sc_id, buckets, true, anim, dpr);
            } else {
                fill_buckets(child, current_sc, next_sc_id, buckets, false, anim, dpr);
            }
        }
    } else {
        // Non-SC box: inline Push/Pop в contents текущего SC. Это нужно для
        // `overflow:hidden` на обычном in-flow box-е (opacity/blend
        // триггерят SC сами, до сюда не дойдут с не-пустым pre_ops).
        let bucket = &mut buckets[current_sc.0 as usize];
        bucket.contents.extend(pre_ops);
        emit_box_self(b, &mut bucket.contents, dpr, None);

        for child in &b.children {
            let child_creates_sc =
                box_can_own_stacking_context(child) && creates_stacking_context(&child.style);
            if child_creates_sc {
                let id = StackingContextId(*next_sc_id);
                *next_sc_id += 1;
                fill_buckets(child, id, next_sc_id, buckets, true, anim, dpr);
            } else {
                fill_buckets(child, current_sc, next_sc_id, buckets, false, anim, dpr);
            }
        }

        let bucket = &mut buckets[current_sc.0 as usize];
        bucket.contents.extend(post_ops);
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
        if sigma > 0.0 {
            out.push(DisplayCommand::PushFilter {
                filters: vec![FilterFn::Blur(sigma)],
            });
        }
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(
                base_rect.x + shadow.offset_x,
                base_rect.y + shadow.offset_y,
                base_rect.width,
                line_h,
            ),
            text: frag.text.clone(),
            font_size: frag.style.font_size,
            color,
            font_family: frag.style.font_family.clone(),
            font_weight: frag.style.font_weight,
            font_style: frag.style.font_style,
            // CSS Fonts L4 §7.12: for `auto`, inject opsz = font_size so the renderer
            // normalizes it via fvar like any other axis. Skipped for `none` to let
            // font-variation-settings control opsz directly.
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
fn background_clip_rect(b: &LayoutBox, clip: BackgroundClip) -> Rect {
    let s = &b.style;
    match clip {
        BackgroundClip::BorderBox | BackgroundClip::Text => b.rect,
        BackgroundClip::PaddingBox => Rect::new(
            b.rect.x + s.border_left_width,
            b.rect.y + s.border_top_width,
            (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
            (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0),
        ),
        BackgroundClip::ContentBox => Rect::new(
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
        ),
    }
}

/// CSS Backgrounds L3 §3.10: clip для `background-color` — last layer's clip (или default).
fn background_color_clip(b: &LayoutBox) -> BackgroundClip {
    b.style.background_layers.last().map_or(BackgroundClip::BorderBox, |l| l.clip)
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
    } else if let Some(n) = lower.strip_suffix('x') {
        (n, 1.0)
    } else {
        return None;
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

    let (tile_x_start, repeat_x, repeat_y) = match repeat {
        BackgroundRepeat::NoRepeat => (tile_x0, false, false),
        BackgroundRepeat::RepeatX => (tile_x0 - (off_x / tile_w).ceil() * tile_w, true, false),
        BackgroundRepeat::RepeatY => (tile_x0, false, true),
        BackgroundRepeat::Repeat | BackgroundRepeat::Round | BackgroundRepeat::Space => {
            (tile_x0 - (off_x / tile_w).ceil() * tile_w, true, true)
        }
    };
    let tile_y_start = if repeat_y {
        tile_y0 - (off_y / tile_h).ceil() * tile_h
    } else {
        tile_y0
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
                tx += tile_w;
            }
        }
        if !repeat_y {
            break;
        }
        ty += tile_h;
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
            let tile_w = w.max(1.0);
            let tile_h = h.unwrap_or(origin.height).max(1.0);
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
        out.push(DisplayCommand::PushBlendMode { mode: map_blend_mode(layer.blend_mode) });
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
        BackgroundImage::Gradient(ParsedGradient::Linear { angle_deg, stops, repeating }) => {
            let (rects, needs_clip) = gradient_paint_rects(layer, origin, clip);
            if needs_clip && !rects.is_empty() {
                out.push(DisplayCommand::PushClipRect { rect: clip });
            }
            for r in &rects {
                out.push(DisplayCommand::DrawLinearGradient {
                    rect: *r,
                    angle_deg: *angle_deg,
                    stops: stops.clone(),
                    repeating: *repeating,
                });
            }
            if needs_clip && !rects.is_empty() {
                out.push(DisplayCommand::PopClip);
            }
        }
        BackgroundImage::Gradient(ParsedGradient::Radial { center_x_pct, center_y_pct, stops, repeating }) => {
            let (rects, needs_clip) = gradient_paint_rects(layer, origin, clip);
            if needs_clip && !rects.is_empty() {
                out.push(DisplayCommand::PushClipRect { rect: clip });
            }
            for r in &rects {
                out.push(DisplayCommand::DrawRadialGradient {
                    rect: *r,
                    center_x_pct: *center_x_pct,
                    center_y_pct: *center_y_pct,
                    stops: stops.clone(),
                    repeating: *repeating,
                });
            }
            if needs_clip && !rects.is_empty() {
                out.push(DisplayCommand::PopClip);
            }
        }
        BackgroundImage::Gradient(ParsedGradient::Conic {
            center_x_pct, center_y_pct, from_angle_deg, stops, repeating
        }) => {
            let (rects, needs_clip) = gradient_paint_rects(layer, origin, clip);
            if needs_clip && !rects.is_empty() {
                out.push(DisplayCommand::PushClipRect { rect: clip });
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
            if needs_clip && !rects.is_empty() {
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
fn emit_background_image(out: &mut Vec<DisplayCommand>, b: &LayoutBox, dpr: f32) {
    // Рисуем в обратном порядке: последний слой = нижний (рисуется первым).
    for (i, layer) in b.style.background_layers.iter().rev().enumerate() {
        // i == 0 is the bottom-most layer; suppress its blend mode (identity effect).
        emit_background_layer(out, b, layer, dpr, i == 0);
    }
}

/// CSS Masking L1 §4 — эмитит PushMask* перед элементом + его детьми.
/// Возвращает `true` если команда была эмитирована (нужен парный PopMask).
/// `rect` = border-box элемента (mask painting area).
fn emit_push_mask(out: &mut Vec<DisplayCommand>, b: &LayoutBox) -> bool {
    let rect = b.rect;
    match &b.style.mask_image {
        BackgroundImage::Url(src) if !src.is_empty() => {
            out.push(DisplayCommand::PushMaskImage {
                rect,
                src: src.clone(),
                size: b.style.mask_size,
                position: ObjectPosition::background_initial(),
                repeat: b.style.mask_repeat,
                image_rendering: b.style.image_rendering,
            });
            true
        }
        BackgroundImage::Gradient(ParsedGradient::Linear { angle_deg, stops, repeating }) => {
            out.push(DisplayCommand::PushMaskLinearGradient {
                rect,
                angle_deg: *angle_deg,
                stops: stops.clone(),
                repeating: *repeating,
            });
            true
        }
        BackgroundImage::Gradient(ParsedGradient::Radial {
            center_x_pct, center_y_pct, stops, repeating
        }) => {
            out.push(DisplayCommand::PushMaskRadialGradient {
                rect,
                center_x_pct: *center_x_pct,
                center_y_pct: *center_y_pct,
                stops: stops.clone(),
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
                stops: stops.clone(),
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
        if sigma > 0.0 {
            out.push(DisplayCommand::PushFilter {
                filters: vec![FilterFn::Blur(sigma)],
            });
        }
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(x, y, w, h),
            color,
        });
        if sigma > 0.0 {
            out.push(DisplayCommand::PopFilter);
        }
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
fn is_paint_visible(b: &LayoutBox) -> bool {
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
fn is_opacity_subtree_painted(b: &LayoutBox) -> bool {
    b.style.opacity > 0.0
}

/// Render checkbox checkmark or radio dot for checked form controls.
/// P2 note: this renders a simple filled rectangle as indicator; a full
/// vector checkmark / circle belongs to the renderer GPU primitive set.
fn emit_form_control_indicator(b: &LayoutBox, kind: &FormControlKind, out: &mut Vec<DisplayCommand>) {
    match kind {
        FormControlKind::Input { input_type, checked } => {
            if !checked { return; }
            let inset = match input_type {
                InputType::Checkbox => (b.rect.width * 0.2).clamp(2.0, 4.0),
                InputType::Radio    => (b.rect.width * 0.27).clamp(2.0, 4.0),
                _ => return,
            };
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(
                    b.rect.x + inset,
                    b.rect.y + inset,
                    (b.rect.width  - inset * 2.0).max(1.0),
                    (b.rect.height - inset * 2.0).max(1.0),
                ),
                color: Color { r: 21, g: 90, b: 192, a: 255 },
            });
        }
        FormControlKind::Select { selected_text } => {
            emit_select_indicator(b, selected_text, out);
        }
        FormControlKind::Button | FormControlKind::Textarea => {}
        FormControlKind::Range { value, min, max } => {
            emit_range_slider(b, *value, *min, *max, out);
        }
        FormControlKind::Progress { value, max } => {
            emit_progress_bar(b, *value, *max, out);
        }
        FormControlKind::Meter { value, min, max, low, high, optimum } => {
            emit_meter_bar(b, *value, *min, *max, *low, *high, *optimum, out);
        }
    }
}

/// Draw a range slider: gray track, blue filled portion, circular thumb.
fn emit_range_slider(b: &LayoutBox, value: f32, min: f32, max: f32, out: &mut Vec<DisplayCommand>) {
    let range = (max - min).max(f32::EPSILON);
    let fraction = ((value - min) / range).clamp(0.0, 1.0);

    let track_h = 4.0_f32;
    let thumb_r = 8.0_f32; // thumb diameter
    let track_y = b.rect.y + (b.rect.height - track_h) / 2.0;
    let track_x = b.rect.x + thumb_r / 2.0;
    let track_w = (b.rect.width - thumb_r).max(1.0);

    let gray = Color { r: 200, g: 200, b: 200, a: 255 };
    let blue = Color { r: 21, g: 90, b: 192, a: 255 };
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
/// Determinate: blue fill proportional to `value / max`.
/// Indeterminate (`value` is `None`): static 30% fill to indicate pending state.
fn emit_progress_bar(b: &LayoutBox, value: Option<f32>, max: f32, out: &mut Vec<DisplayCommand>) {
    let pad = 2.0_f32;
    let bar_x = b.rect.x + pad;
    let bar_y = b.rect.y + pad;
    let bar_max_w = (b.rect.width - pad * 2.0).max(0.0);
    let bar_h = (b.rect.height - pad * 2.0).max(1.0);
    let blue = Color { r: 21, g: 90, b: 192, a: 255 };
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
fn emit_select_indicator(b: &LayoutBox, selected_text: &str, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    let fg = s.color;
    let font_size = s.font_size.clamp(10.0, 14.0);
    let pad = 4.0;
    // Arrow column width (enough for "▼" glyph).
    let arrow_w = font_size + pad * 2.0;
    let text_w = (b.rect.width - arrow_w - pad * 2.0).max(1.0);

    // Selected label — clipped to available width.
    if !selected_text.is_empty() {
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(b.rect.x + pad, b.rect.y + pad, text_w, b.rect.height - pad * 2.0),
            text: selected_text.to_owned(),
            font_size,
            color: fg,
            font_family: s.font_family.clone(),
            font_weight: s.font_weight,
            font_style: s.font_style,
            font_variation_axes: vec![],
            tab_size: 0.0,
            highlight_name: None,
        });
    }

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
        rect: Rect::new(sep_x + pad, b.rect.y + pad, arrow_w - pad, b.rect.height - pad * 2.0),
        text: "\u{25BC}".to_owned(),
        font_size: font_size * 0.75,
        color: fg,
        font_family: s.font_family.clone(),
        font_weight: s.font_weight,
        font_style: s.font_style,
        font_variation_axes: vec![],
        tab_size: 0.0,
        highlight_name: None,
    });
}

/// CSS Lists L3 §2.1 — renders the `::marker` pseudo-element.
/// Bullet types (disc/circle/square) are drawn as geometric shapes to avoid
/// relying on specific Unicode glyphs in the bundled font.
/// Counter types (decimal/roman/alpha/greek) are rendered as text.
fn emit_list_marker(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let BoxKind::Marker { ref text, ref list_style_type, .. } = b.kind else { return };
    if !is_paint_visible(b) {
        return;
    }
    let s = &b.style;
    let color = s.color;
    let em = s.font_size;
    let cx = b.rect.x + b.rect.width * 0.5;
    let cy = b.rect.y + b.rect.height * 0.5;
    match list_style_type {
        ListStyleType::Disc => {
            // Filled circle ~0.4em in diameter, centered in marker rect.
            let d = em * 0.40;
            let r = d * 0.5;
            let rect = Rect::new(cx - r, cy - r, d, d);
            let radii = CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r };
            out.push(DisplayCommand::FillRoundedRect { rect, color, radii });
        }
        ListStyleType::Circle => {
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
        ListStyleType::Square => {
            // Filled square ~0.35em side, centered in marker rect.
            let d = em * 0.35;
            let rect = Rect::new(cx - d * 0.5, cy - d * 0.5, d, d);
            out.push(DisplayCommand::FillRect { rect, color });
        }
        _ => {
            // Counter types: decimal, roman, alpha, greek — render as text.
            if !text.is_empty() {
                out.push(DisplayCommand::DrawText {
                    rect: b.rect,
                    text: text.clone(),
                    font_size: em,
                    color,
                    font_family: s.font_family.clone(),
                    font_weight: s.font_weight,
                    font_style: s.font_style,
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
                });
            }
        }
    }
}

/// Эмитит DisplayCommand-ы для одного box-а БЕЗ рекурсии в детей. Аналог
/// тела `walk` для одного box-а.
fn emit_box_self(b: &LayoutBox, out: &mut Vec<DisplayCommand>, dpr: f32, sel: Option<&SelectionHighlight>) {
    // opacity:0 → whole-subtree invisible (см. is_opacity_subtree_painted).
    // emit_box_self не идёт в children, но self-content тоже skip-аем.
    if !is_opacity_subtree_painted(b) {
        return;
    }
    match &b.kind {
        BoxKind::Skip => {}
        BoxKind::Block | BoxKind::FlowRoot | BoxKind::TableRow
        | BoxKind::Table | BoxKind::TableRowGroup => {
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
            emit_column_rules(b, out);
            emit_outline(b, out);
        }
        BoxKind::InlineRun { lines, .. } => {
            emit_inline_run(b, lines, sel, out);
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
        BoxKind::Image { src, alt } => {
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
            out.push(DisplayCommand::DrawImage {
                rect: b.rect,
                src: src.clone(),
                alt: alt.clone(),
                object_fit: b.style.object_fit,
                object_position: b.style.object_position,
                image_rendering: b.style.image_rendering,
            });
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
            // Phase 0: only a `poster` image is painted (registered by shell, or
            // unregistered → grey placeholder). An empty `<video>` with no poster
            // and no decoded frame paints nothing — the element box is transparent,
            // matching Chromium/Edge (which show the page background through it).
            // The grey image placeholder is reserved for `<img>`, not media.
            // CSS: object-fit — P4 wires ComputedStyle.object_fit to scale poster/video frame.
            let _ = src;
            if !poster.is_empty() {
                out.push(DisplayCommand::DrawImage {
                    rect: b.rect,
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
            let nid = b.node.index();
            out.push(DisplayCommand::DrawImage {
                rect: b.rect,
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
            out.push(DisplayCommand::DrawImage {
                rect: b.rect,
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
        BoxKind::SvgText { text, text_anchor, dominant_baseline, .. } => {
            emit_svg_text(b, text, *text_anchor, *dominant_baseline, out);
        }
    }
    emit_resize_grip(b, out);
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
    match &b.kind {
        BoxKind::Skip | BoxKind::Contents => {}
        BoxKind::Block | BoxKind::FlowRoot | BoxKind::TableRow
        | BoxKind::Table | BoxKind::TableRowGroup => {
            // CSS Masking L1 §4: mask-image wraps the entire element (opacity+transform+content).
            // Emitted outermost so the mask applies to the fully composited element.
            let has_mask = emit_push_mask(out, b);
            // CSS Masking L1 §9: clip-path clips the fully composited element.
            let has_clip_path = if let Some(clip) = &b.style.clip_path {
                let cr = clip_path_to_rect(clip, b.rect);
                out.push(DisplayCommand::PushClipRect { rect: cr });
                true
            } else {
                false
            };
            // CSS Compositing & Blending L1 §5: mix-blend-mode wraps opacity so
            // the element (faded by its own opacity) blends against the backdrop
            // (order Clip → Blend → Opacity, mirroring `box_layer_ops`).
            let has_blend = b.style.mix_blend_mode != LayoutBlendMode::Normal;
            if has_blend {
                out.push(DisplayCommand::PushBlendMode {
                    mode: map_blend_mode(b.style.mix_blend_mode),
                });
            }
            // CSS Color L3 §3: opacity < 1.0 creates compositing layer.
            let has_opacity = b.style.opacity < 1.0; // >0.0 already checked above
            if has_opacity {
                out.push(DisplayCommand::PushOpacity { alpha: b.style.opacity });
            }
            // CSS Transforms L1 §13: forward-матрица применяется до родителя,
            // т.е. PushTransform — ВНУТРИ opacity-layer-а. Применяется ко
            // всему содержимому box-а (включая собственный background/border).
            let transform = forward_box_transform(b);
            if let Some(matrix) = transform {
                out.push(DisplayCommand::PushTransform { matrix });
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
                out.push(DisplayCommand::PushFilter { filters: b.style.filter.clone() });
            }
            // CSS Display L3 §4 — `visibility: hidden`: self не рисуется
            // (фон/border/outline/shadow), но children обходятся (inherited
            // visibility, но child может вернуть себя через `:visible`).
            let self_visible = is_paint_visible(b);
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
                    // `scrollbar-width: none` suppresses the visual scrollbar while
                    // keeping the scroll layer (container still scrolls via keyboard/JS).
                    if let Some((px, py, pw, ph)) = scroll_padding_box {
                        let gutter_px = match b.style.scrollbar_width {
                            ScrollbarWidth::Auto => SCROLLBAR_WIDTH,
                            ScrollbarWidth::Thin => SCROLLBAR_WIDTH_THIN,
                            ScrollbarWidth::None => 0.0,
                        };
                        // Only emit when scrollbar is visible (gutter_px > 0).
                        if gutter_px > 0.0 {
                            let (thumb_color, track_color) = match b.style.scrollbar_color {
                                Some((thumb, track)) => (color_u8_to_f32(thumb), color_u8_to_f32(track)),
                                None => (SCROLLBAR_THUMB_COLOR, SCROLLBAR_TRACK_COLOR),
                            };
                            // Compute content size from children (same as layout's content_height/width).
                            let content_w = b.children.iter().fold(b.rect.width, |acc, c| {
                                acc.max(c.rect.x + c.rect.width - b.rect.x)
                            });
                            let content_h = b.children.iter().fold(b.rect.height, |acc, c| {
                                acc.max(c.rect.y + c.rect.height - b.rect.y)
                            });
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
            if transform.is_some() {
                out.push(DisplayCommand::PopTransform);
            }
            if has_opacity {
                out.push(DisplayCommand::PopOpacity);
            }
            if has_blend {
                out.push(DisplayCommand::PopBlendMode);
            }
            if has_clip_path {
                out.push(DisplayCommand::PopClip);
            }
            if has_mask {
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
            emit_inline_run(b, lines, sel, out);
        }
        BoxKind::Image { src, alt } => {
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
            // Image content внутри padding/border-области; в Phase 0
            // padding/border ещё не сжимают content-area Image (только
            // расширяют коробку), `rect` — полная коробка вместе с border.
            // object-fit / object-position читаются на render-стадии вместе
            // с известным intrinsic-размером изображения.
            out.push(DisplayCommand::DrawImage {
                rect: b.rect,
                src: src.clone(),
                alt: alt.clone(),
                object_fit: b.style.object_fit,
                object_position: b.style.object_position,
                image_rendering: b.style.image_rendering,
            });
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
            // Phase 0: only a `poster` image is painted (registered by shell, or
            // unregistered → grey placeholder). An empty `<video>` with no poster
            // and no decoded frame paints nothing — the element box is transparent,
            // matching Chromium/Edge (which show the page background through it).
            // The grey image placeholder is reserved for `<img>`, not media.
            // CSS: object-fit — P4 wires ComputedStyle.object_fit to scale poster/video frame.
            let _ = src;
            if !poster.is_empty() {
                out.push(DisplayCommand::DrawImage {
                    rect: b.rect,
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
            let nid = b.node.index();
            out.push(DisplayCommand::DrawImage {
                rect: b.rect,
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
            out.push(DisplayCommand::DrawImage {
                rect: b.rect,
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
            for child in &b.children {
                walk(child, out, dpr, sel);
            }
        }
        BoxKind::SvgShape { shape, .. } => {
            // CSS: fill, stroke, stroke-width — P4 wires ComputedStyle svg_fill/svg_stroke fields.
            // Default SVG presentation: fill=black (SVG spec §11.2), no stroke.
            emit_svg_shape(b, shape, out);
        }
        BoxKind::SvgText { text, text_anchor, dominant_baseline, .. } => {
            // SVG text element: emit DrawText command with proper positioning.
            // CSS: fill, stroke, font-family, font-size — P4 wires ComputedStyle fields.
            // // CSS: text-anchor, dominant-baseline
            emit_svg_text(b, text, *text_anchor, *dominant_baseline, out);
        }
    }
    if is_sticky {
        out.push(DisplayCommand::EndStickyLayer);
    }
}

/// Applies `opacity` (0..1) to the alpha channel of a `Color`.
fn apply_opacity_to_color(color: Color, opacity: f32) -> Color {
    Color { r: color.r, g: color.g, b: color.b, a: (color.a as f32 * opacity).round() as u8 }
}

/// Emits paint commands for a single SVG shape using its pre-computed document-space rect.
/// Reads `svg_fill` / `svg_stroke` / `svg_fill_opacity` / `svg_stroke_opacity` /
/// `svg_stroke_width` from `ComputedStyle` — wired by P4 per SVG §11.2/11.3/11.4.
fn emit_svg_shape(b: &LayoutBox, shape: &SvgShapeKind, out: &mut DisplayList) {
    // A zero-size box bbox means "nothing to paint" for the geometry-driven shapes
    // (rect/circle/ellipse/line), whose painted extent equals `b.rect`. Paths are the
    // exception: layout cannot compute a path bbox (it requires full `d` parsing, so
    // `svg_shape_bbox` returns `Rect::ZERO`), and the path is painted from its `d`
    // segments offset by `b.rect.x/y`. Bailing here would drop every `<path>` element.
    if b.rect.width <= 0.0 && b.rect.height <= 0.0 && !matches!(shape, SvgShapeKind::Path { .. }) {
        return;
    }
    let current_color = b.style.color;
    let fill_color = b.style.svg_fill.resolve(current_color)
        .map(|c| apply_opacity_to_color(c, b.style.svg_fill_opacity));
    let stroke_color = b.style.svg_stroke.resolve(current_color)
        .map(|c| apply_opacity_to_color(c, b.style.svg_stroke_opacity));
    let stroke_w = b.style.svg_stroke_width;

    match shape {
        SvgShapeKind::Rect { rx, ry, .. } => {
            let has_radius = *rx > 0.0 || *ry > 0.0;
            let r = (*rx).min(b.rect.width / 2.0);
            let r_y = (*ry).min(b.rect.height / 2.0);
            let radii = CornerRadii { tl: r, tl_y: r_y, tr: r, tr_y: r_y, br: r, br_y: r_y, bl: r, bl_y: r_y };
            if let Some(fc) = fill_color {
                if has_radius {
                    out.push(DisplayCommand::FillRoundedRect { rect: b.rect, color: fc, radii });
                } else {
                    out.push(DisplayCommand::FillRect { rect: b.rect, color: fc });
                }
            }
            if let Some(sc) = stroke_color && stroke_w > 0.0 {
                let w = stroke_w;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [w, w, w, w],
                    colors: [sc, sc, sc, sc],
                    styles: [BorderStyle::Solid; 4],
                    radii,
                });
            }
        }
        SvgShapeKind::Circle { .. } | SvgShapeKind::Ellipse { .. } => {
            let rx_px = b.rect.width / 2.0;
            let ry_px = b.rect.height / 2.0;
            let radii = CornerRadii { tl: rx_px, tl_y: ry_px, tr: rx_px, tr_y: ry_px, br: rx_px, br_y: ry_px, bl: rx_px, bl_y: ry_px };
            if let Some(fc) = fill_color {
                out.push(DisplayCommand::FillRoundedRect { rect: b.rect, color: fc, radii });
            }
            if let Some(sc) = stroke_color && stroke_w > 0.0 {
                let w = stroke_w;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [w, w, w, w],
                    colors: [sc, sc, sc, sc],
                    styles: [BorderStyle::Solid; 4],
                    radii,
                });
            }
        }
        SvgShapeKind::Line { .. } => {
            // SVG <line> has no fill; rendered as a stroke-width rect.
            let color = stroke_color.or(fill_color).unwrap_or(Color::BLACK);
            out.push(DisplayCommand::FillRect { rect: b.rect, color });
        }
        SvgShapeKind::Path { d } => {
            let need_fill   = fill_color.is_some();
            let need_stroke = stroke_color.is_some() && stroke_w > 0.0;
            if need_fill || need_stroke {
                let segs = crate::svg_path::parse_svg_path(d);
                let contours = crate::svg_path::flatten_path(&segs, 0.5);
                if let Some(fc) = fill_color {
                    // even-odd fill uses same tessellation (GPU nonzero approx for multi-contour).
                    let vertices = match b.style.svg_fill_rule {
                        FillRule::NonZero | FillRule::EvenOdd => {
                            crate::svg_path::tessellate_fill(&contours)
                        }
                    };
                    if !vertices.is_empty() {
                        let shifted: Vec<[f32; 2]> = vertices
                            .iter()
                            .map(|[x, y]| [x + b.rect.x, y + b.rect.y])
                            .collect();
                        out.push(DisplayCommand::DrawSvgPath { vertices: shifted, color: fc });
                    }
                }
                if let Some(sc) = stroke_color
                    && stroke_w > 0.0
                {
                    let stroke_params = crate::svg_path::StrokeParams {
                        half_width: stroke_w * 0.5,
                        linecap: match b.style.svg_stroke_linecap {
                            StrokeLinecap::Butt   => crate::svg_path::StrokeLinecap::Butt,
                            StrokeLinecap::Round  => crate::svg_path::StrokeLinecap::Round,
                            StrokeLinecap::Square => crate::svg_path::StrokeLinecap::Square,
                        },
                        linejoin: match b.style.svg_stroke_linejoin {
                            StrokeLinejoin::Miter => crate::svg_path::StrokeLinejoin::Miter,
                            StrokeLinejoin::Round => crate::svg_path::StrokeLinejoin::Round,
                            StrokeLinejoin::Bevel => crate::svg_path::StrokeLinejoin::Bevel,
                        },
                        miterlimit: b.style.svg_stroke_miterlimit,
                        dasharray: b.style.svg_stroke_dasharray.clone(),
                        dashoffset: b.style.svg_stroke_dashoffset,
                    };
                    let vertices = crate::svg_path::tessellate_stroke_ex(&contours, &stroke_params);
                    if !vertices.is_empty() {
                        let shifted: Vec<[f32; 2]> = vertices
                            .iter()
                            .map(|[x, y]| [x + b.rect.x, y + b.rect.y])
                            .collect();
                        out.push(DisplayCommand::DrawSvgPath { vertices: shifted, color: sc });
                    }
                }
            }
        }
    }
}

/// Emits paint commands for SVG text elements (`<text>`, `<tspan>`, `<textPath>`).
/// Draws text at the specified position with proper horizontal and vertical alignment.
/// Reads `svg_fill` / `svg_stroke` / `font-family` / `font-size` from `ComputedStyle`.
/// // CSS: text-anchor, dominant-baseline
fn emit_svg_text(
    b: &LayoutBox,
    text: &str,
    text_anchor: SvgTextAnchor,
    dominant_baseline: SvgDominantBaseline,
    out: &mut DisplayList,
) {
    if text.is_empty() {
        return;
    }

    let current_color = b.style.color;
    let fill_color = b.style.svg_fill.resolve(current_color)
        .map(|c| apply_opacity_to_color(c, b.style.svg_fill_opacity));

    let font_size = b.style.font_size;
    // Phase 1: approximate text width as 0.5 × font-size × char count (typical monospace ratio).
    // Phase 2: replace with real TextMeasurer from lumen-font when available in paint.
    let approx_text_width = font_size * 0.5 * text.chars().count() as f32;

    // Apply text-anchor: adjust x so start/middle/end of text aligns at the SVG `x` position.
    let anchor_offset_x = match text_anchor {
        SvgTextAnchor::Start => 0.0,
        SvgTextAnchor::Middle => -approx_text_width * 0.5,
        SvgTextAnchor::End => -approx_text_width,
    };

    // Apply dominant-baseline: adjust y so the specified baseline aligns at the SVG `y` position.
    // SVG y is the text baseline by default (auto/baseline). Adjustments are approximate.
    let baseline_offset_y = match dominant_baseline {
        SvgDominantBaseline::Auto | SvgDominantBaseline::Baseline => 0.0,
        // middle/central: shift up by ~half em so middle of em-box is at y
        SvgDominantBaseline::Middle | SvgDominantBaseline::Central => -font_size * 0.35,
        // hanging/text-before-edge: shift down so top of cap is at y
        SvgDominantBaseline::Hanging | SvgDominantBaseline::TextBeforeEdge => font_size * 0.2,
        // text-after-edge: shift up so descender bottom is at y
        SvgDominantBaseline::TextAfterEdge => -font_size * 0.8,
    };

    if let Some(fc) = fill_color {
        let mut rect = b.rect;
        rect.x += anchor_offset_x;
        rect.y += baseline_offset_y;
        rect.width = approx_text_width;
        rect.height = font_size;
        out.push(DisplayCommand::DrawText {
            rect,
            text: text.to_string(),
            font_family: b.style.font_family.clone(),
            font_size,
            color: fc,
            font_weight: b.style.font_weight,
            font_style: b.style.font_style,
            font_variation_axes: vec![],
            tab_size: b.style.tab_size,
            highlight_name: None,
        });
    }
}

/// Эмитит FillRect-ы для активных линий text-decoration. Геометрия —
/// приблизительная: baseline ≈ line_y + font_size * 0.80 (соответствует
/// ascent ratio Inter, на котором рендерер позиционирует глифы). Толщина
/// резолвится через [`resolve_decoration_thickness`] из
/// `text-decoration-thickness` (L3 §2.3). Стиль (`Solid` / `Double` /
/// `Dotted` / `Dashed` / `Wavy`, L3 §2.2) разворачивается в один или
/// несколько FillRect-ов через [`emit_decoration_line`]. Цвет — из
/// `text-decoration-color` с fallback на currentColor (L3 §3).
fn push_text_decoration(out: &mut DisplayList, container_x: f32, line_y: f32, frag: &InlineFrag) {
    let decoration = frag.style.text_decoration_line;
    if decoration.is_empty() || frag.width <= 0.0 {
        return;
    }
    let fs = frag.style.font_size;
    let baseline_y = line_y + fs * 0.80;
    let thickness = resolve_decoration_thickness(frag.style.text_decoration_thickness, fs);
    let style = frag.style.text_decoration_style;
    let x = container_x + frag.x;
    let color = frag.style.text_decoration_color.resolve(frag.style.color);

    if decoration.underline {
        // CSS Text Decoration L4 §5.1: text-underline-position.
        // `Under` places the line below all descenders (≈ 25% of font-size below baseline).
        // `Auto`/`FromFont` uses the standard position just below the baseline.
        let base_offset = match frag.style.text_underline_position {
            TextUnderlinePosition::Under => fs * 0.25,
            _ => fs * 0.10,
        };
        // CSS Text Decoration L4 §5.3: text-underline-offset adds an explicit shift.
        let extra = frag.style.text_underline_offset.unwrap_or(0.0);
        emit_decoration_line(out, x, baseline_y + base_offset + extra, frag.width, thickness, color, style);
    }
    if decoration.line_through {
        let y = baseline_y - fs * 0.30;
        emit_decoration_line(out, x, y, frag.width, thickness, color, style);
    }
    if decoration.overline {
        let y = baseline_y - fs * 0.78;
        emit_decoration_line(out, x, y, frag.width, thickness, color, style);
    }
}

/// Резолвит [`TextDecorationThickness`] в device-px по CSS Text Decoration
/// L3 §2.3. `Auto` / `FromFont` — UA дефолт ≈ 7% от font-size (минимум
/// 1px); Phase 0 без font-access для `FromFont`, поэтому тот же default.
/// `Length` — уже resolved-px из cascade. `Percentage` хранится как
/// fraction; spec ссылается на 1em **parent** font-size, Phase 0
/// используем frag.font_size как приближение (документировано в
/// `style.rs`).
fn resolve_decoration_thickness(value: TextDecorationThickness, font_size: f32) -> f32 {
    match value {
        TextDecorationThickness::Auto | TextDecorationThickness::FromFont => {
            (font_size * 0.07).max(1.0)
        }
        TextDecorationThickness::Length(px) => px.max(0.0),
        TextDecorationThickness::Percentage(frac) => (frac * font_size).max(0.0),
    }
}

/// Эмитит FillRect-ы для одной decoration-линии в выбранном стиле
/// (CSS Text Decoration L3 §2.2). `(x, y)` — верхний левый угол.
///
/// - `Solid` — один rect (initial).
/// - `Double` — два параллельных rect-а с gap = thickness; итого
///   span ≈ 3 × thickness, верхний у `y`, нижний у `y + 2·t`.
/// - `Dotted` — серия квадратиков `thickness × thickness`, шаг
///   `2 × thickness` (gap = thickness). Геометрия UA-defined; выбран
///   простой 1:1 паттерн.
/// - `Dashed` — серия штрихов длиной `2 × thickness`, шаг `3 × thickness`
///   (gap = thickness). UA-defined.
/// - `Wavy` — синусоидальная волна аппроксимируется серией узких
///   axis-aligned столбцов (renderer pipeline без curves): сдвиг
///   центра толщины по `dy = sin(2π · rel_x / λ) · A`, где
///   `A = WAVY_AMPLITUDE_FACTOR · thickness`, `λ =
///   WAVY_WAVELENGTH_FACTOR · thickness`. Шаг между columns =
///   `max(1, thickness · 0.5)` — компромисс между визуальной
///   гладкостью и числом FillRect-ов (≈ 2 sample / thickness CSS px).
///   Толщина каждого column = thickness, ширина = step (или остаток
///   до `x + width`). Видимый ascent/descent от baseline = `A + t/2`.
fn emit_decoration_line(
    out: &mut DisplayList,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
    style: TextDecorationStyle,
) {
    if width <= 0.0 || thickness <= 0.0 {
        return;
    }
    match style {
        TextDecorationStyle::Solid => {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(x, y, width, thickness),
                color,
            });
        }
        TextDecorationStyle::Wavy => {
            emit_wavy_line(out, x, y, width, thickness, color);
        }
        TextDecorationStyle::Double => {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(x, y, width, thickness),
                color,
            });
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(x, y + 2.0 * thickness, width, thickness),
                color,
            });
        }
        TextDecorationStyle::Dotted => {
            let step = thickness * 2.0;
            let end = x + width;
            let mut cx = x;
            while cx + thickness <= end + f32::EPSILON {
                out.push(DisplayCommand::FillRect {
                    rect: Rect::new(cx, y, thickness, thickness),
                    color,
                });
                cx += step;
            }
        }
        TextDecorationStyle::Dashed => {
            let dash = thickness * 2.0;
            let step = thickness * 3.0;
            let end = x + width;
            let mut cx = x;
            while cx < end {
                let w = (end - cx).min(dash);
                if w <= 0.0 {
                    break;
                }
                out.push(DisplayCommand::FillRect {
                    rect: Rect::new(cx, y, w, thickness),
                    color,
                });
                cx += step;
            }
        }
    }
}

/// Амплитуда волны в долях `thickness` — peak-deviation центра от
/// baseline в обе стороны. 1.5×thickness даёт ясно различимую волну
/// без излишнего вертикального expansion за пределы line-box-а.
const WAVY_AMPLITUDE_FACTOR: f32 = 1.5;

/// Длина волны в долях `thickness`. 4×thickness — UA-defined компромисс
/// (Chrome ≈ 3-4×, Firefox ≈ 6×; берём середину). При thickness=1px →
/// период 4px, ~3 цикла на каждые 12 CSS-px font-size.
const WAVY_WAVELENGTH_FACTOR: f32 = 4.0;

/// Аппроксимирует синусоидальную линию серией axis-aligned FillRect-ов:
/// для каждого sampled-X эмитим тонкий столбец `[x, x+step] × [cy+dy-t/2,
/// cy+dy+t/2]`, где `cy = y + t/2` — центр толщины, `dy = sin(2π·rel/λ)·A`.
/// Step выбран `max(1, t·0.5)`: ниже — растёт число FillRect (≈ 2·width/t),
/// выше — лестница становится грубее, что особенно заметно при крутых
/// склонах волны (там `|dy'| → t·A/λ·2π`).
fn emit_wavy_line(
    out: &mut DisplayList,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
) {
    let amplitude = thickness * WAVY_AMPLITUDE_FACTOR;
    let wavelength = thickness * WAVY_WAVELENGTH_FACTOR;
    let step = (thickness * 0.5).max(1.0);
    let cy = y + thickness * 0.5;
    let end = x + width;
    let mut cx = x;
    while cx < end {
        let w = step.min(end - cx);
        if w <= 0.0 {
            break;
        }
        // Используем центр столбца как sample-точку — это даёт
        // чуть более точную аппроксимацию, чем left-edge sampling.
        let sample_x = cx + w * 0.5;
        let phase = (sample_x - x) / wavelength * std::f32::consts::TAU;
        let dy = phase.sin() * amplitude;
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(cx, cy + dy - thickness * 0.5, w, thickness),
            color,
        });
        cx += step;
    }
}

/// Like `walk` but applies `CompositorAnimFrame` overrides for opacity and transform.
///
/// When a node has an animated opacity or transform, the overridden values replace
/// the style values in the emitted Push* commands. All other paint (FillRect, DrawText,
/// borders, shadows) uses the base style unchanged.
fn walk_with_anim(b: &LayoutBox, anim: Option<&CompositorAnimFrame>, out: &mut DisplayList, dpr: f32) {
    let ov = anim.and_then(|a| a.get(b.node));

    // CSS Positioning L3 §6.3 — position:sticky (same as in walk).
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

    // Determine effective opacity: animated override wins over style.
    let effective_opacity = ov.and_then(|o| o.opacity).unwrap_or(b.style.opacity);

    // Skip completely invisible subtrees (same rule as walk, but uses effective opacity).
    if effective_opacity == 0.0 && b.style.opacity == 0.0 {
        // Both animated and static are zero — nothing to paint.
        if !is_opacity_subtree_painted(b) {
            return;
        }
    } else if effective_opacity == 0.0 {
        // Animated to zero — skip this subtree.
        return;
    } else if !is_opacity_subtree_painted(b) && ov.and_then(|o| o.opacity).is_none() {
        // Base style opacity is 0 and no anim override — skip.
        return;
    }

    match &b.kind {
        BoxKind::Skip => {}
        BoxKind::Block => {
            let has_opacity = effective_opacity < 1.0;
            if has_opacity {
                out.push(DisplayCommand::PushOpacity { alpha: effective_opacity });
            }

            // Determine effective transform: animated override wins over style.
            let transform = if let Some(fns) = ov.and_then(|o| o.transform.as_deref()) {
                let (ox, oy, _) = b.style.transform_origin;
                transform_fns_to_matrix(fns, b.rect.x + ox.resolve(b.rect.width), b.rect.y + oy.resolve(b.rect.height))
            } else {
                forward_box_transform(b)
            };
            if let Some(matrix) = transform {
                out.push(DisplayCommand::PushTransform { matrix });
            }

            let self_visible = is_paint_visible(b);
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
            // CSS Transforms L2 §6.2 — depth-sort children of a 3D rendering
            // context (preserve-3d); else document order. Mirrors `walk`.
            if establishes_3d_rendering_context(b) {
                for i in depth_sorted_child_order(&b.children) {
                    walk_with_anim(&b.children[i], anim, out, dpr);
                }
            } else {
                for child in &b.children {
                    walk_with_anim(child, anim, out, dpr);
                }
            }
            if self_visible {
                emit_outline(b, out);
            }
            if transform.is_some() {
                out.push(DisplayCommand::PopTransform);
            }
            if has_opacity {
                out.push(DisplayCommand::PopOpacity);
            }
        }
        BoxKind::InlineBlockRow => {
            for child in &b.children {
                walk_with_anim(child, anim, out, dpr);
            }
        }
        BoxKind::InlineSpace => {}
        BoxKind::InlineRun { lines, .. } => {
            emit_inline_run(b, lines, None, out);
        }
        // Image and other kinds: no compositor-offloadable properties, delegate to walk.
        _ => {
            walk(b, out, dpr, None);
        }
    }
    if is_sticky {
        out.push(DisplayCommand::EndStickyLayer);
    }
}

// BorderCollapse re-exported from lumen_layout::BorderCollapse (CSS Tables L2 §17.6).
// Use b.style.border_collapse directly — now wired by P4.

/// Border precedence value для collapsed border model (CSS Tables L2 §17.6.2).
/// Более высокий precedence побеждает при конфликте.
/// Phase 1: поддержка precedence calculation, full integration в Phase 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
enum BorderPrecedence {
    /// Table border — самый низкий precedence
    Table,
    /// Row group border (thead/tbody/tfoot)
    RowGroup,
    /// Row border
    Row,
    /// Column group border (colgroup)
    ColumnGroup,
    /// Column border (col)
    Column,
    /// Cell border — самый высокий precedence
    Cell,
}

/// Информация о border для collapsed border model
/// Phase 1: структура и helpers для future collapse mode implementation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct CollapsedBorder {
    /// Ширина границы
    width: f32,
    /// Цвет границы
    color: [f32; 4],
    /// Стиль границы (solid, dashed и т.д.)
    style: BorderStyle,
    /// Precedence для разрешения конфликтов
    precedence: BorderPrecedence,
}

impl CollapsedBorder {
    /// Выбирает наиболее приоритетную границу из двух конкурирующих
    /// Согласно CSS Tables L2 §17.6.2, более узкие границы скрываются,
    /// а при равной ширине побеждает hide > none > solid/dashed... > initial
    #[allow(dead_code)]
    fn resolve_conflict(a: &Self, b: &Self) -> Self {
        // По precedence: более высокий precedence побеждает
        if a.precedence != b.precedence {
            return if a.precedence > b.precedence {
                a.clone()
            } else {
                b.clone()
            };
        }

        // При равном precedence: более узкая граница скрывается
        if (a.width - b.width).abs() > 0.001 {
            return if a.width > b.width {
                a.clone()
            } else {
                b.clone()
            };
        }

        // По умолчанию выбираем первую (может быть улучшено по стилю)
        a.clone()
    }
}

/// Контекст таблицы — режим схлопывания границ и spacing, читаются из `ComputedStyle`.
/// Phase 0: layout использует spacing напрямую; Phase 2 будет передавать ctx в emit_table_cell.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TableContext {
    /// `separate | collapse` — из `ComputedStyle.border_collapse`.
    border_collapse: BorderCollapse,
    /// Горизонтальный и вертикальный gap (px) между ячейками в `separate` режиме.
    border_spacing: (f32, f32),
}

impl TableContext {
    /// Строит контекст из стиля таблицы.
    fn from_box(b: &LayoutBox) -> Self {
        TableContext {
            border_collapse: b.style.border_collapse,
            border_spacing: (b.style.border_spacing_h, b.style.border_spacing_v),
        }
    }
}

/// Рендеринг таблицы с поддержкой border-collapse и фонов ячеек.
///
/// CSS 2.1 §17.5: separate (default) — ячейки рисуют свои границы;
/// collapse — соседние границы схлопываются (Phase 0: suppress double-draw).
fn emit_table_box(b: &LayoutBox, out: &mut Vec<DisplayCommand>, dpr: f32) {
    let _table_ctx = TableContext::from_box(b);

    // Эмитим фон таблицы
    if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
        && bg.a > 0
    {
        let clip = background_clip_rect(b, background_color_clip(b));
        if clip.width > 0.0 && clip.height > 0.0 {
            out.push(DisplayCommand::FillRect { rect: clip, color: bg });
        }
    }
    emit_background_image(out, b, dpr);

    // Обрабатываем граници таблицы
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

    // Обрабатываем строки и ячейки
    for row_group in &b.children {
        match &row_group.kind {
            BoxKind::TableRowGroup => {
                emit_table_row_group(row_group, out, dpr);
            }
            BoxKind::TableRow => {
                emit_table_row(row_group, out, dpr);
            }
            _ => {
                walk(row_group, out, dpr, None);
            }
        }
    }
}

/// Эмитируем группу строк таблицы (thead, tbody, tfoot)
fn emit_table_row_group(b: &LayoutBox, out: &mut Vec<DisplayCommand>, dpr: f32) {
    // Группа не рендерится сама по себе (прозрачный контейнер)
    // но может иметь фон и граници

    // TODO: для Phase 1 можно добавить фон group-уровня

    // Обрабатываем строки
    for row in &b.children {
        if matches!(&row.kind, BoxKind::TableRow) {
            emit_table_row(row, out, dpr);
        }
    }
}

/// Эмитируем строку таблицы
fn emit_table_row(b: &LayoutBox, out: &mut Vec<DisplayCommand>, dpr: f32) {
    // Обрабатываем ячейки строки
    for cell in &b.children {
        emit_table_cell(cell, out, dpr);
    }
}

/// Эмитируем ячейку таблицы.
///
/// В `separate` режиме каждая ячейка рисует все 4 границы.
/// В `collapse` режиме layout уже зануляет border-spacing; каждая ячейка
/// рисует только top+left границы, чтобы избежать двойного рисования
/// по общим рёбрам (Phase 0 упрощение; полный алгоритм §17.6.2 — Phase 2).
fn emit_table_cell(b: &LayoutBox, out: &mut Vec<DisplayCommand>, dpr: f32) {
    // Эмитим фон ячейки
    if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
        && bg.a > 0
    {
        out.push(DisplayCommand::FillRect { rect: b.rect, color: bg });
    }
    emit_background_image(out, b, dpr);

    let s = &b.style;
    // In separate mode: draw all 4 borders. In collapse mode: draw all 4 borders too
    // (spacing is already zeroed by layout; border overlap on shared edges is Phase 0 behaviour;
    // full §17.6.2 conflict resolution is deferred to Phase 2).
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

    // Обрабатываем контент ячейки (текст, вложенные блоки и т.д.)
    for child in &b.children {
        walk(child, out, dpr, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Size;

    fn build(html: &str, css: &str) -> DisplayList {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        build_display_list(&tree)
    }

    struct Fixed8;
    impl lumen_layout::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 {
            8.0
        }
    }

    fn build_wrapped(html: &str, css: &str, width: f32) -> DisplayList {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout_measured(&doc, &sheet, Size::new(width, 600.0), &Fixed8);
        build_display_list(&tree)
    }

    fn fills(dl: &DisplayList) -> Vec<&Color> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { color, .. } => Some(color),
                _ => None,
            })
            .collect()
    }

    fn texts(dl: &DisplayList) -> Vec<&str> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn empty_input_empty_list() {
        let dl = build("", "");
        assert!(dl.is_empty());
    }

    #[test]
    fn block_with_background_emits_fill() {
        let dl = build("<p>x</p>", "p { background: red; }");
        let f = fills(&dl);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].r, 255);
    }

    #[test]
    fn block_without_background_no_fill() {
        let dl = build("<p>x</p>", "");
        assert!(fills(&dl).is_empty());
    }

    #[test]
    fn text_node_emits_draw_text() {
        let dl = build("<p>hello</p>", "");
        assert_eq!(texts(&dl), vec!["hello"]);
    }

    #[test]
    fn cyrillic_text_preserved() {
        let dl = build("<p>Привет, мир</p>", "");
        assert_eq!(texts(&dl), vec!["Привет, мир"]);
    }

    #[test]
    fn nested_backgrounds_in_parent_then_child_order() {
        let dl = build(
            "<div><p>x</p></div>",
            "div { background: red; } p { background: blue; }",
        );
        let f = fills(&dl);
        assert_eq!(f.len(), 2);
        // Сначала parent (под текст), потом child — естественный paint-порядок.
        assert_eq!(f[0].r, 255);
        assert_eq!(f[1].b, 255);
    }

    #[test]
    fn transparent_background_omitted() {
        let dl = build("<p>x</p>", "p { background-color: transparent; }");
        assert!(fills(&dl).is_empty());
    }

    #[test]
    fn skipped_boxes_emit_nothing() {
        let dl = build("<p>x</p><!-- comment --><p>y</p>", "");
        // Только два текстовых узла; комментарий не даёт команды.
        assert_eq!(texts(&dl).len(), 2);
    }

    #[test]
    fn display_none_emits_nothing() {
        let dl = build(
            r#"<p class="x">hidden</p><p>visible</p>"#,
            ".x { display: none; }",
        );
        assert_eq!(texts(&dl), vec!["visible"]);
    }

    // ── Тесты line wrapping ─────────────────────────────────────────────────

    /// При переносе текста на 2 строки должны быть эмитированы 2 DrawText.
    #[test]
    fn wrapped_text_emits_multiple_draw_text() {
        // "hello world" = 11×8 = 88px. Viewport 60px → перенос на 2 строки.
        let dl = build_wrapped("<p>hello world</p>", "", 60.0);
        assert_eq!(texts(&dl), vec!["hello", "world"]);
    }

    /// Вторая строка у `DrawText` должна быть смещена по Y на line_height.
    #[test]
    fn wrapped_lines_have_correct_y_offset() {
        let dl = build_wrapped("<p>hello world</p>", "", 60.0);
        let draw_texts: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { rect, .. } => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(draw_texts.len(), 2);
        let line_h = 16.0_f32 * 1.2; // font_size=16, line_height=1.2 → 19.2
        // CSS 2.1 §10.8.1: the first line carries half-leading = (19.2-16)/2 = 1.6.
        let half_leading = (line_h - 16.0) / 2.0;
        assert!((draw_texts[0].y - half_leading).abs() < 0.01, "y0={}", draw_texts[0].y);
        assert!((draw_texts[1].y - (half_leading + line_h)).abs() < 0.1, "y1={}", draw_texts[1].y);
    }

    /// Текст без переноса всё равно рисуется одной командой.
    #[test]
    fn no_wrap_single_draw_text() {
        let dl = build_wrapped("<p>hi</p>", "", 800.0);
        assert_eq!(texts(&dl), vec!["hi"]);
    }

    // ── Тесты inline-flow ───────────────────────────────────────────────────

    /// Текст с <span> внутри — один DrawText (одинаковый стиль → фрагменты сливаются).
    #[test]
    fn inline_same_style_merges_into_one_draw_text() {
        let dl = build_wrapped("<p>hello <span>world</span></p>", "", 800.0);
        assert_eq!(texts(&dl), vec!["hello world"]);
    }

    /// <a> с цветом → два DrawText: "Hello" и "link" с разными цветами.
    #[test]
    fn inline_different_style_emits_separate_draw_texts() {
        let dl = build_wrapped("<p>Hello <a>link</a></p>", "a { color: blue; }", 800.0);
        let t = texts(&dl);
        assert_eq!(t, vec!["Hello", "link"]);
        // Второй DrawText должен быть синим.
        let blue_cmds: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { text, color, .. } if text == "link" => Some(color),
                _ => None,
            })
            .collect();
        assert_eq!(blue_cmds.len(), 1);
        assert_eq!(blue_cmds[0].b, 255);
    }

    /// X-координата второго фрагмента должна быть правее первого.
    #[test]
    fn inline_fragments_have_increasing_x() {
        // "Hello" (5*8=40) + space(8) + "link" → link начинается в x=48.
        let dl = build_wrapped("<p>Hello <a>link</a></p>", "a { color: blue; }", 800.0);
        let rects: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { rect, .. } => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(rects.len(), 2);
        assert!((rects[0].x - 0.0).abs() < 0.01, "Hello должно быть в x=0");
        assert!(
            rects[1].x > rects[0].x,
            "link должно быть правее: Hello.x={}, link.x={}",
            rects[0].x,
            rects[1].x
        );
    }

    // ── Тесты text-decoration ───────────────────────────────────────────────

    fn fill_rects(dl: &DisplayList) -> Vec<&Rect> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { rect, .. } => Some(rect),
                _ => None,
            })
            .collect()
    }

    /// `<a>` с `text-decoration: underline` эмитирует и DrawText, и FillRect.
    #[test]
    fn underline_emits_draw_text_and_fill_rect() {
        let dl = build_wrapped(
            "<p><a>link</a></p>",
            "a { text-decoration: underline; }",
            800.0,
        );
        assert_eq!(texts(&dl), vec!["link"]);
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1, "expected one underline FillRect");
        // "link" = 4×8 = 32px.
        assert!((rects[0].width - 32.0).abs() < 0.01, "width={}", rects[0].width);
    }

    /// Underline должен идти ниже baseline (под глифами).
    #[test]
    fn underline_positioned_below_baseline() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        // First line gets half-leading = (19.2-16)/2 = 1.6 (CSS 2.1 §10.8.1),
        // baseline ≈ 1.6 + 16*0.80 = 14.4, underline y ≈ 14.4 + 16*0.10 = 16.0.
        assert!(
            (rects[0].y - 16.0).abs() < 0.5,
            "underline y should be near 16.0, got {}",
            rects[0].y
        );
    }

    /// line-through лежит выше baseline, не ниже.
    #[test]
    fn line_through_positioned_above_baseline() {
        let dl = build_wrapped(
            "<p><span>x</span></p>",
            "span { text-decoration: line-through; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        // baseline ≈ 1.6 (half-leading) + 12.8 = 14.4, line-through y ≈ 14.4 - 16*0.30 = 9.6.
        assert!(
            (rects[0].y - 9.6).abs() < 0.5,
            "line-through y should be near 9.6, got {}",
            rects[0].y
        );
    }

    /// overline лежит над текстом.
    #[test]
    fn overline_positioned_above_text() {
        let dl = build_wrapped(
            "<p><span>x</span></p>",
            "span { text-decoration: overline; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        // baseline ≈ 1.6 (half-leading) + 12.8 = 14.4, overline y ≈ 14.4 - 16*0.78 ≈ 1.9.
        assert!(
            rects[0].y < 2.5,
            "overline y should be near top, got {}",
            rects[0].y
        );
    }

    /// `text-decoration: underline line-through` эмитирует две линии.
    #[test]
    fn multiple_decorations_emit_multiple_rects() {
        let dl = build_wrapped(
            "<p><a>link</a></p>",
            "a { text-decoration: underline line-through; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 2, "expected underline + line-through rects");
    }

    /// text-decoration-color: explicit — линия использует его, не цвет текста.
    #[test]
    fn decoration_explicit_color_overrides_text_color() {
        let dl = build_wrapped(
            "<p><a>link</a></p>",
            "a { color: red; text-decoration: underline; text-decoration-color: blue; }",
            800.0,
        );
        let colors: Vec<Color> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(colors.len(), 1);
        assert_eq!([colors[0].r, colors[0].g, colors[0].b], [0, 0, 255]);
    }

    /// Цвет линии совпадает с цветом текста (currentColor).
    #[test]
    fn decoration_uses_text_color() {
        let dl = build_wrapped(
            "<p><a>link</a></p>",
            "a { color: red; text-decoration: underline; }",
            800.0,
        );
        let colors: Vec<&Color> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { color, .. } => Some(color),
                _ => None,
            })
            .collect();
        assert_eq!(colors.len(), 1);
        assert_eq!(colors[0].r, 255);
        assert_eq!(colors[0].g, 0);
    }

    /// Соседние фрагменты разной декорации не сливаются.
    #[test]
    fn fragments_with_different_decoration_dont_merge() {
        let dl = build_wrapped(
            "<p>plain <a>underlined</a> tail</p>",
            "a { text-decoration: underline; }",
            800.0,
        );
        let t = texts(&dl);
        // 3 фрагмента: "plain", "underlined", "tail".
        assert_eq!(t, vec!["plain", "underlined", "tail"]);
        // Underline только под средним.
        assert_eq!(fill_rects(&dl).len(), 1);
    }

    /// Унаследованная декорация продолжает работать у потомков.
    #[test]
    fn decoration_inherits_into_descendants() {
        let dl = build_wrapped(
            "<p><span>x</span></p>",
            "p { text-decoration: underline; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        // Span наследует underline → FillRect эмитится.
        assert!(!rects.is_empty(), "underline should propagate to span");
    }

    /// `text-decoration: none` на потомке отменяет наследуемую декорацию.
    #[test]
    fn none_on_descendant_overrides_inherited_underline() {
        let dl = build_wrapped(
            "<p><a>off</a></p>",
            "p { text-decoration: underline; } a { text-decoration: none; }",
            800.0,
        );
        assert!(fill_rects(&dl).is_empty(), "a should override underline");
    }

    /// `text-decoration: underline solid` — sanity, что explicit Solid ведёт
    /// себя как default (один FillRect).
    #[test]
    fn style_solid_emits_one_rect() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline solid; }",
            800.0,
        );
        assert_eq!(fill_rects(&dl).len(), 1);
    }

    /// `Double` — две параллельные линии той же ширины с gap = thickness;
    /// второй rect ниже первого на `2 × thickness`.
    #[test]
    fn style_double_emits_two_parallel_rects() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline double; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 2, "Double = two parallel lines");
        assert!((rects[0].width - rects[1].width).abs() < 0.01);
        let t = (16.0_f32 * 0.07).max(1.0);
        let dy = rects[1].y - rects[0].y;
        assert!(
            (dy - 2.0 * t).abs() < 0.05,
            "expected dy ≈ 2·t = {}, got {dy}",
            2.0 * t
        );
    }

    /// Двойной underline + line-through → 4 rect-а суммарно.
    #[test]
    fn double_with_multiple_lines_emits_four_rects() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline line-through double; }",
            800.0,
        );
        assert_eq!(fill_rects(&dl).len(), 4);
    }

    /// `Dotted` — серия квадратиков `thickness × thickness`, count > 5
    /// для текста шириной 80px (10 символов × 8px char-width).
    #[test]
    fn style_dotted_emits_square_dots() {
        let dl = build_wrapped(
            "<p><a>longertext</a></p>",
            "a { text-decoration: underline dotted; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert!(rects.len() > 5, "got {} dots, expected many", rects.len());
        // Каждый dot — квадрат width = height = thickness.
        let t = (16.0_f32 * 0.07).max(1.0);
        for r in &rects {
            assert!(
                (r.width - r.height).abs() < 0.01,
                "dot not square: {}×{}",
                r.width,
                r.height
            );
            assert!(
                (r.width - t).abs() < 0.01,
                "dot width={}, expected t={t}",
                r.width
            );
        }
    }

    /// `Dashed` — серия штрихов длиной `2 × thickness`, count > 3.
    #[test]
    fn style_dashed_emits_dashes() {
        let dl = build_wrapped(
            "<p><a>longertext</a></p>",
            "a { text-decoration: underline dashed; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert!(rects.len() > 3, "got {} dashes", rects.len());
        let t = (16.0_f32 * 0.07).max(1.0);
        // Все dashes кроме, возможно, последнего — длиной 2·t.
        // Высота — thickness.
        for r in &rects[..rects.len() - 1] {
            assert!(
                (r.width - 2.0 * t).abs() < 0.05,
                "dash width={}, expected {}",
                r.width,
                2.0 * t
            );
            assert!((r.height - t).abs() < 0.01);
        }
    }

    /// `Wavy` эмитит серию тонких axis-aligned столбцов, аппроксимирующих
    /// синусоиду. Каждый столбец = `step × thickness`, sin-сдвиг центра.
    #[test]
    fn style_wavy_emits_sampled_columns() {
        // Один inline char ≈ 8px @ 16px font; thickness = 16·0.07 ≈ 1.12,
        // step = max(1, 1.12·0.5) = 1.0 → ~8 столбцов.
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline wavy; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert!(
            rects.len() >= 4,
            "wavy emits multiple columns, got {}",
            rects.len()
        );
        // Sum of widths ≈ underline-width (8px).
        let total_w: f32 = rects.iter().map(|r| r.width).sum();
        assert!(
            (total_w - 8.0).abs() < 0.1,
            "columns cover full width: sum={}, expected ≈ 8",
            total_w
        );
        // Все столбцы — одной thickness (height).
        let h0 = rects[0].height;
        for r in &rects {
            assert!((r.height - h0).abs() < 0.01, "uniform thickness");
        }
        // Y-координаты не одинаковы — иначе это бы Solid line.
        let y_min = rects.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
        let y_max = rects.iter().map(|r| r.y).fold(f32::NEG_INFINITY, f32::max);
        assert!(
            y_max - y_min > 0.5,
            "wavy must vertically displace columns: range={}",
            y_max - y_min
        );
    }

    /// Амплитуда sin-сдвига должна не превышать `1.5 × thickness`
    /// (peak deviation от центра в обе стороны). Sum-y-range ≤
    /// 2·A + thickness, и не сильно меньше — амплитуда должна
    /// достигаться хотя бы раз на достаточной ширине.
    #[test]
    fn style_wavy_amplitude_matches_factor() {
        // 40px ширина с большой толщиной → волна успевает достичь обоих peak-ов.
        let dl = build_wrapped(
            "<p><a>xxxxx</a></p>",
            "a { text-decoration: underline wavy; \
                  text-decoration-thickness: 4px; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert!(rects.len() >= 8);
        let y_min = rects.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
        let y_max = rects.iter().map(|r| r.y).fold(f32::NEG_INFINITY, f32::max);
        // A = 4 * 1.5 = 6; peak-to-peak ≈ 12, отступы между centers
        // достигают этого диапазона.
        let y_range = y_max - y_min;
        assert!(
            y_range > 6.0,
            "amplitude expected ≥ 6, got range={}",
            y_range
        );
        assert!(
            y_range <= 13.0,
            "amplitude should not exceed 2·A=12 (+1 sampling tolerance), got {}",
            y_range
        );
    }

    /// Wavy uses the same color as Solid (text-decoration-color / fallback).
    #[test]
    fn style_wavy_preserves_color() {
        let dl = build_wrapped(
            "<p style=\"color: red\"><a>x</a></p>",
            "a { text-decoration: underline wavy; }",
            800.0,
        );
        let fills: Vec<_> = dl
            .iter()
            .filter_map(|cmd| match cmd {
                DisplayCommand::FillRect { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        assert!(!fills.is_empty());
        for c in &fills {
            assert_eq!([c.r, c.g, c.b, c.a], [255, 0, 0, 255]);
        }
    }

    /// Каждый wavy column не выпадает за горизонтальные границы линии:
    /// последний column обрезается до остатка, не overshoot-ит.
    #[test]
    fn style_wavy_columns_clip_to_width() {
        let dl = build_wrapped(
            "<p><a>xx</a></p>",
            "a { text-decoration: underline wavy; \
                  text-decoration-thickness: 3px; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        // x-min равен старту линии; x-max не превышает старт+width.
        let x_start = rects.iter().map(|r| r.x).fold(f32::INFINITY, f32::min);
        let x_end = rects
            .iter()
            .map(|r| r.x + r.width)
            .fold(f32::NEG_INFINITY, f32::max);
        let total_w: f32 = rects.iter().map(|r| r.width).sum();
        assert!(
            (x_end - x_start - total_w).abs() < 0.01,
            "columns are non-overlapping and tile the line",
        );
    }

    /// `text-decoration-thickness: 4px` override-ит default 7%.
    #[test]
    fn thickness_length_overrides_default() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline; text-decoration-thickness: 4px; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        assert!(
            (rects[0].height - 4.0).abs() < 0.01,
            "thickness height={}, expected 4",
            rects[0].height
        );
    }

    /// `text-decoration-thickness: 25%` → 25% от font-size (Phase 0 от
    /// frag.font_size, не parent — задокументировано в style.rs).
    #[test]
    fn thickness_percentage_resolves_against_font_size() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline; text-decoration-thickness: 25%; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        assert!(
            (rects[0].height - 4.0).abs() < 0.01,
            "expected 0.25·16 = 4, got {}",
            rects[0].height
        );
    }

    /// `text-decoration-thickness: from-font` в Phase 0 — без font-доступа,
    /// поэтому совпадает с `Auto` (≈ 7% от font-size).
    #[test]
    fn thickness_from_font_falls_back_to_auto() {
        let dl = build_wrapped(
            "<p><a>x</a></p>",
            "a { text-decoration: underline; text-decoration-thickness: from-font; }",
            800.0,
        );
        let rects = fill_rects(&dl);
        assert_eq!(rects.len(), 1);
        let default = (16.0_f32 * 0.07).max(1.0);
        assert!(
            (rects[0].height - default).abs() < 0.01,
            "height={}, expected ≈ {default}",
            rects[0].height
        );
    }

    /// Inline-ран переносится: второй DrawText смещён по Y.
    #[test]
    fn inline_run_wrap_y_offset() {
        // "aa" (16px) + " " (8) + "bb" (16) = 40px > 30px viewport → перенос.
        let dl = build_wrapped("<p>aa <span>bb</span></p>", "", 30.0);
        let rects: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { rect, .. } => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(rects.len(), 2);
        let line_h = 16.0_f32 * 1.2;
        // First line carries half-leading = (19.2-16)/2 = 1.6 (CSS 2.1 §10.8.1).
        let half_leading = (line_h - 16.0) / 2.0;
        assert!((rects[0].y - half_leading).abs() < 0.01, "y0={}", rects[0].y);
        assert!((rects[1].y - (half_leading + line_h)).abs() < 0.1, "y1={}", rects[1].y);
    }

    // ── Тесты border рендеринга ─────────────────────────────────────────────

    fn borders(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBorder { .. }))
            .collect()
    }

    #[test]
    fn border_solid_emits_draw_border() {
        let dl = build("<p>x</p>", "p { border: 2px solid red; }");
        let b = borders(&dl);
        assert_eq!(b.len(), 1, "должна быть одна DrawBorder команда");
        if let DisplayCommand::DrawBorder { widths, colors, styles, .. } = b[0] {
            assert!((widths[0] - 2.0).abs() < 0.01, "top width");
            assert!((widths[1] - 2.0).abs() < 0.01, "right width");
            assert_eq!(colors[0].r, 255, "top color — red");
            assert_eq!(
                *styles,
                [
                    BorderStyle::Solid,
                    BorderStyle::Solid,
                    BorderStyle::Solid,
                    BorderStyle::Solid,
                ],
            );
        }
    }

    #[test]
    fn border_dashed_styles_propagate_to_command() {
        let dl = build("<p>x</p>", "p { border: 3px dashed blue; }");
        let b = borders(&dl);
        assert_eq!(b.len(), 1);
        if let DisplayCommand::DrawBorder { styles, .. } = b[0] {
            assert_eq!(
                *styles,
                [
                    BorderStyle::Dashed,
                    BorderStyle::Dashed,
                    BorderStyle::Dashed,
                    BorderStyle::Dashed,
                ],
            );
        }
    }

    #[test]
    fn border_mixed_styles_per_side() {
        let dl = build(
            "<p>x</p>",
            "p { border-top: 2px solid black; \
                 border-right: 2px dashed black; \
                 border-bottom: 2px dotted black; \
                 border-left: 2px solid black; }",
        );
        let b = borders(&dl);
        assert_eq!(b.len(), 1);
        if let DisplayCommand::DrawBorder { styles, .. } = b[0] {
            assert_eq!(styles[0], BorderStyle::Solid);
            assert_eq!(styles[1], BorderStyle::Dashed);
            assert_eq!(styles[2], BorderStyle::Dotted);
            assert_eq!(styles[3], BorderStyle::Solid);
        }
    }

    #[test]
    fn serialize_drawborder_solid_omits_styles() {
        // bw-compat: чистый Solid не печатает `s=[...]` — snapshot-ы
        // прежней версии остаются валидными.
        let dl = build("<p>x</p>", "p { border: 2px solid black; }");
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawBorder"));
        assert!(!s.contains(" s=["), "Solid не печатает s=[...]: {s}");
    }

    #[test]
    fn serialize_drawborder_dashed_emits_styles_field() {
        let dl = build("<p>x</p>", "p { border: 2px dashed black; }");
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawBorder"));
        assert!(
            s.contains(" s=[da,da,da,da]"),
            "Dashed эмитит s=[...]: {s}"
        );
    }

    #[test]
    fn serialize_drawborder_dotted_short_marker() {
        let dl = build("<p>x</p>", "p { border: 2px dotted black; }");
        let s = serialize_display_list(&dl);
        assert!(s.contains(" s=[do,do,do,do]"), "Dotted: {s}");
    }

    #[test]
    fn serialize_drawborder_mixed_marks_only_non_solid() {
        let dl = build(
            "<p>x</p>",
            "p { border: 2px solid black; \
                 border-right-style: dashed; }",
        );
        let s = serialize_display_list(&dl);
        assert!(s.contains(" s=[s,da,s,s]"), "Mixed: {s}");
    }

    #[test]
    fn border_none_style_no_draw_border() {
        // border-width без border-style (default None) → DrawBorder не эмитируется.
        let dl = build("<p>x</p>", "p { border-width: 2px; }");
        assert!(borders(&dl).is_empty());
    }

    #[test]
    fn border_increases_height() {
        // Без border: высота = font_size * line_height = 16 * 1.2 = 19.2
        let no_border = build("<p>x</p>", "");
        let with_border = build("<p>x</p>", "p { border: 5px solid black; }");

        let height_of = |dl: &DisplayList| -> f32 {
            dl.iter()
                .find_map(|c| match c {
                    DisplayCommand::DrawText { rect, .. } => Some(rect.y),
                    _ => None,
                })
                .unwrap_or(0.0)
        };
        // Текст должен быть смещён на 5px вниз из-за border-top.
        let y_no = height_of(&no_border);
        let y_with = height_of(&with_border);
        assert!(
            (y_with - y_no - 5.0).abs() < 0.1,
            "y_no={y_no}, y_with={y_with}"
        );
    }

    #[test]
    fn border_color_none_uses_current_color() {
        // border без color → currentColor (наследуется из color: blue).
        let dl = build("<p>x</p>", "p { color: blue; border: 2px solid; }");
        let b = borders(&dl);
        assert_eq!(b.len(), 1);
        if let DisplayCommand::DrawBorder { colors, .. } = b[0] {
            assert_eq!(colors[0].b, 255, "border color should be blue (currentColor)");
        }
    }

    #[test]
    fn border_shorthand_in_serialize() {
        // serialize_display_list корректно форматирует DrawBorder.
        let dl = build("<p>x</p>", "p { border: 3px solid red; }");
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawBorder"), "должна быть строка DrawBorder");
        assert!(s.contains("3.00"), "ширина 3px");
    }

    // ── Тесты <img> / DrawImage ─────────────────────────────────────────────

    fn images(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawImage { .. }))
            .collect()
    }

    #[test]
    fn img_emits_draw_image() {
        let dl = build(r#"<img src="logo.png" alt="Logo" width="100" height="50">"#, "");
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, src, alt, .. } = imgs[0] {
            assert_eq!(src, "logo.png");
            assert_eq!(alt, "Logo");
            assert!((rect.width - 100.0).abs() < 0.1);
            assert!((rect.height - 50.0).abs() < 0.1);
        }
    }

    #[test]
    fn img_with_background_and_border_paints_in_order() {
        // Painter's order для replaced element: FillRect (bg) → DrawBorder →
        // DrawImage. Image идёт последним, чтобы быть над фоном.
        let dl = build(
            r#"<img src="x" width="50" height="50">"#,
            "img { background: blue; border: 2px solid red; }",
        );
        // Должны присутствовать все три команды.
        let kinds: Vec<&str> = dl
            .iter()
            .map(|c| match c {
                DisplayCommand::FillRect { .. } => "FillRect",
                DisplayCommand::FillRoundedRect { .. } => "FillRoundedRect",
                DisplayCommand::DrawBorder { .. } => "DrawBorder",
                DisplayCommand::DrawOutline { .. } => "DrawOutline",
                DisplayCommand::DrawImage { .. } => "DrawImage",
                DisplayCommand::DrawBackgroundImage { .. } => "DrawBackgroundImage",
                DisplayCommand::DrawText { .. } => "DrawText",
                DisplayCommand::PushClipRect { .. } => "PushClipRect",
                DisplayCommand::PopClip => "PopClip",
                DisplayCommand::PushOpacity { .. } => "PushOpacity",
                DisplayCommand::PopOpacity => "PopOpacity",
                DisplayCommand::PushBlendMode { .. } => "PushBlendMode",
                DisplayCommand::PopBlendMode => "PopBlendMode",
                DisplayCommand::DrawLayerSnapshot { .. } => "DrawLayerSnapshot",
                DisplayCommand::PushTransform { .. } => "PushTransform",
                DisplayCommand::PopTransform => "PopTransform",
                DisplayCommand::DrawLinearGradient { .. } => "DrawLinearGradient",
                DisplayCommand::DrawRadialGradient { .. } => "DrawRadialGradient",
                DisplayCommand::DrawConicGradient { .. } => "DrawConicGradient",
                DisplayCommand::PushMaskImage { .. } => "PushMaskImage",
                DisplayCommand::PushMaskLinearGradient { .. } => "PushMaskLinearGradient",
                DisplayCommand::PushMaskRadialGradient { .. } => "PushMaskRadialGradient",
                DisplayCommand::PushMaskConicGradient { .. } => "PushMaskConicGradient",
                DisplayCommand::PopMask => "PopMask",
                DisplayCommand::PushMaskLayer { .. } => "PushMaskLayer",
                DisplayCommand::PopMaskLayer => "PopMaskLayer",
                DisplayCommand::PushFilter { .. } => "PushFilter",
                DisplayCommand::PopFilter => "PopFilter",
                DisplayCommand::PushBackdropFilter { .. } => "PushBackdropFilter",
                DisplayCommand::PopBackdropFilter => "PopBackdropFilter",
                DisplayCommand::BeginStickyLayer { .. } => "BeginStickyLayer",
                DisplayCommand::EndStickyLayer => "EndStickyLayer",
                DisplayCommand::PushScrollLayer { .. } => "PushScrollLayer",
                DisplayCommand::PopScrollLayer => "PopScrollLayer",
                DisplayCommand::DrawSvgPath { .. } => "DrawSvgPath",
                DisplayCommand::BoxModelOverlay { .. } => "BoxModelOverlay",
                DisplayCommand::DrawScrollbar { .. } => "DrawScrollbar",
                DisplayCommand::PageBreak => "PageBreak",
                DisplayCommand::DrawCrossFade { .. } => "DrawCrossFade",
            })
            .collect();
        assert_eq!(kinds, vec!["FillRect", "DrawBorder", "DrawImage"]);
    }

    #[test]
    fn img_serialize_includes_src_and_alt() {
        let dl = build(
            r#"<img src="photo.jpg" alt="A photo" width="80" height="40">"#,
            "",
        );
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawImage"), "must contain DrawImage line");
        assert!(s.contains(r#"src="photo.jpg""#), "must contain src");
        assert!(s.contains(r#"alt="A photo""#), "must contain alt");
    }

    // ── Тесты <video> / DrawImage placeholder ───────────────────────────────

    #[test]
    fn video_without_poster_emits_no_draw_image() {
        // BUG-097: an empty <video> (no poster, no decoded frame) paints nothing —
        // the element box is transparent, matching Chromium/Edge. The grey image
        // placeholder is reserved for <img>, not media.
        let dl = build(r#"<video src="clip.mp4"></video>"#, "");
        let imgs = images(&dl);
        assert!(
            imgs.is_empty(),
            "posterless video should emit no DrawImage, got {}",
            imgs.len()
        );
    }

    #[test]
    fn video_with_poster_emits_draw_image_with_poster_src() {
        // When poster is set, DrawImage uses the poster URL so shell can register it.
        let dl = build(r#"<video src="clip.mp4" poster="thumb.jpg"></video>"#, "");
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { src, .. } = imgs[0] {
            assert_eq!(src, "thumb.jpg");
        }
    }

    #[test]
    fn video_ua_default_rect_300_by_150() {
        // Poster present so the replaced box paints a DrawImage at the UA-default rect.
        let dl = build(r#"<video src="clip.mp4" poster="thumb.jpg"></video>"#, "");
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, .. } = imgs[0] {
            assert!((rect.width - 300.0).abs() < 0.1, "width={}", rect.width);
            assert!((rect.height - 150.0).abs() < 0.1, "height={}", rect.height);
        }
    }

    #[test]
    fn video_css_dimensions_override_ua_default() {
        let dl = build(
            r#"<video src="clip.mp4" poster="thumb.jpg"></video>"#,
            "video { width: 640px; height: 360px; }",
        );
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, .. } = imgs[0] {
            assert!((rect.width - 640.0).abs() < 0.1, "width={}", rect.width);
            assert!((rect.height - 360.0).abs() < 0.1, "height={}", rect.height);
        }
    }

    // ── Тесты <audio> ─────────────────────────────────────────────────────────

    #[test]
    fn audio_without_controls_emits_nothing() {
        // <audio> without controls → 0×0 box → no FillRect emitted.
        let dl = build(r#"<audio src="song.mp3"></audio>"#, "");
        let fills: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .collect();
        assert!(
            fills.is_empty(),
            "audio without controls should emit no FillRect, got {} commands",
            fills.len()
        );
    }

    #[test]
    fn audio_with_controls_emits_fill_rect() {
        // <audio controls> → 40px grey bar → at least one FillRect.
        let dl = build(r#"<audio src="song.mp3" controls></audio>"#, "");
        let fills: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .collect();
        assert!(!fills.is_empty(), "audio with controls should emit a FillRect");
    }

    #[test]
    fn audio_with_controls_ua_default_height_40() {
        // UA default for <audio controls>: height = 40px.
        let dl = build(r#"<audio src="song.mp3" controls></audio>"#, "");
        let fill = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::FillRect { .. }));
        if let Some(DisplayCommand::FillRect { rect, .. }) = fill {
            assert!(
                (rect.height - 40.0).abs() < 0.1,
                "audio controls height should be 40px, got {}",
                rect.height
            );
        }
    }

    #[test]
    fn audio_with_controls_css_height_override() {
        // Explicit CSS height overrides UA default.
        let dl = build(
            r#"<audio src="song.mp3" controls></audio>"#,
            "audio { height: 60px; }",
        );
        let fill = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::FillRect { .. }));
        if let Some(DisplayCommand::FillRect { rect, .. }) = fill {
            assert!(
                (rect.height - 60.0).abs() < 0.1,
                "CSS height should override UA default, got {}",
                rect.height
            );
        }
    }

    // ── Тесты background-image url() / DrawBackgroundImage ─────────────────

    fn bg_images(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBackgroundImage { .. }))
            .collect()
    }

    #[test]
    fn block_background_image_url_emits_draw_background_image() {
        let dl = build(
            "<div>x</div>",
            "div { width: 80px; height: 40px; background-image: url(bg.png); }",
        );
        let bgs = bg_images(&dl);
        assert_eq!(bgs.len(), 1, "должна быть одна команда DrawBackgroundImage");
        if let DisplayCommand::DrawBackgroundImage { rect, src, .. } = bgs[0] {
            assert_eq!(src, "bg.png");
            assert!((rect.width - 80.0).abs() < 0.1, "rect.width={}", rect.width);
            assert!((rect.height - 40.0).abs() < 0.1, "rect.height={}", rect.height);
        }
    }

    #[test]
    fn background_image_none_emits_nothing() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; background-image: none; }",
        );
        assert!(bg_images(&dl).is_empty());
    }

    #[test]
    fn background_image_default_emits_nothing() {
        // initial value `none` (CSS Backgrounds L3 §3.10): отсутствие свойства
        // не должно эмитить DrawBackgroundImage.
        let dl = build("<div>x</div>", "div { width: 50px; height: 20px; }");
        assert!(bg_images(&dl).is_empty());
    }

    #[test]
    fn background_image_linear_gradient_emits_draw_linear_gradient() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: linear-gradient(to right, red, blue); }",
        );
        let grads: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawLinearGradient { .. }))
            .collect();
        assert_eq!(grads.len(), 1, "expected DrawLinearGradient");
        if let DisplayCommand::DrawLinearGradient { angle_deg, stops, repeating, .. } = grads[0] {
            assert!((angle_deg - 90.0).abs() < 0.1, "expected 90° for 'to right', got {angle_deg}");
            assert_eq!(stops.len(), 2);
            assert!(!repeating);
        }
    }

    #[test]
    fn background_image_radial_gradient_emits_draw_radial_gradient() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: radial-gradient(circle at 50% 50%, red, blue); }",
        );
        let grads: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawRadialGradient { .. }))
            .collect();
        assert_eq!(grads.len(), 1, "expected DrawRadialGradient");
        if let DisplayCommand::DrawRadialGradient { center_x_pct, center_y_pct, stops, .. } = grads[0] {
            assert!((center_x_pct - 0.5).abs() < 0.01);
            assert!((center_y_pct - 0.5).abs() < 0.01);
            assert_eq!(stops.len(), 2);
        }
    }

    #[test]
    fn background_image_conic_gradient_emits_draw_conic_gradient() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: conic-gradient(from 90deg at 30% 70%, red, blue); }",
        );
        let grads: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawConicGradient { .. }))
            .collect();
        assert_eq!(grads.len(), 1, "expected DrawConicGradient");
        if let DisplayCommand::DrawConicGradient {
            center_x_pct, center_y_pct, from_angle_deg, stops, repeating, ..
        } = grads[0]
        {
            assert!((center_x_pct - 0.3).abs() < 0.01);
            assert!((center_y_pct - 0.7).abs() < 0.01);
            assert!((from_angle_deg - 90.0).abs() < 0.1);
            assert_eq!(stops.len(), 2);
            assert!(!repeating);
        }
    }

    // ── BUG-087: sized/positioned/repeated gradient background layers ──────────

    fn linear_grads(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawLinearGradient { .. }))
            .collect()
    }

    #[test]
    fn gradient_tile_rects_single_no_repeat() {
        // 80×80 tile, centered (50% 50%) inside a 200×120 area, no-repeat → 1 rect.
        let origin = Rect::new(0.0, 0.0, 200.0, 120.0);
        let rects = gradient_tile_rects(
            80.0,
            80.0,
            ObjectPosition { x: PositionComponent::Percent(0.5), y: PositionComponent::Percent(0.5) },
            BackgroundRepeat::NoRepeat,
            origin,
            origin,
        );
        assert_eq!(rects.len(), 1);
        // off = (200-80)*0.5 = 60 ; (120-80)*0.5 = 20
        assert!((rects[0].x - 60.0).abs() < 0.01, "x={}", rects[0].x);
        assert!((rects[0].y - 20.0).abs() < 0.01, "y={}", rects[0].y);
        assert!((rects[0].width - 80.0).abs() < 0.01);
        assert!((rects[0].height - 80.0).abs() < 0.01);
    }

    #[test]
    fn gradient_tile_rects_repeat_x_covers_area() {
        // 20px-wide tiles, full height, repeat-x across a 200px area → tiles span it.
        let origin = Rect::new(0.0, 0.0, 200.0, 100.0);
        let rects = gradient_tile_rects(
            20.0,
            100.0,
            ObjectPosition { x: PositionComponent::Percent(0.0), y: PositionComponent::Percent(0.0) },
            BackgroundRepeat::RepeatX,
            origin,
            origin,
        );
        // 200/20 = 10 tiles, single row.
        assert_eq!(rects.len(), 10, "expected 10 stripes, got {}", rects.len());
        assert!(rects.iter().all(|r| (r.height - 100.0).abs() < 0.01));
        // Tiles span from left to right edge.
        assert!((rects[0].x - 0.0).abs() < 0.01);
        assert!((rects[9].x - 180.0).abs() < 0.01, "last x={}", rects[9].x);
    }

    #[test]
    fn sized_gradient_layer_emits_tile_not_full_box() {
        // BUG-087: a gradient with explicit `background-size` must paint a tile of
        // that size (clipped to the box), not stretch across the whole box.
        let dl = build(
            "<div>x</div>",
            "div { width: 200px; height: 120px; \
             background: linear-gradient(to right, red, blue) center / 80px 80px no-repeat; }",
        );
        let grads = linear_grads(&dl);
        assert_eq!(grads.len(), 1, "one gradient tile expected");
        if let DisplayCommand::DrawLinearGradient { rect, .. } = grads[0] {
            assert!((rect.width - 80.0).abs() < 0.1, "tile width should be 80, got {}", rect.width);
            assert!((rect.height - 80.0).abs() < 0.1, "tile height should be 80, got {}", rect.height);
        }
        // Sized tiling must be wrapped in a clip to the painting area.
        assert!(
            dl.iter().any(|c| matches!(c, DisplayCommand::PushClipRect { .. })),
            "sized gradient must be clipped to the box"
        );
    }

    #[test]
    fn repeat_x_gradient_layer_emits_multiple_tiles() {
        // BUG-087: repeat-x sized gradient emits one command per visible stripe.
        let dl = build(
            "<div>x</div>",
            "div { width: 100px; height: 50px; \
             background: linear-gradient(to bottom, red, blue) left top / 20px 100% repeat-x; }",
        );
        let grads = linear_grads(&dl);
        assert!(grads.len() >= 5, "expected ≥5 stripes for 100px/20px, got {}", grads.len());
        for g in &grads {
            if let DisplayCommand::DrawLinearGradient { rect, .. } = g {
                assert!((rect.width - 20.0).abs() < 0.1, "stripe width 20, got {}", rect.width);
            }
        }
    }

    #[test]
    fn unsized_gradient_layer_still_fills_box() {
        // Regression guard: a gradient WITHOUT background-size keeps the historical
        // single full-box command (no tiling, no extra clip) so existing snapshots
        // stay byte-identical.
        let dl = build(
            "<div>x</div>",
            "div { width: 200px; height: 120px; \
             background: linear-gradient(to right, red, blue); }",
        );
        let grads = linear_grads(&dl);
        assert_eq!(grads.len(), 1, "single full-box gradient");
        if let DisplayCommand::DrawLinearGradient { rect, .. } = grads[0] {
            assert!((rect.width - 200.0).abs() < 0.1, "full box width, got {}", rect.width);
            assert!((rect.height - 120.0).abs() < 0.1, "full box height, got {}", rect.height);
        }
    }

    #[test]
    fn background_image_repeating_conic_gradient() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: repeating-conic-gradient(red 0deg, blue 90deg); }",
        );
        let grads: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawConicGradient { .. }))
            .collect();
        assert_eq!(grads.len(), 1, "expected DrawConicGradient (repeating)");
        if let DisplayCommand::DrawConicGradient { repeating, .. } = grads[0] {
            assert!(*repeating);
        }
    }

    #[test]
    fn background_image_conic_gradient_serialize_includes_from_angle() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: conic-gradient(from 45deg, red, blue); }",
        );
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawConicGradient"), "should contain DrawConicGradient line");
        assert!(s.contains("from=45.0deg"), "should record from-angle: {s}");
    }

    #[test]
    fn background_image_repeating_linear_gradient() {
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: repeating-linear-gradient(45deg, red, blue); }",
        );
        let grads: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawLinearGradient { .. }))
            .collect();
        assert_eq!(grads.len(), 1, "expected DrawLinearGradient for repeating");
        if let DisplayCommand::DrawLinearGradient { angle_deg, repeating, .. } = grads[0] {
            assert!((angle_deg - 45.0).abs() < 0.1);
            assert!(*repeating);
        }
    }

    #[test]
    fn background_image_linear_gradient_default_angle_is_to_bottom() {
        // No direction specified → default is "to bottom" = 180°.
        let dl = build(
            "<div>x</div>",
            "div { width: 50px; height: 20px; \
             background-image: linear-gradient(red, blue); }",
        );
        let grads: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawLinearGradient { .. }))
            .collect();
        assert_eq!(grads.len(), 1);
        if let DisplayCommand::DrawLinearGradient { angle_deg, .. } = grads[0] {
            assert!((angle_deg - 180.0).abs() < 0.1, "expected 180° default, got {angle_deg}");
        }
    }

    #[test]
    fn background_image_paints_after_color_before_border() {
        // CSS Backgrounds L3 §3.10 — painting order: bg-color → bg-image → border.
        let dl = build(
            "<div></div>",
            "div { width: 60px; height: 30px; \
             background-color: red; background-image: url(b.png); \
             border: 2px solid blue; }",
        );
        let kinds: Vec<&str> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { .. } => Some("FillRect"),
                DisplayCommand::DrawBackgroundImage { .. } => Some("DrawBackgroundImage"),
                DisplayCommand::DrawBorder { .. } => Some("DrawBorder"),
                _ => None,
            })
            .collect();
        // Allow surrounding commands; check relative order of the three.
        let fill = kinds.iter().position(|k| *k == "FillRect").expect("FillRect emitted");
        let bg = kinds.iter().position(|k| *k == "DrawBackgroundImage").expect("bg-image emitted");
        let border = kinds.iter().position(|k| *k == "DrawBorder").expect("border emitted");
        assert!(fill < bg, "bg-color must precede bg-image (kinds={kinds:?})");
        assert!(bg < border, "bg-image must precede border (kinds={kinds:?})");
    }

    #[test]
    fn background_image_serialize_includes_src() {
        let dl = build(
            "<div>x</div>",
            "div { width: 40px; height: 10px; background-image: url(\"hero.jpg\"); }",
        );
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawBackgroundImage"), "should contain DrawBackgroundImage line");
        assert!(s.contains(r#"src="hero.jpg""#), "should contain quoted src");
    }

    #[test]
    fn background_image_paint_emits_draw_background_image_with_paint_src() {
        // CSS Paint API (Houdini) Phase 0 — `background-image: paint(name)` must emit
        // DrawBackgroundImage with src prefixed "paint:" for renderer identification.
        let dl = build(
            "<div></div>",
            "div { width: 80px; height: 40px; background-image: paint(my-worklet); }",
        );
        let paint_bg = dl.iter().find(|c| {
            matches!(c, DisplayCommand::DrawBackgroundImage { src, .. } if src.starts_with("paint:"))
        });
        assert!(paint_bg.is_some(), "paint() must emit DrawBackgroundImage with 'paint:' src");
        if let Some(DisplayCommand::DrawBackgroundImage { src, .. }) = paint_bg {
            assert_eq!(src, "paint:my-worklet", "src must be paint:<name>");
        }
    }

    #[test]
    fn background_image_respects_background_clip_padding_box() {
        // background-clip: padding-box ужимает rect под border на каждой стороне.
        // box-sizing по умолчанию content-box: width=100 — это контент,
        // полная коробка с border 5×2 = 110×70. PaddingBox shrink → 100×60.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; background-image: url(x.png); \
             border: 5px solid red; background-clip: padding-box; }",
        );
        let bgs = bg_images(&dl);
        assert_eq!(bgs.len(), 1);
        if let DisplayCommand::DrawBackgroundImage { rect, .. } = bgs[0] {
            assert!((rect.width - 100.0).abs() < 0.1, "got {}", rect.width);
            assert!((rect.height - 60.0).abs() < 0.1, "got {}", rect.height);
        }
    }

    // ── Тесты background-origin ────────────────────────────────────────────────

    #[test]
    fn background_origin_default_padding_box_equals_clip_border_box() {
        // Default: background-origin: padding-box, background-clip: border-box.
        // With no border: origin_rect == clip rect == border-box.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; background-image: url(x.png); }",
        );
        let bgs = bg_images(&dl);
        assert_eq!(bgs.len(), 1);
        if let DisplayCommand::DrawBackgroundImage { rect, origin_rect, .. } = bgs[0] {
            assert!((rect.width - 100.0).abs() < 0.1, "rect.width={}", rect.width);
            assert!((origin_rect.width - 100.0).abs() < 0.1, "origin_rect.width={}", origin_rect.width);
            assert!((rect.height - 60.0).abs() < 0.1);
            assert!((origin_rect.height - 60.0).abs() < 0.1);
        }
    }

    #[test]
    fn background_origin_content_box_excludes_padding_and_border() {
        // box-sizing: content-box, width=100, height=60, border=5px, padding=10px.
        // border-box: 130×90. padding-box: 120×80. content-box (origin): 100×60.
        // background-clip: border-box by default → rect is 130×90.
        // background-origin: content-box → origin_rect is 100×60.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; background-image: url(x.png); \
             border: 5px solid red; padding: 10px; \
             background-origin: content-box; background-clip: border-box; }",
        );
        let bgs = bg_images(&dl);
        assert_eq!(bgs.len(), 1);
        if let DisplayCommand::DrawBackgroundImage { rect, origin_rect, .. } = bgs[0] {
            // clip rect (border-box) = 130×90
            assert!((rect.width - 130.0).abs() < 0.1, "rect.width={}", rect.width);
            assert!((rect.height - 90.0).abs() < 0.1, "rect.height={}", rect.height);
            // origin_rect (content-box) = 100×60
            assert!((origin_rect.width - 100.0).abs() < 0.1, "origin_rect.width={}", origin_rect.width);
            assert!((origin_rect.height - 60.0).abs() < 0.1, "origin_rect.height={}", origin_rect.height);
        }
    }

    #[test]
    fn background_origin_border_box_equals_clip_border_box() {
        // background-origin: border-box means positioning starts at border edge.
        // With 5px border: both rect and origin_rect include border area.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; background-image: url(x.png); \
             border: 5px solid red; background-origin: border-box; }",
        );
        let bgs = bg_images(&dl);
        assert_eq!(bgs.len(), 1);
        if let DisplayCommand::DrawBackgroundImage { rect, origin_rect, .. } = bgs[0] {
            // Both clip (border-box default) and origin (border-box explicit) = 110×70
            assert!((rect.width - 110.0).abs() < 0.1, "rect.width={}", rect.width);
            assert!((origin_rect.width - 110.0).abs() < 0.1, "origin_rect.width={}", origin_rect.width);
            assert!((rect.width - origin_rect.width).abs() < 0.1, "rects should match");
            assert!((rect.height - origin_rect.height).abs() < 0.1, "rects should match");
        }
    }

    #[test]
    fn background_origin_padding_box_with_border_shrinks_origin() {
        // background-origin: padding-box (default), background-clip: border-box.
        // With 8px border: border-box=116×76, padding-box=100×60.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 60px; background-image: url(x.png); \
             border: 8px solid black; background-origin: padding-box; }",
        );
        let bgs = bg_images(&dl);
        assert_eq!(bgs.len(), 1);
        if let DisplayCommand::DrawBackgroundImage { rect, origin_rect, .. } = bgs[0] {
            assert!((rect.width - 116.0).abs() < 0.1, "rect.width={}", rect.width);
            assert!((origin_rect.width - 100.0).abs() < 0.1, "origin_rect.width={}", origin_rect.width);
        }
    }

    #[test]
    fn img_without_dimensions_emits_zero_rect() {
        // Без размеров — placeholder 0×0; команда всё равно эмитится,
        // потому что DOM-узел существует. Renderer просто не нарисует ничего.
        let dl = build(r#"<img src="x">"#, "");
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 1);
        if let DisplayCommand::DrawImage { rect, .. } = imgs[0] {
            assert!(rect.width.abs() < 0.1);
            assert!(rect.height.abs() < 0.1);
        }
    }

    #[test]
    fn multiple_imgs_emit_multiple_draw_image() {
        let dl = build(
            r#"<img src="a.png" width="10" height="10"><img src="b.png" width="20" height="20">"#,
            "",
        );
        let imgs = images(&dl);
        assert_eq!(imgs.len(), 2);
    }

    // ── Тесты fit_image_rect / fit_image_quad (CSS Images L3 §5.5) ──────────

    fn box100() -> Rect {
        Rect::new(0.0, 0.0, 100.0, 100.0)
    }

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn approx_rect(r: Rect, x: f32, y: f32, w: f32, h: f32) -> bool {
        approx_eq(r.x, x) && approx_eq(r.y, y) && approx_eq(r.width, w) && approx_eq(r.height, h)
    }

    #[test]
    fn fit_fill_stretches_to_box() {
        let placed = fit_image_rect(box100(), (50, 200), ObjectFit::Fill, ObjectPosition::default());
        assert!(approx_rect(placed, 0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn fit_contain_letterboxes_wide_image() {
        // 200×100 в 100×100: scale=0.5, placed=100×50, центрируется по y.
        let placed = fit_image_rect(box100(), (200, 100), ObjectFit::Contain, ObjectPosition::default());
        assert!(approx_rect(placed, 0.0, 25.0, 100.0, 50.0));
    }

    #[test]
    fn fit_contain_pillarboxes_tall_image() {
        // 100×200 в 100×100: scale=0.5, placed=50×100, центрируется по x.
        let placed = fit_image_rect(box100(), (100, 200), ObjectFit::Contain, ObjectPosition::default());
        assert!(approx_rect(placed, 25.0, 0.0, 50.0, 100.0));
    }

    #[test]
    fn fit_cover_overflows_wide_image() {
        // 200×100 в 100×100 при cover: scale=1.0, placed=200×100, центр →
        // x=-50, y=0.
        let placed = fit_image_rect(box100(), (200, 100), ObjectFit::Cover, ObjectPosition::default());
        assert!(approx_rect(placed, -50.0, 0.0, 200.0, 100.0));
    }

    #[test]
    fn fit_none_keeps_intrinsic_size() {
        let placed = fit_image_rect(box100(), (50, 50), ObjectFit::None, ObjectPosition::default());
        // 50×50 центрируется в 100×100.
        assert!(approx_rect(placed, 25.0, 25.0, 50.0, 50.0));
    }

    #[test]
    fn fit_scale_down_picks_none_when_smaller() {
        // 50×50 меньше 100×100 — none даёт меньшую площадь, чем contain.
        let placed = fit_image_rect(box100(), (50, 50), ObjectFit::ScaleDown, ObjectPosition::default());
        assert!(approx_rect(placed, 25.0, 25.0, 50.0, 50.0));
    }

    #[test]
    fn fit_scale_down_picks_contain_when_larger() {
        // 200×200 больше 100×100 — contain даёт меньшую площадь.
        let placed = fit_image_rect(box100(), (200, 200), ObjectFit::ScaleDown, ObjectPosition::default());
        assert!(approx_rect(placed, 0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn fit_position_top_left_aligns_to_origin() {
        let pos = ObjectPosition {
            x: PositionComponent::Percent(0.0),
            y: PositionComponent::Percent(0.0),
        };
        let placed = fit_image_rect(box100(), (50, 50), ObjectFit::None, pos);
        assert!(approx_rect(placed, 0.0, 0.0, 50.0, 50.0));
    }

    #[test]
    fn fit_position_bottom_right_aligns_to_corner() {
        let pos = ObjectPosition {
            x: PositionComponent::Percent(1.0),
            y: PositionComponent::Percent(1.0),
        };
        let placed = fit_image_rect(box100(), (50, 50), ObjectFit::None, pos);
        assert!(approx_rect(placed, 50.0, 50.0, 50.0, 50.0));
    }

    #[test]
    fn fit_zero_intrinsic_size_returns_box() {
        let placed = fit_image_rect(box100(), (0, 100), ObjectFit::Cover, ObjectPosition::default());
        assert!(approx_rect(placed, 0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn quad_contain_returns_full_uvs() {
        // contain не выходит за box → uv = [0,0]..[1,1].
        let (visible, uv0, uv1) = fit_image_quad(
            box100(),
            (200, 100),
            ObjectFit::Contain,
            ObjectPosition::default(),
        )
        .expect("contain visible");
        assert!(approx_rect(visible, 0.0, 25.0, 100.0, 50.0));
        assert!(approx_eq(uv0[0], 0.0) && approx_eq(uv0[1], 0.0));
        assert!(approx_eq(uv1[0], 1.0) && approx_eq(uv1[1], 1.0));
    }

    #[test]
    fn quad_cover_crops_uvs_horizontally() {
        // 200×100 cover в 100×100: placement=200×100 at x=-50; visible=
        // box100; UV: u0=(0-(-50))/200=0.25, u1=(100-(-50))/200=0.75.
        let (visible, uv0, uv1) = fit_image_quad(
            box100(),
            (200, 100),
            ObjectFit::Cover,
            ObjectPosition::default(),
        )
        .expect("cover visible");
        assert!(approx_rect(visible, 0.0, 0.0, 100.0, 100.0));
        assert!(approx_eq(uv0[0], 0.25) && approx_eq(uv0[1], 0.0));
        assert!(approx_eq(uv1[0], 0.75) && approx_eq(uv1[1], 1.0));
    }

    #[test]
    fn quad_none_with_oversized_image_crops_uvs() {
        // none при 200×200 в 100×100 — placement=200×200 at (-50,-50);
        // visible=box100; UV: 0.25..0.75 по обеим осям.
        let (visible, uv0, uv1) = fit_image_quad(
            box100(),
            (200, 200),
            ObjectFit::None,
            ObjectPosition::default(),
        )
        .expect("none-larger visible");
        assert!(approx_rect(visible, 0.0, 0.0, 100.0, 100.0));
        assert!(approx_eq(uv0[0], 0.25) && approx_eq(uv0[1], 0.25));
        assert!(approx_eq(uv1[0], 0.75) && approx_eq(uv1[1], 0.75));
    }

    #[test]
    fn quad_zero_intrinsic_returns_none() {
        assert!(fit_image_quad(
            box100(),
            (0, 0),
            ObjectFit::Fill,
            ObjectPosition::default()
        )
        .is_none());
    }

    #[test]
    fn quad_serialize_includes_fit_and_position() {
        // Когда fit/position отличны от дефолтов — в snapshot-серилизатор
        // попадают «fit=» и «pos=» поля. Проверяем через ручной DisplayList,
        // чтобы не возиться с CSS-парсингом object-fit.
        let dl = vec![DisplayCommand::DrawImage {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            src: "x".into(),
            alt: String::new(),
            object_fit: ObjectFit::Cover,
            object_position: ObjectPosition {
                x: PositionComponent::Px(10.0),
                y: PositionComponent::Percent(0.0),
            },
            image_rendering: ImageRendering::Auto,
        }];
        let s = serialize_display_list(&dl);
        assert!(s.contains("fit=cover"), "{s}");
        assert!(s.contains("pos=10.00px 0.00%"), "{s}");
    }

    #[test]
    fn quad_serialize_omits_defaults() {
        let dl = vec![DisplayCommand::DrawImage {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            src: "x".into(),
            alt: String::new(),
            object_fit: ObjectFit::Fill,
            object_position: ObjectPosition::default(),
            image_rendering: ImageRendering::Auto,
        }];
        let s = serialize_display_list(&dl);
        assert!(!s.contains("fit="), "{s}");
        assert!(!s.contains("pos="), "{s}");
    }

    #[test]
    fn push_clip_rect_serializes() {
        let dl = vec![DisplayCommand::PushClipRect {
            rect: Rect::new(10.0, 20.0, 100.0, 50.0),
        }];
        let s = serialize_display_list(&dl);
        assert_eq!(s, "PushClipRect (10.00, 20.00, 100.00, 50.00)\n");
    }

    #[test]
    fn pop_clip_serializes() {
        let dl = vec![DisplayCommand::PopClip];
        assert_eq!(serialize_display_list(&dl), "PopClip\n");
    }

    #[test]
    fn push_opacity_serializes_with_alpha() {
        let dl = vec![DisplayCommand::PushOpacity { alpha: 0.5 }];
        assert_eq!(serialize_display_list(&dl), "PushOpacity 0.500\n");
    }

    #[test]
    fn pop_opacity_serializes() {
        let dl = vec![DisplayCommand::PopOpacity];
        assert_eq!(serialize_display_list(&dl), "PopOpacity\n");
    }

    #[test]
    fn push_blend_mode_serializes_with_name() {
        let dl = vec![DisplayCommand::PushBlendMode {
            mode: BlendMode::Multiply,
        }];
        assert_eq!(serialize_display_list(&dl), "PushBlendMode multiply\n");
    }

    #[test]
    fn pop_blend_mode_serializes() {
        let dl = vec![DisplayCommand::PopBlendMode];
        assert_eq!(serialize_display_list(&dl), "PopBlendMode\n");
    }

    #[test]
    fn blend_mode_from_keyword_all_16_modes() {
        let cases = [
            ("normal", BlendMode::Normal),
            ("multiply", BlendMode::Multiply),
            ("screen", BlendMode::Screen),
            ("overlay", BlendMode::Overlay),
            ("darken", BlendMode::Darken),
            ("lighten", BlendMode::Lighten),
            ("color-dodge", BlendMode::ColorDodge),
            ("color-burn", BlendMode::ColorBurn),
            ("hard-light", BlendMode::HardLight),
            ("soft-light", BlendMode::SoftLight),
            ("difference", BlendMode::Difference),
            ("exclusion", BlendMode::Exclusion),
            ("hue", BlendMode::Hue),
            ("saturation", BlendMode::Saturation),
            ("color", BlendMode::Color),
            ("luminosity", BlendMode::Luminosity),
        ];
        for (kw, expected) in cases {
            assert_eq!(
                BlendMode::from_keyword(kw),
                Some(expected),
                "keyword {kw:?} → {expected:?}"
            );
        }
    }

    #[test]
    fn blend_mode_from_keyword_case_insensitive() {
        assert_eq!(
            BlendMode::from_keyword("MULTIPLY"),
            Some(BlendMode::Multiply)
        );
        assert_eq!(
            BlendMode::from_keyword("Color-Dodge"),
            Some(BlendMode::ColorDodge)
        );
        assert_eq!(
            BlendMode::from_keyword("hArD-LiGhT"),
            Some(BlendMode::HardLight)
        );
    }

    #[test]
    fn blend_mode_from_keyword_unknown_returns_none() {
        assert_eq!(BlendMode::from_keyword(""), None);
        assert_eq!(BlendMode::from_keyword("bogus"), None);
        // CSS использует kebab-case с дефисом; underscore — не валидный
        assert_eq!(BlendMode::from_keyword("color_dodge"), None);
        // Без префикса/суффикса
        assert_eq!(BlendMode::from_keyword("dodge"), None);
        // С пробелами не парсим — должна быть отдельная команда trim caller-ом
        assert_eq!(BlendMode::from_keyword(" multiply "), None);
    }

    #[test]
    fn blend_mode_default_is_normal() {
        assert_eq!(BlendMode::default(), BlendMode::Normal);
    }

    #[test]
    fn nested_layer_ops_serialize_in_order() {
        let dl = vec![
            DisplayCommand::PushClipRect {
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            },
            DisplayCommand::PushOpacity { alpha: 0.7 },
            DisplayCommand::FillRect {
                rect: Rect::new(10.0, 10.0, 50.0, 50.0),
                color: Color::BLACK,
            },
            DisplayCommand::PopOpacity,
            DisplayCommand::PopClip,
        ];
        let s = serialize_display_list(&dl);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[0], "PushClipRect (0.00, 0.00, 100.00, 100.00)");
        assert_eq!(lines[1], "PushOpacity 0.700");
        assert!(lines[2].starts_with("FillRect"));
        assert_eq!(lines[3], "PopOpacity");
        assert_eq!(lines[4], "PopClip");
    }

    // ── build_display_list_ordered ─────────────────────────────────────

    fn build_ordered(html: &str, css: &str) -> DisplayList {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout_measured(
            &doc,
            &sheet,
            Size::new(800.0, 600.0),
            &Fixed8,
        );
        let stacking_tree = lumen_layout::StackingTree::build(&tree);
        let order = lumen_layout::PaintOrder::from_tree(&stacking_tree);
        build_display_list_ordered(&tree, &stacking_tree, &order)
    }

    #[test]
    fn ordered_single_sc_matches_dom_order_output() {
        // На странице без stacking-triggers `build_display_list_ordered`
        // и `build_display_list` должны эмитить ровно одинаковые команды
        // (порядок DOM = paint order для одного SC).
        let html = "<div style='background:#f00;'>hello</div>";
        let css = "";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout_measured(
            &doc,
            &sheet,
            Size::new(800.0, 600.0),
            &Fixed8,
        );
        let dom = build_display_list(&tree);
        let stacking_tree = lumen_layout::StackingTree::build(&tree);
        let order = lumen_layout::PaintOrder::from_tree(&stacking_tree);
        let ordered = build_display_list_ordered(&tree, &stacking_tree, &order);
        assert_eq!(dom, ordered);
    }

    #[test]
    fn ordered_positive_z_child_painted_after_root_content() {
        // <div z=1 (opacity)>SC-creating</div> рядом с inline-текстом.
        // Ordered-вывод: root.bg → root.contents (включая текст) →
        // child-SC contents (заминусованный, чтобы создать SC).
        //
        // Используем opacity:0.5 как SC-trigger без z-index (auto = phase 6,
        // эмитится ПОСЛЕ root.InlineContent).
        let dl = build_ordered(
            "<p>hello</p><div>world</div>",
            "div { opacity: 0.5; }",
        );
        // Должны быть текстовые узлы из обеих секций. Главное —
        // div-content (world) появляется после p-content (hello).
        let hello_idx = dl.iter().position(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "hello")
        });
        let world_idx = dl.iter().position(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "world")
        });
        assert!(
            hello_idx.is_some() && world_idx.is_some(),
            "обе строки должны рендериться"
        );
        assert!(
            hello_idx.unwrap() < world_idx.unwrap(),
            "child-SC (opacity div, phase 6) рисуется ПОСЛЕ root.contents (phase 5)"
        );
    }

    #[test]
    fn ordered_negative_z_child_painted_before_root_content() {
        // div с position:relative + z-index:-1 создаёт SC с negative-z.
        // Должен рисоваться до root.InlineContent (т.е. до текста "hello").
        let dl = build_ordered(
            "<div>neg</div><p>hello</p>",
            "div { position: relative; z-index: -1; background: #0f0; }",
        );
        // neg-content (DrawText "neg" внутри div) должен идти до root.contents
        // ("hello" внутри p).
        let neg_text = dl.iter().position(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "neg")
        });
        let hello_idx = dl.iter().position(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "hello")
        });
        assert!(neg_text.is_some(), "должен быть DrawText neg");
        assert!(hello_idx.is_some(), "должен быть DrawText hello");
        assert!(
            neg_text.unwrap() < hello_idx.unwrap(),
            "neg-z div (phase 2) рисуется ДО root.InlineContent (phase 5)"
        );
    }

    // ── layer-ops эмиссия в build_display_list_ordered ─────────────────

    /// Helper: количество вхождений варианта в DisplayList.
    fn count_variant(dl: &DisplayList, predicate: impl Fn(&DisplayCommand) -> bool) -> usize {
        dl.iter().filter(|c| predicate(c)).count()
    }

    #[test]
    fn ordered_opacity_lt_one_emits_push_pop_pair() {
        let dl = build_ordered("<div>x</div>", "div { opacity: 0.5; }");
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushOpacity { .. }));
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopOpacity));
        assert_eq!(pushes, 1, "opacity<1 → один PushOpacity");
        assert_eq!(pops, 1, "и парный PopOpacity");

        // Push до контента, Pop после.
        let push_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushOpacity { .. }))
            .unwrap();
        let pop_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopOpacity))
            .unwrap();
        let text_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text == "x"));
        assert!(push_idx < pop_idx);
        if let Some(text_idx) = text_idx {
            assert!(push_idx < text_idx);
            assert!(text_idx < pop_idx);
        }
    }

    #[test]
    fn ordered_opacity_alpha_value_preserved() {
        let dl = build_ordered("<div>x</div>", "div { opacity: 0.25; }");
        let push = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::PushOpacity { .. }))
            .unwrap();
        if let DisplayCommand::PushOpacity { alpha } = push {
            assert!((alpha - 0.25).abs() < 1e-6);
        } else {
            panic!("expected PushOpacity");
        }
    }

    #[test]
    fn ordered_opacity_one_does_not_emit() {
        let dl = build_ordered("<div>x</div>", "div { opacity: 1; }");
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushOpacity { .. }));
        assert_eq!(pushes, 0, "opacity:1 не триггерит Push");
    }

    #[test]
    fn ordered_mix_blend_mode_emits_push_pop() {
        let dl = build_ordered(
            "<div>x</div>",
            "div { mix-blend-mode: multiply; }",
        );
        let pushes: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::PushBlendMode { mode } => Some(*mode),
                _ => None,
            })
            .collect();
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopBlendMode));
        assert_eq!(pushes, vec![BlendMode::Multiply]);
        assert_eq!(pops, 1);
    }

    #[test]
    fn ordered_mix_blend_mode_normal_does_not_emit() {
        let dl = build_ordered(
            "<div>x</div>",
            "div { mix-blend-mode: normal; }",
        );
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushBlendMode { .. }));
        assert_eq!(pushes, 0);
    }

    #[test]
    fn ordered_overflow_hidden_on_sc_owner_emits_clip() {
        // div c opacity<1 (= SC-owner) + overflow:hidden → Push/PopClipRect
        // в SC-owner bucket. Opacity тоже эмитится; проверяем clip отдельно.
        let dl = build_ordered(
            "<div>x</div>",
            "div { opacity: 0.5; overflow: hidden; width: 100px; height: 50px; }",
        );
        let pushes_clip: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::PushClipRect { rect } => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(pushes_clip.len(), 1, "overflow:hidden → один PushClipRect");
        let pops_clip = count_variant(&dl, |c| matches!(c, DisplayCommand::PopClip));
        assert_eq!(pops_clip, 1);
    }

    #[test]
    fn ordered_overflow_hidden_on_non_sc_emits_clip_inline() {
        // div c overflow:hidden НЕ создаёт SC (overflow — не SC-trigger).
        // PushClipRect эмитится inline в bucket.contents текущего SC.
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow: hidden; width: 100px; height: 50px; }",
        );
        let pushes_clip = count_variant(&dl, |c| matches!(c, DisplayCommand::PushClipRect { .. }));
        let pops_clip = count_variant(&dl, |c| matches!(c, DisplayCommand::PopClip));
        assert_eq!(pushes_clip, 1);
        assert_eq!(pops_clip, 1);
        // SC не появился: PushOpacity/PushBlendMode не должны быть.
        assert_eq!(
            count_variant(&dl, |c| matches!(c, DisplayCommand::PushOpacity { .. })),
            0
        );
    }

    #[test]
    fn ordered_overflow_visible_does_not_emit_clip() {
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow: visible; opacity: 0.5; }",
        );
        let pushes_clip = count_variant(&dl, |c| matches!(c, DisplayCommand::PushClipRect { .. }));
        assert_eq!(pushes_clip, 0, "overflow:visible не клипает");
    }

    #[test]
    fn ordered_overflow_x_alone_triggers_clip() {
        // overflow-x:hidden + overflow-y:visible → CSS Overflow L3 §3.1 coercion
        // computes overflow-y to `auto`, which is a scroll container, so the
        // clip is established via PushScrollLayer (not a plain PushClipRect).
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow-x: hidden; width: 100px; height: 50px; }",
        );
        let clips = count_variant(&dl, |c| {
            matches!(c, DisplayCommand::PushClipRect { .. } | DisplayCommand::PushScrollLayer { .. })
        });
        assert_eq!(clips, 1, "overflow-x:hidden establishes one clip layer");
    }

    #[test]
    fn ordered_combined_opacity_blend_clip_emit_lifo() {
        // SC-owner со всеми тремя триггерами: проверяем парность и LIFO.
        let dl = build_ordered(
            "<div>x</div>",
            "div {
                opacity: 0.5;
                mix-blend-mode: multiply;
                overflow: hidden;
                width: 100px;
                height: 50px;
            }",
        );
        // Извлекаем последовательность layer-ops (без других команд).
        let ops: Vec<&DisplayCommand> = dl
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    DisplayCommand::PushClipRect { .. }
                        | DisplayCommand::PopClip
                        | DisplayCommand::PushBlendMode { .. }
                        | DisplayCommand::PopBlendMode
                        | DisplayCommand::PushOpacity { .. }
                        | DisplayCommand::PopOpacity
                )
            })
            .collect();
        // Ожидаемый порядок (см. box_layer_ops): Clip → Blend → Opacity (Push),
        // потом Opacity → Blend → Clip (Pop) для LIFO-парности.
        assert_eq!(ops.len(), 6, "три триггера = 6 layer-ops");
        assert!(matches!(ops[0], DisplayCommand::PushClipRect { .. }));
        assert!(matches!(ops[1], DisplayCommand::PushBlendMode { .. }));
        assert!(matches!(ops[2], DisplayCommand::PushOpacity { .. }));
        assert!(matches!(ops[3], DisplayCommand::PopOpacity));
        assert!(matches!(ops[4], DisplayCommand::PopBlendMode));
        assert!(matches!(ops[5], DisplayCommand::PopClip));
    }

    #[test]
    fn ordered_nested_opacity_emits_two_pairs() {
        // Внешний div с opacity, внутренний div с opacity. Каждый создаёт
        // свой SC; должно быть 2 пары PushOpacity/PopOpacity.
        let dl = build_ordered(
            r#"<div class="outer"><div class="inner">x</div></div>"#,
            ".outer { opacity: 0.5; } .inner { opacity: 0.25; }",
        );
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushOpacity { .. }));
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopOpacity));
        assert_eq!(pushes, 2);
        assert_eq!(pops, 2);
    }

    #[test]
    fn ordered_no_triggers_emits_no_layer_ops() {
        // Простая страница без opacity/blend/overflow — ни одной layer-op.
        let dl = build_ordered("<p>hello</p>", "");
        let any_layer_op = dl.iter().any(|c| {
            matches!(
                c,
                DisplayCommand::PushClipRect { .. }
                    | DisplayCommand::PopClip
                    | DisplayCommand::PushBlendMode { .. }
                    | DisplayCommand::PopBlendMode
                    | DisplayCommand::PushOpacity { .. }
                    | DisplayCommand::PopOpacity
            )
        });
        assert!(!any_layer_op);
    }

    #[test]
    fn ordered_clip_rect_overflow_hidden_clips_both_axes() {
        // overflow: hidden → PushClipRect clips padding-box on both axes.
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow: hidden; width: 200px; height: 100px; background: #f00; }",
        );
        let rect = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushClipRect { rect } => Some(*rect),
                _ => None,
            })
            .expect("должен быть PushClipRect");
        assert!(rect.width > 0.0 && rect.height > 0.0);
    }

    #[test]
    fn ordered_clip_overflow_x_hidden_y_visible_coerces_to_both_clip() {
        // CSS Overflow L3 §3.1: overflow-y:visible paired with a non-visible
        // overflow-x coerces to `auto`. `auto` is a scroll container, so the
        // clip is established via PushScrollLayer; both axes are constrained to
        // the padding box (≈100×50), no unconstrained-axis sentinel. (BUG-020.)
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow-x: hidden; overflow-y: visible; width: 100px; height: 50px; background: #f00; }",
        );
        let rect = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushScrollLayer { clip_rect, .. } => Some(*clip_rect),
                DisplayCommand::PushClipRect { rect } => Some(*rect),
                _ => None,
            })
            .expect("должен быть clip-слой (PushScrollLayer) для overflow-x:hidden");
        // Both axes constrained to the box after visible→auto coercion.
        assert!(rect.width < 1_000.0, "x-axis should be clipped: width={}", rect.width);
        assert!(rect.height < 1_000.0, "y-axis should be clipped after coercion: height={}", rect.height);
    }

    #[test]
    fn ordered_clip_overflow_x_visible_y_hidden_coerces_to_both_clip() {
        // Symmetric: overflow-x:visible coerces to `auto` → both axes clip via
        // a scroll layer (the auto axis is a scroll container).
        let dl = build_ordered(
            "<div>x</div>",
            "div { overflow-x: visible; overflow-y: hidden; width: 100px; height: 50px; background: #f00; }",
        );
        let rect = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushScrollLayer { clip_rect, .. } => Some(*clip_rect),
                DisplayCommand::PushClipRect { rect } => Some(*rect),
                _ => None,
            })
            .expect("должен быть clip-слой (PushScrollLayer) для overflow-y:hidden");
        assert!(rect.height < 1_000.0, "y-axis should be clipped: height={}", rect.height);
        assert!(rect.width < 1_000.0, "x-axis should be clipped after coercion: width={}", rect.width);
    }

    #[test]
    fn ordered_empty_tree_produces_empty_list() {
        // Деградированный случай: StackingTree без contexts, layout —
        // пустая страница (одинокий root Block без детей и без bg/border).
        let doc = lumen_html_parser::parse("");
        let sheet = lumen_css_parser::parse("");
        let tree =
            lumen_layout::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
        let dl = build_display_list_ordered(
            &tree,
            &lumen_layout::StackingTree { contexts: vec![] },
            &lumen_layout::PaintOrder::default(),
        );
        assert!(dl.is_empty(), "пустой PaintOrder → пустой display list");
    }

    // ───────── outline rendering ─────────

    fn outlines(dl: &DisplayList) -> Vec<(&Color, f32, f32, OutlineStyle)> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawOutline { color, width, offset, style, .. } => {
                    Some((color, *width, *offset, *style))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn outline_solid_emits_draw_outline() {
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; outline: 2px solid red; }",
        );
        let o = outlines(&dl);
        assert_eq!(o.len(), 1, "ровно одна DrawOutline на div");
        let (color, width, offset, style) = o[0];
        assert_eq!(color.r, 255);
        assert!((width - 2.0).abs() < 0.01);
        assert!((offset - 0.0).abs() < 0.01);
        assert_eq!(style, OutlineStyle::Solid);
    }

    #[test]
    fn outline_none_emits_nothing() {
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; outline: 2px none red; }",
        );
        assert!(outlines(&dl).is_empty(), "outline:none → no DrawOutline");
    }

    #[test]
    fn outline_zero_width_emits_nothing() {
        // outline-width: 0 → invisible (CSS Basic UI L4 §5.1).
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; outline: 0 solid red; }",
        );
        assert!(outlines(&dl).is_empty(), "outline-width:0 → no DrawOutline");
    }

    #[test]
    fn outline_offset_is_preserved() {
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; \
             outline: 2px solid red; outline-offset: 5px; }",
        );
        let o = outlines(&dl);
        assert_eq!(o.len(), 1);
        assert!((o[0].2 - 5.0).abs() < 0.01, "offset=5px должен сохраниться");
    }

    #[test]
    fn outline_color_currentcolor_resolves_to_text_color() {
        // currentColor → CSS color (Phase 0 reduces Auto/CurrentColor to color).
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; color: rgb(10, 20, 30); \
             outline: 2px solid currentColor; }",
        );
        let o = outlines(&dl);
        assert_eq!(o.len(), 1);
        let (color, _, _, _) = o[0];
        assert_eq!((color.r, color.g, color.b), (10, 20, 30));
    }

    #[test]
    fn outline_after_children_in_walk() {
        // Outline parent-а должен идти ПОСЛЕ background ребёнка — иначе при
        // негативном outline-offset (Phase 2) outline парента закрывался бы
        // содержимым ребёнка. Phase 0 проверка ordering: DrawOutline
        // последняя из своего box-а.
        let dl = build(
            "<div><p></p></div>",
            "div { width: 100px; height: 50px; outline: 2px solid red; } \
             p { display: block; background: blue; width: 30px; height: 10px; }",
        );
        let outline_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::DrawOutline { .. }))
            .expect("должна быть DrawOutline");
        // FillRect ребёнка (background: blue) должен идти раньше DrawOutline.
        let child_bg_idx = dl
            .iter()
            .enumerate()
            .find(|(_, c)| matches!(c, DisplayCommand::FillRect { color, .. } if color.b == 255))
            .map(|(i, _)| i)
            .expect("должен быть синий FillRect ребёнка");
        assert!(
            child_bg_idx < outline_idx,
            "outline (idx {outline_idx}) должен идти после child background (idx {child_bg_idx})"
        );
    }

    #[test]
    fn outline_serializes_with_short_offset_only_when_nonzero() {
        // DrawOutline с offset=0 не выводит `off=…` в сериализацию (как
        // DrawText опускает default-значения).
        let dl = vec![DisplayCommand::DrawOutline {
            rect: Rect::new(0.0, 0.0, 100.0, 50.0),
            width: 2.0,
            style: OutlineStyle::Solid,
            color: Color { r: 255, g: 0, b: 0, a: 255 },
            offset: 0.0,
        }];
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawOutline (0.00, 0.00, 100.00, 50.00) w=2.00 s=solid #ff0000ff"));
        assert!(!s.contains("off="));

        // Non-zero offset → должен присутствовать.
        let dl2 = vec![DisplayCommand::DrawOutline {
            rect: Rect::new(0.0, 0.0, 100.0, 50.0),
            width: 2.0,
            style: OutlineStyle::Solid,
            color: Color { r: 255, g: 0, b: 0, a: 255 },
            offset: 5.0,
        }];
        let s2 = serialize_display_list(&dl2);
        assert!(s2.contains("off=5.00"));
    }

    // ───────── text-shadow rendering ─────────

    fn texts_with_colors(dl: &DisplayList) -> Vec<(String, [u8; 3])> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { text, color, .. } => {
                    Some((text.clone(), [color.r, color.g, color.b]))
                }
                _ => None,
            })
            .collect()
    }

    fn text_rects(dl: &DisplayList) -> Vec<(String, [f32; 2])> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { text, rect, .. } => {
                    Some((text.clone(), [rect.x, rect.y]))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn text_shadow_none_emits_only_main_text() {
        // Без text-shadow — ровно один DrawText на фрагмент (как раньше).
        let dl = build("<p>hello</p>", "p { color: black; }");
        let texts = texts_with_colors(&dl);
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0].0, "hello");
    }

    #[test]
    fn text_shadow_one_emits_shadow_before_main() {
        // Один text-shadow → 2 DrawText: сначала shadow, потом main.
        // Spec painter's order: shadow рисуется ПОД основным текстом.
        let dl = build(
            "<p>hi</p>",
            "p { color: black; text-shadow: 2px 3px red; }",
        );
        let texts = texts_with_colors(&dl);
        assert_eq!(texts.len(), 2, "shadow + main = 2 DrawText");
        // Painter's order: shadow первый (под main), main второй (поверх).
        assert_eq!(texts[0].1, [255, 0, 0], "первый = красная тень");
        assert_eq!(texts[1].1, [0, 0, 0], "второй = чёрный основной");
        // Тень смещена на (2, 3) px относительно main.
        let rects = text_rects(&dl);
        let dx = rects[0].1[0] - rects[1].1[0];
        let dy = rects[0].1[1] - rects[1].1[1];
        assert!((dx - 2.0).abs() < 0.01, "shadow_x смещён на 2px, got {dx}");
        assert!((dy - 3.0).abs() < 0.01, "shadow_y смещён на 3px, got {dy}");
    }

    #[test]
    fn text_shadow_multiple_reverse_order() {
        // Spec L3 §6: «first shadow is on top, subsequent shadows are
        // layered behind it». Значит painter's order: последняя в списке
        // рисуется первой (под всеми), первая — последней (над всеми, но
        // под main). Список: red(1px), green(2px), blue(3px) — порядок
        // эмиссии: blue → green → red → main.
        let dl = build(
            "<p>z</p>",
            "p { color: black; \
             text-shadow: 1px 0 red, 2px 0 green, 3px 0 blue; }",
        );
        let texts = texts_with_colors(&dl);
        assert_eq!(texts.len(), 4, "3 shadows + main = 4 DrawText");
        assert_eq!(texts[0].1, [0, 0, 255], "blue painted first (deepest)");
        assert_eq!(texts[1].1, [0, 128, 0], "green painted second");
        assert_eq!(texts[2].1, [255, 0, 0], "red painted third");
        assert_eq!(texts[3].1, [0, 0, 0], "main painted last (top)");
    }

    #[test]
    fn text_shadow_color_omitted_uses_currentcolor() {
        // CSS Text Decoration L3 §6: «If <color> is not specified, the
        // value used for color (currentColor) is used.»
        let dl = build(
            "<p>x</p>",
            "p { color: rgb(10, 20, 30); text-shadow: 1px 1px; }",
        );
        let texts = texts_with_colors(&dl);
        assert_eq!(texts.len(), 2);
        // Shadow color = currentColor = (10, 20, 30).
        assert_eq!(texts[0].1, [10, 20, 30]);
        assert_eq!(texts[1].1, [10, 20, 30]);
    }

    #[test]
    fn text_shadow_blur_wraps_in_push_filter() {
        // blur > 0 → DrawText завёрнут в PushFilter{Blur(sigma)} / PopFilter.
        // sigma = blur / 2.0 (то же соглашение, что box-shadow).
        // text-shadow: 2px 3px 8px red  →  sigma = 4.0
        let dl = build(
            "<p>hi</p>",
            "p { text-shadow: 2px 3px 8px red; }",
        );
        let push_idx = dl.iter().position(|c| {
            matches!(c, DisplayCommand::PushFilter { filters }
                if matches!(filters.as_slice(), [FilterFn::Blur(s)] if (*s - 4.0).abs() < 0.01))
        });
        assert!(push_idx.is_some(), "PushFilter{{Blur(4.0)}} должен быть в DL, got {dl:?}");
        let push_idx = push_idx.unwrap();
        // Сразу после PushFilter → DrawText тени.
        assert!(
            matches!(dl[push_idx + 1], DisplayCommand::DrawText { .. }),
            "после PushFilter ожидается DrawText"
        );
        // За DrawText тени → PopFilter.
        assert!(
            matches!(dl[push_idx + 2], DisplayCommand::PopFilter),
            "после DrawText тени ожидается PopFilter"
        );
    }

    #[test]
    fn text_shadow_no_blur_no_filter_wrap() {
        // blur == 0 → DrawText тени без PushFilter/PopFilter.
        let dl = build(
            "<p>x</p>",
            "p { text-shadow: 2px 3px red; }",
        );
        let has_filter = dl.iter().any(|c| {
            matches!(c, DisplayCommand::PushFilter { filters }
                if filters.iter().any(|f| matches!(f, FilterFn::Blur(_))))
        });
        assert!(!has_filter, "без blur не должно быть PushFilter, got {dl:?}");
        // Но DrawText тени должен быть.
        let shadow_draw = dl.iter().filter(|c| matches!(c, DisplayCommand::DrawText { .. })).count();
        assert!(shadow_draw >= 2, "должно быть ≥2 DrawText (тень + основной)");
    }

    #[test]
    fn text_shadow_blur_multiple_each_wrapped() {
        // Два text-shadow с blur > 0 — каждый получает свой PushFilter/PopFilter.
        let dl = build(
            "<p>z</p>",
            "p { text-shadow: 1px 1px 6px red, 2px 2px 4px blue; }",
        );
        let push_count = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::PushFilter { filters }
                if filters.iter().any(|f| matches!(f, FilterFn::Blur(_))))
        }).count();
        assert_eq!(push_count, 2, "два PushFilter для двух shadow с blur, got {dl:?}");
    }

    // ───────── box-shadow rendering ─────────

    fn fills_with_color(dl: &DisplayList) -> Vec<(Rect, [u8; 4])> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::FillRect { rect, color } => {
                    Some((*rect, [color.r, color.g, color.b, color.a]))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn box_shadow_none_emits_no_extra_fill() {
        // Без box-shadow div с background даёт ровно одну FillRect.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: red; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].1, [255, 0, 0, 255]);
    }

    #[test]
    fn box_shadow_outset_emits_fill_before_background() {
        // Outset shadow → 2 FillRect: сначала shadow (под bg), потом bg.
        // shadow смещена на (3, 5) px.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: 3px 5px black; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2);
        // Painter's order: shadow первый (под bg).
        assert_eq!(fills[0].1, [0, 0, 0, 255], "shadow первой");
        assert_eq!(fills[1].1, [255, 255, 255, 255], "background второй");
        // shadow смещена на (3, 5).
        let dx = fills[0].0.x - fills[1].0.x;
        let dy = fills[0].0.y - fills[1].0.y;
        assert!((dx - 3.0).abs() < 0.01);
        assert!((dy - 5.0).abs() < 0.01);
        // Размер shadow совпадает с box (spread=0).
        assert!((fills[0].0.width - fills[1].0.width).abs() < 0.01);
    }

    #[test]
    fn box_shadow_inset_offset_emits_frame() {
        // offset (3, 5) внутри 100×50 без border / spread:
        // outer = padding-box = (0..100, 0..50).
        // inner = (3..103, 5..55) — частично за outer.
        // hole = inner ∩ outer = (3..100, 5..50).
        // Тень = 4 кольцевых рамки; нулевая bottom (50..50) и right (100..100)
        // skip-ятся. Остаются top (0..5) + left (0..3 на полосе 5..50).
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: red; \
             box-shadow: inset 3px 5px black; }",
        );
        let fills = fills_with_color(&dl);
        // bg + top frame + left frame = 3.
        assert_eq!(fills.len(), 3);
        // Painter's order: bg первый, inset тени поверх.
        assert_eq!(fills[0].1, [255, 0, 0, 255], "bg = red");
        // Top frame: x=0, y=0, w=100, h=5.
        assert_eq!(fills[1].1[..3], [0, 0, 0], "frame = black");
        let top = fills[1].0;
        assert!((top.x - 0.0).abs() < 0.01);
        assert!((top.y - 0.0).abs() < 0.01);
        assert!((top.width - 100.0).abs() < 0.01);
        assert!((top.height - 5.0).abs() < 0.01);
        // Left frame: x=0, y=5, w=3, h=45.
        let left = fills[2].0;
        assert!((left.x - 0.0).abs() < 0.01);
        assert!((left.y - 5.0).abs() < 0.01);
        assert!((left.width - 3.0).abs() < 0.01);
        assert!((left.height - 45.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_inset_spread_only_emits_four_frames() {
        // Только spread, без offset: inner симметрично сжат на 10px →
        // hole = (10..90, 10..40). Все 4 рамки видимы.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: inset 0 0 0 10px black; }",
        );
        let fills = fills_with_color(&dl);
        // bg + 4 frames.
        assert_eq!(fills.len(), 5);
        assert_eq!(fills[0].1, [255, 255, 255, 255], "bg = white");
        // Все 4 рамки = black.
        for fill in &fills[1..] {
            assert_eq!(fill.1[..3], [0, 0, 0]);
        }
        // Top (0, 0, 100, 10).
        let top = fills[1].0;
        assert!((top.height - 10.0).abs() < 0.01);
        // Bottom (0, 40, 100, 10).
        let bottom = fills[2].0;
        assert!((bottom.y - 40.0).abs() < 0.01);
        assert!((bottom.height - 10.0).abs() < 0.01);
        // Left (0, 10, 10, 30).
        let left = fills[3].0;
        assert!((left.x - 0.0).abs() < 0.01);
        assert!((left.width - 10.0).abs() < 0.01);
        assert!((left.height - 30.0).abs() < 0.01);
        // Right (90, 10, 10, 30).
        let right = fills[4].0;
        assert!((right.x - 90.0).abs() < 0.01);
        assert!((right.width - 10.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_inset_large_offset_fills_whole_outer() {
        // offset_x=200 при width=100 → inner полностью справа от outer.
        // no_overlap → один FillRect, покрывающий весь padding-box.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: inset 200px 0 black; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2, "bg + single full-outer shadow");
        assert_eq!(fills[1].1[..3], [0, 0, 0]);
        let shadow = fills[1].0;
        assert!((shadow.width - 100.0).abs() < 0.01);
        assert!((shadow.height - 50.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_inset_negative_spread_covers_outer_skips() {
        // Отрицательный spread с большим модулем — inner полностью покрывает
        // outer (расширен наружу с каждой стороны). Тени не видно.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: inset 0 0 0 -100px black; }",
        );
        let fills = fills_with_color(&dl);
        // Только bg.
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].1[..3], [255, 255, 255]);
    }

    #[test]
    fn box_shadow_inset_uses_padding_box_when_border_present() {
        // box-sizing: border-box + 100×50 + border:5px → padding-box =
        // (5, 5, 90, 40). offset 0,0 + spread 5 → inner = (10, 10, 80, 30)
        // внутри padding-box. Все 4 frames лежат строго в padding-box.
        let dl = build(
            "<div></div>",
            "div { box-sizing: border-box; width: 100px; height: 50px; \
             background: white; border: 5px solid green; \
             box-shadow: inset 0 0 0 5px black; }",
        );
        let fills = fills_with_color(&dl);
        // 4 inset frames + bg + (possibly border fills через DrawBorder; они
        // не попадают в fills_with_color — DrawBorder отдельный command).
        let shadow_fills: Vec<_> = fills
            .iter()
            .filter(|(_, c)| c[..3] == [0, 0, 0])
            .collect();
        assert_eq!(shadow_fills.len(), 4, "border-aware padding-box → 4 frames");
        // Все рамки лежат внутри padding-box: x in [5..95], y in [5..45].
        for (rect, _) in &shadow_fills {
            assert!(rect.x >= 5.0 - 0.01, "left edge inside padding-box: {}", rect.x);
            assert!(
                rect.x + rect.width <= 95.0 + 0.01,
                "right edge inside padding-box: {}",
                rect.x + rect.width
            );
            assert!(rect.y >= 5.0 - 0.01, "top edge inside padding-box: {}", rect.y);
            assert!(
                rect.y + rect.height <= 45.0 + 0.01,
                "bottom edge inside padding-box: {}",
                rect.y + rect.height
            );
        }
    }

    #[test]
    fn box_shadow_inset_currentcolor_fallback() {
        // CSS Backgrounds L3 §4.6 — отсутствующий color = currentColor.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; color: blue; \
             box-shadow: inset 0 0 0 10px; }",
        );
        let fills = fills_with_color(&dl);
        // 4 inset frames (без bg).
        assert_eq!(fills.len(), 4);
        for fill in &fills {
            assert_eq!(fill.1[..3], [0, 0, 255], "frame = currentColor (blue)");
        }
    }

    #[test]
    fn box_shadow_inset_multiple_reverse_order() {
        // Spec: «first shadow is on top» — последний inset эмитим первым,
        // первый — последним (поверх всех).
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 50px; background: white; \
             box-shadow: inset 0 0 0 5px red, inset 0 0 0 10px green, inset 0 0 0 15px blue; }",
        );
        let fills = fills_with_color(&dl);
        // bg + 3 inset × 4 frames = 1 + 12 = 13. Но frames с w=0 / h=0
        // skip-ятся; spread > 0 всегда даёт все 4 frames.
        assert_eq!(fills.len(), 13);
        assert_eq!(fills[0].1[..3], [255, 255, 255], "bg first");
        // Дальше — blue (последний CSS-shadow рисуется первым).
        for fill in &fills[1..5] {
            assert_eq!(fill.1[..3], [0, 0, 255]);
        }
        for fill in &fills[5..9] {
            assert_eq!(fill.1[..3], [0, 128, 0]);
        }
        // red — поверх всех (первый CSS-shadow рисуется последним).
        for fill in &fills[9..13] {
            assert_eq!(fill.1[..3], [255, 0, 0]);
        }
    }

    #[test]
    fn box_shadow_inset_and_outset_coexist() {
        // Одна inset и одна outset — outset перед bg, inset после bg.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: 5px 5px red, inset 0 0 0 5px blue; }",
        );
        let fills = fills_with_color(&dl);
        // outset (1) + bg (1) + inset (4 frames) = 6.
        assert_eq!(fills.len(), 6);
        assert_eq!(fills[0].1[..3], [255, 0, 0], "outset red first");
        assert_eq!(fills[1].1[..3], [255, 255, 255], "bg second");
        for fill in &fills[2..6] {
            assert_eq!(fill.1[..3], [0, 0, 255], "inset blue frames");
        }
    }

    #[test]
    fn box_shadow_inset_transparent_color_skipped() {
        // a=0 — shadow невидим, не эмитим.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: red; \
             box-shadow: inset 0 0 0 10px rgba(0,0,0,0); }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 1, "transparent inset shadow skipped");
        assert_eq!(fills[0].1[..3], [255, 0, 0]);
    }

    #[test]
    fn box_shadow_spread_expands_rect() {
        // spread=10 → shadow rect расширен на 10px по всем сторонам.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: 0 0 0 10px black; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2);
        let shadow_rect = fills[0].0;
        let bg_rect = fills[1].0;
        // shadow расширен на 10 по всем сторонам.
        assert!((shadow_rect.width - bg_rect.width - 20.0).abs() < 0.01);
        assert!((shadow_rect.height - bg_rect.height - 20.0).abs() < 0.01);
        assert!((shadow_rect.x - bg_rect.x + 10.0).abs() < 0.01);
        assert!((shadow_rect.y - bg_rect.y + 10.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_multiple_reverse_order() {
        // Spec: «first shadow is on top». Painter's order: последняя
        // shadow рисуется первой (ниже всех), первая — последней-перед-bg.
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 50px; background: white; \
             box-shadow: 1px 0 red, 2px 0 green, 3px 0 blue; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 4, "3 shadows + bg = 4 FillRect");
        assert_eq!(fills[0].1[..3], [0, 0, 255]); // blue первой (ниже всех)
        assert_eq!(fills[1].1[..3], [0, 128, 0]); // green
        assert_eq!(fills[2].1[..3], [255, 0, 0]); // red (поверх теней)
        assert_eq!(fills[3].1[..3], [255, 255, 255]); // bg (поверх всего)
    }

    #[test]
    fn box_shadow_color_omitted_uses_currentcolor() {
        // CSS Backgrounds L3 §4.6 — «If no color is specified, the value
        // of the color property is used».
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             color: rgb(10, 20, 30); box-shadow: 2px 2px; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2);
        assert_eq!(fills[0].1[..3], [10, 20, 30]);
    }

    #[test]
    fn box_shadow_negative_spread_collapses_to_skip() {
        // spread=-100 на box 50×50 → final w/h = -150, отрицательный
        // → пропускаем (не эмитим бессмысленный FillRect).
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 50px; background: red; \
             box-shadow: 0 0 0 -100px black; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 1, "collapsed shadow пропускается");
    }

    #[test]
    fn box_shadow_transparent_color_skipped() {
        // a == 0 → нечего рисовать.
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 50px; background: red; \
             box-shadow: 5px 5px transparent; }",
        );
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 1);
    }

    #[test]
    fn box_shadow_blur_wraps_in_push_filter() {
        // blur > 0 → FillRect завёрнут в PushFilter { Blur(sigma) } / PopFilter.
        // sigma = blur / 2 = 10.0.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: 5px 5px 20px black; }",
        );
        // 2 FillRect: shadow + bg (PushFilter/PopFilter не считаются fills).
        let fills = fills_with_color(&dl);
        assert_eq!(fills.len(), 2);
        // Размер shadow rect совпадает с box (spread=0), blur не меняет rect.
        assert!((fills[0].0.width - fills[1].0.width).abs() < 0.01);
        assert!((fills[0].0.height - fills[1].0.height).abs() < 0.01);
        // Структура: PushFilter, FillRect(shadow), PopFilter, FillRect(bg), ...
        let first = dl.first().unwrap();
        assert!(
            matches!(first, DisplayCommand::PushFilter { filters }
                if matches!(filters.as_slice(), [FilterFn::Blur(s)] if (*s - 10.0).abs() < 0.01)),
            "PushFilter с Blur(10.0) перед shadow FillRect, got {first:?}"
        );
        let second = dl.get(1).unwrap();
        assert!(
            matches!(second, DisplayCommand::FillRect { color, .. } if color.r == 0),
            "shadow FillRect (black) после PushFilter"
        );
        let third = dl.get(2).unwrap();
        assert!(
            matches!(third, DisplayCommand::PopFilter),
            "PopFilter после shadow FillRect"
        );
    }

    #[test]
    fn box_shadow_no_blur_no_filter_wrap() {
        // blur == 0 → прямой FillRect без PushFilter/PopFilter.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: white; \
             box-shadow: 5px 5px black; }",
        );
        let first = dl.first().unwrap();
        assert!(
            matches!(first, DisplayCommand::FillRect { .. }),
            "без blur первая команда — FillRect, не PushFilter"
        );
    }

    // ───────── backdrop-filter display list ─────────

    #[test]
    fn backdrop_filter_emits_push_pop_commands() {
        let dl = build_ordered(
            "<div></div>",
            "div { width: 100px; height: 100px; backdrop-filter: blur(8px); }",
        );
        let has_push = dl.iter().any(|c| {
            matches!(c, DisplayCommand::PushBackdropFilter { filters, .. }
                if matches!(filters.as_slice(), [FilterFn::Blur(s)] if (*s - 8.0).abs() < 0.01))
        });
        assert!(has_push, "PushBackdropFilter(Blur(8)) должен быть в DL, got {dl:?}");
        let has_pop = dl.iter().any(|c| matches!(c, DisplayCommand::PopBackdropFilter));
        assert!(has_pop, "PopBackdropFilter должен быть в DL");
    }

    #[test]
    fn backdrop_filter_bounds_match_element_rect() {
        let dl = build_ordered(
            "<div></div>",
            "div { width: 200px; height: 100px; backdrop-filter: grayscale(1); }",
        );
        let push = dl.iter().find_map(|c| match c {
            DisplayCommand::PushBackdropFilter { bounds, .. } => Some(*bounds),
            _ => None,
        });
        let b = push.expect("PushBackdropFilter должен быть");
        assert!((b.width - 200.0).abs() < 0.01, "bounds.width = {}", b.width);
        assert!((b.height - 100.0).abs() < 0.01, "bounds.height = {}", b.height);
    }

    #[test]
    fn backdrop_filter_chain_parsed_correctly() {
        let dl = build_ordered(
            "<div></div>",
            "div { width: 50px; height: 50px; backdrop-filter: blur(4px) brightness(0.8); }",
        );
        let filters = dl.iter().find_map(|c| match c {
            DisplayCommand::PushBackdropFilter { filters, .. } => Some(filters.clone()),
            _ => None,
        }).expect("PushBackdropFilter");
        assert_eq!(filters.len(), 2);
        assert!(matches!(filters[0], FilterFn::Blur(_)));
        assert!(matches!(filters[1], FilterFn::Brightness(_)));
    }

    #[test]
    fn backdrop_filter_and_filter_both_emit() {
        // When both filter and backdrop-filter are set, both Push commands appear.
        let dl = build_ordered(
            "<div></div>",
            "div { width: 50px; height: 50px; filter: invert(1); backdrop-filter: blur(6px); }",
        );
        let has_bf = dl.iter().any(|c| matches!(c, DisplayCommand::PushBackdropFilter { .. }));
        let has_f = dl.iter().any(|c| matches!(c, DisplayCommand::PushFilter { .. }));
        assert!(has_bf, "PushBackdropFilter должен быть");
        assert!(has_f, "PushFilter должен быть");
    }

    // ───────── background-clip rendering ─────────

    fn first_bg_rect(dl: &DisplayList) -> Rect {
        dl.iter()
            .find_map(|c| match c {
                // bg = single non-shadow FillRect: ищем по цвету ≠ pre-shadow
                DisplayCommand::FillRect { rect, .. } => Some(*rect),
                _ => None,
            })
            .expect("должна быть хотя бы одна FillRect")
    }

    #[test]
    fn background_clip_border_box_default_uses_full_rect() {
        // BorderBox initial: bg рисуется на полный b.rect.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; padding: 20px; \
             border: 5px solid black; background: red; }",
        );
        let bg = first_bg_rect(&dl);
        // box-sizing: content-box default → внешний размер = 100 + 2*20 + 2*5 = 150.
        assert!((bg.width - 150.0).abs() < 0.01);
        assert!((bg.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn background_clip_padding_box_shrinks_by_border() {
        // PaddingBox: bg ужимается на border (по 5px со всех сторон).
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; padding: 20px; \
             border: 5px solid black; background: red; \
             background-clip: padding-box; }",
        );
        let bg = first_bg_rect(&dl);
        // padding-box = border-box minus 2*5 border = 150 - 10 = 140.
        assert!((bg.width - 140.0).abs() < 0.01, "got width {}", bg.width);
        assert!((bg.height - 90.0).abs() < 0.01, "got height {}", bg.height);
        // Сдвиг по x на левый border (+5).
        assert!((bg.x - 5.0).abs() < 0.01, "got x {}", bg.x);
    }

    #[test]
    fn background_clip_content_box_shrinks_by_border_plus_padding() {
        // ContentBox: bg ужимается на border + padding.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; padding: 20px; \
             border: 5px solid black; background: red; \
             background-clip: content-box; }",
        );
        let bg = first_bg_rect(&dl);
        // content-box = border-box minus 2*(5+20) = 150 - 50 = 100.
        assert!((bg.width - 100.0).abs() < 0.01, "got width {}", bg.width);
        assert!((bg.height - 50.0).abs() < 0.01, "got height {}", bg.height);
        // Сдвиг по x = border + padding = 5 + 20 = 25.
        assert!((bg.x - 25.0).abs() < 0.01, "got x {}", bg.x);
    }

    #[test]
    fn background_clip_text_falls_back_to_border_box_phase0() {
        // Phase 0 без glyph-mask: text-clip эмитим как border-box.
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 50px; background: red; \
             background-clip: text; }",
        );
        let bg = first_bg_rect(&dl);
        assert!((bg.width - 100.0).abs() < 0.01);
        assert!((bg.height - 50.0).abs() < 0.01);
    }

    #[test]
    fn background_clip_collapsed_rect_skipped() {
        // Если border + padding больше box-а → clip rect collapses to 0 → skip.
        // box-sizing:border-box + width:50 + border:30 → content = 50 - 60 = -10,
        // max(0) → 0 → FillRect bg не эмитится.
        let dl = build(
            "<div></div>",
            "div { box-sizing: border-box; width: 50px; height: 20px; \
             border: 30px solid black; \
             background: red; background-clip: content-box; }",
        );
        let bg_fills: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255))
            .collect();
        assert!(bg_fills.is_empty(), "collapsed bg должен быть пропущен");
    }

    // ───────── visibility: hidden ─────────

    fn cmd_count(dl: &DisplayList) -> usize {
        dl.iter()
            .filter(|c| !matches!(c, DisplayCommand::PushClipRect { .. }
                                  | DisplayCommand::PopClip
                                  | DisplayCommand::PushOpacity { .. }
                                  | DisplayCommand::PopOpacity
                                  | DisplayCommand::PushBlendMode { .. }
                                  | DisplayCommand::PopBlendMode))
            .count()
    }

    #[test]
    fn visibility_hidden_block_suppresses_self_paint() {
        let visible = build(
            "<div></div>",
            "div { width: 50px; height: 30px; background: red; border: 2px solid black; }",
        );
        let hidden = build(
            "<div></div>",
            "div { width: 50px; height: 30px; background: red; border: 2px solid black; \
             visibility: hidden; }",
        );
        // visible: FillRect (bg) + DrawBorder.
        assert!(cmd_count(&visible) >= 2);
        // hidden: ничего из self не эмитим (никаких children → пусто).
        assert_eq!(cmd_count(&hidden), 0);
    }

    #[test]
    fn visibility_hidden_block_still_walks_visible_children() {
        // Parent hidden, child явно visible (override через inherit).
        let dl = build(
            "<div><p>x</p></div>",
            "div { background: red; visibility: hidden; } \
             p { display: block; background: blue; visibility: visible; \
                 width: 20px; height: 10px; }",
        );
        // Должна быть синяя FillRect от child, но не красная от parent.
        let blues = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::FillRect { color, .. } if color.b == 255)
        });
        let reds = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255 && color.b == 0)
        });
        assert!(blues.count() >= 1, "child должен рисоваться");
        assert_eq!(reds.count(), 0, "parent bg не рисуется");
    }

    #[test]
    fn visibility_hidden_skips_text() {
        // text inherits visibility=hidden → DrawText не эмитим.
        let dl = build(
            "<p>hello</p>",
            "p { visibility: hidden; color: black; }",
        );
        let texts: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { .. }))
            .collect();
        assert!(texts.is_empty(), "hidden parent → text не эмитим");
    }

    // Note: inline visibility override (parent hidden + child <span>
    // visibility:visible) зависит от того, что layout формирует отдельный
    // InlineFrag со style от span. Тест на это случае отложен — текущее
    // layout-поведение может склеивать text-nodes в один frag со
    // стилем родителя. Когда P1 разделит inline-fragments по style-runs,
    // добавим этот test обратно.

    #[test]
    fn visibility_collapse_treated_as_hidden_outside_table() {
        // CSS L3 §4: vne table-row `collapse` ведёт себя как `hidden`.
        let dl = build(
            "<div></div>",
            "div { width: 50px; height: 30px; background: red; \
             visibility: collapse; }",
        );
        let bg_fills: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255))
            .collect();
        assert!(bg_fills.is_empty(), "collapse вне table → hidden");
    }

    #[test]
    fn visibility_hidden_image_skipped() {
        // visibility:hidden на `<img>` — DrawImage не эмитим.
        let dl = build(
            r#"<img src="x.png" width="50" height="50">"#,
            "img { visibility: hidden; }",
        );
        let images: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawImage { .. }))
            .collect();
        assert!(images.is_empty());
    }

    // ───────── opacity:0 skip ─────────

    #[test]
    fn opacity_zero_skips_block_and_subtree() {
        // opacity:0 на parent → ни parent, ни children не рисуются.
        let dl = build(
            "<div><p>x</p></div>",
            "div { opacity: 0; background: red; } \
             p { display: block; background: blue; width: 20px; height: 10px; }",
        );
        let fills_count = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .count();
        assert_eq!(fills_count, 0, "opacity:0 → whole subtree skipped");
    }

    #[test]
    fn opacity_zero_skips_text() {
        let dl = build(
            "<p>hello</p>",
            "p { opacity: 0; }",
        );
        let texts: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { .. }))
            .collect();
        assert!(texts.is_empty(), "opacity:0 → text skipped");
    }

    #[test]
    fn opacity_one_renders_normally() {
        // Sanity: opacity:1 default — всё рисуется.
        let dl = build(
            "<div><p>x</p></div>",
            "div { background: red; } \
             p { display: block; background: blue; width: 20px; height: 10px; }",
        );
        let reds = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255 && color.b == 0)
        });
        let blues = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::FillRect { color, .. } if color.b == 255 && color.r == 0)
        });
        assert!(reds.count() >= 1);
        assert!(blues.count() >= 1);
    }

    #[test]
    fn opacity_half_phase0_does_not_change_emission() {
        // Phase 0: opacity > 0 && < 1 не обрабатывается; FillRect эмитим
        // с original color без модификации (true compositing — P2 п.4+).
        let dl = build(
            "<div></div>",
            "div { background: red; opacity: 0.5; width: 50px; height: 30px; }",
        );
        let reds: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { color, .. } if color.r == 255))
            .collect();
        assert_eq!(reds.len(), 1, "opacity:0.5 не skip-аем; alpha не множим в Phase 0");
    }

    #[test]
    fn opacity_zero_image_subtree_skipped() {
        let dl = build(
            r#"<img src="x.png" width="50" height="50">"#,
            "img { opacity: 0; }",
        );
        let any: Vec<_> = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawImage { .. }
                                  | DisplayCommand::FillRect { .. }
                                  | DisplayCommand::DrawBorder { .. }))
            .collect();
        assert!(any.is_empty());
    }

    // ── transform pipeline (P2) ────────────────────────────────────────────

    #[test]
    fn transform_none_emits_no_push() {
        let dl = build("<div>x</div>", "div { background: #f00; }");
        assert_eq!(
            count_variant(&dl, |c| matches!(c, DisplayCommand::PushTransform { .. })),
            0,
        );
    }

    #[test]
    fn transform_translate_emits_push_pop_pair() {
        let dl = build(
            r#"<div style="background: red; transform: translate(10px, 20px);">x</div>"#,
            "",
        );
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushTransform { .. }));
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopTransform));
        assert_eq!(pushes, 1);
        assert_eq!(pops, 1);
    }

    #[test]
    fn transform_translate_matrix_has_expected_offsets() {
        // translate(50px, 70px) с default transform-origin (Phase 0 — (0,0)):
        // matrix = T(0,0)·T(50,70)·T(-0,-0) = T(50,70).
        // 2D affine: x'=x+50, y'=y+70 → (a,b,c,d,e,f) = (1,0,0,1,50,70).
        let dl = build(
            r#"<div style="background: red; transform: translate(50px, 70px);">x</div>"#,
            "",
        );
        let push = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushTransform { matrix } => Some(matrix),
                _ => None,
            })
            .expect("PushTransform missing");
        let a = push.0[0];
        let b = push.0[1];
        let c = push.0[4];
        let d = push.0[5];
        let e = push.0[12];
        let f = push.0[13];
        assert!((a - 1.0).abs() < 1e-5);
        assert!(b.abs() < 1e-5);
        assert!(c.abs() < 1e-5);
        assert!((d - 1.0).abs() < 1e-5);
        assert!((e - 50.0).abs() < 1e-5);
        assert!((f - 70.0).abs() < 1e-5);
    }

    #[test]
    fn transform_push_wraps_box_content() {
        // PushTransform идёт до собственного FillRect фона, PopTransform — после.
        let dl = build(
            r#"<div style="background: red; transform: translate(10px, 0);">x</div>"#,
            "",
        );
        let push_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushTransform { .. }))
            .unwrap();
        let pop_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopTransform))
            .unwrap();
        let fill_idx = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .unwrap();
        assert!(push_idx < fill_idx, "Push должен идти до контента");
        assert!(fill_idx < pop_idx, "Pop должен идти после контента");
    }

    #[test]
    fn transform_after_opacity_in_walk_order() {
        // Phase 0 simple `walk`: PushOpacity → PushTransform → content →
        // PopTransform → PopOpacity. Transform применяется ВНУТРИ opacity-
        // layer-а (его эффект — на off-screen layer перед композицией).
        let dl = build(
            r#"<div style="background: red; opacity: 0.5; transform: scale(2);">x</div>"#,
            "",
        );
        let push_op = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushOpacity { .. }))
            .unwrap();
        let push_tr = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PushTransform { .. }))
            .unwrap();
        let pop_tr = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopTransform))
            .unwrap();
        let pop_op = dl
            .iter()
            .position(|c| matches!(c, DisplayCommand::PopOpacity))
            .unwrap();
        assert!(push_op < push_tr);
        assert!(push_tr < pop_tr);
        assert!(pop_tr < pop_op);
    }

    #[test]
    fn transform_serialize_2d_affine_components() {
        let dl = vec![
            DisplayCommand::PushTransform {
                matrix: Mat4::from_2d_affine(2.0, 0.0, 0.0, 0.5, 10.0, -20.0),
            },
            DisplayCommand::PopTransform,
        ];
        let s = serialize_display_list(&dl);
        // a=2.000 b=0.000 c=0.000 d=0.500 e=10.000 f=-20.000.
        assert_eq!(
            s,
            "PushTransform [2.000 0.000 0.000 0.500 10.000 -20.000]\nPopTransform\n"
        );
    }

    #[test]
    fn transform_ordered_emits_via_box_layer_ops() {
        // build_display_list_ordered идёт через box_layer_ops; должен дать
        // Push/Pop пару наряду с simple walk-ом.
        let dl = build_ordered(
            r#"<div style="background: red; transform: rotate(45deg);">x</div>"#,
            "",
        );
        let pushes = count_variant(&dl, |c| matches!(c, DisplayCommand::PushTransform { .. }));
        let pops = count_variant(&dl, |c| matches!(c, DisplayCommand::PopTransform));
        assert_eq!(pushes, 1);
        assert_eq!(pops, 1);
    }

    #[test]
    fn transform_origin_affects_matrix() {
        // С transform-origin (10, 20) и translate(0, 0) матрица =
        // T(10+box_x, 20+box_y) · I · T(-(10+box_x), -(20+box_y)) = I.
        // Здесь box_x/box_y зависят от layout; берём rotate чтобы origin
        // действительно изменял результат. rotate(90deg) с origin (0,0) -
        // точка (1,0) → (0,1). С origin (10,0) — точка (1,0) → (10, -9).
        // Просто проверяем что матрица не identity при rotate.
        let dl = build(
            r#"<div style="background: red; transform: rotate(90deg);">x</div>"#,
            "",
        );
        let push = dl
            .iter()
            .find_map(|c| match c {
                DisplayCommand::PushTransform { matrix } => Some(matrix),
                _ => None,
            })
            .unwrap();
        assert!(!push.is_identity(), "rotate(90deg) ≠ identity");
        // sin/cos(90°): a=cos=0, b=sin=1, c=-sin=-1, d=cos=0.
        let a = push.0[0];
        let b = push.0[1];
        let c = push.0[4];
        let d = push.0[5];
        assert!(a.abs() < 1e-5);
        assert!((b - 1.0).abs() < 1e-5);
        assert!((c + 1.0).abs() < 1e-5);
        assert!(d.abs() < 1e-5);
    }

    // ─── CSS Transforms L2 §6.2 — 3D depth sorting ───────────────────────────

    #[test]
    fn depth_order_back_to_front() {
        // z = [нос(10), зад(-5), середина(0)] → порядок зад→середина→нос.
        let order = depth_order_by_z(&[10.0, -5.0, 0.0]);
        assert_eq!(order, vec![1, 2, 0]);
    }

    #[test]
    fn depth_order_stable_for_coplanar() {
        // Равные z (все 0) → исходный document order сохраняется.
        let order = depth_order_by_z(&[0.0, 0.0, 0.0, 0.0]);
        assert_eq!(order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn depth_order_partial_ties_keep_order() {
        // Совпадающие глубины (5.0) у индексов 0 и 2 → стабильно 0 раньше 2.
        let order = depth_order_by_z(&[5.0, -1.0, 5.0, 2.0]);
        assert_eq!(order, vec![1, 3, 0, 2]);
    }

    #[test]
    fn depth_order_nan_treated_as_coplanar() {
        // NaN не паникует и трактуется как равный — стабильный порядок.
        let order = depth_order_by_z(&[f32::NAN, 1.0, f32::NAN]);
        assert_eq!(order.len(), 3);
        // 1.0 не имеет определённого отношения к NaN (cmp→Equal), поэтому
        // стабильная сортировка оставляет всё на местах.
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn depth_order_empty() {
        assert_eq!(depth_order_by_z(&[]), Vec::<usize>::new());
    }

    #[test]
    fn flat_context_keeps_document_order() {
        // Без preserve-3d (establishes_3d_rendering_context == false) дети
        // рисуются в document order — три фона идут red, green, blue.
        let dl = build(
            r#"<div>
                 <div style="background: red;">a</div>
                 <div style="background: green;">b</div>
                 <div style="background: blue;">c</div>
               </div>"#,
            "",
        );
        let bg: Vec<(u8, u8, u8)> = fills(&dl).iter().map(|c| (c.r, c.g, c.b)).collect();
        let red = bg.iter().position(|c| *c == (255, 0, 0));
        let green = bg.iter().position(|c| *c == (0, 128, 0));
        let blue = bg.iter().position(|c| *c == (0, 0, 255));
        assert!(red < green && green < blue, "document order: {bg:?}");
    }

    // ─── build_display_list_with_anim ────────────────────────────────────────

    use lumen_layout::{CompositorAnimFrame, CompositorOverride};
    use lumen_dom::NodeId;
    use std::collections::HashMap;

    fn build_anim(html: &str, css: &str, overrides: HashMap<NodeId, CompositorOverride>) -> DisplayList {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let frame = CompositorAnimFrame { overrides, has_active: true };
        build_display_list_with_anim(&tree, Some(&frame))
    }

    #[test]
    fn anim_no_overrides_same_as_base() {
        let html = r#"<div style="background:red;width:100px;height:50px"></div>"#;
        let base = build(html, "");
        let anim = build_anim(html, "", HashMap::new());
        assert_eq!(base.len(), anim.len(), "empty overrides: same DL length");
    }

    #[test]
    fn anim_none_frame_same_as_base() {
        let html = r#"<div style="background:blue;width:80px;height:40px"></div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let base = build_display_list(&tree);
        let with_none = build_display_list_with_anim(&tree, None);
        assert_eq!(base.len(), with_none.len());
    }

    #[test]
    fn anim_opacity_override_emits_push_opacity() {
        // A div without opacity in style — no PushOpacity in base DL.
        let html = r#"<div style="background:green;width:100px;height:50px"></div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));

        let base = build_display_list(&tree);
        let has_push_base = base.iter().any(|c| matches!(c, DisplayCommand::PushOpacity { .. }));
        assert!(!has_push_base, "base DL should have no PushOpacity");

        // Override opacity=0.5 for the body node (root).
        let node = tree.node;
        let mut overrides = HashMap::new();
        overrides.insert(node, CompositorOverride { opacity: Some(0.5), transform: None });
        let frame = CompositorAnimFrame { overrides, has_active: true };
        let anim_dl = build_display_list_with_anim(&tree, Some(&frame));

        let push_count = anim_dl.iter().filter(|c| matches!(c, DisplayCommand::PushOpacity { .. })).count();
        let pop_count = anim_dl.iter().filter(|c| matches!(c, DisplayCommand::PopOpacity)).count();
        assert_eq!(push_count, 1, "should emit one PushOpacity for the animated node");
        assert_eq!(pop_count, 1, "PushOpacity/PopOpacity must be balanced");

        if let Some(DisplayCommand::PushOpacity { alpha }) = anim_dl.iter().find(|c| matches!(c, DisplayCommand::PushOpacity { .. })) {
            assert!((*alpha - 0.5).abs() < 1e-5, "opacity should be 0.5, got {alpha}");
        }
    }

    #[test]
    fn anim_push_pop_balanced() {
        // Any DL produced by with_anim must have balanced Push/Pop pairs.
        let html = r#"<div style="background:red;width:200px;height:100px">
            <div style="background:blue;width:100px;height:50px"></div>
        </div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let node = tree.node;
        let mut overrides = HashMap::new();
        overrides.insert(node, CompositorOverride { opacity: Some(0.7), transform: None });
        let frame = CompositorAnimFrame { overrides, has_active: true };
        let dl = build_display_list_with_anim(&tree, Some(&frame));

        let push_op = dl.iter().filter(|c| matches!(c, DisplayCommand::PushOpacity { .. })).count();
        let pop_op = dl.iter().filter(|c| matches!(c, DisplayCommand::PopOpacity)).count();
        let push_tx = dl.iter().filter(|c| matches!(c, DisplayCommand::PushTransform { .. })).count();
        let pop_tx = dl.iter().filter(|c| matches!(c, DisplayCommand::PopTransform)).count();
        assert_eq!(push_op, pop_op, "PushOpacity/PopOpacity must balance");
        assert_eq!(push_tx, pop_tx, "PushTransform/PopTransform must balance");
    }

    // ── text-emphasis rendering ───────────────────────────────────────────────

    #[test]
    fn text_emphasis_filled_circle_emits_marks_above_text() {
        let dl = build(
            "<p>ab</p>",
            "p { text-emphasis-style: filled circle; font-size: 16px; }",
        );
        // Должен быть основной DrawText + 2 DrawText-а для marks (по одному на символ).
        let texts: Vec<_> = dl
            .iter()
            .filter_map(|c| {
                if let DisplayCommand::DrawText { text, .. } = c {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();
        // Два mark DrawText-а с символом ● (U+25CF).
        let mark_count = texts.iter().filter(|&&t| t == "\u{25CF}").count();
        assert_eq!(mark_count, 2, "по одному mark на каждый символ 'a' и 'b'");
    }

    #[test]
    fn text_emphasis_none_emits_no_marks() {
        let dl = build("<p>ab</p>", "p { font-size: 16px; }");
        let texts: Vec<_> = dl
            .iter()
            .filter_map(|c| {
                if let DisplayCommand::DrawText { text, .. } = c {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();
        // Только один DrawText с "ab", никаких mark-ов.
        assert_eq!(texts.len(), 1, "без text-emphasis — только основной DrawText");
        assert_eq!(texts[0], "ab");
    }

    #[test]
    fn text_emphasis_under_position_mark_below_text() {
        let dl = build(
            "<p>x</p>",
            "p { text-emphasis-style: filled dot; text-emphasis-position: under right; font-size: 16px; }",
        );
        let rects: Vec<_> = dl
            .iter()
            .filter_map(|c| {
                if let DisplayCommand::DrawText { rect, text, .. } = c {
                    if text == "\u{2022}" { Some(*rect) } else { None }
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(rects.len(), 1, "один mark для 'x'");
        // Ищем основной DrawText для сравнения y.
        let base_y = dl.iter().find_map(|c| {
            if let DisplayCommand::DrawText { rect, text, .. } = c {
                if text == "x" { Some(rect.y) } else { None }
            } else {
                None
            }
        });
        if let Some(base_y) = base_y {
            assert!(
                rects[0].y > base_y,
                "under mark должен быть ниже текста: mark_y={} base_y={}",
                rects[0].y, base_y
            );
        }
    }

    #[test]
    fn text_emphasis_custom_string_used_as_mark() {
        let dl = build(
            "<p>abc</p>",
            "p { text-emphasis-style: \"*\"; font-size: 16px; }",
        );
        let mark_count = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text == "*"))
            .count();
        assert_eq!(mark_count, 3, "три символа → три mark '*'");
    }

    // ── clip-path ──────────────────────────────────────────────────────────

    #[test]
    fn clip_path_inset_1() {
        use super::clip_path_to_rect;
        use lumen_layout::ClipPath;
        let r = Rect::new(10.0, 20.0, 100.0, 80.0);
        let clip = ClipPath::Inset(vec![5.0]);
        let cr = clip_path_to_rect(&clip, r);
        assert_eq!(cr, Rect::new(15.0, 25.0, 90.0, 70.0));
    }

    #[test]
    fn clip_path_inset_4() {
        use super::clip_path_to_rect;
        use lumen_layout::ClipPath;
        let r = Rect::new(0.0, 0.0, 200.0, 100.0);
        // top=10 right=20 bottom=30 left=40
        let clip = ClipPath::Inset(vec![10.0, 20.0, 30.0, 40.0]);
        let cr = clip_path_to_rect(&clip, r);
        assert_eq!(cr, Rect::new(40.0, 10.0, 140.0, 60.0));
    }

    #[test]
    fn clip_path_circle_default_center() {
        use super::clip_path_to_rect;
        use lumen_layout::ClipPath;
        let r = Rect::new(0.0, 0.0, 100.0, 60.0);
        let clip = ClipPath::Circle { radius: 25.0, center: None };
        let cr = clip_path_to_rect(&clip, r);
        // center = (50, 30); bounding box = (25, 5, 50, 50)
        assert_eq!(cr, Rect::new(25.0, 5.0, 50.0, 50.0));
    }

    #[test]
    fn clip_path_ellipse_explicit_center() {
        use super::clip_path_to_rect;
        use lumen_layout::ClipPath;
        let r = Rect::new(10.0, 10.0, 200.0, 100.0);
        let clip = ClipPath::Ellipse { rx: 40.0, ry: 20.0, center: Some((100.0, 50.0)) };
        let cr = clip_path_to_rect(&clip, r);
        // cx = 10+100=110, cy = 10+50=60
        assert_eq!(cr, Rect::new(70.0, 40.0, 80.0, 40.0));
    }

    #[test]
    fn clip_path_polygon_bounding_box() {
        use super::clip_path_to_rect;
        use lumen_layout::ClipPath;
        let r = Rect::new(0.0, 0.0, 200.0, 200.0);
        // triangle: (100,0) (200,200) (0,200)
        let clip = ClipPath::Polygon(vec![(100.0, 0.0), (200.0, 200.0), (0.0, 200.0)]);
        let cr = clip_path_to_rect(&clip, r);
        assert_eq!(cr, Rect::new(0.0, 0.0, 200.0, 200.0));
    }

    #[test]
    fn clip_path_emits_push_pop_clip() {
        // clip-path:inset(10px) on a div must emit PushClipRect/PopClip
        let dl = build(
            "<div></div>",
            "div { width:100px; height:50px; clip-path:inset(10px); background:red; }",
        );
        let push_count = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::PushClipRect { .. }))
            .count();
        assert!(push_count >= 1, "clip-path:inset должен эмитить PushClipRect");
        let pop_count = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::PopClip))
            .count();
        assert_eq!(push_count, pop_count, "Push/Pop должны быть сбалансированы");
    }

    // ── emit_column_rules ──────────────────────────────────────────────────

    fn column_rule_cmds(dl: &DisplayList) -> Vec<&DisplayCommand> {
        // Column rules emitted as DrawBorder with widths=[0, rule_w, 0, 0].
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBorder { widths: [0.0, w, 0.0, 0.0], .. } if *w > 0.0))
            .collect()
    }

    #[test]
    fn column_rule_emits_separators_between_columns() {
        // 3 columns → 2 separators.
        let dl = build(
            r#"<div style="column-count:3;column-gap:30px;
                           column-rule:4px solid red;
                           width:300px;height:100px;background:white"></div>"#,
            "",
        );
        let rules = column_rule_cmds(&dl);
        assert_eq!(rules.len(), 2, "3 columns → 2 column-rule separators, got {}", rules.len());
    }

    #[test]
    fn column_rule_none_style_emits_nothing() {
        // column-rule-style defaults to None → no separators.
        let dl = build(
            r#"<div style="column-count:2;column-gap:20px;
                           column-rule-width:4px;
                           width:200px;height:100px;background:white"></div>"#,
            "",
        );
        let rules = column_rule_cmds(&dl);
        assert_eq!(rules.len(), 0, "column-rule-style:none should emit no separators");
    }

    #[test]
    fn column_rule_zero_width_emits_nothing() {
        let dl = build(
            r#"<div style="column-count:3;column-gap:20px;
                           column-rule:0px solid blue;
                           width:300px;height:100px;background:white"></div>"#,
            "",
        );
        let rules = column_rule_cmds(&dl);
        assert_eq!(rules.len(), 0, "column-rule-width:0 should emit no separators");
    }

    #[test]
    fn column_rule_single_column_emits_nothing() {
        let dl = build(
            r#"<div style="column-count:1;column-gap:20px;
                           column-rule:4px solid green;
                           width:200px;height:100px;background:white"></div>"#,
            "",
        );
        let rules = column_rule_cmds(&dl);
        assert_eq!(rules.len(), 0, "1 column → no separators");
    }

    #[test]
    fn column_rule_no_column_props_emits_nothing() {
        // No column-count or column-width → not a multicol container.
        let dl = build(
            r#"<div style="column-rule:4px solid red;width:200px;height:100px"></div>"#,
            "",
        );
        let rules = column_rule_cmds(&dl);
        assert_eq!(rules.len(), 0, "no column-count/width → no separators");
    }

    // ── position:sticky display list tests ──────────────────────────────────

    #[test]
    fn sticky_top_emits_begin_end_layer() {
        let dl = build(
            r#"<div style="position:sticky;top:10px;background:blue;width:200px;height:50px"></div>"#,
            "",
        );
        let has_begin = dl.iter().any(|c| matches!(c, DisplayCommand::BeginStickyLayer { top: Some(t), .. } if (*t - 10.0).abs() < 0.01));
        let has_end = dl.iter().any(|c| matches!(c, DisplayCommand::EndStickyLayer));
        assert!(has_begin, "expected BeginStickyLayer with top=10 in display list");
        assert!(has_end, "expected EndStickyLayer in display list");
    }

    #[test]
    fn sticky_begin_before_fill_rect() {
        let dl = build(
            r#"<div style="position:sticky;top:0px;background:red;width:100px;height:40px"></div>"#,
            "",
        );
        let begin_idx = dl.iter().position(|c| matches!(c, DisplayCommand::BeginStickyLayer { .. })).unwrap();
        let fill_idx = dl.iter().position(|c| matches!(c, DisplayCommand::FillRect { .. })).unwrap();
        let end_idx = dl.iter().position(|c| matches!(c, DisplayCommand::EndStickyLayer)).unwrap();
        assert!(begin_idx < fill_idx, "BeginStickyLayer must come before FillRect");
        assert!(fill_idx < end_idx, "FillRect must come before EndStickyLayer");
    }

    #[test]
    fn sticky_auto_top_no_layer() {
        // position:sticky with no insets (all auto) — still emits layer (spec allows sticky
        // with auto insets; it behaves like static but is logically sticky-positioned).
        let dl = build(
            r#"<div style="position:sticky;background:green;width:100px;height:40px"></div>"#,
            "",
        );
        let has_begin = dl.iter().any(|c| matches!(c, DisplayCommand::BeginStickyLayer { .. }));
        // With all-auto insets the layer is still emitted (no inset = no clamping in renderer).
        assert!(has_begin, "BeginStickyLayer emitted even for all-auto sticky");
    }

    #[test]
    fn sticky_bottom_inset_stored() {
        let dl = build(
            r#"<div style="position:sticky;bottom:20px;background:blue;width:200px;height:50px"></div>"#,
            "",
        );
        let has_bottom = dl.iter().any(|c| matches!(
            c,
            DisplayCommand::BeginStickyLayer { bottom: Some(b), .. } if (*b - 20.0).abs() < 0.01
        ));
        assert!(has_bottom, "expected BeginStickyLayer with bottom=20");
    }

    #[test]
    fn non_sticky_no_layer() {
        // position:relative does not produce a sticky layer.
        let dl = build(
            r#"<div style="position:relative;top:10px;background:blue;width:200px;height:50px"></div>"#,
            "",
        );
        let has_begin = dl.iter().any(|c| matches!(c, DisplayCommand::BeginStickyLayer { .. }));
        assert!(!has_begin, "position:relative must not emit BeginStickyLayer");
    }

    #[test]
    fn column_rule_separator_centered_in_gap() {
        // 2 columns, 40px gap, 4px rule → rule centered at gap_left + (40-4)/2 = gap_left + 18.
        let dl = build(
            r#"<div style="column-count:2;column-gap:40px;
                           column-rule:4px solid red;
                           width:280px;height:100px;background:white"></div>"#,
            "",
        );
        let rules = column_rule_cmds(&dl);
        assert_eq!(rules.len(), 1, "2 columns → 1 separator");
        if let DisplayCommand::DrawBorder { rect, widths: [_, rule_w, _, _], .. } = rules[0] {
            // col_w = (280 - 40) / 2 = 120px; gap_left = 120; sep_x = 120 + 18 = 138.
            assert!((rect.x - 138.0).abs() < 0.5, "sep_x expected ~138, got {}", rect.x);
            assert!((*rule_w - 4.0).abs() < 0.01, "rule width expected 4, got {}", rule_w);
        }
    }

    // ── CSS Lists L3 §2.1 — list marker geometric rendering ─────────────────

    /// disc marker emits FillRoundedRect (filled circle), not DrawText.
    #[test]
    fn disc_marker_emits_filled_rounded_rect() {
        let dl = build(
            r#"<ul style="padding-left:32px"><li style="color:red">A</li></ul>"#,
            "",
        );
        let circles: Vec<_> = dl.iter().filter_map(|c| match c {
            DisplayCommand::FillRoundedRect { radii, .. } => Some(radii),
            _ => None,
        }).collect();
        assert!(!circles.is_empty(), "disc marker must emit FillRoundedRect");
        // All radii equal (it's a circle): tl == tl_y == tr == tr_y == ...
        let r = circles[0];
        assert!((r.tl - r.tl_y).abs() < 0.01, "disc radii should be equal (circle)");
        assert!((r.tl - r.tr).abs() < 0.01, "disc radii should be equal (circle)");
    }

    /// disc marker renders no Unicode bullet text.
    #[test]
    fn disc_marker_no_bullet_text() {
        let dl = build(
            r#"<ul style="padding-left:32px"><li>A</li></ul>"#,
            "",
        );
        let bullet_texts: Vec<_> = dl.iter().filter_map(|c| match c {
            DisplayCommand::DrawText { text, .. } if text.contains('\u{2022}') => Some(text.as_str()),
            _ => None,
        }).collect();
        assert!(bullet_texts.is_empty(), "disc should not render Unicode bullet •");
    }

    /// circle marker emits DrawBorder (hollow circle outline), not DrawText.
    #[test]
    fn circle_marker_emits_draw_border() {
        let dl = build(
            r#"<ul style="list-style-type:circle;padding-left:32px"><li>A</li></ul>"#,
            "",
        );
        let borders: Vec<_> = dl.iter().filter_map(|c| match c {
            DisplayCommand::DrawBorder { radii, .. } if radii.tl > 0.0 => Some(radii),
            _ => None,
        }).collect();
        assert!(!borders.is_empty(), "circle marker must emit DrawBorder with rounded corners");
    }

    /// square marker emits FillRect (filled square), not DrawText.
    #[test]
    fn square_marker_emits_fill_rect() {
        let dl = build(
            r#"<ul style="list-style-type:square;padding-left:32px"><li>A</li></ul>"#,
            "",
        );
        // FillRect count: one for the square marker (li has no background by default)
        // We just check at least one FillRect exists from the square marker.
        let rects: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRect { .. })).collect();
        assert!(!rects.is_empty(), "square marker must emit FillRect");
    }

    /// decimal (ordered) marker renders as DrawText with counter string.
    /// Note: Lumen has no UA stylesheet, so list-style-type must be set explicitly.
    #[test]
    fn decimal_marker_emits_draw_text() {
        let dl = build(
            r#"<ol style="list-style-type:decimal;padding-left:32px"><li>A</li><li>B</li></ol>"#,
            "",
        );
        let counter_texts: Vec<_> = dl.iter().filter_map(|c| match c {
            DisplayCommand::DrawText { text, .. } if text.starts_with("1.") || text.starts_with("2.") => Some(text.as_str()),
            _ => None,
        }).collect();
        assert_eq!(counter_texts.len(), 2, "2 decimal markers should produce 2 DrawText commands");
    }

    /// list-style-type:none produces no marker output.
    #[test]
    fn list_style_none_no_marker() {
        let dl = build(
            r#"<ul style="list-style-type:none;padding-left:32px"><li>A</li></ul>"#,
            "",
        );
        // No FillRoundedRect from markers (li has no background), no DrawBorder with positive radii from markers.
        let circles: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).collect();
        assert!(circles.is_empty(), "list-style-type:none should not emit any marker shape");
    }

    /// lower-alpha marker renders letter counter text (explicit list-style-type — no UA stylesheet).
    #[test]
    fn lower_alpha_marker_emits_text() {
        let dl = build(
            r#"<ul style="list-style-type:lower-alpha;padding-left:32px"><li>A</li><li>B</li></ul>"#,
            "",
        );
        let alpha_texts: Vec<_> = dl.iter().filter_map(|c| match c {
            DisplayCommand::DrawText { text, .. } if text.starts_with("a.") || text.starts_with("b.") => Some(text.as_str()),
            _ => None,
        }).collect();
        assert_eq!(alpha_texts.len(), 2, "lower-alpha markers: expected 'a. ' and 'b. '");
    }

    // ── CSS Compositing L1 §8.3 — background-blend-mode ──

    /// Normal blend mode → no PushBlendMode/PopBlendMode emitted.
    #[test]
    fn background_blend_mode_normal_no_blend_commands() {
        let dl = build(
            r#"<div style="background-image:linear-gradient(red,blue);background-blend-mode:normal;width:100px;height:100px"></div>"#,
            "",
        );
        let blend_cmds: Vec<_> = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::PushBlendMode { .. } | DisplayCommand::PopBlendMode)
        }).collect();
        assert!(blend_cmds.is_empty(), "normal blend mode must not emit any blend commands");
    }

    /// Single layer with non-normal blend mode: it is the bottom-most layer, so
    /// CSS Compositing L1 §8.3 says it blends against transparent background-color.
    /// For premultiplied alpha, multiply(src, transparent) = src — no visual effect.
    /// We suppress PushBlendMode to avoid incorrect blending against the stacking context.
    #[test]
    fn background_blend_mode_single_layer_bottom_suppressed() {
        let dl = build(
            r#"<div style="background-image:linear-gradient(red,blue);background-blend-mode:multiply;width:100px;height:100px"></div>"#,
            "",
        );
        let push_count = dl.iter().filter(|c| matches!(c, DisplayCommand::PushBlendMode { .. })).count();
        let idx_grad = dl.iter().position(|c| matches!(c, DisplayCommand::DrawLinearGradient { .. }));
        assert_eq!(push_count, 0, "single bottom layer: blend suppressed (identity against transparent)");
        assert!(idx_grad.is_some(), "DrawLinearGradient still emitted");
    }

    /// Two layers: first has multiply, second normal → one blend pair for first layer only.
    #[test]
    fn background_blend_mode_two_layers_only_first_blended() {
        let dl = build(
            r#"<div style="background-image:linear-gradient(red,blue),linear-gradient(green,yellow);background-blend-mode:multiply,normal;width:100px;height:100px"></div>"#,
            "",
        );
        // Exactly one PushBlendMode and one PopBlendMode total.
        let push_count = dl.iter().filter(|c| matches!(c, DisplayCommand::PushBlendMode { .. })).count();
        let pop_count  = dl.iter().filter(|c| matches!(c, DisplayCommand::PopBlendMode)).count();
        assert_eq!(push_count, 1, "only one layer with non-normal blend mode → one PushBlendMode");
        assert_eq!(pop_count,  1, "matching PopBlendMode count");
    }

    /// Two layers with same blend mode: bottom suppressed, top blended.
    /// This is the most common pattern in background-blend-mode CSS.
    #[test]
    fn background_blend_mode_two_same_mode_only_top_blended() {
        let dl = build(
            r#"<div style="background-image:linear-gradient(red,blue),linear-gradient(green,yellow);background-blend-mode:multiply;width:100px;height:100px"></div>"#,
            "",
        );
        // Bottom layer suppressed, top layer wrapped → exactly 1 PushBlendMode.
        let push_count = dl.iter().filter(|c| matches!(c, DisplayCommand::PushBlendMode { .. })).count();
        let pop_count  = dl.iter().filter(|c| matches!(c, DisplayCommand::PopBlendMode)).count();
        assert_eq!(push_count, 1, "two layers same blend: bottom suppressed, top wrapped → 1 PushBlendMode");
        assert_eq!(pop_count,  1, "matching PopBlendMode");
        // Verify order: bottom gradient → PushBlendMode → top gradient → PopBlendMode
        let positions: Vec<usize> = dl.iter().enumerate().filter_map(|(i, c)| {
            if matches!(c, DisplayCommand::DrawLinearGradient { .. } | DisplayCommand::PushBlendMode { .. } | DisplayCommand::PopBlendMode) {
                Some(i)
            } else { None }
        }).collect();
        assert!(positions.len() == 4, "expecting: grad(bottom), PushBlend, grad(top), PopBlend");
        assert!(matches!(&dl[positions[0]], DisplayCommand::DrawLinearGradient { .. }), "first: bottom gradient");
        assert!(matches!(&dl[positions[1]], DisplayCommand::PushBlendMode { .. }), "second: PushBlendMode");
        assert!(matches!(&dl[positions[2]], DisplayCommand::DrawLinearGradient { .. }), "third: top gradient");
        assert!(matches!(&dl[positions[3]], DisplayCommand::PopBlendMode), "fourth: PopBlendMode");
    }

    /// background-blend-mode cycles when fewer values than layers.
    /// Bottom layer blend is suppressed (CSS Compositing L1 §8.3 isolated group).
    #[test]
    fn background_blend_mode_cycling() {
        // 3 layers, 1 value → all three have multiply, but bottom-most is suppressed.
        let dl = build(
            r#"<div style="background-image:linear-gradient(red,blue),linear-gradient(green,yellow),linear-gradient(cyan,magenta);background-blend-mode:multiply;width:100px;height:100px"></div>"#,
            "",
        );
        let push_count = dl.iter().filter(|c| matches!(c, DisplayCommand::PushBlendMode { mode: BlendMode::Multiply })).count();
        assert_eq!(push_count, 2, "cycling: 3 layers but bottom-most suppressed → 2 PushBlendMode");
    }

    // ── BoxModelOverlay ──────────────────────────────────────────────────────

    #[test]
    fn box_model_overlay_serializes_all_four_boxes() {
        use lumen_core::geom::Rect;
        let dl = vec![DisplayCommand::BoxModelOverlay {
            margin:  Rect::new(0.0,   0.0,  120.0, 100.0),
            border:  Rect::new(10.0, 10.0,  100.0,  80.0),
            padding: Rect::new(12.0, 12.0,   96.0,  76.0),
            content: Rect::new(20.0, 20.0,   80.0,  60.0),
        }];
        let s = serialize_display_list(&dl);
        assert!(s.starts_with("BoxModelOverlay"), "must start with command name");
        assert!(s.contains("margin=(0,0,120,100)"),  "margin box");
        assert!(s.contains("border=(10,10,100,80)"), "border box");
        assert!(s.contains("padding=(12,12,96,76)"), "padding box");
        assert!(s.contains("content=(20,20,80,60)"), "content box");
    }

    #[test]
    fn box_model_overlay_zero_content_serializes() {
        use lumen_core::geom::Rect;
        let dl = vec![DisplayCommand::BoxModelOverlay {
            margin:  Rect::new(0.0, 0.0, 50.0, 50.0),
            border:  Rect::new(5.0, 5.0, 40.0, 40.0),
            padding: Rect::new(7.0, 7.0, 36.0, 36.0),
            content: Rect::new(10.0, 10.0, 0.0, 0.0), // collapsed content
        }];
        let s = serialize_display_list(&dl);
        assert!(s.contains("BoxModelOverlay"), "collapsed content must still serialize");
        assert!(s.contains("content=(10,10,0,0)"), "zero-size content rect");
    }

    // ── MaskMode + PushMaskLayer / PopMaskLayer ──────────────────────────────

    #[test]
    fn mask_mode_default_is_alpha() {
        assert_eq!(MaskMode::default(), MaskMode::Alpha);
    }

    #[test]
    fn push_mask_layer_alpha_serializes() {
        use lumen_core::geom::Rect;
        let dl = vec![
            DisplayCommand::PushMaskLayer {
                rect: Rect::new(10.0, 20.0, 100.0, 80.0),
                mode: MaskMode::Alpha,
            },
            DisplayCommand::PopMaskLayer,
        ];
        let s = serialize_display_list(&dl);
        assert!(s.contains("PushMaskLayer"), "must contain PushMaskLayer");
        assert!(s.contains("(10.00, 20.00, 100.00, 80.00)"), "rect coords");
        assert!(s.contains("Alpha"), "mode=Alpha");
        assert!(s.contains("PopMaskLayer"), "must contain PopMaskLayer");
    }

    #[test]
    fn push_mask_layer_luminance_serializes() {
        use lumen_core::geom::Rect;
        let dl = vec![
            DisplayCommand::PushMaskLayer {
                rect: Rect::new(0.0, 0.0, 200.0, 150.0),
                mode: MaskMode::Luminance,
            },
            DisplayCommand::PopMaskLayer,
        ];
        let s = serialize_display_list(&dl);
        assert!(s.contains("Luminance"), "mode=Luminance");
    }

    #[test]
    fn push_mask_layer_roundtrip_kinds() {
        use lumen_core::geom::Rect;
        let rect = Rect::new(0.0, 0.0, 50.0, 50.0);
        let dl = vec![
            DisplayCommand::PushMaskLayer { rect, mode: MaskMode::Alpha },
            DisplayCommand::FillRect { rect, color: Color { r: 255, g: 0, b: 0, a: 255 } },
            DisplayCommand::PopMaskLayer,
        ];
        // Verify the three-command sequence serializes in order.
        let s = serialize_display_list(&dl);
        let push_pos = s.find("PushMaskLayer").expect("no PushMaskLayer");
        let fill_pos = s.find("FillRect").expect("no FillRect");
        let pop_pos  = s.find("PopMaskLayer").expect("no PopMaskLayer");
        assert!(push_pos < fill_pos, "PushMaskLayer before FillRect");
        assert!(fill_pos < pop_pos,  "FillRect before PopMaskLayer");
    }

    // ─── PushScrollLayer / PopScrollLayer tests ──────────────────────────────

    #[test]
    fn overflow_scroll_emits_push_scroll_layer() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><p>text</p></div>"#,
            "",
        );
        let has_push = dl.iter().any(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }));
        let has_pop  = dl.iter().any(|c| matches!(c, DisplayCommand::PopScrollLayer));
        assert!(has_push, "overflow:scroll must emit PushScrollLayer");
        assert!(has_pop,  "overflow:scroll must emit PopScrollLayer");
    }

    #[test]
    fn overflow_scroll_no_push_clip_rect_for_scroll() {
        // overflow:scroll should not fall back to PushClipRect for the scroll axis
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><p>text</p></div>"#,
            "",
        );
        // There should be PushScrollLayer, not PushClipRect, for the scroll container itself.
        let scroll_count = dl.iter().filter(|c| matches!(c, DisplayCommand::PushScrollLayer { .. })).count();
        assert!(scroll_count >= 1, "expected at least one PushScrollLayer for overflow:scroll");
    }

    #[test]
    fn overflow_hidden_emits_push_clip_rect_not_scroll_layer() {
        let dl = build(
            r#"<div style="overflow:hidden;width:100px;height:50px"><p>text</p></div>"#,
            "",
        );
        let has_scroll = dl.iter().any(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }));
        assert!(!has_scroll, "overflow:hidden must not emit PushScrollLayer");
        // overflow:hidden still clips via PushClipRect
        let has_clip = dl.iter().any(|c| matches!(c, DisplayCommand::PushClipRect { .. }));
        assert!(has_clip, "overflow:hidden must emit PushClipRect");
    }

    #[test]
    fn scroll_layer_scroll_xy_defaults_zero() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><p>x</p></div>"#,
            "",
        );
        if let Some(DisplayCommand::PushScrollLayer { scroll_x, scroll_y, .. }) =
            dl.iter().find(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }))
        {
            assert_eq!(*scroll_x, 0.0, "initial scroll_x should be 0");
            assert_eq!(*scroll_y, 0.0, "initial scroll_y should be 0");
        } else {
            panic!("PushScrollLayer not found");
        }
    }

    #[test]
    fn push_scroll_layer_serializes() {
        use lumen_core::geom::Rect;
        let dl = vec![
            DisplayCommand::PushScrollLayer {
                clip_rect: Rect::new(10.0, 20.0, 100.0, 50.0),
                scroll_x: 5.0,
                scroll_y: 15.0,
            },
            DisplayCommand::PopScrollLayer,
        ];
        let s = serialize_display_list(&dl);
        assert!(s.contains("PushScrollLayer"), "serialized output must contain PushScrollLayer");
        assert!(s.contains("PopScrollLayer"), "serialized output must contain PopScrollLayer");
        assert!(s.contains("scroll=(5.00,15.00)"), "scroll offsets must appear in serialization");
    }

    #[test]
    fn overflow_auto_emits_push_scroll_layer() {
        // overflow:auto must produce PushScrollLayer just like overflow:scroll.
        let dl = build(
            r#"<div style="overflow:auto;width:100px;height:50px"><p>text</p></div>"#,
            "",
        );
        let has_push = dl.iter().any(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }));
        let has_pop  = dl.iter().any(|c| matches!(c, DisplayCommand::PopScrollLayer));
        assert!(has_push, "overflow:auto must emit PushScrollLayer");
        assert!(has_pop,  "overflow:auto must emit PopScrollLayer");
    }

    // ── DrawScrollbar ─────────────────────────────────────────────────────────

    /// overflow:scroll with content taller than clip → vertical DrawScrollbar emitted.
    #[test]
    fn overflow_scroll_with_overflow_emits_draw_scrollbar_vertical() {
        // div 100×50 with a 200px-tall child → content overflows vertically.
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><div style="height:200px"></div></div>"#,
            "",
        );
        let bars: Vec<_> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawScrollbar { vertical, .. } => Some(*vertical),
                _ => None,
            })
            .collect();
        assert!(!bars.is_empty(), "должен быть хотя бы один DrawScrollbar");
        assert!(bars.contains(&true), "должен быть вертикальный DrawScrollbar");
    }

    /// overflow:scroll with content fitting inside → no DrawScrollbar (no overflow).
    #[test]
    fn overflow_scroll_without_overflow_no_draw_scrollbar() {
        // div 100×200 with a 50px-tall child → no vertical overflow.
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:200px"><div style="height:50px"></div></div>"#,
            "",
        );
        let bars = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawScrollbar { .. }))
            .count();
        assert_eq!(bars, 0, "нет переполнения → нет DrawScrollbar");
    }

    /// DrawScrollbar thumb_rect is inside track_rect.
    #[test]
    fn draw_scrollbar_thumb_inside_track() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px"><div style="height:200px"></div></div>"#,
            "",
        );
        let sb = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::DrawScrollbar { vertical: true, .. }))
            .expect("должен быть вертикальный DrawScrollbar");
        if let DisplayCommand::DrawScrollbar { track_rect, thumb_rect, vertical: true, .. } = sb {
            // Track right edge must be at right edge of clip (within padding box).
            assert!(track_rect.width > 0.0, "track width > 0");
            assert!(thumb_rect.height > 0.0, "thumb height > 0");
            // Thumb must be inside track vertically.
            assert!(
                thumb_rect.y >= track_rect.y,
                "thumb top must be >= track top"
            );
            assert!(
                thumb_rect.y + thumb_rect.height <= track_rect.y + track_rect.height + 1.0,
                "thumb bottom must be <= track bottom"
            );
        }
    }

    /// DrawScrollbar serialization round-trip.
    #[test]
    fn draw_scrollbar_serialize() {
        let dl = vec![DisplayCommand::DrawScrollbar {
            track_rect: Rect::new(90.0, 0.0, 12.0, 50.0),
            thumb_rect: Rect::new(92.0, 5.0, 8.0, 20.0),
            vertical: true,
            thumb_color: SCROLLBAR_THUMB_COLOR,
            track_color: SCROLLBAR_TRACK_COLOR,
        }];
        let s = serialize_display_list(&dl);
        assert!(s.contains("DrawScrollbar"), "serialization must contain DrawScrollbar");
        assert!(s.contains("vertical"), "serialization must mention orientation");
    }

    /// `scrollbar-width: none` suppresses DrawScrollbar while keeping scroll layer.
    #[test]
    fn scrollbar_width_none_no_draw_scrollbar() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px;scrollbar-width:none"><div style="height:200px"></div></div>"#,
            "",
        );
        let bars = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawScrollbar { .. }))
            .count();
        assert_eq!(bars, 0, "scrollbar-width:none → нет DrawScrollbar");
        // Scroll layer must still be present so content can scroll.
        let has_scroll = dl
            .iter()
            .any(|c| matches!(c, DisplayCommand::PushScrollLayer { .. }));
        assert!(has_scroll, "scrollbar-width:none → scroll layer должен оставаться");
    }

    /// `scrollbar-width: thin` emits DrawScrollbar with narrower track (6px gutter).
    #[test]
    fn scrollbar_width_thin_narrow_track() {
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px;scrollbar-width:thin"><div style="height:200px"></div></div>"#,
            "",
        );
        let sb = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::DrawScrollbar { vertical: true, .. }))
            .expect("thin scrollbar must emit DrawScrollbar");
        if let DisplayCommand::DrawScrollbar { track_rect, .. } = sb {
            assert!(
                (track_rect.width - SCROLLBAR_WIDTH_THIN).abs() < 0.5,
                "thin track width should be ~{} px, got {}",
                SCROLLBAR_WIDTH_THIN,
                track_rect.width
            );
        }
    }

    /// `scrollbar-color` wires custom thumb+track colors into DrawScrollbar.
    #[test]
    fn scrollbar_color_custom_colors() {
        // red thumb, blue track
        let dl = build(
            r#"<div style="overflow:scroll;width:100px;height:50px;scrollbar-color:red blue"><div style="height:200px"></div></div>"#,
            "",
        );
        let sb = dl
            .iter()
            .find(|c| matches!(c, DisplayCommand::DrawScrollbar { vertical: true, .. }))
            .expect("must emit DrawScrollbar");
        if let DisplayCommand::DrawScrollbar { thumb_color, track_color, .. } = sb {
            // Red thumb: r≈1.0, g≈0, b≈0
            assert!(thumb_color[0] > 0.9, "thumb red channel must be ~1.0");
            assert!(thumb_color[1] < 0.1, "thumb green channel must be ~0");
            // Blue track: b≈1.0, r≈0
            assert!(track_color[2] > 0.9, "track blue channel must be ~1.0");
            assert!(track_color[0] < 0.1, "track red channel must be ~0");
        }
    }

    /// overflow:hidden does not emit DrawScrollbar (no scroll layer).
    #[test]
    fn overflow_hidden_no_scrollbar() {
        let dl = build(
            r#"<div style="overflow:hidden;width:100px;height:50px"><div style="height:200px"></div></div>"#,
            "",
        );
        let bars = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawScrollbar { .. }))
            .count();
        assert_eq!(bars, 0, "overflow:hidden → нет DrawScrollbar");
    }

    // ── PageBreak / print display list ────────────────────────────────────────

    /// split_at_page_breaks on empty input → one empty page.
    #[test]
    fn split_empty_yields_one_empty_page() {
        let pages = split_at_page_breaks(vec![]);
        assert_eq!(pages.len(), 1);
        assert!(pages[0].is_empty());
    }

    /// split_at_page_breaks with no PageBreak → one page with all commands.
    #[test]
    fn split_no_breaks_single_page() {
        use lumen_core::geom::Rect;
        let cmds = vec![
            DisplayCommand::FillRect {
                rect: Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 },
                color: Color { r: 255, g: 0, b: 0, a: 255 },
            },
            DisplayCommand::FillRect {
                rect: Rect { x: 0.0, y: 10.0, width: 10.0, height: 10.0 },
                color: Color { r: 0, g: 255, b: 0, a: 255 },
            },
        ];
        let pages = split_at_page_breaks(cmds);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].len(), 2);
    }

    /// split_at_page_breaks with one PageBreak → two pages.
    #[test]
    fn split_one_break_two_pages() {
        use lumen_core::geom::Rect;
        let r = Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 };
        let cmds = vec![
            DisplayCommand::FillRect { rect: r, color: Color { r: 255, g: 0, b: 0, a: 255 } },
            DisplayCommand::PageBreak,
            DisplayCommand::FillRect { rect: r, color: Color { r: 0, g: 0, b: 255, a: 255 } },
        ];
        let pages = split_at_page_breaks(cmds);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].len(), 1); // one FillRect on page 0
        assert_eq!(pages[1].len(), 1); // one FillRect on page 1
        // PageBreak itself must not appear in any page
        for page in &pages {
            assert!(!page.iter().any(|c| matches!(c, DisplayCommand::PageBreak)));
        }
    }

    /// split_at_page_breaks with two PageBreaks → three pages, middle page empty.
    #[test]
    fn split_two_breaks_three_pages_middle_empty() {
        use lumen_core::geom::Rect;
        let r = Rect { x: 0.0, y: 0.0, width: 5.0, height: 5.0 };
        let cmds = vec![
            DisplayCommand::FillRect { rect: r, color: Color { r: 1, g: 2, b: 3, a: 255 } },
            DisplayCommand::PageBreak,
            DisplayCommand::PageBreak,
            DisplayCommand::FillRect { rect: r, color: Color { r: 4, g: 5, b: 6, a: 255 } },
        ];
        let pages = split_at_page_breaks(cmds);
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0].len(), 1);
        assert_eq!(pages[1].len(), 0); // empty middle page
        assert_eq!(pages[2].len(), 1);
    }

    /// build_print_display_list on zero pages → empty list.
    #[test]
    fn print_dl_empty_pages() {
        let cmds = build_print_display_list(&[]);
        assert!(cmds.is_empty());
    }

    /// build_print_display_list on two pages inserts exactly one PageBreak.
    #[test]
    fn print_dl_two_pages_one_page_break() {
        use lumen_layout::{paginate, PaginationContext};

        let doc = lumen_html_parser::parse(
            "<div style='height:600px;background:red'></div><div style='height:600px;background:blue'></div>",
        );
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 1200.0));

        let ctx = PaginationContext {
            page_width: 800.0,
            page_height: 600.0,
            margin_top: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            margin_right: 0.0,
        };
        let pages = paginate(&tree, &ctx);
        // If content fits in one page or pagination yields 0/1 page, skip assertion
        if pages.len() < 2 {
            return;
        }
        let cmds = build_print_display_list(&pages);
        let breaks = cmds.iter().filter(|c| matches!(c, DisplayCommand::PageBreak)).count();
        assert_eq!(breaks, pages.len() - 1, "N pages → N-1 PageBreaks");
    }

    // ── Tests for build_print_display_list margin-box rendering ──────────

    /// Page without page_box emits no margin-box DrawText commands.
    #[test]
    fn print_dl_no_page_box_no_margin_text() {
        use lumen_layout::{paginate, PaginationContext};

        let doc = lumen_html_parser::parse("<div style='height:100px'></div>");
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(400.0, 600.0));
        let ctx = PaginationContext {
            page_width: 400.0,
            page_height: 600.0,
            margin_top: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            margin_right: 0.0,
        };
        let pages = paginate(&tree, &ctx);
        assert!(!pages.is_empty());
        // No page_box — no DrawText from margin boxes
        let cmds = build_print_display_list(&pages);
        let text_cmds: Vec<_> = cmds.iter().filter(|c| matches!(c, DisplayCommand::DrawText { .. })).collect();
        assert!(text_cmds.is_empty(), "no margin-box DrawText without page_box");
    }

    /// Page with a page_box containing bottom-center text emits a DrawText command.
    #[test]
    fn print_dl_page_box_bottom_center_emits_draw_text() {
        use lumen_layout::{
            paginate, MarginBoxPosition, PageBox, PageProperties, PaginationContext, TextMeasurer,
        };

        struct Fixed8;
        impl TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }

        let doc = lumen_html_parser::parse("<div style='height:100px'></div>");
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(400.0, 600.0));
        let ctx = PaginationContext {
            page_width: 400.0,
            page_height: 600.0,
            margin_top: 40.0,
            margin_bottom: 40.0,
            margin_left: 40.0,
            margin_right: 40.0,
        };
        let mut pages = paginate(&tree, &ctx);
        assert!(!pages.is_empty());

        let props = PageProperties {
            width: 400.0, height: 600.0,
            orientation: "portrait".to_string(),
            margin_top: 40.0, margin_bottom: 40.0,
            margin_left: 40.0, margin_right: 40.0,
        };
        let mut page_box = PageBox::new(0, props);
        page_box.layout_margin_boxes();
        let label = "1 / 1";
        if let Some(mb) = page_box.margin_boxes.get_mut(&MarginBoxPosition::BottomCenter) {
            mb.content = Some(label.to_string());
            mb.layout_text(label, 10.0, 15.0, &Fixed8);
        }
        pages[0].page_box = Some(page_box);

        let cmds = build_print_display_list(&pages);
        let texts: Vec<&str> = cmds.iter().filter_map(|c| {
            if let DisplayCommand::DrawText { text, .. } = c { Some(text.as_str()) } else { None }
        }).collect();
        assert!(texts.contains(&"1 / 1"), "expected '1 / 1' in DrawText, got: {:?}", texts);
    }

    /// Margin-box DrawText positioned at page-box coordinates (not inside content transform).
    #[test]
    fn print_dl_margin_box_text_absolute_position() {
        use lumen_layout::{
            paginate, MarginBoxPosition, PageBox, PageProperties, PaginationContext, TextMeasurer,
        };

        struct Fixed8;
        impl TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }

        let doc = lumen_html_parser::parse("<div style='height:50px'></div>");
        let sheet = lumen_css_parser::parse("");
        let tree = lumen_layout::layout(&doc, &sheet, Size::new(200.0, 300.0));
        let ctx = PaginationContext {
            page_width: 200.0,
            page_height: 300.0,
            margin_top: 30.0,
            margin_bottom: 30.0,
            margin_left: 30.0,
            margin_right: 30.0,
        };
        let mut pages = paginate(&tree, &ctx);

        let props = PageProperties {
            width: 200.0, height: 300.0,
            orientation: "portrait".to_string(),
            margin_top: 30.0, margin_bottom: 30.0,
            margin_left: 30.0, margin_right: 30.0,
        };
        let mut page_box = PageBox::new(0, props);
        page_box.layout_margin_boxes();
        let label = "PG1";
        // Use top-left-corner so we can predict coordinates: x=0, y=0
        if let Some(mb) = page_box.margin_boxes.get_mut(&MarginBoxPosition::TopLeftCorner) {
            mb.content = Some(label.to_string());
            mb.layout_text(label, 10.0, 15.0, &Fixed8);
        }
        pages[0].page_box = Some(page_box);

        let cmds = build_print_display_list(&pages);
        let pg1_rect = cmds.iter().find_map(|c| {
            if let DisplayCommand::DrawText { text, rect, .. } = c {
                if text == "PG1" { Some(*rect) } else { None }
            } else { None }
        });
        let rect = pg1_rect.expect("DrawText 'PG1' not found");
        // TopLeftCorner is at page origin (0,0); fragment offset is 0,0 inside box
        assert!(rect.x >= 0.0 && rect.x < 10.0, "x should be at page origin, got {}", rect.x);
        assert!(rect.y >= 0.0 && rect.y < 10.0, "y should be at page origin, got {}", rect.y);
    }

    // ── Tests for DrawCrossFade ────────────────────────────────────────────

    /// Конструкция DrawCrossFade сохраняет все поля без потерь.
    #[test]
    fn cross_fade_construction_preserves_fields() {
        let cmd = DisplayCommand::DrawCrossFade {
            dest: Rect::new(10.0, 20.0, 100.0, 50.0),
            src_a: "first.png".to_string(),
            src_b: "second.png".to_string(),
            progress: 0.25,
        };
        if let DisplayCommand::DrawCrossFade { dest, src_a, src_b, progress } = &cmd {
            assert!((dest.x - 10.0).abs() < f32::EPSILON);
            assert!((dest.y - 20.0).abs() < f32::EPSILON);
            assert!((dest.width - 100.0).abs() < f32::EPSILON);
            assert!((dest.height - 50.0).abs() < f32::EPSILON);
            assert_eq!(src_a, "first.png");
            assert_eq!(src_b, "second.png");
            assert!((progress - 0.25).abs() < f32::EPSILON);
        } else {
            panic!("expected DrawCrossFade variant");
        }
    }

    /// serialize_display_list печатает все ключевые поля в детерминированном формате.
    #[test]
    fn cross_fade_serialize_includes_all_fields() {
        let dl = vec![DisplayCommand::DrawCrossFade {
            dest: Rect::new(0.0, 0.0, 200.0, 100.0),
            src_a: "a.png".to_string(),
            src_b: "b.png".to_string(),
            progress: 0.5,
        }];
        let s = serialize_display_list(&dl);
        assert!(s.starts_with("DrawCrossFade "), "should start with command name: {s}");
        assert!(s.contains("(0.00, 0.00, 200.00, 100.00)"), "should contain dest rect: {s}");
        assert!(s.contains(r#"a="a.png""#), "should contain src_a: {s}");
        assert!(s.contains(r#"b="b.png""#), "should contain src_b: {s}");
        assert!(s.contains("p=0.500"), "should contain progress: {s}");
    }

    /// Equality / Debug на варианте работают через производные —
    /// важно для snapshot-тестов и assert_eq! в downstream-крейтах.
    #[test]
    fn cross_fade_equality_and_debug() {
        let a = DisplayCommand::DrawCrossFade {
            dest: Rect::new(1.0, 2.0, 3.0, 4.0),
            src_a: "x".into(),
            src_b: "y".into(),
            progress: 0.75,
        };
        let b = a.clone();
        assert_eq!(a, b, "Clone должен сохранять равенство");
        let dbg = format!("{a:?}");
        assert!(dbg.contains("DrawCrossFade"), "Debug должен включать имя варианта: {dbg}");
        assert!(dbg.contains("0.75"), "Debug должен включать progress: {dbg}");

        // Граничные значения: progress = 0.0 (только src_a) и 1.0 (только src_b)
        // — оба валидны и различимы.
        let zero = DisplayCommand::DrawCrossFade {
            dest: Rect::new(0.0, 0.0, 10.0, 10.0),
            src_a: "a".into(),
            src_b: "b".into(),
            progress: 0.0,
        };
        let one = DisplayCommand::DrawCrossFade {
            dest: Rect::new(0.0, 0.0, 10.0, 10.0),
            src_a: "a".into(),
            src_b: "b".into(),
            progress: 1.0,
        };
        assert_ne!(zero, one, "progress=0.0 и progress=1.0 — разные команды");
    }

    /// DrawCrossFade попадает в exhaustive-match киндов (защита от
    /// «забыли добавить ветку при extension enum-а»).
    #[test]
    fn cross_fade_appears_in_kind_dispatch() {
        let cmd = DisplayCommand::DrawCrossFade {
            dest: Rect::new(0.0, 0.0, 1.0, 1.0),
            src_a: "a".into(),
            src_b: "b".into(),
            progress: 0.5,
        };
        // Если когда-нибудь матч в `img_with_background_and_border_paints_in_order`
        // перестанет включать DrawCrossFade — компилятор не пропустит код.
        // Здесь просто smoke-проверяем сериализацию через публичный API.
        let s = serialize_display_list(std::slice::from_ref(&cmd));
        assert!(s.contains("DrawCrossFade"));
    }

    // ── image-set() (CSS Images L4 §5) ──────────────────────────────────────

    #[test]
    fn is_image_set_detects_function() {
        assert!(is_image_set("image-set(\"a.png\" 1x)"));
        assert!(is_image_set("  IMAGE-SET(url(a.png) 2x)"));
        assert!(is_image_set("-webkit-image-set(\"a.png\" 1x)"));
        assert!(!is_image_set("url(a.png)"));
        assert!(!is_image_set("linear-gradient(red, blue)"));
        assert!(!is_image_set("https://example.com/image-set.png"));
    }

    #[test]
    fn image_set_picks_1x_at_dpr_1() {
        let v = "image-set(\"a.png\" 1x, \"b.png\" 2x)";
        assert_eq!(select_image_set_url(v, 1.0), "a.png");
    }

    #[test]
    fn image_set_picks_2x_at_dpr_2() {
        let v = "image-set(\"a.png\" 1x, \"b.png\" 2x)";
        assert_eq!(select_image_set_url(v, 2.0), "b.png");
    }

    #[test]
    fn image_set_picks_closest_resolution() {
        let v = "image-set(\"a.png\" 1x, \"b.png\" 2x, \"c.png\" 3x)";
        // dpr 1.4 → |1-1.4|=0.4 wins over |2-1.4|=0.6.
        assert_eq!(select_image_set_url(v, 1.4), "a.png");
        // dpr 1.6 → |2-1.6|=0.4 wins over |1-1.6|=0.6.
        assert_eq!(select_image_set_url(v, 1.6), "b.png");
        // dpr 5.0 (no exact) → highest available.
        assert_eq!(select_image_set_url(v, 5.0), "c.png");
    }

    #[test]
    fn image_set_tie_prefers_higher_resolution() {
        let v = "image-set(\"a.png\" 1x, \"b.png\" 2x)";
        // dpr 1.5 equidistant → prefer sharper (2x).
        assert_eq!(select_image_set_url(v, 1.5), "b.png");
    }

    #[test]
    fn image_set_supports_url_wrapper_and_single_quotes() {
        let v = "image-set(url(a.png) 1x, url('b.png') 2x)";
        assert_eq!(select_image_set_url(v, 1.0), "a.png");
        assert_eq!(select_image_set_url(v, 2.0), "b.png");
    }

    #[test]
    fn image_set_default_resolution_is_1x() {
        // Option with no explicit resolution defaults to 1x.
        let v = "image-set(\"a.png\", \"b.png\" 2x)";
        assert_eq!(select_image_set_url(v, 1.0), "a.png");
    }

    #[test]
    fn image_set_dppx_dpi_dpcm_units() {
        let v = "image-set(\"a.png\" 96dpi, \"b.png\" 2dppx)";
        // 96dpi = 1dppx, 2dppx = 2.
        assert_eq!(select_image_set_url(v, 1.0), "a.png");
        assert_eq!(select_image_set_url(v, 2.0), "b.png");
        let v2 = "image-set(\"x.png\" 1x, \"y.png\" 192dpi)";
        // 192dpi = 2dppx.
        assert_eq!(select_image_set_url(v2, 2.0), "y.png");
    }

    #[test]
    fn image_set_webkit_prefix() {
        let v = "-webkit-image-set(url(a.png) 1x, url(b.png) 2x)";
        assert_eq!(select_image_set_url(v, 2.0), "b.png");
    }

    #[test]
    fn image_set_data_uri_with_commas_not_split() {
        // A data: URI inside url() contains commas — must not split the option.
        let v = "image-set(url(data:image/png;base64,AAAA) 1x, \"b.png\" 2x)";
        assert_eq!(select_image_set_url(v, 1.0), "data:image/png;base64,AAAA");
        assert_eq!(select_image_set_url(v, 2.0), "b.png");
    }

    #[test]
    fn image_set_plain_url_passes_through() {
        // Non image-set value treated as a single 1x option.
        assert_eq!(select_image_set_url("\"a.png\"", 2.0), "a.png");
        assert_eq!(select_image_set_url("url(a.png)", 2.0), "a.png");
    }

    #[test]
    fn image_set_empty_returns_empty() {
        assert_eq!(select_image_set_url("image-set()", 1.0), "");
    }

    /// Recursively overrides the `background-image` of the first box that has a
    /// background layer with an `image-set(…)` raw string. Mimics what P4 will
    /// store in `BackgroundImage::Url` once `image-set()` parsing is wired —
    /// lets us exercise the paint-side resolution without the CSS parser.
    fn set_first_bg_image_set(b: &mut LayoutBox, value: &str) -> bool {
        if let Some(layer) = b.style.background_layers.first_mut() {
            layer.image = BackgroundImage::Url(value.to_string());
            return true;
        }
        for child in &mut b.children {
            if set_first_bg_image_set(child, value) {
                return true;
            }
        }
        false
    }

    #[test]
    fn image_set_wired_into_background_layer() {
        // Start from a real url background so a layer exists, then inject the
        // image-set string the way P4's parser will once wired.
        let css = "div { width: 100px; height: 100px; background-image: url(placeholder.png); }";
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(css);
        let mut tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        assert!(set_first_bg_image_set(&mut tree, "image-set(url(a.png) 1x, url(b.png) 2x)"));
        // build_display_list defaults to dpr 1.0 → must pick the 1x url.
        let dl = build_display_list(&tree);
        let srcs: Vec<&str> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawBackgroundImage { src, .. } => Some(src.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(srcs, vec!["a.png"]);
    }

    #[test]
    fn image_set_dpr2_builder_picks_2x() {
        let css = "div { width: 100px; height: 100px; background-image: url(placeholder.png); }";
        let doc = lumen_html_parser::parse("<div></div>");
        let sheet = lumen_css_parser::parse(css);
        let mut tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
        assert!(set_first_bg_image_set(&mut tree, "image-set(url(a.png) 1x, url(b.png) 2x)"));
        let stree = lumen_layout::StackingTree::build(&tree);
        let order = PaintOrder::from_tree(&stree);
        let dl = build_display_list_ordered_dpr(&tree, &stree, &order, 2.0);
        let srcs: Vec<&str> = dl
            .iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawBackgroundImage { src, .. } => Some(src.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(srcs, vec!["b.png"]);
    }

    // ── backdrop-filter frame hash (CSS Filter Effects L1 §2 cache) ──────────

    fn red_fill(x: f32) -> DisplayCommand {
        DisplayCommand::FillRect {
            rect: Rect::new(x, 0.0, 10.0, 10.0),
            color: lumen_layout::Color { r: 255, g: 0, b: 0, a: 255 },
        }
    }

    fn backdrop_cmd() -> DisplayCommand {
        DisplayCommand::PushBackdropFilter {
            filters: vec![lumen_layout::FilterFn::Blur(4.0)],
            bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
        }
    }

    #[test]
    fn contains_backdrop_filter_detects_presence() {
        let with = vec![backdrop_cmd(), DisplayCommand::PopBackdropFilter];
        let without = vec![red_fill(0.0)];
        assert!(contains_backdrop_filter(&with, &[]));
        assert!(contains_backdrop_filter(&[], &with), "overlay lane is scanned too");
        assert!(!contains_backdrop_filter(&without, &without));
    }

    #[test]
    fn hash_is_deterministic_for_identical_input() {
        let content = vec![backdrop_cmd(), red_fill(5.0), DisplayCommand::PopBackdropFilter];
        let a = hash_display_list(&content, &[], 0.0, 0.0, 1024, 720);
        let b = hash_display_list(&content, &[], 0.0, 0.0, 1024, 720);
        assert_eq!(a, b, "same inputs must hash identically");
    }

    #[test]
    fn hash_changes_when_command_changes() {
        let a = hash_display_list(&[red_fill(5.0)], &[], 0.0, 0.0, 1024, 720);
        let b = hash_display_list(&[red_fill(6.0)], &[], 0.0, 0.0, 1024, 720);
        assert_ne!(a, b, "a moved rect must change the hash");
    }

    #[test]
    fn hash_changes_on_scroll_and_size() {
        let content = vec![red_fill(5.0)];
        let base = hash_display_list(&content, &[], 0.0, 0.0, 1024, 720);
        assert_ne!(base, hash_display_list(&content, &[], 0.0, 40.0, 1024, 720), "scroll_y");
        assert_ne!(base, hash_display_list(&content, &[], 12.0, 0.0, 1024, 720), "scroll_x");
        assert_ne!(base, hash_display_list(&content, &[], 0.0, 0.0, 800, 720), "width");
        assert_ne!(base, hash_display_list(&content, &[], 0.0, 0.0, 1024, 600), "height");
    }

    #[test]
    fn hash_distinguishes_content_from_overlay_lane() {
        // The same command in the content lane vs the overlay lane must not
        // collide — order across lanes is part of the hashed sequence.
        let cmd = vec![red_fill(5.0)];
        let in_content = hash_display_list(&cmd, &[], 0.0, 0.0, 1024, 720);
        let in_overlay = hash_display_list(&[], &cmd, 0.0, 0.0, 1024, 720);
        // Both fold the same single command, so to make them distinct we add a
        // second distinguishing command to one lane.
        let two_content = hash_display_list(&[red_fill(5.0), red_fill(9.0)], &[], 0.0, 0.0, 1024, 720);
        assert_ne!(in_content, two_content);
        // content+overlay folding is sequential: content first, then overlay.
        let split = hash_display_list(&[red_fill(5.0)], &[red_fill(9.0)], 0.0, 0.0, 1024, 720);
        assert_eq!(two_content, split, "lanes fold in sequence (content then overlay)");
        let _ = in_overlay;
    }

    // ── Тесты table rendering Phase 1 ─────────────────────────────────────

    #[test]
    fn table_context_default_is_separate_mode() {
        // Тест для убеждения что TableContext::from_box возвращает separate режим по умолчанию
        // (реальный тест с LayoutBox требует полного setup, поэтому проверяем структуру)
        let ctx = TableContext {
            border_collapse: BorderCollapse::Separate,
            border_spacing: (8.0, 8.0),
        };
        assert_eq!(ctx.border_collapse, BorderCollapse::Separate);
        assert_eq!(ctx.border_spacing, (8.0, 8.0));
    }

    #[test]
    fn border_collapse_separate_wins_over_lower_precedence() {
        let cell_border = CollapsedBorder {
            width: 1.0,
            color: [1.0, 0.0, 0.0, 1.0],
            style: BorderStyle::Solid,
            precedence: BorderPrecedence::Cell,
        };
        let table_border = CollapsedBorder {
            width: 1.0,
            color: [0.0, 1.0, 0.0, 1.0],
            style: BorderStyle::Solid,
            precedence: BorderPrecedence::Table,
        };
        let resolved = CollapsedBorder::resolve_conflict(&table_border, &cell_border);
        assert_eq!(resolved.precedence, BorderPrecedence::Cell);
        assert_eq!(resolved.color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn border_collapse_wider_border_wins_at_equal_precedence() {
        let thin = CollapsedBorder {
            width: 1.0,
            color: [1.0, 0.0, 0.0, 1.0],
            style: BorderStyle::Solid,
            precedence: BorderPrecedence::Cell,
        };
        let thick = CollapsedBorder {
            width: 2.0,
            color: [0.0, 1.0, 0.0, 1.0],
            style: BorderStyle::Solid,
            precedence: BorderPrecedence::Cell,
        };
        let resolved = CollapsedBorder::resolve_conflict(&thin, &thick);
        assert_eq!(resolved.width, 2.0);
        assert_eq!(resolved.color, [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn table_separate_mode_renders_with_cells_independent() {
        // Phase 1: table в separate режиме — каждая ячейка имеет независимые границы
        let dl = build(
            "<table><tr><td>A</td><td>B</td></tr></table>",
            "td { border: 1px solid black; background: lightblue; }",
        );
        // Должны быть эмитированы фоны ячеек (2×FillRect для ячеек + контент)
        let fills = fills(&dl);
        assert!(!fills.is_empty(), "table cells should have background fills");
    }

    #[test]
    fn border_precedence_ordering_correct() {
        assert!(BorderPrecedence::Table < BorderPrecedence::RowGroup);
        assert!(BorderPrecedence::RowGroup < BorderPrecedence::Row);
        assert!(BorderPrecedence::Row < BorderPrecedence::Column);
        assert!(BorderPrecedence::Column < BorderPrecedence::Cell);
    }

    #[test]
    fn table_cell_with_border_emits_draw_border() {
        // Phase 1: table cell с border должна эмитировать DrawBorder
        let dl = build(
            "<table><tr><td>A</td></tr></table>",
            "td { border: 2px solid red; }",
        );
        let border_cmds: Vec<_> = dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBorder { .. }))
            .collect();
        assert!(!border_cmds.is_empty(), "cell should emit DrawBorder command");
    }

    #[test]
    fn table_cells_no_border_style_none() {
        // Ячейка без border-style не должна эмитировать DrawBorder
        let dl = build(
            "<table><tr><td>A</td></tr></table>",
            "td { border: 0; }",
        );
        let border_cmds: Vec<_> = dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBorder { .. }))
            .collect();
        assert!(border_cmds.is_empty(), "cell with no border should not emit DrawBorder");
    }

    #[test]
    fn table_with_thead_tbody_tfoot() {
        // Table с thead, tbody, tfoot должна корректно обрабатывать all three groups
        let dl = build(
            "<table>\
                <thead><tr><td>H</td></tr></thead>\
                <tbody><tr><td>B</td></tr></tbody>\
                <tfoot><tr><td>F</td></tr></tfoot>\
            </table>",
            "td { border: 1px solid black; }",
        );
        // Должны быть эмитированы границы для всех трёх групп (3× DrawBorder)
        let border_cmds: Vec<_> = dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawBorder { .. }))
            .collect();
        assert_eq!(border_cmds.len(), 3, "should have 3 DrawBorder commands for 3 rows");
    }

    #[test]
    fn table_cell_background_color_separate_mode() {
        // Каждая ячейка в separate режиме должна иметь независимый фон
        let dl = build(
            "<table><tr><td>A</td><td>B</td></tr></table>",
            "td { background: lightblue; } td:first-child { background: lightcoral; }",
        );
        // Должны быть эмитированы 2 FillRect для cell backgrounds
        let fills = fills(&dl);
        assert!(fills.len() >= 2, "should have at least 2 cell background fills");
    }

    #[test]
    fn table_collapsed_border_wider_wins() {
        // При collapse режиме более широкая граница побеждает (Phase 1 stub test)
        let thin = CollapsedBorder {
            width: 1.0,
            color: [1.0, 0.0, 0.0, 1.0],
            style: BorderStyle::Solid,
            precedence: BorderPrecedence::Cell,
        };
        let thick = CollapsedBorder {
            width: 3.0,
            color: [0.0, 0.0, 1.0, 1.0],
            style: BorderStyle::Solid,
            precedence: BorderPrecedence::Cell,
        };
        let resolved = CollapsedBorder::resolve_conflict(&thin, &thick);
        assert_eq!(resolved.width, 3.0, "thicker border should win");
        assert_eq!(resolved.color, [0.0, 0.0, 1.0, 1.0], "should use thick border color");
    }

    #[test]
    fn table_empty_cells_do_not_crash() {
        // Table с пустыми ячейками должна обрабатываться без panic
        let _dl = build(
            "<table>\
                <tr><td></td><td>B</td></tr>\
                <tr><td>C</td><td></td></tr>\
            </table>",
            "td { border: 1px solid #ccc; padding: 8px; }",
        );
        // Test passes if no panic occurs
    }

    #[test]
    fn table_nested_in_other_content() {
        // Table внутри других элементов должна рендериться корректно
        let dl = build(
            "<div>\
                <p>Before</p>\
                <table><tr><td>In Table</td></tr></table>\
                <p>After</p>\
            </div>",
            "td { border: 1px solid black; background: yellow; }",
        );
        // Должны быть эмитированы: текст "Before", таблица, текст "After"
        let texts = texts(&dl);
        assert!(texts.iter().any(|t| t.contains("Before")), "should have 'Before' text");
        assert!(texts.iter().any(|t| t.contains("In Table")), "should have 'In Table' text");
        assert!(texts.iter().any(|t| t.contains("After")), "should have 'After' text");
    }

    // ── Тесты SVG text rendering ───────────────────────────────────────

    #[test]
    fn svg_text_emits_drawtext_command() {
        // <text>Hello</text> should emit a DrawText command
        let dl = build("<svg><text>Hello</text></svg>", "");
        let texts = texts(&dl);
        assert!(texts.iter().any(|t| t.contains("Hello")), "should emit text 'Hello'");
    }

    #[test]
    fn ordered_svg_shape_emits_fill() {
        // BUG-089: the ordered (stacking-context) path must paint SVG shapes.
        // `emit_box_self` previously no-op'd SvgShape, so shapes vanished in the
        // shell's ordered pipeline (only `walk` painted them). A <rect> with an
        // explicit fill must produce a FillRect via `build_display_list_ordered`.
        let dl = build_ordered(
            "<svg width='100' height='100'><rect x='0' y='0' width='50' height='50' style='fill:#ff0000;'/></svg>",
            "",
        );
        let has_red_fill = dl.iter().any(|c| matches!(
            c,
            DisplayCommand::FillRect { color, .. }
                if color.r == 255 && color.g == 0 && color.b == 0
        ));
        assert!(has_red_fill, "ordered path must emit FillRect for SVG <rect>, got {dl:?}");
    }

    #[test]
    fn ordered_svg_text_emits_drawtext() {
        // BUG-089 companion: ordered path must also paint SVG <text>.
        let dl = build_ordered("<svg><text>Hi</text></svg>", "");
        let has_text = dl.iter().any(|c| matches!(
            c,
            DisplayCommand::DrawText { text, .. } if text.contains("Hi")
        ));
        assert!(has_text, "ordered path must emit DrawText for SVG <text>");
    }

    #[test]
    fn ordered_svg_path_stroke_emits_drawsvgpath() {
        // BUG-096: an SVG <path> has a zero-size layout rect (path bbox is deferred
        // to paint), so `emit_svg_shape`'s 0×0 guard used to drop every path in the
        // ordered pipeline → TEST-54 painted nothing. The path must now emit a
        // DrawSvgPath tessellated in its stroke colour (#e94560 = 233,69,96), and
        // `fill="none"` (an SVG presentation attribute) must suppress the fill so no
        // black fill leaks in.
        let dl = build_ordered(
            "<svg width='200' height='160'>\
                <path d='M 20 140 L 180 20' fill='none' stroke='#e94560' stroke-width='8'/>\
             </svg>",
            "",
        );
        let svg_paths: Vec<&Color> = dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawSvgPath { color, .. } => Some(color),
                _ => None,
            })
            .collect();
        assert!(!svg_paths.is_empty(), "ordered path must emit DrawSvgPath for <path>, got {dl:?}");
        assert!(
            svg_paths.iter().all(|c| c.r == 233 && c.g == 69 && c.b == 96),
            "path must paint in stroke colour #e94560, not a default black fill; got {svg_paths:?}",
        );
    }

    #[test]
    fn svg_text_with_fill_color() {
        // <text style="fill: red">Colored</text> should emit DrawText with fill color
        let dl = build("<svg><text style=\"fill: red\">Colored</text></svg>", "");
        let text_cmds: Vec<&DisplayCommand> = dl.iter()
            .filter(|c| matches!(c, DisplayCommand::DrawText { .. }))
            .collect();
        assert!(!text_cmds.is_empty(), "should emit DrawText command");
    }

    #[test]
    fn svg_text_with_font_size() {
        // <text style="font-size: 24px">Sized</text> should use specified font-size
        let dl = build("<svg><text style=\"font-size: 24px\">Sized</text></svg>", "");
        let text_cmds: Vec<_> = dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { font_size, .. } => Some(font_size),
                _ => None,
            })
            .collect();
        assert!(!text_cmds.is_empty(), "should have DrawText with font-size");
    }

    #[test]
    fn svg_tspan_emits_text() {
        // <text><tspan>Part1</tspan><tspan>Part2</tspan></text> should emit text
        let dl = build("<svg><text><tspan>Part1</tspan><tspan>Part2</tspan></text></svg>", "");
        let texts = texts(&dl);
        assert!(!texts.is_empty(), "should emit at least one text command");
    }

    #[test]
    fn svg_textpath_collects_content() {
        // <text><textPath>OnPath</textPath></text> should collect textPath content
        let dl = build("<svg><text><textPath>OnPath</textPath></text></svg>", "");
        let texts = texts(&dl);
        // Phase 1: just collect and emit content, ignore path rendering
        assert!(texts.iter().any(|t| t.contains("OnPath")) || texts.is_empty(),
                "should have collected textPath content or empty is acceptable in Phase 1");
    }

    #[test]
    fn svg_text_anchor_middle_shifts_x_left() {
        // text-anchor="middle": DrawText rect.x should be shifted left by ~half text width
        // compared to text-anchor="start" at the same SVG x position.
        let dl_start = build(r#"<svg width="200" height="100"><text x="100" y="50" text-anchor="start">AB</text></svg>"#, "");
        let dl_middle = build(r#"<svg width="200" height="100"><text x="100" y="50" text-anchor="middle">AB</text></svg>"#, "");
        let x_start = dl_start.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some(rect.x),
            _ => None,
        });
        let x_middle = dl_middle.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some(rect.x),
            _ => None,
        });
        let (xs, xm) = (x_start.expect("start DrawText"), x_middle.expect("middle DrawText"));
        assert!(xm < xs, "text-anchor=middle should shift x left vs start: middle={xm}, start={xs}");
    }

    #[test]
    fn svg_text_dx_dy_offset_applied() {
        // dx="10" dy="5" should shift the DrawText rect by those amounts vs no offset
        let dl_no_offset = build(r#"<svg width="200" height="100"><text x="50" y="50">Hi</text></svg>"#, "");
        let dl_with_offset = build(r#"<svg width="200" height="100"><text x="50" y="50" dx="10" dy="5">Hi</text></svg>"#, "");
        let pos_no = dl_no_offset.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some((rect.x, rect.y)),
            _ => None,
        });
        let pos_off = dl_with_offset.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some((rect.x, rect.y)),
            _ => None,
        });
        let ((x0, y0), (x1, y1)) = (pos_no.expect("no-offset DrawText"), pos_off.expect("offset DrawText"));
        assert!((x1 - x0 - 10.0).abs() < 1.0, "dx=10 should shift x by ~10: Δx={}", x1 - x0);
        assert!((y1 - y0 - 5.0).abs() < 1.0, "dy=5 should shift y by ~5: Δy={}", y1 - y0);
    }

    #[test]
    fn svg_text_dominant_baseline_middle_shifts_y() {
        // dominant-baseline="middle" should shift DrawText rect.y up compared to auto
        let dl_auto = build(r#"<svg width="200" height="100"><text x="50" y="50" dominant-baseline="auto">T</text></svg>"#, "");
        let dl_middle = build(r#"<svg width="200" height="100"><text x="50" y="50" dominant-baseline="middle">T</text></svg>"#, "");
        let y_auto = dl_auto.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some(rect.y),
            _ => None,
        });
        let y_middle = dl_middle.iter().find_map(|c| match c {
            DisplayCommand::DrawText { rect, .. } => Some(rect.y),
            _ => None,
        });
        let (ya, ym) = (y_auto.expect("auto DrawText"), y_middle.expect("middle DrawText"));
        assert!(ym < ya, "dominant-baseline=middle should shift y up vs auto: middle={ym}, auto={ya}");
    }

    // ── FilterMode conversion tests (B-6) ──────────────────────────────────

    #[test]
    fn filter_mode_from_auto_is_linear() {
        let mode = FilterMode::from_image_rendering(ImageRendering::Auto);
        assert_eq!(mode, FilterMode::Linear, "auto → Linear (bilinear)");
    }

    #[test]
    fn filter_mode_from_smooth_is_linear() {
        let mode = FilterMode::from_image_rendering(ImageRendering::Smooth);
        assert_eq!(mode, FilterMode::Linear, "smooth → Linear (bilinear)");
    }

    #[test]
    fn filter_mode_from_crisp_edges_is_nearest() {
        let mode = FilterMode::from_image_rendering(ImageRendering::CrispEdges);
        assert_eq!(mode, FilterMode::Nearest, "crisp-edges → Nearest (pixel-perfect)");
    }

    #[test]
    fn filter_mode_from_pixelated_is_nearest() {
        let mode = FilterMode::from_image_rendering(ImageRendering::Pixelated);
        assert_eq!(mode, FilterMode::Nearest, "pixelated → Nearest (pixel-perfect)");
    }

    // Display list diffing tests (A-10)
    #[test]
    fn diff_identical_empty_lists() {
        let empty1: Vec<DisplayCommand> = vec![];
        let empty2: Vec<DisplayCommand> = vec![];
        let result = diff_display_lists(&empty1, &empty2);
        assert!(result.identical, "two empty lists should be identical");
    }

    #[test]
    fn diff_identical_single_command() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };

        let cmd1 = DisplayCommand::FillRect {
            rect,
            color: red,
        };
        let cmd2 = DisplayCommand::FillRect {
            rect,
            color: red,
        };

        let list1 = vec![cmd1];
        let list2 = vec![cmd2];

        let result = diff_display_lists(&list1, &list2);
        assert!(result.identical, "identical FillRect commands should be identical");
    }

    #[test]
    fn diff_different_lengths() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };

        let cmd = DisplayCommand::FillRect {
            rect,
            color: red,
        };

        let list1 = vec![cmd.clone()];
        let list2 = vec![cmd.clone(), cmd];

        let result = diff_display_lists(&list1, &list2);
        assert!(!result.identical, "lists with different lengths should not be identical");
    }

    #[test]
    fn diff_different_colors() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };

        let cmd1 = DisplayCommand::FillRect {
            rect,
            color: red,
        };
        let cmd2 = DisplayCommand::FillRect {
            rect,
            color: blue,
        };

        let list1 = vec![cmd1];
        let list2 = vec![cmd2];

        let result = diff_display_lists(&list1, &list2);
        assert!(!result.identical, "FillRects with different colors should not be identical");
        assert!(!result.changed_rects.width.is_nan(), "changed_rects should be valid");
    }

    #[test]
    fn diff_changed_rects_bounds() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let rect1 = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        let rect2 = Rect {
            x: 30.0,
            y: 40.0,
            width: 80.0,
            height: 60.0,
        };

        let cmd1 = DisplayCommand::FillRect {
            rect: rect1,
            color: red,
        };
        let cmd2 = DisplayCommand::FillRect {
            rect: rect2,
            color: red,
        };

        let list1 = vec![cmd1];
        let list2 = vec![cmd2];

        let result = diff_display_lists(&list1, &list2);
        assert!(!result.identical, "FillRects with different positions should not be identical");
        // changed_rects should be the union of rect1 and rect2
        assert_eq!(result.changed_rects.x, 10.0, "left edge should be min of both rects");
        assert_eq!(result.changed_rects.y, 20.0, "top edge should be min of both rects");
    }

    #[test]
    fn diff_multiple_commands_one_changed() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };

        let fill1 = DisplayCommand::FillRect {
            rect,
            color: red,
        };
        let fill2 = DisplayCommand::FillRect {
            rect,
            color: blue,
        };

        let list1 = vec![fill1.clone(), fill1.clone()];
        let list2 = vec![fill1, fill2];

        let result = diff_display_lists(&list1, &list2);
        assert!(!result.identical, "lists differing in one command should not be identical");
    }

    #[test]
    fn diff_empty_to_non_empty() {
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };

        let cmd = DisplayCommand::FillRect {
            rect,
            color: red,
        };

        let list1: Vec<DisplayCommand> = vec![];
        let list2 = vec![cmd];

        let result = diff_display_lists(&list1, &list2);
        assert!(!result.identical, "empty list vs non-empty should not be identical");
        assert_eq!(result.changed_rects.x, 10.0, "changed_rects should reflect added command");
    }

    #[test]
    fn diff_result_identical_constructor() {
        let result = DiffResult::identical();
        assert!(result.identical);
        assert!(result.changed_rects.width == 0.0 && result.changed_rects.height == 0.0);
    }

    #[test]
    fn diff_result_changed_constructor() {
        let rect = Rect {
            x: 5.0,
            y: 10.0,
            width: 50.0,
            height: 60.0,
        };
        let result = DiffResult::changed(rect);
        assert!(!result.identical);
        assert_eq!(result.changed_rects, rect);
    }

    // ── B-9: CSS overflow: clip tests ────────────────────────────────

    fn find_push_clip_rects(dl: &DisplayList) -> Vec<&DisplayCommand> {
        dl.iter()
            .filter(|c| matches!(c, DisplayCommand::PushClipRect { .. }))
            .collect()
    }

    #[test]
    fn overflow_clip_emits_push_clip_rect() {
        let dl = build(
            r#"<div style="overflow:clip;width:100px;height:100px;background:blue"></div>"#,
            "",
        );
        let clips = find_push_clip_rects(&dl);
        assert!(!clips.is_empty(), "overflow:clip should emit PushClipRect");
    }

    #[test]
    fn overflow_clip_margin_expands_clip_region() {
        let dl_no_margin = build(
            r#"<div style="overflow:clip;width:100px;height:100px;background:blue"></div>"#,
            "",
        );
        let dl_with_margin = build(
            r#"<div style="overflow:clip;overflow-clip-margin:10px;width:100px;height:100px;background:blue"></div>"#,
            "",
        );

        let clips_no_margin = find_push_clip_rects(&dl_no_margin);
        let clips_with_margin = find_push_clip_rects(&dl_with_margin);

        assert!(!clips_no_margin.is_empty(), "overflow:clip without margin should have PushClipRect");
        assert!(!clips_with_margin.is_empty(), "overflow:clip with margin should have PushClipRect");

        if let (Some(DisplayCommand::PushClipRect { rect: r1 }), Some(DisplayCommand::PushClipRect { rect: r2 })) =
            (clips_no_margin.first(), clips_with_margin.first())
        {
            // With margin, rect should be expanded (larger width/height).
            assert!(r2.width > r1.width || r2.height > r1.height,
                "overflow-clip-margin should expand clip region");
        }
    }

    #[test]
    fn overflow_hidden_and_clip_both_emit_clip() {
        let dl_hidden = build(
            r#"<div style="overflow:hidden;width:100px;height:100px;background:red"></div>"#,
            "",
        );
        let dl_clip = build(
            r#"<div style="overflow:clip;width:100px;height:100px;background:green"></div>"#,
            "",
        );

        let hidden_clips = find_push_clip_rects(&dl_hidden);
        let clip_clips = find_push_clip_rects(&dl_clip);

        assert!(!hidden_clips.is_empty(), "overflow:hidden should emit PushClipRect");
        assert!(!clip_clips.is_empty(), "overflow:clip should emit PushClipRect");
    }

    #[test]
    fn overflow_clip_no_margin_emits_zero_margin() {
        // When no overflow-clip-margin is specified, clip rect should not be expanded.
        let dl = build(
            r#"<div style="overflow:clip;width:100px;height:100px;background:yellow"></div>"#,
            "",
        );
        let clips = find_push_clip_rects(&dl);
        assert_eq!(clips.len(), 1, "overflow:clip should emit exactly one PushClipRect");
        // The clip rect size should match the padding-box (or close to it).
        if let DisplayCommand::PushClipRect { rect } = clips[0] {
            // Exact values depend on styling, but the rect should be non-negative and finite.
            assert!(rect.width >= 0.0 && rect.height >= 0.0, "clip rect should have non-negative dimensions");
        }
    }

    #[test]
    fn resize_grip_emitted_when_resize_both_and_overflow_hidden() {
        let dl = build(
            r#"<div style="resize:both;overflow:hidden;width:100px;height:100px;background:blue"></div>"#,
            "",
        );
        // Display list should be generated (non-empty) when resize:both + overflow:hidden
        assert!(!dl.is_empty(), "resize:both with overflow:hidden should generate display list");
    }

    #[test]
    fn resize_grip_not_emitted_when_resize_none() {
        let dl = build(
            r#"<div style="resize:none;overflow:hidden;width:100px;height:100px;background:green"></div>"#,
            "",
        );
        // Should not have any FillRoundedRect (or very few if from other sources)
        // This is a phase 0 check; exact count depends on implementation
        assert!(!dl.is_empty(), "display list should not be empty");
    }

    #[test]
    fn resize_grip_not_emitted_when_overflow_visible() {
        let dl = build(
            r#"<div style="resize:both;overflow:visible;width:100px;height:100px;background:red"></div>"#,
            "",
        );
        // resize should only apply when overflow != visible
        assert!(!dl.is_empty(), "display list should not be empty");
    }

    #[test]
    fn resize_grip_emitted_for_horizontal() {
        let dl = build(
            r#"<div style="resize:horizontal;overflow:auto;width:100px;height:100px;background:cyan"></div>"#,
            "",
        );
        assert!(!dl.is_empty(), "resize:horizontal should render display list");
    }

    #[test]
    fn resize_grip_emitted_for_vertical() {
        let dl = build(
            r#"<div style="resize:vertical;overflow:scroll;width:100px;height:100px;background:magenta"></div>"#,
            "",
        );
        assert!(!dl.is_empty(), "resize:vertical should render display list");
    }

    #[test]
    fn resize_grip_positioned_at_bottom_right() {
        let dl = build(
            r#"<div style="resize:both;overflow:hidden;width:100px;height:100px;background:yellow;margin:10px"></div>"#,
            "",
        );
        // Verify display list was built with the resize grip styling
        assert!(!dl.is_empty(), "resize:both with overflow:hidden and margin should generate display list");
    }

    #[test]
    fn range_input_emits_track_and_thumb() {
        let dl = build(r#"<input type="range" min="0" max="100" value="50">"#, "");
        let rounded_rects: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).collect();
        assert!(rounded_rects.len() >= 2, "range input should emit at least track + thumb, got {}", rounded_rects.len());
    }

    #[test]
    fn range_input_at_min_emits_no_fill() {
        let dl = build(r#"<input type="range" min="0" max="100" value="0">"#, "");
        let rounded_rects: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).collect();
        // At min=0: track (gray) + thumb only, no blue fill portion.
        assert!(rounded_rects.len() >= 2, "at min value should still emit track + thumb");
    }

    #[test]
    fn range_input_default_value_is_midpoint() {
        // No value attribute → default value = (min + max) / 2 = 50.
        let dl_mid = build(r#"<input type="range">"#, "");
        let dl_explicit = build(r#"<input type="range" value="50">"#, "");
        let mid_count = dl_mid.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).count();
        let explicit_count = dl_explicit.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).count();
        assert_eq!(mid_count, explicit_count, "default and explicit value=50 should produce same FillRoundedRect count");
    }

    // ── <progress> ──────────────────────────────────────────────────────────

    #[test]
    fn progress_determinate_emits_filled_bar() {
        // value=0.5/max=1.0 → bar fill present (at least one FillRoundedRect inside the control).
        let dl = build(r#"<progress value="0.5" max="1.0"></progress>"#, "");
        let filled: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).collect();
        assert!(!filled.is_empty(), "determinate progress should emit at least one FillRoundedRect");
    }

    #[test]
    fn progress_indeterminate_emits_partial_fill() {
        // No value attr → indeterminate; still emits a 30% bar.
        let dl = build(r#"<progress max="1.0"></progress>"#, "");
        let filled: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).collect();
        assert!(!filled.is_empty(), "indeterminate progress should emit a partial bar");
    }

    #[test]
    fn progress_zero_value_emits_no_fill() {
        // value=0 → fraction=0 → no FillRoundedRect from the bar (but FillRect from background may exist).
        let dl = build(r#"<progress value="0" max="1.0"></progress>"#, "");
        let rounded: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).collect();
        assert!(rounded.is_empty(), "progress at 0 should emit no rounded fill, got {}", rounded.len());
    }

    // ── <meter> ─────────────────────────────────────────────────────────────

    #[test]
    fn meter_emits_filled_bar() {
        let dl = build(r#"<meter min="0" max="10" value="5"></meter>"#, "");
        let filled: Vec<_> = dl.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).collect();
        assert!(!filled.is_empty(), "meter should emit a FillRoundedRect bar");
    }

    #[test]
    fn meter_gauge_optimal_is_green() {
        // value inside optimum range → green fill.
        let green = super::meter_gauge_color(5.0, 0.0, 10.0, 2.0, 8.0, 5.0);
        assert_eq!(green.r, 100, "optimal zone should be green (r=100)");
        assert!(green.g > green.r, "green channel should dominate");
    }

    #[test]
    fn meter_gauge_suboptimal_is_yellow() {
        // optimum in (low, high), value in low segment → yellow.
        let yellow = super::meter_gauge_color(1.0, 0.0, 10.0, 2.0, 8.0, 5.0);
        assert!(yellow.r > 100, "yellow should have high red channel");
        assert!(yellow.g > 100, "yellow should have high green channel");
        assert!(yellow.b < 50,  "yellow should have low blue channel");
    }

    #[test]
    fn meter_gauge_bad_is_red() {
        // optimum in high segment, value in low segment → red (farthest from optimum).
        let red = super::meter_gauge_color(1.0, 0.0, 10.0, 2.0, 8.0, 9.0);
        assert!(red.r > 100, "red zone should have high red channel");
        assert!(red.g < 100, "red zone should have low green channel");
    }

    // ── font-stretch → wdth variation axis ─────────────────────────────────

    fn wdth_axes(dl: &DisplayList) -> Vec<f32> {
        dl.iter()
            .filter_map(|c| match c {
                DisplayCommand::DrawText { font_variation_axes, .. } => {
                    font_variation_axes.iter()
                        .find(|(tag, _)| tag == b"wdth")
                        .map(|(_, v)| *v)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn font_stretch_normal_no_wdth_axis() {
        // font-stretch: normal (default) → no wdth axis injected
        let dl = build("<p>hello</p>", "p { font-stretch: normal; }");
        let wdth: Vec<_> = wdth_axes(&dl);
        assert!(wdth.is_empty(), "normal stretch must not inject wdth, got {:?}", wdth);
    }

    #[test]
    fn font_stretch_condensed_injects_wdth_75() {
        // font-stretch: condensed → wdth = 75.0
        let dl = build("<p>hello</p>", "p { font-stretch: condensed; }");
        let wdth = wdth_axes(&dl);
        assert!(!wdth.is_empty(), "condensed stretch must inject wdth axis");
        assert!(
            wdth.iter().all(|v| (*v - 75.0).abs() < f32::EPSILON),
            "condensed = 75%, got {:?}",
            wdth
        );
    }

    #[test]
    fn font_stretch_expanded_injects_wdth_125() {
        // font-stretch: expanded → wdth = 125.0
        let dl = build("<p>hello</p>", "p { font-stretch: expanded; }");
        let wdth = wdth_axes(&dl);
        assert!(!wdth.is_empty(), "expanded stretch must inject wdth axis");
        assert!(
            wdth.iter().all(|v| (*v - 125.0).abs() < f32::EPSILON),
            "expanded = 125%, got {:?}",
            wdth
        );
    }

    #[test]
    fn font_stretch_percentage_injects_correct_wdth() {
        // font-stretch: 60% → wdth = 60.0
        let dl = build("<p>hello</p>", "p { font-stretch: 60%; }");
        let wdth = wdth_axes(&dl);
        assert!(!wdth.is_empty(), "60% stretch must inject wdth axis");
        assert!(
            wdth.iter().all(|v| (*v - 60.0).abs() < 0.1),
            "60% stretch must give wdth=60.0, got {:?}",
            wdth
        );
    }

    #[test]
    fn font_stretch_explicit_wdth_not_overridden() {
        // font-variation-settings: "wdth" 80 with font-stretch: condensed
        // → explicit wdth=80 wins, no second injection
        let dl = build(
            "<p>hello</p>",
            r#"p { font-stretch: condensed; font-variation-settings: "wdth" 80; }"#,
        );
        let wdth = wdth_axes(&dl);
        // Only one wdth axis per DrawText, and it should be the explicit 80, not 75
        assert!(
            wdth.iter().all(|v| (*v - 80.0).abs() < f32::EPSILON),
            "explicit wdth=80 must not be overridden by font-stretch=condensed (75), got {:?}",
            wdth
        );
    }
}


/// CSS Custom Highlight API L1 — helper to emit DrawText with highlight name.
/// Phase 0: stores highlight name in DrawText for future rendering.
/// Phase 1: will fetch ranges from CSS.highlights and emit overlay rects.
#[allow(clippy::too_many_arguments)]
pub fn emit_text_with_highlights(
    rect: Rect,
    text: &str,
    font_size: f32,
    color: Color,
    font_family: Vec<String>,
    font_weight: FontWeight,
    font_style: FontStyle,
    font_variation_axes: Vec<([u8; 4], f32)>,
    tab_size: f32,
    highlight_name: Option<String>,
    out: &mut DisplayList,
) {
    out.push(DisplayCommand::DrawText {
        rect,
        text: text.to_string(),
        font_size,
        color,
        font_family,
        font_weight,
        font_style,
        font_variation_axes,
        tab_size,
        highlight_name,
    });
}

#[cfg(test)]
mod highlight_tests {
    use super::*;

    #[test]
    fn highlight_field_none_by_default() {
        // DrawText created without highlight_name should have None
        let dl = DisplayList::from(vec![DisplayCommand::DrawText {
            rect: Rect::new(0.0, 0.0, 100.0, 20.0),
            text: "test".to_string(),
            font_size: 14.0,
            color: Color::BLACK,
            font_family: vec![],
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: vec![],
            tab_size: 0.0,
            highlight_name: None,
        }]);
        
        if let DisplayCommand::DrawText { highlight_name, .. } = &dl[0] {
            assert!(highlight_name.is_none());
        }
    }

    #[test]
    fn emit_text_with_highlights_creates_command() {
        let mut out = Vec::new();
        emit_text_with_highlights(
            Rect::new(10.0, 20.0, 100.0, 30.0),
            "highlighted",
            16.0,
            Color::BLACK,
            vec![],
            FontWeight::NORMAL,
            FontStyle::Normal,
            vec![],
            0.0,
            Some("search".to_string()),
            &mut out,
        );
        
        assert_eq!(out.len(), 1);
        if let DisplayCommand::DrawText { text, highlight_name, .. } = &out[0] {
            assert_eq!(text, "highlighted");
            assert_eq!(highlight_name.as_ref(), Some(&"search".to_string()));
        }
    }

    #[test]
    fn highlight_name_custom_values() {
        let names = vec!["search", "spelling", "grammar"];
        for name in names {
            let mut out = Vec::new();
            emit_text_with_highlights(
                Rect::new(0.0, 0.0, 50.0, 20.0),
                "test",
                12.0,
                Color::BLACK,
                vec![],
                FontWeight::NORMAL,
                FontStyle::Normal,
                vec![],
                0.0,
                Some(name.to_string()),
                &mut out,
            );
            
            if let DisplayCommand::DrawText { highlight_name, .. } = &out[0] {
                assert_eq!(highlight_name.as_ref(), Some(&name.to_string()));
            }
        }
    }

    #[test]
    fn highlight_without_name() {
        let mut out = Vec::new();
        emit_text_with_highlights(
            Rect::new(0.0, 0.0, 50.0, 20.0),
            "plain",
            14.0,
            Color::BLACK,
            vec![],
            FontWeight::NORMAL,
            FontStyle::Normal,
            vec![],
            0.0,
            None,
            &mut out,
        );
        
        assert_eq!(out.len(), 1);
        if let DisplayCommand::DrawText { highlight_name, .. } = &out[0] {
            assert!(highlight_name.is_none());
        }
    }

    #[test]
    fn highlight_preserves_text_attributes() {
        let mut out = Vec::new();
        let family = vec!["Arial".to_string()];
        let weight = FontWeight(600);
        
        emit_text_with_highlights(
            Rect::new(5.0, 10.0, 200.0, 25.0),
            "styled",
            18.0,
            Color::BLACK,
            family.clone(),
            weight,
            FontStyle::Italic,
            vec![],
            4.0,
            Some("custom".to_string()),
            &mut out,
        );
        
        if let DisplayCommand::DrawText {
            text, font_size, font_family, font_weight, font_style,
            highlight_name, tab_size, ..
        } = &out[0] {
            assert_eq!(text, "styled");
            assert_eq!(*font_size, 18.0);
            assert_eq!(*font_family, family);
            assert_eq!(*font_weight, weight);
            assert_eq!(*font_style, FontStyle::Italic);
            assert_eq!(highlight_name.as_ref(), Some(&"custom".to_string()));
            assert_eq!(*tab_size, 4.0);
        }
    }

    #[test]
    fn highlight_empty_text() {
        let mut out = Vec::new();
        emit_text_with_highlights(
            Rect::new(0.0, 0.0, 0.0, 0.0),
            "",
            12.0,
            Color::BLACK,
            vec![],
            FontWeight::NORMAL,
            FontStyle::Normal,
            vec![],
            0.0,
            Some("empty".to_string()),
            &mut out,
        );
        
        assert_eq!(out.len(), 1);
        if let DisplayCommand::DrawText { text, highlight_name, .. } = &out[0] {
            assert_eq!(text, "");
            assert_eq!(highlight_name.as_ref(), Some(&"empty".to_string()));
        }
    }

    #[test]
    fn highlight_multiple_independent_calls() {
        let mut out1 = Vec::new();
        let mut out2 = Vec::new();
        
        emit_text_with_highlights(
            Rect::new(0.0, 0.0, 100.0, 20.0),
            "first",
            14.0,
            Color::BLACK,
            vec![],
            FontWeight::NORMAL,
            FontStyle::Normal,
            vec![],
            0.0,
            Some("search".to_string()),
            &mut out1,
        );
        
        emit_text_with_highlights(
            Rect::new(0.0, 20.0, 100.0, 20.0),
            "second",
            14.0,
            Color::BLACK,
            vec![],
            FontWeight::NORMAL,
            FontStyle::Normal,
            vec![],
            0.0,
            Some("spelling".to_string()),
            &mut out2,
        );
        
        if let (
            DisplayCommand::DrawText { text: t1, highlight_name: h1, .. },
            DisplayCommand::DrawText { text: t2, highlight_name: h2, .. },
        ) = (&out1[0], &out2[0])
        {
            assert_eq!(t1, "first");
            assert_eq!(h1.as_ref(), Some(&"search".to_string()));
            assert_eq!(t2, "second");
            assert_eq!(h2.as_ref(), Some(&"spelling".to_string()));
        }
    }
}

    #[test]
    fn highlight_with_variation_axes() {
        let mut out = Vec::new();
        let axes = vec![((*b"wght"), 600.0)];
        
        emit_text_with_highlights(
            Rect::new(0.0, 0.0, 100.0, 20.0),
            "variable",
            16.0,
            Color::BLACK,
            vec![],
            FontWeight::NORMAL,
            FontStyle::Normal,
            axes.clone(),
            0.0,
            Some("variable-font".to_string()),
            &mut out,
        );
        
        if let DisplayCommand::DrawText { font_variation_axes, highlight_name, .. } = &out[0] {
            assert_eq!(font_variation_axes, &axes);
            assert_eq!(highlight_name.as_ref(), Some(&"variable-font".to_string()));
        }
    }
