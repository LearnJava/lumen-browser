//! Разбор сторон бокса: сеттеры `margin`/`inset`/`padding`, шортхенды
//! `margin`/`padding`/`border`/`outline`, ширины и стили рамки, `border-radius`,
//! scroll-snap, `overscroll-behavior`, `break-*` и функции якорного
//! позиционирования `anchor()`/`anchor-size()`.
//!
//! Перенесено батчем SPLIT-ST4 из `crates/engine/layout/src/style.rs`
//! (анкеры `fn resolve_box_length` … `fn split_radius_pair`) без правок тел:
//! изменены только пути модулей и видимость тех items, которые продолжает
//! звать `style.rs` и его тест-модули.

use lumen_core::geom::Size;

use crate::style::parse::color::{parse_color_legacy, parse_css_color_legacy};
use crate::style::{
    BorderStyle, BreakValue, ComputedStyle, CssColor, Length, LengthOrAuto, OutlineColor,
    OutlineStyle, OverscrollBehavior, ScrollSnapAlign, ScrollSnapAlignKeyword, ScrollSnapAxis,
    ScrollSnapStrictness, ScrollSnapType, parse_length_q,
};

/// Резолвит длину для margin / padding / border. `%` в Phase 0 не поддержан
/// (нужна containing-block-width), возвращает None.
pub(in crate::style) fn resolve_box_length(val: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<f32> {
    let len = parse_length_q(val, is_quirks)?;
    match len {
        Length::Percent(_) => None,
        other => other.resolve(em_basis, None, viewport),
    }
}

/// Resolves an SVG geometric length (`stroke-width`, `stroke-dasharray` items,
/// `stroke-dashoffset`) to px. SVG presentation properties accept a bare
/// **unitless** number as a user-unit length (≡ px) regardless of the document's
/// HTML quirks/standards mode (SVG 2 §7.10: SVG geometry `<length>` is extended
/// with `<number>`). `parse_length_q`/`resolve_box_length` reject unitless
/// non-zero numbers in standards mode, which silently dropped `stroke-width="20"`
/// on standards-mode pages (BUG-102) — every `<path>` painted at the inherited
/// default width of 1px. Fall back to parsing the bare number as px.
/// `%` stays unsupported (Lumen does not yet resolve it against the viewport
/// diagonal), matching prior behaviour.
pub(in crate::style) fn resolve_svg_length(val: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<f32> {
    let t = val.trim();
    if let Some(v) = resolve_box_length(t, em_basis, viewport, is_quirks) {
        return Some(v);
    }
    t.parse::<f32>().ok()
}

/// Парсит значение border-radius. Абсолютные единицы (px, em, rem, vw, vh и т.д.)
/// резолвятся в `Length::Px` сразу. `%` сохраняется как `Length::Percent` — резолвинг
/// откладывается до момента рисования, когда известен border-box (CSS Backgrounds L3 §5.5).
pub(in crate::style) fn parse_radius_length(val: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<Length> {
    let len = parse_length_q(val, is_quirks)?;
    match len {
        Length::Percent(p) => Some(Length::Percent(p.max(0.0))),
        other => other.resolve(em_basis, None, viewport).map(|v| Length::Px(v.max(0.0))),
    }
}

/// Устанавливает одну сторону margin как typed `LengthOrAuto`.
/// `auto` → `Auto`; length → `Length(...)`.
pub(in crate::style) fn set_margin_side(target: &mut LengthOrAuto, val: &str, is_quirks: bool) {
    if val.trim() == "auto" {
        *target = LengthOrAuto::Auto;
    } else if let Some(len) = parse_length_q(val, is_quirks) {
        *target = LengthOrAuto::Length(len);
    }
}

/// Sets one inset side (`top`/`right`/`bottom`/`left`), intercepting the CSS
/// Anchor Positioning L1 `anchor()` function before falling back to a normal
/// length/`auto` value. Mirrors the `anchor-size()` interception used for
/// `width`/`height` (see `parse_anchor_size_func`).
pub(in crate::style) fn set_inset_side(
    target: &mut LengthOrAuto,
    anchor_target: &mut Option<crate::anchor::AnchorFunc>,
    val: &str,
    is_quirks: bool,
) {
    if let Some(func) = parse_anchor_func(val, is_quirks) {
        *anchor_target = Some(func);
    } else {
        set_margin_side(target, val, is_quirks);
        *anchor_target = None;
    }
}

/// Устанавливает одну сторону padding как typed `Length`.
/// `auto` не валиден для padding — игнорируем; отрицательные px — игнорируем.
pub(in crate::style) fn set_padding_side(target: &mut Length, val: &str, is_quirks: bool) {
    if let Some(len) = parse_length_q(val, is_quirks) {
        if matches!(&len, Length::Px(v) if *v < 0.0) { return; }
        *target = len;
    }
}

/// Токенизирует CSS box shorthand значение по пробелам вне скобок.
/// Нужно для `calc(5px + 3px)` — пробелы внутри calc() не разделяют tokens.
pub(in crate::style) fn split_box_tokens(val: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut depth: u32 = 0;
    let mut start: Option<usize> = None;
    for (i, ch) in val.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ' ' | '\t' if depth == 0 => {
                if let Some(s) = start {
                    tokens.push(&val[s..i]);
                    start = None;
                }
                continue;
            }
            _ => {}
        }
        if start.is_none() && (ch != ' ' && ch != '\t') {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        tokens.push(&val[s..]);
    }
    tokens
}

/// Парсит CSS `margin` shorthand — 1-4 токена. CSS 2.1 §8.3.
/// Возвращает `(top, right, bottom, left)` как `LengthOrAuto`.
pub(in crate::style) fn parse_margin_shorthand(
    val: &str,
    is_quirks: bool,
) -> Option<(LengthOrAuto, LengthOrAuto, LengthOrAuto, LengthOrAuto)> {
    let parse = |s: &str| -> Option<LengthOrAuto> {
        if s.trim() == "auto" { return Some(LengthOrAuto::Auto); }
        parse_length_q(s, is_quirks).map(LengthOrAuto::Length)
    };
    let parts = split_box_tokens(val);
    match parts.as_slice() {
        [a] => { let v = parse(a)?; Some((v.clone(), v.clone(), v.clone(), v)) }
        [tb, lr] => {
            let t = parse(tb)?; let r = parse(lr)?;
            Some((t.clone(), r.clone(), t, r))
        }
        [t, lr, b] => {
            let tv = parse(t)?; let rv = parse(lr)?; let bv = parse(b)?;
            Some((tv, rv.clone(), bv, rv))
        }
        [t, r, b, l] => {
            Some((parse(t)?, parse(r)?, parse(b)?, parse(l)?))
        }
        _ => None,
    }
}

/// Парсит CSS `padding` shorthand — 1-4 токена. Аналогично margin.
/// `auto` не валиден; отрицательные px тоже. При ошибке — None.
pub(in crate::style) fn parse_padding_shorthand(val: &str, is_quirks: bool) -> Option<(Length, Length, Length, Length)> {
    let parse = |s: &str| -> Option<Length> {
        let len = parse_length_q(s, is_quirks)?;
        if matches!(&len, Length::Px(v) if *v < 0.0) { return None; }
        Some(len)
    };
    let parts = split_box_tokens(val);
    match parts.as_slice() {
        [a] => { let v = parse(a)?; Some((v.clone(), v.clone(), v.clone(), v)) }
        [tb, lr] => {
            let t = parse(tb)?; let r = parse(lr)?;
            Some((t.clone(), r.clone(), t, r))
        }
        [t, lr, b] => {
            let tv = parse(t)?; let rv = parse(lr)?; let bv = parse(b)?;
            Some((tv, rv.clone(), bv, rv))
        }
        [t, r, b, l] => {
            Some((parse(t)?, parse(r)?, parse(b)?, parse(l)?))
        }
        _ => None,
    }
}

fn is_border_style_kw(s: &str) -> bool {
    matches!(s.trim(), "none" | "solid" | "dashed" | "dotted" | "double")
}

pub(in crate::style) fn parse_border_style_kw(s: &str) -> BorderStyle {
    match s.trim() {
        "solid" => BorderStyle::Solid,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        "double" => BorderStyle::Double,
        _ => BorderStyle::None,
    }
}

pub(in crate::style) fn parse_border_style_opt(s: &str) -> Option<BorderStyle> {
    match s.trim() {
        "none" => Some(BorderStyle::None),
        "solid" => Some(BorderStyle::Solid),
        "dashed" => Some(BorderStyle::Dashed),
        "dotted" => Some(BorderStyle::Dotted),
        "double" => Some(BorderStyle::Double),
        _ => None,
    }
}

/// CSS Backgrounds L3 §4.2 / Basic UI L4 §5.2 — `<line-width>` =
/// `<length> | thin | medium | thick`. UA convention: thin=1, medium=3,
/// thick=5 (Chromium/Firefox/WebKit совпадают).
pub(in crate::style) fn parse_line_width(val: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<f32> {
    match val.trim() {
        s if s.eq_ignore_ascii_case("thin") => Some(1.0),
        s if s.eq_ignore_ascii_case("medium") => Some(3.0),
        s if s.eq_ignore_ascii_case("thick") => Some(5.0),
        other => resolve_box_length(other, em_basis, viewport, is_quirks),
    }
}

/// CSS Basic UI L4 §5.3 — `outline-style: auto | <'border-style'>`. Возвращает
/// `None` для невалидного токена, чтобы caller мог попробовать его как
/// width/color в shorthand.
pub(in crate::style) fn parse_outline_style_opt(s: &str) -> Option<OutlineStyle> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("auto") {
        return Some(OutlineStyle::Auto);
    }
    match parse_border_style_opt(s)? {
        BorderStyle::None => Some(OutlineStyle::None),
        BorderStyle::Solid | BorderStyle::Double => Some(OutlineStyle::Solid),
        BorderStyle::Dashed => Some(OutlineStyle::Dashed),
        BorderStyle::Dotted => Some(OutlineStyle::Dotted),
    }
}

/// CSS Basic UI L4 §5.4 — `outline-color: auto | <color>`. `currentcolor`
/// — это CSS Color L3 keyword, выделяется в отдельный variant, чтобы
/// renderer мог разрешить его в момент paint (а не подмёшивать
/// `style.color` на этапе cascade — последнее ломает наследование при
/// последующем изменении `color`).
pub(in crate::style) fn parse_outline_color_opt(s: &str, is_quirks: bool) -> Option<OutlineColor> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("auto") {
        return Some(OutlineColor::Auto);
    }
    if s.eq_ignore_ascii_case("currentcolor") {
        return Some(OutlineColor::CurrentColor);
    }
    parse_color_legacy(s, is_quirks).map(OutlineColor::Color)
}

/// Расширяет 1-4 значения `Vec<f32>` в (top, right, bottom, left) по
/// стандартному CSS-правилу (1 значение → все четыре, 2 значения → v-h,
/// 3 значения → top-h-bottom, 4 значения — TRBL).
pub(in crate::style) fn expand_4_sides(parts: &[f32]) -> (f32, f32, f32, f32) {
    match parts.len() {
        1 => (parts[0], parts[0], parts[0], parts[0]),
        2 => (parts[0], parts[1], parts[0], parts[1]),
        3 => (parts[0], parts[1], parts[2], parts[1]),
        _ if parts.len() >= 4 => (parts[0], parts[1], parts[2], parts[3]),
        _ => (0.0, 0.0, 0.0, 0.0),
    }
}

pub(in crate::style) fn parse_scroll_snap_type(s: &str) -> Option<ScrollSnapType> {
    let s = s.trim().to_ascii_lowercase();
    if s == "none" {
        return Some(ScrollSnapType::default());
    }
    let mut axis = ScrollSnapAxis::None;
    let mut strict = ScrollSnapStrictness::Proximity;
    for tok in s.split_whitespace() {
        match tok {
            "x" => axis = ScrollSnapAxis::X,
            "y" => axis = ScrollSnapAxis::Y,
            "block" => axis = ScrollSnapAxis::Block,
            "inline" => axis = ScrollSnapAxis::Inline,
            "both" => axis = ScrollSnapAxis::Both,
            "mandatory" => strict = ScrollSnapStrictness::Mandatory,
            "proximity" => strict = ScrollSnapStrictness::Proximity,
            _ => {}
        }
    }
    Some(ScrollSnapType {
        axis,
        strictness: strict,
    })
}

pub(in crate::style) fn parse_scroll_snap_align(s: &str) -> Option<ScrollSnapAlign> {
    let parts: Vec<ScrollSnapAlignKeyword> = s
        .split_whitespace()
        .map(|p| match p.to_ascii_lowercase().as_str() {
            "none" => ScrollSnapAlignKeyword::None,
            "start" => ScrollSnapAlignKeyword::Start,
            "end" => ScrollSnapAlignKeyword::End,
            "center" => ScrollSnapAlignKeyword::Center,
            _ => ScrollSnapAlignKeyword::None,
        })
        .collect();
    match parts.len() {
        1 => Some(ScrollSnapAlign {
            block: parts[0],
            inline: parts[0],
        }),
        2 => Some(ScrollSnapAlign {
            block: parts[0],
            inline: parts[1],
        }),
        _ => None,
    }
}

pub(in crate::style) fn parse_overscroll_behavior(s: &str) -> Option<OverscrollBehavior> {
    match s.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(OverscrollBehavior::Auto),
        "contain" => Some(OverscrollBehavior::Contain),
        "none" => Some(OverscrollBehavior::None),
        _ => None,
    }
}

