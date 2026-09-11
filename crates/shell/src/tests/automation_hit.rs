//! BUG-1044: the automation channel must not answer `success` for a click that
//! lands somewhere other than the element the caller named.
//!
//! The predicate under test is [`hit_belongs_to_target`] — the whole of the
//! decision `Lumen::automation_hit_mismatch` makes; everything around it is
//! coordinate conversion already covered by BUG-437/CC-14. Every fixture here
//! runs a real `layout` + `hit_test`, not a hand-built `HitTestResult`: the
//! question is what the *engine's* hit test returns at the centre of a resolved
//! box, and a synthetic result would answer a question nobody asks.

use super::*;

use crate::lumen::hit_belongs_to_target;
use lumen_core::geom::Point;
use lumen_paint::hit_test;

/// Layout `html` with `css` at a fixed viewport and hand back both trees.
fn layout_fixture(html: &str, css: &str) -> (lumen_dom::Document, lumen_layout::LayoutBox) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let layout = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
    (doc, layout)
}

/// The point `resolve_automation_target` would produce for `#id`, in page
/// space: the centre of the element's own box.
fn target_centre(
    doc: &lumen_dom::Document,
    layout: &lumen_layout::LayoutBox,
    id: &str,
) -> (lumen_dom::NodeId, Point) {
    let found = *lumen_layout::selector_query::find_all_by_selector(layout, doc, &format!("#{id}"))
        .first()
        .expect("fixture element must have a box");
    let rect = found.rect;
    (
        found.node,
        Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
    )
}

#[test]
fn click_on_a_submit_button_inside_a_form_belongs_to_the_button() {
    // The original BUG-1044 repro, verbatim. It is the *green* direction now
    // (BUG-926 gave an inline `<button>` its own width), so this fixture's job
    // is to prove the new check does not reject the very click E2E-4 needs.
    let (doc, layout) = layout_fixture(
        concat!(
            "<form id=\"fp\" method=\"post\" action=\"/login\">",
            "<input type=\"text\" name=\"user\" value=\"admin\">",
            "<button id=\"gopost\" type=\"submit\">Vojti POST</button>",
            "</form>",
        ),
        "",
    );
    let (button, point) = target_centre(&doc, &layout, "gopost");
    let hit = hit_test(point, &layout).expect("the button's centre must hit something");
    assert!(
        hit_belongs_to_target(&hit, button),
        "click at the button's own centre must count as reaching the button, \
         hit node {} path {:?}",
        hit.node.index(),
        hit.path.iter().map(|n| n.index()).collect::<Vec<_>>(),
    );
}

#[test]
fn click_on_text_inside_the_target_belongs_to_the_target() {
    // A descendant hit is a hit: the target's centre usually lands on its own
    // text/child box, and `HitTestResult::node` is then that child (or the
    // inline run's owner). Rejecting those would fail nearly every real click.
    let (doc, layout) = layout_fixture(
        "<div id=\"outer\" style=\"width:200px\"><span id=\"inner\">text</span></div>",
        "",
    );
    let (outer, point) = target_centre(&doc, &layout, "outer");
    let hit = hit_test(point, &layout).expect("the div's centre must hit something");
    assert!(
        hit_belongs_to_target(&hit, outer),
        "a hit on the inner span is a hit on the outer div's subtree",
    );
}

#[test]
fn click_covered_by_an_overlay_does_not_belong_to_the_target() {
    // The shape BUG-1044 keeps open: the target resolves to a point, the point
    // hit-tests to a foreign element, and the caller used to be told `success`.
    // An absolutely-positioned overlay is the honest form of it — the target
    // has a perfectly good box, it is just not what a click there reaches.
    let (doc, layout) = layout_fixture(
        concat!(
            "<div id=\"under\" style=\"width:200px;height:100px\">under</div>",
            "<div id=\"over\" style=\"position:absolute;left:0;top:0;",
            "width:400px;height:300px\">over</div>",
        ),
        "",
    );
    let (under, point) = target_centre(&doc, &layout, "under");
    let over = doc.find_by_id("over").expect("fixture has #over");
    let hit = hit_test(point, &layout).expect("the overlay covers the point");
    assert!(
        hit_belongs_to_target(&hit, over),
        "sanity: the overlay is what is actually under the point",
    );
    assert!(
        !hit_belongs_to_target(&hit, under),
        "a click swallowed by the overlay must not count as reaching #under",
    );
}

#[test]
fn click_on_an_ancestor_does_not_belong_to_the_target() {
    // The direction the predicate must not get backwards: `#wrap` is on the
    // hit's path when the hit is `#kid`, but a hit on `#wrap` itself is NOT a
    // hit on `#kid`. Getting this symmetric would restore exactly the
    // "click landed on the parent `<form>`, reported success" behaviour.
    let (doc, layout) = layout_fixture(
        "<div id=\"wrap\" style=\"width:300px;height:200px\"><span id=\"kid\">k</span></div>",
        "",
    );
    let kid = doc.find_by_id("kid").expect("fixture has #kid");
    let (wrap, _) = target_centre(&doc, &layout, "wrap");
    // Bottom-right corner of the wrapper: inside the wrapper, far from the kid.
    let wrap_box = lumen_layout::find_box_by_node(&layout, wrap).expect("wrapper has a box");
    let point = Point::new(
        wrap_box.rect.x + wrap_box.rect.width - 2.0,
        wrap_box.rect.y + wrap_box.rect.height - 2.0,
    );
    let hit = hit_test(point, &layout).expect("the wrapper covers the point");
    assert!(hit_belongs_to_target(&hit, wrap), "sanity: the wrapper is hit");
    assert!(
        !hit_belongs_to_target(&hit, kid),
        "an ancestor hit is not a hit on the descendant the caller named",
    );
}
