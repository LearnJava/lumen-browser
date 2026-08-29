//! Разбор `font-size`, `line-height` и `font`-шортхенда: pre-pass-применение
//! `font-size` с его CSS-wide keyword-ами, резолв `<font-size>` в абсолютные px
//! и токенайзер `font`-шортхенда (CSS Fonts L4 §6.10).
//!
//! Перенесено батчем SPLIT-ST4 из `crates/engine/layout/src/style.rs`
//! (анкеры `fn apply_font_size` … `fn parse_font_shorthand`) без правок тел:
//! изменены только пути модулей и видимость тех items, которые продолжает
//! звать `style.rs` и его тест-модули.

use lumen_core::geom::Size;
use lumen_css_parser::Declaration;

use crate::style::{
    ComputedStyle, CssWideKeyword, FontStretch, FontStyle, Length, ROOT_FONT_SIZE,
    expand_vars_and_env, parse_css_wide_keyword, parse_length, parse_length_q,
};

/// Применяет `font-size`-декларацию, если она задана. Размер `em` берётся
/// относительно `parent_fs` (родительский font-size), `rem` — относительно
/// ROOT_FONT_SIZE, `%` — относительно `parent_fs`.
///
/// Обрабатывает также `font`-shorthand (BUG-114): в pre-pass резолвится только
/// `<font-size>`-компонент; остальные longhand-ы (style/variant/weight/stretch/
/// line-height/family) применяются в main-pass — арм `"font" =>`.
///
/// `ua_baseline_fs` — `font-size` из UA-снэпшота (см. `compute_style`), источник
/// для `revert`: элементы вроде `<small>`/`<sub>`/`<sup>`/`<h1>`–`<h6>` получают
/// UA-хинт на font-size (`ua_font_size_factor`/`apply_ua_heading_style`) до этого
/// pre-pass-а, так что `revert` должен откатываться к нему, а не к голому `parent_fs`.
///
/// BUG-731: `var()`/`env()` раскрываются здесь так же, как это делает
/// `apply_declaration` для main-pass-свойств. Раньше pre-pass парсил сырую
/// строку, поэтому `font-size: var(--fs)` и `font: var(--f)` молча теряли
/// размер (остальные longhand-ы `font` при этом применялись — их считает
/// main-pass, который var() раскрывает).
pub(in crate::style) fn apply_font_size(
    style: &mut ComputedStyle,
    decl: &Declaration,
    parent_fs: f32,
    ua_baseline_fs: f32,
    viewport: Size,
    is_quirks: bool,
) -> Option<FontSizeBasis> {
    if decl.property != "font" && decl.property != "font-size" {
        return None;
    }
    // CSS Variables L1 §3.3: нераскрываемый `var()` делает декларацию invalid at
    // computed value time — она просто не применяется.
    let expanded;
    let raw: &str = if decl.value.contains("var(") || decl.value.contains("env(") {
        expanded = expand_vars_and_env(&decl.value, &style.custom_props)?;
        expanded.as_str()
    } else {
        decl.value.as_str()
    };

    if decl.property == "font" {
        let parts = parse_font_shorthand(raw)?;
        return resolve_font_size(style, &parts.size, parent_fs, viewport, is_quirks);
    }
    let val = raw;
    // CSS Cascade L4 §7: CSS-wide keywords. font-size — inherited; unset ==
    // inherit; revert rolls back to the UA-hinted value (falls back to
    // `parent_fs` when the element has no font-size UA hint, since
    // `ua_baseline_fs` then equals `parent_fs` anyway).
    if let Some(kw) = parse_css_wide_keyword(val) {
        let (px, basis) = match kw {
            CssWideKeyword::Inherit | CssWideKeyword::Unset => {
                (parent_fs, FontSizeBasis::ParentRelative)
            }
            // The UA baseline is itself usually an `em` factor over the parent
            // (`h1 { font-size: 2em }`), and where the element has no UA hint it
            // *is* `parent_fs` — parent-relative either way.
            CssWideKeyword::Revert => (ua_baseline_fs, FontSizeBasis::ParentRelative),
            CssWideKeyword::Initial => (ROOT_FONT_SIZE, FontSizeBasis::Absolute),
        };
        style.font_size = px;
        return Some(basis);
    }
    resolve_font_size(style, val, parent_fs, viewport, is_quirks)
}

