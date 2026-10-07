//! BUG-1253, CSS Flexbox L1 §4.5: the automatic minimum size also bounds
//! `flex-grow` distribution and column shrinking, not just a row's shrink.

use lumen_core::geom::Size;

use super::super::{layout, BoxKind, LayoutBox};

fn flex_items(html: &str, css: &str) -> Vec<LayoutBox> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = layout(&doc, &sheet, Size::new(400.0, 400.0));
    fn find(b: &LayoutBox) -> Option<&LayoutBox> {
        if matches!(b.kind, BoxKind::Block) && b.style.display == crate::style::Display::Flex {
            return Some(b);
        }
        b.children.iter().find_map(find)
    }
    find(&root).expect("flex container").children.clone()
}

#[test]
fn row_flex_one_zero_basis_keeps_content_width() {
    let items = flex_items(
        "<div class='f'><div class='i'><div class='c'></div></div><div class='i'><div class='c'></div></div></div>",
        ".f { display: flex; width: 100px; } .i { flex: 1 1 0; } .c { width: 90px; height: 10px; }",
    );
    assert_eq!(items[0].rect.width, 90.0);
    assert_eq!(items[1].rect.width, 90.0);
}

#[test]
fn column_definite_height_keeps_content_height() {
    let items = flex_items(
        "<div class='f'><div class='i'><div class='c'></div></div><div class='i'><div class='c'></div></div></div>",
        ".f { display: flex; flex-direction: column; width: 100px; height: 80px; } \
         .i { flex: 1 1 0; } .c { height: 60px; }",
    );
    assert_eq!(items[0].rect.height, 60.0);
    assert_eq!(items[1].rect.height, 60.0);
}
