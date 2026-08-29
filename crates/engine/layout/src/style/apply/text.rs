//! Шрифт и текст — ветки `match prop` функции `apply_declaration`.
//!
//! `font-*`, выключка и переносы, режим письма и ruby, MathML, list-style и
//! счётчики, `content`, `text-decoration`/`text-emphasis`, SVG-выключка базовой
//! линии.
//!
//! Перенесено батчем SPLIT-ST8 из `crates/engine/layout/src/style.rs`: тела
//! веток скопированы побайтово, изменены только пути импортов и форма выхода
//! (`return` → `return true`, см. шапку `style/apply.rs`). Метка ветки в
//! группу не входит по алфавиту, а по смыслу — порядок веток внутри `match`
//! семантики не несёт, потому что все метки уникальны.

use crate::mathml::MathStyle;
use crate::ruby::{RubyAlign, RubyMerge, RubyPosition};
use crate::style::{
    ComputedStyle,
    Content,
    CssColor,
    Direction,
    FontOpticalSizing,
    FontSizeAdjust,
    FontStretch,
    FontStyle,
    FontVariantCaps,
    FontVariantEmoji,
    FontWeight,
    Hyphens,
    Length,
    LineBreak,
    ListStylePosition,
    ListStyleType,
    OverflowWrap,
    ROOT_FONT_SIZE,
    TextAlign,
    TextAlignLast,
    TextDecorationSkipInk,
    TextDecorationStyle,
    TextOrientation,
    TextOverflow,
    TextTransform,
    TextUnderlinePosition,
    TextWrapMode,
    TextWrapStyle,
    UserSelect,
    VerticalAlign,
    WhiteSpace,
    WhiteSpaceCollapse,
    WordBreak,
    WritingMode,
    match_unicode_bidi,
    parse_font_family,
    parse_font_feature_settings,
    parse_font_palette,
    parse_font_variation_settings,
    parse_font_weight,
    parse_initial_letter,
    parse_length,
    parse_length_q,
    parse_text_shadow_one,
    split_top_level_commas,
};
use crate::style::parse::box_sides::{resolve_box_length, resolve_svg_length};
use crate::style::parse::color::parse_css_color_legacy;
use crate::style::parse::content::parse_content_items;
use crate::style::parse::counters::{parse_counter_list, parse_quotes};
use crate::style::parse::font_size::{apply_line_height_value, parse_font_shorthand};
use crate::style::parse::image::parse_url_value;
use crate::style::parse::transform::parse_length_px;
use crate::style::shorthand::{
    apply_text_emphasis_shorthand,
    apply_text_wrap_shorthand,
    parse_text_decoration_shorthand_q,
    parse_text_decoration_thickness,
    parse_text_emphasis_position,
    parse_text_emphasis_style,
};
use lumen_core::geom::Size;

