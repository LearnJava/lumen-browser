//! SVG layout tests: viewBox, basic shapes (rect/circle/ellipse/line/path).
//! Phase 2 — verifies that SVG root and shape boxes are positioned correctly
//! in document coordinates after the viewBox-to-CSS-px transform.

use lumen_core::geom::Size;
use lumen_layout::{layout, BoxKind, SvgShapeKind};
use lumen_html_parser::parse as parse_html;
use lumen_css_parser::parse as parse_css;

fn do_layout(html: &str) -> lumen_layout::LayoutBox {
    let doc = parse_html(html);
    let sheet = parse_css("");
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
    assert!(matches!(shape.kind, BoxKind::SvgShape { shape: SvgShapeKind::Rect { .. } }));
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
    assert!(matches!(shape.kind, BoxKind::SvgShape { shape: SvgShapeKind::Circle { .. } }));
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
    assert!(matches!(shape.kind, BoxKind::SvgShape { shape: SvgShapeKind::Ellipse { .. } }));
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
    assert!(matches!(shape.kind, BoxKind::SvgShape { shape: SvgShapeKind::Line { .. } }));
}

// ── path ──────────────────────────────────────────────────────────────────────

#[test]
fn svg_path_zero_rect() {
    // Path bbox is deferred to paint → rect = ZERO for Phase 2.
    let tree = do_layout(r#"<svg width="100" height="100"><path d="M10,10 L90,90"/></svg>"#);
    let shape = first_svg_shape(&tree).expect("SvgShape not found");
    assert_eq!(shape.rect.x, 0.0);
    assert_eq!(shape.rect.width, 0.0);
    assert!(matches!(shape.kind, BoxKind::SvgShape { shape: SvgShapeKind::Path { .. } }));
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
