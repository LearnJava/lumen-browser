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

struct Half;
impl crate::TextMeasurer for Half {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

fn lanes_measured(html: &str, css: &str) -> LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout_measured(&doc, &sheet, Size::new(400.0, 400.0), &Half);
    fn find(b: &LayoutBox) -> Option<&LayoutBox> {
        if matches!(b.kind, BoxKind::Block) && b.style.grid_lanes.enabled {
            return Some(b);
        }
        b.children.iter().find_map(find)
    }
    find(&root).expect("grid-lanes container").clone()
}

#[test]
fn edge_whitespace_does_not_widen_max_content() {
    // `\n  ab\n` и `ab` одной ширины: пробелы по краям строки не считаются (CSS Text L3 §4.1.1).
    let css = "body { margin: 0 } .g { display: grid-lanes; grid-template-columns: repeat(auto-fill, auto); width: 100px; \
               flow-tolerance: 0 }";
    let spaced = lanes_measured("<div class='g'><div>\n   ab\n </div><div>\n   ab\n </div></div>", css);
    let tight = lanes_measured("<div class='g'><div>ab</div><div>ab</div></div>", css);
    assert_eq!(positions(&spaced), positions(&tight));
}

#[test]
fn intrinsic_auto_repeat_counts_by_content() {
    // Три элемента шириной 50 в 160px: `repeat(auto-fill, auto)` — 3 полосы (160 / 50), а не одна.
    let g = lanes(
        "<div class='g'><div style='width:50px;height:10px'></div><div style='width:50px;height:10px'></div>\
         <div style='width:50px;height:10px'></div></div>",
        "body { margin: 0 } .g { display: grid-lanes; grid-template-columns: repeat(auto-fill, auto); width: 160px; \
              flow-tolerance: 0 }",
    );
    let p = positions(&g);
    assert_eq!(p[0], (0.0, 0.0));
    assert!((p[1].0 - 160.0 / 3.0).abs() < 0.01 && p[1].1 == 0.0, "{p:?}");
    assert!((p[2].0 - 320.0 / 3.0).abs() < 0.01 && p[2].1 == 0.0, "{p:?}");
}

#[test]
fn alignment_area_reaches_the_next_item_of_the_lane() {
    // Область `align-items: stretch` тянется до следующего элемента полосы (минус промежуток):
    // A (авто-высота) в полосе 0 доходит до начала C, которое лежит под B (80) через gap.
    let g = lanes(
        "<div class='g'><div></div><div style='height:80px'></div><div style='grid-column: span 2; height:10px'></div></div>",
        "body { margin: 0 } .g { display: grid-lanes; grid-template-columns: 100px 100px; gap: 10px; width: 300px; \
              height: 300px; flow-tolerance: 0; align-items: stretch }",
    );
    let blocks: Vec<_> = g.children.iter().filter(|c| matches!(c.kind, BoxKind::Block)).collect();
    assert_eq!(blocks[0].rect.height, 80.0);
    assert_eq!(blocks[2].rect.y, 90.0);
}
