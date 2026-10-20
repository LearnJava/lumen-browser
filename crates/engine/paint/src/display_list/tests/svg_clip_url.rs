//! SVG-CLIPPATH: `clip-path: url(#id)` → слои клипа дисплей-листа
//! (`display_list/clip_url.rs`).

use super::*;
use super::clip_url::clip_path_commands;
use super::text_and_images::build;
use lumen_layout::{SvgClipPath, SvgClipPlacement, SvgClipSubpath};
use std::sync::Arc;

fn square(x: f32, y: f32, s: f32) -> SvgClipSubpath {
    SvgClipSubpath { pts: vec![(x, y), (x + s, y), (x + s, y + s), (x, y + s)], even_odd: false }
}

fn def(subpaths: Vec<SvgClipSubpath>) -> SvgClipPath {
    SvgClipPath { object_bbox_units: false, subpaths, nested: None, placement: None }
}

fn polygon(cmd: &DisplayCommand) -> (&[(f32, f32)], bool) {
    match cmd {
        DisplayCommand::PushClipPath { shape: ResolvedClipShape::Polygon { verts, even_odd } } => {
            (verts.as_slice(), *even_odd)
        }
        other => panic!("ожидался PushClipPath{{Polygon}}, получен {other:?}"),
    }
}

#[test]
fn unresolved_url_clips_nothing() {
    let cmds = clip_path_commands(&ClipPath::Url("missing".into()), Rect::new(0.0, 0.0, 100.0, 100.0));
    assert!(cmds.is_empty());
}

#[test]
fn svg_clip_is_polygon_in_page_coordinates() {
    let clip = ClipPath::Svg(Arc::new(def(vec![square(50.0, 50.0, 100.0)])));
    let cmds = clip_path_commands(&clip, Rect::new(10.0, 20.0, 200.0, 200.0));
    assert_eq!(cmds.len(), 1);
    let (verts, even_odd) = polygon(&cmds[0]);
    assert!(!even_odd);
    assert_eq!(verts[0], (60.0, 70.0));
    assert_eq!(verts[2], (160.0, 170.0));
}

#[test]
fn single_even_odd_contour_keeps_rule() {
    let mut s = square(0.0, 0.0, 10.0);
    s.even_odd = true;
    let clip = ClipPath::Svg(Arc::new(def(vec![s])));
    let cmds = clip_path_commands(&clip, Rect::new(0.0, 0.0, 10.0, 10.0));
    assert!(polygon(&cmds[0]).1);
}

#[test]
fn empty_clip_path_clips_everything() {
    let clip = ClipPath::Svg(Arc::new(def(Vec::new())));
    let cmds = clip_path_commands(&clip, Rect::new(5.0, 6.0, 100.0, 100.0));
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        DisplayCommand::PushClipRect { rect } => assert_eq!((rect.width, rect.height), (0.0, 0.0)),
        other => panic!("ожидался нулевой PushClipRect, получен {other:?}"),
    }
}

#[test]
fn nested_clip_adds_second_layer() {
    let mut outer = def(vec![square(0.0, 0.0, 100.0)]);
    outer.nested = Some(Box::new(ClipPath::Svg(Arc::new(def(vec![square(20.0, 20.0, 30.0)])))));
    let cmds = clip_path_commands(&ClipPath::Svg(Arc::new(outer)), Rect::new(0.0, 0.0, 100.0, 100.0));
    assert_eq!(cmds.len(), 2);
    assert_eq!(polygon(&cmds[1]).0[0], (20.0, 20.0));
}

#[test]
fn several_shapes_merge_into_one_union_contour() {
    let clip = ClipPath::Svg(Arc::new(def(vec![square(0.0, 0.0, 10.0), square(50.0, 0.0, 10.0)])));
    let cmds = clip_path_commands(&clip, Rect::new(0.0, 0.0, 100.0, 100.0));
    assert_eq!(cmds.len(), 1);
    let (verts, even_odd) = polygon(&cmds[0]);
    assert!(!even_odd);
    // Квадрат + возврат к его началу, возврат к якорю + квадрат + возврат: 5 + 6.
    assert_eq!(verts.len(), 11);
    assert_eq!(verts[4], verts[0]);
    assert_eq!(verts[5], verts[0]);
}

#[test]
fn clockwise_and_counter_clockwise_shapes_get_one_orientation() {
    let mut cw = square(0.0, 0.0, 10.0);
    let mut ccw = square(50.0, 0.0, 10.0);
    ccw.pts.reverse();
    cw.even_odd = false;
    let clip = ClipPath::Svg(Arc::new(def(vec![cw, ccw])));
    let cmds = clip_path_commands(&clip, Rect::new(0.0, 0.0, 100.0, 100.0));
    let (verts, _) = polygon(&cmds[0]);
    // Площадь второго контура (вершины 5..9) имеет тот же знак, что и первого.
    let area = |r: &[(f32, f32)]| -> f32 {
        (0..r.len()).map(|i| r[i].0 * r[(i + 1) % r.len()].1 - r[(i + 1) % r.len()].0 * r[i].1).sum()
    };
    assert!(area(&verts[0..4]) * area(&verts[6..10]) > 0.0);
}

#[test]
fn object_bounding_box_units_scale_to_element_box() {
    let mut d = def(vec![square(0.25, 0.25, 0.5)]);
    d.object_bbox_units = true;
    let cmds = clip_path_commands(&ClipPath::Svg(Arc::new(d)), Rect::new(100.0, 0.0, 200.0, 80.0));
    let (verts, _) = polygon(&cmds[0]);
    assert_eq!(verts[0], (150.0, 20.0));
    assert_eq!(verts[2], (250.0, 60.0));
}

#[test]
fn svg_placement_matrix_is_applied() {
    let mut d = def(vec![square(0.0, 0.0, 10.0)]);
    d.placement = Some(SvgClipPlacement { matrix: [2.0, 0.0, 0.0, 2.0, 5.0, 5.0], user_bbox: (0.0, 0.0, 10.0, 10.0) });
    let cmds = clip_path_commands(&ClipPath::Svg(Arc::new(d)), Rect::new(100.0, 100.0, 20.0, 20.0));
    let (verts, _) = polygon(&cmds[0]);
    assert_eq!(verts[0], (105.0, 105.0));
    assert_eq!(verts[2], (125.0, 125.0));
}

#[test]
fn html_box_with_url_clip_gets_push_pop_pair() {
    let list = build(
        r##"<svg width=0 height=0><clipPath id=c><rect x=0 y=0 width=1 height=1 /></clipPath></svg>
        <div style="width:100px;height:100px;background:green;clip-path:url(#c)"></div>"##,
        "",
    );
    let pushes = list.iter().filter(|c| matches!(c, DisplayCommand::PushClipPath { .. })).count();
    let pops = list.iter().filter(|c| matches!(c, DisplayCommand::PopClip)).count();
    assert_eq!(pushes, 1);
    assert!(pops >= pushes);
}

#[test]
fn svg_shape_with_url_clip_is_wrapped_in_clip() {
    let list = build(
        r##"<svg width=200 height=200><clipPath id=c><rect x=50 y=50 width=100 height=100 /></clipPath>
        <rect width=200 height=200 fill=green clip-path="url(#c)" /></svg>"##,
        "",
    );
    let push = list.iter().position(|c| matches!(c, DisplayCommand::PushClipPath { .. })).expect("нет PushClipPath");
    let pop = list.iter().rposition(|c| matches!(c, DisplayCommand::PopClip)).expect("нет PopClip");
    assert!(push < pop);
}
