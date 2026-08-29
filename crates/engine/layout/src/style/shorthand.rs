//! Развёртка CSS-шортхендов, у которых нет отдельного longhand-парсера:
//! `text-decoration`, `text-emphasis`, `contain-intrinsic-size`, `text-wrap`,
//! `flex-flow`/`flex`, `grid-column`/`grid-row`/`grid-area`.
//!
//! Перенесено батчем SPLIT-ST7 из `crates/engine/layout/src/style.rs`
//! (анкер `struct ParsedTextDecorationShorthand`) без правок тел: изменены
//! только видимость item-ов и пути импортов.

use lumen_core::geom::Size;

use crate::style::parse::color::parse_css_color_legacy;
use crate::style::{
    parse_length, ComputedStyle, CssColor, FlexBasis, FlexDirection, FlexWrap, GridLine, Length,
    TextDecorationLine, TextDecorationStyle, TextDecorationThickness, TextEmphasisPosition,
    TextEmphasisShape, TextEmphasisStyle, TextWrapMode, TextWrapStyle, WhiteSpace,
};

/// Результат разбора `text-decoration` shorthand-а.
///
/// `any_recognized` отличает «полностью невалидный shorthand → declaration
/// ignored» от «частично распознанный → применяется initial для непроставленных
/// сторон». Без него `text-decoration: foo` молча сбрасывал бы существующее
/// значение к initial.
pub(crate) struct ParsedTextDecorationShorthand {
    pub line: Option<TextDecorationLine>,
    pub color: Option<CssColor>,
    pub style: Option<TextDecorationStyle>,
    pub any_recognized: bool,
}

// parse_text_decoration_shorthand_q: разбирает `text-decoration` shorthand или
// `text-decoration-line`. CSS Text Decoration L3 §2.1 shorthand: `<line> || <style>
// || <color>` в любом порядке. `text-decoration-thickness` исключена из L3
// shorthand-а. Phase 0 keyword-ы линий: `underline`, `overline`, `line-through`,
// `none`. `none` сбрасывает все линии. `currentcolor` keyword сбрасывает color в
// None. Wrapper для тестов: parse_text_decoration_shorthand (#[cfg(test)]).

#[cfg(test)]
pub(in crate::style) fn parse_text_decoration_shorthand(val: &str) -> ParsedTextDecorationShorthand {
    parse_text_decoration_shorthand_q(val, false)
}

pub(in crate::style) fn parse_text_decoration_shorthand_q(val: &str, is_quirks: bool) -> ParsedTextDecorationShorthand {
    let mut out_line = TextDecorationLine::default();
    let mut any_line = false;
    let mut none_seen = false;
    let mut out_style: Option<TextDecorationStyle> = None;
    let mut color: Option<CssColor> = None;
    let mut any_recognized = false;
    // Цвет может быть многословным: `rgb(0, 0, 0)`, `hsl(0 0% 0% / 1)`, …
    // Соберём «не-линия / не-стиль» токены и попытаемся склеить.
    let mut residue: Vec<&str> = Vec::new();
    for token in val.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "none" => {
                none_seen = true;
                any_line = true;
                any_recognized = true;
            }
            "underline" => {
                out_line.underline = true;
                any_line = true;
                any_recognized = true;
            }
            "overline" => {
                out_line.overline = true;
                any_line = true;
                any_recognized = true;
            }
            "line-through" => {
                out_line.line_through = true;
                any_line = true;
                any_recognized = true;
            }
            "solid" => {
                out_style = Some(TextDecorationStyle::Solid);
                any_recognized = true;
            }
            "double" => {
                out_style = Some(TextDecorationStyle::Double);
                any_recognized = true;
            }
            "dotted" => {
                out_style = Some(TextDecorationStyle::Dotted);
                any_recognized = true;
            }
            "dashed" => {
                out_style = Some(TextDecorationStyle::Dashed);
                any_recognized = true;
            }
            "wavy" => {
                out_style = Some(TextDecorationStyle::Wavy);
                any_recognized = true;
            }
            "blink" => {
                // CSS2 deprecated; токен поглощаем, чтобы он не попал в
                // color-парсер.
                any_recognized = true;
            }
            "currentcolor" => {
                color = Some(CssColor::CurrentColor);
                any_recognized = true;
            }
            _ => residue.push(token),
        }
    }
    if !residue.is_empty() {
        // Попробуем сначала весь residue (на случай color-функции с
        // пробелами: `rgb(0 0 0)` → токены `rgb(0`, `0`, `0)`).
        let joined = residue.join(" ");
        if let Some(c) = parse_css_color_legacy(joined.trim(), is_quirks) {
            color = Some(c);
            any_recognized = true;
        } else {
            // Иначе пробуем токен за токеном — для named-color / hex без
            // пробелов внутри.
            for tok in &residue {
                if let Some(c) = parse_css_color_legacy(tok, is_quirks) {
                    color = Some(c);
                    any_recognized = true;
                    break;
                }
            }
        }
    }
    let line = if any_line {
        if none_seen { Some(TextDecorationLine::default()) } else { Some(out_line) }
    } else {
        None
    };
    ParsedTextDecorationShorthand {
        line,
        color,
        style: out_style,
        any_recognized,
    }
}

