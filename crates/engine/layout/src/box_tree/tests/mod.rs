use lumen_core::geom::Size;

fn layout_div(css: &str, viewport_w: f32, viewport_h: f32) -> super::LayoutBox {
    let html = "<div></div>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::layout(&doc, &sheet, Size::new(viewport_w, viewport_h));
    // html box > body box > div box
    fn find_empty_block(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
        for child in &b.children {
            if matches!(child.kind, super::BoxKind::Block) && child.children.is_empty() {
                return Some(child);
            }
            if let Some(found) = find_empty_block(child) {
                return Some(found);
            }
        }
        None
    }
    find_empty_block(&root).cloned().expect("empty Block not found in layout tree")
}

/// Border box of the first `<canvas>` box produced by laying out `html`+`css`.
fn canvas_border_box(html: &str, css: &str) -> (f32, f32) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    fn find(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
        if matches!(b.kind, super::BoxKind::Canvas { .. }) {
            return Some(b);
        }
        b.children.iter().find_map(find)
    }
    let c = find(&root).expect("Canvas box not found in layout tree");
    (c.rect.width, c.rect.height)
}

/// BUG-099 — HTML Rendering §15.4.1 leaves `<canvas>` out of the elements
/// whose dimension attributes map to the `width`/`height` properties: the
/// bitmap size is the *intrinsic* (content-box) size. Under `box-sizing:
/// border-box` the border must therefore grow the border box, not shrink
/// the bitmap — Edge lays TEST-57's `c3` out at 186×156, not 180×150.
#[test]
fn canvas_intrinsic_size_is_a_content_box_under_border_box_sizing() {
    assert_eq!(
        canvas_border_box(
            r#"<canvas width="180" height="150"></canvas>"#,
            "*{box-sizing:border-box;margin:0;padding:0}canvas{border:3px solid red}",
        ),
        (186.0, 156.0),
    );
}

/// The same element under the default `content-box` sizing — the border box
/// has always been intrinsic + border there, so the fix must not move it.
#[test]
fn canvas_intrinsic_size_unchanged_under_content_box_sizing() {
    assert_eq!(
        canvas_border_box(
            r#"<canvas width="180" height="150"></canvas>"#,
            "*{margin:0;padding:0}canvas{border:3px solid red}",
        ),
        (186.0, 156.0),
    );
}

/// BUG-441 — a form control renders its *current* value: the runtime
/// («dirty») value once it has one, the `value=` attribute / child text
/// only until then. Before the fix the box tree read the attribute, so a
/// field the user typed into or a script assigned kept painting the old
/// text (`el.value = 'ZZ'` → screen still empty).
#[test]
fn form_control_paints_runtime_value_over_the_default() {
    let html = r#"<input id="i" value="default"><textarea id="t">seed</textarea>"#;
    let mut doc = lumen_html_parser::parse(html);
    // Find the two controls by tag — the arena is small and ordered.
    let (mut input, mut textarea) = (None, None);
    for i in 0..doc.node_count() {
        let id = lumen_dom::NodeId::from_index(i);
        match doc.get(id).element_name().map(|q| q.local.as_str()) {
            Some("input") => input = Some(id),
            Some("textarea") => textarea = Some(id),
            _ => {}
        }
    }
    let (input, textarea) = (input.expect("<input>"), textarea.expect("<textarea>"));

    let sheet = lumen_css_parser::parse("");
    let defaults = collect_form_control_values(&super::layout(
        &doc,
        &sheet,
        Size::new(800.0, 600.0),
    ));
    assert_eq!(defaults, vec!["default".to_string(), "seed".to_string()]);

    doc.set_control_value(input, "typed");
    doc.set_control_value(textarea, "edited");
    let runtime = collect_form_control_values(&super::layout(
        &doc,
        &sheet,
        Size::new(800.0, 600.0),
    ));
    assert_eq!(runtime, vec!["typed".to_string(), "edited".to_string()]);
}

