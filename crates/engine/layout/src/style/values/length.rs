//! CSS-длины: типы `Length`/`LengthOrAuto`, их резолв в пиксели и разбор
//! `<length>`-значений, включая quirks-режим, где голое число значит `px`.
//!
//! Перенесено батчем SPLIT-ST9 из `crates/engine/layout/src/style.rs`
//! (анкер `enum LengthOrAuto`) без правок тел: изменена только видимость тех
//! items, которые продолжают звать `style.rs` и его потомки.

// Долг по документации переезжает вместе с кодом (§2 очереди SPLIT, правило 3):
// варианты `pub enum Length`/`LengthOrAuto` написаны до включения `missing_docs`.
// Область исключения — файл. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use lumen_core::geom::Size;

use crate::style::calc::{looks_like_function_call, parse_math_function_value};
use crate::style::{CalcNode, CONTAINER_CQ, FONT_CH_EX, ROOT_FONT_SIZE};

/// CSS `<length> | auto` — для margin и offset-свойств, где `auto` имеет
/// отдельную семантику (centering). Typed; `%` резолвится при layout с
/// known containing block. Initial value margin = `Length(Px(0.0))`, не `Auto`.
#[derive(Debug, Clone, PartialEq)]
pub enum LengthOrAuto {
    Auto,
    Length(Length),
}

impl LengthOrAuto {
    pub const ZERO: LengthOrAuto = LengthOrAuto::Length(Length::Px(0.0));

    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }

    /// Returns the raw pixel value for `Length::Px` variants; `Auto` and all
    /// other length units (em, %, rem, …) return `None`.
    /// Used by the paint layer where the layout context is unavailable.
    pub fn to_px_opt(&self) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Length(Length::Px(px)) => Some(*px),
            Self::Length(_) => None,
        }
    }

    /// Резолвит в пиксели. `Auto` → `None`; нерезолвируемый `%` → `None`.
    /// `em` = font_size элемента, `cb_width` = containing-block width.
    pub fn resolve(&self, em: f32, cb_width: f32, vp: Size) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Length(l) => l.resolve(em, Some(cb_width), vp),
        }
    }

    /// Резолвит в пиксели; для `Auto` и нерезолвируемых значений → 0.0.
    pub fn resolve_or_zero(&self, em: f32, cb_width: f32, vp: Size) -> f32 {
        self.resolve(em, cb_width, vp).unwrap_or(0.0)
    }
}

