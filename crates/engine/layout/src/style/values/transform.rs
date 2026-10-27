//! Форма для `clip-path` (`ShapeValue`/`ClipPath`), 3D-трансформации
//! (`transform-style`/`backface-visibility`/`TransformFn`), фильтры
//! (`FilterFn`), маска (`GradientStop`/`mask-mode`/`mask-composite`/
//! `MaskLayer`).
//!
//! Перенесено батчем SPLIT-ST17 из `crates/engine/layout/src/style.rs`
//! (анкер `enum ShapeValue` до конца `impl Default` для `MaskLayer`) без правок тел.

use std::sync::Arc;

use lumen_core::ColorSpace;
use serde::{Deserialize, Serialize};

use crate::style::values::background::{BackgroundImage, BackgroundOrigin, BackgroundRepeat, BackgroundSize, MaskClip};
use crate::style::values::box_model::FillRule;
use crate::style::values::color::Color;
use crate::style::values::flexgrid::ObjectPosition;
use crate::style::values::length::Length;

/// CSS Masking L1 §3.5 — `<length-percentage>` значение координаты/размера
/// basic-shape для `clip-path`. Проценты резолвятся на этапе paint
/// относительно reference box (border-box элемента): горизонтальные — по
/// width, вертикальные — по height, радиус `circle()` — по
/// `sqrt(w²+h²)/√2` (CSS Shapes L1 §5.1, BUG-140).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeValue {
    /// Абсолютное значение в px (em/rem уже приведены к px при парсинге).
    Px(f32),
    /// Процент соответствующего базиса reference box (0–100).
    Pct(f32),
    /// `closest-side` — только как радиус `circle()`/`ellipse()` (CSS Shapes L1
    /// §3.1): расстояние от центра до ближайшей стороны reference box.
    /// `resolve` отдаёт 0 — значение зависит от центра, считает paint.
    ClosestSide,
    /// `farthest-side` — расстояние от центра до дальней стороны reference box.
    FarthestSide,
    /// `px + pct% · basis` — результат `calc()`-свёртки, нужен `rect()`/`xywh()`:
    /// отступ от дальней стороны `100% − x − w` смешивает px и проценты.
    Lin {
        /// Абсолютная часть, px.
        px: f32,
        /// Процентная часть базиса (0–100).
        pct: f32,
    },
}

impl ShapeValue {
    /// Резолвит значение в px. `basis` — размер reference box по
    /// соответствующей оси (px); для `Px` игнорируется.
    pub fn resolve(self, basis: f32) -> f32 {
        match self {
            Self::Px(v) => v,
            Self::Pct(p) => p / 100.0 * basis,
            Self::Lin { px, pct } => px + pct / 100.0 * basis,
            Self::ClosestSide | Self::FarthestSide => 0.0,
        }
    }
}