/// What the winning `font-size` declaration resolved *against* — the one thing
/// `zoom` needs to know about it (CSS Viewport L1 §5).
///
/// A parent-relative value (`em`, `%`, `ex`/`ch`, `inherit`) is computed from
/// `parent_fs`, which already carries every ancestor's zoom, so only the
/// element's *own* factor may be applied on top. An absolute one (`px`, `rem`,
/// viewport units, `initial`) comes from a zoom-independent basis and therefore
/// takes the full compounded `effective_zoom`. Multiplying the first kind by
/// `effective_zoom` would re-apply the ancestors' factor once per level, so a
/// tree of `em` sizes under a zoomed container would shrink geometrically.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::style) enum FontSizeBasis {
    /// Resolved against the parent's (already zoomed) font-size.
    ParentRelative,
    /// Resolved against a basis no ancestor's `zoom` has touched.
    Absolute,
}

/// Резолвит `<font-size>`-значение (без CSS-wide keyword-ов) в абсолютный px и
/// записывает в `style.font_size`. `em`/`%` — от `parent_fs`, `rem` — от
/// ROOT_FONT_SIZE, viewport-единицы — от `viewport`. Используется и longhand-ом
/// `font-size`, и `<font-size>`-компонентом `font`-shorthand.
///
/// Возвращает [`FontSizeBasis`] применённого значения (`None` — значение не
/// разобралось и `style.font_size` не тронут): вызывающему это нужно, чтобы
/// решить, каким множителем `zoom` домножать результат.
fn resolve_font_size(
    style: &mut ComputedStyle,
    val: &str,
    parent_fs: f32,
    viewport: Size,
    is_quirks: bool,
) -> Option<FontSizeBasis> {
    let len = parse_length_q(val, is_quirks)?;
    // Для font-size: em и % считаются от parent_fs; vh/vw/vmin/vmax — от viewport.
    let (px, basis) = match &len {
        Length::Px(v) => (*v, FontSizeBasis::Absolute),
        Length::Em(v) => (*v * parent_fs, FontSizeBasis::ParentRelative),
        Length::Rem(v) => (*v * ROOT_FONT_SIZE, FontSizeBasis::Absolute),
        // CSS Values L4 §5.1.1 — font-relative units on `font-size` itself refer to
        // the *parent* font. Real ch/ex metrics for the parent are not available at
        // computed-value time, so use the spec `0.5em` fallback against `parent_fs`.
        Length::Ch(v) | Length::Ex(v) => (*v * 0.5 * parent_fs, FontSizeBasis::ParentRelative),
        Length::Percent(v) => (*v / 100.0 * parent_fs, FontSizeBasis::ParentRelative),
        Length::Vh(v) => (*v / 100.0 * viewport.height, FontSizeBasis::Absolute),
        Length::Vw(v) => (*v / 100.0 * viewport.width, FontSizeBasis::Absolute),
        Length::Vmin(v) => {
            (*v / 100.0 * viewport.width.min(viewport.height), FontSizeBasis::Absolute)
        }
        Length::Vmax(v) => {
            (*v / 100.0 * viewport.width.max(viewport.height), FontSizeBasis::Absolute)
        }
        // cq* units — resolved via CONTAINER_CQ thread-local (set during container re-layout).
        Length::Cqw(_) | Length::Cqh(_) | Length::Cqi(_) | Length::Cqb(_)
        | Length::Cqmin(_) | Length::Cqmax(_) => {
            (len.resolve(parent_fs, None, viewport)?, FontSizeBasis::Absolute)
        }
        // `calc()` для font-size: резолвим с em_basis = parent_fs и
        // percent_basis = parent_fs (для `%` внутри выражения). vh/vw
        // используют viewport, что уже делает CalcNode::resolve.
        //
        // Basis: a `calc()` may mix both kinds (`calc(1em + 2px)`); it is counted
        // as parent-relative because under-applying `zoom` to the absolute term
        // is a bounded error, while double-applying it to the relative one
        // compounds once per nesting level.
        Length::Calc(node) => (
            node.resolve(parent_fs, Some(parent_fs), viewport)?,
            FontSizeBasis::ParentRelative,
        ),
        // Intrinsic keywords not meaningful for font-size — ignore.
        Length::MinContent | Length::MaxContent | Length::FitContent(_) => return None,
    };
    style.font_size = px;
    Some(basis)
}

