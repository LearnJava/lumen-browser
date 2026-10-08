//! Типы значений CSS: содержимое (`content`), маркеры списков
//! (`list-style-*`), перенос текста (`overflow-wrap`/`line-break`/
//! `word-break`/`hyphens`), полосы прокрутки (`scrollbar-width`/
//! `scrollbar-gutter`), интерактивность (`touch-action`/`appearance`/
//! `field-sizing`/`pointer-events`/`resize`).
//!
//! Перенесено батчем SPLIT-ST17 из `crates/engine/layout/src/style.rs`
//! (анкер `enum Content` до конца `impl Resize`) без правок тел.

use crate::style::values::length::{parse_length_q, split_top_level_ws, Length};
use crate::style::values::scroll::WritingMode;

/// CSS Content L3 — value свойства `content`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Content {
    /// `normal` (default) — поведение по умолчанию для каждого element.
    #[default]
    Normal,
    /// `none` — pseudo-element не генерируется.
    None,
    /// Список фрагментов: строки, counter()/counters(), attr(), url().
    /// Phase 0 хранит список typed-фрагментов; конкатенация для render —
    /// задача paint pipeline.
    Items(Vec<ContentItem>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentItem {
    /// Литеральная строка из CSS-string-literal (без кавычек).
    String(String),
    /// `attr(name)` — значение HTML-атрибута текущего element.
    Attr(String),
    /// `url("path")` — изображение / external resource.
    Url(String),
    /// `counter(name [, style])` — значение counter-а. `style` — пока
    /// сырая строка (Phase 0 разрешит только `decimal` etc.).
    Counter {
        name: String,
        style: Option<String>,
    },
    /// `counters(name, separator [, style])` — вложенные counters
    /// (`1.2.3` через `.`).
    Counters {
        name: String,
        separator: String,
        style: Option<String>,
    },
    /// `open-quote` / `close-quote` — quotation marks per `quotes` property.
    OpenQuote,
    CloseQuote,
    NoOpenQuote,
    NoCloseQuote,
}

/// CSS Generated Content L3 §3.2 — `quotes`. Inherited. Initial: `auto`.
///
/// Controls the quotation marks produced by `content: open-quote` /
/// `close-quote`. The nesting depth (which pair is used) is tracked in
/// document order by the counters pre-pass; this value only supplies the
/// glyph pairs to choose from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Quotes {
    /// `auto` — UA language-appropriate quotation marks. Lumen uses English
    /// curly quotes: primary “ ”, secondary ‘ ’.
    #[default]
    Auto,
    /// `none` — `open-quote` / `close-quote` produce no marks (depth still
    /// advances).
    None,
    /// Explicit `[<string> <string>]+` pairs — outermost (depth 0) first.
    /// Each tuple is `(open, close)`.
    Pairs(Vec<(String, String)>),
}

impl Quotes {
    /// Returns the `(open, close)` glyph strings for the given nesting `depth`.
    ///
    /// `Auto` uses the built-in English pairs; `Pairs` clamps `depth` to the
    /// last available pair (CSS Content L3 §3.2). Returns `None` for `quotes:
    /// none` or an empty explicit list — the caller emits nothing in that case.
    pub fn pair_for_depth(&self, depth: usize) -> Option<(&str, &str)> {
        const AUTO: &[(&str, &str)] = &[("\u{201C}", "\u{201D}"), ("\u{2018}", "\u{2019}")];
        match self {
            Quotes::None => None,
            Quotes::Auto => {
                let idx = depth.min(AUTO.len() - 1);
                Some(AUTO[idx])
            }
            Quotes::Pairs(pairs) => {
                if pairs.is_empty() {
                    return None;
                }
                let idx = depth.min(pairs.len() - 1);
                let (o, c) = &pairs[idx];
                Some((o.as_str(), c.as_str()))
            }
        }
    }
}

/// CSS Scrollbars 1 — `scrollbar-width`. Inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollbarWidth {
    #[default]
    Auto,
    /// `thin` — тонкий scrollbar.
    Thin,
    /// `none` — без visible scrollbar (контент всё ещё скроллится через
    /// keyboard / touch / programmatic).
    None,
}

