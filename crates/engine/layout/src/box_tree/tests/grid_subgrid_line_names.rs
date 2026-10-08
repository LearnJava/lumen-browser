//! CSS Grid L2 §9 (BUG-1319): a subgrid's lines carry the names of the parent lines it spans,
//! plus its own `subgrid [a] [b]` names (`repeat(auto-fill, …)` included), so its items can be
//! placed by the parent's line names.

use lumen_core::geom::Size;

use super::super::{layout, LayoutBox};
use crate::style::{parse_subgrid_name_fill, parse_track_line_names, GridTrackSize, NameFill};

struct Page {
    doc: lumen_dom::Document,
    root: LayoutBox,
}

fn root(html: &str, css: &str) -> Page {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
    Page { doc, root }
}

fn find_class<'a>(doc: &lumen_dom::Document, b: &'a LayoutBox, class: &str) -> Option<&'a LayoutBox> {
    if let lumen_dom::NodeData::Element { attrs, .. } = &doc.get(b.node).data
        && attrs.iter().any(|a| a.name.local == "class" && a.value.split_whitespace().any(|c| c == class))
    {
        return Some(b);
    }
    b.children.iter().find_map(|c| find_class(doc, c, class))
}

/// `(x, width)` of the item `class`, `x` from the left edge of the outer grid `.o`.
fn span_of(p: &Page, class: &str) -> (f32, f32) {
    let item = find_class(&p.doc, &p.root, class).unwrap_or_else(|| panic!("no .{class}"));
    let grid = find_class(&p.doc, &p.root, "o").expect("no .o");
    (item.rect.x - grid.rect.x, item.rect.width)
}

const BASE: &str = ".o{display:grid;width:300px;grid-auto-rows:20px} \
    .s{display:grid;grid-column:1/4;grid-template-columns:subgrid} i{display:block}";

#[test]
fn item_directly_in_the_parent_resolves_a_name() {
    let r = root(
        "<div class=o><i class=t style='grid-column:y'></i></div>",
        &format!("{BASE} .o{{grid-template-columns:[x] 100px 100px [y] 100px [z]}}"),
    );
    assert_eq!(span_of(&r, "t"), (200.0, 100.0));
}

#[test]
fn subgrid_item_resolves_a_parent_line_name() {
    let r = root(
        "<div class=o><div class=s><i class=t style='grid-column:y'></i></div></div>",
        &format!("{BASE} .o{{grid-template-columns:[x] 100px 100px [y] 100px [z]}}"),
    );
    assert_eq!(span_of(&r, "t"), (200.0, 100.0));
}

#[test]
fn subgrid_spanning_a_part_of_the_parent_keeps_the_offset_of_its_lines() {
    // Subgrid on tracks 2..4: parent line `y` is its line 2 (the second track starts there).
    let r = root(
        "<div class=o><div class=s style='grid-column:2/4'><i class=t style='grid-column:y'></i></div></div>",
        &format!("{BASE} .o{{grid-template-columns:50px [a] 100px [y] 100px 50px}}"),
    );
    assert_eq!(span_of(&r, "t"), (150.0, 100.0));
}

#[test]
fn subgrid_own_names_are_added_to_the_parent_ones() {
    // `subgrid [p] [q]` names the subgrid's lines 1 and 2; the parent's `y` is on line 3.
    let r = root(
        "<div class=o><div class=s style='grid-template-columns:subgrid [p] [q]'>\
         <i class=a style='grid-column:q / y'></i></div></div>",
        &format!("{BASE} .o{{grid-template-columns:100px 100px [y] 100px}}"),
    );
    assert_eq!(span_of(&r, "a"), (100.0, 100.0));
}

#[test]
fn subgrid_name_list_with_repeat_auto_fill() {
    // 3 tracks → 4 lines; `[x] repeat(auto-fill, [y]) [z]` → x y y z.
    let r = root(
        "<div class=o><div class=s style='grid-template-columns:subgrid [x] repeat(auto-fill, [y]) [z]'>\
         <i class=a style='grid-column:y 2'></i><i class=b style='grid-column:x / z'></i></div></div>",
        &format!("{BASE} .o{{grid-template-columns:100px 100px 100px}}"),
    );
    assert_eq!(span_of(&r, "a"), (200.0, 100.0));
    assert_eq!(span_of(&r, "b"), (0.0, 300.0));
}

#[test]
fn auto_fill_names_of_a_parent_repeat_follow_the_expansion() {
    // `repeat(auto-fill, [n] 100px)` in 300px → 3 repetitions: n n n, then the tail name.
    let r = root(
        "<div class=o><div class=s><i class=a style='grid-column:n 3'></i>\
         <i class=b style='grid-column:t'></i></div></div>",
        &format!("{BASE} .o{{grid-template-columns:repeat(auto-fill, [n] 100px) [t]}}"),
    );
    assert_eq!(span_of(&r, "a"), (200.0, 100.0));
}

#[test]
fn nested_subgrid_passes_names_through() {
    let r = root(
        "<div class=o><div class=s><div class=s style='grid-column:2/4'>\
         <i class=t style='grid-column:y'></i></div></div></div>",
        &format!("{BASE} .o{{grid-template-columns:[x] 100px 100px [y] 100px [z]}}"),
    );
    // Inner subgrid covers parent lines 2..4; `y` is parent line 3, the inner subgrid's line 2.
    assert_eq!(span_of(&r, "t").0, 200.0);
}

#[test]
fn subgrid_keyword_with_names_is_a_subgrid() {
    let t = GridTrackSize::parse_track_list("subgrid [a] [b c] repeat(2, [d])", false);
    assert_eq!(t, vec![GridTrackSize::Subgrid]);
    assert!(GridTrackSize::parse_track_list("subgrid 100px", false) != vec![GridTrackSize::Subgrid]);
}

#[test]
fn subgrid_names_parse_with_fixed_and_auto_fill_repeats() {
    let names = parse_track_line_names("subgrid [a] [] repeat(2, [b] [c]) [d]", false);
    let flat: Vec<String> = names.iter().map(|g| g.join(",")).collect();
    assert_eq!(flat, ["a", "", "b", "c", "b", "c", "d"]);
    assert_eq!(parse_subgrid_name_fill("subgrid [a] repeat(auto-fill, [y] [z]) [d]"), Some(NameFill { at: 1, len: 2 }));
    assert_eq!(parse_subgrid_name_fill("subgrid [a]"), None);
    // Several lines in one token: `[][a][]`.
    assert_eq!(parse_track_line_names("subgrid [][a][]", false).len(), 3);
}

#[test]
fn name_fill_repeats_as_often_as_fits() {
    let names: Vec<Vec<String>> = ["x", "y", "z", "z"].iter().map(|n| vec![n.to_string()]).collect();
    let fill = NameFill { at: 1, len: 1 };
    let join = |v: Vec<Vec<String>>| v.iter().map(|g| g.join("")).collect::<String>();
    assert_eq!(join(fill.expand(&names, 5)), "xyyzz");
    assert_eq!(join(fill.expand(&names, 4)), "xyzz");
    // Nothing fits next to the two `z`: no repetition, the list is cut.
    assert_eq!(join(fill.expand(&names, 3)), "xzz");
}