/// Типизированная длина CSS до резолва в пиксели.
///
/// Не `Copy`, потому что вариант `Calc` хранит `Box<CalcNode>` с поддеревом
/// выражения. Использования полагались только на `Clone` / match-pattern-ы,
/// где `v` копируется как `f32`, а не `len` как `Length`.
#[derive(Debug, Clone, PartialEq)]
pub enum Length {
    Px(f32),
    /// `em` — относительно font-size текущего/родительского элемента
    /// (для свойства `font-size` — родительского, для остального — текущего).
    Em(f32),
    /// `rem` — относительно font-size корня документа (ROOT_FONT_SIZE).
    Rem(f32),
    /// CSS Values L4 §5.1.1 — `ch`: advance measure (width) of the "0" (U+0030)
    /// glyph in the element's own font. Resolved to px against the font-metric
    /// thread-local `FONT_CH_EX` (set per box by `lay_out_inner` from the active
    /// `TextMeasurer`). When that metric is unavailable (outside a layout pass, or
    /// no measurer), spec §5.1.1 says to assume the "0" glyph is `0.5em` wide.
    Ch(f32),
    /// CSS Values L4 §5.1.1 — `ex`: the used x-height of the element's own font.
    /// Resolved via the same `FONT_CH_EX` thread-local; the spec fallback when the
    /// metric is unavailable is `0.5em` (the assumed x-height).
    Ex(f32),
    /// `%` — процент. Базис зависит от свойства: для `font-size` это
    /// `em_basis`, для `line-height` — текущий font-size, для
    /// margin/padding/width — containing block width (Phase 0 пока не считает,
    /// нужны honest contain blocks; до тех пор `%` в margin/padding
    /// игнорируется).
    Percent(f32),
    /// `vh` — 1% от высоты viewport (CSS Values L3 §6.1.2).
    Vh(f32),
    /// `vw` — 1% от ширины viewport.
    Vw(f32),
    /// `vmin` — 1% от меньшей из двух сторон viewport.
    Vmin(f32),
    /// `vmax` — 1% от большей из двух сторон viewport.
    Vmax(f32),
    /// CSS Container Queries L1 §6.2 — `cqw`: 1% of the nearest container's inline size (width
    /// in horizontal writing mode). Resolves via thread-local `CONTAINER_CQ`; returns `None`
    /// outside a container re-layout pass.
    Cqw(f32),
    /// `cqh`: 1% of the nearest container's block size (height in horizontal writing mode).
    /// Returns `None` when the container is `inline-size` type (block axis not queryable).
    Cqh(f32),
    /// `cqi`: 1% of the nearest container's inline size. Alias for `cqw` in horizontal writing
    /// mode; writing-mode-aware in vertical contexts (Phase 0: treated as cqw).
    Cqi(f32),
    /// `cqb`: 1% of the nearest container's block size. Alias for `cqh` in horizontal writing
    /// mode; writing-mode-aware (Phase 0: treated as cqh).
    Cqb(f32),
    /// `cqmin`: 1% of the smaller of `cqi` and `cqb`.
    Cqmin(f32),
    /// `cqmax`: 1% of the larger of `cqi` and `cqb`.
    Cqmax(f32),
    /// CSS Values L4 §10 — `calc()` выражение. Резолвится через
    /// `CalcNode::resolve`, который рекурсивно вычисляет поддерево
    /// в `f32`-пикселях, используя те же `em_basis` / `percent_basis` /
    /// `viewport`, что и обычный `Length`.
    Calc(Box<CalcNode>),
    /// CSS Intrinsic Sizing L3 §4 — `min-content` keyword.
    /// Minimum content width: narrowest the box can be without overflowing
    /// its content (longest word / longest unbreakable run). Needs layout
    /// context to resolve; `resolve()` returns `None`.
    MinContent,
    /// CSS Intrinsic Sizing L3 §4 — `max-content` keyword.
    /// Maximum content width: as wide as the content prefers with no forced
    /// line breaks. Needs layout context to resolve; `resolve()` returns `None`.
    MaxContent,
    /// CSS Intrinsic Sizing L3 §4 — `fit-content` / `fit-content(<length>)`.
    /// Bare `fit-content` = `min(available, max-content)`. With argument =
    /// `min(available, max(min-content, arg))`. Needs layout context.
    FitContent(Option<Box<Length>>),
}