impl ScrollbarWidth {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "thin" => Some(Self::Thin),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// CSS Overflow L3 — `scrollbar-gutter`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollbarGutter {
    /// `auto` (default) — gutter появляется когда overflow:scroll.
    #[default]
    Auto,
    /// `stable` — gutter всегда зарезервирован (не двигает контент при scroll).
    Stable,
    /// `stable both-edges` — gutter на обоих краях для симметрии.
    StableBothEdges,
}

impl ScrollbarGutter {
    pub fn parse(s: &str) -> Option<Self> {
        let lc = s.trim().to_ascii_lowercase();
        if lc == "auto" {
            return Some(Self::Auto);
        }
        if lc == "stable" {
            return Some(Self::Stable);
        }
        // `stable && both-edges?` — double-bar grammar, order-independent
        // (CSS Overflow L4 §3.3, confirmed by WPT
        // `scrollbar-gutter-valid.html`'s `"both-edges stable"` case).
        let tokens: Vec<&str> = lc.split_whitespace().collect();
        if tokens == ["stable", "both-edges"] || tokens == ["both-edges", "stable"] {
            return Some(Self::StableBothEdges);
        }
        None
    }
}

/// CSS Scroll Anchoring 1 — `overflow-anchor`. Не наследуется, initial `auto`
/// (BUG-524 срез 1 — только грамматика/CSSOM, сам алгоритм якорения
/// (выбор anchor node на scroll-контейнере и компенсация scroll offset при
/// relayout) не реализован — см. `bugs/BUG-524-OPEN.md`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OverflowAnchor {
    /// `auto` (default) — элемент участвует в выборе anchor node.
    #[default]
    Auto,
    /// `none` — элемент (и поддерево) исключён из выбора anchor node.
    None,
}

impl OverflowAnchor {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// CSS Overflow L3 §overflow-clip-margin — the `<visual-box>` component of
/// the property's `[<visual-box> || <length [0,∞]>]` grammar (BUG-505
/// срез 4). Initial `padding-box`, same as `background-origin`'s box triplet
/// (`BackgroundOrigin`, `style/values/background.rs`) — a separate enum
/// rather than reusing that one, matching this codebase's existing "one
/// small enum per property" convention for the identical
/// content-box/padding-box/border-box triplet (`BackgroundClip`, `MaskClip`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OverflowClipMarginBox {
    /// `content-box` — clip region extends from the content edge.
    ContentBox,
    /// `padding-box` (initial) — clip region extends from the padding edge.
    #[default]
    PaddingBox,
    /// `border-box` — clip region extends from the border edge.
    BorderBox,
}

impl OverflowClipMarginBox {
    /// Parses a single `<visual-box>` keyword. `None` for anything else,
    /// including `margin-box` (not part of this property's grammar, per
    /// WPT `overflow-clip-margin.html`'s `test_invalid_value(...,
    /// 'margin-box')`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "content-box" => Some(Self::ContentBox),
            "padding-box" => Some(Self::PaddingBox),
            "border-box" => Some(Self::BorderBox),
            _ => None,
        }
    }

    /// Serializes back to its CSS keyword.
    pub fn to_css(self) -> &'static str {
        match self {
            Self::ContentBox => "content-box",
            Self::PaddingBox => "padding-box",
            Self::BorderBox => "border-box",
        }
    }
}

/// CSS Overflow L5 §scroll-target-group — `none | auto`. Not to be confused
/// with `scroll-marker-group`: this property opts an element's descendant
/// scroll-snap targets into an implicit `::scroll-marker` group, it doesn't
/// place a pseudo-element.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollTargetGroup {
    #[default]
    None,
    Auto,
}

impl ScrollTargetGroup {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }

    pub fn to_css(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Auto => "auto",
        }
    }
}

/// `before`/`after` half of `scroll-marker-group`'s value (BUG-505 срез 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollMarkerGroupPlacement {
    Before,
    After,
}

