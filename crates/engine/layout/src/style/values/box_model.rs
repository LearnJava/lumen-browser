//! Типы значений CSS для бокс-модели: SVG-краска (`fill`/`stroke`), границы
//! таблиц (`border-collapse`/`empty-cells`), SVG-параметры отрисовки
//! (`fill-rule`/`stroke-linecap`/`stroke-linejoin`/`paint-order`), стили
//! рамок и outline, `box-sizing`, позиционирование (`position`/`float`/
//! `clear`/`isolation`/`mix-blend-mode`/`vertical-align`).
//!
//! Перенесено батчем SPLIT-ST16 из `crates/engine/layout/src/style.rs`
//! (анкер `enum SvgPaint` до конца `impl VerticalAlign`) без правок тел.

use crate::style::values::color::Color;

/// SVG Presentation §11.2 — `fill` / `stroke` paint value (`<paint>` type).
/// Used by SVG shape elements. Inherited by descendants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SvgPaint {
    /// `none` — shape not painted (fully transparent).
    None,
    /// `currentColor` — resolves to the element's computed CSS `color`.
    CurrentColor,
    /// Explicit sRGB color value.
    Color(Color),
}

impl Default for SvgPaint {
    /// SVG §11.2 default fill is black; stroke default is none.
    /// For fill fields use `SvgPaint::Color(Color::BLACK)`; for stroke use `SvgPaint::None`.
    fn default() -> Self {
        SvgPaint::None
    }
}

impl SvgPaint {
    /// Resolves the paint value to a concrete `Color`. Returns `None` if paint is `none`.
    pub fn resolve(self, current_color: Color) -> Option<Color> {
        match self {
            SvgPaint::None => None,
            SvgPaint::CurrentColor => Some(current_color),
            SvgPaint::Color(c) => Some(c),
        }
    }
}

/// CSS Tables L2 §17.6 — `border-collapse`. Inherited. Initial: `Separate`.
/// Controls whether adjacent cell borders are merged (`collapse`) or kept separate (`separate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderCollapse {
    /// Each cell has its own borders separated by `border-spacing`.
    #[default]
    Separate,
    /// Adjacent borders are merged into a single shared border (no `border-spacing`).
    Collapse,
}

impl BorderCollapse {
    /// Parse CSS keyword; returns `None` for unrecognised values.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "separate" => Some(Self::Separate),
            "collapse" => Some(Self::Collapse),
            _ => None,
        }
    }
}

/// CSS Tables L2 §17.6.1.1 — `empty-cells`. Inherited. Initial: `Show`.
/// In the separated-borders model, controls whether borders and backgrounds
/// are drawn around table cells that have no in-flow content. Has no effect
/// when `border-collapse: collapse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmptyCells {
    /// Empty cells are painted normally (borders + background drawn).
    #[default]
    Show,
    /// Empty cells suppress their borders and background.
    Hide,
}

impl EmptyCells {
    /// Parse CSS keyword; returns `None` for unrecognised values.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "show" => Some(Self::Show),
            "hide" => Some(Self::Hide),
            _ => None,
        }
    }
}

/// SVG §11.3 — `fill-rule`. Inherited. Initial: `NonZero`.
/// Controls how the interior of a shape is determined for overlapping contours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FillRule {
    /// Nonzero winding rule: count crossings, fill if winding number ≠ 0.
    #[default]
    NonZero,
    /// Even-odd rule: count crossings, fill if count is odd.
    EvenOdd,
}

/// SVG §11.4 — `stroke-linecap`. Inherited. Initial: `Butt`.
/// Shape of the cap at the end of open sub-paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokeLinecap {
    /// Flat cap exactly at the endpoint (default).
    #[default]
    Butt,
    /// Semicircular cap extending `stroke-width/2` past the endpoint.
    Round,
    /// Rectangular cap extending `stroke-width/2` past the endpoint.
    Square,
}

/// SVG §11.4 — `stroke-linejoin`. Inherited. Initial: `Miter`.
/// Shape of join between connected path segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokeLinejoin {
    /// Pointed join, bounded by `stroke-miterlimit` (default).
    #[default]
    Miter,
    /// Circular join.
    Round,
    /// Flat bevel cut at the join.
    Bevel,
}

/// CSS Fill & Stroke L3 §6 / SVG 2 §13.7 — one component of `paint-order`.
/// Identifies which of fill, stroke or markers occupies a given paint slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintOrderSlot {
    /// The element's fill.
    Fill,
    /// The element's stroke.
    Stroke,
    /// SVG markers. Lumen does not yet render markers; the slot is preserved so
    /// that fill/stroke ordering around it stays spec-correct.
    Markers,
}

/// CSS Fill & Stroke L3 §6 / SVG 2 §13.7 — `paint-order`. Inherited.
/// Resolved order in which the three components are painted, first slot drawn
/// first (so the last slot ends up on top). Initial value `normal` resolves to
/// `[Fill, Stroke, Markers]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SvgPaintOrder(pub [PaintOrderSlot; 3]);

