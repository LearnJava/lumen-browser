//! Типы значений CSS: содержимое (`content`), маркеры списков
//! (`list-style-*`), перенос текста (`overflow-wrap`/`line-break`/
//! `word-break`/`hyphens`), полосы прокрутки (`scrollbar-width`/
//! `scrollbar-gutter`), интерактивность (`touch-action`/`appearance`/
//! `field-sizing`/`pointer-events`/`resize`).
//!
//! Перенесено батчем SPLIT-ST17 из `crates/engine/layout/src/style.rs`
//! (анкер `enum Content` до конца `impl Resize`) без правок тел.

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
        // `stable both-edges` — двухтокеновая форма.
        let tokens: Vec<&str> = lc.split_whitespace().collect();
        if tokens == ["stable", "both-edges"] {
            return Some(Self::StableBothEdges);
        }
        None
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
}

impl WordBreak {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "keep-all" => Some(Self::KeepAll),
            "break-all" => Some(Self::BreakAll),
            "break-word" => Some(Self::BreakWord),
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

