use lumen_core::geom::Size;

// ── CSS Shapes L1 — shape-outside circle() ────────────────────────────────

#[test]
fn parse_circle_px_valid() {
    assert_eq!(super::super::parse_circle_px("circle(50px)"), Some(50.0));
    assert_eq!(super::super::parse_circle_px("circle(0px)"), None);
    assert_eq!(super::super::parse_circle_px("circle(10)"), Some(10.0));
    assert_eq!(super::super::parse_circle_px("CIRCLE(30PX)"), Some(30.0)); // case-insensitive
}

#[test]
fn parse_circle_px_invalid() {
    assert_eq!(super::super::parse_circle_px("none"), None);
    assert_eq!(super::super::parse_circle_px("ellipse(30px 20px)"), None);
    assert_eq!(super::super::parse_circle_px("polygon(0 0, 10 0, 10 10)"), None);
}

#[test]
fn shape_outside_circle_computation() {
    // Circle with radius 50px centered at (100, 50): at y=50 (center),
    // horizontal extent = center_x + radius = 100 + 50 = 150.
    // At y=0 (50px above center): hw = sqrt(50^2 - 50^2) = 0, extent = 100.
    let mut fc = super::super::FloatContext::new();
    fc.shape_circles.push((0.0, 100.0, true, 100.0, 50.0, 50.0));
    assert!((fc.left_edge_at(50.0, 0.0) - 150.0).abs() < 0.01);
    assert!((fc.left_edge_at(0.0, 0.0) - 100.0).abs() < 0.01);
}

// ── CSS Shapes L1 — shape-outside polygon() ───────────────────────────────

#[test]
fn parse_shape_polygon_valid() {
    // Triangle with px values.
    let pts = super::super::parse_shape_polygon_px("polygon(0px 0px, 100px 0px, 50px 100px)");
    assert_eq!(pts, Some(vec![(0.0, 0.0), (100.0, 0.0), (50.0, 100.0)]));
    // Bare numbers (no "px" suffix).
    let pts2 = super::super::parse_shape_polygon_px("polygon(0 0, 10 0, 10 10, 0 10)");
    assert_eq!(pts2, Some(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]));
    // With fill-rule prefix.
    let pts3 = super::super::parse_shape_polygon_px("polygon(nonzero, 0 0, 50 0, 50 50)");
    assert_eq!(pts3, Some(vec![(0.0, 0.0), (50.0, 0.0), (50.0, 50.0)]));
}

#[test]
fn parse_shape_polygon_invalid() {
    // Fewer than 3 points.
    assert_eq!(super::super::parse_shape_polygon_px("polygon(0 0, 10 10)"), None);
    // Not a polygon.
    assert_eq!(super::super::parse_shape_polygon_px("circle(50px)"), None);
    assert_eq!(super::super::parse_shape_polygon_px("none"), None);
}

// ── CSS Shapes L1 — shape-outside path() ──────────────────────────────────