/// The experimental `tabs`/`links` interaction-mode component (tentative,
/// github.com/w3c/csswg-drafts/issues/12122 — not in the stable spec text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollMarkerGroupMode {
    Tabs,
    Links,
}

/// CSS Overflow L5 §scroll-marker-group-property. Grammar: `none | [ before
/// | after ] [ tabs | links ]?` — order-dependent (the direction keyword
/// must come first; `links after`/`tabs before` are invalid, confirmed by
/// WPT `scroll-markers-invalid{,.tentative}.html`), unlike `scrollbar-
/// gutter`'s order-independent `&&` combinator above. The property's own
/// `none` initial value is represented by the *absence* of this type
/// (`ComputedStyle::scroll_marker_group: Option<Self>`), not a variant of
/// it — there's nothing to place `before`/`after` when the value is `none`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollMarkerGroup {
    pub placement: ScrollMarkerGroupPlacement,
    pub mode: Option<ScrollMarkerGroupMode>,
}

impl ScrollMarkerGroup {
    /// Parses the whole property value, `none` included. `Some(None)` for
    /// `none`, `Some(Some(value))` for a valid placement(+mode), `None` for
    /// anything invalid — the double `Option` mirrors the property's own
    /// value space (initial-as-absence) rather than reusing this module's
    /// usual bare-`Option<Self>` `parse` convention.
    pub fn parse(s: &str) -> Option<Option<Self>> {
        let lc = s.trim().to_ascii_lowercase();
        if lc == "none" {
            return Some(None);
        }
        let tokens: Vec<&str> = lc.split_whitespace().collect();
        match tokens.as_slice() {
            [side] => Self::parse_placement(side)
                .map(|placement| Some(Self { placement, mode: None })),
            [side, mode] => {
                let placement = Self::parse_placement(side)?;
                let mode = match *mode {
                    "tabs" => ScrollMarkerGroupMode::Tabs,
                    "links" => ScrollMarkerGroupMode::Links,
                    _ => return None,
                };
                Some(Some(Self { placement, mode: Some(mode) }))
            }
            _ => None,
        }
    }

    fn parse_placement(s: &str) -> Option<ScrollMarkerGroupPlacement> {
        match s {
            "before" => Some(ScrollMarkerGroupPlacement::Before),
            "after" => Some(ScrollMarkerGroupPlacement::After),
            _ => None,
        }
    }

    pub fn to_css(self) -> String {
        let side = match self.placement {
            ScrollMarkerGroupPlacement::Before => "before",
            ScrollMarkerGroupPlacement::After => "after",
        };
        match self.mode {
            None => side.to_string(),
            Some(ScrollMarkerGroupMode::Tabs) => format!("{side} tabs"),
            Some(ScrollMarkerGroupMode::Links) => format!("{side} links"),
        }
    }
}

/// CSS Lists L3 §2.1 — markers для list items.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ListStyleType {
    /// `none` — без marker.
    None,
    /// `disc` — закрашенный кружок (default для ul).
    #[default]
    Disc,
    /// `circle` — пустой кружок.
    Circle,
    /// `square` — квадратик.
    Square,
    /// `decimal` — 1, 2, 3, ... (default для ol).
    Decimal,
    /// `decimal-leading-zero` — 01, 02, ..., 09, 10, ...
    DecimalLeadingZero,
    /// `lower-roman` — i, ii, iii, ...
    LowerRoman,
    /// `upper-roman` — I, II, III, ...
    UpperRoman,
    /// `lower-alpha` / `lower-latin` — a, b, c, ...
    LowerAlpha,
    /// `upper-alpha` / `upper-latin` — A, B, C, ...
    UpperAlpha,
    /// `lower-greek` — α, β, γ, ...
    LowerGreek,
    /// `<custom-ident>` — ссылка на именованный `@counter-style`.
    Custom(Box<str>),
}

