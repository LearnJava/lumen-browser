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
    BorderStyle, BoxSizing, Color, ComputedStyle, CssColor, Cursor, Direction, Display,
    FontStretch, FontStyle, FontVariant, FontWeight, Length, LengthOrAuto, OutlineColor, Position,
    OutlineStyle, Overflow, TextAlign, TextOverflow, TextTransform, Visibility, WhiteSpace,
};

fn fmt_len(l: &Length) -> String {
    match l {
        Length::Px(v) => format!("{v:.2}"),
        Length::Em(v) => format!("{v:.2}em"),
        Length::Rem(v) => format!("{v:.2}rem"),
        Length::Percent(v) => format!("{v:.2}%"),
        Length::Vh(v) => format!("{v:.2}vh"),
        Length::Vw(v) => format!("{v:.2}vw"),
        Length::Vmin(v) => format!("{v:.2}vmin"),
        Length::Vmax(v) => format!("{v:.2}vmax"),
        Length::Cqw(v) => format!("{v:.2}cqw"),
        Length::Cqh(v) => format!("{v:.2}cqh"),
        Length::Cqi(v) => format!("{v:.2}cqi"),
        Length::Cqb(v) => format!("{v:.2}cqb"),
        Length::Cqmin(v) => format!("{v:.2}cqmin"),
        Length::Cqmax(v) => format!("{v:.2}cqmax"),
        Length::Calc(_) => "calc(?)".to_string(),
        Length::MinContent => "min-content".to_string(),
        Length::MaxContent => "max-content".to_string(),
        Length::FitContent(None) => "fit-content".to_string(),
        Length::FitContent(Some(inner)) => format!("fit-content({})", fmt_len(inner)),
    }
}

fn fmt_loa(l: &LengthOrAuto) -> String {
    match l {
        LengthOrAuto::Auto => "auto".to_string(),
        LengthOrAuto::Length(inner) => fmt_len(inner),
    }
}

fn len_is_nonzero(l: &Length) -> bool {
    !matches!(l, Length::Px(v) if *v == 0.0)
}