#[test]
fn parse_shape_path_triangle() {
    // Straight-line triangle: M 0 0 L 100 0 L 50 100 Z → 3 distinct vertices.
    let pts = super::super::parse_shape_path_px(r#"path("M 0 0 L 100 0 L 50 100 Z")"#)
        .expect("triangle path should parse");
    assert_eq!(pts[0], (0.0, 0.0));
    assert_eq!(pts[1], (100.0, 0.0));
    assert_eq!(pts[2], (50.0, 100.0));
    // Close returns to the sub-path start.
    assert_eq!(*pts.last().unwrap(), (0.0, 0.0));
}

#[test]
fn parse_shape_path_fill_rule_and_quotes() {
    // Leading fill-rule is accepted and ignored; single quotes work too.
    let pts = super::super::parse_shape_path_px(r#"path(evenodd, 'M 0 0 L 10 0 L 10 10 Z')"#)
        .expect("path with fill-rule + single quotes should parse");
    assert_eq!(pts[0], (0.0, 0.0));
    assert_eq!(pts[1], (10.0, 0.0));
    assert_eq!(pts[2], (10.0, 10.0));
}

#[test]
fn parse_shape_path_invalid() {
    // Not a path() function.
    assert_eq!(super::super::parse_shape_path_px("polygon(0 0, 10 0, 10 10)"), None);
    assert_eq!(super::super::parse_shape_path_px("none"), None);
    // Missing quotes around the d-string.
    assert_eq!(super::super::parse_shape_path_px("path(M 0 0 L 10 0 L 10 10 Z)"), None);
    // Degenerate (< 3 vertices).
    assert_eq!(super::super::parse_shape_path_px(r#"path("M 0 0 L 10 10")"#), None);
}

#[test]
fn float_context_path_left_float() {
    // path() flattened to the same right-triangle as the polygon case:
    // M 0 0 L 100 0 L 0 100 Z. At y=50 the hypotenuse right edge = 50.
    let pts = super::super::parse_shape_path_px(r#"path("M 0 0 L 100 0 L 0 100 Z")"#)
        .expect("triangle path should parse");
    let mut fc = super::super::FloatContext::new();
    fc.shape_polygons.push(super::super::ShapePolygon {
        top_y: 0.0, bottom_y: 100.0, is_left: true, points: pts,
    });
    assert!((fc.left_edge_at(50.0, 0.0) - 50.0).abs() < 0.01);
}

#[test]
fn polygon_edge_at_y_triangle() {
    // Right-triangle: (0,0)→(100,0)→(0,100)→(0,0).
    // At y=50 the right edge is the hypotenuse at x = 100 - 50 = 50.
    let pts = vec![(0.0_f32, 0.0), (100.0, 0.0), (0.0, 100.0)];
    let right = super::super::polygon_right_edge_at_y(&pts, 50.0);
    assert!(right.is_some());
    assert!((right.unwrap() - 50.0).abs() < 0.01, "right edge at y=50 should be 50, got {:?}", right);
    // Left edge at y=50: leftmost intersection = 0.0 (vertical left side).
    let left = super::super::polygon_left_edge_at_y(&pts, 50.0);
    assert!(left.is_some());
    assert!((left.unwrap() - 0.0).abs() < 0.01);
}

#[test]
fn float_context_polygon_left_float() {
    // Triangle left float: (0,0)→(100,0)→(0,100)→(0,0) in content-area coords.
    // At y=50: rightmost edge = 50. Should narrow left boundary to 50.
    let mut fc = super::super::FloatContext::new();
    fc.shape_polygons.push(super::super::ShapePolygon {
        top_y: 0.0, bottom_y: 100.0, is_left: true,
        points: vec![(0.0, 0.0), (100.0, 0.0), (0.0, 100.0)],
    });
    assert!((fc.left_edge_at(50.0, 0.0) - 50.0).abs() < 0.01);
    // Outside float range: falls back to default.
    assert!((fc.left_edge_at(110.0, 0.0) - 0.0).abs() < 0.01);
}

// ── CSS Shapes L1 — shape-outside ellipse() ───────────────────────────────

#[test]
fn parse_shape_ellipse_valid() {
    let r = super::super::parse_shape_ellipse_px("ellipse(50px 80px at 100px 150px)");
    assert_eq!(r, Some((50.0, 80.0, 100.0, 150.0)));
    // Bare numbers.
    let r2 = super::super::parse_shape_ellipse_px("ellipse(30 40 at 60 70)");
    assert_eq!(r2, Some((30.0, 40.0, 60.0, 70.0)));
}

#[test]
fn parse_shape_ellipse_invalid() {
    // No "at" keyword.
    assert_eq!(super::super::parse_shape_ellipse_px("ellipse(50px 80px)"), None);
    // Zero radius.
    assert_eq!(super::super::parse_shape_ellipse_px("ellipse(0px 40px at 50px 50px)"), None);
    // Not an ellipse.
    assert_eq!(super::super::parse_shape_ellipse_px("circle(50px)"), None);
}

#[test]
fn float_context_ellipse_left_float() {
    // Ellipse: rx=50, ry=50, center (100,50). At y=50 (center): right edge = 150.
    // At y=0 (top): norm=(0-50)/50=-1.0, hw=0, right edge=100.
    let mut fc = super::super::FloatContext::new();
    fc.shape_ellipses.push(super::super::ShapeEllipse {
        top_y: 0.0, bottom_y: 100.0, is_left: true,
        cx: 100.0, cy: 50.0, rx: 50.0, ry: 50.0,
    });
    assert!((fc.left_edge_at(50.0, 0.0) - 150.0).abs() < 0.01);
    assert!((fc.left_edge_at(0.0, 0.0) - 100.0).abs() < 0.01);
}

#[test]
fn float_context_ellipse_right_float() {
    // Ellipse: rx=50, ry=50, center (200,50). Right float.
    // At y=50 (center): left edge = 200 - 50 = 150.
    let mut fc = super::super::FloatContext::new();
    fc.shape_ellipses.push(super::super::ShapeEllipse {
        top_y: 0.0, bottom_y: 100.0, is_left: false,
        cx: 200.0, cy: 50.0, rx: 50.0, ry: 50.0,
    });
    assert!((fc.right_edge_at(50.0, 400.0) - 150.0).abs() < 0.01);
}

// ── CSS Shapes L1 — shape-outside inset() ─────────────────────────────────

#[test]
fn parse_shape_inset_valid() {
    // 4-value form, no rounding.
    assert_eq!(
        super::super::parse_shape_inset_px("inset(10px 20px 30px 40px)"),
        Some((10.0, 20.0, 30.0, 40.0, 0.0))
    );
    // 1-value form expands to all sides.
    assert_eq!(
        super::super::parse_shape_inset_px("inset(15px)"),
        Some((15.0, 15.0, 15.0, 15.0, 0.0))
    );
    // 2-value form: vertical horizontal.
    assert_eq!(
        super::super::parse_shape_inset_px("inset(10 20)"),
        Some((10.0, 20.0, 10.0, 20.0, 0.0))
    );
    // 3-value form: top horizontal bottom.
    assert_eq!(
        super::super::parse_shape_inset_px("inset(5 10 15)"),
        Some((5.0, 10.0, 15.0, 10.0, 0.0))
    );
    // With round clause (single radius).
    assert_eq!(
        super::super::parse_shape_inset_px("inset(10px round 8px)"),
        Some((10.0, 10.0, 10.0, 10.0, 8.0))
    );
}

#[test]
fn parse_shape_inset_invalid() {
    // Not an inset.
    assert_eq!(super::super::parse_shape_inset_px("circle(50px)"), None);
    assert_eq!(super::super::parse_shape_inset_px("none"), None);
    // Too many length values.
    assert_eq!(super::super::parse_shape_inset_px("inset(1 2 3 4 5)"), None);
    // `round` keyword without a radius value.
    assert_eq!(super::super::parse_shape_inset_px("inset(10px round )"), None);
}

#[test]
fn float_context_inset_left_float_sharp() {
    // Left float inset rect spanning x∈[10,90], y∈[0,100], no rounding.
    // Content to the right must clear the inset right edge = 90 for all y in range.
    let mut fc = super::super::FloatContext::new();
    fc.shape_insets.push(super::super::ShapeInset {
        top_y: 0.0, bottom_y: 100.0, is_left: true,
        left_x: 10.0, right_x: 90.0, radius: 0.0,
    });
    assert!((fc.left_edge_at(0.0, 0.0) - 90.0).abs() < 0.01);
    assert!((fc.left_edge_at(50.0, 0.0) - 90.0).abs() < 0.01);
    // Outside the vertical range: falls back to default.
    assert!((fc.left_edge_at(150.0, 0.0) - 0.0).abs() < 0.01);
}

#[test]
fn float_context_inset_right_float_sharp() {
    // Right float inset rect x∈[210,290]. Content to the left clears left edge = 210.
    let mut fc = super::super::FloatContext::new();
    fc.shape_insets.push(super::super::ShapeInset {
        top_y: 0.0, bottom_y: 100.0, is_left: false,
        left_x: 210.0, right_x: 290.0, radius: 0.0,
    });
    assert!((fc.right_edge_at(50.0, 400.0) - 210.0).abs() < 0.01);
}

#[test]
fn float_context_inset_rounded_corner() {
    // Left float rect x∈[0,100], y∈[0,100], corner radius 20.
    // At the very top (y=0), the rounded corner recedes the right edge fully
    // by `radius` → edge = 100 - 20 = 80. At mid-height (y=50, flat band) the
    // edge is the full right_x = 100.
    let mut fc = super::super::FloatContext::new();
    fc.shape_insets.push(super::super::ShapeInset {
        top_y: 0.0, bottom_y: 100.0, is_left: true,
        left_x: 0.0, right_x: 100.0, radius: 20.0,
    });
    assert!((fc.left_edge_at(50.0, 0.0) - 100.0).abs() < 0.01);
    assert!((fc.left_edge_at(0.0, 0.0) - 80.0).abs() < 0.01);
    // Quarter-circle midpoint: at y=20-20*cos(45°)... use exact: dy at y where
    // inward = radius - sqrt(r^2 - dy^2). At y = top_band - dy. top_band=20.
    // For y=20 (band edge), inward=0 → edge=100.
    assert!((fc.left_edge_at(20.0, 0.0) - 100.0).abs() < 0.01);
}

#[test]
fn inset_corner_inward_helper() {
    // No radius → no inward offset.
    assert_eq!(super::super::inset_corner_inward(0.0, 0.0, 100.0, 0.0), 0.0);
    // Flat middle band → no offset.
    assert_eq!(super::super::inset_corner_inward(50.0, 0.0, 100.0, 20.0), 0.0);
    // Exactly at the top edge → full radius recession.
    assert!((super::super::inset_corner_inward(0.0, 0.0, 100.0, 20.0) - 20.0).abs() < 0.01);
    // Exactly at the bottom edge → full radius recession.
    assert!((super::super::inset_corner_inward(100.0, 0.0, 100.0, 20.0) - 20.0).abs() < 0.01);
}

#[test]
fn content_visibility_hidden_produces_empty_children() {
    let html = r#"<div class="hidden"><span>should be skipped</span></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(".hidden { content-visibility: hidden; }");
    let root = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));
    fn find_hidden(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if b.style.content_visibility == crate::style::ContentVisibility::Hidden {
            return Some(b);
        }
        b.children.iter().find_map(find_hidden)
    }
    if let Some(hidden_box) = find_hidden(&root) {
        assert!(hidden_box.children.is_empty(), "content-visibility:hidden should have no children");
    }
}

#[test]
fn content_visibility_visible_children_present() {
    let html = r#"<div><span>hello</span></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let root = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));
    let has_children = root.children.iter().any(|c| !c.children.is_empty());
    assert!(has_children, "visible elements should have children");
}

// ── content-visibility: auto — off-screen subtree skip (BB-4) ───────────

/// Find the deepest box whose style has `content-visibility: auto`.
fn find_cv_auto(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
    if b.style.content_visibility == crate::style::ContentVisibility::Auto {
        return Some(b);
    }
    b.children.iter().find_map(find_cv_auto)
}

#[test]
fn content_visibility_auto_below_viewport_skips_children() {
    crate::content_visibility::set_cv_scroll(0.0, 0.0);
    crate::content_visibility::set_cv_relevant(std::collections::HashSet::new());
    // Spacer pushes the auto box to y=2000 — beyond 300 * 1.5 = 450.
    let html = r#"<div class="spacer"></div><div class="cv"><span>off screen</span></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(
        ".spacer { height: 2000px; } .cv { content-visibility: auto; }",
    );
    let root = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));
    let cv = find_cv_auto(&root).expect("auto box present in tree");
    assert!(cv.children.is_empty(), "off-screen auto subtree must be skipped");
    let skipped = crate::content_visibility::take_cv_skipped();
    assert_eq!(skipped.len(), 1, "exactly one node recorded as skipped");
    assert_eq!(skipped[0].0, cv.node);
    assert!(skipped[0].1 >= 2000.0, "recorded top is the collapsed flow position");
}