impl ListStyleType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "disc" => Some(Self::Disc),
            "circle" => Some(Self::Circle),
            "square" => Some(Self::Square),
            "decimal" => Some(Self::Decimal),
            "decimal-leading-zero" => Some(Self::DecimalLeadingZero),
            "lower-roman" => Some(Self::LowerRoman),
            "upper-roman" => Some(Self::UpperRoman),
            "lower-alpha" | "lower-latin" => Some(Self::LowerAlpha),
            "upper-alpha" | "upper-latin" => Some(Self::UpperAlpha),
            "lower-greek" => Some(Self::LowerGreek),
            // Any unrecognised ident is a reference to a named @counter-style.
            s if !s.is_empty() => Some(Self::Custom(s.into())),
            _ => None,
        }
    }
}

/// CSS Lists L3 §2.3 — `list-style-position`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ListStylePosition {
    /// `outside` (default) — marker вне content-area.
    #[default]
    Outside,
    /// `inside` — marker внутри content-area.
    Inside,
}

impl ListStylePosition {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "outside" => Some(Self::Outside),
            "inside" => Some(Self::Inside),
            _ => None,
        }
    }
}

/// CSS Text L3 §5.2 — `overflow-wrap`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OverflowWrap {
    #[default]
    Normal,
    /// `break-word` — разрешает перенос любого слова, чтобы не было overflow.
    BreakWord,
    /// `anywhere` — как `break-word`, но также влияет на intrinsic-width
    /// computation (CSS Text L3).
    Anywhere,
}

impl OverflowWrap {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "break-word" => Some(Self::BreakWord),
            "anywhere" => Some(Self::Anywhere),
            _ => None,
        }
    }
}

/// CSS Text L3 §5.2 — `line-break`. Inherited. Initial: `Auto`.
/// Управляет строгостью правил переноса CJK-текста по пробелам.
/// Phase 0: parse + store; реальный CJK-wrap — отдельная задача.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LineBreak {
    #[default]
    Auto,
    Loose,
    Normal,
    Strict,
    Anywhere,
}

/// CSS Text L3 §5.1 — `word-break`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WordBreak {
    #[default]
    Normal,
    /// `keep-all` — CJK не разбивается.
    KeepAll,
    /// `break-all` — разрыв в любом месте, кроме whitespace.
    BreakAll,
    /// `break-word` — legacy для `overflow-wrap: break-word`.
    BreakWord,
    /// `auto-phrase` (CSS Text L4 §5.1) — фразовые разрывы CJK. Хранится ради
    /// computed-значения; layout ведёт себя как `normal`.
    AutoPhrase,
}

impl WordBreak {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "keep-all" => Some(Self::KeepAll),
            "break-all" => Some(Self::BreakAll),
            "break-word" => Some(Self::BreakWord),
            "auto-phrase" => Some(Self::AutoPhrase),
            _ => None,
        }
    }
}

/// CSS Text L3 §6 — `hyphens`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Hyphens {
    /// `none` — переносы запрещены.
    None,
    /// `manual` (default) — переносы только при явных hyphenation-точках
    /// (`&shy;` / U+00AD).
    #[default]
    Manual,
    /// `auto` — UA расставляет переносы по алгоритму (требует hyphenation
    /// dictionary).
    Auto,
}

impl Hyphens {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "manual" => Some(Self::Manual),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// CSS Pointer Events L3 / Touch Events — `touch-action`. NOT inherited. Initial: `Auto`.
/// Указывает, какими жестами UA управляет самостоятельно (pan/zoom).
/// Phase 0: parse + store; реальная обработка touch-жестов — P3 task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TouchAction {
    #[default]
    Auto,
    None,
    PanX,
    PanLeft,
    PanRight,
    PanY,
    PanUp,
    PanDown,
    PinchZoom,
    Manipulation,
}

/// CSS Basic UI L4 §5 — `appearance`. NOT inherited. Initial: `Auto`.
/// Контролирует отображение элемента согласно UA-теме (форм-виджеты).
/// Phase 0: parse + store; реальная стилизация форм-виджетов — P2/P3 task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    Auto,
    None,
    /// `menulist-button` / `searchfield` / `textfield` / `button` и прочие
    /// platform-специфичные значения — хранятся как Compat.
    Compat,
    /// `base-select` (HTML/CSS «Customizable Select») — `<select>` рендерится
    /// как author-стилизуемое дерево (кнопка-триггер + `<selectedcontent>` +
    /// `::picker(select)` со списком опций) вместо непрозрачного нативного
    /// контрола. См. `box_tree.rs` (построение дерева) и `forms.rs` (поповер).
    BaseSelect,
}