/// Парсит CSS Fragmentation L3 §3.1 `break-*` keyword.
/// CSS Anchor Positioning L1 §5 — parses a single `inset-area` axis keyword.
pub(in crate::style) fn parse_inset_area_keyword(s: &str) -> Option<crate::anchor::InsetAreaKeyword> {
    use crate::anchor::InsetAreaKeyword as K;
    match s.trim().to_ascii_lowercase().as_str() {
        "none"       => Some(K::None),
        "start"      => Some(K::Start),
        "center"     => Some(K::Center),
        "end"        => Some(K::End),
        "span-start" => Some(K::SpanStart),
        "span-end"   => Some(K::SpanEnd),
        "span-all"   => Some(K::SpanAll),
        "self-start" => Some(K::SelfStart),
        "self-end"   => Some(K::SelfEnd),
        // Physical keywords that map to logical equivalents in LTR.
        "top" | "left"   => Some(K::Start),
        "bottom" | "right" => Some(K::End),
        "x-start" | "y-start" | "inline-start" | "block-start" => Some(K::Start),
        "x-end" | "y-end" | "inline-end" | "block-end" => Some(K::End),
        "span-x-start" | "span-y-start" | "span-inline-start" | "span-block-start" => Some(K::SpanStart),
        "span-x-end" | "span-y-end" | "span-inline-end" | "span-block-end" => Some(K::SpanEnd),
        _ => None,
    }
}