// ── contain-intrinsic-size under size containment (CSS Box Sizing L4 §5) ──

/// Find the first box that is size-contained via `contain: size`.
fn find_size_contained(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
    if b.style.contain.0 & crate::style::ContainFlags::SIZE.0 != 0 {
        return Some(b);
    }
    b.children.iter().find_map(find_size_contained)
}

#[test]
fn contain_intrinsic_size_sets_block_height() {
    // Size-contained block ignores its tall child and uses the
    // contain-intrinsic-height placeholder (content-box → 100px border-box,
    // no padding/border here).
    let html = r#"<div class="c"><div class="tall"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(
        ".c { contain: size; contain-intrinsic-size: 200px 100px; } .tall { height: 999px; }",
    );
    let root = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));
    let c = find_size_contained(&root).expect("size-contained box present");
    assert!((c.rect.height - 100.0).abs() < 0.5, "height should be 100px, got {}", c.rect.height);
}

#[test]
fn contain_intrinsic_size_none_collapses_block_height() {
    // Size containment with no contain-intrinsic-size → auto height collapses to 0
    // (plus padding/border, which are 0 here).
    let html = r#"<div class="c"><div class="tall"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(
        ".c { contain: size; } .tall { height: 999px; }",
    );
    let root = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));
    let c = find_size_contained(&root).expect("size-contained box present");
    assert!(c.rect.height.abs() < 0.5, "height should collapse to 0, got {}", c.rect.height);
}