impl Default for SvgPaintOrder {
    fn default() -> Self {
        Self([PaintOrderSlot::Fill, PaintOrderSlot::Stroke, PaintOrderSlot::Markers])
    }
}

impl SvgPaintOrder {
    /// Parses `normal | [ fill || stroke || markers ]` (CSS Fill & Stroke L3 §6).
    /// Returns `None` for an unknown token or a repeated component. Components
    /// omitted from an otherwise-valid list are appended in the canonical
    /// `fill, stroke, markers` order, as the spec requires.
    pub fn parse(value: &str) -> Option<Self> {
        use PaintOrderSlot::{Fill, Markers, Stroke};
        let v = value.trim();
        if v.eq_ignore_ascii_case("normal") {
            return Some(Self::default());
        }
        let mut order: Vec<PaintOrderSlot> = Vec::with_capacity(3);
        for tok in v.split_whitespace() {
            let slot = if tok.eq_ignore_ascii_case("fill") {
                Fill
            } else if tok.eq_ignore_ascii_case("stroke") {
                Stroke
            } else if tok.eq_ignore_ascii_case("markers") {
                Markers
            } else {
                return None;
            };
            if order.contains(&slot) {
                return None; // repeated component — invalid per grammar
            }
            order.push(slot);
        }
        if order.is_empty() {
            return None;
        }
        for slot in [Fill, Stroke, Markers] {
            if !order.contains(&slot) {
                order.push(slot);
            }
        }
        Some(Self([order[0], order[1], order[2]]))
    }

    /// True when fill is painted before stroke (so the stroke is drawn on top).
    /// Markers are ignored — Lumen does not render them. Default `normal`
    /// (fill, stroke, markers) returns `true`; `paint-order: stroke` → `false`.
    pub fn fill_before_stroke(&self) -> bool {
        let fill_idx = self.0.iter().position(|s| *s == PaintOrderSlot::Fill);
        let stroke_idx = self.0.iter().position(|s| *s == PaintOrderSlot::Stroke);
        match (fill_idx, stroke_idx) {
            (Some(f), Some(s)) => f <= s,
            _ => true,
        }
    }
}

/// Стиль линии CSS border. None = рамка не отображается (как `display: none`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BorderStyle {
    #[default]
    None,
    Solid,
    Dashed,
    Dotted,
    Double,
}

impl BorderStyle {
    pub fn is_visible(self) -> bool {
        !matches!(self, BorderStyle::None)
    }
}

/// CSS Basic UI L4 §5.3 — `outline-style`. Включает все `<border-style>`
/// keyword-ы плюс `auto` (UA-defined focus indicator).
///
/// Phase 0: `Auto` рендерится как Solid с currentColor; отдельный variant
/// сохраняется, чтобы позже отличить «явный solid от автора» от «default
/// UA focus ring» — нужно для accessibility (нельзя глушить focus ring
/// через `outline-style: none` при `:focus-visible` в стиле UA).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutlineStyle {
    #[default]
    None,
    Auto,
    Solid,
    Dashed,
    Dotted,
}

impl OutlineStyle {
    pub fn is_visible(self) -> bool {
        !matches!(self, OutlineStyle::None)
    }
}

/// CSS Basic UI L4 §5.4 — `outline-color`. Помимо явного цвета поддерживает
/// `auto` (UA-defined контрастный цвет) и `currentColor` (вычисленный `color`
/// элемента).
///
/// Phase 0: `Auto` и `CurrentColor` оба резолвятся в `style.color` при
/// рендеринге — настоящий UA contrast требует знания фона за outline и
/// откладывается.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutlineColor {
    #[default]
    Auto,
    CurrentColor,
    Color(Color),
}

/// CSS Fragmentation L3 §3.1 — break-before / break-after / break-inside.
/// Phase 0: parse+store; реальный break enforcement требует pagination /
/// multi-column layout pipeline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BreakValue {
    #[default]
    Auto,
    /// `avoid` / `avoid-page` / `avoid-column` / `avoid-region` — все
    /// нормализуются в `Avoid`. Phase 0 не различает page vs column vs region.
    Avoid,
    /// `always` / `page` (для break-before/after).
    Always,
    /// `column` — принудительный column break.
    Column,
    /// `page` — принудительный page break.
    Page,
    /// `region` — принудительный region break.
    Region,
}

/// CSS `box-sizing`. Определяет, что именно задаёт `width` / `height`:
///   - `ContentBox` (CSS default): размер контента; padding и border прибавляются сверху.
///   - `BorderBox`: размер вместе с padding и border; контент сжимается, чтобы влезть.
///
/// Свойство НЕ наследуется (CSS Basic UI 3 §4.1) — сбрасывается на default в каждом
/// `compute_style`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum BoxSizing {
    #[default]
    ContentBox,
    BorderBox,
}

/// CSS Positioned Layout L3 §3 — `position`. Не наследуется.
/// `Static` — нормальный поток (default). Остальные создают
/// containing-block-альтернативу и (для `Fixed` / `Sticky`, а также
/// `Relative` / `Absolute` с явным `z-index`) могут создавать
/// stacking context (§9.10).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