/// CSS Anchor Positioning L1 §4 — parse `anchor-size(<anchor-el>? <anchor-size>)`.
///
/// Accepts forms:
/// - `anchor-size(width)` / `anchor-size(height)` / `anchor-size(block)` / etc.
/// - `anchor-size(--name, width)` / `anchor-size(--name, height)` / etc.
///
/// Returns `None` when `val` is not an `anchor-size()` expression.
pub(in crate::style) fn parse_anchor_size_func(val: &str) -> Option<crate::anchor::AnchorSizeFunc> {
    use crate::anchor::{AnchorSizeDimension, AnchorSizeFunc};
    let v = val.trim();
    let inner = v.strip_prefix("anchor-size(")?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.splitn(2, ',').map(str::trim).collect();
    let (anchor_name, dim_str) = if parts.len() == 2 {
        let name = parts[0];
        let anchor_name = if name.starts_with("--") { Some(name.into()) } else { return None };
        (anchor_name, parts[1])
    } else {
        (None, parts[0])
    };
    let dimension = match dim_str.to_ascii_lowercase().as_str() {
        "width"       => AnchorSizeDimension::Width,
        "height"      => AnchorSizeDimension::Height,
        "block"       => AnchorSizeDimension::Block,
        "inline"      => AnchorSizeDimension::Inline,
        "self-block"  => AnchorSizeDimension::SelfBlock,
        "self-inline" => AnchorSizeDimension::SelfInline,
        _ => return None,
    };
    Some(AnchorSizeFunc { anchor_name, dimension })
}

