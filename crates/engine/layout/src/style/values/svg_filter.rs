//! Граф SVG-фильтра (Filter Effects L1 §«The `<filter>` element», §«Filter
//! primitives»).
//!
//! `filter: url(#id)` / `filter="url(#id)"` хранится после каскада как
//! [`FilterFn::Url`](super::transform::FilterFn::Url) с id; здесь, где есть
//! `Document`, ссылка разворачивается в [`SvgFilter`] (`box_tree/svg_filter.rs`):
//! paint (без `Document`) получает готовый граф примитивов и выполняет его над
//! растром элемента.
//!
//! Все длины графа приведены к числам: для `*Units="objectBoundingBox"` это доли
//! bounding box элемента, для `userSpaceOnUse` — user units (px). Единицы
//! переводит paint, зная bbox.

use crate::style::Color;

/// Скомпилированный `<filter>`.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgFilter {
    /// `filterUnits="objectBoundingBox"` (значение по умолчанию): `region` —
    /// доли bbox ссылающегося элемента, иначе user units.
    pub filter_units_obb: bool,
    /// `primitiveUnits="objectBoundingBox"`: числа примитивов (`dx`, `stdDeviation`,
    /// подобласть) — доли bbox, иначе user units.
    pub primitive_units_obb: bool,
    /// Область фильтра `(x, y, width, height)`; значения по умолчанию
    /// (`-10% -10% 120% 120%`) уже подставлены.
    pub region: (f32, f32, f32, f32),
    /// `[x, y, width, height]` области заданы процентами в `userSpaceOnUse` и ещё
    /// не раскрыты: в `region` лежит доля (`%` / 100) от размера viewport, который
    /// знает только paint (border box HTML-бокса). У SVG-элементов базу берёт
    /// каскад (ближайший `<svg>`), и флаги сброшены.
    pub region_pct: [bool; 4],
    /// Примитивы в порядке документа.
    pub primitives: Vec<SvgFilterPrimitive>,
}

/// Вход примитива (`in` / `in2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeInput {
    /// Результат предыдущего примитива (или `SourceGraphic` у первого).
    Previous,
    /// `SourceGraphic`.
    SourceGraphic,
    /// `SourceAlpha`.
    SourceAlpha,
    /// `BackgroundImage`/`BackgroundAlpha`/`FillPaint`/`StrokePaint` не
    /// поддержаны (Filter Effects L1 §«Accessing the background image»): прозрачный чёрный.
    Empty,
    /// Результат примитива с этим индексом (`result="name"`, выше по списку).
    Result(usize),
}

/// Один примитив графа.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgFilterPrimitive {
    /// Что считает примитив.
    pub kind: FePrimitive,
    /// Подобласть `x`/`y`/`width`/`height` (`None` — по умолчанию из входов).
    pub x: Option<f32>,
    /// См. [`Self::x`].
    pub y: Option<f32>,
    /// См. [`Self::x`].
    pub width: Option<f32>,
    /// См. [`Self::x`].
    pub height: Option<f32>,
    /// Как [`SvgFilter::region_pct`], для `[x, y, width, height]` подобласти.
    pub pct: [bool; 4],
    /// `color-interpolation-filters: linearRGB` (значение по умолчанию) или `sRGB`.
    pub linear_rgb: bool,
    /// Сам примитив «заражён» (Filter Effects L1 §«Tainted filter primitives»):
    /// цвет зависит от `currentcolor`, который может раскрыть `:visited`.
    /// Заражение переходит на все примитивы, читающие его результат;
    /// `feDisplacementMap` с заражённой картой отдаёт вход без смещения.
    pub tainted: bool,
}

/// Режим `feComposite`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FeCompositeOp {
    /// `over`.
    Over,
    /// `in`.
    In,
    /// `out`.
    Out,
    /// `atop`.
    Atop,
    /// `xor`.
    Xor,
    /// `arithmetic` с `k1..k4`.
    Arithmetic([f32; 4]),
}

/// Режим `feBlend` (Compositing & Blending L1 §«Blend modes»).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeBlendMode {
    /// `normal`.
    Normal,
    /// `multiply`.
    Multiply,
    /// `screen`.
    Screen,
    /// `darken`.
    Darken,
    /// `lighten`.
    Lighten,
    /// `overlay`.
    Overlay,
    /// `color-dodge`.
    ColorDodge,
    /// `color-burn`.
    ColorBurn,
    /// `hard-light`.
    HardLight,
    /// `soft-light`.
    SoftLight,
    /// `difference`.
    Difference,
    /// `exclusion`.
    Exclusion,
    /// `hue`.
    Hue,
    /// `saturation`.
    Saturation,
    /// `color`.
    Color,
    /// `luminosity`.
    Luminosity,
}

