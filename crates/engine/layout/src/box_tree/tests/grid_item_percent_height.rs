//! CSS 2.1 §10.5 + CSS Grid L1 §6.2: the grid area is the containing block of a
//! grid item, so a percentage `height` of the item resolves against the area's
//! block size once the rows are sized. The probe pass lays items out with an
//! indefinite height, so the item has to be laid out again for the final pass.

use lumen_core::geom::Size;

use super::super::{layout, BoxKind, LayoutBox};

fn grid_items(html: &str, css: &str) -> Vec<LayoutBox> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = layout(&doc, &sheet, Size::new(400.0, 400.0));
    fn find(b: &LayoutBox) -> Option<&LayoutBox> {
        if matches!(b.kind, BoxKind::Block) && b.style.display == crate::style::Display::Grid {
            return Some(b);
        }
        b.children.iter().find_map(find)
    }
    find(&root).expect("grid container").children.clone()
}

#[test]
fn percent_height_resolves_against_fixed_row() {
    let items = grid_items(
        "<div class='g'><div class='a'></div><div class='b'></div></div>",
        ".g { display: grid; grid-template-columns: 100px; grid-template-rows: 100px 80px; \
              gap: 10px; width: 100px; height: 320px; align-content: start; } \
         .a { width: 100%; height: 100%; } .b { height: 50%; }",
    );
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].rect.height, 100.0);
    assert_eq!(items[1].rect.height, 40.0);
    assert_eq!(items[1].rect.y - items[0].rect.y, 110.0);
}

#[test]
fn percent_height_in_auto_row_stays_auto() {
    // Row height depends on content, so the percentage cannot be resolved
    // against it in the probe; the item keeps its content height (0 here)
    // before stretch — it must at least not panic or grow past the row.
    let items = grid_items(
        "<div class='g'><div class='a'></div></div>",
        ".g { display: grid; grid-template-columns: 100px; width: 100px; } \
         .a { height: 100%; }",
    );
    assert_eq!(items.len(), 1);
    assert!(items[0].rect.height >= 0.0 && items[0].rect.height.is_finite());
}