/// Парсит значение `text-decoration-thickness` (CSS Text Decoration L3 §2.3).
///
/// `auto | from-font | <length> | <percentage>`. Длина резолвится в
/// resolved-px через [`Length::resolve`] (поддерживает px/em/rem/vw/vh/calc).
/// Процент сохраняется как fraction (`5%` → 0.05) — финальное домножение на
/// parent.font_size происходит в renderer-е по spec.
pub(in crate::style) fn parse_text_decoration_thickness(
    val: &str,
    em_basis: f32,
    viewport: Size,
) -> Option<TextDecorationThickness> {
    let trimmed = val.trim();
    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "auto" => return Some(TextDecorationThickness::Auto),
        "from-font" => return Some(TextDecorationThickness::FromFont),
        _ => {}
    }
    if let Some(pct_str) = trimmed.strip_suffix('%')
        && let Ok(n) = pct_str.trim().parse::<f32>()
    {
        return Some(TextDecorationThickness::Percentage(n / 100.0));
    }
    let len = parse_length(trimmed)?;
    let px = len.resolve(em_basis, None, viewport)?;
    Some(TextDecorationThickness::Length(px))
}

fn parse_text_emphasis_shape(s: &str) -> Option<TextEmphasisShape> {
    match s.to_ascii_lowercase().as_str() {
        "dot" => Some(TextEmphasisShape::Dot),
        "circle" => Some(TextEmphasisShape::Circle),
        "double-circle" => Some(TextEmphasisShape::DoubleCircle),
        "triangle" => Some(TextEmphasisShape::Triangle),
        "sesame" => Some(TextEmphasisShape::Sesame),
        _ => None,
    }
}

fn parse_text_emphasis_fill(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "filled" => Some(true),
        "open" => Some(false),
        _ => None,
    }
}

/// Извлекает первый строковый литерал в value: `"X"` или `'X'`. Возвращает
/// (content_without_quotes, rest_after_close). Невалидное / unterminated → None.
fn extract_first_string(val: &str) -> Option<(String, &str)> {
    let trimmed = val.trim_start();
    let mut chars = trimmed.char_indices();
    let (_, quote) = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    for (i, ch) in chars {
        if ch == quote {
            let start_byte = trimmed.char_indices().next()?.0 + quote.len_utf8();
            let content = trimmed[start_byte..i].to_string();
            return Some((content, &trimmed[i + ch.len_utf8()..]));
        }
    }
    None
}

