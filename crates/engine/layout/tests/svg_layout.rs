//! SVG layout tests: viewBox, basic shapes (rect/circle/ellipse/line/path),
//! and SVG presentational attributes fill/stroke/fill-opacity/stroke-opacity/stroke-width.

use lumen_core::geom::Size;
use lumen_layout::{layout, BoxKind, Color, SvgPaint, SvgShapeKind};
use lumen_html_parser::parse as parse_html;
use lumen_css_parser::parse as parse_css;

// Neutralise the UA `body { margin: 8px }` (HTML Rendering §14.3.3, BUG-204) so
// SVG shapes are positioned from the page origin, as these tests expect.
const BODY_RESET: &str = "body{margin:0}";

fn do_layout(html: &str) -> lumen_layout::LayoutBox {
    let doc = parse_html(html);
    let sheet = parse_css(BODY_RESET);
    layout(&doc, &sheet, Size::new(800.0, 600.0))
}

fn do_layout_css(html: &str, css: &str) -> lumen_layout::LayoutBox {
    let doc = parse_html(html);
    let sheet = parse_css(&format!("{BODY_RESET}{css}"));
    layout(&doc, &sheet, Size::new(800.0, 600.0))
}

/// Walks the layout tree and returns the first box matching `pred`.
fn find_box<'a>(
    b: &'a lumen_layout::LayoutBox,
    pred: &dyn Fn(&lumen_layout::LayoutBox) -> bool,
) -> Option<&'a lumen_layout::LayoutBox> {
    if pred(b) {
        return Some(b);
    }
    for child in &b.children {
        if let Some(found) = find_box(child, pred) {
            return Some(found);
        }
    }
    None
}

/// Returns the first `SvgRoot` box in the tree.
fn first_svg_root(root: &lumen_layout::LayoutBox) -> Option<&lumen_layout::LayoutBox> {
    find_box(root, &|b| matches!(b.kind, BoxKind::SvgRoot { .. }))
}

/// Returns the first `SvgShape` box in the tree.
fn first_svg_shape(root: &lumen_layout::LayoutBox) -> Option<&lumen_layout::LayoutBox> {
    find_box(root, &|b| matches!(b.kind, BoxKind::SvgShape { .. }))
}

// ── SvgRoot sizing ────────────────────────────────────────────────────────────

#[test]
fn svg_root_default_size() {
    // No width/height attributes → SVG defaults: 300×150.
    let tree = do_layout("<svg></svg>");
    let svg = first_svg_root(&tree).expect("SvgRoot not found");
    assert_eq!(svg.rect.width, 300.0, "default width");
    assert_eq!(svg.rect.height, 150.0, "default height");
}

