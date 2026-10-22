use super::*;

// ── Predefined counter styles in `::marker` (BUG-1570) and the two layout
// ── defects that kept the WPT `css-counter-styles` references from matching
// ── (BUG-1546 inline phrasing elements, BUG-1548 `inside` marker width).

fn list_markers(html: &str, css: &str) -> Vec<LayoutBox> {
    let root = lay_measured(html, css, 400.0);
    let list = first_element_child(&root);
    list.children
        .iter()
        .filter(|c| matches!(c.kind, BoxKind::Block))
        .filter_map(|li| li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })))
        .cloned()
        .collect()
}

fn marker_text(marker: &LayoutBox) -> &str {
    let BoxKind::Marker { text, .. } = &marker.kind else { panic!("not a marker box") };
    text
}

#[test]
fn predefined_style_text_includes_its_suffix() {
    let m = list_markers(
        "<ol><li>a<li>b<li>c</ol>",
        "ol { list-style-type: cjk-decimal; }",
    );
    let texts: Vec<_> = m.iter().map(marker_text).collect();
    assert_eq!(texts, ["一、", "二、", "三、"]);
}

#[test]
fn hebrew_marker_uses_the_default_suffix() {
    let m = list_markers("<ol><li>a<li>b</ol>", "ol { list-style-type: hebrew; }");
    assert_eq!(marker_text(&m[0]), "א. ");
    assert_eq!(marker_text(&m[1]), "ב. ");
}

#[test]
fn inside_marker_is_as_wide_as_its_text() {
    // Fixed8: 8px per glyph, so "א. " is 24px; the 1.5em floor (60px at 40px)
    // must not widen an `inside` marker.
    let m = list_markers(
        "<ul><li>x</ul>",
        "ul { padding: 0; list-style-position: inside; list-style-type: hebrew; font-size: 40px; }",
    );
    assert_eq!(m[0].rect.width, 24.0);
}

#[test]
fn outside_marker_keeps_the_gutter() {
    let m = list_markers(
        "<ul><li>x</ul>",
        "ul { padding: 0 0 0 80px; list-style-type: hebrew; font-size: 40px; }",
    );
    assert_eq!(m[0].rect.width, 60.0);
}

#[test]
fn inside_marker_width_is_not_rounded_to_whole_pixels() {
    // Fixed8 glyphs are 8px; a marker of 3 glyphs under `letter-spacing: 0.25px`
    // measures 24.75px and the first line starts exactly there.
    let root = lay_measured(
        "<ul><li>x</ul>",
        "ul { padding: 0; list-style-position: inside; list-style-type: hebrew; }\n\
         li::marker { letter-spacing: 0.25px; }",
        400.0,
    );
    let list = first_element_child(&root);
    let li = list.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
    let run = li.children.iter().find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
    let BoxKind::InlineRun { first_line_inset, .. } = &run.kind else { unreachable!() };
    assert_eq!(*first_line_inset, marker.rect.width);
    assert!(marker.rect.width.fract() != 0.0, "width {} was rounded", marker.rect.width);
}

#[test]
fn phrasing_elements_without_a_ua_rule_flow_inline() {
    for tag in ["bdi", "bdo", "kbd", "samp", "var", "dfn", "time", "data", "tt", "big", "font", "output"] {
        let root = lay(&format!("<div>A<{tag}>B</{tag}>C</div>"), "");
        let div = first_element_child(&root);
        let runs = div.children.iter().filter(|c| matches!(c.kind, BoxKind::InlineRun { .. })).count();
        assert_eq!(div.children.len(), 1, "<{tag}> must not split the line");
        assert_eq!(runs, 1, "<{tag}> must stay in the parent's inline run");
    }
}

#[test]
fn autonomous_custom_element_is_inline() {
    let root = lay("<div>A<my-el>B</my-el>C</div>", "");
    let div = first_element_child(&root);
    assert_eq!(div.children.len(), 1, "custom element must not split the line");
}
