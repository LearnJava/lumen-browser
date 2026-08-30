//! Перенос текста (`text-wrap-mode`/`-style`), flex
//! (`flex-direction`/`-wrap`/`-basis`), CSS Grid треки
//! (`GridTrackSize`/`GridRepeat`/`repeat()`-разбор/`grid-auto-flow`/
//! `masonry-auto-flow`/`GridLine`), позиционирование и выравнивание
//! (`PositionComponent`/`ObjectPosition`/`AlignValue`).
//!
//! Перенесено батчем SPLIT-ST17 из `crates/engine/layout/src/style.rs`
//! (анкер `enum TextWrapMode` до конца `impl AlignValue`) без правок тел.

use lumen_core::geom::Size;

use crate::style::parse::counters::is_css_ident;
use crate::style::values::length::{parse_length, parse_length_q, Length};

/// CSS Text Module Level 4 §6.4.1 — `text-wrap-mode`. Inherited.
///
/// Управляет тем, переносятся ли строки внутри блока. `wrap` — нормальный
/// перенос по soft wrap opportunities (initial). `nowrap` — текст растягивается
/// в одну линию, до явного break-control (`<br>`, preserved newline).
///
/// Является non-shorthand-частью `text-wrap` (§6.4.3) и одновременно
/// частью legacy `white-space` shorthand (§2.1 — `white-space-collapse` ||
/// `text-wrap-mode` || `white-space-trim`). В этой кодовой базе `white-space`
/// исторически хранится отдельным [`WhiteSpace`] enum-ом — связка двух полей
/// уйдёт в типизацию декрараций (P1 1B).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextWrapMode {
    /// `wrap` (initial) — обычный перенос строк.
    #[default]
    Wrap,
    /// `nowrap` — без переноса, текст в одну линию.
    Nowrap,
}

impl TextWrapMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "wrap" => Some(Self::Wrap),
            "nowrap" => Some(Self::Nowrap),
            _ => None,
        }
    }
}

/// CSS Text Module Level 4 §6.4.2 — `text-wrap-style`. Inherited.
///
/// Расширенные стратегии перевода строк. `auto` — UA выбирает по умолчанию
/// (обычно greedy first-fit). Остальные значения — типографические
/// улучшения, требующие реального line-breaker-а (Knuth–Plass / Latin
/// last-line orphan-prevention) — Phase 0 хранит как atom, применение
/// откладывается до интеграции с `UnicodeProvider` (provisional `icu4x`,
/// P1 п.5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextWrapStyle {
    /// `auto` (initial) — UA-default стратегия (обычно greedy).
    #[default]
    Auto,
    /// `balance` — балансировать длины строк короткого блока (≤ ~10 строк).
    Balance,
    /// `stable` — стабильные break-points при редактировании (для contenteditable).
    Stable,
    /// `pretty` — улучшенный last-line (без orphan / висячих слов).
    Pretty,
}

impl TextWrapStyle {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "balance" => Some(Self::Balance),
            "stable" => Some(Self::Stable),
            "pretty" => Some(Self::Pretty),
            _ => None,
        }
    }
}

/// CSS Flexbox L1 §5.1 — `flex-direction`. Non-inherited.
///
/// Задаёт направление главной оси flex-контейнера. Phase 0: parsing + storage;
/// реальный flex-layout pass — задача 4B.3.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FlexDirection {
    /// `row` (initial) — горизонтально, слева направо.
    #[default]
    Row,
    /// `row-reverse` — горизонтально, справа налево.
    RowReverse,
    /// `column` — вертикально, сверху вниз.
    Column,
    /// `column-reverse` — вертикально, снизу вверх.
    ColumnReverse,
}

impl FlexDirection {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "row" => Some(Self::Row),
            "row-reverse" => Some(Self::RowReverse),
            "column" => Some(Self::Column),
            "column-reverse" => Some(Self::ColumnReverse),
            _ => None,
        }
    }
}

