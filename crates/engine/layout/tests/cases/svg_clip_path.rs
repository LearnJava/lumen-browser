//! SVG-CLIPPATH: `clip-path: url(#id)` / `clip-path="url(#id)"` разворачивается
//! в полигоны `<clipPath>` (CSS Masking L1 §3.1, SVG 1.1 §14.3).

use lumen_core::geom::Size;
use lumen_css_parser::parse as parse_css;
use lumen_html_parser::parse as parse_html;
use lumen_layout::{layout, ClipPath, LayoutBox, SvgClipPath};

fn do_layout(html: &str) -> LayoutBox {
    let doc = parse_html(html);
    let sheet = parse_css("body{margin:0}");
    layout(&doc, &sheet, Size::new(800.0, 600.0))
}

fn find<'a>(b: &'a LayoutBox, pred: &dyn Fn(&LayoutBox) -> bool) -> Option<&'a LayoutBox> {
    if pred(b) {
        return Some(b);
    }
    b.children.iter().find_map(|c| find(c, pred))
}

fn svg_clip(b: &LayoutBox) -> Option<&SvgClipPath> {
    match &b.style.clip_path {
        Some(ClipPath::Svg(def)) => Some(def),
        _ => None,
    }
}

fn with_clip(b: &LayoutBox) -> Option<&LayoutBox> {
    find(b, &|b| b.style.clip_path.is_some())
}

fn bbox(pts: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    let x0 = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min);
    let y0 = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min);
    let x1 = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max);
    let y1 = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max);
    (x0, y0, x1, y1)
}

#[test]
fn attribute_url_resolves_to_rect_polygon() {
    let tree = do_layout(
        r##"<svg width=200 height=200><clipPath id=c><rect x=50 y=50 width=100 height=100 /></clipPath>
        <rect width=200 height=200 fill=green clip-path="url(#c)"/></svg>"##,
    );
    let b = with_clip(&tree).expect("clip-path не дошёл до бокса");
    let def = svg_clip(b).expect("url(#c) не разрешён в ClipPath::Svg");
    assert_eq!(def.subpaths.len(), 1);
    assert_eq!(bbox(&def.subpaths[0].pts), (50.0, 50.0, 150.0, 150.0));
    assert!(!def.subpaths[0].even_odd);
    assert!(!def.object_bbox_units);
    let placement = def.placement.expect("у SVG-элемента есть положение");
    assert_eq!((placement.matrix[4] + b.rect.x, placement.matrix[5] + b.rect.y), (0.0, 0.0));
}

#[test]
fn style_property_url_resolves_too() {
    let tree = do_layout(
        r##"<svg width=200 height=200><clipPath id=c><circle cx=100 cy=100 r=40 /></clipPath>
        <rect width=200 height=200 style="clip-path:url(#c)"/></svg>"##,
    );
    let def = svg_clip(with_clip(&tree).unwrap()).expect("resolved");
    let (x0, y0, x1, y1) = bbox(&def.subpaths[0].pts);
    assert!((x0 - 60.0).abs() < 0.5 && (x1 - 140.0).abs() < 0.5, "{x0} {x1}");
    assert!((y0 - 60.0).abs() < 0.5 && (y1 - 140.0).abs() < 0.5, "{y0} {y1}");
}

#[test]
fn clip_rule_evenodd_and_path_holes_survive() {
    let tree = do_layout(
        r##"<svg width=200 height=200><clipPath id=c><path clip-rule=evenodd d="M0 0H200V200H0Z M50 50H150V150H50Z"/></clipPath>
        <rect width=200 height=200 clip-path="url(#c)"/></svg>"##,
    );
    let def = svg_clip(with_clip(&tree).unwrap()).unwrap();
    assert_eq!(def.subpaths.len(), 1, "подпути одного <path> — одна фигура");
    assert!(def.subpaths[0].even_odd);
    assert!(def.subpaths[0].pts.len() >= 8);
}

#[test]
fn clip_path_element_itself_is_not_painted() {
    let tree = do_layout(
        r##"<svg width=200 height=200><clipPath id=c><rect x=50 y=50 width=100 height=100 /></clipPath>
        <rect width=200 height=200 clip-path="url(#c)"/></svg>"##,
    );
    let mut shapes = 0;
    fn count(b: &LayoutBox, n: &mut usize) {
        if matches!(b.kind, lumen_layout::BoxKind::SvgShape { .. }) {
            *n += 1;
        }
        b.children.iter().for_each(|c| count(c, n));
    }
    count(&tree, &mut shapes);
    assert_eq!(shapes, 1, "дети <clipPath> не должны рисоваться");
}

#[test]
fn missing_or_foreign_target_leaves_clip_unset() {
    let tree = do_layout(
        r##"<svg width=200 height=200><rect id=r width=10 height=10 />
        <rect width=200 height=200 clip-path="url(#nope)"/>
        <rect width=200 height=200 clip-path="url(#r)"/></svg>"##,
    );
    assert!(with_clip(&tree).is_none());
}

#[test]
fn html_element_gets_object_bbox_clip() {
    let tree = do_layout(
        r##"<svg width=0 height=0><clipPath id=c clipPathUnits=objectBoundingBox><circle cx=.5 cy=.5 r=.5 /></clipPath></svg>
        <div style="width:200px;height:200px;background:green;clip-path:url(#c)"></div>"##,
    );
    let div = find(&tree, &|b| b.style.clip_path.is_some() && matches!(b.kind, lumen_layout::BoxKind::Block))
        .expect("div с clip-path");
    let def = svg_clip(div).expect("resolved");
    assert!(def.object_bbox_units);
    assert!(def.placement.is_none());
    let (x0, _, x1, _) = bbox(&def.subpaths[0].pts);
    assert!((x0 - 0.0).abs() < 0.01 && (x1 - 1.0).abs() < 0.01);
}

#[test]
fn nested_clip_path_on_clip_path_element() {
    let tree = do_layout(
        r##"<svg width=200 height=200><clipPath id=inner><rect width=100 height=200 /></clipPath>
        <clipPath id=outer clip-path="url(#inner)"><rect width=200 height=100 /></clipPath>
        <rect width=200 height=200 clip-path="url(#outer)"/></svg>"##,
    );
    let def = svg_clip(with_clip(&tree).unwrap()).unwrap();
    let nested = def.nested.as_ref().expect("вложенный clip-path");
    assert!(matches!(nested.as_ref(), ClipPath::Svg(_)));
}

#[test]
fn recursive_reference_clips_everything() {
    let tree = do_layout(
        r##"<svg width=200 height=200><clipPath id=c clip-path="url(#c)"><rect width=100 height=100 /></clipPath>
        <rect width=200 height=200 clip-path="url(#c)"/></svg>"##,
    );
    let def = svg_clip(with_clip(&tree).unwrap()).unwrap();
    match def.nested.as_deref() {
        Some(ClipPath::Svg(inner)) => assert!(inner.subpaths.is_empty()),
        other => panic!("ожидался пустой вложенный клип, получено {other:?}"),
    }
}