impl Position {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "static" => Some(Self::Static),
            "relative" => Some(Self::Relative),
            "absolute" => Some(Self::Absolute),
            "fixed" => Some(Self::Fixed),
            "sticky" => Some(Self::Sticky),
            _ => None,
        }
    }
}

/// CSS 2.1 §9.5.1 — `float`. Не наследуется. `Left`/`Right` выводят
/// элемент из нормального потока и размещают его у соответствующего
/// края контейнера; следующий контент обтекает float сбоку.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FloatSide {
    #[default]
    None,
    Left,
    Right,
}

impl FloatSide {
    /// Parses `float` keyword value.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "inline-start" => Some(Self::Left),
            "inline-end" => Some(Self::Right),
            _ => None,
        }
    }

    /// Returns `true` for `float: none`.
    pub fn is_none(self) -> bool {
        matches!(self, Self::None)
    }
}

/// CSS 2.1 §9.5.2 — `clear`. Не наследуется. Указывает, мимо
/// каких float-ов следующий блок должен «пройти» перед размещением.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClearSide {
    #[default]
    None,
    Left,
    Right,
    Both,
}

impl ClearSide {
    /// Parses `clear` keyword value.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "left" | "inline-start" => Some(Self::Left),
            "right" | "inline-end" => Some(Self::Right),
            "both" => Some(Self::Both),
            _ => None,
        }
    }
}

/// CSS Compositing & Blending L1 §2.1 — `isolation`. Не наследуется.
/// `Isolate` принудительно создаёт stacking context, обеспечивая
/// изоляцию blend / backdrop-filter эффектов потомков от внешних
/// слоёв.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Isolation {
    #[default]
    Auto,
    Isolate,
}

impl Isolation {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "isolate" => Some(Self::Isolate),
            _ => None,
        }
    }
}

/// CSS Compositing & Blending L1 §3.1 — `mix-blend-mode`. Не наследуется.
/// Любое значение, отличное от `Normal`, создаёт stacking context
/// (§9.10). Phase 0 layout только хранит — реальный compositor pipeline
/// для blend-effects появится у P2 (§16 трек, п.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MixBlendMode {
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

impl MixBlendMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "multiply" => Some(Self::Multiply),
            "screen" => Some(Self::Screen),
            "overlay" => Some(Self::Overlay),
            "darken" => Some(Self::Darken),
            "lighten" => Some(Self::Lighten),
            "color-dodge" => Some(Self::ColorDodge),
            "color-burn" => Some(Self::ColorBurn),
            "hard-light" => Some(Self::HardLight),
            "soft-light" => Some(Self::SoftLight),
            "difference" => Some(Self::Difference),
            "exclusion" => Some(Self::Exclusion),
            "hue" => Some(Self::Hue),
            "saturation" => Some(Self::Saturation),
            "color" => Some(Self::Color),
            "luminosity" => Some(Self::Luminosity),
            "plus-lighter" => Some(Self::PlusLighter),
            _ => None,
        }
    }
}

/// CSS Inline Layout / CSS 2.1 §10.8.1 — `vertical-align`. Не наследуется.
/// Default `Baseline`.
///
/// Keyword-варианты (`Baseline`, `Sub`, `Super`, `Top`, `TextTop`, `Middle`,
/// `Bottom`, `TextBottom`) — fixed enum values. `Length(px)` — resolved
/// сдвиг по вертикали от baseline (positive = up по CSS, как у всех
/// vertical-shift свойств). `Percent(p)` — процент от `line-height` текущего
/// элемента; разрешается во время layout-а, поскольку требует line-box
/// геометрии.
///
/// Phase 0: parsing + storage. Реальное применение к inline-flow требует
/// поля `y_offset` в `InlineFrag` и совместной правки `lumen-paint`
/// (DrawText.y-offset) — отдельная задача с согласованием P2.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum VerticalAlign {
    #[default]
    Baseline,
    Sub,
    Super,
    Top,
    TextTop,
    Middle,
    Bottom,
    TextBottom,
    /// Resolved px. Положительное — выше baseline, отрицательное — ниже
    /// (как `<length>` в CSS 2.1 §10.8.1).
    Length(f32),
    /// Процент от `line-height` элемента (CSS 2.1 §10.8.1). Резолвится
    /// в layout-pass — здесь хранится как есть.
    Percent(f32),
}

impl VerticalAlign {
    /// Парсит keyword-формы vertical-align. Не покрывает `<length>` /
    /// `<percentage>` — те идут через [`parse_length`] (см. apply_declaration).
    pub fn parse_keyword(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "baseline" => Some(Self::Baseline),
            "sub" => Some(Self::Sub),
            "super" => Some(Self::Super),
            "top" => Some(Self::Top),
            "text-top" => Some(Self::TextTop),
            "middle" => Some(Self::Middle),
            "bottom" => Some(Self::Bottom),
            "text-bottom" => Some(Self::TextBottom),
            _ => None,
        }
    }
}