fn loa_is_nonzero(l: &LengthOrAuto) -> bool {
    match l {
        LengthOrAuto::Auto => true,
        LengthOrAuto::Length(inner) => len_is_nonzero(inner),
    }
}
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
        BoxKind::Table => "Table",
        BoxKind::TableRowGroup => "TableRowGroup",
        BoxKind::TableRow => "TableRow",
        BoxKind::InlineRun { .. } => "InlineRun",
        BoxKind::InlineBlockRow => "InlineBlockRow",
        BoxKind::InlineSpace => "InlineSpace",
        BoxKind::Image { .. } => "Image",
        BoxKind::Video { .. } => "Video",
        BoxKind::Canvas { .. } => "Canvas",
        BoxKind::Audio { .. } => "Audio",
        BoxKind::Iframe { .. } => "Iframe",
        BoxKind::FormControl { .. } => "FormControl",
        BoxKind::Skip => "Skip",
        BoxKind::Marker { .. } => "Marker",
        BoxKind::FlowRoot => "FlowRoot",
        BoxKind::Contents => "Contents",
        BoxKind::SvgRoot { .. } => "SvgRoot",
        BoxKind::SvgShape { .. } => "SvgShape",
        BoxKind::SvgText { .. } => "SvgText",
    };
    let _ = write!(
        out,
        "{indent}{kind} rect=({:.2}, {:.2}, {:.2}, {:.2})",
        b.rect.x, b.rect.y, b.rect.width, b.rect.height
    );
    if let BoxKind::Image { src, alt, .. } = &b.kind {
        let _ = write!(out, " src={src:?} alt={alt:?}");
    }
    if let BoxKind::Video { src, poster } = &b.kind {
        let _ = write!(out, " src={src:?} poster={poster:?}");
    }
    if let BoxKind::Canvas { width, height } = &b.kind {
        let _ = write!(out, " canvas={width}x{height}");
    }
    if let BoxKind::Audio { src, controls } = &b.kind {
        let _ = write!(out, " src={src:?} controls={controls}");
    }
    if let BoxKind::Iframe { src, .. } = &b.kind {
        let _ = write!(out, " src={src:?}");
    }
    if let BoxKind::SvgShape { shape, .. } = &b.kind {
        use crate::box_tree::SvgShapeKind;
        match shape {
            SvgShapeKind::Rect { x, y, width, height, rx, ry } =>
                { let _ = write!(out, " rect({x},{y},{width},{height}) rx={rx} ry={ry}"); }
            SvgShapeKind::Circle { cx, cy, r } =>
                { let _ = write!(out, " circle({cx},{cy}) r={r}"); }
            SvgShapeKind::Ellipse { cx, cy, rx, ry } =>
                { let _ = write!(out, " ellipse({cx},{cy}) rx={rx} ry={ry}"); }
            SvgShapeKind::Line { x1, y1, x2, y2 } =>
                { let _ = write!(out, " line({x1},{y1}→{x2},{y2})"); }
            SvgShapeKind::Path { d } =>
                { let _ = write!(out, " path d={d:?}"); }
        }
    }
    write_style_attrs(out, &b.style);
    out.push('\n');

    if let BoxKind::InlineRun { segments, lines, .. } = &b.kind {
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
    if let Some(CssColor::Rgba(bg)) = s.background_color
        && bg.a > 0
    {
        let _ = write!(out, " bg={}", color_hex(bg));
    }
    match s.position {
        Position::Static => {}
        Position::Relative => out.push_str(" position=relative"),
        Position::Absolute => out.push_str(" position=absolute"),
        Position::Fixed => out.push_str(" position=fixed"),
        Position::Sticky => out.push_str(" position=sticky"),
    }
    match s.display {
        Display::Block => {}
        Display::Inline => out.push_str(" display=inline"),
        Display::None => out.push_str(" display=none"),
        Display::Flex => out.push_str(" display=flex"),
        Display::InlineFlex => out.push_str(" display=inline-flex"),
        Display::Grid => out.push_str(" display=grid"),
        Display::InlineGrid => out.push_str(" display=inline-grid"),
        Display::InlineBlock => out.push_str(" display=inline-block"),
        Display::FlowRoot => out.push_str(" display=flow-root"),
        Display::Contents => out.push_str(" display=contents"),
        Display::Table => out.push_str(" display=table"),
        Display::InlineTable => out.push_str(" display=inline-table"),
        Display::TableRowGroup => out.push_str(" display=table-row-group"),
        Display::TableHeaderGroup => out.push_str(" display=table-header-group"),
        Display::TableFooterGroup => out.push_str(" display=table-footer-group"),
        Display::TableRow => out.push_str(" display=table-row"),
        Display::TableColumnGroup => out.push_str(" display=table-column-group"),
        Display::TableColumn => out.push_str(" display=table-column"),
        Display::TableCell => out.push_str(" display=table-cell"),
        Display::TableCaption => out.push_str(" display=table-caption"),
        Display::ListItem => out.push_str(" display=list-item"),
    }
    if let Some(w) = &s.width {
        let _ = write!(out, " w={}", fmt_len(w));
    }
    if let Some(h) = &s.height {
        let _ = write!(out, " h={}", fmt_len(h));
    }
    if let Some(v) = &s.min_width {
        let _ = write!(out, " min-w={}", fmt_len(v));
    }
    if let Some(v) = &s.max_width {
        let _ = write!(out, " max-w={}", fmt_len(v));
    }
    if let Some(v) = &s.min_height {
        let _ = write!(out, " min-h={}", fmt_len(v));
    }
    if let Some(v) = &s.max_height {
        let _ = write!(out, " max-h={}", fmt_len(v));
    }
    if matches!(s.box_sizing, BoxSizing::BorderBox) {
        out.push_str(" box-sizing=border-box");
    }
    write_text_style_attrs(out, s);
    if loa_is_nonzero(&s.margin_top)
        || loa_is_nonzero(&s.margin_right)
        || loa_is_nonzero(&s.margin_bottom)
        || loa_is_nonzero(&s.margin_left)
    {
        let _ = write!(
            out,
            " m=({}, {}, {}, {})",
            fmt_loa(&s.margin_top),
            fmt_loa(&s.margin_right),
            fmt_loa(&s.margin_bottom),
            fmt_loa(&s.margin_left),
        );
    }
    if len_is_nonzero(&s.padding_top)
        || len_is_nonzero(&s.padding_right)
        || len_is_nonzero(&s.padding_bottom)
        || len_is_nonzero(&s.padding_left)
    {
        let _ = write!(
            out,
            " p=({}, {}, {}, {})",
            fmt_len(&s.padding_top),
            fmt_len(&s.padding_right),
            fmt_len(&s.padding_bottom),
            fmt_len(&s.padding_left),
        );
    }
    match s.text_align {
        TextAlign::Left => {}
        TextAlign::Start => {}
        TextAlign::End => out.push_str(" text-align=end"),
        TextAlign::Center => out.push_str(" text-align=center"),
        TextAlign::Right => out.push_str(" text-align=right"),
    }
    if matches!(s.direction, Direction::Rtl) {
        out.push_str(" direction=rtl");
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
            BorderStyle::Double => "double",
        };
        let _ = write!(
            out,
            " bs=({},{},{},{})",
            bs_str(s.border_top_style), bs_str(s.border_right_style),
            bs_str(s.border_bottom_style), bs_str(s.border_left_style)
        );
        let any_color = matches!(s.border_top_color, CssColor::Rgba(_))
            || matches!(s.border_right_color, CssColor::Rgba(_))
            || matches!(s.border_bottom_color, CssColor::Rgba(_))
            || matches!(s.border_left_color, CssColor::Rgba(_));
        if any_color {
            let c = |cc: CssColor| match cc {
                CssColor::Rgba(col) => color_hex(col),
                CssColor::CurrentColor => "currentColor".into(),
                CssColor::Wide(f) => color_hex(f.to_srgb_color()),
                CssColor::System(sc) => color_hex(sc.resolve_color(false)),
            };
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
    if s.font_variant == FontVariant::SmallCaps {
        let _ = write!(out, " font-variant=small-caps");
    }
    if s.font_stretch != FontStretch::NORMAL {
        let whole = s.font_stretch.0 / 10;
        let frac = s.font_stretch.0 % 10;
        if frac == 0 {
            let _ = write!(out, " font-stretch={whole}%");
        } else {
            let _ = write!(out, " font-stretch={whole}.{frac}%");
        }
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
    if len_is_nonzero(&s.text_indent) {
        let _ = write!(out, " text-indent={}", fmt_len(&s.text_indent));
    }
    if s.letter_spacing.abs() > 0.01 {
        let _ = write!(out, " letter-spacing={:.2}", s.letter_spacing);
    }
    if s.word_spacing.abs() > 0.01 {
        let _ = write!(out, " word-spacing={:.2}", s.word_spacing);
    }
    if s.white_space == WhiteSpace::Nowrap {
        let _ = write!(out, " white-space=nowrap");
    }
    if (s.opacity - 1.0).abs() > 0.001 {
        let _ = write!(out, " opacity={:.3}", s.opacity);
    }
    let used_outline = s.outline_used_width();
    if s.outline_style.is_visible() && used_outline > 0.0 {
        let _ = write!(
            out,
            " outline={}/{:.2}",
            outline_style_str(s.outline_style),
            used_outline
        );
        if !matches!(s.outline_color, OutlineColor::Auto) {
            let _ = write!(out, "/{}", outline_color_str(s.outline_color));
        }
    }
    if let Length::Px(v) = &s.outline_offset {
        if v.abs() > 0.01 { let _ = write!(out, " outline-offset={v:.2}"); }
    } else {
        let _ = write!(out, " outline-offset={:?}", s.outline_offset);
    }
    if let Some(ac) = s.accent_color {
        let _ = write!(out, " accent={}", color_hex(ac));
    }
    match s.visibility {
        Visibility::Visible => {}
        Visibility::Hidden => {
            let _ = write!(out, " visibility=hidden");
        }
        Visibility::Collapse => {
            let _ = write!(out, " visibility=collapse");
        }
    }
    if s.overflow_x != Overflow::Visible || s.overflow_y != Overflow::Visible {
        let _ = write!(
            out,
            " overflow={}/{}",
            overflow_str(s.overflow_x),
            overflow_str(s.overflow_y)
        );
    }
    if s.text_overflow == TextOverflow::Ellipsis {
        let _ = write!(out, " text-overflow=ellipsis");
    }
    if s.cursor != Cursor::Auto {
        let _ = write!(out, " cursor={:?}", s.cursor);
    }
    if !s.box_shadow.is_empty() {
        let _ = write!(out, " box-shadow={}", s.box_shadow.len());
    }
    if !s.text_shadow.is_empty() {
        let _ = write!(out, " text-shadow={}", s.text_shadow.len());
    }
    if radius_nonzero(&s.border_top_left_radius)
        || radius_nonzero(&s.border_top_right_radius)
        || radius_nonzero(&s.border_bottom_right_radius)
        || radius_nonzero(&s.border_bottom_left_radius)
    {
        let _ = write!(
            out,
            " border-radius=({},{},{},{})",
            radius_display(&s.border_top_left_radius),
            radius_display(&s.border_top_right_radius),
            radius_display(&s.border_bottom_right_radius),
            radius_display(&s.border_bottom_left_radius),
        );
    }
}

fn radius_nonzero(len: &Length) -> bool {
    match len {
        Length::Px(v) => *v != 0.0,
        Length::Percent(p) => *p != 0.0,
        _ => true,
    }
}

fn radius_display(len: &Length) -> String {
    match len {
        Length::Px(v) => format!("{:.2}", v),
        Length::Percent(p) => format!("{:.2}%", p),
        _ => "?".to_owned(),
    }
}

fn overflow_str(o: Overflow) -> &'static str {
    match o {
        Overflow::Visible => "visible",
        Overflow::Hidden => "hidden",
        Overflow::Clip => "clip",
        Overflow::Scroll => "scroll",
        Overflow::Auto => "auto",
    }
}

fn outline_style_str(s: OutlineStyle) -> &'static str {
    match s {
        OutlineStyle::None => "none",
        OutlineStyle::Auto => "auto",
        OutlineStyle::Solid => "solid",
        OutlineStyle::Dashed => "dashed",
        OutlineStyle::Dotted => "dotted",
    }
}

fn outline_color_str(c: OutlineColor) -> String {
    match c {
        OutlineColor::Auto => "auto".to_string(),
        OutlineColor::CurrentColor => "currentcolor".to_string(),
        OutlineColor::Color(col) => color_hex(col),
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

    /// Navigate root → html → body and return the body `LayoutBox`.
    /// Adapts tests written before the HTML5 parser started injecting
    /// implicit html/head/body wrappers.
    fn body_layout_box(mut root: LayoutBox) -> LayoutBox {
        if let Some(html_idx) = root
            .children
            .iter()
            .position(|c| matches!(c.kind, BoxKind::Block))
        {
            let mut html_box = root.children.remove(html_idx);
            if let Some(body_idx) = html_box
                .children
                .iter()
                .position(|c| matches!(c.kind, BoxKind::Block))
            {
                return html_box.children.remove(body_idx);
            }
            return html_box;
        }
        root
    }

    fn lay(html: &str, css: &str) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        body_layout_box(layout(&doc, &sheet, Size::new(800.0, 600.0)))
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
