//! BUG-1255, CSS Sizing L3 §5.2.1: a percentage `height` of a column flex item
//! resolves against the container's definite main size.

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
fn percent_height_resolves_against_definite_column() {
    let items = flex_items(
        "<div class='f'><div class='a'></div></div>",
        ".f { display: flex; flex-direction: column; width: 100px; height: 100px; } \
         .a { height: 50%; }",
    );
    assert_eq!(items[0].rect.height, 50.0);
}

#[test]
fn percent_height_in_auto_column_stays_auto() {
    let items = flex_items(
        "<div class='f'><div class='a'></div></div>",
        ".f { display: flex; flex-direction: column; width: 100px; } .a { height: 50%; }",
    );
    assert_eq!(items[0].rect.height, 0.0);
}