/// `edgeMode` у `feConvolveMatrix`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeEdgeMode {
    /// `duplicate` (по умолчанию): край входа повторяется.
    Duplicate,
    /// `wrap`: вход замыкается по кругу.
    Wrap,
    /// `none`: за краем — прозрачный чёрный.
    None,
}

/// Источник света `feDiffuseLighting`/`feSpecularLighting`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FeLight {
    /// `feDistantLight`.
    Distant {
        /// `azimuth` (градусы).
        azimuth: f32,
        /// `elevation` (градусы).
        elevation: f32,
    },
    /// `fePointLight` (координаты в единицах примитива).
    Point {
        /// `x`, `y`, `z`.
        pos: [f32; 3],
    },
    /// `feSpotLight`.
    Spot {
        /// `x`, `y`, `z`.
        pos: [f32; 3],
        /// `pointsAtX/Y/Z`.
        points_at: [f32; 3],
        /// `specularExponent`.
        exponent: f32,
        /// `limitingConeAngle` (градусы).
        cone: Option<f32>,
    },
}

/// Вид освещения.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FeLighting {
    /// `feDiffuseLighting`: `diffuseConstant`.
    Diffuse {
        /// `diffuseConstant`.
        kd: f32,
    },
    /// `feSpecularLighting`.
    Specular {
        /// `specularConstant`.
        ks: f32,
        /// `specularExponent`.
        exponent: f32,
    },
}

/// Вид `feColorMatrix`.
#[derive(Debug, Clone, PartialEq)]
pub enum FeColorMatrix {
    /// `matrix` — 4×5 по строкам.
    Matrix([f32; 20]),
    /// `saturate`.
    Saturate(f32),
    /// `hueRotate` (градусы).
    HueRotate(f32),
    /// `luminanceToAlpha`.
    LuminanceToAlpha,
}

/// Передаточная функция канала `feComponentTransfer` (`feFuncR/G/B/A`).
#[derive(Debug, Clone, PartialEq)]
pub enum FeTransfer {
    /// `identity` (или нет `feFunc*`).
    Identity,
    /// `table`.
    Table(Vec<f32>),
    /// `discrete`.
    Discrete(Vec<f32>),
    /// `linear`: `slope · C + intercept`.
    Linear {
        /// `slope`.
        slope: f32,
        /// `intercept`.
        intercept: f32,
    },
    /// `gamma`: `amplitude · C^exponent + offset`.
    Gamma {
        /// `amplitude`.
        amplitude: f32,
        /// `exponent`.
        exponent: f32,
        /// `offset`.
        offset: f32,
    },
}

/// `preserveAspectRatio` примитива `feImage`: как вписать картинку в подобласть.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeImageFit {
    /// `none`: картинка растягивается на всю подобласть без сохранения пропорций.
    pub stretch: bool,
    /// Выравнивание по x: 0 — `xMin`, 0.5 — `xMid`, 1 — `xMax`.
    pub align_x: f32,
    /// Выравнивание по y: 0 — `YMin`, 0.5 — `YMid`, 1 — `YMax`.
    pub align_y: f32,
    /// `slice` (картинка покрывает подобласть, лишнее обрезается), иначе `meet`.
    pub slice: bool,
}

impl Default for FeImageFit {
    /// `xMidYMid meet`.
    fn default() -> Self {
        Self { stretch: false, align_x: 0.5, align_y: 0.5, slice: false }
    }
}

