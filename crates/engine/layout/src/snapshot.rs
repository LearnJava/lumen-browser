//! Сериализация `LayoutBox` в детерминированный текст для snapshot-тестов.
//!
//! Аналог `lumen_paint::serialize_display_list`, но на уровне выше — фиксирует
//! всю структуру layout-дерева (тип бокса, rect, ключевые стилевые свойства,
//! сегменты и строки InlineRun-а).
//!
//! Из стиля выводятся только поля, отличающиеся от значений root-а (черный
//! текст, 16px, line-height 1.2, нулевые margin/padding, без фона). Это
//! даёт компактный читаемый снапшот: всё, что напечатано — отличается от
//! «дефолта».

use crate::box_tree::{BoxKind, InlineFrag, InlineSegment, LayoutBox};
use crate::style::{
    BorderStyle, BoxSizing, Color, ComputedStyle, Display, FontStyle, FontWeight, TextAlign,
    TextTransform,
};
use std::fmt::Write;

/// Корневой entry-point: рекурсивно сериализует всё дерево.
pub fn serialize_layout_tree(root: &LayoutBox) -> String {
    let mut out = String::new();
    write_box(&mut out, root, 0);
    out
}

fn write_box(out: &mut String, b: &LayoutBox, depth: usize) {
    let indent = "  ".repeat(depth);
    let kind = match &b.kind {
        BoxKind::Block => "Block",
        BoxKind::InlineRun { .. } => "InlineRun",
        BoxKind::Skip => "Skip",
    };
    let _ = write!(
        out,
        "{indent}{kind} rect=({:.2}, {:.2}, {:.2}, {:.2})",
        b.rect.x, b.rect.y, b.rect.width, b.rect.height
    );
    write_style_attrs(out, &b.style);
    out.push('\n');

    if let BoxKind::InlineRun { segments, lines } = &b.kind {
        let inner = "  ".repeat(depth + 1);
        for (i, seg) in segments.iter().enumerate() {
            write_segment(out, &inner, i, seg);
        }
        for (li, line) in lines.iter().enumerate() {
            let _ = writeln!(out, "{inner}line[{li}]:");
            let frag_indent = "  ".repeat(depth + 2);
            for (fi, frag) in line.iter().enumerate() {
                write_frag(out, &frag_indent, fi, frag);
            }
        }
    }

    for child in &b.children {
        write_box(out, child, depth + 1);
    }
}

fn write_segment(out: &mut String, indent: &str, i: usize, seg: &InlineSegment) {
    let _ = write!(out, "{indent}seg[{i}] {:?}", seg.text);
    write_text_style_attrs(out, &seg.style);
    out.push('\n');
}

fn write_frag(out: &mut String, indent: &str, i: usize, frag: &InlineFrag) {
    let _ = writeln!(out, "{indent}frag[{i}] x={:.2} {:?}", frag.x, frag.text);
}

/// Полный набор отличий стиля от root (включая display / width / height / margin / padding).
fn write_style_attrs(out: &mut String, s: &ComputedStyle) {
    if let Some(bg) = s.background_color
        && bg.a > 0
    {
        let _ = write!(out, " bg={}", color_hex(bg));
    }
    match s.display {
        Display::Block => {}
        Display::Inline => out.push_str(" display=inline"),
        Display::None => out.push_str(" display=none"),
    }
    if let Some(w) = s.width {
        let _ = write!(out, " w={w:.2}");
    }
    if let Some(h) = s.height {
        let _ = write!(out, " h={h:.2}");
    }
    if matches!(s.box_sizing, BoxSizing::BorderBox) {
        out.push_str(" box-sizing=border-box");
    }
    write_text_style_attrs(out, s);
    if s.margin_top != 0.0
        || s.margin_right != 0.0
        || s.margin_bottom != 0.0
        || s.margin_left != 0.0
    {
        let _ = write!(
            out,
            " m=({:.2}, {:.2}, {:.2}, {:.2})",
            s.margin_top, s.margin_right, s.margin_bottom, s.margin_left
        );
    }
    if s.padding_top != 0.0
        || s.padding_right != 0.0
        || s.padding_bottom != 0.0
        || s.padding_left != 0.0
    {
        let _ = write!(
            out,
            " p=({:.2}, {:.2}, {:.2}, {:.2})",
            s.padding_top, s.padding_right, s.padding_bottom, s.padding_left
        );
    }
    match s.text_align {
        TextAlign::Left => {}
        TextAlign::Center => out.push_str(" text-align=center"),
        TextAlign::Right => out.push_str(" text-align=right"),
    }
    let has_border = s.border_top_width > 0.0 || s.border_right_width > 0.0
        || s.border_bottom_width > 0.0 || s.border_left_width > 0.0;
    if has_border {
        let _ = write!(
            out,
            " bw=({:.2},{:.2},{:.2},{:.2})",
            s.border_top_width, s.border_right_width,
            s.border_bottom_width, s.border_left_width
        );
        let bs_str = |bs: BorderStyle| match bs {
            BorderStyle::None => "none",
            BorderStyle::Solid => "solid",
            BorderStyle::Dashed => "dashed",
            BorderStyle::Dotted => "dotted",
        };
        let _ = write!(
            out,
            " bs=({},{},{},{})",
            bs_str(s.border_top_style), bs_str(s.border_right_style),
            bs_str(s.border_bottom_style), bs_str(s.border_left_style)
        );
        let any_color = s.border_top_color.is_some() || s.border_right_color.is_some()
            || s.border_bottom_color.is_some() || s.border_left_color.is_some();
        if any_color {
            let c = |opt: Option<Color>| opt.map(color_hex).unwrap_or_else(|| "currentColor".into());
            let _ = write!(
                out,
                " bc=({},{},{},{})",
                c(s.border_top_color), c(s.border_right_color),
                c(s.border_bottom_color), c(s.border_left_color)
            );
        }
    }
}