/// CSS Text Decoration L4 §5.3 — `text-emphasis-style`. Returns `None` если
/// value не парсится (invalid declaration ignored).
pub(in crate::style) fn parse_text_emphasis_style(val: &str) -> Option<TextEmphasisStyle> {
    let trimmed = val.trim();
    if trimmed.eq_ignore_ascii_case("none") {
        return Some(TextEmphasisStyle::None);
    }
    if let Some((s, rest)) = extract_first_string(trimmed) {
        if !rest.trim().is_empty() {
            return None;
        }
        return Some(TextEmphasisStyle::String(s));
    }
    let mut fill: Option<bool> = None;
    let mut shape: Option<TextEmphasisShape> = None;
    for tok in trimmed.split_whitespace() {
        if let Some(f) = parse_text_emphasis_fill(tok) {
            if fill.is_some() {
                return None;
            }
            fill = Some(f);
        } else {
            let sh = parse_text_emphasis_shape(tok)?;
            if shape.is_some() {
                return None;
            }
            shape = Some(sh);
        }
    }
    if fill.is_none() && shape.is_none() {
        return None;
    }
    Some(TextEmphasisStyle::Symbol {
        filled: fill.unwrap_or(true),
        shape: shape.unwrap_or(TextEmphasisShape::Circle),
    })
}

/// CSS Text Decoration L4 §5.5 — `text-emphasis-position`. Grammar
/// `[ over | under ] && [ right | left ]?`. Spec: vertical axis (over/under)
/// обязателен, horizontal axis (right/left) опционален с default `right`.
pub(in crate::style) fn parse_text_emphasis_position(val: &str) -> Option<TextEmphasisPosition> {
    let mut over: Option<bool> = None;
    let mut right: Option<bool> = None;
    for tok in val.split_whitespace() {
        match tok.to_ascii_lowercase().as_str() {
            "over" => {
                if over.is_some() {
                    return None;
                }
                over = Some(true);
            }
            "under" => {
                if over.is_some() {
                    return None;
                }
                over = Some(false);
            }
            "right" => {
                if right.is_some() {
                    return None;
                }
                right = Some(true);
            }
            "left" => {
                if right.is_some() {
                    return None;
                }
                right = Some(false);
            }
            _ => return None,
        }
    }
    let over = over?;
    let right = right.unwrap_or(true);
    Some(match (over, right) {
        (true, true) => TextEmphasisPosition::OverRight,
        (true, false) => TextEmphasisPosition::OverLeft,
        (false, true) => TextEmphasisPosition::UnderRight,
        (false, false) => TextEmphasisPosition::UnderLeft,
    })
}

/// CSS Text Decoration L4 §5.6 — `text-emphasis` shorthand для `-style` и
/// `-color`. По spec position НЕ часть shorthand-а.
///
/// Извлекает первый color-токен (consumes полностью) и оставшийся текст
/// парсит как text-emphasis-style. Невалидные cases — оба longhand-а
/// сбрасываются к initial.
pub(in crate::style) fn apply_text_emphasis_shorthand(style: &mut ComputedStyle, val: &str, is_quirks: bool) {
    style.text_emphasis_style = TextEmphasisStyle::None;
    style.text_emphasis_color = CssColor::CurrentColor;
    let trimmed = val.trim();
    if trimmed.is_empty() {
        return;
    }

    // string-форма `text-emphasis: "★"` — без других токенов.
    if let Some((s, rest)) = extract_first_string(trimmed)
        && rest.trim().is_empty()
    {
        style.text_emphasis_style = TextEmphasisStyle::String(s);
        return;
    }

    let mut color: Option<CssColor> = None;
    let mut style_tokens: Vec<&str> = Vec::new();
    for tok in trimmed.split_whitespace() {
        if tok.eq_ignore_ascii_case("currentcolor") {
            if color.is_some() {
                return;
            }
            color = Some(CssColor::CurrentColor);
            continue;
        }
        if parse_text_emphasis_fill(tok).is_some() || parse_text_emphasis_shape(tok).is_some() {
            style_tokens.push(tok);
            continue;
        }
        if color.is_none()
            && let Some(c) = parse_css_color_legacy(tok, is_quirks)
        {
            color = Some(c);
            continue;
        }
        return;
    }

    style.text_emphasis_color = color.unwrap_or(CssColor::CurrentColor);

    if style_tokens.is_empty() {
        return;
    }
    let joined = style_tokens.join(" ");
    if joined.eq_ignore_ascii_case("none") {
        style.text_emphasis_style = TextEmphasisStyle::None;
        return;
    }
    if let Some(s) = parse_text_emphasis_style(&joined) {
        style.text_emphasis_style = s;
    }
}