/// CSS Flexbox L1 §5.2 — `flex-wrap`. Non-inherited.
///
/// Разрешает или запрещает перенос flex-элементов на новые строки/столбцы.
/// Phase 0: parsing + storage; реальный multi-line flex — задача 4B.5.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FlexWrap {
    /// `nowrap` (initial) — все элементы в одну строку.
    #[default]
    Nowrap,
    /// `wrap` — перенос вперёд (вниз или вправо).
    Wrap,
    /// `wrap-reverse` — перенос назад.
    WrapReverse,
}

impl FlexWrap {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "nowrap" => Some(Self::Nowrap),
            "wrap" => Some(Self::Wrap),
            "wrap-reverse" => Some(Self::WrapReverse),
            _ => None,
        }
    }
}

/// CSS Flexbox L1 §7.3 — `flex-basis`. Non-inherited.
///
/// Размер flex-элемента вдоль главной оси до применения grow/shrink.
/// Phase 0: parsing + storage; реальный flex-layout — задача 4B.3.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum FlexBasis {
    /// `auto` (initial) — использовать width/height элемента.
    #[default]
    Auto,
    /// `content` — intrinsic content-size (CSS Flexbox L1 §7.3.2).
    Content,
    /// Explicit length/percentage.
    Length(Length),
}

impl FlexBasis {
    pub fn parse(s: &str, is_quirks: bool) -> Option<Self> {
        let trimmed = s.trim();
        match trimmed.to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "content" => Some(Self::Content),
            _ => parse_length_q(trimmed, is_quirks).map(Self::Length),
        }
    }
}

/// CSS Grid Layout L3 §9 — `repeat(auto-fill | auto-fit | <count>, <track-list>)`.
/// Stored in grid_template_columns/rows during Phase 0 to preserve repeat information
/// until resolution time (lay_out_grid). Expanded via `resolve_grid_template` before layout.
#[derive(Debug, Clone, PartialEq)]
pub struct GridRepeat {
    /// `Count::Fixed(N)` for `repeat(N, ...)`, `AutoFill` for auto-fill, `AutoFit` for auto-fit.
    pub count: RepeatCount,
    /// The track sizing functions inside the parentheses, e.g. `minmax(100px, 1fr)`.
    pub tracks: Vec<GridTrackSize>,
}

/// Count type for grid-template-columns/rows `repeat()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RepeatCount {
    /// Fixed count: `repeat(3, ...)`.
    Fixed(usize),
    /// Auto-fill: `repeat(auto-fill, ...)` — fill available space, prefer empty tracks over overflow.
    AutoFill,
    /// Auto-fit: `repeat(auto-fit, ...)` — fill available space, collapse empty tracks.
    AutoFit,
}

/// CSS Grid Layout L1 §7.2 — sizing function for a grid track.
/// Non-inherited. Appears in `grid-template-columns` / `grid-template-rows`
/// and `grid-auto-columns` / `grid-auto-rows`.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum GridTrackSize {
    /// `auto` — sized by content (min-content as min, max-content as max).
    #[default]
    Auto,
    /// Fixed length (px, em, rem, %).
    Length(Length),
    /// `<number>fr` — fractional unit of remaining free space.
    Fr(f32),
    /// `min-content` — minimum content size.
    MinContent,
    /// `max-content` — maximum content size.
    MaxContent,
    /// `minmax(min, max)` — track between min and max sizing functions.
    Minmax(Box<GridTrackSize>, Box<GridTrackSize>),
    /// `fit-content(N)` — track sized to fit content with max limit (CSS Grid L3 §9.1).
    /// Equivalent to `minmax(auto, max(auto, min(N, max-content)))`.
    FitContent(Box<GridTrackSize>),
    /// `subgrid` — inherit track sizes from the spanning tracks of the parent grid
    /// (CSS Grid Layout L2 §9). The grid item must itself be a grid container;
    /// its column/row tracks are replaced by the parent's resolved track sizes
    /// for the cells it spans. Stored as a sentinel `vec![GridTrackSize::Subgrid]`
    /// in `grid_template_columns` or `grid_template_rows`.
    Subgrid,
    /// `masonry` — CSS Grid L3 §14 waterfall layout axis sentinel.
    /// Stored as `vec![GridTrackSize::Masonry]` in `grid_template_columns` or
    /// `grid_template_rows` to signal that the axis uses masonry placement.
    /// The perpendicular axis defines track sizes; `masonry.rs` handles placement.
    /// P4 handoff: `masonry-auto-flow`, `align-tracks`, `justify-tracks` in ComputedStyle.
    Masonry,
}

