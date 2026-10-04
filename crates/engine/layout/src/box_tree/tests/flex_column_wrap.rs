use lumen_core::geom::Size;

// ── `flex-direction: column` + `flex-wrap: wrap` (CSS Flexbox L1 §9.3) ──────
//
// Lines of a column container are cut against its height and stacked along the
// x axis. Before this slice the container laid out as a single overflowing
// column (`flex_wrap` was honoured for rows only).

fn lay(html: &str, css: &str) -> (lumen_dom::Document, crate::box_tree::LayoutBox) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    (doc, root)
}

fn rect(root: &crate::box_tree::LayoutBox, doc: &lumen_dom::Document, id: &str) -> lumen_core::geom::Rect {
    super::find_by_id_all(root, doc, id).unwrap_or_else(|| panic!("no #{id}")).rect
}

const SIX: &str = r#"<div id="f"><div id="a" class="i"></div><div id="b" class="i"></div><div id="c" class="i"></div><div id="d" class="i"></div><div id="e" class="i"></div><div id="g" class="i"></div></div>"#;

#[test]
fn column_wrap_breaks_into_lines_along_x() {
    // 170px tall: three 50px items per line (150px) → two lines.
    let (doc, root) = lay(
        SIX,
        "body{margin:0} #f{display:flex;flex-direction:column;flex-wrap:wrap;align-content:flex-start;width:120px;height:170px} .i{width:50px;height:50px}",
    );
    assert_eq!(rect(&root, &doc, "a").x, 0.0);
    assert_eq!(rect(&root, &doc, "c").y, 100.0);
    assert_eq!(rect(&root, &doc, "d").x, 50.0, "line 2 starts right after the 50px line 1");
    assert_eq!(rect(&root, &doc, "d").y, 0.0);
    assert_eq!(rect(&root, &doc, "g").y, 100.0);
}

#[test]
fn column_wrap_gaps_separate_items_and_lines() {
    // row-gap (main axis) 10px, column-gap (between lines) 20px.
    let (doc, root) = lay(
        SIX,
        "body{margin:0} #f{display:flex;flex-direction:column;flex-wrap:wrap;width:120px;height:170px;row-gap:10px;column-gap:20px} .i{width:50px;height:50px}",
    );
    // 50+10+50+10+50 = 170 → three per line.
    assert_eq!(rect(&root, &doc, "b").y, 60.0);
    assert_eq!(rect(&root, &doc, "c").y, 120.0);
    assert_eq!(rect(&root, &doc, "d").x, 70.0, "50px line + 20px column-gap");
}

#[test]
fn column_wrap_line_width_is_widest_item() {
    // Line 1 holds a 30px and a 50px item → its cross size is 50px.
    let (doc, root) = lay(
        r#"<div id="f"><div id="a" class="i" style="width:30px"></div><div id="b" class="i" style="width:50px"></div><div id="c" class="i" style="width:20px"></div></div>"#,
        "body{margin:0} #f{display:flex;flex-direction:column;flex-wrap:wrap;align-content:flex-start;width:200px;height:100px} .i{height:50px}",
    );
    assert_eq!(rect(&root, &doc, "c").x, 50.0);
}

#[test]
fn column_wrap_align_items_centres_inside_the_line() {
    let (doc, root) = lay(
        r#"<div id="f"><div id="a" class="i" style="width:30px"></div><div id="b" class="i" style="width:50px"></div><div id="c" class="i" style="width:20px"></div></div>"#,
        "body{margin:0} #f{display:flex;flex-direction:column;flex-wrap:wrap;align-content:flex-start;align-items:center;width:200px;height:100px} .i{height:50px}",
    );
    // Line 1 is 50px wide: the 30px item is centred at x = 10.
    assert_eq!(rect(&root, &doc, "a").x, 10.0);
    assert_eq!(rect(&root, &doc, "b").x, 0.0);
    assert_eq!(rect(&root, &doc, "c").x, 50.0, "alone on line 2, which is 20px wide");
}

