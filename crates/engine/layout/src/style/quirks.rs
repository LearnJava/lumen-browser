//! CSS Quirks Mode UA-rules: `<table>` font/color/text-align/white-space
//! reset к initial-values (эквивалент `-webkit-*` UA-stylesheet правила),
//! `line-height` и HTML `height`-атрибут для quirks.
//!
//! Перенесено батчем SPLIT-ST11 из `crates/engine/layout/src/style.rs`
//! (анкер `fn apply_quirks_table_reset`) без правок тел: изменены только
//! видимость item-ов (`pub(in crate::style)`) и пути импортов.

use lumen_dom::{Document, DocumentMode, NodeData, NodeId};

use crate::style::{
    default_font_family, Color, ComputedStyle, FontStretch, FontStyle, FontVariantCaps,
    FontWeight, Length, TextAlign, WhiteSpace, ROOT_FONT_SIZE,
};

/// CSS Quirks Mode — UA-rule только для Quirks-mode: элемент `<table>`
/// сбрасывает font / color / text-align / white-space-related свойства
/// к initial-values, не наследует от родителя. Эквивалент UA-stylesheet
/// правила (как в Chromium / Firefox / WebKit):
///
/// ```css
/// table {
///     font-size: medium;
///     font-weight: normal;
///     font-style: normal;
///     font-variant: normal;
///     line-height: normal;
///     color: -webkit-text;
///     text-align: -webkit-auto;
///     white-space: normal;
///     font-family: -webkit-default;
/// }
/// ```
///
/// Эффект: classics 90-х/2000-х с `<body style="font: 20px serif; color:
/// blue">` + table-layout не «протекают» в таблицу — таблица отрисовывается
/// дефолтным шрифтом / цветом. В Standards / LimitedQuirks таблица
/// наследует обычно. Author CSS поверх Quirks-reset выигрывает: spec
/// §UA-stylesheet — это самый низкий cascade origin.
pub(in crate::style) fn apply_quirks_table_reset(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    if doc.mode() != DocumentMode::Quirks {
        return;
    }
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if name.local.as_str() != "table" {
        return;
    }
    style.font_size = ROOT_FONT_SIZE;
    style.line_height = 1.2;
    style.font_family = default_font_family();
    style.font_style = FontStyle::Normal;
    style.font_variant_caps = FontVariantCaps::Normal;
    style.font_weight = FontWeight::NORMAL;
    style.font_stretch = FontStretch::NORMAL;
    style.color = Color::BLACK;
    style.text_align = TextAlign::Start;
    style.white_space = WhiteSpace::Normal;
}
/// CSS Quirks Mode §3.2: в quirks-mode replaced-элементы получают UA-правило
/// `line-height: 1`, которое блокирует наследование «normal» и убирает зазор
/// под `<img>` в inline-контексте (так делал IE7). Author CSS поверх — выигрывает.
pub(in crate::style) fn apply_quirks_line_height(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    if doc.mode() != DocumentMode::Quirks {
        return;
    }
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if matches!(
        name.local.as_str(),
        "img" | "video" | "canvas" | "embed" | "object"
            | "iframe" | "input" | "textarea" | "select" | "audio"
    ) {
        style.line_height = 1.0;
    }
}
/// CSS Quirks Mode §3.5 — viewport height as percentage basis for `<html>`.
///
/// In quirks mode the `<html>` element acts as if it has a definite height
/// equal to the viewport height, so that descendant elements can resolve
/// percentage heights against it (e.g. `body { height: 100% }`).
///
/// Implemented as a UA rule `html { height: 100vh }` applied before the CSS
/// cascade.  `Vh` resolves against the viewport directly and therefore does
/// not need a definite `available_height` from the parent (Document) box,
/// which currently propagates `None`.  Author CSS (`height: 200px`,
/// `height: auto`) overrides this UA rule through normal cascade ordering.
pub(in crate::style) fn apply_quirks_html_height(doc: &Document, node: NodeId, style: &mut ComputedStyle) {
    if doc.mode() != DocumentMode::Quirks {
        return;
    }
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return;
    };
    if name.local.as_str() == "html" {
        style.height = Some(Length::Vh(100.0));
    }
}