/// CSS Text Module Level 4 §6.4.3 — `text-wrap` shorthand.
///
/// Сбрасывает обе longhand-компоненты (`text-wrap-mode` / `text-wrap-style`)
/// к initial-value и применяет распознанные токены. Грамматика
/// `<'text-wrap-mode'> || <'text-wrap-style'>` — 1..=2 keyword-а, любой
/// порядок, без повторов внутри своего слота. Нераспознанный токен ⇒
/// весь shorthand невалиден (initial-значения сохраняются как «после reset»).
/// CSS Box Sizing L4 §5 — parse one `contain-intrinsic-*` component:
/// `auto? [ none | <length> ]`. Returns `Some(None)` for `none` (no placeholder),
/// `Some(Some(len))` for a length, and `None` on a parse error (declaration is
/// then ignored, leaving the previous value). The leading `auto` keyword
/// (last-remembered-size hint) is accepted and discarded.
/// Returns `(auto_keyword_present, placeholder)` — the `auto` flag is carried
/// separately because it changes nothing in layout but is part of the computed
/// value the CSSOM must serialise (BUG-852).
pub(in crate::style) fn parse_contain_intrinsic_one(val: &str) -> Option<(bool, Option<Length>)> {
    let mut v = val.trim();
    let mut auto = false;
    if let Some(rest) = v.strip_prefix("auto")
        && (rest.is_empty() || rest.starts_with(char::is_whitespace))
    {
        auto = true;
        v = rest.trim_start();
    }
    if v.eq_ignore_ascii_case("none") {
        return Some((auto, None));
    }
    parse_length(v).map(|l| (auto, Some(l)))
}

/// CSS Box Sizing L4 §5 — parse the `contain-intrinsic-size` shorthand:
/// `[ auto? [ none | <length> ] ]{1,2}`. One component sets both axes; two set
/// width then height. Each component carries its own `auto` keyword — the
/// shorthand's two halves are independent (`auto 1px 2px` is legal), so the flag
/// travels per axis, like the length. Returns `None` on any parse error.
#[allow(clippy::type_complexity)]
pub(in crate::style) fn parse_contain_intrinsic_size(
    val: &str,
) -> Option<((bool, Option<Length>), (bool, Option<Length>))> {
    let tokens: Vec<&str> = val.split_whitespace().collect();
    let mut comps: Vec<(bool, Option<Length>)> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let mut auto = false;
        if tokens[i].eq_ignore_ascii_case("auto") {
            auto = true;
            i += 1;
            if i >= tokens.len() {
                return None;
            }
        }
        let t = tokens[i];
        if t.eq_ignore_ascii_case("none") {
            comps.push((auto, None));
        } else {
            let l = parse_length(t)?;
            comps.push((auto, Some(l)));
        }
        i += 1;
    }
    match comps.len() {
        1 => Some((comps[0].clone(), comps[0].clone())),
        2 => Some((comps[0].clone(), comps[1].clone())),
        _ => None,
    }
}

