//! ANCHOR-SCROLL: CSS Anchor Positioning L1 §4.3 — a box positioned against an anchor that sits
//! in a scroll container follows that container's scroll offset.

use lumen_core::geom::{Rect, Size};

use super::super::{layout, LayoutBox};
use crate::set_scroll_position;

fn build(html: &str, css: &str) -> (LayoutBox, lumen_dom::Document) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
    (root, doc)
}

fn find<'a>(root: &'a LayoutBox, doc: &lumen_dom::Document, id: &str) -> &'a LayoutBox {
    super::find_by_id_all(root, doc, id).unwrap_or_else(|| panic!("нет #{id}"))
}

/// Offsets of the scroll containers `id` follows, as `(container id attr, x, y)` order of the chain.
fn followed(root: &LayoutBox, doc: &lumen_dom::Document, id: &str) -> Vec<(lumen_dom::NodeId, f32, f32)> {
    find(root, doc, id)
        .anchor_scroll
        .as_ref()
        .map(|a| a.scrollers.iter().map(|s| (s.node, s.x, s.y)).collect())
        .unwrap_or_default()
}

const PAGE: &str = r#"<div id="s"><div id="a"></div><div id="spacer"></div></div><div id="t"></div>"#;
const SHEET: &str = "#s{width:100px;height:100px;overflow-y:scroll}\
    #a{anchor-name:--a;width:50px;height:50px;margin-top:50px}\
    #spacer{height:400px}\
    #t{position:absolute;width:20px;height:20px;position-anchor:--a;left:anchor(right);top:anchor(top)}";

#[test]
fn target_outside_the_scroller_follows_it() {
    let (mut root, doc) = build(PAGE, SHEET);
    let scroller = find(&root, &doc, "s").node;
    let before = find(&root, &doc, "t").rect;
    assert_eq!((before.x, before.y), (50.0, 50.0), "unscrolled position {before:?}");
    assert_eq!(followed(&root, &doc, "t"), vec![(scroller, 0.0, 0.0)]);

    assert!(set_scroll_position(&mut root, scroller, 0.0, 30.0));
    assert_eq!(followed(&root, &doc, "t"), vec![(scroller, 0.0, 30.0)]);
    let after = find(&root, &doc, "t").rect;
    assert_eq!(after, before, "the layout position is the unscrolled one; paint applies the offset");
}

#[test]
fn target_inside_the_scroller_is_scrolled_by_it_already() {
    let (root, doc) = build(
        r#"<div id="s"><div id="a"></div><div id="t"></div><div id="spacer"></div></div>"#,
        "#s{position:relative;width:100px;height:100px;overflow-y:scroll}\
         #a{anchor-name:--a;width:50px;height:50px}#spacer{height:400px}\
         #t{position:absolute;width:20px;height:20px;position-anchor:--a;left:anchor(right);top:anchor(top)}",
    );
    assert!(find(&root, &doc, "t").anchor_scroll.is_none());
}

#[test]
fn anchor_outside_any_scroller_leaves_the_target_alone() {
    let (root, doc) = build(
        r#"<div id="a"></div><div id="t"></div>"#,
        "#a{anchor-name:--a;width:50px;height:50px}\
         #t{position:absolute;width:20px;height:20px;position-anchor:--a;left:anchor(right);top:anchor(top)}",
    );
    assert!(find(&root, &doc, "t").anchor_scroll.is_none());
}

#[test]
fn nested_scrollers_both_apply_outermost_first() {
    let (mut root, doc) = build(
        r#"<div id="o"><div id="i"><div id="a"></div><div id="spacer"></div></div><div id="spacer2"></div></div><div id="t"></div>"#,
        "#o{width:200px;height:150px;overflow:auto}#i{width:100px;height:100px;overflow:auto}\
         #a{anchor-name:--a;width:50px;height:50px}#spacer{height:400px}#spacer2{height:600px}\
         #t{position:absolute;width:20px;height:20px;position-anchor:--a;left:anchor(right);top:anchor(top)}",
    );
    let (o, i) = (find(&root, &doc, "o").node, find(&root, &doc, "i").node);
    assert!(set_scroll_position(&mut root, o, 0.0, 20.0));
    assert!(set_scroll_position(&mut root, i, 0.0, 70.0));
    assert_eq!(followed(&root, &doc, "t"), vec![(o, 0.0, 20.0), (i, 0.0, 70.0)]);
}