#[test]
fn contain_intrinsic_size_sets_inline_block_width() {
    // Size-contained inline-block uses contain-intrinsic-width for shrink-to-fit.
    let html = r#"<div class="c"><div class="tall"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(
        ".c { display: inline-block; contain: size; contain-intrinsic-size: 200px 100px; } \
             .tall { height: 999px; }",
    );
    let root = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));
    let c = find_size_contained(&root).expect("size-contained box present");
    assert!((c.rect.width - 200.0).abs() < 0.5, "width should be 200px, got {}", c.rect.width);
    assert!((c.rect.height - 100.0).abs() < 0.5, "height should be 100px, got {}", c.rect.height);
}

fn find_inline_block(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
    if b.style.display == crate::style::Display::InlineBlock {
        return Some(b);
    }
    b.children.iter().find_map(find_inline_block)
}

#[test]
fn text_only_inline_block_shrinks_to_fit() {
    // BUG-202: a text-only inline-block (no explicit width) must shrink to fit
    // its text, not stretch to the whole line. `preferred_inline_block_width`
    // previously measured only child *boxes* and ignored `InlineRun` segment
    // text, returning None → no shrink-to-fit → the box filled the container.
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 {
            8.0
        }
    }
    let html = r#"<div class="wrap"><span class="pill">abcd</span></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(
        ".wrap { width: 500px; } .pill { display: inline-block; padding: 0 10px; }",
    );
    let root = super::super::layout_measured(&doc, &sheet, Size::new(500.0, 300.0), &Fixed8);
    let pill = find_inline_block(&root).expect("inline-block box present");
    // "abcd" = 4 × 8px = 32px text + 10+10 padding = 52px border-box width —
    // NOT the ~500px container width.
    assert!(
        pill.rect.width < 100.0,
        "inline-block must shrink to fit text, got {}",
        pill.rect.width
    );
    assert!(
        (pill.rect.width - 52.0).abs() < 1.0,
        "expected ~52px (32 text + 20 padding), got {}",
        pill.rect.width
    );
}