pub(in crate::style) fn apply_text_wrap_shorthand(style: &mut ComputedStyle, val: &str) {
    style.text_wrap_mode = TextWrapMode::Wrap;
    style.text_wrap_style = TextWrapStyle::Auto;

    'parse: {
        let mut mode: Option<TextWrapMode> = None;
        let mut wrap_style: Option<TextWrapStyle> = None;
        for tok in val.split_whitespace() {
            if let Some(m) = TextWrapMode::parse(tok) {
                if mode.is_some() {
                    break 'parse;
                }
                mode = Some(m);
                continue;
            }
            if let Some(s) = TextWrapStyle::parse(tok) {
                if wrap_style.is_some() {
                    break 'parse;
                }
                wrap_style = Some(s);
                continue;
            }
            break 'parse;
        }
        if let Some(m) = mode {
            style.text_wrap_mode = m;
        }
        if let Some(s) = wrap_style {
            style.text_wrap_style = s;
        }
    }
    // CSS Text L4 §2.1: text-wrap-mode — компонента white-space; пересчитать
    // эффективное значение (shorthand мог сбросить mode к initial).
    style.white_space = WhiteSpace::combine(style.white_space_collapse, style.text_wrap_mode);
}

/// CSS Flexbox L1 §5.3 — `flex-flow` shorthand.
///
/// Грамматика: `<'flex-direction'> || <'flex-wrap'>`. Сбрасывает обе
/// longhand-компоненты к initial-value и применяет распознанные токены.
pub(in crate::style) fn apply_flex_flow_shorthand(style: &mut ComputedStyle, val: &str) {
    style.flex_direction = FlexDirection::Row;
    style.flex_wrap = FlexWrap::Nowrap;

    let mut dir: Option<FlexDirection> = None;
    let mut wrap: Option<FlexWrap> = None;
    for tok in val.split_whitespace() {
        if let Some(d) = FlexDirection::parse(tok) {
            if dir.is_some() {
                return;
            }
            dir = Some(d);
            continue;
        }
        if let Some(w) = FlexWrap::parse(tok) {
            if wrap.is_some() {
                return;
            }
            wrap = Some(w);
            continue;
        }
        return;
    }
    if let Some(d) = dir {
        style.flex_direction = d;
    }
    if let Some(w) = wrap {
        style.flex_wrap = w;
    }
}

/// CSS Flexbox L1 §7 — `flex` shorthand.
///
/// Грамматика: `none | auto | [ <'flex-grow'> <'flex-shrink'>? || <'flex-basis'> ]`.
/// Специальные ключевые слова:
/// - `none` → `0 0 auto`
/// - `auto` → `1 1 auto`
/// - одно `<number>` → flex-grow; flex-shrink=1; flex-basis=0 (не auto!)
/// - два `<number>` → flex-grow flex-shrink; flex-basis=0
/// - `<number> <length>` → flex-grow 1 flex-basis
/// - `<number> <number> <length|auto|content>` → полная форма
///
/// Shorthand всегда сбрасывает все три longhand-а перед применением.
pub(in crate::style) fn apply_flex_shorthand(style: &mut ComputedStyle, val: &str, is_quirks: bool) {
    // Shorthand reset: spec §7 «when flex is given a single value, the
    // other components are set to their initial values for the shorthand»:
    // flex-grow=1, flex-shrink=1, flex-basis=0 (NB: not `auto`!).
    // We reset to these "shorthand defaults" and then apply tokens.
    style.flex_grow = 1.0;
    style.flex_shrink = 1.0;
    style.flex_basis = FlexBasis::Length(Length::Px(0.0));

    let trimmed = val.trim();
    // Special keyword forms.
    match trimmed.to_ascii_lowercase().as_str() {
        "none" => {
            style.flex_grow = 0.0;
            style.flex_shrink = 0.0;
            style.flex_basis = FlexBasis::Auto;
            return;
        }
        "auto" => {
            style.flex_grow = 1.0;
            style.flex_shrink = 1.0;
            style.flex_basis = FlexBasis::Auto;
            return;
        }
        _ => {}
    }

    // Tokenized form: up to 3 tokens.
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() {
        return;
    }

    // Try to classify each token as number or length/keyword.
    let mut numbers: Vec<f32> = Vec::new();
    let mut basis: Option<FlexBasis> = None;

    for tok in &tokens {
        if let Ok(n) = tok.parse::<f32>() {
            if n >= 0.0 {
                numbers.push(n);
            } else {
                return; // invalid
            }
        } else if let Some(b) = FlexBasis::parse(tok, is_quirks) {
            if basis.is_some() {
                return; // duplicate
            }
            basis = Some(b);
        } else {
            return; // unrecognized token
        }
    }

    match (numbers.len(), basis) {
        (1, None) => {
            // flex: <number> → grow=N, shrink=1, basis=0
            style.flex_grow = numbers[0];
        }
        (2, None) => {
            // flex: <number> <number> → grow shrink, basis=0
            style.flex_grow = numbers[0];
            style.flex_shrink = numbers[1];
        }
        (1, Some(b)) => {
            // flex: <number> <basis> → grow=N, shrink=1, basis=b
            style.flex_grow = numbers[0];
            style.flex_basis = b;
        }
        (2, Some(b)) => {
            // flex: <grow> <shrink> <basis>
            style.flex_grow = numbers[0];
            style.flex_shrink = numbers[1];
            style.flex_basis = b;
        }
        (0, Some(b)) => {
            // flex: <basis> only (e.g. flex: 100px)
            style.flex_basis = b;
        }
        _ => {} // invalid combinations ignored
    }
}