/// CSS Basic UI L4 §4.4 — `field-sizing`. NOT inherited. Initial: `Fixed`.
/// `Fixed` — UA-specified dimensions apply (default browser behaviour).
/// `Content` — intrinsic size comes from the control's text content.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FieldSizing {
    /// UA default dimensions (e.g. `<input>` is 174×21 px).
    #[default]
    Fixed,
    /// Size the control to fit its text content (CSS Basic UI L4 §4.4).
    Content,
}

/// CSS Pointer Events L1. Default `auto`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PointerEvents {
    #[default]
    Auto,
    None,
    Visible,
    /// `painted` / `fill` / `stroke` / `all` — для SVG. В non-SVG
    /// контексте трактуются как `auto`.
    Painted,
    Fill,
    Stroke,
    All,
}

impl PointerEvents {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "none" => Some(Self::None),
            "visible" | "visiblepainted" | "visiblefill" | "visiblestroke" => {
                Some(Self::Visible)
            }
            "painted" => Some(Self::Painted),
            "fill" => Some(Self::Fill),
            "stroke" => Some(Self::Stroke),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// CSS Basic UI L4 §6 — `resize`. NOT inherited. Initial: `None`.
/// Позволяет пользователю изменять размер элемента мышью.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Resize {
    /// `none` — resize запрещён.
    #[default]
    None,
    /// `both` — resize по обеим физическим осям.
    Both,
    /// `horizontal` — resize только по физической ширине.
    Horizontal,
    /// `vertical` — resize только по физической высоте.
    Vertical,
    /// `block` — resize вдоль block-оси (логическая, зависит от `writing-mode`).
    Block,
    /// `inline` — resize вдоль inline-оси (логическая, зависит от `writing-mode`).
    Inline,
}

impl Resize {
    /// Разрешает логическую ось `resize` (`Block`/`Inline`) в физическую пару
    /// `(разрешена ширина, разрешена высота)` с учётом `writing-mode`.
    ///
    /// В `horizontal-tb` block-ось — вертикальная, inline-ось — горизонтальная;
    /// в вертикальных режимах (`vertical-rl`/`vertical-lr`/`sideways-rl`) — наоборот.
    /// Используется драг-хендлером grip-а (`crates/shell/src/main.rs`), чтобы
    /// вложенный корректно гейтить, какую из осей (`width`/`height`) двигать.
    pub fn allowed_axes(self, writing_mode: WritingMode) -> (bool, bool) {
        let vertical_wm = matches!(
            writing_mode,
            WritingMode::VerticalRl
                | WritingMode::VerticalLr
                | WritingMode::SidewaysRl
                | WritingMode::SidewaysLr
        );
        match self {
            Resize::None => (false, false),
            Resize::Both => (true, true),
            Resize::Horizontal => (true, false),
            Resize::Vertical => (false, true),
            Resize::Block => (vertical_wm, !vertical_wm),
            Resize::Inline => (!vertical_wm, vertical_wm),
        }
    }
}

/// CSS Rhythmic Sizing L1 §3.3 — `block-step-insert`. NOT inherited.
/// Initial: `margin-box`. BUG-517.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BlockStepInsert {
    /// `margin-box` — the step unit is inserted in the margin box.
    #[default]
    MarginBox,
    /// `padding-box` — the step unit is inserted in the padding box.
    PaddingBox,
    /// `content-box` — the step unit is inserted in the content box.
    ContentBox,
}

