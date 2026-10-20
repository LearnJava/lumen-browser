//! CSS Grid L3 §2–3: `display: grid-lanes` кладёт элемент в самую короткую полосу.

use lumen_core::geom::Size;

use super::super::{layout, BoxKind, LayoutBox};

fn lanes(html: &str, css: &str) -> LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = layout(&doc, &sheet, Size::new(400.0, 400.0));
    fn find(b: &LayoutBox) -> Option<&LayoutBox> {
        if matches!(b.kind, BoxKind::Block) && b.style.grid_lanes.enabled {
            return Some(b);
        }
        b.children.iter().find_map(find)
    }
    find(&root).expect("grid-lanes container").clone()
}

const ITEMS: &str = "<div class='g'><div style='height:50px'></div><div style='height:30px'></div>\
                     <div style='height:20px'></div><div style='height:20px'></div></div>";

fn positions(g: &LayoutBox) -> Vec<(f32, f32)> {
    g.children.iter().filter(|c| matches!(c.kind, BoxKind::Block)).map(|c| (c.rect.x, c.rect.y)).collect()
}

#[test]
fn items_fall_into_the_shortest_lane() {
    let g = lanes(
        ITEMS,
        "body { margin: 0 } .g { display: grid-lanes; grid-template-columns: 100px 100px; gap: 10px; width: 300px; \
              flow-tolerance: 0 }",
    );
    // 50 → полоса 0, 30 → полоса 1, 20 → полоса 1 (40 < 60), 20 → полоса 0 (60 < 70).
    assert_eq!(positions(&g), vec![(0.0, 0.0), (110.0, 0.0), (110.0, 40.0), (0.0, 60.0)]);
    assert_eq!(g.rect.height, 80.0);
}

#[test]
fn default_tolerance_keeps_items_in_the_first_lane() {
    // `normal` = 1em = 16px: полосы 50 и 30 различаются на 20 > 16, а 20/20 на 10 внутри допуска.
    let g = lanes(
        ITEMS,
        "body { margin: 0 } .g { display: grid-lanes; grid-template-columns: 100px 100px; width: 300px }",
    );
    let p = positions(&g);
    assert_eq!(p[1], (100.0, 0.0));
    assert_eq!(p[2], (100.0, 30.0));
    // после 20px: полосы 50 и 50 равны — первая.
    assert_eq!(p[3], (0.0, 50.0));
}

#[test]
fn display_resets_the_lanes_flag() {
    let doc = lumen_html_parser::parse("<div class='g'></div>");
    let sheet = lumen_css_parser::parse("body { margin: 0 } .g { display: grid-lanes; display: flex; }");
    let root = layout(&doc, &sheet, Size::new(400.0, 400.0));
    fn any(b: &LayoutBox) -> bool {
        b.style.grid_lanes.enabled || b.children.iter().any(any)
    }
    assert!(!any(&root));
}