/// BUG-441 — a textarea's text is inline content laid out from its DOM
/// children, so the runtime value has to replace those children in the box
/// tree; otherwise `el.value = …` updated `FormControlKind::Textarea`
/// (which the painter ignores) while the screen kept the markup text.
#[test]
fn textarea_lays_out_runtime_value_instead_of_child_text() {
    let html = "<textarea>seed</textarea>";
    let mut doc = lumen_html_parser::parse(html);
    let ta = (0..doc.node_count())
        .map(lumen_dom::NodeId::from_index)
        .find(|&id| {
            doc.get(id).element_name().map(|q| q.local.as_str()) == Some("textarea")
        })
        .expect("<textarea>");
    let sheet = lumen_css_parser::parse("");

    let before = concat_inline_text(&super::layout(&doc, &sheet, Size::new(800.0, 600.0)));
    assert_eq!(before, "seed");

    doc.set_control_value(ta, "line1\nline2");
    let after = concat_inline_text(&super::layout(&doc, &sheet, Size::new(800.0, 600.0)));
    // The forced break between the two lines contributes an empty segment.
    assert_eq!(after, "line1line2");
    // The child text node is untouched — it is the default value.
    assert_eq!(doc.dirty_value(ta), Some("line1\nline2"));
}

/// Concatenated text of every inline segment in the tree.
fn concat_inline_text(root: &super::LayoutBox) -> String {
    let mut out = String::new();
    fn walk(b: &super::LayoutBox, out: &mut String) {
        if let super::BoxKind::InlineRun { segments, .. } = &b.kind {
            for s in segments {
                out.push_str(&s.text);
            }
        }
        for child in &b.children {
            walk(child, out);
        }
    }
    walk(root, &mut out);
    out
}

/// Painted text of every `<input>`/`<textarea>` box, in tree order.
fn collect_form_control_values(root: &super::LayoutBox) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(b: &super::LayoutBox, out: &mut Vec<String>) {
        if let super::BoxKind::FormControl { kind } = &b.kind {
            match kind {
                super::FormControlKind::Input { value_text, .. } => {
                    out.push(value_text.clone())
                }
                super::FormControlKind::Textarea { value_text } => {
                    out.push(value_text.clone())
                }
                _ => {}
            }
        }
        for child in &b.children {
            walk(child, out);
        }
    }
    walk(root, &mut out);
    out
}

/// An author-specified CSS `width`/`height` keeps its ordinary `border-box`
/// meaning — the intrinsic-size fill-in is skipped entirely.
#[test]
fn canvas_explicit_css_size_is_not_grown_by_the_border() {
    assert_eq!(
        canvas_border_box(
            r#"<canvas width="180" height="150"></canvas>"#,
            "*{box-sizing:border-box;margin:0;padding:0}\
             canvas{border:3px solid red;width:100px;height:80px}",
        ),
        (100.0, 80.0),
    );
}

/// Строит один текстовый сегмент с заданным `font-variant-caps`.
fn caps_seg(text: &str, caps: crate::style::FontVariantCaps) -> super::InlineSegment {
    let mut style = crate::style::ComputedStyle::root();
    style.font_size = 20.0;
    style.font_variant_caps = caps;
    super::InlineSegment {
        text: text.to_string(),
        style,
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: super::PseudoKind::None,
        source_node: lumen_dom::NodeId::from_index(0),
        source_char_offset: 0,
        bidi_level: 0,
    }
}

#[test]
fn caps_synthesis_skipped_for_normal_and_titling() {
    use crate::style::FontVariantCaps;
    // Normal нечего синтезировать; titling-caps уходит фичей `titl`,
    // а не геометрией — режущий проход обязан пропустить оба.
    for caps in [FontVariantCaps::Normal, FontVariantCaps::TitlingCaps] {
        assert!(super::caps_synthesis(&[caps_seg("Hello", caps)], None).is_none());
    }
}

#[test]
fn caps_synthesis_small_caps_splits_and_shrinks_lowercase() {
    use crate::style::FontVariantCaps;
    let (segs, no_break) =
        super::caps_synthesis(&[caps_seg("Hello", FontVariantCaps::SmallCaps)], None)
            .expect("small-caps must be synthesized");
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].text, "H");
    assert_eq!(segs[1].text, "ELLO");
    assert!((segs[0].style.font_size - 20.0).abs() < f32::EPSILON);
    assert!((segs[1].style.font_size - 20.0 * super::SMALL_CAPS_SCALE).abs() < f32::EPSILON);
    // Разрез прошёл внутри слова — перенос перед хвостом запрещён.
    assert_eq!(no_break, vec![false, true]);
}