fn collect_inline_blocks<'a>(b: &'a super::super::LayoutBox, out: &mut Vec<&'a super::super::LayoutBox>) {
    if b.style.display == crate::style::Display::InlineBlock {
        out.push(b);
    }
    for c in &b.children {
        collect_inline_blocks(c, out);
    }
}

#[test]
fn bug182_vertical_align_middle_uses_baseline_not_line_center() {
    // BUG-182 / TEST-24 row1: three inline-blocks of different heights with
    // vertical-align top/middle/bottom. A 100px top-aligned box pulls the baseline
    // up off the line-box centre, so `vertical-align: middle` (centre aligned to
    // baseline − x-height/2) must NOT be centred on the line box. With default font
    // metrics (ascent 0.8em, x-height 0.5em at 16px) the 60px middle box lands with
    // its top at the line-box top (dy = 0), matching Edge — not at dy = 20 (line
    // centre). The bottom-aligned 40px box sits at dy = 60.
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 {
            8.0
        }
    }
    let html = r#"<div class="row"><div class="ib top"></div><div class="ib mid"></div><div class="ib bot"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(
        ".row { width: 400px; height: 120px; } \
             .ib { display: inline-block; width: 80px; } \
             .top { vertical-align: top; height: 100px; } \
             .mid { vertical-align: middle; height: 60px; } \
             .bot { vertical-align: bottom; height: 40px; }",
    );
    let root = super::super::layout_measured(&doc, &sheet, Size::new(400.0, 300.0), &Fixed8);
    let mut ibs = Vec::new();
    collect_inline_blocks(&root, &mut ibs);
    assert_eq!(ibs.len(), 3, "expected 3 inline-blocks, got {}", ibs.len());
    let top = ibs.iter().find(|b| (b.rect.height - 100.0).abs() < 0.5).unwrap();
    let mid = ibs.iter().find(|b| (b.rect.height - 60.0).abs() < 0.5).unwrap();
    let bot = ibs.iter().find(|b| (b.rect.height - 40.0).abs() < 0.5).unwrap();
    // Middle box top aligns with the line-box top (baseline at 34px from top,
    // box centre at baseline − x/2 = 30px ⇒ top at 0), NOT centred at dy = 20px.
    assert!(
        (mid.rect.y - top.rect.y).abs() < 1.0,
        "middle box should align to line top (baseline-correct), got dy={}",
        mid.rect.y - top.rect.y
    );
    // Bottom-aligned box bottom touches the line-box bottom (top.y + 100).
    assert!(
        ((bot.rect.y + bot.rect.height) - (top.rect.y + 100.0)).abs() < 1.0,
        "bottom box bottom should touch line bottom, got {}",
        bot.rect.y + bot.rect.height
    );
}

/// Fixed-metric measurer for the IFC tests: every glyph is 8px wide, so the
/// default trait metrics apply (ascent 0.8em, descent 0.2em) and every
/// expected number below is hand-computable.
struct Ifc8;
impl super::super::super::TextMeasurer for Ifc8 {
    fn char_width(&self, _: char, _: f32) -> f32 {
        8.0
    }
}

fn ifc_row(html: &str, css: &str) -> super::super::LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout_measured(&doc, &sheet, Size::new(500.0, 300.0), &Ifc8);
    fn find(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::InlineBlockRow) {
            return Some(b);
        }
        b.children.iter().find_map(find)
    }
    find(&root).expect("InlineBlockRow present").clone()
}

#[test]
fn ifc1_text_before_inline_block_stays_on_one_line() {
    // IFC-1: the run's box is as wide as the space it was offered, so
    // advancing by `rect.width` pushed the inline-block past the container's
    // right edge and onto a line of its own — «Aa <ib> Bb» came out three
    // lines tall. The run must advance by the extent of its own last line.
    let row = ifc_row(
        r#"<p>Aa <span class="ib"></span> Bb</p>"#,
        "p { width: 500px; } .ib { display: inline-block; width: 16px; height: 16px; }",
    );
    let kids: Vec<&super::super::LayoutBox> = row
        .children
        .iter()
        .filter(|c| !matches!(c.kind, super::super::BoxKind::Skip))
        .collect();
    assert_eq!(kids.len(), 3, "run + inline-block + run");
    // "Aa" = 2 × 8px, then the collapsed space (8px) → x = 24; the trailing
    // run opens with its own collapsed space → 24 + 16 + 8 = 48.
    assert_eq!(kids[0].rect.x - row.rect.x, 0.0);
    assert_eq!(
        kids[1].rect.x - row.rect.x,
        24.0,
        "inline-block follows the text, not a new line"
    );
    assert_eq!(
        kids[2].rect.x - row.rect.x,
        48.0,
        "trailing text follows the inline-block"
    );
    // One line box: strut ascent 12.8 vs the 16px box's bottom margin edge →
    // above = 16, below = max(strut 3.2, run 4.8) = 4.8.
    assert!(
        (row.rect.height - 20.8).abs() < 0.01,
        "one line box expected, got h={}",
        row.rect.height
    );
}