impl BlockStepInsert {
    /// Parses a single keyword token; `None` for anything else (including
    /// `border-box`, which this property's grammar does not accept).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "margin-box" => Some(Self::MarginBox),
            "padding-box" => Some(Self::PaddingBox),
            "content-box" => Some(Self::ContentBox),
            _ => None,
        }
    }

    /// Serializes back to its CSS keyword (specified value round-trip and
    /// computed-value serialization share the same text for this property).
    pub fn to_css(self) -> &'static str {
        match self {
            Self::MarginBox => "margin-box",
            Self::PaddingBox => "padding-box",
            Self::ContentBox => "content-box",
        }
    }
}

/// CSS Rhythmic Sizing L1 §3.4 — `block-step-align`. NOT inherited.
/// Initial: `auto`. BUG-517.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BlockStepAlign {
    /// `auto` — UA decides which side absorbs the extra step space.
    #[default]
    Auto,
    /// `center` — extra step space split evenly on both sides.
    Center,
    /// `start` — extra step space goes after the box (block-start edge fixed).
    Start,
    /// `end` — extra step space goes before the box (block-end edge fixed).
    End,
}

impl BlockStepAlign {
    /// Parses a single keyword token; `None` for anything else.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "auto" => Some(Self::Auto),
            "center" => Some(Self::Center),
            "start" => Some(Self::Start),
            "end" => Some(Self::End),
            _ => None,
        }
    }

    /// Serializes back to its CSS keyword.
    pub fn to_css(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Center => "center",
            Self::Start => "start",
            Self::End => "end",
        }
    }
}

/// CSS Rhythmic Sizing L1 §3.5 — `block-step-round`. NOT inherited.
/// Initial: `up`. BUG-517.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BlockStepRound {
    /// `up` — round the block size up to the next step multiple.
    #[default]
    Up,
    /// `down` — round the block size down to the previous step multiple.
    Down,
    /// `nearest` — round to whichever step multiple is closest.
    Nearest,
}

impl BlockStepRound {
    /// Parses a single keyword token; `None` for anything else.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "nearest" => Some(Self::Nearest),
            _ => None,
        }
    }

    /// Serializes back to its CSS keyword.
    pub fn to_css(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Nearest => "nearest",
        }
    }
}

/// CSS Gap Decorations L1 §3.2 — `column-rule-break` / `row-rule-break`.
/// NOT inherited. Initial: `normal`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RuleBreak {
    /// `none` — one continuous decoration from one end of the gap to the other.
    None,
    /// `normal` — container-dependent: grid breaks at "T" intersections only,
    /// flex behaves as `none`, multicol as `intersection` (columns) / `none` (rows).
    #[default]
    Normal,
    /// `intersection` — decorations start and end at every "T" and "cross".
    Intersection,
}

impl RuleBreak {
    /// Parses a single keyword token; `None` for anything else.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "normal" => Some(Self::Normal),
            "intersection" => Some(Self::Intersection),
            _ => None,
        }
    }

    /// Serializes back to its CSS keyword.
    pub fn to_css(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Normal => "normal",
            Self::Intersection => "intersection",
        }
    }
}

/// CSS Gap Decorations L1 §3.4 — `column-rule-visibility-items` /
/// `row-rule-visibility-items`. NOT inherited. Initial: `normal`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RuleVisibilityItems {
    /// `all` — paint in every gap segment, whether or not items are adjacent.
    All,
    /// `around` — paint when at least one of the two adjacent areas holds an item.
    Around,
    /// `between` — paint only when both adjacent areas hold items.
    Between,
    /// `normal` — container-dependent: grid `all`; multicol `between` (columns)
    /// / `all` (rows).
    #[default]
    Normal,
}

impl RuleVisibilityItems {
    /// Parses a single keyword token; `None` for anything else.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "all" => Some(Self::All),
            "around" => Some(Self::Around),
            "between" => Some(Self::Between),
            "normal" => Some(Self::Normal),
            _ => None,
        }
    }

    /// Serializes back to its CSS keyword.
    pub fn to_css(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Around => "around",
            Self::Between => "between",
            Self::Normal => "normal",
        }
    }
}

