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
    /// `color-interpolation-filters: linearRGB` (значение по умолчанию) или `sRGB`.
    pub linear_rgb: bool,
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

/// Вид примитива с параметрами.
#[derive(Debug, Clone, PartialEq)]
pub enum FePrimitive {
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
    /// Примитив, которого движок ещё не исполняет (`feDisplacementMap`, `feTile`,
    /// `feMorphology`, `feConvolveMatrix`, `feImage`, `feTurbulence`, освещение).
    /// Результат — вход без изменений; примитив без входа (`feImage`, `feTurbulence`,
    /// освещение) даёт [`FeInput::Empty`], то есть прозрачный чёрный.
    Unsupported {
        /// Вход, который проходит как есть.
        input: FeInput,
    },
}