/// Применяет `<line-height>`-значение в `style.line_height` (+ флаг
/// `line_height_is_relative`). `1.5` (unitless) — относительный коэффициент;
/// unit-несущие значения (`24px`, `1.5em`, `150%`) фиксируются в абсолютную
/// высоту (CSS2 §10.8.1) и хранятся как ratio для горячего пути layout-а.
/// Используется longhand-ом `line-height` и `<line-height>`-компонентом
/// `font`-shorthand.
pub(in crate::style) fn apply_line_height_value(style: &mut ComputedStyle, val: &str, em_basis: f32, viewport: Size) {
    if let Ok(v) = val.parse::<f32>() {
        // Unitless `<number>` — relative: the line box scales with the
        // used font-size (incl. any `font-size-adjust` rescale).
        style.line_height = v;
        style.line_height_is_relative = true;
    } else if let Some(len) = parse_length(val) {
        // Every unit-bearing value computes to an absolute length
        // (CSS2 §10.8.1: `<length>`/`<percentage>`/`em`/`rem` line-height
        // resolves at computed-value time), so the line box must be frozen
        // and must NOT rescale when `font-size-adjust` changes the used
        // font-size. Stored as a ratio purely for the layout hot path.
        style.line_height_is_relative = false;
        match &len {
            Length::Px(v) => style.line_height = v / style.font_size,
            Length::Em(v) => style.line_height = *v,
            Length::Rem(v) => {
                style.line_height = v * ROOT_FONT_SIZE / style.font_size;
            }
            Length::Percent(v) => style.line_height = v / 100.0,
            Length::Ch(_)
            | Length::Ex(_)
            | Length::Vh(_)
            | Length::Vw(_)
            | Length::Vmin(_)
            | Length::Vmax(_)
            | Length::Cqw(_)
            | Length::Cqh(_)
            | Length::Cqi(_)
            | Length::Cqb(_)
            | Length::Cqmin(_)
            | Length::Cqmax(_)
            | Length::Calc(_) => {
                // Резолвим в px и переводим в коэффициент.
                // Для calc() — то же самое: если выражение содержит
                // только unitless (`calc(1 + 0.5)`) → результат уже
                // коэффициент, но мы не умеем сейчас отличить unitless
                // от px; делим всегда на font_size — это даёт верный
                // ответ для length-результатов и неверный для чистых
                // чисел внутри calc. Phase 0 ограничение: для чистых
                // чисел используйте bare-form `line-height: 1.5`.
                if let Some(px) = len.resolve(em_basis, None, viewport) {
                    style.line_height = px / style.font_size;
                }
            }
            // Intrinsic keywords not meaningful for line-height — ignore.
            Length::MinContent | Length::MaxContent | Length::FitContent(_) => {}
        }
    }
}

/// Разобранные компоненты CSS `font`-shorthand (CSS Fonts L4 §6.10):
///
/// `font = [ <font-style> || <font-variant-css2> || <font-weight> ||
///           <font-width-css3> ]? <font-size> [ / <line-height> ]? <font-family>`
///
/// Каждое поле хранит «сырой» токен(ы) соответствующего longhand-а, чтобы
/// финальный разбор делали уже существующие парсеры (`parse_font_weight`,
/// `parse_font_family`, резолвер `line-height`) — единый источник истины.
/// `None`/пустое = компонент опущен и должен сброситься в initial-значение
/// (shorthand сбрасывает все управляемые им longhand-ы — CSS Cascade L4 §3.1).
pub(in crate::style) struct FontShorthand {
    /// `italic` / `oblique`; `None` → initial `normal`.
    pub(in crate::style) style: Option<FontStyle>,
    /// `true`, если в leading-секции встретился `small-caps`.
    pub(in crate::style) small_caps: bool,
    /// Сырой токен веса (`bold`/`bolder`/`lighter`/`100`..`900`); `None` → `normal`.
    pub(in crate::style) weight: Option<String>,
    /// `font-stretch` keyword; `None` → initial `normal`.
    pub(in crate::style) stretch: Option<FontStretch>,
    /// Сырой `<font-size>`-токен (обязателен), напр. `13px`.
    pub(in crate::style) size: String,
    /// Сырой `<line-height>`-токен после `/`; `None` → initial `normal`.
    pub(in crate::style) line_height: Option<String>,
    /// Сырой `<font-family>`-хвост (обязателен), напр. `"Helvetica Neue", sans-serif`.
    pub(in crate::style) family: String,
}

/// `true`, если токен — валидный `<font-size>`: absolute/relative-size keyword
/// (CSS Fonts L4 §2.2) либо `<length>`/`<percentage>`. Bare-number (вес) в
/// standards-mode отбрасывается `parse_length_q`, поэтому `700` не спутается с
/// размером.
pub(in crate::style) fn is_font_size_token(tok: &str) -> bool {
    if matches!(
        tok,
        "xx-small" | "x-small" | "small" | "medium" | "large" | "x-large" | "xx-large"
            | "xxx-large" | "larger" | "smaller"
    ) {
        return true;
    }
    // Набор вариантов — ровно тот, что умеет `resolve_font_size`; intrinsic-
    // keyword-ы (`min-content` и Ко) для font-size бессмысленны и им отсеяны.
    // BUG-731: `Calc` и cq*-единицы раньше отсутствовали здесь, хотя
    // `resolve_font_size` их считает — из-за чего `font: 700 calc(…)/… serif`
    // целиком признавался невалидным shorthand-ом, а longhand
    // `font-size: calc(…)` с тем же значением работал.
    matches!(parse_length_q(tok, false), Some(Length::Px(_) | Length::Em(_) | Length::Rem(_)
        | Length::Ch(_) | Length::Ex(_)
        | Length::Percent(_) | Length::Vh(_) | Length::Vw(_) | Length::Vmin(_) | Length::Vmax(_)
        | Length::Cqw(_) | Length::Cqh(_) | Length::Cqi(_) | Length::Cqb(_)
        | Length::Cqmin(_) | Length::Cqmax(_)
        | Length::Calc(_)))
}

