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
fn inside_text_marker_is_the_first_fragment_of_the_first_line() {
    // The marker's text flows with the item's text (CSS Lists L3 §3.1): one
    // run, the marker's glyphs first, the item's text right behind them.
    let root = lay_measured(
        "<ul><li>x</ul>",
        "ul { padding: 0; list-style-position: inside; list-style-type: hebrew; }",
        400.0,
    );
    let list = first_element_child(&root);
    let li = list.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
    let BoxKind::Marker { in_run, .. } = &marker.kind else { unreachable!() };
    assert!(*in_run, "the run paints the marker, the marker box must not");
    let run = li.children.iter().find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
    let BoxKind::InlineRun { segments, lines, first_line_inset, .. } = &run.kind else { unreachable!() };
    assert_eq!(*first_line_inset, 0.0);
    assert_eq!(segments[0].text, "א. ");
    assert_eq!(segments[0].pseudo_kind, PseudoKind::Marker);
    // The marker's trailing space is a gap between fragments, not part of a glyph run.
    let line: String = lines[0].iter().map(|f| f.text.as_str()).collect();
    assert!(line.starts_with("א."), "marker text leads line 0, got {line:?}");
    assert!(line.ends_with('x'));
    let item = lines[0].last().unwrap();
    assert!(item.x >= 24.0, "the item's text starts after the marker, x = {}", item.x);
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

// ── `counter()` / `counters()` отдают представление без prefix/suffix (BUG-1550).

fn flow_text(b: &LayoutBox, out: &mut String) {
    if let BoxKind::InlineRun { segments, .. } = &b.kind {
        for s in segments {
            out.push_str(&s.text);
        }
    }
    for c in &b.children {
        flow_text(c, out);
    }
}

fn generated_text(css: &str) -> String {
    let root = lay_measured("<p>x</p>", css, 400.0);
    let mut out = String::new();
    flow_text(&root, &mut out);
    out
}

#[test]
fn counter_function_omits_default_affixes_of_custom_style() {
    let t = generated_text(
        "@counter-style f { system: cyclic; symbols: \"X\" \"Y\"; } \
         p { counter-reset: a 3; } p::before { content: \"[\" counter(a, f) \"]\"; }",
    );
    assert!(t.starts_with("[X]"), "got {t:?}");
}

#[test]
fn counter_function_omits_explicit_affixes_of_custom_style() {
    let t = generated_text(
        "@counter-style f { system: cyclic; symbols: \"X\" \"Y\"; prefix: \"<\"; suffix: \"!\"; } \
         p { counter-reset: a 3; } p::before { content: \"[\" counter(a, f) \"]\"; }",
    );
    assert!(t.starts_with("[X]"), "got {t:?}");
}

#[test]
fn counters_function_omits_affixes_of_custom_style() {
    let t = generated_text(
        "@counter-style f { system: cyclic; symbols: \"X\" \"Y\"; } \
         p { counter-reset: a 3; } p::before { content: \"[\" counters(a, \"-\", f) \"]\"; }",
    );
    assert!(t.starts_with("[X]"), "got {t:?}");
}
