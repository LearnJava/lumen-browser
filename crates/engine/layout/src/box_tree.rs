//! Box tree и block-флоу.
//!
//! Phase 0 минимум: каждый DOM-узел даёт один LayoutBox; всё трактуем как
//! block-flow (даже inline-элементы вроде `<a>` идут отдельной строкой).
//! Текстовый узел занимает одну строку высоты `font_size * line_height`.
//! Whitespace-only текст и комментарии пропускаются.

use lumen_core::geom::{Rect, Size};
use lumen_css_parser::Stylesheet;
use lumen_dom::{Document, NodeData, NodeId};

use crate::style::{compute_style, ComputedStyle, Display};

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
    /// Текстовый узел.
    Text(String),
    /// Не участвует в layout (whitespace, комментарий, doctype, display:none).
    Skip,
}

pub fn layout(doc: &Document, sheet: &Stylesheet, viewport: Size) -> LayoutBox {
    let root_style = ComputedStyle::root();
    let mut root = build_box(doc, sheet, doc.root(), &root_style);
    lay_out(&mut root, 0.0, 0.0, viewport.width);
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
                BoxKind::Text(s.clone())
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

fn lay_out(b: &mut LayoutBox, start_x: f32, start_y: f32, available_width: f32) {
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

    match &b.kind {
        BoxKind::Text(_) => {
            b.rect.height = s.font_size * s.line_height;
        }
        BoxKind::Block => {
            let mut child_y = content_y;
            for child in &mut b.children {
                lay_out(child, content_x, child_y, content_width);
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
