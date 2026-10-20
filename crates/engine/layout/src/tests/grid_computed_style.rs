use super::*;

// BUG-1307: `getComputedStyle()` answers the grid longhands and the gaps.

/// The `getComputedStyle()` snapshot entry of the element matching `sel` on a page laid out at 800×600.
fn grid_style_of(html: &str, css: &str, sel: &str) -> crate::StyleMap {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
    let nid = find_first_dom_node_by_selector(&doc, sel).expect("selector must match").index() as u32;
    collect_computed_styles(&root, &doc, None, Size::new(800.0, 600.0))
        .remove(&nid)
        .expect("element must have a computed-style entry")
}

fn val(s: &crate::StyleMap, name: &str) -> String {
    s.get(name).cloned().unwrap_or_else(|| "<absent>".to_owned())
}

#[test]
fn initial_values_of_a_plain_element() {
    let s = grid_style_of("<html><body><div id=a></div></body></html>", "", "#a");
    for (name, want) in [
        ("grid-template-columns", "none"),
        ("grid-template-rows", "none"),
        ("grid-template-areas", "none"),
        ("grid-auto-columns", "auto"),
        ("grid-auto-rows", "auto"),
        ("grid-auto-flow", "row"),
        ("grid-column-start", "auto"),
        ("grid-row-end", "auto"),
        ("grid-column", "auto"),
        ("grid-row", "auto"),
        ("grid-area", "auto"),
        ("row-gap", "normal"),
        ("column-gap", "normal"),
        ("gap", "normal"),
    ] {
        assert_eq!(val(&s, name), want, "{name}");
    }
}

#[test]
fn gaps_and_auto_tracks_are_serialised_in_px() {
    let s = grid_style_of(
        "<html><body><div id=a></div></body></html>",
        "#a{display:grid;font-size:10px;row-gap:3px;column-gap:0.5em;grid-auto-columns:minmax(2em,1fr);\
         grid-auto-rows:fit-content(4%);grid-auto-flow:row dense}",
        "#a",
    );
    assert_eq!(val(&s, "row-gap"), "3px");
    assert_eq!(val(&s, "column-gap"), "5px");
    assert_eq!(val(&s, "gap"), "3px 5px");
    assert_eq!(val(&s, "grid-row-gap"), "3px");
    assert_eq!(val(&s, "grid-auto-columns"), "minmax(20px, 1fr)");
    assert_eq!(val(&s, "grid-auto-rows"), "fit-content(4%)");
    assert_eq!(val(&s, "grid-auto-flow"), "dense");
}

#[test]
fn equal_gaps_collapse_in_the_shorthand() {
    let s = grid_style_of("<html><body><div id=a></div></body></html>", "#a{display:grid;gap:7px}", "#a");
    assert_eq!(val(&s, "gap"), "7px");
}

#[test]
fn template_of_an_empty_grid_container_is_resolved_to_used_tracks() {
    let s = grid_style_of(
        "<html><body><div id=a><i></i><i></i><i></i><i></i></div></body></html>",
        "body{margin:0} #a{display:grid;width:300px;grid-template-columns:100px 1fr [x] 50px;\
         grid-template-rows:30px auto;grid-template-areas:'a b' 'c d'}",
        "#a",
    );
    assert_eq!(val(&s, "grid-template-columns"), "100px 150px [x] 50px");
    assert_eq!(val(&s, "grid-template-areas"), "\"a b\" \"c d\"");
    // `[x]` precedes the third track.
    assert!(val(&s, "grid-template-rows").starts_with("30px "), "{}", val(&s, "grid-template-rows"));
}

#[test]
fn implicit_tracks_and_gaps_are_part_of_the_resolved_list() {
    let s = grid_style_of(
        "<html><body><div id=a><i></i><i></i><i></i></div></body></html>",
        "body{margin:0} #a{display:grid;width:100px;grid-template-columns:20px;grid-auto-columns:10px;\
         column-gap:5px;grid-auto-flow:column}",
        "#a",
    );
    assert_eq!(val(&s, "grid-template-columns"), "20px 10px 10px");
}

#[test]
fn auto_fit_collapses_empty_tracks_to_zero() {
    let s = grid_style_of(
        "<html><body><div id=a><i></i></div></body></html>",
        "body{margin:0} #a{display:grid;width:100px;grid-template-columns:repeat(auto-fit,20px)}",
        "#a",
    );
    assert_eq!(val(&s, "grid-template-columns"), "20px 0px 0px 0px 0px");
}

#[test]
fn not_a_grid_container_keeps_the_computed_template() {
    let s = grid_style_of(
        "<html><body><div id=a></div></body></html>",
        "#a{display:block;font-size:2px;grid-template-columns:[a] 1em minmax(10%,max-content) [b];\
         grid-template-rows:repeat(auto-fill,[c] 3px [d])}",
        "#a",
    );
    assert_eq!(val(&s, "grid-template-columns"), "[a] 2px minmax(10%, max-content) [b]");
    assert_eq!(val(&s, "grid-template-rows"), "repeat(auto-fill, [c] 3px [d])");
}

#[test]
fn line_placement_is_serialised_in_the_shortest_form() {
    let s = grid_style_of(
        "<html><body><div id=a></div></body></html>",
        "#a{grid-column:2 / span 3 k;grid-row:i;grid-area:auto / j / auto / j}",
        "#a",
    );
    // `grid-area` overrides the longhands set before it: the check is on the last declaration.
    assert_eq!(val(&s, "grid-row-start"), "auto");
    assert_eq!(val(&s, "grid-column-start"), "j");
    assert_eq!(val(&s, "grid-area"), "auto / j");

    let s = grid_style_of(
        "<html><body><div id=a></div></body></html>",
        "#a{grid-column:2 / span 3 k;grid-row:i}",
        "#a",
    );
    assert_eq!(val(&s, "grid-column-start"), "2");
    assert_eq!(val(&s, "grid-column-end"), "span 3 k");
    assert_eq!(val(&s, "grid-column"), "2 / span 3 k");
    assert_eq!(val(&s, "grid-row"), "i");
    assert_eq!(val(&s, "grid-row-end"), "i");
    assert_eq!(val(&s, "grid-area"), "i / 2 / i / span 3 k");
}

#[test]
fn span_of_one_name_drops_the_count() {
    let s = grid_style_of(
        "<html><body><div id=a></div></body></html>",
        "#a{grid-row-start:span 1 i;grid-row-end:span 1;grid-column-start:-3 n}",
        "#a",
    );
    assert_eq!(val(&s, "grid-row-start"), "span i");
    assert_eq!(val(&s, "grid-row-end"), "span 1");
    assert_eq!(val(&s, "grid-column-start"), "-3 n");
}
