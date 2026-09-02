//! P1/SPLIT-DL18: `enum DisplayCommand` + `impl` + `type DisplayList`.
//! Вынесено из `display_list.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-18).

use super::*;

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