/// Токенизирует значение `font`-shorthand с учётом вложенных скобок: пробел и
/// `/` разделяют токены только на глубине 0, поэтому `calc(0px + 44px)` и
/// `calc((44px) * 1.09)` остаются одним токеном вместе со своими пробелами и
/// делением внутри (BUG-731). `/` на глубине 0 выдаётся отдельным токеном —
/// вызывающему не нужно нормализовать значение заранее.
///
/// Незакрытая скобка не отбрасывается: остаток становится последним токеном, а
/// невалидность поймает уже разбор компонентов.
pub(in crate::style) fn split_font_shorthand_tokens(val: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    for ch in val.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(ch);
            }
            '/' if depth == 0 => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
                tokens.push("/".to_string());
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

/// Разбирает CSS `font`-shorthand в компоненты. Возвращает `None`, если значение
/// невалидно или это system-font/CSS-wide keyword (их раскрытие не делаем).
///
/// Алгоритм: токенизируем `split_font_shorthand_tokens` (скобко-аварная
/// разбивка, `/` — отдельный токен). Leading-секция = `style || variant
/// || weight || width` в любом порядке (плюс no-op `normal` и `oblique <angle>`),
/// потребляется до первого `<font-size>`-токена. Дальше — `<font-size>`,
/// опциональный `/ <line-height>`, остаток — обязательный `<font-family>`.
pub(in crate::style) fn parse_font_shorthand(val: &str) -> Option<FontShorthand> {
    let trimmed = val.trim();
    if trimmed.is_empty() {
        return None;
    }
    // System-font keywords и CSS-wide keywords здесь не раскрываем.
    let lower = trimmed.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "caption" | "icon" | "menu" | "message-box" | "small-caption" | "status-bar"
            | "inherit" | "initial" | "unset" | "revert" | "revert-layer"
    ) {
        return None;
    }

    // `13px/1.4`, `13px /1.4`, `13px/ 1.4`, `13px / 1.4` → одинаковая токенизация.
    let tokens: Vec<String> = split_font_shorthand_tokens(trimmed);

    let mut style: Option<FontStyle> = None;
    let mut small_caps = false;
    let mut weight: Option<String> = None;
    let mut stretch: Option<FontStretch> = None;

    // Leading-секция: потребляем до первого валидного <font-size>-токена.
    let mut i = 0;
    while i < tokens.len() {
        let tl = tokens[i].to_ascii_lowercase();
        if is_font_size_token(&tl) {
            break;
        }
        match tl.as_str() {
            "normal" => {}
            "italic" => style = Some(FontStyle::Italic),
            "oblique" => style = Some(FontStyle::Oblique),
            "small-caps" => small_caps = true,
            "bold" | "bolder" | "lighter" => weight = Some(tl),
            // `oblique <angle>` — угол после oblique игнорируем (Phase 0 берёт
            // oblique без угла), но потребляем как часть leading-секции.
            _ if tl.ends_with("deg")
                || tl.ends_with("grad")
                || tl.ends_with("rad")
                || tl.ends_with("turn") => {}
            _ => {
                if let Some(fs) = FontStretch::from_keyword(&tl) {
                    stretch = Some(fs);
                } else if tl.parse::<u16>().ok().filter(|&n| (1..=1000).contains(&n)).is_some() {
                    weight = Some(tl);
                } else {
                    // Неизвестный leading-токен → невалидный shorthand.
                    return None;
                }
            }
        }
        i += 1;
    }

    // <font-size> обязателен.
    let size = tokens.get(i)?;
    if !is_font_size_token(&size.to_ascii_lowercase()) {
        return None;
    }
    let size = size.clone();
    i += 1;

    // Опциональный `/ <line-height>`.
    let mut line_height = None;
    if tokens.get(i).map(String::as_str) == Some("/") {
        i += 1;
        line_height = Some(tokens.get(i)?.clone());
        i += 1;
    }

    // <font-family> обязателен — весь остаток.
    if i >= tokens.len() {
        return None;
    }
    let family = tokens[i..].join(" ");

    Some(FontShorthand { style, small_caps, weight, stretch, size, line_height, family })
}