/// Вид примитива с параметрами.
#[derive(Debug, Clone, PartialEq)]
pub enum FePrimitive {
    /// `feImage` со ссылкой на картинку (растровую или SVG): `href` — ключ, под
    /// которым shell регистрирует декодированное изображение (адрес как в разметке,
    /// без разрешения от базового URL).
    Image {
        /// Адрес картинки.
        href: String,
        /// `preserveAspectRatio`.
        fit: FeImageFit,
    },
    /// `feFlood`: цвет и непрозрачность.
    Flood {
        /// `flood-color`.
        color: Color,
        /// `flood-opacity`.
        opacity: f32,
    },
    /// `feOffset`.
    Offset {
        /// Вход.
        input: FeInput,
        /// `dx`.
        dx: f32,
        /// `dy`.
        dy: f32,
    },
    /// `feGaussianBlur`.
    GaussianBlur {
        /// Вход.
        input: FeInput,
        /// `stdDeviation` по X.
        sx: f32,
        /// `stdDeviation` по Y.
        sy: f32,
    },
    /// `feColorMatrix`.
    ColorMatrix {
        /// Вход.
        input: FeInput,
        /// Преобразование.
        matrix: FeColorMatrix,
    },
    /// `feComposite`.
    Composite {
        /// `in`.
        a: FeInput,
        /// `in2`.
        b: FeInput,
        /// `operator`.
        op: FeCompositeOp,
    },
    /// `feBlend`.
    Blend {
        /// `in` (верхний слой).
        a: FeInput,
        /// `in2` (нижний слой).
        b: FeInput,
        /// `mode`.
        mode: FeBlendMode,
    },
    /// `feMerge`: слои снизу вверх.
    Merge(Vec<FeInput>),
    /// `feComponentTransfer`.
    ComponentTransfer {
        /// Вход.
        input: FeInput,
        /// Функции R, G, B, A.
        funcs: Box<[FeTransfer; 4]>,
    },
    /// `feDropShadow`.
    DropShadow {
        /// Вход.
        input: FeInput,
        /// `dx`.
        dx: f32,
        /// `dy`.
        dy: f32,
        /// `stdDeviation` по X.
        sx: f32,
        /// `stdDeviation` по Y.
        sy: f32,
        /// `flood-color`.
        color: Color,
        /// `flood-opacity`.
        opacity: f32,
    },
    /// `feTile`: подобласть входа замощает подобласть примитива.
    Tile {
        /// Вход.
        input: FeInput,
    },
    /// `feMorphology`.
    Morphology {
        /// Вход.
        input: FeInput,
        /// `operator="dilate"` (иначе `erode`).
        dilate: bool,
        /// `radius` по X.
        rx: f32,
        /// `radius` по Y.
        ry: f32,
    },
    /// `feConvolveMatrix`; пустое `kernel` — недопустимые атрибуты, результат
    /// прозрачный чёрный.
    ConvolveMatrix {
        /// Вход.
        input: FeInput,
        /// `order` (столбцы, строки).
        order: (usize, usize),
        /// `kernelMatrix` по строкам.
        kernel: Vec<f32>,
        /// `divisor` (уже подставлена сумма ядра, если не задан или 0).
        divisor: f32,
        /// `bias`.
        bias: f32,
        /// `targetX`, `targetY`.
        target: (i32, i32),
        /// `edgeMode`.
        edge: FeEdgeMode,
        /// `preserveAlpha`.
        preserve_alpha: bool,
    },
    /// `feDisplacementMap`.
    DisplacementMap {
        /// `in`.
        input: FeInput,
        /// `in2` (карта смещений).
        map: FeInput,
        /// `scale`.
        scale: f32,
        /// `xChannelSelector`: 0..3 = R, G, B, A.
        x_channel: usize,
        /// `yChannelSelector`.
        y_channel: usize,
    },
    /// `feTurbulence`.
    Turbulence {
        /// `baseFrequency` по X.
        fx: f32,
        /// `baseFrequency` по Y.
        fy: f32,
        /// `numOctaves`.
        octaves: u32,
        /// `seed` (округлён).
        seed: i32,
        /// `stitchTiles="stitch"`.
        stitch: bool,
        /// `type="fractalNoise"` (иначе `turbulence`).
        fractal: bool,
    },
    /// `feDiffuseLighting` / `feSpecularLighting`; без источника света — прозрачный
    /// чёрный.
    Lighting {
        /// Вход (карта высот — его альфа).
        input: FeInput,
        /// Диффузное или бликовое.
        kind: FeLighting,
        /// `surfaceScale`.
        surface_scale: f32,
        /// `lighting-color`.
        color: Color,
        /// Источник света.
        light: Option<FeLight>,
    },
    /// Примитив, которого движок ещё не исполняет (`feImage` со ссылкой на элемент). Результат — вход
    /// без изменений; примитив без входа даёт [`FeInput::Empty`], то есть
    /// прозрачный чёрный.
    Unsupported {
        /// Вход, который проходит как есть.
        input: FeInput,
    },
}
