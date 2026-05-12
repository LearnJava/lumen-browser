//! Box tree и block-флоу.
//!
//! Каждый DOM-узел даёт один LayoutBox; блочные элементы стэкаются
//! вертикально. Текстовые узлы разбиваются по словам на строки (line
//! wrapping), если передан `TextMeasurer`. Inline-элементы вроде `<a>`
//! пока трактуются как block (каждый получает собственную строку) — до
//! появления полноценных inline-boxes с line-box-ами.
//!
//! Whitespace-only текст и комментарии пропускаются.

use lumen_core::geom::{Rect, Size};
use lumen_css_parser::Stylesheet;
use lumen_dom::{Document, NodeData, NodeId};

use crate::style::{compute_style, ComputedStyle, Display};
use crate::TextMeasurer;

#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub node: NodeId,
    pub rect: Rect,
    pub style: ComputedStyle,
    pub kind: BoxKind,
    pub children: Vec<LayoutBox>,
}

#[derive(Debug, Clone)]
pub enum BoxKind {
    /// Block-уровневый бокс (элемент или корень документа).
    Block,
    /// Текстовый узел. Каждый элемент Vec — одна строка после line wrapping.
    /// Всегда содержит хотя бы один элемент (оригинальный текст или разбитые строки).
    Text(Vec<String>),
    /// Не участвует в layout (whitespace, комментарий, doctype, display:none).
    Skip,
}

pub fn layout(doc: &Document, sheet: &Stylesheet, viewport: Size) -> LayoutBox {
    let root_style = ComputedStyle::root();
    let mut root = build_box(doc, sheet, doc.root(), &root_style);
    lay_out(&mut root, 0.0, 0.0, viewport.width, None);
    root
}

pub fn layout_measured(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
) -> LayoutBox {
    let root_style = ComputedStyle::root();
    let mut root = build_box(doc, sheet, doc.root(), &root_style);
    lay_out(&mut root, 0.0, 0.0, viewport.width, Some(measurer));
    root
}

fn build_box(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
) -> LayoutBox {
    let style = compute_style(doc, id, sheet, inherited);

    let kind = match &doc.get(id).data {
        NodeData::Text(s) => {
            if s.chars().all(char::is_whitespace) {
                BoxKind::Skip
            } else {
                BoxKind::Text(vec![s.clone()])
            }
        }
        NodeData::Comment(_) | NodeData::Doctype { .. } => BoxKind::Skip,
        NodeData::Document | NodeData::Element { .. } => {
            if style.display == Display::None {
                BoxKind::Skip
            } else {
                BoxKind::Block
            }
        }
    };

    let mut children = Vec::new();
    if matches!(kind, BoxKind::Block) {
        for &child in &doc.get(id).children {
            children.push(build_box(doc, sheet, child, &style));
        }
    }

    LayoutBox {
        node: id,
        rect: Rect::ZERO,
        style,
        kind,
        children,
    }
}

fn lay_out(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    measurer: Option<&dyn TextMeasurer>,
) {
    if matches!(b.kind, BoxKind::Skip) {
        b.rect = Rect::new(start_x, start_y, 0.0, 0.0);
        return;
    }

    let s = b.style.clone();
    b.rect.x = start_x + s.margin_left;
    b.rect.y = start_y + s.margin_top;
    b.rect.width = (available_width - s.margin_left - s.margin_right).max(0.0);

    let content_x = b.rect.x + s.padding_left;
    let content_y = b.rect.y + s.padding_top;
    let content_width = (b.rect.width - s.padding_left - s.padding_right).max(0.0);

    // Применяем line wrapping к текстовым боксам до основного match.
    if let (BoxKind::Text(lines), Some(m)) = (&mut b.kind, measurer) {
        let original = lines[0].clone();
        *lines = wrap_text(&original, content_width, s.font_size, m);
    }

    let line_count = match &b.kind {
        BoxKind::Text(lines) => lines.len().max(1),
        _ => 0,
    };

    match &mut b.kind {
        BoxKind::Text(_) => {
            b.rect.height = line_count as f32 * (s.font_size * s.line_height);
        }
        BoxKind::Block => {
            let mut child_y = content_y;
            for child in &mut b.children {
                lay_out(child, content_x, child_y, content_width, measurer);
                if matches!(child.kind, BoxKind::Skip) {
                    continue;
                }
                child_y = child.rect.y + child.rect.height + child.style.margin_bottom;
            }
            let content_height = (child_y - content_y).max(0.0);
            b.rect.height = content_height + s.padding_top + s.padding_bottom;
        }
        BoxKind::Skip => unreachable!(),
    }
}

/// Разбивает `text` на строки так, чтобы каждая умещалась в `max_width` px.
/// Перенос только по пробелам (word wrap). Одно слово, широкое само по себе,
/// остаётся на одной строке (нет посимвольного разрыва).
fn wrap_text(text: &str, max_width: f32, font_size: f32, m: &dyn TextMeasurer) -> Vec<String> {
    let space_w = m.char_width(' ', font_size);

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_width = 0.0_f32;

    for word in text.split_whitespace() {
        let word_width: f32 = word.chars().map(|c| m.char_width(c, font_size)).sum();

        if current.is_empty() {
            // Первое слово строки — добавляем всегда, даже если шире max_width.
            current.push_str(word);
            current_width = word_width;
        } else if current_width + space_w + word_width <= max_width {
            current.push(' ');
            current.push_str(word);
            current_width += space_w + word_width;
        } else {
            lines.push(current.clone());
            current = word.to_string();
            current_width = word_width;
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}