#[test]
fn column_nowrap_and_auto_height_stay_single_line() {
    let (doc, root) = lay(
        SIX,
        "body{margin:0} #f{display:flex;flex-direction:column;width:120px;height:170px} .i{width:50px;height:50px;flex-shrink:0}",
    );
    assert_eq!(rect(&root, &doc, "d").x, 0.0, "nowrap: one overflowing column");
    let (doc, root) = lay(
        SIX,
        "body{margin:0} #f{display:flex;flex-direction:column;flex-wrap:wrap;width:120px} .i{width:50px;height:50px}",
    );
    assert_eq!(rect(&root, &doc, "g").x, 0.0, "auto height: no limit to wrap against");
    assert_eq!(rect(&root, &doc, "g").y, 250.0);
}

#[test]
fn row_wrap_align_content_stretch_grows_auto_height_items() {
    // Three empty auto-height items, one per line, in a 200px box: `stretch`
    // hands every line 200/3px and the items fill their line.
    let (doc, root) = lay(
        r#"<div id="f"><div id="a" class="i"></div><div id="b" class="i"></div><div id="c" class="i"></div></div>"#,
        "body{margin:0} #f{display:flex;flex-wrap:wrap;width:100px;height:200px} .i{width:60px}",
    );
    let third = 200.0_f32 / 3.0;
    assert!((rect(&root, &doc, "a").height - third).abs() < 0.01, "a.h {}", rect(&root, &doc, "a").height);
    assert!((rect(&root, &doc, "c").y - 2.0 * third).abs() < 0.01, "c.y {}", rect(&root, &doc, "c").y);
}

// ── `align-content` of a wrapped column (CSS Flexbox L1 §8.3) ───────────────

const FOUR: &str = r#"<div id="f"><div id="a" class="i"></div><div id="b" class="i"></div><div id="c" class="i"></div><div id="d" class="i"></div></div>"#;

fn ac(value: &str) -> (lumen_dom::Document, crate::box_tree::LayoutBox) {
    // 100px tall → two 50px items per line, two lines of 40px; 200px wide → 120px free.
    lay(
        FOUR,
        &format!(
            "body{{margin:0}} #f{{display:flex;flex-direction:column;flex-wrap:wrap;width:200px;height:100px;align-content:{value}}} .i{{width:40px;height:50px}}"
        ),
    )
}

#[test]
fn column_wrap_align_content_space_between_spreads_lines_along_x() {
    let (doc, root) = ac("space-between");
    assert_eq!(rect(&root, &doc, "a").x, 0.0);
    assert_eq!(rect(&root, &doc, "c").x, 160.0, "last line flush with the right edge");
}

#[test]
fn column_wrap_align_content_end_center_around_evenly() {
    let (doc, root) = ac("flex-end");
    assert_eq!(rect(&root, &doc, "a").x, 120.0);
    let (doc, root) = ac("center");
    assert_eq!(rect(&root, &doc, "a").x, 60.0);
    let (doc, root) = ac("space-around");
    assert_eq!(rect(&root, &doc, "a").x, 30.0);
    assert_eq!(rect(&root, &doc, "c").x, 130.0);
    let (doc, root) = ac("space-evenly");
    assert_eq!(rect(&root, &doc, "a").x, 40.0);
    assert_eq!(rect(&root, &doc, "c").x, 120.0);
}

#[test]
fn column_wrap_align_content_stretch_widens_lines_and_realigns_items() {
    // Default (`normal` = stretch): each line grows 120/2 = 60px → 100px wide.
    let (doc, root) = ac("normal");
    assert_eq!(rect(&root, &doc, "a").x, 0.0);
    assert_eq!(rect(&root, &doc, "c").x, 100.0);
    assert_eq!(rect(&root, &doc, "a").width, 40.0, "explicit-width items keep their width");
    // `align-items: center` centres inside the widened line.
    let (doc, root) = lay(
        FOUR,
        "body{margin:0} #f{display:flex;flex-direction:column;flex-wrap:wrap;align-items:center;width:200px;height:100px} .i{width:40px;height:50px}",
    );
    assert_eq!(rect(&root, &doc, "a").x, 30.0);
    assert_eq!(rect(&root, &doc, "c").x, 130.0);
}