impl GridTrackSize {
    /// Resolve to a concrete pixel size given container width, em, viewport.
    /// For `fr`, `auto`, `fit-content`, `subgrid`, and `masonry` returns `None` — caller handles those specially.
    pub fn resolve_fixed(&self, em: f32, cb: f32, viewport: Size) -> Option<f32> {
        match self {
            Self::Length(l) => l.resolve(em, Some(cb), viewport),
            Self::Fr(_) | Self::Auto | Self::MinContent | Self::MaxContent | Self::FitContent(_) | Self::Subgrid | Self::Masonry => None,
            Self::Minmax(min, _max) => min.resolve_fixed(em, cb, viewport),
        }
    }

    /// True for fractional tracks.
    pub fn is_fr(&self) -> bool {
        matches!(self, Self::Fr(_))
    }

    /// Extract fr value.
    pub fn fr(&self) -> Option<f32> {
        if let Self::Fr(v) = self { Some(*v) } else { None }
    }

    /// True when this track inherits its size from the parent grid (subgrid axis).
    pub fn is_subgrid(&self) -> bool {
        matches!(self, Self::Subgrid)
    }

    /// True when this axis uses masonry placement (CSS Grid L3 §14).
    pub fn is_masonry(&self) -> bool {
        matches!(self, Self::Masonry)
    }

    /// Parse a single track sizing keyword / value (no `repeat()`).
    pub(in crate::style) fn parse_single(s: &str, is_quirks: bool) -> Option<Self> {
        let lc = s.trim().to_ascii_lowercase();
        match lc.as_str() {
            "auto" => return Some(Self::Auto),
            "min-content" => return Some(Self::MinContent),
            "max-content" => return Some(Self::MaxContent),
            // `subgrid` / `masonry` as single tokens are handled in parse_track_list;
            // reaching here means they appeared inside a repeat() context — treat as auto.
            "subgrid" | "masonry" => return Some(Self::Auto),
            _ => {}
        }
        // `<number>fr`
        if let Some(n) = lc.strip_suffix("fr")
            && let Ok(v) = n.trim().parse::<f32>()
        {
            return Some(Self::Fr(v.max(0.0)));
        }
        // `minmax(min, max)`
        if lc.starts_with("minmax(") && lc.ends_with(')') {
            let inner = &s.trim()[7..s.trim().len() - 1];
            if let Some((a, b)) = split_paren_aware_comma(inner) {
                let min = Self::parse_single(a.trim(), is_quirks)?;
                let max = Self::parse_single(b.trim(), is_quirks)?;
                return Some(Self::Minmax(Box::new(min), Box::new(max)));
            }
        }
        // `fit-content(<length-percentage>)` (CSS Grid L3 §9.1)
        if lc.starts_with("fit-content(") && lc.ends_with(')') {
            let inner = &s.trim()[12..s.trim().len() - 1];
            if let Some(limit) = Self::parse_single(inner.trim(), is_quirks) {
                return Some(Self::FitContent(Box::new(limit)));
            }
        }
        // length / percentage
        parse_length_q(s.trim(), is_quirks).map(Self::Length)
    }