/// CSS Masking L1 §3.5 — basic-shapes для `clip-path`. Phase 0
/// поддерживает: `inset(...)`, `circle(...)`, `ellipse(...)`,
/// `polygon(...)`. URL / `path()` / `none` отложены.
/// Координаты — `ShapeValue` (px или %), проценты резолвятся по
/// border-box на этапе paint (BUG-140: `circle(40% at 50% 50%)` раньше
/// молча отбрасывался целиком).
#[derive(Debug, Clone, PartialEq)]
pub enum ClipPath {
    /// `inset(top right bottom left)` — 1..=4 length-percentage значения
    /// (top/bottom — % от height, left/right — % от width).
    Inset(Vec<ShapeValue>),
    /// `inset(<sides> round <radii> [/ <radii>])` — CSS Shapes L1 §3.3. `sides` как у
    /// [`ClipPath::Inset`]; `rx`/`ry` — 1..=4 радиуса углов в порядке
    /// top-left, top-right, bottom-right, bottom-left (раскрываются по правилу
    /// `border-radius`); `ry` пуст, если `/` не было (радиусы круговые).
    InsetRound {
        /// Смещения сторон (top right bottom left, 1..=4 значения).
        sides: Vec<ShapeValue>,
        /// Горизонтальные радиусы углов.
        rx: Vec<ShapeValue>,
        /// Вертикальные радиусы углов; пусто — равны `rx`.
        ry: Vec<ShapeValue>,
    },
    /// `<geometry-box>` (CSS Masking L1 §4.5): reference box формы, отличный от
    /// `border-box`, либо сам бокс без формы (`shape: None` — граница бокса).
    Boxed {
        /// Форма; `None` — `clip-path: <geometry-box>` без `<basic-shape>`.
        shape: Option<Box<ClipPath>>,
        /// Выбранный reference box.
        geometry: GeometryBox,
    },
    /// `circle(radius at cx cy)` — radius и center (опц.).
    Circle {
        /// Радиус: % резолвится по `sqrt(w²+h²)/√2`.
        radius: ShapeValue,
        /// Центр (cx — % от width, cy — % от height); `None` = 50% 50%.
        center: Option<(ShapeValue, ShapeValue)>,
    },
    /// `ellipse(rx ry at cx cy)` — rx — % от width, ry — % от height.
    Ellipse {
        /// Горизонтальный радиус.
        rx: ShapeValue,
        /// Вертикальный радиус.
        ry: ShapeValue,
        /// Центр; `None` = 50% 50%.
        center: Option<(ShapeValue, ShapeValue)>,
    },
    /// `polygon([<fill-rule>,]? x1 y1, x2 y2, ...)` — список вершин (x — % от
    /// width, y — % от height) + правило заливки. `FillRule` (CSS Shapes L1
    /// §3) управляет самопересекающимися полигонами: `EvenOdd` оставляет
    /// «дырки» в местах перекрытия, `NonZero` (default) заливает их.
    Polygon(Vec<(ShapeValue, ShapeValue)>, FillRule),
    /// `path([<fill-rule>,]? "<svg-path>")` — CSS Shapes L1 §4. Хранит
    /// предварительно флэттенный полигон в px-координатах системы пути
    /// (origin = верхний левый угол reference box; проценты в `path()`
    /// недопустимы по спецификации). Кривые разбиты на отрезки на этапе
    /// парсинга через `motion_path::flatten_path_to_polygon`. Второе поле —
    /// `FillRule` (default `NonZero`); `EvenOdd` делает дырки в
    /// самопересекающихся путях (звёзды-пентаграммы и т. п.).
    Path(Vec<(f32, f32)>, FillRule),
    /// `url(#id)` — CSS Masking L1 §3.1: ссылка на `<clipPath>` в том же
    /// документе. Только результат парсинга: `compute_style` (там есть
    /// `Document`) подменяет её на [`ClipPath::Svg`] либо на `None`, если
    /// элемент не найден или это не `<clipPath>` (спека: свойство тогда
    /// считается не заданным). Не разрешённая ссылка ничего не клипует.
    Url(String),
    /// Разрешённая ссылка на `<clipPath>` (SVG 1.1 §14.3, CSS Masking L1
    /// §3.1): содержимое элемента уже сведено к полигонам.
    Svg(Arc<SvgClipPath>),
}

/// `<geometry-box>` (CSS Masking L1 §4.5). Для CSS-боксов `fill-box` = content-box,
/// `stroke-box`/`view-box` = border-box (как в `mask-clip`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryBox {
    /// `border-box` (значение по умолчанию).
    BorderBox,
    /// `padding-box`.
    PaddingBox,
    /// `content-box`.
    ContentBox,
    /// `margin-box`.
    MarginBox,
    /// `fill-box`.
    FillBox,
    /// `stroke-box`.
    StrokeBox,
    /// `view-box`.
    ViewBox,
}

/// Содержимое `<clipPath>`, сведённое к полигонам в его системе координат
/// (`transform` самого `<clipPath>` и его детей уже учтены). Объединение
/// подпутей — область, которую клип оставляет видимой.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgClipPath {
    /// `id` элемента `<clipPath>` — нужен, чтобы `getComputedStyle` вернул `url("#id")` (BUG-1407).
    pub id: String,
    /// `clipPathUnits="objectBoundingBox"`: координаты полигонов — доли
    /// bounding box ссылающегося элемента (0..1), а не user units.
    pub object_bbox_units: bool,
    /// Фигуры содержимого; пустой список клипует всё (нет видимых детей).
    pub subpaths: Vec<SvgClipSubpath>,
    /// `clip-path` на самом `<clipPath>`: пересекается с этим клипом.
    pub nested: Option<Box<ClipPath>>,
    /// Где ссылающийся SVG-элемент стоит в документе; `None` — HTML-бокс,
    /// у которого user space — левый верхний угол border-box.
    pub placement: Option<SvgClipPlacement>,
}