#[test]
fn only_the_scrollers_beyond_the_targets_own_ancestors_count() {
    // `o` scrolls both the anchor and the target; only `i` (the anchor's own scroller) is extra.
    let (mut root, doc) = build(
        r#"<div id="o"><div id="i"><div id="a"></div><div id="spacer"></div></div><div id="t"></div><div id="spacer2"></div></div>"#,
        "#o{position:relative;width:200px;height:150px;overflow:auto}#i{width:100px;height:100px;overflow:auto}\
         #a{anchor-name:--a;width:50px;height:50px}#spacer{height:400px}#spacer2{height:600px}\
         #t{position:absolute;width:20px;height:20px;position-anchor:--a;left:anchor(right);top:anchor(top)}",
    );
    let (o, i) = (find(&root, &doc, "o").node, find(&root, &doc, "i").node);
    assert!(set_scroll_position(&mut root, o, 0.0, 20.0));
    assert!(set_scroll_position(&mut root, i, 0.0, 70.0));
    assert_eq!(followed(&root, &doc, "t"), vec![(i, 0.0, 70.0)]);
}

#[test]
fn a_boxs_own_anchor_name_does_not_make_it_follow_its_own_scroll() {
    // The anchor *is* the scroller: its content scrolls, the anchor box itself does not move.
    let (mut root, doc) = build(
        r#"<div id="a"><div id="spacer"></div></div><div id="t"></div>"#,
        "#a{anchor-name:--a;width:100px;height:100px;overflow:auto}#spacer{height:400px}\
         #t{position:absolute;width:20px;height:20px;position-anchor:--a;left:anchor(right);top:anchor(top)}",
    );
    let a = find(&root, &doc, "a").node;
    assert!(set_scroll_position(&mut root, a, 0.0, 30.0));
    assert!(find(&root, &doc, "t").anchor_scroll.is_none());
}

#[test]
fn a_box_anchored_to_an_anchored_box_follows_the_same_scrollers() {
    // anchor-scroll-chained-001: `t2` is positioned against `t1`, which is positioned against `a`.
    let (mut root, doc) = build(
        r#"<div id="s"><div id="spacer"></div><div id="a"></div><div id="spacer2"></div></div><div id="t1"></div><div id="t2"></div>"#,
        "#s{width:100px;height:100px;overflow:scroll}#spacer{height:230px}#spacer2{height:230px}\
         #a{anchor-name:--a1;width:100px;height:20px}\
         #t1{position:absolute;width:100px;height:100px;position-anchor:--a1;left:anchor(left);top:anchor(bottom);anchor-name:--a2}\
         #t2{position:absolute;width:100px;height:100px;position-anchor:--a2;left:anchor(left);top:anchor(bottom)}",
    );
    let s = find(&root, &doc, "s").node;
    assert!(set_scroll_position(&mut root, s, 0.0, 200.0));
    assert_eq!(followed(&root, &doc, "t1"), vec![(s, 0.0, 200.0)]);
    assert_eq!(followed(&root, &doc, "t2"), vec![(s, 0.0, 200.0)]);
}

#[test]
fn anchored_to_each_other_does_not_loop() {
    let (root, doc) = build(
        r#"<div id="t1"></div><div id="t2"></div>"#,
        "#t1{position:absolute;width:10px;height:10px;position-anchor:--b;left:anchor(left);top:anchor(top);anchor-name:--a}\
         #t2{position:absolute;width:10px;height:10px;position-anchor:--a;left:anchor(left);top:anchor(top);anchor-name:--b}",
    );
    assert!(followed(&root, &doc, "t1").is_empty());
}

// ─── ANCHOR-SCROLL-3-S2: position-area in a scrollable containing block ──────

