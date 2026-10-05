//! CSS Grid L1 §7.1: a grid container without items still has its explicit
//! tracks, so it takes their size (plus gaps). `css-gaps/grid/…-046` relies on
//! it: the container holds only an absolutely positioned child and gets its
//! 100px rule from the `0px 0px` columns with a `100px` gap.

use lumen_core::geom::Size;

use super::super::{layout, BoxKind, LayoutBox};

fn grid(html: &str, css: &str) -> LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = layout(&doc, &sheet, Size::new(400.0, 400.0));
    fn find(b: &LayoutBox) -> Option<&LayoutBox> {
        if matches!(b.kind, BoxKind::Block) && b.style.display == crate::style::Display::Grid {
            return Some(b);
        }
        b.children.iter().find_map(find)
    }
    find(&root).expect("grid container").clone()
}

#[test]
fn empty_grid_takes_the_size_of_its_fixed_tracks() {
    let g = grid(
        "<div class='g'></div>",
        ".g { display: grid; grid-template-columns: 50px 50px; grid-template-rows: 30px 20px; \
              gap: 10px; width: max-content; }",
    );
    assert_eq!(g.rect.width, 110.0);
    assert_eq!(g.rect.height, 60.0);
}

#[test]
fn grid_with_only_an_abspos_child_takes_the_size_of_its_tracks() {
    let g = grid(
        "<div class='g'><div class='a'></div></div>",
        ".g { display: grid; position: relative; grid-template-columns: 0px 0px; \
              grid-template-rows: 100px; column-gap: 100px; width: max-content; } \
         .a { position: absolute; left: 0; top: 0; }",
    );
    assert_eq!(g.rect.width, 100.0);
    assert_eq!(g.rect.height, 100.0);
}

#[test]
fn empty_grid_without_a_template_stays_zero_height() {
    let g = grid("<div class='g'></div>", ".g { display: grid; gap: 10px; }");
    assert_eq!(g.rect.height, 0.0);
}

#[test]
fn trailing_empty_explicit_rows_keep_their_size_and_gaps() {
    // Rows 3..5 hold no item, but they are explicit tracks: 5 × 10px + 4 × 10px gaps.
    let g = grid(
        "<div class='g'><div class='i'></div></div>",
        ".g { display: grid; grid-template-columns: 20px; grid-template-rows: repeat(5, 10px); \
              gap: 10px; } .i { grid-row: 1 / 3; }",
    );
    assert_eq!(g.rect.height, 90.0);
}