#[test]
fn caps_synthesis_allows_break_after_whitespace() {
    use crate::style::FontVariantCaps;
    let (segs, no_break) =
        super::caps_synthesis(&[caps_seg("go home", FontVariantCaps::SmallCaps)], None)
            .expect("small-caps must be synthesized");
    // "GO" | " " | "HOME": пробел — отдельный Plain-прогон.
    assert_eq!(segs.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["GO", " ", "HOME"]);
    // Перенос перед "HOME" разрешён: слева от него пробел.
    assert_eq!(no_break, vec![false, true, false]);
}

#[test]
fn caps_synthesis_all_small_caps_shrinks_uppercase_too() {
    use crate::style::FontVariantCaps;
    let (segs, _) =
        super::caps_synthesis(&[caps_seg("Hi!", FontVariantCaps::AllSmallCaps)], None)
            .expect("all-small-caps must be synthesized");
    // Буквы — один уменьшенный прогон, `!` остаётся полноразмерным.
    assert_eq!(segs.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["HI", "!"]);
    assert!((segs[0].style.font_size - 20.0 * super::SMALL_CAPS_SCALE).abs() < f32::EPSILON);
    assert!((segs[1].style.font_size - 20.0).abs() < f32::EPSILON);
}

#[test]
fn caps_synthesis_unicase_shrinks_uppercase_without_recasing() {
    use crate::style::FontVariantCaps;
    let (segs, _) = super::caps_synthesis(&[caps_seg("Hi", FontVariantCaps::Unicase)], None)
        .expect("unicase must be synthesized");
    // `unicase`: заглавная становится капителью, строчная не трогается.
    assert_eq!(segs.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["H", "i"]);
    assert!((segs[0].style.font_size - 20.0 * super::SMALL_CAPS_SCALE).abs() < f32::EPSILON);
    assert!((segs[1].style.font_size - 20.0).abs() < f32::EPSILON);
}

#[test]
fn caps_synthesis_keeps_non_cased_scripts_full_size() {
    use crate::style::FontVariantCaps;
    // У иероглифов нет регистра — уменьшать их нельзя, хотя
    // is_alphabetic() для них истинно.
    let (segs, _) = super::caps_synthesis(&[caps_seg("漢a", FontVariantCaps::AllSmallCaps)], None)
        .expect("all-small-caps must be synthesized");
    assert_eq!(segs.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["漢", "A"]);
    assert!((segs[0].style.font_size - 20.0).abs() < f32::EPSILON);
}

#[test]
fn caps_synthesis_baseline_compensation_lowers_capitals() {
    use crate::style::{FontVariantCaps, VerticalAlign};
    let (segs, _) =
        super::caps_synthesis(&[caps_seg("ab", FontVariantCaps::SmallCaps)], None)
            .expect("small-caps must be synthesized");
    // apply_inline_vertical_align центрирует content-area по half-leading:
    // без компенсации капитель всплыла бы над базовой линией соседей.
    // px = (big − small)·(0.5 − ascent_ratio) = 4·(0.5 − 0.8) = −1.2,
    // а отрицательный `length` опускает фрагмент вниз.
    let VerticalAlign::Length(px) = segs[0].style.vertical_align else {
        panic!("capital run must carry a baseline correction");
    };
    assert!((px - (-1.2)).abs() < 1e-4, "px = {px}");
}

#[test]
fn caps_synthesis_respects_author_vertical_align() {
    use crate::style::{FontVariantCaps, VerticalAlign};
    let mut seg = caps_seg("ab", FontVariantCaps::SmallCaps);
    seg.style.vertical_align = VerticalAlign::Super;
    let (segs, _) = super::caps_synthesis(&[seg], None).expect("must be synthesized");
    // Автор задал выравнивание явно — компенсацию не навязываем.
    assert_eq!(segs[0].style.vertical_align, VerticalAlign::Super);
}

