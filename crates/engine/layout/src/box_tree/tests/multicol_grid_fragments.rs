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
fn a_fragment_knows_which_row_gaps_of_the_container_it_starts_at() {
    // Three tracks, two gaps; each fragment holds one track, so it starts at gap 0, 1, 2 (the
    // last one past the end) out of the container's two (`row-rule-color` lists are dealt over
    // the gaps of the whole container, not of the fragment).
    let root = lay(
        HTML,
        "body{margin:0} #m{columns:3;column-fill:auto;column-gap:0;width:300px;height:100px}          #g{display:grid;grid-template-rows:repeat(3,80px);row-gap:30px}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    let bases: Vec<_> = v.iter().map(|b| b.subgrid_tracks.as_ref().unwrap().row_gap_base).collect();
    assert_eq!(bases, vec![Some((0, 2)), Some((1, 2)), Some((2, 2))], "{bases:?}");
}

#[test]
fn a_gap_kept_at_a_break_by_a_bridging_item_is_taken_over_by_the_next_track() {
    // Two 100px rows with a 10px gap; the second column of the grid is one item spanning both
    // rows, so the break at 100px (the leading edge of the gap) keeps the gap. The next
    // fragment has no gap to draw at its top: its first track grows upwards over the gap and
    // starts at the fragment's own top (WPT `grid-gap-decorations-fragmentation-028`).
    let root = lay(
        "<div id=\"m\"><div id=\"g\"><div></div><div style=\"grid-row:1/3;grid-column:2\"></div><div style=\"grid-row:2;grid-column:1\"></div></div></div>",
        "body{margin:0} #m{columns:3;column-fill:auto;column-gap:0;width:300px;height:100px}          #g{display:grid;grid-template-columns:50px 50px;grid-template-rows:repeat(2,100px);row-gap:10px}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    let rows: Vec<_> = v.iter().map(|b| b.subgrid_tracks.as_ref().unwrap().rows.clone().unwrap()).collect();
    assert_eq!(rows[1][0], (0.0, 100.0), "{rows:?}");
    let second_item = v[1].children.iter().find(|c| c.rect.x < 100.0 + 50.0 && c.rect.height > 0.0).unwrap();
    assert_eq!(second_item.rect.y, v[1].rect.y, "{rows:?}");
}

fn forced_break_frags(fill: &str, item: &str) -> Vec<(f32, f32, usize)> {
    let root = lay(
        &format!("<div id=\"m\"><div id=\"g\"><div></div><div></div><div {item}></div></div></div>"),
        &format!(
            "body{{margin:0}} #m{{columns:3;{fill}column-gap:0;width:300px;height:100px}}              #g{{display:grid;grid-template-rows:repeat(3,20px);row-gap:10px}}"
        ),
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    v.iter().map(|b| (b.rect.x, b.rect.height, b.children.iter().filter(|c| c.rect.height > 0.0).count())).collect()
}

#[test]
fn a_forced_column_break_inside_a_grid_cuts_at_the_item_and_keeps_the_column_height() {
    // Rows 0..20, 30..50, 60..80; `break-before: column` on the third item starts the second
    // fragment at its track. The first column is cut there and keeps its full 100px (the column
    // gaps run through it), the second one is the 20px track.
    let v = forced_break_frags("column-fill:auto;", "style=\"break-before:column\"");
    assert_eq!(v, vec![(0.0, 100.0, 2), (100.0, 20.0, 1)], "{v:?}");
}

#[test]
fn a_forced_break_in_a_balanced_multicol_keeps_the_grid_atomic() {
    let v = forced_break_frags("", "style=\"break-before:column\"");
    assert!(v.is_empty(), "{v:?}");
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

#[test]
fn a_balanced_wrapped_flex_row_is_cut_in_three_columns_under_the_limit() {
    // Five 50px lines with 10px gaps = 290px in three balanced columns under a 97px limit
    // (flex/fragmentation/026: ten items, two per line). 290 < 3 x 97, so the container used to
    // stay atomic in column 0; it is now cut into three fragments, none taller than the limit,
    // and the cuts at ~97 and ~193 fall inside the lines (no row gap is dropped).
    let root = lay(
        "<div id=\"m\"><div id=\"f\"><div></div><div></div><div></div><div></div><div></div><div></div><div></div><div></div><div></div><div></div></div></div>",
        "body{margin:0} #m{columns:3;column-gap:10px;width:350px;height:97px} \
         #f{display:flex;flex-wrap:wrap;width:110px;row-gap:10px;column-gap:10px} #f>div{width:50px;height:50px}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    let h: Vec<f32> = v.iter().map(|b| b.rect.height).collect();
    assert_eq!(h.len(), 3, "{h:?}");
    assert!(h.iter().all(|&x| x <= 97.0 + 0.01), "{h:?}");
    assert!(h[0] > 96.0, "{h:?}");
}

fn bordered_flex(items: &str) -> Vec<LayoutBox> {
    // Two 44px lines + 10px gap in 60px-tall columns (`column-fill: auto`), a 2px border on the
    // container (flex/fragmentation/010: the border is cut with the box).
    let root = lay(
        &format!("<div id=\"m\"><div id=\"f\">{items}</div></div>"),
        "body{margin:0} #m{columns:3;column-fill:auto;column-gap:10px;width:350px;height:60px} \
         #f{display:flex;flex-wrap:wrap;width:110px;border:2px solid;row-gap:10px;column-gap:10px} \
         #f>div{width:50px;height:44px}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    v.into_iter().cloned().collect()
}

#[test]
fn a_bordered_flex_container_is_cut_and_keeps_only_the_border_edges_it_owns() {
    let v = bordered_flex("<div></div><div></div><div></div><div></div><div></div><div></div>");
    assert_eq!(v.len(), 3, "{} fragments", v.len());
    let (top, bottom): (Vec<f32>, Vec<f32>) =
        v.iter().map(|b| (b.style.border_top_width, b.style.border_bottom_width)).unzip();
    assert_eq!(top, vec![2.0, 0.0, 0.0], "only the first fragment keeps the top border");
    assert_eq!(bottom, vec![0.0, 0.0, 2.0], "only the last fragment keeps the bottom border");
    // Row tracks are measured from the content box; the first fragment's window starts at the
    // border edge, so its first track sits 2px down, later ones at the top.
    let first = v[0].subgrid_tracks.as_ref().unwrap().rows.as_ref().unwrap()[0];
    assert!((first.0 - 0.0).abs() < 0.01, "{first:?}");
}

#[test]
fn an_item_whose_child_straddles_a_break_keeps_the_container_atomic() {
    // The window edge at 60px falls inside the first item's 44px child only when the cut is not
    // at a track boundary; items with a child that is cut through must not be split (a child with content of its own;
    // an empty one is clipped, see below).
    let v = bordered_flex(
        "<div><p style=\"margin:0\"><b style=\"display:block;height:100px\"></b></p></div><div></div><div></div><div></div>",
    );
    assert!(v.is_empty(), "{} fragments", v.len());
}

#[test]
fn a_flex_fragment_keeps_the_line_gaps_of_items_that_stayed_in_an_earlier_column() {
    // flex/fragmentation/011: a 25px first item and a 50px second one share a line; the cut at
    // 47px leaves the first one whole in column 0, but the gap next to it (x 52..62 from the box)
    // still runs through the part of the line that continues in column 1.
    let root = lay(
        "<div id=\"m\"><div id=\"f\"><div id=\"one\"></div><div></div><div></div><div></div></div></div>",
        "body{margin:0} #m{columns:3;column-fill:auto;column-gap:10px;width:350px;height:47px} \
         #f{display:flex;flex-wrap:wrap;width:110px;height:110px;border:2px solid;row-gap:10px;column-gap:10px} \
         #f>div{width:50px;height:50px} #one{height:25px}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    assert!(v.len() >= 2, "{} fragments", v.len());
    let second = v[1].subgrid_tracks.as_ref().unwrap().line_gaps.as_ref().expect("line gaps");
    let first_line = &second[0];
    assert_eq!(first_line.len(), 1, "{second:?}");
    assert!((first_line[0].0 - 52.0).abs() < 0.01 && (first_line[0].1 - 62.0).abs() < 0.01, "{first_line:?}");
}

#[test]
fn a_monolithic_flex_item_stays_whole_in_its_column_and_keeps_its_line_gap_there() {
    // flex/fragmentation/012: two 55px lines, columns 47px tall; the second item of line 1 is
    // `contain: size` (monolithic). It is not cut: it stays whole in column 0 (overflowing it),
    // so the gap in front of it belongs to column 0 and the line remnant in column 1 has none.
    let root = lay(
        "<div id=\"m\"><div id=\"f\"><div></div><div id=\"mono\"></div><div></div><div></div></div></div>",
        "body{margin:0} #m{columns:3;column-fill:auto;column-gap:10px;width:350px;height:47px} \
         #f{display:flex;flex-wrap:wrap;width:110px;height:110px;row-gap:10px;column-gap:10px} \
         #f>div{width:50px;height:55px} #mono{contain:size}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    assert!(v.len() >= 2, "{} fragments", v.len());
    let heights: Vec<f32> = v[0].children.iter().map(|c| c.rect.height).collect();
    assert!(heights.contains(&55.0), "the monolithic item keeps its height in column 0: {heights:?}");
    assert!(v[1].children.iter().all(|c| c.rect.height < 55.0), "no whole item in column 1");
    let first = v[0].subgrid_tracks.as_ref().unwrap().line_gaps.as_ref().expect("line gaps");
    assert_eq!(first[0].len(), 1, "{first:?}");
    let second = v[1].subgrid_tracks.as_ref().unwrap().line_gaps.as_ref().expect("line gaps");
    assert!(second[0].is_empty(), "the line remnant in column 1 has no gap: {second:?}");
}

#[test]
fn a_balanced_column_reaches_the_bottom_of_a_monolithic_item_the_next_column_starts_there() {
    // flex/fragmentation/021: a balanced 47px container, a 110px wrapped flex whose second item is
    // `contain: size`. Column 0 grows to the item's bottom (55px) instead of being cut at the balanced
    // 37px; with `column-fill: auto` (012) the overflow does not move the next column.
    let root = lay(
        "<div id=\"m\"><div id=\"f\"><div>1</div><div id=\"mono\">2</div><div>3</div><div>4</div></div></div>",
        "body{margin:0} #m{columns:3;column-gap:10px;width:350px;height:47px} \
         #f{display:flex;flex-wrap:wrap;width:110px;height:110px;column-gap:10px} \
         #f>div{width:50px} #mono{contain:size}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    assert!(v.len() >= 2, "{} fragments", v.len());
    let first = v[0].rect.height;
    assert!((first - 55.0).abs() < 0.01, "column 0 reaches the item's bottom (past the 47px limit): {first}");
    let line0 = v[0].subgrid_tracks.as_ref().unwrap().rows.as_ref().unwrap()[0];
    assert!(line0.1 - line0.0 >= 54.9, "the first line reaches the monolithic item's bottom: {line0:?}");
}

#[test]
fn a_wrapped_column_flex_is_cut_by_its_items_and_keeps_a_gap_the_break_falls_in_front_of() {
    // flex/fragmentation/014: 110px column flex (two lines of 50px items, `row-gap: 10px`) in
    // 47px columns; the first item is `contain: size`, so column 0 reaches its bottom (50px), the
    // break falls at the leading edge of the 10px gap that follows, and column 1 starts at that
    // gap (rule heights 50/47/13 in the reference).
    let root = lay(
        "<div id=\"m\"><div id=\"f\"><div id=\"mono\">One</div><div>Two</div><div>Three</div><div>Four</div></div></div>",
        "body{margin:0} #m{columns:3;column-fill:auto;column-gap:10px;width:350px;height:47px} \
         #f{display:flex;flex-direction:column;flex-wrap:wrap;width:110px;height:110px;row-gap:10px;column-gap:10px} \
         #f>div{width:50px;height:50px} #mono{contain:size}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    let heights: Vec<f32> = v.iter().map(|b| b.rect.height).collect();
    assert_eq!(heights, vec![50.0, 47.0, 13.0], "{heights:?}");
}

#[test]
fn a_column_flex_fragment_after_a_dropped_gap_keeps_the_block_size_the_gap_took() {
    // flex/fragmentation/006: 200px column flex (two columns of four 30px items, `row-gap: 20px`,
    // centred) in a 150px multicol, balanced to 100px columns. The break falls inside the gap
    // before item 4; its rest is dropped, but the second fragment is as tall as the first
    // (102px with the borders) instead of losing the dropped part of the gap, so the column
    // rule of the second column runs 100px as in the reference.
    let root = lay(
        "<div id=\"m\"><div id=\"f\"><div>1</div><div>2</div><div>3</div><div>4</div><div>5</div><div>6</div><div>7</div><div>8</div></div></div>",
        "body{margin:0} #m{columns:2;column-width:100px;height:150px;width:330px} \
         #f{border:2px solid #688;display:flex;column-gap:10px;row-gap:20px;width:90px;flex-wrap:wrap;\
         flex-direction:column;height:200px;align-items:center;justify-content:center} \
         #f>div{width:30px;height:30px}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    let heights: Vec<f32> = v.iter().map(|b| b.rect.height).collect();
    assert_eq!(heights, vec![102.0, 102.0], "{heights:?}");
}

#[test]
fn a_column_flex_break_inside_a_line_box_moves_up_to_the_top_of_the_line() {
    // flex/fragmentation/009: four 30px items with a text line each in a 170px column flex, cut
    // by 90px columns. The first break falls inside the text line of item 3 (the items sit at
    // y 2/42/82/122), so it moves up to the top of that line and the item goes whole into the
    // second column: two items per fragment instead of item 3 being cut in two.
    let root = lay(
        "<div id=\"m\"><div id=\"f\"><div>1</div><div>2</div><div>3</div><div>4</div></div></div>",
        "body{margin:0} #m{columns:2;column-width:100px;height:90px;width:330px} \
         #f{border:2px solid #688;display:flex;column-gap:10px;row-gap:10px;width:90px;flex-wrap:wrap;\
         flex-direction:column;height:170px} #f>div{width:30px;height:30px}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    let items: Vec<usize> = v.iter().map(|b| b.children.len()).collect();
    assert_eq!(items, vec![2, 2], "{items:?}");
}

#[test]
fn a_column_flex_break_inside_an_item_whose_line_cannot_fit_pushes_the_line_not_the_break() {
    // flex/fragmentation/013: 110px wrapped column flex (line 1: 25px + 50px items, line 2: two
    // 50px items), `row-gap: 10px`, in 47px columns. The break at 47px falls inside the text line
    // of the second item of line 1. Chrome cuts the container at 47/94 (rule heights 45/47/18); only
    // that item moves to the next column, the lines next to it keep their geometry.
    let root = lay(
        "<div id=\"m\"><div id=\"f\"><div id=\"one\">One</div><div>Two</div><div>Three</div><div>Four</div></div></div>",
        "body{margin:0} #m{columns:3;height:47px;column-width:110px;width:330px} \
         #f{border:2px solid #688;display:flex;column-gap:10px;row-gap:10px;width:110px;flex-wrap:wrap;\
         height:110px;flex-direction:column;column-rule:10px solid blue;row-rule:10px solid gold;row-rule-break:intersection;\n         row-rule-inset:0} #f>div{width:50px;height:50px} #one{height:25px}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    let r: Vec<(f32, f32, f32)> = v.iter().map(|b| (b.rect.x, b.rect.y, b.rect.height)).collect();
    assert_eq!(r.iter().map(|f| f.2).collect::<Vec<_>>(), vec![47.0, 47.0, 20.0], "{r:?}");
}

#[test]
fn an_empty_grid_is_cut_by_its_row_tracks_and_drops_the_gaps_at_the_breaks() {
    // grid-gap-decorations-fragmentation-016: 13 rows of 10px with a 10px row gap (250px) and no
    // items, in three 92px columns. Rows end at 10/30/…; the breaks at 92 and 184 fall inside a
    // track, so every column is cut there (92/92/66px) and keeps its own row tracks.
    let root = lay(
        "<div id=\"m\"><div id=\"g\"></div></div>",
        "body{margin:0} #m{columns:3;column-fill:auto;column-gap:10px;width:320px;height:92px} \
         #g{display:grid;grid-template-columns:repeat(2,1fr);grid-template-rows:repeat(13,10px);\
         column-gap:10px;row-gap:10px;row-rule:solid 5px red;column-rule:solid 6px blue}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    let h: Vec<f32> = v.iter().map(|b| b.rect.height).collect();
    assert_eq!(h.len(), 3, "{h:?}");
    assert!(h[0] > 90.0 && h[1] > 90.0 && h[2] < h[0], "{h:?}");
}

#[test]
fn a_subgrid_item_cut_by_the_columns_keeps_only_the_tracks_of_its_window() {
    // subgrid-gap-decorations-fragmentation-001: a subgrid spanning the three 80px rows (30px
    // gap) of a grid cut by 100px columns. The subgrid is cut with the container, so each of its
    // fragments knows only the one track in its window and the gap a break splits is not painted.
    let root = lay(
        "<div id=\"m\"><div id=\"g\"><div id=\"s\"><div></div><div></div><div></div></div></div></div>",
        "body{margin:0} #m{columns:3;column-fill:auto;column-gap:0;width:300px;height:100px} \
         #g{display:grid;grid-template-rows:repeat(3,80px);row-gap:30px} \
         #s{display:grid;grid-template-columns:subgrid;grid-template-rows:subgrid;grid-column:1/-1;\
         grid-row:1/-1;row-rule:solid 10px}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    let subs: Vec<usize> = v
        .iter()
        .flat_map(|g| g.children.iter())
        .filter_map(|c| c.subgrid_tracks.as_ref().filter(|t| t.fragment))
        .map(|t| t.rows.as_ref().unwrap().len())
        .collect();
    assert_eq!(subs, vec![1, 1, 1], "{subs:?}");
}

#[test]
fn a_subgrid_fragment_starting_in_its_own_row_gap_drops_the_gap_at_its_top() {
    // subgrid-gap-decorations-fragmentation-015: an empty subgrid on rows 2..-2 of a 5×(~20px)
    // grid with 10px gaps, cut by 80px columns. The second window starts inside the subgrid's
    // gap between its tracks: that gap is not drawn at the top of the column, so the first track
    // of the fragment starts at 0 and the fragment is that much shorter.
    let root = lay(
        "<div id=\"m\"><div id=\"g\"><div id=\"s\"></div></div></div>",
        "body{margin:0} #m{columns:2;column-fill:auto;column-gap:0;width:280px;height:80px} \
         #g{display:grid;grid-template-columns:repeat(5,1fr);grid-template-rows:repeat(5,1fr);\
         gap:10px;height:140px} \
         #s{display:grid;grid-template-columns:subgrid;grid-template-rows:subgrid;\
         grid-column:2/-2;grid-row:2/-2;row-rule:blue solid 10px}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    let firsts: Vec<f32> = v
        .iter()
        .flat_map(|g| g.children.iter())
        .filter_map(|c| c.subgrid_tracks.as_ref().filter(|t| t.fragment))
        .filter_map(|t| t.rows.as_ref().and_then(|r| r.first().map(|f| f.0)))
        .collect();
    assert!(!firsts.is_empty(), "no subgrid fragments");
    assert!(firsts.iter().all(|&y| y.abs() < 0.01), "{firsts:?}");
}

#[test]
fn an_empty_child_of_an_item_that_a_break_runs_through_is_cut_with_the_item() {
    // An item on all three 60px rows holds an empty 150px block (an empty subgrid cell in
    // `subgrid-gap-decorations-fragmentation-007`). The break at 100px runs through the block:
    // before, such a child kept the whole grid atomic; now each window keeps its part of it.
    let root = lay(
        "<div id=\"m\"><div id=\"g\"><div id=\"i\"><div id=\"k\"></div></div></div></div>",
        "body{margin:0} #m{columns:2;column-fill:auto;column-gap:0;width:200px;height:100px} \
         #g{display:grid;grid-template-rows:repeat(3,60px);row-gap:10px} \
         #i{grid-row:1/-1} #k{height:150px}",
    );
    let mut v = Vec::new();
    fragments(&root, &mut v);
    assert_eq!(v.len(), 2, "the grid is cut in two columns");
    let parts: Vec<f32> = v
        .iter()
        .map(|g| {
            let item = g.children.iter().find(|c| c.rect.height > 0.0).expect("item");
            item.children.iter().map(|k| k.rect.height).sum::<f32>()
        })
        .collect();
    assert!((parts[0] - 100.0).abs() < 0.01 && (parts[1] - 50.0).abs() < 0.01, "{parts:?}");
}

#[test]
fn minmax_zero_fr_tracks_share_the_free_space_like_plain_fr() {
    // BUG-1217: Tailwind's `grid-cols-12` is `repeat(12, minmax(0, 1fr))`; the tracks collapsed to 0
    // and `span 7` was 6 gaps wide. 12 tracks in 1612px with 24px gaps → 7 spans ≈ 930px.
    let root = lay(
        "<div id=\"g\"><div id=\"p\"></div></div>",
        "body{margin:0} #g{display:grid;grid-template-columns:repeat(12,minmax(0,1fr));gap:24px;width:1612px} \
         #p{grid-column:span 7}",
    );
    let g = by_width(&root, 1612.0).expect("grid");
    let w = g.children[0].rect.width;
    assert!((w - 930.33).abs() < 0.5, "span 7 width {w}");
}

#[test]
fn minmax_fixed_min_fr_track_keeps_its_floor() {
    // `minmax(300px, 1fr) 1fr` in 400px: the first track's share (200) is under its floor, so it
    // freezes at 300 and the second takes the remaining 100.
    let root = lay(
        "<div id=\"g\"><div id=\"a\"></div><div id=\"b\"></div></div>",
        "body{margin:0} #g{display:grid;grid-template-columns:minmax(300px,1fr) 1fr;width:400px}",
    );
    let g = by_width(&root, 400.0).expect("grid");
    assert!((g.children[0].rect.width - 300.0).abs() < 0.5, "{}", g.children[0].rect.width);
    assert!((g.children[1].rect.width - 100.0).abs() < 0.5, "{}", g.children[1].rect.width);
}

fn by_width(b: &LayoutBox, w: f32) -> Option<&LayoutBox> {
    if (b.rect.width - w).abs() < 0.01 && !b.children.is_empty() {
        return Some(b);
    }
    b.children.iter().find_map(|c| by_width(c, w))
}