    /// Parse a track-list value string into a Vec of GridTrackSize.
    /// Handles `repeat(N, <track-list>)` by expanding.
    /// `subgrid` as the entire value returns `vec![Subgrid]` (sentinel for the whole axis).
    /// `masonry` as the entire value returns `vec![Masonry]` (CSS Grid L3 §14 sentinel).
    pub fn parse_track_list(s: &str, is_quirks: bool) -> Vec<Self> {
        let trimmed = s.trim();
        // CSS Grid L2 §9: `subgrid` replaces the entire track list for that axis.
        if trimmed.eq_ignore_ascii_case("subgrid") {
            return vec![Self::Subgrid];
        }
        // CSS Grid L3 §14: `masonry` replaces the entire track list — waterfall placement axis.
        if trimmed.eq_ignore_ascii_case("masonry") {
            return vec![Self::Masonry];
        }
        let mut result = Vec::new();
        for token in split_track_list_tokens(trimmed) {
            let t = token.trim();
            let lc = t.to_ascii_lowercase();
            if lc.starts_with("repeat(") && lc.ends_with(')') {
                let inner = &t[7..t.len() - 1];
                if let Some((count_s, rest)) = split_paren_aware_comma(inner) {
                    let count_s_trim = count_s.trim();
                    let count_lc = count_s_trim.to_ascii_lowercase();
                    let count = if count_lc == "auto-fill" {
                        RepeatCount::AutoFill
                    } else if count_lc == "auto-fit" {
                        RepeatCount::AutoFit
                    } else if let Ok(n) = count_s_trim.parse::<usize>() {
                        RepeatCount::Fixed(n)
                    } else {
                        continue; // Invalid repeat count, skip
                    };

                    let tracks = Self::parse_track_list(rest.trim(), is_quirks);
                    if count == RepeatCount::Fixed(0) {
                        // zero repeat, add nothing
                    } else if matches!(count, RepeatCount::Fixed(_)) {
                        // Expand fixed repeat immediately
                        let n = match count {
                            RepeatCount::Fixed(n) => n,
                            _ => unreachable!(),
                        };
                        for _ in 0..n {
                            result.extend(tracks.iter().cloned());
                        }
                    } else {
                        // For auto-fill / auto-fit, store GridRepeat sentinel for resolution at layout time
                        // Phase 1: Add first track as GridRepeat sentinel. Caller (lay_out_grid) resolves count.
                        // For now, expand as single "repeat" marker that resolver can recognize and expand.
                        if !tracks.is_empty() {
                            // Store info in a way resolver can find: mark with a sentinel or new enum variant.
                            // Currently: add the first track once, and store GridRepeat in ComputedStyle separately.
                            // Phase 1 simplified: treat as auto (no expansion) until resolver wire-up
                            result.extend(tracks.iter().cloned());
                        }
                    }
                }
            } else if let Some(ts) = Self::parse_single(t, is_quirks) {
                result.push(ts);
            }
        }
        result
    }
}

/// Extracts auto-fill/auto-fit repeat metadata from a track-list string.
/// Returns `Some(GridRepeat)` when the string is exactly `repeat(auto-fill|auto-fit, ...)`.
/// Used in Phase 2 of CSS Grid auto-repeat expansion (CSS Grid L1 §7.2.3.4).
pub(crate) fn parse_auto_repeat(s: &str) -> Option<GridRepeat> {
    let trimmed = s.trim();
    // Must start with "repeat(" (case-insensitive) and end with ")"
    let lc = trimmed.to_ascii_lowercase();
    let inner = lc.strip_prefix("repeat(")?.strip_suffix(')')?;
    let (count_s, rest) = split_paren_aware_comma(inner)?;
    let count = match count_s.trim() {
        "auto-fill" => RepeatCount::AutoFill,
        "auto-fit" => RepeatCount::AutoFit,
        _ => return None,
    };
    // Re-parse from original string to preserve case in track sizes
    let orig_inner = trimmed
        .get("repeat(".len()..trimmed.len() - 1)?;
    let (_, orig_rest) = split_paren_aware_comma(orig_inner)?;
    let tracks = GridTrackSize::parse_track_list(orig_rest.trim(), false);
    if tracks.is_empty() {
        return None;
    }
    let _ = rest; // suppress unused warning from lc version
    Some(GridRepeat { count, tracks })
}