/// Одна фигура `<clipPath>` в виде замкнутого полигона.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgClipSubpath {
    /// Вершины.
    pub pts: Vec<(f32, f32)>,
    /// `clip-rule: evenodd` (иначе nonzero).
    pub even_odd: bool,
}

/// Положение ссылающегося SVG-элемента, выставляется layout-ом SVG
/// (`lay_out_svg_element_position`). Сдвиг матрицы хранится относительно
/// `LayoutBox::rect.x/y`, поэтому `shift_tree` (abs-pos, scroll) его не ломает.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SvgClipPlacement {
    /// user space элемента → документ, `[a, b, c, d, e, f]`; `e`/`f` — смещение
    /// относительно левого верхнего угла `rect` бокса.
    pub matrix: [f32; 6],
    /// bounding box элемента в его user space `(x, y, w, h)` — для
    /// `clipPathUnits="objectBoundingBox"`.
    pub user_bbox: (f32, f32, f32, f32),
}

/// CSS Transforms L1 §11 — функции `transform`. Phase 0 поддерживает
/// translate/translateX/translateY, rotate, scale/scaleX/scaleY,
/// CSS Transforms L2 §6 — `transform-style: flat | preserve-3d`.
/// `Flat` = children are flattened into the parent plane (default).
/// `Preserve3d` = children participate in the parent 3D rendering context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransformStyle {
    #[default]
    Flat,
    Preserve3d,
}

/// CSS Transforms L2 §5.1 — `backface-visibility: visible | hidden`.
/// `Hidden` = element is invisible when its back face is oriented toward
/// the viewer (requires a 3D rendering context to have any effect).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackfaceVisibility {
    /// Back face is visible (initial value).
    #[default]
    Visible,
    /// Back face is hidden.
    Hidden,
}

/// CSS transform functions — translate/scale/rotate/skew/skewX/skewY/matrix
/// and all 3D variants (CSS Transforms L2).
#[derive(Debug, Clone, PartialEq)]
pub enum TransformFn {
    Translate(f32, f32),
    TranslateX(f32),
    TranslateY(f32),
    /// `translateZ(<length>)` — translate along Z axis in px.
    TranslateZ(f32),
    /// `translate3d(<tx>, <ty>, <tz>)` — all three axes in px.
    Translate3d(f32, f32, f32),
    /// `translate*()` с `<length-percentage>` по X/Y (`translateX(50%)`,
    /// `translate(10%, calc(100% - 4px))`, `translate3d(50%, 0, 5px)`) и Z в px.
    /// Процент ссылается на border box самого элемента (CSS Transforms L1 §3),
    /// поэтому разрешается при построении матрицы
    /// (`property_trees::translate_lp_matrix`), а не при разборе.
    /// Чисто px-значения остаются в `Translate`/`TranslateX`/… .
    TranslateLp(Length, Length, f32),
    /// Угол в радианах (нормализован парсером из deg/rad/turn/grad).
    Rotate(f32),
    /// `rotateX(<angle>)` — angle in radians.
    RotateX(f32),
    /// `rotateY(<angle>)` — angle in radians.
    RotateY(f32),
    /// `rotateZ(<angle>)` — alias for 2D rotate, angle in radians.
    RotateZ(f32),
    /// `rotate3d(<x>, <y>, <z>, <angle>)` — arbitrary axis rotation.
    Rotate3d(f32, f32, f32, f32),
    Scale(f32, f32),
    ScaleX(f32),
    ScaleY(f32),
    /// `scaleZ(<s>)` — scale along Z axis.
    ScaleZ(f32),
    /// `scale3d(<sx>, <sy>, <sz>)` — all three axes.
    Scale3d(f32, f32, f32),
    SkewX(f32),
    SkewY(f32),
    Matrix([f32; 6]),
    /// `matrix3d(<16 values>)` — column-major 4×4 matrix.
    Matrix3d([f32; 16]),
    /// `perspective(<length>)` — perspective distance in px (> 0).
    Perspective(f32),
}

