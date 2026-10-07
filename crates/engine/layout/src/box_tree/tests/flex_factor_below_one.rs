//! BUG-1260, CSS Flexbox L1 §9.7 п. 4.c: при сумме `flex-grow` < 1 делится
//! лишь эта доля свободного места.

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
fn single_item_grow_half_takes_half() {
    let items = flex_items(
        "<div class='f'><div class='i'></div></div>",
        ".f { display: flex; width: 100px; } .i { flex: .5 0 0; height: 10px; }",
    );
    assert_eq!(items[0].rect.width, 50.0);
}

#[test]
fn two_items_fractional_grow() {
    let items = flex_items(
        "<div class='f'><div class='a'></div><div class='b'></div></div>",
        ".f { display: flex; width: 100px; } .a { flex: .5 0 0; height: 10px; } \
         .b { flex: .25 0 0; height: 10px; }",
    );
    assert_eq!(items[0].rect.width, 50.0);
    assert_eq!(items[1].rect.width, 25.0);
}