/// `position-area-scrolling-001.tentative.html` without the scripts: scroller 500×500,
/// overflowing content 1000×1000, anchor at (200,200,100,100), `.pos` abs target.
/// Expected rects are relative to the scroller's padding box (it sits at the origin).
#[test]
fn position_area_grid_stretches_to_the_scrollable_overflow() {
    // WPT e1/e3/e9 (border-box percentages): the grid covers the scrollable
    // overflow (1000×1000), not the padding box (500×500); left/top band is 200
    // wide, right/bottom 700, so 50% → 100 and 350, edges hugged toward the anchor.
    let (root, doc) = build(
        r#"<div id="s"><div style="width:1000px;height:1000px"><div style="height:200px"></div><div id="a"></div><div id="e1" class="pos" style="position-area:top left"></div><div id="e3" class="pos" style="position-area:top right"></div><div id="e9" class="pos" style="position-area:bottom right"></div></div></div>"#,
        "#s{position:relative;overflow:hidden;width:500px;height:500px}\
         #a{anchor-name:--a;margin-left:200px;width:100px;height:100px}\
         .pos{position:absolute;box-sizing:border-box;position-anchor:--a;width:50%;height:50%}",
    );
    assert_eq!(find(&root, &doc, "e1").rect, Rect::new(100.0, 100.0, 100.0, 100.0));
    assert_eq!(find(&root, &doc, "e3").rect, Rect::new(300.0, 100.0, 350.0, 100.0));
    assert_eq!(find(&root, &doc, "e9").rect, Rect::new(300.0, 300.0, 350.0, 350.0));
}

#[test]
fn content_box_percentage_adds_padding_and_border_to_the_band_share() {
    let (root, doc) = build(
        r#"<div id="s"><div style="width:1000px;height:1000px"><div style="height:200px"></div><div id="a"></div><div id="e" class="pos" style="position-area:top left"></div></div></div>"#,
        "#s{position:relative;overflow:hidden;width:500px;height:500px}\
         #a{anchor-name:--a;margin-left:200px;width:100px;height:100px}\
         .pos{position:absolute;box-sizing:content-box;position-anchor:--a;width:50%;height:50%;padding:10px;border:5px solid}",
    );
    // 50% of the 200-wide top-left band → 100 content + 30 padding+border = 130
    // border-box, end-aligned toward the anchor.
    assert_eq!(find(&root, &doc, "e").rect, Rect::new(70.0, 70.0, 130.0, 130.0));
}

#[test]
fn auto_axes_fill_their_band_in_a_scroll_container() {
    let (root, doc) = build(
        r#"<div id="s"><div style="width:1000px;height:1000px"><div style="height:200px"></div><div id="a"></div><div id="e" class="pos" style="position-area:bottom right"></div></div></div>"#,
        "#s{position:relative;overflow:hidden;width:500px;height:500px}\
         #a{anchor-name:--a;margin-left:200px;width:100px;height:100px}\
         .pos{position:absolute;position-anchor:--a}",
    );
    // width/height auto stretch to the whole bottom-right band of the 1000×1000 grid.
    assert_eq!(find(&root, &doc, "e").rect, Rect::new(300.0, 300.0, 700.0, 700.0));
}

#[test]
fn an_axis_whose_insets_name_another_anchor_does_not_follow() {
    // anchor-scroll-006: `left` uses the default anchor (inside the scroller), `top` an anchor
    // outside any scroller — only the horizontal scrolling moves the target.
    let (mut root, doc) = build(
        r#"<div id="s"><div id="spacer"></div><div id="a"></div></div><div id="o"></div><div id="t"></div>"#,
        "#s{width:100px;height:100px;overflow:scroll}#spacer{width:400px;height:200px}\
         #a{anchor-name:--a;width:50px;height:50px}#o{anchor-name:--o;width:10px;height:10px}\
         #t{position:fixed;width:20px;height:20px;position-anchor:--a;left:anchor(left);top:anchor(--o bottom)}",
    );
    let scroller = find(&root, &doc, "s").node;
    assert!(set_scroll_position(&mut root, scroller, 40.0, 60.0));
    assert_eq!(followed(&root, &doc, "t"), vec![(scroller, 40.0, 0.0)]);
}

#[test]
fn position_area_follows_both_axes() {
    let (mut root, doc) = build(
        r#"<div id="s"><div id="spacer"></div><div id="a"></div></div><div id="t"></div>"#,
        "#s{width:100px;height:100px;overflow:scroll}#spacer{width:400px;height:200px}\
         #a{anchor-name:--a;width:50px;height:50px}\
         #t{position:absolute;width:20px;height:20px;position-anchor:--a;position-area:bottom right}",
    );
    let scroller = find(&root, &doc, "s").node;
    assert!(set_scroll_position(&mut root, scroller, 40.0, 60.0));
    assert_eq!(followed(&root, &doc, "t"), vec![(scroller, 40.0, 60.0)]);
}