/// CSS Anchor Positioning L1 §3.1 — parse `anchor(<anchor-el>? <anchor-side>, <fallback>?)`.
///
/// Accepts forms:
/// - `anchor(top)` / `anchor(50%)` / `anchor(start)` — anchor-side only, uses the
///   element's `position-anchor` default anchor.
/// - `anchor(--name top)` / `anchor(--name 25%)` — explicit anchor-element argument.
/// - `anchor(top, 10px)` / `anchor(--name left, 1em)` — trailing `<length-percentage>`
///   fallback, used when the anchor can't be resolved.
///
/// Returns `None` when `val` is not an `anchor()` expression.
fn parse_anchor_func(val: &str, is_quirks: bool) -> Option<crate::anchor::AnchorFunc> {
    use crate::anchor::AnchorFunc;
    let v = val.trim();
    let inner = v.strip_prefix("anchor(")?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.splitn(2, ',').map(str::trim).collect();
    let fallback = match parts.get(1) {
        Some(fb) => Some(parse_length_q(fb, is_quirks)?),
        None => None,
    };
    let head_parts: Vec<&str> = parts[0].split_whitespace().collect();
    let (anchor_name, side_str) = match head_parts.as_slice() {
        [side] => (None, *side),
        [name, side] if name.starts_with("--") => (Some((*name).into()), *side),
        _ => return None,
    };
    let side = parse_anchor_side(side_str)?;
    Some(AnchorFunc { anchor_name, side, fallback })
}