#[test]
fn ifc1_empty_inline_block_sits_on_the_text_baseline() {
    // CSS 2.1 §10.8.1: an inline-block with no in-flow line box aligns by its
    // bottom margin edge, so a 16px box pushes the baseline down to 16 and the
    // text (ascent 12.8 + half-leading 1.6 = 14.4 above its own top) drops by
    // the 1.6px difference, rounded to 2.
    let row = ifc_row(
        r#"<p>Aa <span class="ib"></span> Bb</p>"#,
        "p { width: 500px; } .ib { display: inline-block; width: 16px; height: 16px; }",
    );
    let ib = row
        .children
        .iter()
        .find(|c| c.style.display == crate::style::Display::InlineBlock)
        .expect("inline-block");
    let run = row
        .children
        .iter()
        .find(|c| matches!(c.kind, super::super::BoxKind::InlineRun { .. }))
        .expect("text run");
    assert_eq!(ib.rect.y - row.rect.y, 0.0, "tallest box opens the line");
    assert_eq!(run.rect.y - row.rect.y, 2.0, "text drops onto the shared baseline");
}

#[test]
fn ifc1_inline_block_with_text_shares_the_outer_baseline() {
    // The case that separates baseline alignment from the bottom-alignment it
    // replaced: an inline-block whose own last line box carries text offers
    // that line's baseline, so both texts sit at the same y and the line box
    // is exactly one line-height tall.
    let row = ifc_row(
        r#"<p>Aa <span class="ib">Xx</span> Bb</p>"#,
        "p { width: 500px; } .ib { display: inline-block; }",
    );
    let ib = row
        .children
        .iter()
        .find(|c| c.style.display == crate::style::Display::InlineBlock)
        .expect("inline-block");
    let runs: Vec<&super::super::LayoutBox> = row
        .children
        .iter()
        .filter(|c| matches!(c.kind, super::super::BoxKind::InlineRun { .. }))
        .collect();
    assert_eq!(ib.rect.y, runs[0].rect.y, "inner and outer text share a baseline");
    assert_eq!(runs[0].rect.y, runs[1].rect.y);
    assert!(
        (row.rect.height - 19.2).abs() < 0.01,
        "one line-height tall, got h={}",
        row.rect.height
    );
}

#[test]
fn ifc1_overflow_hidden_inline_block_aligns_by_bottom_margin_edge() {
    // CSS 2.1 §10.8.1 — `overflow` other than `visible` suppresses the inner
    // baseline, so the box behaves like the empty one above even though it
    // does have a line box of its own.
    let row = ifc_row(
        r#"<p>Aa <span class="ib">Xx</span> Bb</p>"#,
        "p { width: 500px; } \
             .ib { display: inline-block; overflow: hidden; width: 16px; height: 16px; }",
    );
    let ib = row
        .children
        .iter()
        .find(|c| c.style.display == crate::style::Display::InlineBlock)
        .expect("inline-block");
    let run = row
        .children
        .iter()
        .find(|c| matches!(c.kind, super::super::BoxKind::InlineRun { .. }))
        .expect("text run");
    assert_eq!(ib.rect.y - row.rect.y, 0.0);
    assert_eq!(run.rect.y - row.rect.y, 2.0);
}

#[test]
fn ifc1_form_control_offers_its_label_baseline() {
    // A `<button>`'s label is a real line box of its own, so the control
    // aligns on that baseline (1px border + 14.4) rather than on its bottom
    // margin edge — the text next to it must not move at all.
    let row = ifc_row(
        r#"<p>Aa <button>Xx</button> Bb</p>"#,
        "p { width: 500px; }",
    );
    let ctl = row
        .children
        .iter()
        .find(|c| matches!(c.kind, super::super::BoxKind::FormControl { .. }))
        .expect("form control");
    let run = row
        .children
        .iter()
        .find(|c| matches!(c.kind, super::super::BoxKind::InlineRun { .. }))
        .expect("text run");
    // Control baseline = 1px top border + 14.4; text baseline = 14.4, so the
    // line's baseline is 15.4 and the text drops by 1.
    assert_eq!(run.rect.y - row.rect.y, 1.0, "text drops by the control's border");
    assert_eq!(ctl.rect.y - row.rect.y, 0.0);
}

