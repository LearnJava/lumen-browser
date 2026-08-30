//! Формы обтекания и мотион-путь (`shape-outside`/`offset-rotate`),
//! печать и адаптация шрифта (`print-color-adjust`/`font-size-adjust`),
//! режим письма (`writing-mode`/`text-orientation`), выделение текста
//! (`user-select`), прокрутка (`scroll-behavior`/`scroll-snap-*`/
//! `overscroll-behavior`).
//!
//! Перенесено батчем SPLIT-ST17 из `crates/engine/layout/src/style.rs`
//! (анкер `enum ShapeOutside` до конца `impl ScrollBehavior`) без правок тел.

/// CSS Shapes L1 §3 — `shape-outside` value. NOT inherited. Initial: `None`.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ShapeOutside {
    #[default]
    None,
    /// `<basic-shape>` or `<url>` or `<box-value>` — stored as raw string for Phase 0.
    Value(String),
}

/// CSS Motion Path L1 §3 — `offset-rotate`. NOT inherited. Initial: `Auto`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum OffsetRotate {
    #[default]
    Auto,
    /// `auto <angle>` — auto direction plus a fixed rotation offset.
    AutoAngle(f32),
    Reverse,
    Angle(f32),
}

/// CSS Color Adjustment L1 §5 — `print-color-adjust`. NOT inherited. Initial: `Economy`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PrintColorAdjust {
    #[default]
    Economy,
    Exact,
}

/// CSS Fonts L5 §4 — `font-size-adjust`. Inherited. Initial: `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum FontSizeAdjust {
    #[default]
    None,
    Auto,
    Value(f32),
}

/// CSS Writing Modes L3 §2.1 — `writing-mode`. Inherited. Initial: `HorizontalTb`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WritingMode {
    /// `horizontal-tb` — left-to-right horizontal, top-to-bottom block.
    #[default]
    HorizontalTb,
    /// `vertical-rl` — top-to-bottom vertical, right-to-left block.
    VerticalRl,
    /// `vertical-lr` — top-to-bottom vertical, left-to-right block.
    VerticalLr,
    /// `sideways-rl` — same as vertical-rl but glyphs rotated 90° CW.
    SidewaysRl,
    /// `sideways-lr` — same as vertical-lr but glyphs rotated 90° CCW.
    SidewaysLr,
}

/// CSS Writing Modes L3 §6.5 — `text-orientation`. Inherited. Initial: `Mixed`.
/// Only meaningful in vertical writing modes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextOrientation {
    /// `mixed` — rotate CJK upright, rotate others 90° CW.
    #[default]
    Mixed,
    /// `upright` — all glyphs upright; implies `direction: ltr`.
    Upright,
    /// `sideways` — all glyphs rotated 90° CW (like vertical-rl inline).
    Sideways,
}

/// CSS UI L4 §6.2 — `user-select`. Inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UserSelect {
    #[default]
    Auto,
    Text,
    None,
    Contain,
    All,
}

impl UserSelect {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "text" => Some(Self::Text),
            "none" => Some(Self::None),
            "contain" => Some(Self::Contain),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// CSS Overflow L3 — `scroll-behavior`. Inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollBehavior {
    #[default]
    Auto,
    Smooth,
}

/// CSS Scroll Snap L1 §3.1 — `scroll-snap-type: none | <axis> [mandatory | proximity]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollSnapType {
    pub axis: ScrollSnapAxis,
    pub strictness: ScrollSnapStrictness,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapAxis {
    #[default]
    None,
    X,
    Y,
    Block,
    Inline,
    Both,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapStrictness {
    #[default]
    Proximity,
    Mandatory,
}

/// CSS Scroll Snap L1 §6.1 — `scroll-snap-align: none | <axis-keyword>{1,2}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollSnapAlign {
    pub block: ScrollSnapAlignKeyword,
    pub inline: ScrollSnapAlignKeyword,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapAlignKeyword {
    #[default]
    None,
    Start,
    End,
    Center,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapStop {
    #[default]
    Normal,
    Always,
}

/// CSS Overscroll Behavior L1 §2 — `overscroll-behavior: auto | contain | none`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OverscrollBehavior {
    #[default]
    Auto,
    Contain,
    None,
}

impl ScrollBehavior {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "smooth" => Some(Self::Smooth),
            _ => None,
        }
    }
}