#[test]
fn caps_synthesis_keeps_source_offsets_monotonic() {
    use crate::style::FontVariantCaps;
    let (segs, _) =
        super::caps_synthesis(&[caps_seg("Hello", FontVariantCaps::SmallCaps)], None)
            .expect("must be synthesized");
    // Смещения в исходном тексте нужны Selection/Range — они обязаны
    // указывать на начало своего прогона в ОРИГИНАЛЕ, не в поднятом тексте.
    assert_eq!(segs[0].source_char_offset, 0);
    assert_eq!(segs[1].source_char_offset, 1);
}

#[test]
fn anon_style_resets_float_clear_position() {
    // BUG-152: anonymous boxes must not inherit the parent's non-inherited
    // float/clear/position (CSS 2.1 §9.2.2.1) — an anon InlineRun cloning a
    // floated parent re-entered the parent's float branch and overlapped
    // following block siblings.
    use crate::style::{ClearSide, ComputedStyle, FloatSide, Position};
    let mut parent = ComputedStyle::root();
    parent.float_side = FloatSide::Left;
    parent.clear = ClearSide::Both;
    parent.position = Position::Absolute;
    let anon = super::anon_style(&parent);
    assert_eq!(anon.float_side, FloatSide::None);
    assert_eq!(anon.clear, ClearSide::None);
    assert_eq!(anon.position, Position::Static);
}

#[test]
fn aspect_ratio_height_from_width() {
    // width: 200px, aspect-ratio: 2/1 → height should be 100px border-box
    let div = layout_div("div { width: 200px; aspect-ratio: 2/1; }", 800.0, 600.0);
    assert_eq!(div.rect.width, 200.0);
    assert_eq!(div.rect.height, 100.0);
}

#[test]
fn aspect_ratio_16_9() {
    // width: 160px, aspect-ratio: 16/9 → height = 160 * 9/16 = 90px
    let div = layout_div("div { width: 160px; aspect-ratio: 16/9; }", 800.0, 600.0);
    assert_eq!(div.rect.width, 160.0);
    assert!((div.rect.height - 90.0).abs() < 0.5, "height={}", div.rect.height);
}

#[test]
fn aspect_ratio_explicit_height_wins() {
    // Explicit height overrides aspect-ratio.
    let div = layout_div("div { width: 200px; height: 50px; aspect-ratio: 2/1; }", 800.0, 600.0);
    assert_eq!(div.rect.width, 200.0);
    assert_eq!(div.rect.height, 50.0);
}

#[test]
fn aspect_ratio_no_height_without_ratio() {
    // Without aspect-ratio, height collapses to 0 for empty div.
    let div = layout_div("div { width: 200px; }", 800.0, 600.0);
    assert_eq!(div.rect.width, 200.0);
    assert_eq!(div.rect.height, 0.0);
}

fn find_by_id_all<'a>(b: &'a super::LayoutBox, doc: &lumen_dom::Document, id: &str) -> Option<&'a super::LayoutBox> {
    if let lumen_dom::NodeData::Element { attrs, .. } = &doc.get(b.node).data
        && attrs.iter().any(|a| a.name.local == "id" && a.value == id)
    {
        return Some(b);
    }
    for child in &b.children {
        if let Some(f) = find_by_id_all(child, doc, id) { return Some(f); }
    }
    None
}

fn find_markers(b: &super::LayoutBox, out: &mut Vec<super::LayoutBox>) {
    if matches!(b.kind, super::BoxKind::Marker { .. }) { out.push(b.clone()); }
    for c in &b.children { find_markers(c, out); }
}

fn collect_seg_text(b: &super::LayoutBox, out: &mut String) {
    if let super::BoxKind::InlineRun { segments, .. } = &b.kind {
        for s in segments { out.push_str(&s.text); }
    }
    for c in &b.children { collect_seg_text(c, out); }
}

mod intrinsic_and_wrap;
mod flow_modes;
mod pseudo_first_line;

mod generated_float;
mod shapes_and_contain;
mod flex_align_content;

mod svg_transform_and_misc;
mod bug341_differential;