#[test]
fn svg_root_explicit_size() {
    let tree = do_layout(r#"<svg width="200" height="100"></svg>"#);
    let svg = first_svg_root(&tree).expect("SvgRoot not found");
    assert_eq!(svg.rect.width, 200.0);
    assert_eq!(svg.rect.height, 100.0);
}

#[test]
fn svg_root_view_box_size() {
    // viewBox only → SVG root inherits viewBox dimensions.
    let tree = do_layout(r#"<svg viewBox="0 0 400 300"></svg>"#);
    let svg = first_svg_root(&tree).expect("SvgRoot not found");
    assert_eq!(svg.rect.width, 400.0);
    assert_eq!(svg.rect.height, 300.0);
}

// ── rect ──────────────────────────────────────────────────────────────────────

#[test]
fn svg_rect_no_viewbox() {
    // Without viewBox, shapes are mapped 1:1 from SVG user units to CSS px.
    let tree = do_layout(r#"<svg width="200" height="200"><rect x="10" y="20" width="50" height="30"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    assert!((shape.rect.x - 10.0).abs() < 0.01, "x");
    assert!((shape.rect.y - 20.0).abs() < 0.01, "y");
    assert!((shape.rect.width - 50.0).abs() < 0.01, "width");
    assert!((shape.rect.height - 30.0).abs() < 0.01, "height");
    assert!(matches!(shape.kind, BoxKind::SvgShape { shape: SvgShapeKind::Rect { .. }, .. }));
}

#[test]
fn svg_rect_with_viewbox_scale() {
    // viewBox="0 0 100 100" + CSS 200×200 → scale 2×.
    let tree = do_layout(r#"<svg width="200" height="200" viewBox="0 0 100 100">
        <rect x="10" y="10" width="20" height="20"/>
    </svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    // x=10 * scale2 = 20  (plus SVG rect.x origin)
    let svg = first_svg_root(&tree).unwrap();
    let expected_x = svg.rect.x + 10.0 * 2.0;
    let expected_y = svg.rect.y + 10.0 * 2.0;
    assert!((shape.rect.x - expected_x).abs() < 0.01, "scaled x: got {}, expected {}", shape.rect.x, expected_x);
    assert!((shape.rect.y - expected_y).abs() < 0.01, "scaled y");
    assert!((shape.rect.width - 40.0).abs() < 0.01, "scaled width");
    assert!((shape.rect.height - 40.0).abs() < 0.01, "scaled height");
}

#[test]
fn svg_rect_viewbox_with_min_xy() {
    // viewBox="50 50 100 100" + CSS 100×100 → scale 1×, origin shifted by -50px.
    let tree = do_layout(r#"<svg width="100" height="100" viewBox="50 50 100 100">
        <rect x="60" y="60" width="10" height="10"/>
    </svg>"#);
    let svg = first_svg_root(&tree).unwrap();
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    // origin_x = svg.rect.x - 50*(100/100) = svg.rect.x - 50
    // shape.x = origin_x + 60*1.0 = svg.rect.x + 10
    let expected_x = svg.rect.x + (60.0 - 50.0);
    assert!((shape.rect.x - expected_x).abs() < 0.01, "min_x offset: got {}, expected {}", shape.rect.x, expected_x);
}

// ── circle ────────────────────────────────────────────────────────────────────

#[test]
fn svg_circle_bbox() {
    let tree = do_layout(r#"<svg width="100" height="100"><circle cx="50" cy="50" r="20"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    let svg = first_svg_root(&tree).unwrap();
    // bbox: x = cx-r, y = cy-r, w = 2r, h = 2r
    assert!((shape.rect.x - (svg.rect.x + 30.0)).abs() < 0.01);
    assert!((shape.rect.y - (svg.rect.y + 30.0)).abs() < 0.01);
    assert!((shape.rect.width - 40.0).abs() < 0.01);
    assert!((shape.rect.height - 40.0).abs() < 0.01);
    assert!(matches!(shape.kind, BoxKind::SvgShape { shape: SvgShapeKind::Circle { .. }, .. }));
}

// ── ellipse ───────────────────────────────────────────────────────────────────

#[test]
fn svg_ellipse_bbox() {
    let tree = do_layout(r#"<svg width="200" height="100"><ellipse cx="100" cy="50" rx="40" ry="30"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    let svg = first_svg_root(&tree).unwrap();
    assert!((shape.rect.x - (svg.rect.x + 60.0)).abs() < 0.01, "x");
    assert!((shape.rect.y - (svg.rect.y + 20.0)).abs() < 0.01, "y");
    assert!((shape.rect.width - 80.0).abs() < 0.01, "width");
    assert!((shape.rect.height - 60.0).abs() < 0.01, "height");
    assert!(matches!(shape.kind, BoxKind::SvgShape { shape: SvgShapeKind::Ellipse { .. }, .. }));
}

// ── line ──────────────────────────────────────────────────────────────────────

#[test]
fn svg_line_bbox() {
    let tree = do_layout(r#"<svg width="100" height="100"><line x1="10" y1="10" x2="90" y2="50"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    let svg = first_svg_root(&tree).unwrap();
    // min_x=10, min_y=10, width=80, height=40
    assert!((shape.rect.x - (svg.rect.x + 10.0)).abs() < 0.01, "x");
    assert!((shape.rect.y - (svg.rect.y + 10.0)).abs() < 0.01, "y");
    assert!((shape.rect.width - 80.0).abs() < 0.01, "width");
    assert!((shape.rect.height - 40.0).abs() < 0.01, "height");
    assert!(matches!(shape.kind, BoxKind::SvgShape { shape: SvgShapeKind::Line { .. }, .. }));
}

// ── path ──────────────────────────────────────────────────────────────────────

#[test]
fn svg_path_zero_rect() {
    // Path bbox is deferred to paint → rect = ZERO for Phase 2.
    let tree = do_layout(r#"<svg width="100" height="100"><path d="M10,10 L90,90"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    assert_eq!(shape.rect.x, 0.0);
    assert_eq!(shape.rect.width, 0.0);
    assert!(matches!(shape.kind, BoxKind::SvgShape { shape: SvgShapeKind::Path { .. }, .. }));
}

// ── <g> group ─────────────────────────────────────────────────────────────────

#[test]
fn svg_g_group_contains_children() {
    let tree = do_layout(r#"<svg width="200" height="200">
        <g>
            <rect x="10" y="10" width="50" height="50"/>
            <circle cx="100" cy="100" r="20"/>
        </g>
    </svg>"#);
    let svg = first_svg_root(&tree).unwrap();
    // Group should be Block-kind with 2 children
    let group = svg.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).expect("group not found");
    assert_eq!(group.children.len(), 2, "group must have 2 shape children");
}

// ── SVG fill / stroke presentation attributes ────────────────────────────────

#[test]
fn svg_fill_explicit_color() {
    let tree = do_layout(r#"<svg width="100" height="100"><rect x="0" y="0" width="50" height="50" style="fill: #ff0000"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    assert_eq!(shape.style.svg_fill, SvgPaint::Color(Color { r: 255, g: 0, b: 0, a: 255 }));
}

#[test]
fn svg_fill_none() {
    let tree = do_layout(r#"<svg width="100" height="100"><circle cx="50" cy="50" r="20" style="fill: none"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    assert_eq!(shape.style.svg_fill, SvgPaint::None);
}

#[test]
fn svg_fill_currentcolor() {
    let tree = do_layout(r#"<svg width="100" height="100"><rect x="0" y="0" width="50" height="50" style="fill: currentColor"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    assert_eq!(shape.style.svg_fill, SvgPaint::CurrentColor);
}

#[test]
fn svg_fill_default_is_black() {
    let tree = do_layout(r#"<svg width="100" height="100"><rect x="0" y="0" width="50" height="50"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    assert_eq!(shape.style.svg_fill, SvgPaint::Color(Color::BLACK));
}

#[test]
fn svg_stroke_explicit_color() {
    let tree = do_layout(r#"<svg width="100" height="100"><rect x="0" y="0" width="50" height="50" style="stroke: #0000ff"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    assert_eq!(shape.style.svg_stroke, SvgPaint::Color(Color { r: 0, g: 0, b: 255, a: 255 }));
}

#[test]
fn svg_stroke_default_is_none() {
    let tree = do_layout(r#"<svg width="100" height="100"><rect x="0" y="0" width="50" height="50"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    assert_eq!(shape.style.svg_stroke, SvgPaint::None);
}

#[test]
fn svg_fill_opacity() {
    let tree = do_layout(r#"<svg width="100" height="100"><circle cx="50" cy="50" r="20" style="fill: red; fill-opacity: 0.5"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    assert!((shape.style.svg_fill_opacity - 0.5).abs() < 0.001);
}

#[test]
fn svg_stroke_opacity() {
    let tree = do_layout(r#"<svg width="100" height="100"><rect x="0" y="0" width="50" height="50" style="stroke: blue; stroke-opacity: 0.3"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    assert!((shape.style.svg_stroke_opacity - 0.3).abs() < 0.001);
}

#[test]
fn svg_stroke_width_px() {
    let tree = do_layout(r#"<svg width="100" height="100"><rect x="0" y="0" width="50" height="50" style="stroke-width: 4px"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    assert!((shape.style.svg_stroke_width - 4.0).abs() < 0.01);
}

#[test]
fn svg_fill_inherited_from_parent() {
    // fill on <g> is inherited by <rect> inside it.
    let tree = do_layout(r#"<svg width="100" height="100">
        <g style="fill: #00ff00">
            <rect x="0" y="0" width="50" height="50"/>
        </g>
    </svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    assert_eq!(shape.style.svg_fill, SvgPaint::Color(Color { r: 0, g: 255, b: 0, a: 255 }));
}

#[test]
fn svg_fill_css_rule() {
    let tree = do_layout_css(
        r#"<svg width="100" height="100"><rect class="r" x="0" y="0" width="50" height="50"/></svg>"#,
        ".r { fill: #aabbcc; }",
    );
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    assert_eq!(shape.style.svg_fill, SvgPaint::Color(Color { r: 0xaa, g: 0xbb, b: 0xcc, a: 255 }));
}

// ── multiple shapes ───────────────────────────────────────────────────────────

#[test]
fn svg_multiple_shapes_all_built() {
    let tree = do_layout(r#"<svg width="300" height="300">
        <rect x="0" y="0" width="100" height="100"/>
        <circle cx="150" cy="150" r="50"/>
        <ellipse cx="200" cy="100" rx="30" ry="20"/>
        <line x1="0" y1="0" x2="100" y2="100"/>
        <path d="M0,0 Z"/>
    </svg>"#);
    let svg = first_svg_root(&tree).unwrap();
    assert_eq!(svg.children.len(), 5, "all 5 shapes must be built");
}

// ── preserveAspectRatio (Phase 1) ───────────────────────────────────────────

#[test]
fn svg_preserve_aspect_ratio_meet_default() {
    // Default preserveAspectRatio="xMidYMid meet" — uniform scale to fit inside.
    let tree = do_layout(r#"<svg width="200" height="100" viewBox="0 0 100 100"></svg>"#);
    let svg = first_svg_root(&tree).expect("SvgRoot not found");
    // viewBox is square (100×100), SVG is 200×100 (2:1 aspect ratio).
    // With 'meet', scale to fit: min(200/100, 100/100) = 1.0
    // viewBox should be scaled 1:1, centered horizontally.
    assert_eq!(svg.rect.width, 200.0);
    assert_eq!(svg.rect.height, 100.0);
}

#[test]
fn svg_preserve_aspect_ratio_slice() {
    // preserveAspectRatio="xMidYMid slice" — uniform scale to cover.
    let tree = do_layout(r#"<svg width="200" height="100" viewBox="0 0 100 100" preserveAspectRatio="xMidYMid slice"></svg>"#);
    let svg = first_svg_root(&tree).expect("SvgRoot not found");
    // With 'slice', scale to cover: max(200/100, 100/100) = 2.0
    // viewBox will be scaled 2× and clipped.
    assert_eq!(svg.rect.width, 200.0);
    assert_eq!(svg.rect.height, 100.0);
}

#[test]
fn svg_preserve_aspect_ratio_xmin() {
    // preserveAspectRatio="xMinYMid meet" — left-aligned, centered vertically.
    let tree = do_layout(r#"<svg width="200" height="100" viewBox="0 0 100 100" preserveAspectRatio="xMinYMid meet"></svg>"#);
    let svg = first_svg_root(&tree).expect("SvgRoot not found");
    // Scaling is still 1:1 to fit inside, but aligned to left instead of center.
    assert_eq!(svg.rect.width, 200.0);
    assert_eq!(svg.rect.height, 100.0);
}

// ── SVG transform parsing (Phase 1) ──────────────────────────────────────

#[test]
fn svg_transform_attribute_present() {
    // SVG transform attribute should be parsed and prepared for P4 CSS wiring.
    // For now, we just verify it doesn't break layout.
    let tree = do_layout(r#"<svg width="100" height="100">
        <rect x="10" y="10" width="50" height="50" transform="translate(5 10)"/>
    </svg>"#);
    let svg = first_svg_root(&tree).expect("SvgRoot not found");
    assert_eq!(svg.children.len(), 1);
}

#[test]
fn svg_nested_svg_basic() {
    // Nested SVG elements create new coordinate systems.
    // Phase 1 basic support: nested SVG is treated as a container.
    let tree = do_layout(r#"<svg width="100" height="100">
        <svg x="10" y="10" width="50" height="50" viewBox="0 0 100 100">
            <rect x="0" y="0" width="50" height="50"/>
        </svg>
    </svg>"#);
    let svg = first_svg_root(&tree).expect("SvgRoot not found");
    // Nested SVG should be present in children (as Block for now).
    assert!(!svg.children.is_empty());
}

// ── SVG transform composition (Phase 2) ───────────────────────────────────

#[test]
fn svg_transform_translate_basic() {
    let tree = do_layout(r#"<svg width="100" height="100">
        <rect x="10" y="10" width="20" height="20" transform="translate(5 5)"/>
    </svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    let svg = first_svg_root(&tree).unwrap();
    // rect @ (10,10) + translate(5,5) = (15,15)
    let expected_x = svg.rect.x + 15.0;
    let expected_y = svg.rect.y + 15.0;
    assert!((shape.rect.x - expected_x).abs() < 0.1, "translated x: got {}, expected {}", shape.rect.x, expected_x);
    assert!((shape.rect.y - expected_y).abs() < 0.1, "translated y");
}

#[test]
fn svg_transform_scale_basic() {
    let tree = do_layout(r#"<svg width="100" height="100">
        <rect x="10" y="10" width="20" height="20" transform="scale(2)"/>
    </svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    let svg = first_svg_root(&tree).unwrap();
    // rect @ (10,10) size 20×20, scaled 2× = (20,20) size 40×40
    let expected_x = svg.rect.x + 20.0;
    let expected_y = svg.rect.y + 20.0;
    assert!((shape.rect.x - expected_x).abs() < 0.1, "scaled x");
    assert!((shape.rect.y - expected_y).abs() < 0.1, "scaled y");
    assert!((shape.rect.width - 40.0).abs() < 0.1, "scaled width");
    assert!((shape.rect.height - 40.0).abs() < 0.1, "scaled height");
}

#[test]
fn svg_group_transform_propagates_to_children() {
    let tree = do_layout(r#"<svg width="100" height="100">
        <g transform="translate(10 10)">
            <rect x="0" y="0" width="20" height="20"/>
        </g>
    </svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    let svg = first_svg_root(&tree).unwrap();
    // group @ translate(10,10), rect @ (0,0) = (10,10)
    let expected_x = svg.rect.x + 10.0;
    let expected_y = svg.rect.y + 10.0;
    assert!((shape.rect.x - expected_x).abs() < 0.1, "group transform applied: got {}, expected {}", shape.rect.x, expected_x);
    assert!((shape.rect.y - expected_y).abs() < 0.1);
}

#[test]
fn svg_nested_transforms_compose() {
    let tree = do_layout(r#"<svg width="100" height="100">
        <g transform="translate(10 10)">
            <rect x="5" y="5" width="20" height="20" transform="translate(5 5)"/>
        </g>
    </svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    let svg = first_svg_root(&tree).unwrap();
    // group @ translate(10,10) + rect @ translate(5,5) = combined translate(15,15)
    // rect @ (5,5) + translate(15,15) = (20,20)
    let expected_x = svg.rect.x + 20.0;
    let expected_y = svg.rect.y + 20.0;
    assert!((shape.rect.x - expected_x).abs() < 0.1, "composed transforms: got {}, expected {}", shape.rect.x, expected_x);
    assert!((shape.rect.y - expected_y).abs() < 0.1);
}

#[test]
fn svg_transform_rotate_basic() {
    let tree = do_layout(r#"<svg width="100" height="100">
        <circle cx="50" cy="50" r="20" transform="rotate(90)"/>
    </svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    // Circle rotated 90° around origin — bbox should change
    // Original bbox: (30,30) to (70,70)
    // After rotate(90): bbox becomes roughly (30,30) to (70,70) (circle is symmetric)
    assert!(shape.rect.width > 0.0, "rotated shape has non-zero width");
    assert!(shape.rect.height > 0.0, "rotated shape has non-zero height");
}

/// Collects every box matching `pred` in document order.
fn collect_boxes<'a>(
    b: &'a lumen_layout::LayoutBox,
    pred: &dyn Fn(&lumen_layout::LayoutBox) -> bool,
    out: &mut Vec<&'a lumen_layout::LayoutBox>,
) {
    if pred(b) {
        out.push(b);
    }
    for child in &b.children {
        collect_boxes(child, pred, out);
    }
}

#[test]
fn svg_path_inline_block_wrap_keeps_child_in_root() {
    // BUG-262 regression: an inline-block `<svg>` whose `<path>` wraps to a second
    // line is laid out twice. The first pass used to overwrite `svg_transform` with
    // the document-space paint matrix (carrying the line-1 viewport translation);
    // the second pass then misread that as the element transform and drifted the
    // path child far outside its SVG root's clip rect (TEST-119 lost a whole cell).
    // Two 460px-wide inline-block SVGs do not both fit in the 800px viewport, so the
    // second wraps. Its path child must stay anchored at the second SVG root origin.
    let html = r##"
        <div style="display:inline-block">
          <svg width="460" height="320"><path d="M 40 40 L 420 40 L 420 280 L 40 280 Z" fill="#ff0"/></svg>
        </div><div style="display:inline-block">
          <svg width="460" height="320"><path d="M 40 40 L 420 40 L 420 280 L 40 280 Z" fill="#0ff"/></svg>
        </div>"##;
    let tree = do_layout(html);

    let mut roots = Vec::new();
    collect_boxes(&tree, &|b| matches!(b.kind, BoxKind::SvgRoot { .. }), &mut roots);
    let mut paths = Vec::new();
    collect_boxes(
        &tree,
        &|b| matches!(b.kind, BoxKind::SvgShape { shape: SvgShapeKind::Path { .. }, .. }),
        &mut paths,
    );
    assert_eq!(roots.len(), 2, "two SVG roots");
    assert_eq!(paths.len(), 2, "two SVG paths");

    // Sanity: the second SVG actually wrapped to a new line (below the first).
    assert!(roots[1].rect.y > roots[0].rect.y, "second SVG wrapped to line 2");

    // The path child branch anchors `rect` at its SVG root origin (zero-size box;
    // paint shifts the raw `d` coords by this origin). Both paths must therefore
    // land exactly on their own root origin — the wrapped one included.
    for (i, (path, root)) in paths.iter().zip(roots.iter()).enumerate() {
        assert!(
            (path.rect.x - root.rect.x).abs() < 0.5,
            "path[{i}] x {} should match its SVG root x {}",
            path.rect.x, root.rect.x,
        );
        assert!(
            (path.rect.y - root.rect.y).abs() < 0.5,
            "path[{i}] y {} should match its SVG root y {}",
            path.rect.y, root.rect.y,
        );
    }
}

#[test]
fn svg_use_clones_in_flex_keep_position_across_relayout() {
    // BUG-261 regression (same root cause as BUG-262): a flex container lays out its
    // SVG child more than once (intrinsic-size pass + final pass). BUG-244 stored the
    // document-space paint matrix back into `BoxKind::SvgShape.svg_transform`; on the
    // second pass that matrix was misread as the element's own transform, drifting
    // `<use>` clones out of their SVG clip rect (TEST-82 rendered only a subset of the
    // clones — `<g>`/`<symbol>`/nested rows vanished entirely). The fix routes the
    // paint matrix through the separate `svg_paint_matrix` output field, leaving
    // `svg_transform` an immutable layout input. Both clones must land at their exact
    // document positions regardless of how many times layout runs.
    let html = r##"
        <div style="display:flex">
          <svg width="200" height="120">
            <defs><rect id="r1" x="0" y="0" width="50" height="35" fill="#f00"/></defs>
            <use href="#r1" x="20" y="20"/>
            <use href="#r1" x="100" y="60"/>
          </svg>
        </div>"##;
    let tree = do_layout(html);

    let mut shapes = Vec::new();
    collect_boxes(
        &tree,
        &|b| matches!(b.kind, BoxKind::SvgShape { shape: SvgShapeKind::Rect { .. }, .. }),
        &mut shapes,
    );
    // Both `<use>` clones expand to a rect shape; the original in `<defs>` is not laid
    // out (it is only painted through instantiation), so exactly two shapes appear.
    assert_eq!(shapes.len(), 2, "two <use> clones laid out");

    // Body margin is reset to 0, so the SVG root sits at the page origin and the clone
    // rects map directly to their user-space (x, y) offsets: 50×35 at (20,20)/(100,60).
    let mut rects: Vec<(f32, f32)> = shapes.iter().map(|s| (s.rect.x, s.rect.y)).collect();
    rects.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    assert!(
        (rects[0].0 - 20.0).abs() < 0.5 && (rects[0].1 - 20.0).abs() < 0.5,
        "first clone at (20,20), got {:?}", rects[0],
    );
    assert!(
        (rects[1].0 - 100.0).abs() < 0.5 && (rects[1].1 - 60.0).abs() < 0.5,
        "second clone at (100,60), got {:?}", rects[1],
    );
    for s in &shapes {
        assert!(
            (s.rect.width - 50.0).abs() < 0.5 && (s.rect.height - 35.0).abs() < 0.5,
            "clone keeps 50×35 size, got {}×{}", s.rect.width, s.rect.height,
        );
    }
}