impl Length {
    /// Возвращает длину в пикселях. `em_basis` — fs, относительно которого
    /// считать `em` (родителя для font-size; текущего элемента для остального).
    /// `percent_basis` — длина, относительно которой считать `%` (None если
    /// контекст ещё не определён — тогда `%` даёт None).
    /// `viewport` — размер viewport-а для `vh`/`vw`/`vmin`/`vmax`.
    pub fn resolve(&self, em_basis: f32, percent_basis: Option<f32>, viewport: Size) -> Option<f32> {
        match self {
            Length::Px(v) => Some(*v),
            Length::Em(v) => Some(*v * em_basis),
            Length::Rem(v) => Some(*v * ROOT_FONT_SIZE),
            // CSS Values L4 §5.1.1 — `ch`/`ex` against the box's own font metrics
            // (thread-local `FONT_CH_EX`, absolute px per unit). Outside a layout
            // pass the metric is unavailable → spec fallback of `0.5em`.
            Length::Ch(v) => {
                Some(FONT_CH_EX.with(|c| c.get()).map_or(*v * 0.5 * em_basis, |(ch, _)| *v * ch))
            }
            Length::Ex(v) => {
                Some(FONT_CH_EX.with(|c| c.get()).map_or(*v * 0.5 * em_basis, |(_, ex)| *v * ex))
            }
            Length::Percent(v) => percent_basis.map(|b| *v / 100.0 * b),
            Length::Vh(v) => Some(*v / 100.0 * viewport.height),
            Length::Vw(v) => Some(*v / 100.0 * viewport.width),
            Length::Vmin(v) => Some(*v / 100.0 * viewport.width.min(viewport.height)),
            Length::Vmax(v) => Some(*v / 100.0 * viewport.width.max(viewport.height)),
            // CSS Container Queries L1 §6.2 — resolved against the nearest container's
            // dimensions, available via thread-local CONTAINER_CQ set before re-layout.
            Length::Cqw(v) | Length::Cqi(v) => {
                CONTAINER_CQ.with(|c| c.get()).map(|(w, _h)| *v / 100.0 * w)
            }
            Length::Cqh(v) | Length::Cqb(v) => {
                // Block size is 0.0 when the container is `inline-size` type (not queryable).
                CONTAINER_CQ.with(|c| c.get()).and_then(|(_w, h)| {
                    if h > 0.0 { Some(*v / 100.0 * h) } else { None }
                })
            }
            Length::Cqmin(v) => {
                CONTAINER_CQ.with(|c| c.get()).and_then(|(w, h)| {
                    // When block size is 0 (inline-size container), block axis is unknown → None.
                    if h > 0.0 { Some(*v / 100.0 * w.min(h)) } else { None }
                })
            }
            Length::Cqmax(v) => {
                CONTAINER_CQ.with(|c| c.get()).and_then(|(w, h)| {
                    if h > 0.0 { Some(*v / 100.0 * w.max(h)) } else { None }
                })
            }
            Length::Calc(node) => node.resolve(em_basis, percent_basis, viewport),
            // Intrinsic sizing keywords require layout context — not resolvable here.
            Length::MinContent | Length::MaxContent | Length::FitContent(_) => None,
        }
    }

    /// Returns `true` if this is an intrinsic sizing keyword (min-content,
    /// max-content, or fit-content). These are handled specially in layout.
    pub fn is_intrinsic(&self) -> bool {
        matches!(self, Length::MinContent | Length::MaxContent | Length::FitContent(_))
    }

    /// Резолвит с `cb_width` как percent_basis; возвращает 0.0 при неудаче.
    /// Удобна для padding, gap и других не-auto полей.
    pub fn resolve_or_zero(&self, em: f32, cb_width: f32, vp: Size) -> f32 {
        self.resolve(em, Some(cb_width), vp).unwrap_or(0.0)
    }

    /// Извлекает пиксельное значение для уже-разрешённых `Px`-значений
    /// (после layout/cascade). Non-Px варианты → 0.0.
    pub fn px(&self) -> f32 {
        match self {
            Length::Px(v) => *v,
            _ => 0.0,
        }
    }
}

/// Парсит sizing-значение для `width`/`height`/`min-width`/`max-width` и т.д.
/// Обрабатывает:
/// - `auto` → `None`
/// - `min-content` / `max-content` → `Some(Length::MinContent/MaxContent)`
/// - `fit-content` → `Some(Length::FitContent(None))`
/// - `fit-content(<length>)` → `Some(Length::FitContent(Some(l)))`
/// - всё остальное → `parse_length_q()`
pub(in crate::style) fn parse_sizing_length(s: &str, is_quirks: bool) -> Option<Length> {
    let v = s.trim();
    match v {
        "auto" => None,
        "min-content" => Some(Length::MinContent),
        "max-content" => Some(Length::MaxContent),
        "fit-content" | "stretch" | "-webkit-fill-available" | "-moz-available" => {
            // CSS Sizing L3/L4 §4: stretch = fill available; treat same as fit-content.
            Some(Length::FitContent(None))
        }
        _ if v.starts_with("fit-content(") && v.ends_with(')') => {
            let inner = &v["fit-content(".len()..v.len() - 1];
            Some(Length::FitContent(parse_length_q(inner, is_quirks).map(Box::new)))
        }
        _ => parse_length_q(s, is_quirks),
    }
}

