//! BUG-1313: CSS Box Alignment L3 §5.3 / Grid L1 §11.5 —
//! (1) `space-around` / `space-evenly` fall back to `safe center` on overflow, plain
//! `center` / `end` stay unsafe and `safe` clamps them to the start edge;
//! (2) `minmax(auto, <length>)` takes the items' minimum contribution as its base size and
//! raises the growth limit to it when the length is smaller.

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

/// Position of the only item of a 50 × 50 grid holding a 100 × 100 item, relative to the
/// container, with `content` as `align-content` and `justify-content`.
fn overflow_offset(content: &str) -> (f32, f32) {
    let css = format!(
        "body {{ margin: 0 }} .g {{ display: grid; width: 50px; height: 50px; \
         align-content: {content}; justify-content: {content}; \
         grid-template: 100px / 100px; }} \
         .a {{ width: 100px; height: 100px; }}"
    );
    let doc = lumen_html_parser::parse("<div class='g'><div class='a'></div></div>");
    let sheet = lumen_css_parser::parse(&css);
    let root = layout(&doc, &sheet, Size::new(400.0, 400.0));
    fn find(b: &LayoutBox) -> Option<&LayoutBox> {
        if matches!(b.kind, BoxKind::Block) && b.style.display == crate::style::Display::Grid {
            return Some(b);
        }
        b.children.iter().find_map(find)
    }
    let grid = find(&root).expect("grid container");
    let item = &grid.children[0];
    (item.rect.x - grid.rect.x, item.rect.y - grid.rect.y)
}

#[test]
fn space_evenly_and_around_fall_back_to_safe_center() {
    assert_eq!(overflow_offset("space-evenly"), (0.0, 0.0));
    assert_eq!(overflow_offset("space-around"), (0.0, 0.0));
}

#[test]
fn plain_center_and_end_overflow_unsafely() {
    assert_eq!(overflow_offset("center"), (-25.0, -25.0));
    assert_eq!(overflow_offset("end"), (-50.0, -50.0));
}

#[test]
fn safe_keyword_clamps_overflow_to_start() {
    assert_eq!(overflow_offset("safe center"), (0.0, 0.0));
    assert_eq!(overflow_offset("safe end"), (0.0, 0.0));
    assert_eq!(overflow_offset("unsafe center"), (-25.0, -25.0));
}

#[test]
fn minmax_auto_with_small_max_grows_to_min_content() {
    let items = grid_items(
        "<div class='g'><div class='a'></div><div class='b'></div></div>",
        ".g { display: grid; width: 100px; grid: 10px 10px / minmax(auto, 0px); } \
         .a { width: 60px; }",
    );
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].rect.width, 60.0);
    assert_eq!(items[1].rect.width, 60.0);
}

#[test]
fn minmax_auto_base_counts_item_margin() {
    let items = grid_items(
        "<div class='g'><div class='a'></div><div class='b'></div></div>",
        ".g { display: grid; width: 100px; grid: 10px 10px / minmax(auto, 0px); } \
         .a { width: 60px; margin-left: 5px; }",
    );
    assert_eq!(items[0].rect.width, 60.0);
    assert_eq!(items[1].rect.width, 65.0);
}

#[test]
fn minmax_auto_with_large_max_stops_at_the_length() {
    let items = grid_items(
        "<div class='g'><div class='a'></div></div>",
        ".g { display: grid; width: 200px; grid-template-columns: minmax(auto, 80px); } \
         .a { width: 30px; }",
    );
    // base 30 grows towards the 80px limit with the free space, no further.
    assert_eq!(items[0].rect.width, 30.0);
    let stretched = grid_items(
        "<div class='g'><div class='a'></div></div>",
        ".g { display: grid; width: 200px; grid-template-columns: minmax(auto, 80px); }",
    );
    assert_eq!(stretched[0].rect.width, 80.0);
}

#[test]
fn minmax_auto_base_honours_item_min_width() {
    let items = grid_items(
        "<div class='g'><div class='a'></div><div class='b'></div></div>",
        ".g { display: grid; width: 100px; grid: 10px 10px / minmax(auto, 0px); } \
         .a { min-width: 60px; padding-left: 6px; }",
    );
    assert_eq!(items[0].rect.width, 66.0);
    assert_eq!(items[1].rect.width, 66.0);
}

#[test]
fn inline_grid_with_minmax_auto_track_shrinks_to_min_content() {
    let doc = lumen_html_parser::parse("<div class='g'><div class='a'></div><div class='b'></div></div>");
    let sheet = lumen_css_parser::parse(
        "body { margin: 0 } .g { display: inline-grid; border: solid 5px; \
         grid: 10px 10px / minmax(auto, 0px); } .a { width: 60px; }",
    );
    let root = layout(&doc, &sheet, Size::new(400.0, 400.0));
    fn find(b: &LayoutBox) -> Option<&LayoutBox> {
        if b.style.display == crate::style::Display::InlineGrid {
            return Some(b);
        }
        b.children.iter().find_map(find)
    }
    let grid = find(&root).expect("inline-grid container");
    assert_eq!(grid.rect.width, 70.0);
    assert_eq!(grid.children[1].rect.width, 60.0);
}