/// CSS Filter Effects L1 §3 — функции `filter`: 9 цветовых/блюр-функций,
/// `drop-shadow()` и `url(#id)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FilterFn {
    /// `blur(<length>)` — радиус gaussian blur.
    Blur(f32),
    /// `brightness(<number-percentage>)`. 1.0 = unchanged.
    Brightness(f32),
    /// `contrast(<number-percentage>)`. 1.0 = unchanged.
    Contrast(f32),
    /// `grayscale(<number-percentage>)`. 0.0 = unchanged, 1.0 = full grayscale.
    Grayscale(f32),
    /// `hue-rotate(<angle>)` — угол в радианах.
    HueRotate(f32),
    /// `invert(<number-percentage>)`. 0.0 = unchanged, 1.0 = inverted.
    Invert(f32),
    /// `opacity(<number-percentage>)`. 1.0 = unchanged.
    Opacity(f32),
    /// `saturate(<number-percentage>)`. 1.0 = unchanged.
    Saturate(f32),
    /// `sepia(<number-percentage>)`. 0.0 = unchanged, 1.0 = full sepia.
    Sepia(f32),
    /// `drop-shadow(<color>? <length>{2,3})` — смещённая размытая копия альфа-маски
    /// под элементом (Filter Effects L1 §«drop-shadow()»).
    DropShadow {
        /// Смещение по X в px.
        dx: f32,
        /// Смещение по Y в px.
        dy: f32,
        /// Третья длина — стандартное отклонение размытия (как у `blur()`), px.
        blur: f32,
        /// `None` — `currentcolor`: раскрывается при построении display list от `color` бокса.
        color: Option<Color>,
    },
    /// `url(#id)` — ссылка на SVG `<filter>` (Filter Effects L1 §3). Граф
    /// подставляет каскад (`box_tree/svg_filter.rs`), где виден `Document`.
    Url(FilterUrl),
}

impl FilterFn {
    /// Функция исполняется на CPU-растре окном слоя (`cpu_raster::svg_filter_chain_window`):
    /// `url(#id)` и `drop-shadow()` красят за пределами ink слоя, GPU-пасс блюра/матрицы
    /// цвета их не покрывает. Группа с такой функцией не обрезается по bbox содержимого.
    pub fn is_cpu_window_pass(&self) -> bool {
        matches!(self, FilterFn::Url(_) | FilterFn::DropShadow { .. })
    }

    /// Копия функции для paint: `drop-shadow()` без цвета получает `current` (`color` бокса).
    pub fn with_current_color(&self, current: Color) -> FilterFn {
        match self {
            FilterFn::DropShadow { dx, dy, blur, color: None } => {
                FilterFn::DropShadow { dx: *dx, dy: *dy, blur: *blur, color: Some(current) }
            }
            other => other.clone(),
        }
    }
}

/// `url(#id)` из `filter` и (после каскада) разобранный граф `<filter>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterUrl {
    /// Фрагмент без `#`.
    pub id: String,
    /// `None` — ссылка в никуда или не на `<filter>`: вся цепочка игнорируется.
    #[serde(skip)]
    pub graph: Option<std::sync::Arc<super::svg_filter::SvgFilter>>,
    /// Где user space ссылающегося SVG-элемента стоит в документе (выставляет layout
    /// SVG); `None` — HTML-бокс: user space — левый верхний угол border box, bbox —
    /// border box.
    #[serde(skip)]
    pub placement: Option<SvgClipPlacement>,
}

/// CSS Images L3 §3.4 — единичный `<color-stop>` градиента.
///
/// `position == None` означает auto-распределение: при resolve до used-value
/// auto-stops равномерно разносятся между фиксированными соседями (spec §3.4.3
/// "Color stop processing"). Здесь типизация специфицированного значения —
/// auto хранится как `None`, без раскрытия.
///
/// Только цвет и позиция (length / percentage). Hint-stops (`<color-stop>,
/// <length-percentage>, <color-stop>`) — без позиции цвета, чисто
/// midpoint-маркер — пока не моделируем: они отрабатывают на интерполяции
/// между соседями и не имеют animation-смысла на уровне per-stop pair.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GradientStop {
    pub color: Color,
    /// Source color space of this stop. `Srgb` for legacy `<color>` values;
    /// `DisplayP3` / `Rec2020` when the stop was written as `color(display-p3 …)`
    /// / `color(rec2020 …)`. Carried through the display list so the renderer
    /// can apply the correct output transform (ph3-color-management Step 3).
    pub color_space: ColorSpace,
    pub position: Option<Length>,
}