/// Split a comma inside a track-list token that may contain nested parens.
fn split_paren_aware_comma(s: &str) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => return Some((&s[..i], &s[i + 1..])),
            _ => {}
        }
    }
    None
}

/// Tokenize a track-list string into individual track tokens,
/// respecting parentheses (so `minmax(...)` stays as one token).
fn split_track_list_tokens(s: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b' ' | b'\t' | b'\n' if depth == 0 => {
                let tok = s[start..i].trim();
                if !tok.is_empty() {
                    tokens.push(tok);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = s[start..].trim();
    if !last.is_empty() {
        tokens.push(last);
    }
    tokens
}

/// CSS Grid Layout L1 §8.5 — `grid-auto-flow`. Non-inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GridAutoFlow {
    /// `row` (initial) — fill rows, add new rows as needed.
    #[default]
    Row,
    /// `column` — fill columns, add new columns as needed.
    Column,
    /// `row dense` — row flow with dense packing.
    RowDense,
    /// `column dense` — column flow with dense packing.
    ColumnDense,
}

impl GridAutoFlow {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "row" => Some(Self::Row),
            "column" => Some(Self::Column),
            "row dense" | "dense row" => Some(Self::RowDense),
            "column dense" | "dense column" => Some(Self::ColumnDense),
            _ => None,
        }
    }
}

/// CSS Masonry Layout §9 — `masonry-auto-flow`. Controls the placement order
/// of auto-placed items in a masonry container. Non-inherited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum MasonryAutoFlow {
    /// `definite-first` (initial) — items with an explicit grid-axis position are
    /// placed first, then auto items in source order.
    #[default]
    DefiniteFirst,
    /// `next` — all items placed in source order, no definite-first prioritisation.
    Next,
    /// `ordered` — items sorted by their CSS `order` property before placement.
    Ordered,
}

impl MasonryAutoFlow {
    /// Parse a CSS `masonry-auto-flow` value string.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "definite-first" => Some(Self::DefiniteFirst),
            "next" => Some(Self::Next),
            "ordered" => Some(Self::Ordered),
            _ => None,
        }
    }
}

/// CSS Grid Layout L1 §8.3 — a grid-line reference for grid-column-start,
/// grid-column-end, grid-row-start, grid-row-end. Non-inherited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum GridLine {
    /// `auto` — automatic placement.
    #[default]
    Auto,
    /// Integer line number (1-based from start, negative from end).
    Line(i32),
    /// `span <integer>` — span N tracks.
    Span(u32),
    /// Named grid area reference (CSS Grid L1 §8.3). Resolved at layout time
    /// by looking up the name in the containing grid's `grid-template-areas`.
    Named(String),
}

impl GridLine {
    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.eq_ignore_ascii_case("auto") {
            return Some(Self::Auto);
        }
        // `span N` or `span`
        if let Some(rest) = trimmed.to_ascii_lowercase().strip_prefix("span") {
            let rest = rest.trim();
            if rest.is_empty() {
                return Some(Self::Span(1));
            }
            if let Ok(n) = rest.parse::<u32>() {
                return Some(Self::Span(n.max(1)));
            }
        }
        // integer line number
        if let Ok(n) = trimmed.parse::<i32>() && n != 0 {
            return Some(Self::Line(n));
        }
        // CSS custom-ident: named grid area or named line.
        // Only accept valid CSS idents (letters, digits, hyphens, underscores;
        // cannot start with a digit or two hyphens without a letter).
        if is_css_ident(trimmed) {
            return Some(Self::Named(trimmed.to_string()));
        }
        None
    }
}