/// CSS Anchor Positioning L1 §3.1 — parse a single `<anchor-side>` keyword or
/// `<percentage>`.
fn parse_anchor_side(s: &str) -> Option<crate::anchor::AnchorSide> {
    use crate::anchor::AnchorSide;
    match s.trim().to_ascii_lowercase().as_str() {
        "top" => Some(AnchorSide::Top),
        "right" => Some(AnchorSide::Right),
        "bottom" => Some(AnchorSide::Bottom),
        "left" => Some(AnchorSide::Left),
        "center" => Some(AnchorSide::Center),
        "start" => Some(AnchorSide::Start),
        "end" => Some(AnchorSide::End),
        other => other.strip_suffix('%')?.trim().parse::<f32>().ok().map(AnchorSide::Percentage),
    }
}

pub(in crate::style) fn parse_break_value(s: &str) -> Option<BreakValue> {
    match s.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(BreakValue::Auto),
        "avoid" | "avoid-page" | "avoid-column" | "avoid-region" => Some(BreakValue::Avoid),
        "always" => Some(BreakValue::Always),
        "page" => Some(BreakValue::Page),
        "column" => Some(BreakValue::Column),
        "region" => Some(BreakValue::Region),
        _ => None,
    }
}

/// Разбирает `border: <width> <style> <color>` (порядок произвольный, каждая
/// часть опциональна). Применяет найденные значения ко всем четырём сторонам.
pub(in crate::style) fn apply_border_shorthand(style: &mut ComputedStyle, val: &str, em_basis: f32, viewport: Size, is_quirks: bool) {
    let tokens: Vec<&str> = val.split_whitespace().collect();
    for tok in &tokens {
        if let Some(v) = resolve_box_length(tok, em_basis, viewport, is_quirks) {
            style.border_top_width = v;
            style.border_right_width = v;
            style.border_bottom_width = v;
            style.border_left_width = v;
        } else if is_border_style_kw(tok) {
            let bs = parse_border_style_kw(tok);
            style.border_top_style = bs;
            style.border_right_style = bs;
            style.border_bottom_style = bs;
            style.border_left_style = bs;
        } else if let Some(c) = parse_css_color_legacy(tok, is_quirks) {
            style.border_top_color = c;
            style.border_right_color = c;
            style.border_bottom_color = c;
            style.border_left_color = c;
        }
    }
}

