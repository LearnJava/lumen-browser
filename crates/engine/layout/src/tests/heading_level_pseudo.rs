use super::*;

// ── `:heading` / `:heading(n)` (WHATWG «heading level» draft, BUG-1023) ──
//
// Fixtures mirror the shapes exercised by
// `tests/wpt/html/semantics/sections/headingoffset-and-headingreset.html`
// (minus the shadow-DOM/`<slot>` cases, which need shadow-tree support this
// crate doesn't have yet — see that file's own §Не проверялось).

/// Color of the first element in DOM order whose `id` attribute equals
/// `id`, per the laid-out `root`/`doc` pair from `lay_with_doc`.
fn color_of_id(doc: &lumen_dom::Document, root: &LayoutBox, id: &str) -> Color {
    fn walk<'a>(b: &'a LayoutBox, out: &mut Vec<&'a LayoutBox>) {
        out.push(b);
        for c in &b.children {
            walk(c, out);
        }
    }
    let mut boxes = Vec::new();
    walk(root, &mut boxes);
    for b in boxes {
        if doc.get(b.node).get_attr("id") == Some(id) {
            return b.style.color;
        }
    }
    panic!("element #{id} not found in layout tree");
}

/// red (255,0,0) if the element matched the rule, black (default) otherwise.
const RED: (u8, u8, u8) = (255, 0, 0);
const BLACK: (u8, u8, u8) = (0, 0, 0);

fn rgb(c: Color) -> (u8, u8, u8) {
    (c.r, c.g, c.b)
}

#[test]
fn heading_bare_matches_h1_through_h6_not_div() {
    let (root, doc) = lay_with_doc(
        r#"<h1 id="a"></h1><h6 id="b"></h6><div id="c"></div>"#,
        ":heading { color: red; }",
    );
    assert_eq!(rgb(color_of_id(&doc, &root, "a")), RED);
    assert_eq!(rgb(color_of_id(&doc, &root, "b")), RED);
    assert_eq!(rgb(color_of_id(&doc, &root, "c")), BLACK);
}

#[test]
fn heading_level_matches_base_level_with_no_offset() {
    let (root, doc) = lay_with_doc(
        r#"<h1 id="a"></h1><h3 id="b"></h3>"#,
        ":heading(1) { color: red; }",
    );
    assert_eq!(rgb(color_of_id(&doc, &root, "a")), RED);
    assert_eq!(rgb(color_of_id(&doc, &root, "b")), BLACK);
}

#[test]
fn heading_offset_on_ancestor_shifts_level() {
    // WPT fixture: `<div headingoffset="1"><h1></h1></div>` → h1 is level 2.
    let (root, doc) = lay_with_doc(
        r#"<div headingoffset="1"><h1 id="a"></h1></div>"#,
        ":heading(2) { color: red; }",
    );
    assert_eq!(rgb(color_of_id(&doc, &root, "a")), RED);
}

#[test]
fn heading_offset_accumulates_across_nested_ancestors() {
    // WPT: headingoffset=1 then headingoffset=2 nested → h1 is level 4.
    let (root, doc) = lay_with_doc(
        r#"<div headingoffset="1"><div headingoffset="2"><h1 id="a"></h1></div></div>"#,
        ":heading(4) { color: red; }",
    );
    assert_eq!(rgb(color_of_id(&doc, &root, "a")), RED);
}

#[test]
fn heading_reset_zeroes_accumulated_offset() {
    // WPT: headingoffset=1 > headingoffset=2 > headingreset > h1 → level 1.
    let (root, doc) = lay_with_doc(
        r#"<div headingoffset="1"><div headingoffset="2"><div headingreset><h1 id="a"></h1></div></div></div>"#,
        ":heading(1) { color: red; }",
    );
    assert_eq!(rgb(color_of_id(&doc, &root, "a")), RED);
}

#[test]
fn heading_reset_applies_after_its_own_offset() {
    // WPT «Resetting applies after headingOffset»: a container carrying
    // BOTH `headingoffset` and `headingreset` zeroes the inherited sum, then
    // still adds its own offset on top — h1 ends up level 5, not level 1.
    let (root, doc) = lay_with_doc(
        r#"<div headingreset>
             <div headingoffset="2" headingreset>
               <div headingoffset="2">
                 <h1 id="a"></h1>
               </div>
             </div>
           </div>"#,
        ":heading(5) { color: red; }",
    );
    assert_eq!(rgb(color_of_id(&doc, &root, "a")), RED);
}

#[test]
fn heading_level_clamps_to_nine() {
    // WPT: headingoffset=8 on an <h2> ancestor container → 2 + 8 = 10, clamped to 9.
    let (root, doc) = lay_with_doc(
        r#"<div headingoffset="8"><h2 id="a"></h2></div>"#,
        ":heading(9) { color: red; }",
    );
    assert_eq!(rgb(color_of_id(&doc, &root, "a")), RED);
}

#[test]
fn heading_negative_offset_is_clamped_to_zero_not_subtracted() {
    // WPT «Negative headingoffsets are clamped to `0`»: headingoffset=-3 on
    // an <h3> ancestor container leaves the level unchanged at 3, it does not
    // go to 0.
    let (root, doc) = lay_with_doc(
        r#"<div headingoffset="-3"><h3 id="a"></h3></div>"#,
        ":heading(3) { color: red; }",
    );
    assert_eq!(rgb(color_of_id(&doc, &root, "a")), RED);
}

#[test]
fn heading_own_offset_and_reset_attributes_apply_too() {
    // The heading element itself (not just an ancestor container) can carry
    // `headingoffset`/`headingreset` — WPT tests this directly on `<h1>`.
    let (root, doc) = lay_with_doc(
        r#"<div headingoffset="2"><h1 id="a" headingreset></h1></div>"#,
        ":heading(1) { color: red; }",
    );
    assert_eq!(rgb(color_of_id(&doc, &root, "a")), RED);
}

#[test]
fn heading_matches_only_its_own_level_not_neighbors() {
    let (root, doc) = lay_with_doc(
        r#"<div headingoffset="3"><h1 id="a"></h1></div>"#,
        ":heading(1) { color: red; }",
    );
    // Level is 1 + 3 = 4, not 1 — the bare-base rule must NOT match.
    assert_eq!(rgb(color_of_id(&doc, &root, "a")), BLACK);
}
