//! CSS Fragmentation L3 §5 / CSS Gap Decorations L1 §6.2 — a plain grid container in a
//! `column-fill: auto` multicol container is cut into one fragment per column, and a row gap that
//! a break falls in is dropped (WPT `css/css-gaps/grid/fragmentation/grid-gap-decorations-
//! fragmentation-001/006`: three 80px rows with a 30px row gap in three 100px columns).

use lumen_core::geom::Size;

use crate::box_tree::LayoutBox;

fn lay(html: &str, css: &str) -> LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    super::super::layout(&doc, &sheet, Size::new(800.0, 600.0))
}

/// Every box that is a grid fragment (`SubgridTracks::fragment`), in tree order.
fn fragments<'a>(b: &'a LayoutBox, out: &mut Vec<&'a LayoutBox>) {
    if b.subgrid_tracks.as_ref().is_some_and(|t| t.fragment) {
        out.push(b);
    }
    for c in &b.children {
        fragments(c, out);
    }
}

const HTML: &str = "<div id=\"m\"><div id=\"g\"><div></div><div></div><div></div></div></div>";

fn frags(extra: &str) -> Vec<(f32, f32, f32, usize, usize)> {
    let root = lay(
        HTML,
        &format!(
            "body{{margin:0}} #m{{columns:3;column-fill:auto;column-gap:0;width:300px;height:100px}} \
             #g{{display:grid;grid-template-rows:repeat(3,80px);row-gap:30px;{extra}}} #g>div{{background:green}}"
        ),
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    v.iter()
        .map(|b| {
            let rows = b.subgrid_tracks.as_ref().unwrap().rows.as_ref().unwrap().len();
            let items = b.children.iter().filter(|c| c.rect.height > 0.0).count();
            (b.rect.x, b.rect.y, b.rect.height, rows, items)
        })
        .collect()
}

#[test]
fn a_row_gap_at_a_column_break_is_dropped_and_each_fragment_keeps_one_track() {
    // Rows end at 80/190/300 and the windows are 0..100, 110..210, 220..300: the break in
    // every gap skips the rest of it.
    let v = frags("");
    assert_eq!(v.len(), 3, "{v:?}");
    assert_eq!(v.iter().map(|f| f.0).collect::<Vec<_>>(), vec![0.0, 100.0, 200.0], "{v:?}");
    assert_eq!(v.iter().map(|f| f.2).collect::<Vec<_>>(), vec![100.0, 100.0, 80.0], "{v:?}");
    assert!(v.iter().all(|f| f.3 == 1 && f.4 == 1), "one track and one item per fragment: {v:?}");
}

#[test]
fn a_grid_with_a_forced_break_inside_stays_atomic() {
    let root = lay(
        "<div id=\"m\"><div id=\"g\"><div></div><div style=\"break-before:column\"></div></div></div>",
        "body{margin:0} #m{columns:3;column-fill:auto;column-gap:0;width:300px;height:100px} \
         #g{display:grid;grid-template-rows:repeat(2,80px);row-gap:30px}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    assert!(v.is_empty(), "no fragments expected");
}