/// Одна компонента `object-position`. Length-варианты резолвятся в px
/// относительно края коробки (positive = от left/top); percentage —
/// относительно **свободного места** `box_size - content_size` (может быть
/// отрицательным, тогда излишек уходит за противоположный край). См.
/// CSS Images L3 §5.5 «object-position».
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PositionComponent {
    /// Length в px (после resolve em/rem/vw/...).
    Px(f32),
    /// Percentage в долях 1.0 (`50%` → 0.5). Резолвится на paint-стадии
    /// против свободного места: `offset = free_space * percent`.
    Percent(f32),
}

impl PositionComponent {
    /// Резолв в финальный px-offset относительно левого/верхнего края
    /// коробки. `free_space = box_size - content_size`; может быть
    /// отрицательным (content > box) — тогда offset тоже отрицательный,
    /// и излишек уезжает за противоположный край.
    pub fn resolve(self, free_space: f32) -> f32 {
        match self {
            Self::Px(px) => px,
            Self::Percent(p) => free_space * p,
        }
    }
}

/// CSS Images L3 §5.5 — `object-position` (две компоненты, x + y).
/// Default — `50% 50%` (центр).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectPosition {
    pub x: PositionComponent,
    pub y: PositionComponent,
}

impl Default for ObjectPosition {
    fn default() -> Self {
        Self {
            x: PositionComponent::Percent(0.5),
            y: PositionComponent::Percent(0.5),
        }
    }
}

impl ObjectPosition {
    /// CSS Backgrounds L3 §3.5 — initial value `background-position: 0% 0%`
    /// (top-left). Отличается от Object Position default (`50% 50%`, центр)
    /// специально потому, что `background-image` обычно anchored к top-left
    /// при первой укладке (см. CSS 2.1 §14.2.1).
    pub const fn background_initial() -> Self {
        Self {
            x: PositionComponent::Percent(0.0),
            y: PositionComponent::Percent(0.0),
        }
    }
}

impl ObjectPosition {
    /// CSS Values L4 §9.4 — `<position>` для object-position. Phase 0
    /// поддерживает:
    ///   - keyword `center` (= 50%),
    ///   - axis-keywords `left|right|top|bottom`,
    ///   - один token (`50%`, `10px`, keyword) — второй = `center`,
    ///   - два token-а — первый x, второй y.
    ///
    /// Tri- и quad-форма (`<keyword> <length> <keyword> <length>` для
    /// сторон-якорей) — отложены: на современных страницах редкость.
    pub fn parse(s: &str, em_basis: f32, viewport: Size) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.is_empty() || tokens.len() > 2 {
            return None;
        }
        // Single-token: применяется к horizontal оси; вертикальная = center.
        // Если token — vertical keyword (`top`/`bottom`), то horizontal = center.
        if tokens.len() == 1 {
            let t = tokens[0];
            if t.eq_ignore_ascii_case("top") {
                return Some(Self {
                    x: PositionComponent::Percent(0.5),
                    y: PositionComponent::Percent(0.0),
                });
            }
            if t.eq_ignore_ascii_case("bottom") {
                return Some(Self {
                    x: PositionComponent::Percent(0.5),
                    y: PositionComponent::Percent(1.0),
                });
            }
            let x = parse_position_component(t, em_basis, viewport, /*vertical*/ false)?;
            return Some(Self {
                x,
                y: PositionComponent::Percent(0.5),
            });
        }
        // Two-token form: <x> <y>. Swap, если порядок инвертирован
        // (`top left` ≡ `left top`).
        let (t0, t1) = (tokens[0], tokens[1]);
        let (xtok, ytok) = if is_vertical_keyword(t0) || is_horizontal_keyword(t1) {
            (t1, t0)
        } else {
            (t0, t1)
        };
        let x = parse_position_component(xtok, em_basis, viewport, false)?;
        let y = parse_position_component(ytok, em_basis, viewport, true)?;
        Some(Self { x, y })
    }
}