/// CSS Gap Decorations L1 §3.5 — `rule-overlap`: paint order of overlapping
/// row and column decorations. NOT inherited. Initial: `row-over-column`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RuleOverlap {
    /// `row-over-column` — row decorations are painted above column ones.
    #[default]
    RowOverColumn,
    /// `column-over-row` — column decorations are painted above row ones.
    ColumnOverRow,
}

impl RuleOverlap {
    /// Parses a single keyword token; `None` for anything else.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "row-over-column" => Some(Self::RowOverColumn),
            "column-over-row" => Some(Self::ColumnOverRow),
            _ => None,
        }
    }

    /// Serializes back to its CSS keyword.
    pub fn to_css(self) -> &'static str {
        match self {
            Self::RowOverColumn => "row-over-column",
            Self::ColumnOverRow => "column-over-row",
        }
    }
}

/// CSS Gap Decorations L1 §3.3 — `<inset-value>` = `<length-percentage> | overlap-join`:
/// смещение конца линии щели относительно её «естественного» края.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleInset {
    /// `<length-percentage>`; процент считается от ширины пересекающей щели
    /// (на краю контейнера она 0, так что процент там даёт 0).
    Length(Length),
    /// `overlap-join` — заходит в стык на полширины пересекающей щели плюс
    /// полширины её линии; на «колпачковом» конце (cap) равно 0.
    OverlapJoin,
}

impl Default for RuleInset {
    /// Initial value: `0`.
    fn default() -> Self {
        Self::Length(Length::Px(0.0))
    }
}

impl RuleInset {
    /// Разбирает один токен; `None` — для всего, что не `<length-percentage>`
    /// и не `overlap-join`.
    pub(in crate::style) fn parse(s: &str, is_quirks: bool) -> Option<Self> {
        let t = s.trim();
        if t.eq_ignore_ascii_case("overlap-join") {
            return Some(Self::OverlapJoin);
        }
        parse_length_q(t, is_quirks).map(Self::Length)
    }

    /// Сериализация computed-значения (as specified).
    pub fn to_css(&self) -> String {
        match self {
            Self::Length(l) => crate::selector_query::length_to_css(l),
            Self::OverlapJoin => "overlap-join".into(),
        }
    }
}

/// Восемь смещений концов линий одной оси: `{cap,junction}-{start,end}`
/// (`column-rule-inset-*` / `row-rule-inset-*`). Не наследуются.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RuleInsets {
    /// `*-rule-inset-cap-start`.
    pub cap_start: RuleInset,
    /// `*-rule-inset-cap-end`.
    pub cap_end: RuleInset,
    /// `*-rule-inset-junction-start`.
    pub junction_start: RuleInset,
    /// `*-rule-inset-junction-end`.
    pub junction_end: RuleInset,
}

impl RuleInsets {
    /// Слот по порядку `[cap-start, cap-end, junction-start, junction-end]`.
    pub fn slot_mut(&mut self, i: usize) -> &mut RuleInset {
        match i {
            0 => &mut self.cap_start,
            1 => &mut self.cap_end,
            2 => &mut self.junction_start,
            _ => &mut self.junction_end,
        }
    }

    /// Слот по порядку `[cap-start, cap-end, junction-start, junction-end]`.
    pub fn slot(&self, i: usize) -> &RuleInset {
        match i {
            0 => &self.cap_start,
            1 => &self.cap_end,
            2 => &self.junction_start,
            _ => &self.junction_end,
        }
    }
}

/// Грамматика свойства из семейства `*-rule-inset*` (CSS Gap Decorations L1 §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleInsetShape {
    /// Одно `<inset-value>`, записывается во все перечисленные слоты
    /// (`*-cap-start`: `[0]`; `*-start`: `[0, 2]`).
    Single(&'static [usize]),
    /// `<inset-value> <inset-value>?` в пару слотов `(start, end)`
    /// (`*-cap`, `*-junction`).
    Pair(usize, usize),
    /// `cap-start cap-end? [/ junction-start junction-end?]?` (`*-rule-inset`).
    Full,
}

