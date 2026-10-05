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

#[test]
fn a_break_at_the_end_of_a_track_drops_the_whole_row_gap() {
    // Two 100px rows with a 10px gap in 100px columns: the first window ends exactly where the
    // gap begins, so the gap is dropped and the second fragment starts at the next track
    // (without the fix a third fragment holding the tail of the gap appeared).
    let root = lay(
        "<div id=\"m\"><div id=\"g\"><div></div><div></div></div></div>",
        "body{margin:0} #m{columns:3;column-fill:auto;column-gap:0;width:300px;height:100px} \
         #g{display:grid;grid-template-rows:repeat(2,100px);row-gap:10px}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    let h: Vec<f32> = v.iter().map(|b| b.rect.height).collect();
    assert_eq!(h, vec![100.0, 100.0], "{h:?}");
}

const FLEX_HTML: &str =
    "<div id=\"m\"><div id=\"f\"><div></div><div></div><div></div><div></div><div></div><div></div></div></div>";

fn flex_frags(extra: &str, fill: &str) -> Vec<(f32, f32, usize)> {
    let root = lay(
        FLEX_HTML,
        &format!(
            "body{{margin:0}} #m{{columns:2;{fill}column-gap:10px;width:290px;height:87px}}              #f{{display:flex;flex-wrap:wrap;width:140px;height:180px;{extra}}} #f>div{{width:70px;height:50px}}"
        ),
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    v.iter().map(|b| (b.rect.x, b.rect.height, b.subgrid_tracks.as_ref().unwrap().rows.as_ref().unwrap().len())).collect()
}

#[test]
fn a_wrapped_flex_row_container_is_cut_by_its_lines() {
    // Three 50px lines spread by `align-content: space-between` over 180px: lines at 0..50,
    // 65..115, 130..180; the 87px window ends inside the second one, the next starts at 87.
    let v = flex_frags("align-content:space-between;", "");
    assert_eq!(v.len(), 3, "{v:?}");
    assert_eq!(v.iter().map(|f| f.0).collect::<Vec<_>>(), vec![0.0, 150.0, 300.0], "{v:?}");
    assert_eq!(v[0].2, 2, "two lines in the first fragment: {v:?}");
}

#[test]
fn a_flex_container_without_two_lines_stays_atomic() {
    let root = lay(
        "<div id=\"m\"><div id=\"f\"><div></div></div></div>",
        "body{margin:0} #m{columns:2;column-fill:auto;width:290px;height:30px}          #f{display:flex;flex-wrap:wrap;height:80px} #f>div{width:70px;height:50px}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    assert!(v.is_empty(), "{} fragments", v.len());
}