#[test]
fn ifc2_image_shares_the_line_with_the_text_around_it() {
    // IFC-2: `<img>` used to be block-level, so «Aa <img> Bb» came out three
    // lines tall. It is inline-level replaced content now — one line box,
    // three pieces in it, laid left to right exactly like an inline-block.
    let row = ifc_row(
        r#"<p>Aa <img width="16" height="16"> Bb</p>"#,
        "p { width: 500px; }",
    );
    let kids: Vec<&super::super::LayoutBox> = row
        .children
        .iter()
        .filter(|c| !matches!(c.kind, super::super::BoxKind::Skip))
        .collect();
    assert_eq!(kids.len(), 3, "run + image + run");
    assert!(matches!(kids[1].kind, super::super::BoxKind::Image { .. }));
    // "Aa" = 2 × 8px + the collapsed space = 24; the trailing run opens with
    // its own collapsed space → 24 + 16 + 8 = 48.
    assert_eq!(kids[0].rect.x - row.rect.x, 0.0);
    assert_eq!(kids[1].rect.x - row.rect.x, 24.0, "image follows the text");
    assert_eq!(kids[2].rect.x - row.rect.x, 48.0, "text follows the image");
    // above = 16 (the image's bottom margin edge), below = max(strut 3.2,
    // run 4.8) = 4.8.
    assert!(
        (row.rect.height - 20.8).abs() < 0.01,
        "one line box expected, got h={}",
        row.rect.height
    );
}

#[test]
fn ifc2_image_sits_on_the_baseline_by_its_bottom_margin_edge() {
    // CSS 2.1 §10.8.1 — a replaced element offers no baseline of its own, so
    // its bottom MARGIN edge (not its border box, and not the top of the
    // line) is what lands on the line's baseline.
    let row = ifc_row(
        r#"<p>Aa <img class="i" width="16" height="16"> Bb</p>"#,
        "p { width: 500px; } .i { margin-bottom: 4px; }",
    );
    let img = row
        .children
        .iter()
        .find(|c| matches!(c.kind, super::super::BoxKind::Image { .. }))
        .expect("image");
    let run = row
        .children
        .iter()
        .find(|c| matches!(c.kind, super::super::BoxKind::InlineRun { .. }))
        .expect("text run");
    // Margin box height 20 → baseline at 20 from the top of the line; the
    // text's own baseline is 14.4, so it drops by 5.6, rounded to 6.
    assert_eq!(img.rect.y - row.rect.y, 0.0, "the tallest box opens the line");
    assert_eq!(run.rect.y - row.rect.y, 6.0, "text drops onto the shared baseline");
    assert!(
        (row.rect.height - 24.8).abs() < 0.01,
        "margin counts towards the line box, got h={}",
        row.rect.height
    );
}

#[test]
fn ifc2_two_images_share_one_line_with_a_collapsed_space() {
    // The whitespace between two images is one collapsed inter-word gap
    // (CSS Text L3 §4.1.1), the same `InlineSpace` two inline-blocks get —
    // not a line break, which is what block-level `<img>` produced.
    let row = ifc_row(
        r#"<p><img width="20" height="20"> <img width="20" height="20"></p>"#,
        "p { width: 500px; }",
    );
    let imgs: Vec<&super::super::LayoutBox> = row
        .children
        .iter()
        .filter(|c| matches!(c.kind, super::super::BoxKind::Image { .. }))
        .collect();
    assert_eq!(imgs.len(), 2);
    assert_eq!(imgs[0].rect.y, imgs[1].rect.y, "both images on one line");
    assert_eq!(imgs[1].rect.x - imgs[0].rect.x, 28.0, "20px box + 8px space");
}

#[test]
fn ifc2_image_wraps_onto_the_next_line_when_it_does_not_fit() {
    // An atomic inline that overflows the line moves to the next one whole
    // (CSS 2.1 §9.4.2) — it is never split, and it never overhangs the
    // container the way advancing by the run's full box width used to make
    // the inline-block do (IFC-1).
    let row = ifc_row(
        r#"<p>Aa <img width="60" height="10"> <img width="60" height="10"></p>"#,
        "p { width: 100px; }",
    );
    let imgs: Vec<&super::super::LayoutBox> = row
        .children
        .iter()
        .filter(|c| matches!(c.kind, super::super::BoxKind::Image { .. }))
        .collect();
    assert_eq!(imgs.len(), 2);
    assert_eq!(imgs[0].rect.x - row.rect.x, 24.0, "first image follows «Aa »");
    assert_eq!(imgs[1].rect.x - row.rect.x, 0.0, "second image opens a new line");
    assert!(
        imgs[1].rect.y > imgs[0].rect.y,
        "second image must be below the first, got y={} vs {}",
        imgs[1].rect.y,
        imgs[0].rect.y
    );
}