/// Подмножество, влияющее на рендеринг текста (color / font-size / line-height /
/// text-decoration). Используется и для боксов, и для inline-сегментов.
fn write_text_style_attrs(out: &mut String, s: &ComputedStyle) {
    if s.color != Color::BLACK {
        let _ = write!(out, " color={}", color_hex(s.color));
    }
    if (s.font_size - 16.0).abs() > 0.01 {
        let _ = write!(out, " fs={:.2}", s.font_size);
    }
    if (s.line_height - 1.2).abs() > 0.01 {
        let _ = write!(out, " lh={:.2}", s.line_height);
    }
    if !s.text_decoration_line.is_empty() {
        let mut parts: Vec<&str> = Vec::new();
        if s.text_decoration_line.underline {
            parts.push("underline");
        }
        if s.text_decoration_line.overline {
            parts.push("overline");
        }
        if s.text_decoration_line.line_through {
            parts.push("line-through");
        }
        let _ = write!(out, " decoration={}", parts.join("+"));
    }
    match s.font_style {
        FontStyle::Normal => {}
        FontStyle::Italic => {
            let _ = write!(out, " font-style=italic");
        }
        FontStyle::Oblique => {
            let _ = write!(out, " font-style=oblique");
        }
    }
    if s.font_weight != FontWeight::NORMAL {
        let _ = write!(out, " font-weight={}", s.font_weight.0);
    }
    match s.text_transform {
        TextTransform::None => {}
        TextTransform::Uppercase => {
            let _ = write!(out, " text-transform=uppercase");
        }
        TextTransform::Lowercase => {
            let _ = write!(out, " text-transform=lowercase");
        }
        TextTransform::Capitalize => {
            let _ = write!(out, " text-transform=capitalize");
        }
    }
    if s.text_indent.abs() > 0.01 {
        let _ = write!(out, " text-indent={:.2}", s.text_indent);
    }
    if s.letter_spacing.abs() > 0.01 {
        let _ = write!(out, " letter-spacing={:.2}", s.letter_spacing);
    }
}

fn color_hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout;
    use lumen_core::geom::Size;

    fn lay(html: &str, css: &str) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        layout(&doc, &sheet, Size::new(800.0, 600.0))
    }

    #[test]
    fn empty_tree_serializes_to_single_root() {
        let root = lay("", "");
        let s = serialize_layout_tree(&root);
        assert_eq!(s, "Block rect=(0.00, 0.00, 800.00, 0.00)\n");
    }

    #[test]
    fn paragraph_renders_inline_run() {
        let root = lay("<p>hi</p>", "");
        let s = serialize_layout_tree(&root);
        assert!(s.contains("Block "), "{s}");
        assert!(s.contains("InlineRun "), "{s}");
        assert!(s.contains("\"hi\""), "{s}");
    }

    #[test]
    fn background_color_shows_as_bg() {
        let root = lay("<p>x</p>", "p { background: red; }");
        let s = serialize_layout_tree(&root);
        assert!(s.contains("bg=#ff0000ff"), "{s}");
    }

    #[test]
    fn default_style_fields_are_omitted() {
        let root = lay("<p>x</p>", "");
        let s = serialize_layout_tree(&root);
        // color, fs, lh, margin, padding не должны попасть — все дефолтные.
        assert!(!s.contains("color="), "{s}");
        assert!(!s.contains("fs="), "{s}");
        assert!(!s.contains("lh="), "{s}");
        assert!(!s.contains("m=("), "{s}");
        assert!(!s.contains("p=("), "{s}");
    }
}