/// Разбирает `border-{top,right,bottom,left}: <width> <style> <color>` в одну сторону.
pub(in crate::style) fn apply_border_side_shorthand(
    width: &mut f32,
    bstyle: &mut BorderStyle,
    color: &mut CssColor,
    val: &str,
    em_basis: f32,
    viewport: Size,
    is_quirks: bool,
) {
    for tok in val.split_whitespace() {
        if let Some(v) = resolve_box_length(tok, em_basis, viewport, is_quirks) {
            *width = v;
        } else if is_border_style_kw(tok) {
            *bstyle = parse_border_style_kw(tok);
        } else if let Some(c) = parse_css_color_legacy(tok, is_quirks) {
            *color = c;
        }
    }
}

/// Разворачивает 1–4 токена в 4-элементный массив по CSS-правилу:
/// 1 → (T, R, B, L) = all same
/// 2 → (T=B, R=L)
/// 3 → (T, R=L, B)
/// 4 → (T, R, B, L)
pub(in crate::style) fn expand_border_4(val: &str) -> [&str; 4] {
    let parts: Vec<&str> = val.split_whitespace().collect();
    match parts.len() {
        // Пустое (или состоящее из одних пробелов) значение — невалидное
        // объявление вида `border-radius: ;`. React-приложения пишут такие
        // пустышки в inline-стиль пачками (`style="height: ; color: ;"`) для
        // «неустановленных» пропсов, поэтому это не экзотика: до BUG-724 ветка
        // `_` индексировала пустой `parts` и роняла поток `lumen-engine`
        // целиком. Четыре пустых токена ниже не парсятся ни одним из
        // потребителей — объявление игнорируется, как и требует CSS.
        0 => [val; 4],
        1 => [parts[0], parts[0], parts[0], parts[0]],
        2 => [parts[0], parts[1], parts[0], parts[1]],
        3 => [parts[0], parts[1], parts[2], parts[1]],
        _ => {
            let t = parts[0];
            let r = parts.get(1).copied().unwrap_or(t);
            let b = parts.get(2).copied().unwrap_or(t);
            let l = parts.get(3).copied().unwrap_or(r);
            [t, r, b, l]
        }
    }
}

/// CSS Backgrounds L3 §5.5: splits `border-radius` value at the `/` separator
/// that divides horizontal from vertical radii. The `/` must be surrounded by
/// whitespace-separated tokens (e.g. `10px 20px / 5px`). Returns
/// `(horizontal_part, Some(vertical_part))` when `/` is present, else
/// `(full_value, None)`.
pub(in crate::style) fn split_border_radius_slash(val: &str) -> (&str, Option<&str>) {
    // Find `/` that is not inside parentheses (e.g. `calc(1/2)` must not split).
    let mut depth = 0u32;
    let bytes = val.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b'/' if depth == 0 => {
                let h = val[..i].trim();
                let v = val[i + 1..].trim();
                return (h, Some(v));
            }
            _ => {}
        }
    }
    (val.trim(), None)
}

/// CSS Backgrounds L3 §5.5: individual corner `border-*-*-radius` accepts
/// one or two `<length-percentage>` values: `rx [ry]`. Returns `(rx, Some(ry))`
/// or `(rx, None)` when only one value.
pub(in crate::style) fn split_radius_pair(val: &str) -> (&str, Option<&str>) {
    let mut parts = val.split_whitespace();
    let rx = parts.next().unwrap_or(val);
    let ry = parts.next();
    (rx, ry)
}