#[test]
fn ifc2_floated_image_stays_out_of_the_inline_row() {
    // CSS 2.1 §9.7 — a float is block-level whatever its `display` says, and
    // only the block branch of `lay_out` implements the wrap-around. Before
    // IFC-2 every `<img>` was block-level, so a floated one worked; letting
    // the UA-default `inline` pull it into `InlineBlockRow` would have traded
    // one layout for another instead of adding one.
    let doc = lumen_html_parser::parse(r#"<div>Aa <img class="f" width="16" height="16"> Bb</div>"#);
    let sheet = lumen_css_parser::parse(".f { float: left; }");
    let root = super::super::layout_measured(&doc, &sheet, Size::new(500.0, 300.0), &Ifc8);
    fn image_parent_is_row(b: &super::super::LayoutBox, in_row: bool) -> Option<bool> {
        for c in &b.children {
            if matches!(c.kind, super::super::BoxKind::Image { .. }) {
                return Some(in_row);
            }
            if let Some(v) = image_parent_is_row(
                c,
                matches!(c.kind, super::super::BoxKind::InlineBlockRow),
            ) {
                return Some(v);
            }
        }
        None
    }
    assert_eq!(
        image_parent_is_row(&root, false),
        Some(false),
        "плавающая картинка обязана остаться блочной"
    );
}

#[test]
fn content_visibility_auto_in_viewport_lays_out_children() {
    crate::content_visibility::set_cv_scroll(0.0, 0.0);
    crate::content_visibility::set_cv_relevant(std::collections::HashSet::new());
    let html = r#"<div class="cv"><div class="inner"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(
        ".cv { content-visibility: auto; } .inner { height: 50px; }",
    );
    let root = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));
    let cv = find_cv_auto(&root).expect("auto box present in tree");
    assert!(!cv.children.is_empty(), "on-screen auto subtree is laid out");
    assert!(crate::content_visibility::take_cv_skipped().is_empty());
}

#[test]
fn content_visibility_auto_relevant_ratchet_forces_layout() {
    crate::content_visibility::set_cv_scroll(0.0, 0.0);
    crate::content_visibility::set_cv_relevant(std::collections::HashSet::new());
    let html = r#"<div class="spacer"></div><div class="cv"><div class="inner"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(
        ".spacer { height: 2000px; } .cv { content-visibility: auto; } .inner { height: 50px; }",
    );
    // First pass: skipped.
    let root = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));
    let cv_node = find_cv_auto(&root).expect("auto box present").node;
    let skipped = crate::content_visibility::take_cv_skipped();
    assert_eq!(skipped.len(), 1);
    // Shell ratchets the node relevant (user scrolled near it) → laid out.
    let mut rel = std::collections::HashSet::new();
    rel.insert(cv_node);
    crate::content_visibility::set_cv_relevant(rel);
    let root2 = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));
    let cv2 = find_cv_auto(&root2).expect("auto box present");
    assert!(!cv2.children.is_empty(), "relevant node must not be skipped");
    assert!(crate::content_visibility::take_cv_skipped().is_empty());
    crate::content_visibility::set_cv_relevant(std::collections::HashSet::new());
}

#[test]
fn content_visibility_auto_scroll_offset_expands_layout() {
    crate::content_visibility::set_cv_relevant(std::collections::HashSet::new());
    // Root scroll 1800 → bound = 1800 + 450 = 2250 ≥ 2000 → laid out.
    crate::content_visibility::set_cv_scroll(0.0, 1800.0);
    let html = r#"<div class="spacer"></div><div class="cv"><div class="inner"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(
        ".spacer { height: 2000px; } .cv { content-visibility: auto; } .inner { height: 50px; }",
    );
    let root = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));
    let cv = find_cv_auto(&root).expect("auto box present");
    assert!(!cv.children.is_empty(), "auto subtree inside scrolled viewport is laid out");
    assert!(crate::content_visibility::take_cv_skipped().is_empty());
    crate::content_visibility::set_cv_scroll(0.0, 0.0);
}

#[test]
fn content_visibility_auto_skipped_keeps_explicit_height() {
    crate::content_visibility::set_cv_scroll(0.0, 0.0);
    crate::content_visibility::set_cv_relevant(std::collections::HashSet::new());
    let html = r#"<div class="spacer"></div><div class="cv"><div class="inner"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(
        ".spacer { height: 2000px; } .cv { content-visibility: auto; height: 300px; } .inner { height: 50px; }",
    );
    let root = super::super::layout(&doc, &sheet, Size::new(300.0, 300.0));
    let cv = find_cv_auto(&root).expect("auto box present");
    assert!(cv.children.is_empty(), "subtree skipped");
    assert!((cv.rect.height - 300.0).abs() < 0.5, "explicit height preserved, got {}", cv.rect.height);
    let _ = crate::content_visibility::take_cv_skipped();
}