#[test]
fn the_followed_containers_are_listed_per_target() {
    let (root, doc) = build(PAGE, SHEET);
    let map = crate::collect_anchor_scrollers(&root);
    let t = find(&root, &doc, "t").node;
    assert_eq!(map.get(&(t.index() as u32)), Some(&vec![(find(&root, &doc, "s").node.raw(), true, true)]));
    assert_eq!(map.len(), 1, "{map:?}");
}

// ─── ANCHOR-SCROLL-3-S3: position:fixed follows the page scroll ──────────────

/// `anchor_scroll` of `#t`: `(page, scroller count)`.
fn page_of(root: &LayoutBox, doc: &lumen_dom::Document, id: &str) -> (bool, usize) {
    find(root, doc, id)
        .anchor_scroll
        .as_ref()
        .map_or((false, 0), |a| (a.page, a.scrollers.len()))
}

/// anchor-scroll-fixedpos.html: the anchor is in the document flow, so the fixed
/// target paints as page content — it scrolls with the page (empty scroller list:
/// the page scroll is the renderer's own, no container is listed).
#[test]
fn fixed_target_of_a_flow_anchor_is_page_content() {
    let (root, doc) = build(
        r#"<div id="a"></div><div id="t"></div>"#,
        "#a{anchor-name:--a;margin:300px 0 0 300px;width:100px;height:100px}\
         #t{position:fixed;width:100px;height:100px;position-anchor:--a;left:anchor(right);top:anchor(top)}",
    );
    // Layout is the at-scroll-0 document position; the paint-side `page` flag makes
    // the renderer scroll it (anchor-scroll-fixedpos-ref.html expects (400,100) at scroll 200).
    assert_eq!(find(&root, &doc, "t").rect, Rect::new(400.0, 300.0, 100.0, 100.0));
    assert_eq!(page_of(&root, &doc, "t"), (true, 0));
}

/// anchor-scroll-fixedpos-002.html: the anchor itself is viewport-pinned, the target
/// stays a viewport overlay — no `anchor_scroll` at all.
#[test]
fn fixed_target_of_a_fixed_anchor_stays_a_viewport_overlay() {
    let (root, doc) = build(
        r#"<div id="a"></div><div id="t"></div>"#,
        "#a{position:fixed;anchor-name:--a;margin:300px 0 0 300px;width:100px;height:100px}\
         #t{position:fixed;width:100px;height:100px;position-anchor:--a;left:anchor(right);top:anchor(top)}",
    );
    assert!(find(&root, &doc, "t").anchor_scroll.is_none());
}

/// A fixed box under a transformed ancestor has that ancestor as its containing
/// block — it scrolls with the ancestor already (LAYOUT-FIXED-CB-2), the page flag
/// would double-compensate.
#[test]
fn fixed_target_in_a_transformed_subtree_is_not_page_content() {
    let (root, doc) = build(
        r#"<div id="w"><div id="a"></div><div id="t"></div></div>"#,
        "#w{transform:scale(1)}\
         #a{anchor-name:--a;width:50px;height:50px}\
         #t{position:fixed;width:20px;height:20px;position-anchor:--a;left:anchor(right);top:anchor(top)}",
    );
    assert!(find(&root, &doc, "t").anchor_scroll.is_none());
}

/// A fixed target chained behind a page-content target is page content too
/// (`anchor-scroll-fixedpos-nested-001.html`: `f2`/`f3` inside `f1`).
#[test]
fn the_page_flag_chains_through_intermediate_targets() {
    let (root, doc) = build(
        r#"<div id="a"></div><div id="f1"><div id="f2"></div></div>"#,
        "#a{anchor-name:--a;width:50px;height:50px}\
         #f1{position:fixed;width:50px;height:50px;position-anchor:--a;left:anchor(right);top:anchor(top);anchor-name:--b}\
         #f2{position:fixed;width:20px;height:20px;position-anchor:--b;left:anchor(right);top:anchor(top)}",
    );
    assert_eq!(page_of(&root, &doc, "f1"), (true, 0));
    assert_eq!(page_of(&root, &doc, "f2"), (true, 0));
}