/// Разобранное имя свойства `*-rule-inset*`: на какие оси оно действует и какая у него грамматика.
#[derive(Debug, Clone, Copy)]
pub(in crate::style) struct RuleInsetProp {
    /// Действует на `column_rule_inset`.
    pub cols: bool,
    /// Действует на `row_rule_inset`.
    pub rows: bool,
    shape: RuleInsetShape,
}

impl RuleInsetProp {
    /// `None`, если `prop` не из семейства `column-/row-/rule-inset*`.
    pub(in crate::style) fn of(prop: &str) -> Option<Self> {
        let (cols, rows, rest) = if let Some(r) = prop.strip_prefix("column-rule-inset") {
            (true, false, r)
        } else if let Some(r) = prop.strip_prefix("row-rule-inset") {
            (false, true, r)
        } else {
            (true, true, prop.strip_prefix("rule-inset")?)
        };
        // Слоты: 0 cap-start, 1 cap-end, 2 junction-start, 3 junction-end.
        let shape = match rest {
            "" => RuleInsetShape::Full,
            "-cap-start" => RuleInsetShape::Single(&[0]),
            "-cap-end" => RuleInsetShape::Single(&[1]),
            "-junction-start" => RuleInsetShape::Single(&[2]),
            "-junction-end" => RuleInsetShape::Single(&[3]),
            "-start" => RuleInsetShape::Single(&[0, 2]),
            "-end" => RuleInsetShape::Single(&[1, 3]),
            "-cap" => RuleInsetShape::Pair(0, 1),
            "-junction" => RuleInsetShape::Pair(2, 3),
            _ => return None,
        };
        Some(Self { cols, rows, shape })
    }

    /// Слоты `[cap-start, cap-end, junction-start, junction-end]`, которые свойство задаёт.
    pub(in crate::style) fn slots(&self) -> &'static [usize] {
        match self.shape {
            RuleInsetShape::Single(s) => s,
            RuleInsetShape::Pair(0, _) => &[0, 1],
            RuleInsetShape::Pair(..) => &[2, 3],
            RuleInsetShape::Full => &[0, 1, 2, 3],
        }
    }

    /// Разбирает значение в пары `(слот, значение)`; `None` — декларация невалидна.
    pub(in crate::style) fn parse(&self, val: &str, is_quirks: bool) -> Option<Vec<(usize, RuleInset)>> {
        let list = |s: &str| -> Option<Vec<RuleInset>> {
            let toks = split_top_level_ws(s.trim());
            if toks.is_empty() || toks.len() > 2 {
                return None;
            }
            toks.iter().map(|t| RuleInset::parse(t, is_quirks)).collect()
        };
        match self.shape {
            RuleInsetShape::Single(slots) => {
                let v = list(val).filter(|v| v.len() == 1)?.remove(0);
                Some(slots.iter().map(|&i| (i, v.clone())).collect())
            }
            RuleInsetShape::Pair(a, b) => {
                let v = list(val)?;
                let start = v[0].clone();
                let end = v.get(1).cloned().unwrap_or_else(|| start.clone());
                Some(vec![(a, start), (b, end)])
            }
            RuleInsetShape::Full => {
                let (cap, junction) = match split_top_level_slash(val) {
                    (c, Some(j)) => (list(c)?, Some(list(j)?)),
                    (c, None) => (list(c)?, None),
                };
                let cap_start = cap[0].clone();
                let cap_end = cap.get(1).cloned().unwrap_or_else(|| cap_start.clone());
                let (js, je) = match junction {
                    Some(j) => {
                        let s = j[0].clone();
                        let e = j.get(1).cloned().unwrap_or_else(|| s.clone());
                        (s, e)
                    }
                    None => (cap_start.clone(), cap_end.clone()),
                };
                Some(vec![(0, cap_start), (1, cap_end), (2, js), (3, je)])
            }
        }
    }
}

/// Делит `s` по первому `/` вне скобок (`calc(1px / 2)` не режется).
fn split_top_level_slash(s: &str) -> (&str, Option<&str>) {
    let mut depth = 0usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '/' if depth == 0 => return (&s[..i], Some(&s[i + 1..])),
            _ => {}
        }
    }
    (s, None)
}