/// Find a `/` that is not inside parentheses.
pub(in crate::style) fn find_slash(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            '/' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Parse `grid-column` / `grid-row` shorthand: `<start> / <end>`.
pub(in crate::style) fn apply_grid_line_shorthand(val: &str, start: &mut GridLine, end: &mut GridLine) {
    let trimmed = val.trim();
    if let Some(pos) = trimmed.find('/') {
        let s = trimmed[..pos].trim();
        let e = trimmed[pos + 1..].trim();
        if let Some(v) = GridLine::parse(s) {
            *start = v;
        }
        if let Some(v) = GridLine::parse(e) {
            *end = v;
        }
    } else if let Some(v) = GridLine::parse(trimmed) {
        *start = v.clone();
        // end stays Auto per spec when only start provided
        let _ = end; // keep lint quiet
    }
}

/// Parse `grid-area` shorthand: `row-start / col-start / row-end / col-end`.
///
/// CSS Grid L1 §8.3: when only a single `<custom-ident>` is provided
/// (not `auto`, not an integer, not `span`), it is a named area reference —
/// all four grid-line properties are set to `Named(ident)` and resolved at
/// layout time against the parent's `grid-template-areas`.
pub(in crate::style) fn apply_grid_area_shorthand(val: &str, style: &mut ComputedStyle) {
    let parts: Vec<&str> = val.split('/').map(str::trim).collect();
    match parts.as_slice() {
        [single] => {
            if let Some(v) = GridLine::parse(single) {
                // Single named area: propagate to all four placement properties.
                match &v {
                    GridLine::Named(_) => {
                        style.grid_row_start = v.clone();
                        style.grid_row_end = v.clone();
                        style.grid_column_start = v.clone();
                        style.grid_column_end = v;
                    }
                    _ => {
                        style.grid_row_start = v;
                    }
                }
            }
        }
        [rs, cs] => {
            if let Some(v) = GridLine::parse(rs) { style.grid_row_start = v; }
            if let Some(v) = GridLine::parse(cs) { style.grid_column_start = v; }
        }
        [rs, cs, re] => {
            if let Some(v) = GridLine::parse(rs) { style.grid_row_start = v; }
            if let Some(v) = GridLine::parse(cs) { style.grid_column_start = v; }
            if let Some(v) = GridLine::parse(re) { style.grid_row_end = v; }
        }
        [rs, cs, re, ce] => {
            if let Some(v) = GridLine::parse(rs) { style.grid_row_start = v; }
            if let Some(v) = GridLine::parse(cs) { style.grid_column_start = v; }
            if let Some(v) = GridLine::parse(re) { style.grid_row_end = v; }
            if let Some(v) = GridLine::parse(ce) { style.grid_column_end = v; }
        }
        _ => {}
    }
}