fn is_vertical_keyword(t: &str) -> bool {
    t.eq_ignore_ascii_case("top") || t.eq_ignore_ascii_case("bottom")
}

fn is_horizontal_keyword(t: &str) -> bool {
    t.eq_ignore_ascii_case("left") || t.eq_ignore_ascii_case("right")
}

pub(in crate::style) fn parse_position_component(
    t: &str,
    em_basis: f32,
    viewport: Size,
    vertical: bool,
) -> Option<PositionComponent> {
    // Keyword-формы.
    if t.eq_ignore_ascii_case("center") {
        return Some(PositionComponent::Percent(0.5));
    }
    if !vertical {
        if t.eq_ignore_ascii_case("left") {
            return Some(PositionComponent::Percent(0.0));
        }
        if t.eq_ignore_ascii_case("right") {
            return Some(PositionComponent::Percent(1.0));
        }
        // top/bottom в horizontal-позиции — недопустимо.
        if is_vertical_keyword(t) {
            return None;
        }
    } else {
        if t.eq_ignore_ascii_case("top") {
            return Some(PositionComponent::Percent(0.0));
        }
        if t.eq_ignore_ascii_case("bottom") {
            return Some(PositionComponent::Percent(1.0));
        }
        if is_horizontal_keyword(t) {
            return None;
        }
    }
    // Length / percentage. Percent-форма `50%` сохраняется как доля 0..=1
    // (без clamp — отрицательные и >100% валидны по спеке и используются
    // художниками для художественных смещений).
    if let Some(pct) = t.strip_suffix('%')
        && let Ok(n) = pct.trim().parse::<f32>()
    {
        return Some(PositionComponent::Percent(n / 100.0));
    }
    let len = parse_length(t)?;
    let px = len.resolve(em_basis, None, viewport)?;
    Some(PositionComponent::Px(px))
}

/// CSS Box Alignment L3 §6.1 — значения для align-/justify- свойств.
/// Phase 0: основной набор keyword-ов. `Auto` — default (resolve в
/// `Normal` или specific behavior контекстом).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AlignValue {
    /// CSS keyword `auto` — default. Behavior зависит от контекста
    /// (parent layout type). Для absolute-positioned — `normal`.
    #[default]
    Auto,
    /// `normal` — default-behavior для conteneur'а (stretch for grid,
    /// start for flex).
    Normal,
    /// `stretch` — растянуть на доступное место (default для grid).
    Stretch,
    /// `start` / `flex-start` — выровнять к началу cross/main axis.
    Start,
    /// `end` / `flex-end` — выровнять к концу.
    End,
    /// `center` — выровнять по центру.
    Center,
    /// `baseline` — выровнять text-baseline (для align-items).
    Baseline,
    /// `space-between` — равные промежутки между items, по краям нет.
    SpaceBetween,
    /// `space-around` — промежутки между + половинные по краям.
    SpaceAround,
    /// `space-evenly` — все промежутки одинаковые, включая края.
    SpaceEvenly,
}

impl AlignValue {
    pub fn parse(s: &str) -> Option<Self> {
        let lc = s.trim().to_ascii_lowercase();
        match lc.as_str() {
            "auto" => Some(Self::Auto),
            "normal" => Some(Self::Normal),
            "stretch" => Some(Self::Stretch),
            "start" | "flex-start" | "self-start" => Some(Self::Start),
            "end" | "flex-end" | "self-end" => Some(Self::End),
            "center" => Some(Self::Center),
            "baseline" | "first baseline" | "last baseline" => Some(Self::Baseline),
            "space-between" => Some(Self::SpaceBetween),
            "space-around" => Some(Self::SpaceAround),
            "space-evenly" => Some(Self::SpaceEvenly),
            _ => None,
        }
    }
}