/// Парсит CSS-длину: число + опциональная единица (`px`, `em`, `rem`, `%`,
/// `vh`/`vw`/`vmin`/`vmax`). Голое число (`0`) считаем `Px(0)` — CSS позволяет
/// опускать единицу только для нуля, но мы прощаем и для других чисел.
///
/// Порядок проверки суффиксов важен: более длинные сначала (`vmin`/`vmax`
/// перед `vw`/`vh`, `rem` перед `em`).
/// CSS Quirks Mode §3.3: в quirks-mode unitless non-zero число принимается
/// как px; в standards-mode — только `0` валиден без единицы (CSS Values §6).
pub(in crate::style) fn parse_length_q(s: &str, is_quirks: bool) -> Option<Length> {
    let s = s.trim();
    // CSS Values L4: math-функции calc() / min() / max() / clamp().
    // Если значение начинается с буквы и содержит `(` — обрабатываем как
    // функциональный вызов через общий tokenize_calc + parse_calc_expr;
    // parse_calc_factor распознаёт ident+lparen как function call.
    if looks_like_function_call(s)
        && let Some(len) = parse_math_function_value(s) {
        return Some(len);
    }
    if let Some(num) = s.strip_suffix("px") {
        return num.trim().parse::<f32>().ok().map(Length::Px);
    }
    if let Some(num) = s.strip_suffix("rem") {
        return num.trim().parse::<f32>().ok().map(Length::Rem);
    }
    // ── Font-relative units ──────────────────────────────────────────────────
    // `ch` = advance width of the '0' glyph; `ex` = x-height. Both resolve to px
    // against the box's real font metrics at layout time (`FONT_CH_EX`), with a
    // `0.5em` spec fallback (CSS Values L4 §5.1.1).
    // `cap` = cap-height; Phase 0 approximation: 0.7em.
    // `lh` = computed line-height; Phase 0 approximation: 1.2em.
    if let Some(num) = s.strip_suffix("ch") {
        return num.trim().parse::<f32>().ok().map(Length::Ch);
    }
    if let Some(num) = s.strip_suffix("ex") {
        return num.trim().parse::<f32>().ok().map(Length::Ex);
    }
    if let Some(num) = s.strip_suffix("cap") {
        return num.trim().parse::<f32>().ok().map(|n| Length::Em(n * 0.7));
    }
    if let Some(num) = s.strip_suffix("lh") {
        return num.trim().parse::<f32>().ok().map(|n| Length::Em(n * 1.2));
    }
    if let Some(num) = s.strip_suffix("em") {
        return num.trim().parse::<f32>().ok().map(Length::Em);
    }
    // ── Viewport units — longer suffixes before shorter to avoid partial match ─
    // `vmin`/`svmin`/… must precede `in`; `vmax` before `ax` (no conflict
    // but kept together for clarity). `svh`/`dvh` before `vh`, etc.
    if let Some(num) = s.strip_suffix("svmin").or_else(|| s.strip_suffix("dvmin")).or_else(|| s.strip_suffix("lvmin")) {
        return num.trim().parse::<f32>().ok().map(Length::Vmin);
    }
    if let Some(num) = s.strip_suffix("svmax").or_else(|| s.strip_suffix("dvmax")).or_else(|| s.strip_suffix("lvmax")) {
        return num.trim().parse::<f32>().ok().map(Length::Vmax);
    }
    if let Some(num) = s.strip_suffix("vmin") {
        return num.trim().parse::<f32>().ok().map(Length::Vmin);
    }
    if let Some(num) = s.strip_suffix("vmax") {
        return num.trim().parse::<f32>().ok().map(Length::Vmax);
    }
    // ── Small/Large/Dynamic viewport units (CSS Values L4 §7.8) ─────────────
    // Phase 0: fixed viewport → svh/dvh/lvh = vh, svw/dvw/lvw = vw.
    if let Some(num) = s.strip_suffix("svh").or_else(|| s.strip_suffix("dvh")).or_else(|| s.strip_suffix("lvh")) {
        return num.trim().parse::<f32>().ok().map(Length::Vh);
    }
    if let Some(num) = s.strip_suffix("svw").or_else(|| s.strip_suffix("dvw")).or_else(|| s.strip_suffix("lvw")) {
        return num.trim().parse::<f32>().ok().map(Length::Vw);
    }
    if let Some(num) = s.strip_suffix("vh") {
        return num.trim().parse::<f32>().ok().map(Length::Vh);
    }
    if let Some(num) = s.strip_suffix("vw") {
        return num.trim().parse::<f32>().ok().map(Length::Vw);
    }
    // ── Container-relative units (CSS Container Queries L1 §6.2) ─────────────
    // Longer suffixes (cqmin/cqmax) before shorter (cqw/cqh/cqi/cqb).
    if let Some(num) = s.strip_suffix("cqmin") {
        return num.trim().parse::<f32>().ok().map(Length::Cqmin);
    }
    if let Some(num) = s.strip_suffix("cqmax") {
        return num.trim().parse::<f32>().ok().map(Length::Cqmax);
    }
    if let Some(num) = s.strip_suffix("cqw") {
        return num.trim().parse::<f32>().ok().map(Length::Cqw);
    }
    if let Some(num) = s.strip_suffix("cqh") {
        return num.trim().parse::<f32>().ok().map(Length::Cqh);
    }
    if let Some(num) = s.strip_suffix("cqi") {
        return num.trim().parse::<f32>().ok().map(Length::Cqi);
    }
    if let Some(num) = s.strip_suffix("cqb") {
        return num.trim().parse::<f32>().ok().map(Length::Cqb);
    }
    // ── Absolute units → px at parse time (CSS Values L3 §5.2) ──────────────
    // Reference: 1in = 96px, 1pt = 1/72in = 4/3px, 1pc = 12pt = 16px,
    // 1cm = 96/2.54px ≈ 37.7953px, 1mm = 1/10cm, 1Q = 1/4mm.
    // `in` comes after `vmin` (already handled above) to avoid partial match.
    if let Some(num) = s.strip_suffix("pt") {
        return num.trim().parse::<f32>().ok().map(|n| Length::Px(n * 4.0 / 3.0));
    }
    if let Some(num) = s.strip_suffix("pc") {
        return num.trim().parse::<f32>().ok().map(|n| Length::Px(n * 16.0));
    }
    if let Some(num) = s.strip_suffix("in") {
        return num.trim().parse::<f32>().ok().map(|n| Length::Px(n * 96.0));
    }
    if let Some(num) = s.strip_suffix("cm") {
        return num.trim().parse::<f32>().ok().map(|n| Length::Px(n * 96.0 / 2.54));
    }
    if let Some(num) = s.strip_suffix("mm") {
        return num.trim().parse::<f32>().ok().map(|n| Length::Px(n * 96.0 / 25.4));
    }
    if let Some(num) = s.strip_suffix('Q').or_else(|| s.strip_suffix('q')) {
        return num.trim().parse::<f32>().ok().map(|n| Length::Px(n * 96.0 / 101.6));
    }
    if let Some(num) = s.strip_suffix('%') {
        return num.trim().parse::<f32>().ok().map(Length::Percent);
    }
    let n = s.parse::<f32>().ok()?;
    if n == 0.0 || is_quirks { Some(Length::Px(n)) } else { None }
}

pub fn parse_length(s: &str) -> Option<Length> {
    parse_length_q(s, true)
}