/// CSS Masking L1 §6.4 — `mask-mode`. Selects which channel of the mask image
/// is used as the per-pixel mask value when compositing the masked element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaskMode {
    /// Use the alpha channel of the mask image directly. Initial behaviour for
    /// `<image>` mask sources (`match-source` resolves here for gradients/URLs).
    #[default]
    Alpha,
    /// Use luminance (`0.2126·R + 0.7152·G + 0.0722·B`, sRGB) multiplied by the
    /// source alpha as the mask value. A dark mask pixel hides the element even
    /// when fully opaque.
    Luminance,
}

/// CSS Masking L1 §4.7 — `mask-composite`. Determines how a mask layer is
/// combined with the mask already assembled from the layers **below** it
/// (Porter-Duff on the mask channel: `add` = source-over, `subtract` =
/// source-out, `intersect` = source-in, `exclude` = xor).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MaskComposite {
    /// `add` (initial) — Porter-Duff source-over on the mask channel.
    #[default]
    Add,
    /// `subtract` — Porter-Duff source-out: the layer below is removed where
    /// this layer paints.
    Subtract,
    /// `intersect` — Porter-Duff source-in: only the overlap survives.
    Intersect,
    /// `exclude` — Porter-Duff xor: the overlap is removed.
    Exclude,
}

impl MaskComposite {
    /// Parses a single `mask-composite` keyword (CSS Masking L1 §4.7).
    /// Case-insensitive; returns `None` on an unrecognised keyword.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "add" => Some(Self::Add),
            "subtract" => Some(Self::Subtract),
            "intersect" => Some(Self::Intersect),
            "exclude" => Some(Self::Exclude),
            _ => None,
        }
    }
}

/// CSS Masking L1 §4.9 — один слой маски.
///
/// `mask-image` задаёт количество слоёв; остальные longhand-ы циклически
/// повторяются по этому количеству («Layering Multiple Mask Layers»). Тот же
/// приём, что и у [`BackgroundLayer`], поэтому типы значений переиспользуются
/// из background (`mask-repeat` / `mask-size` / `mask-position` имеют ту же
/// грамматику, `mask-clip` — надмножество `background-clip`).
///
/// Порядок в [`ComputedStyle::mask_layers`]: первый = верхний слой. Слои
/// собираются в одну маску снизу вверх, каждый — оператором своего
/// [`MaskLayer::composite`].
#[derive(Debug, Clone, PartialEq)]
pub struct MaskLayer {
    /// `mask-image` этого слоя. `BackgroundImage` переиспользуется как тип
    /// (та же структура: None / Url / Gradient).
    pub image: BackgroundImage,
    /// Computed-запись `mask-image` для градиента (BUG-1407), как её отдаёт `getComputedStyle`;
    /// `None` — запись строится из `image`.
    pub image_css: Option<Arc<str>>,
    /// `mask-repeat` этого слоя (§4.3). Initial `repeat`.
    pub repeat: BackgroundRepeat,
    /// `mask-size` этого слоя (§4.2). Initial `auto`.
    pub size: BackgroundSize,
    /// `mask-position` этого слоя (§4.4). Initial `center`.
    pub position: ObjectPosition,
    /// `mask-origin` этого слоя (§4.5). Initial `border-box`.
    pub origin: BackgroundOrigin,
    /// `mask-clip` этого слоя (§4.6). Initial `border-box`.
    pub clip: MaskClip,
    /// `mask-mode` этого слоя (§6.4). Initial `match-source`, который для
    /// поддерживаемых `<image>`-источников резолвится в `alpha`.
    pub mode: MaskMode,
    /// `mask-composite` этого слоя (§4.7) — оператор смешивания с уже
    /// собранными слоями ниже. Initial `add`.
    pub composite: MaskComposite,
}

impl Default for MaskLayer {
    fn default() -> Self {
        Self {
            image: BackgroundImage::None,
            image_css: None,
            repeat: BackgroundRepeat::REPEAT,
            size: BackgroundSize::Auto,
            position: ObjectPosition::background_initial(),
            origin: BackgroundOrigin::BorderBox,
            clip: MaskClip::BorderBox,
            mode: MaskMode::Alpha,
            composite: MaskComposite::Add,
        }
    }
}