/// Применить одну декларацию из группы «шрифт и текст».
///
/// Возвращает `true`, если свойство принадлежит этой группе и было обработано;
/// `false` — если метка не наша и декларацию нужно предложить следующему
/// помощнику в цепочке `apply_declaration`.
#[allow(clippy::too_many_arguments)]
pub(in crate::style) fn apply_decl_text(
    style: &mut ComputedStyle,
    prop: &str,
    val: &str,
    em_basis: f32,
    viewport: Size,
    parent_font_weight: FontWeight,
    inherited: &ComputedStyle,
    is_quirks: bool,
) -> bool {
    match prop {
        "text-align" => {
            style.text_align = match val {
                "start" => TextAlign::Start,
                "end" => TextAlign::End,
                "left" => TextAlign::Left,
                "center" => TextAlign::Center,
                "right" => TextAlign::Right,
                _ => style.text_align,
            };
        }
        "text-align-last" => {
            style.text_align_last = match val.trim() {
                "auto" => TextAlignLast::Auto,
                "start" => TextAlignLast::Start,
                "end" => TextAlignLast::End,
                "left" => TextAlignLast::Left,
                "right" => TextAlignLast::Right,
                "center" => TextAlignLast::Center,
                "justify" => TextAlignLast::Justify,
                _ => style.text_align_last,
            };
        }
        "direction" => {
            // CSS Writing Modes L3 §2.1. Keyword-ы case-insensitive по
            // правилам CSS («property keyword values are ASCII case-
            // insensitive», CSS Values L4 §2.4). Невалидное значение
            // оставляет inherited (или предыдущее) направление.
            if val.eq_ignore_ascii_case("ltr") {
                style.direction = Direction::Ltr;
            } else if val.eq_ignore_ascii_case("rtl") {
                style.direction = Direction::Rtl;
            }
        }
        "unicode-bidi" => {
            // CSS Writing Modes L4 §2.2. Legacy `-webkit-`/`-moz-` префиксы
            // изолятов принимаются как алиасы — их до сих пор пишут в CSS
            // локализованных страниц. Невалидное значение оставляет прежнее.
            let v = val.trim();
            if let Some(parsed) = match_unicode_bidi(v) {
                style.unicode_bidi = parsed;
            }
        }
        "vertical-align" => {
            // CSS 2.1 §10.8.1: keyword | <percentage> | <length>.
            // Keyword первым (text-top / text-bottom — двусоставные, не
            // конфликтуют с length-парсером); затем length: Percent сохраняем
            // как Percent (резолвится по line-height в layout-pass), остальные
            // резолвим к px относительно текущего font-size.
            if let Some(va) = VerticalAlign::parse_keyword(val) {
                style.vertical_align = va;
            } else if let Some(len) = parse_length_q(val, is_quirks) {
                match len {
                    Length::Percent(p) => style.vertical_align = VerticalAlign::Percent(p),
                    other => {
                        if let Some(px) = other.resolve(em_basis, None, viewport) {
                            style.vertical_align = VerticalAlign::Length(px);
                        }
                    }
                }
            }
        }
        "text-wrap-mode" => {
            // CSS Text Module Level 4 §6.4.1: wrap | nowrap. Inherited.
            // Пересчитываем эффективное white_space (L4 §2.1: white-space —
            // shorthand над white-space-collapse + text-wrap-mode).
            if let Some(v) = TextWrapMode::parse(val) {
                style.text_wrap_mode = v;
                style.white_space = WhiteSpace::combine(style.white_space_collapse, v);
            }
        }
        "text-wrap-style" => {
            // CSS Text Module Level 4 §6.4.2: auto | balance | stable | pretty. Inherited.
            if let Some(v) = TextWrapStyle::parse(val) {
                style.text_wrap_style = v;
            }
        }
        "text-wrap" => {
            // CSS Text Module Level 4 §6.4.3: shorthand для text-wrap-mode и
            // text-wrap-style; синтаксис `<'text-wrap-mode'> || <'text-wrap-style'>`.
            // 1 или 2 идентификатора, любой порядок. Каждый shorthand сбрасывает
            // обе longhand-компоненты к initial-value, после чего применяются
            // указанные. См. CSS Cascade L4 §3.1 для семантики shorthand reset.
            apply_text_wrap_shorthand(style, val);
        }
        "font-style" => {
            // CSS Fonts L4 — normal | italic | oblique. Прочее (`oblique 10deg`,
            // `oblique -5deg`) пока не поддерживаем — берём как oblique.
            style.font_style = match val.split_whitespace().next() {
                Some("italic") => FontStyle::Italic,
                Some("oblique") => FontStyle::Oblique,
                Some("normal") => FontStyle::Normal,
                _ => style.font_style,
            };
        }
        "font-weight" => {
            if let Some(w) = parse_font_weight(val, parent_font_weight) {
                style.font_weight = w;
            }
        }
        "font-family" => {
            let list = parse_font_family(val);
            if !list.is_empty() {
                style.font_family = list;
            }
        }
        "font" => {
            // CSS Fonts L4 §6.10 — `font` shorthand (BUG-114). `<font-size>`
            // уже резолвлен в pre-pass (`apply_font_size`); здесь применяем
            // остальные longhand-ы. Shorthand сбрасывает ВСЕ управляемые
            // longhand-ы в initial (CSS Cascade L4 §3.1), затем выставляет
            // явно заданные компоненты.
            if let Some(parts) = parse_font_shorthand(val) {
                style.font_style = parts.style.unwrap_or(FontStyle::Normal);
                style.font_variant_caps =
                    if parts.small_caps { FontVariantCaps::SmallCaps } else { FontVariantCaps::Normal };
                // `font` сбрасывает ВСЕ longhand-ы `font-variant`, включая те,
                // что сам выразить не может (§6.10) — emoji-компоненту тоже.
                style.font_variant_emoji = FontVariantEmoji::Normal;
                style.font_weight = parts
                    .weight
                    .as_deref()
                    .and_then(|w| parse_font_weight(w, parent_font_weight))
                    .unwrap_or(FontWeight::NORMAL);
                style.font_stretch = parts.stretch.unwrap_or(FontStretch::NORMAL);
                // line-height: initial `normal` ≈ 1.2 relative (как в root()).
                style.line_height = 1.2;
                style.line_height_is_relative = true;
                if let Some(lh) = parts.line_height.as_deref()
                    && lh != "normal"
                {
                    apply_line_height_value(style, lh, em_basis, viewport);
                }
                let fam = parse_font_family(&parts.family);
                if !fam.is_empty() {
                    style.font_family = fam;
                }
            }
        }
        "font-variation-settings" => {
            if let Some(v) = parse_font_variation_settings(val) {
                style.font_variation_settings = v;
            }
        }
        "font-feature-settings" => {
            if let Some(v) = parse_font_feature_settings(val) {
                style.font_feature_settings = v;
            }
        }
        "font-palette" => {
            if let Some(v) = parse_font_palette(val) {
                style.font_palette = v;
            }
        }
        "font-optical-sizing" => {
            style.font_optical_sizing = match val {
                "auto" => FontOpticalSizing::Auto,
                "none" => FontOpticalSizing::None,
                _ => style.font_optical_sizing,
            };
        }
        "font-variant-caps" => {
            // CSS Fonts L4 §6.2 — longhand: ровно один keyword.
            if let Some(caps) = FontVariantCaps::from_keyword(val.trim()) {
                style.font_variant_caps = caps;
            }
        }
        "font-variant-emoji" => {
            // CSS Fonts L4 §6.6 — longhand: ровно один keyword.
            if let Some(v) = FontVariantEmoji::from_keyword(val.trim()) {
                style.font_variant_emoji = v;
            }
        }
        "font-variant" => {
            // CSS Fonts L4 §6.10 — shorthand над font-variant-{caps,ligatures,
            // numeric,east-asian,position,alternates,emoji}. Реализованы только
            // caps- и emoji-компоненты, но сбросить их обязан любой валидный
            // shorthand (CSS Cascade L4 §3.1): `font-variant: common-ligatures`
            // должен вернуть caps в initial, а не оставить унаследованное
            // small-caps. `none` (отключение лигатур) и любые нереализованные
            // keyword-ы этих компонент не содержат — значит они в initial.
            style.font_variant_caps = val
                .split_whitespace()
                .find_map(FontVariantCaps::from_keyword)
                .unwrap_or(FontVariantCaps::Normal);
            // `normal` в shorthand-е принадлежит caps-компоненте (она стоит
            // первой в грамматике), поэтому emoji-компоненту ищем только среди
            // её собственных keyword-ов — иначе `font-variant: small-caps`
            // ничего бы не сбросил, а `font-variant: normal` совпал бы дважды.
            style.font_variant_emoji = val
                .split_whitespace()
                .find_map(|kw| match kw {
                    "text" | "emoji" | "unicode" => FontVariantEmoji::from_keyword(kw),
                    _ => None,
                })
                .unwrap_or(FontVariantEmoji::Normal);
        }
        "font-stretch" => {
            if let Some(fs) = FontStretch::parse(val) {
                style.font_stretch = fs;
            }
        }
        "text-indent" => {
            // CSS Text L3 §7.1: <length> | <percentage>. `%` теперь хранится
            // typed — резолвится при layout с known cb_width.
            if let Some(len) = parse_length_q(val, is_quirks) {
                style.text_indent = len;
            }
        }
        "letter-spacing" => {
            // CSS Text L3 §11.2: normal (= 0) | <length>. `%` не валиден.
            // Резолвим сразу — em_basis уже известен на каскаде.
            if val.trim() == "normal" {
                style.letter_spacing = 0.0;
            } else if let Some(len) = parse_length_q(val, is_quirks)
                && let Some(px) = len.resolve(em_basis, None, viewport)
            {
                style.letter_spacing = px;
            }
        }
        "word-spacing" => {
            // CSS Text L3 §11.3: normal (= 0) | <length>. `%` требует
            // метрику space-glyph — Phase 0 не считаем, игнорируем.
            if val.trim() == "normal" {
                style.word_spacing = 0.0;
            } else if let Some(px) = parse_length_q(val, is_quirks).and_then(|len| match len {
                Length::Percent(_) => None,
                other => other.resolve(em_basis, None, viewport),
            }) {
                style.word_spacing = px;
            }
        }
        "text-transform" => {
            // CSS Text L3: none | uppercase | lowercase | capitalize.
            // `full-width` / `full-size-kana` отложены (CJK-специфика).
            style.text_transform = match val.split_whitespace().next() {
                Some("none") => TextTransform::None,
                Some("uppercase") => TextTransform::Uppercase,
                Some("lowercase") => TextTransform::Lowercase,
                Some("capitalize") => TextTransform::Capitalize,
                _ => style.text_transform,
            };
        }
        "white-space" => {
            // CSS Text L4 §2.1: shorthand над white-space-collapse и
            // text-wrap-mode — раскладываем на обе longhand-компоненты,
            // чтобы каскад последующих longhand-ов пересчитывал корректно.
            let parsed = match val.trim() {
                "normal" => Some(WhiteSpace::Normal),
                "nowrap" => Some(WhiteSpace::Nowrap),
                "pre" => Some(WhiteSpace::Pre),
                "pre-wrap" => Some(WhiteSpace::PreWrap),
                "pre-line" => Some(WhiteSpace::PreLine),
                "break-spaces" => Some(WhiteSpace::BreakSpaces),
                _ => None,
            };
            if let Some(ws) = parsed {
                style.white_space = ws;
                style.white_space_collapse = ws.collapse_component();
                style.text_wrap_mode = ws.wrap_component();
            }
        }
        "white-space-collapse" => {
            // CSS Text L4 §3.1: longhand; эффективное white_space
            // пересчитывается из пары (collapse, text-wrap-mode).
            if let Some(v) = WhiteSpaceCollapse::parse(val) {
                style.white_space_collapse = v;
                style.white_space = WhiteSpace::combine(v, style.text_wrap_mode);
            }
        }
        "text-overflow" => {
            // CSS UI L4: clip | ellipsis. <string> (custom marker) и
            // two-value формы не поддерживаем в Phase 0.
            style.text_overflow = match val.split_whitespace().next() {
                Some("clip") => TextOverflow::Clip,
                Some("ellipsis") => TextOverflow::Ellipsis,
                _ => style.text_overflow,
            };
        }
        "-webkit-line-clamp" | "line-clamp" => {
            // CSS Overflow L4 §13.4 / compat -webkit-line-clamp.
            // Значения: `none` → None; <integer> > 0 → Some(n).
            let v = val.trim();
            style.line_clamp = if v == "none" {
                None
            } else if let Ok(n) = v.parse::<u32>() {
                if n > 0 { Some(n) } else { None }
            } else {
                style.line_clamp
            };
        }
        "text-shadow" => {
            // CSS Text Decoration L3 §4: то же что box-shadow, но без inset
            // и spread. `none` сбрасывает (важно: text-shadow inherited,
            // явное `none` нужно чтобы откатить родительское).
            if val.trim() == "none" {
                style.text_shadow = Vec::new();
            } else {
                let mut shadows = Vec::new();
                for piece in split_top_level_commas(val) {
                    if let Some(s) = parse_text_shadow_one(piece.trim(), em_basis, viewport, is_quirks) {
                        shadows.push(s);
                    }
                }
                if !shadows.is_empty() {
                    style.text_shadow = shadows;
                }
            }
        }
        "counter-reset" => {
            // CSS Lists L3 §3 — `none | (<custom-ident> <integer>?)+`.
            // Default value на счётчик при отсутствии числа = 0 (по spec).
            // `none` сбрасывает всё.
            style.counter_reset = parse_counter_list(val, 0);
        }
        "counter-increment" => {
            // CSS Lists L3 §3 — `none | (<custom-ident> <integer>?)+`.
            // Default value = 1 (по spec).
            style.counter_increment = parse_counter_list(val, 1);
        }
        "counter-set" => {
            // CSS Lists L3 §4 — `none | (<custom-ident> <integer>?)+`.
            // Default value на счётчик при отсутствии числа = 0 (по spec).
            style.counter_set = parse_counter_list(val, 0);
        }
        "quotes" => {
            // CSS Generated Content L3 §3.2 — `auto | none | [<string> <string>]+`.
            if let Some(q) = parse_quotes(val) {
                style.quotes = q;
            }
        }
        "font-size-adjust" => {
            let v = val.trim();
            style.font_size_adjust = if v.eq_ignore_ascii_case("none") {
                FontSizeAdjust::None
            } else if v.eq_ignore_ascii_case("auto") {
                FontSizeAdjust::Auto
            } else if let Ok(n) = v.parse::<f32>() {
                if n > 0.0 { FontSizeAdjust::Value(n) } else { FontSizeAdjust::None }
            } else {
                style.font_size_adjust
            };
        }
        "initial-letter" => {
            if let Some((size, sink)) = parse_initial_letter(val) {
                style.initial_letter_size = size;
                style.initial_letter_sink = sink;
            }
        }
        "writing-mode" => {
            style.writing_mode = match val.trim() {
                "horizontal-tb" => WritingMode::HorizontalTb,
                "vertical-rl" => WritingMode::VerticalRl,
                "vertical-lr" => WritingMode::VerticalLr,
                "sideways-rl" => WritingMode::SidewaysRl,
                "sideways-lr" => WritingMode::SidewaysLr,
                "lr" | "lr-tb" => WritingMode::HorizontalTb,
                "rl" | "rl-tb" => WritingMode::HorizontalTb,
                "tb" | "tb-rl" => WritingMode::VerticalRl,
                "tb-lr" => WritingMode::VerticalLr,
                _ => style.writing_mode,
            };
        }
        "text-orientation" => {
            style.text_orientation = match val.trim() {
                "mixed" => TextOrientation::Mixed,
                "upright" => TextOrientation::Upright,
                "sideways" | "sideways-right" => TextOrientation::Sideways,
                _ => style.text_orientation,
            };
        }
        "ruby-position" => {
            style.ruby_position = match val.trim() {
                // `alternate` (одиночный) по спеке ведёт себя как over.
                "over" | "alternate" => RubyPosition::Over,
                "under" => RubyPosition::Under,
                _ => style.ruby_position,
            };
        }
        "ruby-align" => {
            style.ruby_align = match val.trim() {
                "start" => RubyAlign::Start,
                "center" => RubyAlign::Center,
                "space-between" => RubyAlign::SpaceBetween,
                "space-around" => RubyAlign::SpaceAround,
                _ => style.ruby_align,
            };
        }
        "ruby-merge" => {
            style.ruby_merge = match val.trim() {
                "separate" => RubyMerge::Separate,
                "merge" => RubyMerge::Merge,
                "auto" => RubyMerge::Auto,
                _ => style.ruby_merge,
            };
        }
        "math-style" => {
            style.math_style = match val.trim() {
                "normal" => MathStyle::Normal,
                "compact" => MathStyle::Compact,
                _ => style.math_style,
            };
        }
        "math-depth" => {
            // Computed value — целое; относительные формы резолвятся от inherited
            // (MathML Core §2.1.2: auto-add и add(n) — относительно родителя).
            let v = val.trim();
            if v == "auto-add" {
                // +1 только если унаследованный math-style компактный.
                style.math_depth = inherited.math_depth
                    + i32::from(inherited.math_style == MathStyle::Compact);
            } else if let Some(inner) = v.strip_prefix("add(").and_then(|s| s.strip_suffix(')')) {
                if let Ok(n) = inner.trim().parse::<i32>() {
                    style.math_depth = inherited.math_depth + n;
                }
            } else if let Ok(n) = v.parse::<i32>() {
                style.math_depth = n;
            }
        }
        "user-select" => {
            if let Some(v) = UserSelect::parse(val) {
                style.user_select = v;
            }
        }
        "tab-size" => {
            // CSS Text L3 §10.1: <integer> или <length>. Integer = ширина
            // в spaces; принимаем как 8px-per-space heuristic. Length —
            // resolved-px.
            let trimmed = val.trim();
            if let Ok(n) = trimmed.parse::<i32>() {
                style.tab_size = (n.max(0) as f32) * 8.0;
            } else if let Some(px) = resolve_box_length(trimmed, em_basis, viewport, is_quirks) {
                style.tab_size = px.max(0.0);
            }
        }
        "overflow-wrap" | "word-wrap" => {
            // `word-wrap` — legacy alias для `overflow-wrap`.
            if let Some(v) = OverflowWrap::parse(val) {
                style.overflow_wrap = v;
            }
        }
        "word-break" => {
            if let Some(v) = WordBreak::parse(val) {
                style.word_break = v;
            }
        }
        "line-break" => {
            style.line_break = match val.trim() {
                "auto" => LineBreak::Auto,
                "loose" => LineBreak::Loose,
                "normal" => LineBreak::Normal,
                "strict" => LineBreak::Strict,
                "anywhere" => LineBreak::Anywhere,
                _ => style.line_break,
            };
        }
        "hyphens" => {
            if let Some(v) = Hyphens::parse(val) {
                style.hyphens = v;
            }
        }
        "list-style-type" => {
            if let Some(v) = ListStyleType::parse(val) {
                style.list_style_type = v;
            }
        }
        "list-style-position" => {
            if let Some(v) = ListStylePosition::parse(val) {
                style.list_style_position = v;
            }
        }
        "list-style-image" => {
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.list_style_image = None;
            } else if let Some(u) = parse_url_value(trimmed) {
                style.list_style_image = Some(u);
            }
        }
        "list-style" => {
            // Shorthand: type | position | image, в любом порядке.
            // Позиция проверяется раньше типа — иначе "inside"/"outside" могут
            // быть поглощены как Custom counter-style ident.
            for token in val.split_whitespace() {
                if token.eq_ignore_ascii_case("none") {
                    // `none` неоднозначен: type=None И image=None. Per spec,
                    // `none` сначала применяется к type, потом к image (если
                    // повторяется). Простая трактовка: первый none → type=None,
                    // последующие → image=None.
                    if !matches!(style.list_style_type, ListStyleType::None) {
                        style.list_style_type = ListStyleType::None;
                    } else {
                        style.list_style_image = None;
                    }
                } else if let Some(p) = ListStylePosition::parse(token) {
                    style.list_style_position = p;
                } else if let Some(u) = parse_url_value(token) {
                    style.list_style_image = Some(u);
                } else if let Some(t) = ListStyleType::parse(token) {
                    style.list_style_type = t;
                }
            }
        }
        "content" => {
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("normal") {
                style.content = Content::Normal;
            } else if trimmed.eq_ignore_ascii_case("none") {
                style.content = Content::None;
            } else {
                let items = parse_content_items(trimmed);
                if !items.is_empty() {
                    style.content = Content::Items(items);
                }
            }
        }
        "text-anchor" => {
            // SVG 2 §11.6 — horizontal anchoring of SVG <text>. As a CSS property
            // it overrides the `text-anchor` presentation attribute. Unknown values
            // are ignored (leave the cascaded value untouched).
            use crate::box_tree::SvgTextAnchor;
            match val.trim().to_ascii_lowercase().as_str() {
                "start" => style.text_anchor = Some(SvgTextAnchor::Start),
                "middle" => style.text_anchor = Some(SvgTextAnchor::Middle),
                "end" => style.text_anchor = Some(SvgTextAnchor::End),
                _ => {}
            }
        }
        "dominant-baseline" => {
            // SVG 2 §11.10.2 — vertical baseline alignment of SVG <text>. As a CSS
            // property it overrides the `dominant-baseline` presentation attribute.
            use crate::box_tree::SvgDominantBaseline;
            match val.trim().to_ascii_lowercase().as_str() {
                "auto" => style.dominant_baseline = Some(SvgDominantBaseline::Auto),
                "baseline" => style.dominant_baseline = Some(SvgDominantBaseline::Baseline),
                "hanging" => style.dominant_baseline = Some(SvgDominantBaseline::Hanging),
                "middle" => style.dominant_baseline = Some(SvgDominantBaseline::Middle),
                "central" => style.dominant_baseline = Some(SvgDominantBaseline::Central),
                "text-before-edge" => style.dominant_baseline = Some(SvgDominantBaseline::TextBeforeEdge),
                "text-after-edge" => style.dominant_baseline = Some(SvgDominantBaseline::TextAfterEdge),
                _ => {}
            }
        }
        "baseline-shift" => {
            // SVG 1.1 §10.9.2 / CSS Inline L3 §5.2 — vertical baseline shift of SVG
            // <text>/<tspan>. `baseline | sub | super | <length> | <percentage>`.
            // Percentage is relative to the current font-size; unknown values are
            // ignored (leave the cascaded value untouched).
            use crate::box_tree::SvgBaselineShift;
            let v = val.trim();
            match v.to_ascii_lowercase().as_str() {
                "baseline" => style.baseline_shift = SvgBaselineShift::Baseline,
                "sub" => style.baseline_shift = SvgBaselineShift::Sub,
                "super" => style.baseline_shift = SvgBaselineShift::Super,
                other => {
                    if let Some(pct) = other.strip_suffix('%') {
                        if let Ok(n) = pct.trim().parse::<f32>() {
                            style.baseline_shift = SvgBaselineShift::Percentage(n / 100.0);
                        }
                    } else if let Some(px) = resolve_svg_length(v, em_basis, viewport, is_quirks) {
                        style.baseline_shift = SvgBaselineShift::Length(px);
                    }
                }
            }
        }
        "line-height" => {
            apply_line_height_value(style, val, em_basis, viewport);
        }
        "line-height-step" => {
            // CSS Rhythmic Sizing L1 §2 — `<length>` step unit (`none`/`normal`/`0`
            // disable). Resolved to absolute px now; em/rem use the element's own
            // font-size. Percentages are invalid per spec → ignored. Negative → ignored.
            if matches!(val, "none" | "normal" | "0") {
                style.line_height_step = 0.0;
            } else if let Some(len) = parse_length(val) {
                let px = match &len {
                    Length::Px(v) => Some(*v),
                    Length::Em(v) => Some(v * style.font_size),
                    Length::Rem(v) => Some(v * ROOT_FONT_SIZE),
                    Length::Percent(_)
                    | Length::MinContent
                    | Length::MaxContent
                    | Length::FitContent(_) => None,
                    _ => len.resolve(em_basis, None, viewport),
                };
                if let Some(px) = px
                    && px >= 0.0
                {
                    style.line_height_step = px;
                }
            }
        }
        "text-decoration" => {
            // Shorthand: `<line> || <style> || <color>` в любом порядке (CSS Text
            // Decoration L3 §2.1). Спецификация L3 не включает thickness в
            // shorthand — для неё отдельный longhand. Per spec shorthand сбрасывает
            // все 4 longhand-а к initial, затем применяет указанные значения.
            let parsed = parse_text_decoration_shorthand_q(val, is_quirks);
            // Если shorthand был полностью невалиден (ни одного распознанного
            // токена) — declaration ignored. Распознаём по тому, что хоть
            // что-то распарсилось.
            if parsed.any_recognized {
                style.text_decoration_line = parsed.line.unwrap_or_default();
                style.text_decoration_color = parsed.color.unwrap_or(CssColor::CurrentColor);
                style.text_decoration_style = parsed.style.unwrap_or_default();
                // text-decoration-thickness shorthand-ом не сбрасывается
                // (исключена из L3 shorthand-а; см. §2.1).
            }
        }
        "text-decoration-line" => {
            let parsed = parse_text_decoration_shorthand_q(val, is_quirks);
            if let Some(d) = parsed.line {
                style.text_decoration_line = d;
            }
        }
        "text-decoration-color" => {
            if let Some(c) = parse_css_color_legacy(val, is_quirks) {
                style.text_decoration_color = c;
            }
        }
        "text-decoration-style" => {
            // CSS Text Decoration L3 §2.2 — единственный keyword из
            // `solid | double | dotted | dashed | wavy`. Невалидное — ignored.
            if let Some(s) = TextDecorationStyle::parse(val) {
                style.text_decoration_style = s;
            }
        }
        "text-decoration-thickness" => {
            // CSS Text Decoration L3 §2.3 — `auto | from-font | <length> |
            // <percentage>`. Невалидное — ignored.
            if let Some(t) = parse_text_decoration_thickness(val, em_basis, viewport) {
                style.text_decoration_thickness = t;
            }
        }
        "text-emphasis-style" => {
            // CSS Text Decoration L4 §5.3 — `none | [ filled | open ] ||
            // [ dot | circle | double-circle | triangle | sesame ] | <string>`.
            if let Some(s) = parse_text_emphasis_style(val) {
                style.text_emphasis_style = s;
            }
        }
        "text-emphasis-color" => {
            if let Some(c) = parse_css_color_legacy(val, is_quirks) {
                style.text_emphasis_color = c;
            }
        }
        "text-emphasis-position" => {
            // CSS Text Decoration L4 §5.5 — `[over | under] && [right | left]?`.
            if let Some(p) = parse_text_emphasis_position(val) {
                style.text_emphasis_position = p;
            }
        }
        "text-underline-position" => {
            // CSS Text Decoration L3 §6.1 / L4 §5.1.
            // Значения: auto | from-font | under | left | right.
            // Phase 0: однословный keyword — комбинации (under left) игнорируем.
            style.text_underline_position = match val.trim() {
                "auto" => TextUnderlinePosition::Auto,
                "from-font" => TextUnderlinePosition::FromFont,
                "under" => TextUnderlinePosition::Under,
                "left" => TextUnderlinePosition::Left,
                "right" => TextUnderlinePosition::Right,
                _ => style.text_underline_position,
            };
        }
        "text-underline-offset" => {
            // CSS Text Decoration L4 §5.3: auto | <length>.
            // Positive offset shifts underline away from text (downward for horizontal).
            style.text_underline_offset = if val.trim() == "auto" {
                None
            } else {
                parse_length_px(val.trim())
            };
        }
        "text-decoration-skip-ink" => {
            // CSS Text Decoration L4 §3.5: auto | all | none.
            style.text_decoration_skip_ink = match val.trim() {
                "auto" => TextDecorationSkipInk::Auto,
                "all" => TextDecorationSkipInk::All,
                "none" => TextDecorationSkipInk::None,
                _ => return true,
            };
        }
        "text-emphasis" => {
            // CSS Text Decoration L4 §5.6 — shorthand для -style и -color
            // (НЕ включает -position по spec). Сбрасывает обе longhand-ы в
            // initial и потом извлекает style+color из value.
            apply_text_emphasis_shorthand(style, val, is_quirks);
        }
        _ => return false,
    }
    true
}
